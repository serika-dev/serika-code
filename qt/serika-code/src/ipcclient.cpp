#include "ipcclient.h"

#include <QBuffer>
#include <QClipboard>
#include <QCoreApplication>
#include <QDateTime>
#include <QDir>
#include <QFile>
#include <QFileInfo>
#include <QFont>
#include <QGuiApplication>
#include <QImage>
#include <QJsonArray>
#include <QJsonDocument>
#include <QDesktopServices>
#include <QMimeData>
#include <QPainter>
#include <QStandardPaths>
#include <QTextStream>
#include <QUuid>

namespace {
QString s(const char *text) { return QString::fromUtf8(text); }

QString toolDetail(const QString &arguments) {
    const QJsonObject obj = QJsonDocument::fromJson(arguments.toUtf8()).object();
    for (const char *key : {"path", "file_path", "command", "query", "pattern", "url", "from", "text", "key"}) {
        const QString value = obj.value(QString::fromLatin1(key)).toString();
        if (!value.isEmpty())
            return value.length() > 140 ? value.left(139) + QStringLiteral("…") : value;
    }
    if (obj.contains(QStringLiteral("id")))
        return QStringLiteral("element %1").arg(obj.value(QStringLiteral("id")).toInt());
    return {};
}

bool isEditTool(const QString &name) {
    static const QStringList names = {QStringLiteral("write_file"), QStringLiteral("replace_in_file"), QStringLiteral("apply_patch"),
                                      QStringLiteral("delete_file"), QStringLiteral("move_file"), QStringLiteral("Edit"),
                                      QStringLiteral("Write"), QStringLiteral("MultiEdit"), QStringLiteral("NotebookEdit")};
    return names.contains(name);
}

QString toStringValue(const QVariant &value) {
    if (value.typeId() == QMetaType::Bool)
        return value.toBool() ? QStringLiteral("true") : QStringLiteral("false");
    return value.toString();
}

QString dataUrl(const QString &base64) { return QStringLiteral("data:image/jpeg;base64,") + base64; }
} // namespace

IpcClient::IpcClient(bool demo, QObject *parent) : QObject(parent), m_demo(demo) {
    connect(&m_process, &QProcess::readyReadStandardOutput, this, &IpcClient::onReadyRead);
    connect(&m_process, &QProcess::errorOccurred, this, [this](QProcess::ProcessError) {
        m_bridgeError = m_process.errorString();
        emit bridgeErrorChanged();
    });
    connect(&m_process, &QProcess::finished, this, [this](int, QProcess::ExitStatus) {
        if (m_bridgeError.isEmpty() && !m_demo) {
            m_bridgeError = QStringLiteral("The Serika agent stopped unexpectedly.");
            emit bridgeErrorChanged();
        }
    });
    m_queueTimer.setSingleShot(true);
    connect(&m_queueTimer, &QTimer::timeout, this, &IpcClient::resumeQueue);
    startProcess();
}

IpcClient::~IpcClient() {
    if (m_process.state() != QProcess::NotRunning) {
        m_process.closeWriteChannel();
        if (!m_process.waitForFinished(1500)) {
            m_process.kill();
            m_process.waitForFinished(1000);
        }
    }
}

void IpcClient::startProcess() {
    const QString program = findSerika();
    if (program.isEmpty()) {
        m_bridgeError = QStringLiteral("Could not find the serika command line. Build it with cargo first.");
        emit bridgeErrorChanged();
        return;
    }
    m_process.setProgram(program);
    m_process.setArguments({QStringLiteral("ipc")});
    m_process.setWorkingDirectory(QDir::currentPath());
    m_process.setProcessChannelMode(QProcess::SeparateChannels);
    m_process.start();
    if (!m_process.waitForStarted(5000)) {
        m_bridgeError = QStringLiteral("serika ipc did not start");
        emit bridgeErrorChanged();
        return;
    }
    request(QStringLiteral("settings.get"), {}, QStringLiteral("settings"));
    request(QStringLiteral("ui.get"), {}, QStringLiteral("ui"));
    request(QStringLiteral("auth.background"), {}, QStringLiteral("background"));
    if (m_demo) {
        loadDemo();
        return;
    }
    refresh();
}

QString IpcClient::findSerika() const {
    const QString env = qEnvironmentVariable("SERIKA_BIN");
    if (!env.isEmpty() && QFileInfo::exists(env))
        return env;
#ifdef Q_OS_WIN
    const QString exe = QStringLiteral("serika.exe");
#else
    const QString exe = QStringLiteral("serika");
#endif
    const QString beside = QDir(QCoreApplication::applicationDirPath()).filePath(exe);
    if (QFileInfo::exists(beside))
        return beside;
    // Running from a source checkout: use whichever cargo build is newest.
    QDir dir(QCoreApplication::applicationDirPath());
    for (int i = 0; i < 7; ++i) {
        const QFileInfo debug(dir.filePath(QStringLiteral("target/debug/") + exe));
        const QFileInfo release(dir.filePath(QStringLiteral("target/release/") + exe));
        if (debug.exists() && release.exists())
            return debug.lastModified() > release.lastModified() ? debug.filePath() : release.filePath();
        if (release.exists())
            return release.filePath();
        if (debug.exists())
            return debug.filePath();
        if (!dir.cdUp())
            break;
    }
    return QStandardPaths::findExecutable(QStringLiteral("serika"));
}

void IpcClient::request(const QString &method, const QJsonObject &params, const QString &tag) {
    if (m_process.state() != QProcess::Running)
        return;
    const int id = m_nextId++;
    m_tags.insert(id, tag);
    QJsonObject payload{{QStringLiteral("id"), id}, {QStringLiteral("method"), method}};
    if (!params.isEmpty())
        payload.insert(QStringLiteral("params"), params);
    m_process.write(QJsonDocument(payload).toJson(QJsonDocument::Compact) + '\n');
}

void IpcClient::onReadyRead() {
    m_buffer += m_process.readAllStandardOutput();
    while (true) {
        const int nl = m_buffer.indexOf('\n');
        if (nl < 0)
            break;
        const QByteArray line = m_buffer.left(nl).trimmed();
        m_buffer.remove(0, nl + 1);
        if (!line.isEmpty())
            handleLine(line);
    }
}

void IpcClient::handleLine(const QByteArray &line) {
    const QJsonDocument doc = QJsonDocument::fromJson(line);
    if (!doc.isObject())
        return;
    const QJsonObject obj = doc.object();
    if (obj.contains(QStringLiteral("event"))) {
        handleEvent(obj);
        return;
    }
    const int id = obj.value(QStringLiteral("id")).toInt();
    QString tag = m_tags.take(id);
    if (tag.startsWith(QLatin1String("browserPoll#"))) {
        // A poll that left before the latest tab action describes the old state.
        if (tag.mid(12).toULongLong() != m_browserEpoch || m_browserInFlight)
            return;
        tag = QStringLiteral("browserPoll");
    } else if (tag == QLatin1String("browserAction")) {
        m_browserInFlight = false;
        QTimer::singleShot(0, this, &IpcClient::browserNext);
        tag = QStringLiteral("browser");
        // A newer action is waiting; its answer will describe the browser.
        if (!m_browserQueue.isEmpty() && obj.value(QStringLiteral("ok")).toBool())
            return;
    } else if (tag == QLatin1String("uiSet")) {
        // Local preferences already hold the change; a late answer could undo a newer one.
        if (!obj.value(QStringLiteral("ok")).toBool())
            setNotice(obj.value(QStringLiteral("error")).toString());
        return;
    }
    if (!obj.value(QStringLiteral("ok")).toBool()) {
        const QString error = obj.value(QStringLiteral("error")).toString();
        if (tag.startsWith(QLatin1String("auth.")))
            emit authFinished(false, error, false);
        else if (tag == QLatin1String("chat")) {
            setRunning(false);
            setStatus(error);
            if (!m_viewRun.isEmpty()) {
                m_runs.remove(m_viewRun);
                m_viewRun.clear();
                emit liveChatsChanged();
            }
        } else if (tag == QLatin1String("gitAction")) {
            m_gitBusy.clear();
            setGit(m_git);
            emit gitMessage(error, true);
            gitRefresh();
        } else if (tag == QLatin1String("gitAccounts")) {
            emit gitMessage(error, true);
        } else if (tag == QLatin1String("gitDiff") || tag == QLatin1String("gitConflict")) {
            emit gitMessage(error, true);
        } else if (tag == QLatin1String("browser")) {
            m_browserBusy = false;
            m_browserError = error;
            emit browserChanged();
        } else if (tag == QLatin1String("providers")) {
            m_providersLoading = false;
            emit providersChanged();
            setNotice(error);
        } else if (tag != QLatin1String("background") && tag != QLatin1String("account") && tag != QLatin1String("models") &&
                   tag != QLatin1String("browserPoll") && tag != QLatin1String("browserMouse") &&
                   tag != QLatin1String("browserStream") && tag != QLatin1String("git") &&
                   tag != QLatin1String("sessionSettings")) {
            setNotice(error);
        }
        return;
    }
    if (tag == QLatin1String("providers")) {
        if (!m_demo)
            m_providers = obj.value(QStringLiteral("result")).toArray().toVariantList();
        m_providersLoading = false;
        emit providersChanged();
        emit modelsChanged();
        return;
    }
    if (tag == QLatin1String("probe")) {
        refreshProviders();
        return;
    }
    handleResponse(tag, obj.value(QStringLiteral("result")).toObject());
}

void IpcClient::handleResponse(const QString &tag, const QJsonObject &result) {
    const bool authTag = tag == QLatin1String("auth.login") || tag == QLatin1String("auth.register") ||
                         tag == QLatin1String("auth.magicConsume") || tag == QLatin1String("auth.logout");
    if (tag == QLatin1String("account") || authTag) {
        if (result.value(QStringLiteral("needsSecondFactor")).toBool()) {
            emit authFinished(false, result.value(QStringLiteral("message")).toString(), true);
            return;
        }
        if (m_demo && tag == QLatin1String("account"))
            return;
        applyAccount(result);
        if (authTag)
            emit authFinished(m_signedIn, QString(), false);
        if (m_signedIn && authTag)
            refresh();
    } else if (tag == QLatin1String("auth.magic")) {
        emit authFinished(true, result.value(QStringLiteral("message")).toString(), false);
    } else if (tag == QLatin1String("models")) {
        if (!m_demo)
            applyModels(result);
    } else if (tag == QLatin1String("settings")) {
        applySettings(result);
    } else if (tag == QLatin1String("ui")) {
        if (m_demo && qEnvironmentVariableIsSet("SERIKA_DEMO_UI"))
            return;
        m_ui = result.toVariantMap();
        emit uiChanged();
        if (!m_draftsLoaded) {
            m_draftsLoaded = true;
            const QJsonDocument doc = QJsonDocument::fromJson(m_ui.value(QStringLiteral("drafts")).toString().toUtf8());
            if (doc.isObject())
                m_drafts = doc.object().toVariantMap();
            emit draftsChanged();
        }
    } else if (tag == QLatin1String("i18n")) {
        applyDictionary(result);
    } else if (tag == QLatin1String("projects")) {
        if (!m_demo) {
            m_projects = result.value(QStringLiteral("projects")).toArray().toVariantList();
            emit projectsChanged();
            // Removing the folder you were in moved you to another one.
            if (result.contains(QStringLiteral("settings"))) {
                applySettings(result.value(QStringLiteral("settings")).toObject());
                request(QStringLiteral("sessions.list"), {}, QStringLiteral("sessions"));
            }
        }
    } else if (tag == QLatin1String("sessions")) {
        if (!m_demo) {
            m_sessions = result.value(QStringLiteral("sessions")).toArray().toVariantList();
            emit sessionsChanged();
        }
    } else if (tag == QLatin1String("sessionSettings")) {
        applySettings(result.value(QStringLiteral("settings")).toObject());
        m_folderMissing = result.value(QStringLiteral("folderMissing")).toBool();
        request(QStringLiteral("projects.list"), {}, QStringLiteral("projects"));
        if (!m_running && !m_queue.isEmpty())
            QTimer::singleShot(200, this, &IpcClient::dispatchQueue);
    } else if (tag == QLatin1String("git")) {
        // An answer for a project you have since left is not this project's state.
        if (result.value(QStringLiteral("project")).toString().isEmpty() || result.value(QStringLiteral("project")).toString() == m_projectPath)
            setGit(result.toVariantMap());
    } else if (tag == QLatin1String("gitAction")) {
        const QString action = m_gitBusy.value(QStringLiteral("action")).toString();
        m_gitBusy.clear();
        setGit(result.value(QStringLiteral("overview")).toObject().toVariantMap());
        if (result.contains(QStringLiteral("accounts"))) {
            m_gitAccounts = result.value(QStringLiteral("accounts")).toObject().toVariantMap();
            emit gitAccountsChanged();
        }
        const QString message = result.value(QStringLiteral("message")).toString();
        emit gitMessage(message.isEmpty() ? QStringLiteral("Done.") : message, false);
        if (action == QLatin1String("switch") || action == QLatin1String("branch.create") || action == QLatin1String("fetch"))
            gitBranchesRefresh();
    } else if (tag.startsWith(QLatin1String("projectDetails:"))) {
        if (result.contains(QStringLiteral("accounts"))) {
            m_gitAccounts = result.value(QStringLiteral("accounts")).toObject().toVariantMap();
            emit gitAccountsChanged();
        }
        emit projectDetailsReady(tag.mid(15), result.toVariantMap());
    } else if (tag.startsWith(QLatin1String("projectInstructions:"))) {
        const QString path = result.value(QStringLiteral("path")).toString();
        QDesktopServices::openUrl(QUrl::fromLocalFile(path));
        projectDetails(tag.mid(20));
    } else if (tag.startsWith(QLatin1String("gitAccountIn:"))) {
        const QString project = tag.mid(13);
        m_gitAccounts = result.value(QStringLiteral("accounts")).toObject().toVariantMap();
        emit gitAccountsChanged();
        if (project == m_projectPath)
            setGit(result.value(QStringLiteral("overview")).toObject().toVariantMap());
        projectDetails(project);
    } else if (tag.startsWith(QLatin1String("gitRemoteAuth:"))) {
        const QString project = tag.mid(14);
        if (result.contains(QStringLiteral("accounts"))) {
            m_gitAccounts = result.value(QStringLiteral("accounts")).toObject().toVariantMap();
            emit gitAccountsChanged();
        }
        emit gitRemoteAuthReady(project, result.toVariantMap());
        projectDetails(project);
        if (project == m_projectPath)
            gitRefresh();
    } else if (tag == QLatin1String("gitBranches")) {
        m_gitBranches = result.value(QStringLiteral("branches")).toArray().toVariantList();
        emit gitBranchesChanged();
    } else if (tag == QLatin1String("gitAccounts")) {
        m_gitAccounts = result.toVariantMap();
        emit gitAccountsChanged();
    } else if (tag == QLatin1String("gitConflict")) {
        emit gitConflictReady(result.value(QStringLiteral("path")).toString(), result.toVariantMap());
    } else if (tag == QLatin1String("gitDiff")) {
        const QString key = result.contains(QStringLiteral("hash")) ? result.value(QStringLiteral("hash")).toString()
                          : result.contains(QStringLiteral("index")) ? QStringLiteral("stash:%1").arg(result.value(QStringLiteral("index")).toInt())
                          : QStringLiteral("file:") + result.value(QStringLiteral("path")).toString();
        QVariantList files;
        if (result.contains(QStringLiteral("files")))
            files = result.value(QStringLiteral("files")).toArray().toVariantList();
        else
            files.append(QVariantMap{{QStringLiteral("path"), result.value(QStringLiteral("path")).toString()},
                                     {QStringLiteral("diff"), result.value(QStringLiteral("diff")).toString()}});
        emit gitDiffReady(key, files);
    } else if (tag == QLatin1String("session")) {
        // Another chat is loading: one still running on screen carries on in the background.
        const QString incoming = result.value(QStringLiteral("id")).toString();
        if (!m_viewRun.isEmpty()) {
            if (incoming == m_activeSession && m_running)
                return;
            detachView();
        }
        applySettings(result.value(QStringLiteral("settings")).toObject());
        m_view->replaceAll(result.value(QStringLiteral("turns")).toArray().toVariantList());
        m_diffs = result.value(QStringLiteral("diffs")).toArray().toVariantList();
        m_activeSession = result.value(QStringLiteral("id")).toString();
        m_folderMissing = result.value(QStringLiteral("folderMissing")).toBool();
        emit activeSessionChanged();
        emit diffsChanged();
        emit turnsReloaded();
        emit composerFill(m_drafts.value(m_activeSession).toMap().value(QStringLiteral("text")).toString());
        request(QStringLiteral("projects.list"), {}, QStringLiteral("projects"));
        request(QStringLiteral("sessions.list"), {}, QStringLiteral("sessions"));
        m_agents.clear();
        emit agentsChanged();
        request(QStringLiteral("sessions.children"), {{QStringLiteral("id"), m_activeSession}}, QStringLiteral("agents"));
    } else if (tag == QLatin1String("terminalOpen")) {
        QVariantMap row = result.toVariantMap();
        row.insert(QStringLiteral("running"), false);
        row.insert(QStringLiteral("exited"), false);
        m_terminals.append(row);
        emit terminalsChanged();
    } else if (tag == QLatin1String("mcp")) {
        m_mcp = result.toVariantMap();
        emit mcpChanged();
    } else if (tag == QLatin1String("skills")) {
        m_skills = result.value(QStringLiteral("skills")).toArray().toVariantList();
        emit skillsChanged();
    } else if (tag == QLatin1String("browserCopy")) {
        const QString text = result.value(QStringLiteral("text")).toString();
        if (!text.isEmpty())
            copyText(text);
    } else if (tag == QLatin1String("agents")) {
        m_agents = result.value(QStringLiteral("agents")).toArray().toVariantList();
        emit agentsChanged();
    } else if (tag == QLatin1String("turnAction")) {
        const QString action = m_turnAction;
        m_turnAction.clear();
        handleResponse(QStringLiteral("session"), result.value(QStringLiteral("session")).toObject());
        const QString prompt = result.value(QStringLiteral("prompt")).toString();
        const QVariantList files = result.value(QStringLiteral("attachments")).toArray().toVariantList();
        if (action == QLatin1String("edit")) {
            m_attachments = files;
            emit attachmentsChanged();
            emit composerFill(prompt);
        } else if (action == QLatin1String("retry")) {
            startSend(prompt, files);
        } else {
            const int restored = result.value(QStringLiteral("restored")).toInt();
            setNotice(restored > 0 ? QStringLiteral("Rewound. %1 file(s) restored.").arg(restored) : QStringLiteral("Rewound."));
        }
    } else if (tag == QLatin1String("background")) {
        m_artUrl = result.value(QStringLiteral("url")).toString();
        m_artArtist = result.value(QStringLiteral("artist")).toString();
        emit artChanged();
    } else if (tag == QLatin1String("browser")) {
        applyFrame(result);
    } else if (tag == QLatin1String("browserPoll")) {
        // A background refresh must not end a navigation that is still running.
        const bool busy = m_browserBusy;
        applyFrame(result);
        m_browserBusy = busy;
    } else if (tag == QLatin1String("attachments")) {
        const QVariantList added = result.value(QStringLiteral("attachments")).toArray().toVariantList();
        for (const QVariant &item : added)
            m_attachments.append(item);
        if (!added.isEmpty())
            emit attachmentsChanged();
        QStringList problems;
        for (const QJsonValue &error : result.value(QStringLiteral("errors")).toArray())
            problems << error.toString();
        if (!problems.isEmpty())
            setNotice(problems.join(QLatin1Char('\n')));
    } else if (tag == QLatin1String("update")) {
        m_updateInfo = result.toVariantMap();
        emit updateInfoChanged();
    } else if (tag == QLatin1String("mutated")) {
        request(QStringLiteral("sessions.list"), {}, QStringLiteral("sessions"));
        request(QStringLiteral("projects.list"), {}, QStringLiteral("projects"));
    }
}

void IpcClient::applyFrame(const QJsonObject &frame) {
    if (frame.contains(QStringLiteral("unchanged")))
        return;
    if (frame.value(QStringLiteral("running")).toBool(true) == false) {
        m_browserBusy = false;
        if (!m_browser.isEmpty() || !m_browserImage.isEmpty()) {
            m_browser.clear();
            m_browserImage.clear();
            emit browserChanged();
            emit browserImageChanged();
        }
        return;
    }
    QVariantMap map = frame.toVariantMap();
    // Remember when the agent last acted, so the panel can show that Serika is browsing.
    if (frame.value(QStringLiteral("agent")).toBool())
        map.insert(QStringLiteral("agentAt"), QDateTime::currentMSecsSinceEpoch());
    else if (m_browser.contains(QStringLiteral("agentAt")))
        map.insert(QStringLiteral("agentAt"), m_browser.value(QStringLiteral("agentAt")));
    const QString image = frame.value(QStringLiteral("image")).toString();
    if (!image.isEmpty())
        setBrowserFrame(image);
    // A frame without a picture (streaming, or a quick tab switch) keeps the last one.
    map.insert(QStringLiteral("imageUrl"), m_browserImage);
    map.insert(QStringLiteral("running"), true);
    map.remove(QStringLiteral("image"));
    m_browser = map;
    m_browserBusy = false;
    m_browserError.clear();
    emit browserChanged();
}

void IpcClient::setBrowserFrame(const QString &base64) {
    QImage image;
    if (!image.loadFromData(QByteArray::fromBase64(base64.toLatin1())))
        return; // a broken frame keeps the last good one on screen
    m_frames->set(image);
    // A new name each time, or the Image would think nothing changed.
    m_browserImage = QStringLiteral("image://browser/%1").arg(++m_frameSerial);
    emit browserImageChanged();
}

// Keep one row per changed file, and link the change to the edit tool that made it, so its
// card can open the diff.
static void mergeDiff(QVariantList &diffs, TurnModel *model, const QJsonObject &obj) {
    const QString path = obj.value(QStringLiteral("path")).toString();
    const QVariantMap item{{QStringLiteral("path"), path}, {QStringLiteral("diff"), obj.value(QStringLiteral("diff")).toString()}};
    bool replaced = false;
    for (QVariant &existing : diffs) {
        if (existing.toMap().value(QStringLiteral("path")).toString() == path) {
            existing = item;
            replaced = true;
            break;
        }
    }
    if (!replaced)
        diffs.append(item);
    int row = -1;
    if (Turn *turn = model->lastAssistant(&row)) {
        for (int i = int(turn->tools.size()) - 1; i >= 0; --i) {
            QVariantMap tool = turn->tools.at(i).toMap();
            if (isEditTool(tool.value(QStringLiteral("name")).toString()) && !tool.contains(QStringLiteral("diffPath"))) {
                tool.insert(QStringLiteral("diffPath"), path);
                turn->tools[i] = tool;
                model->changed(row, {TurnModel::ToolsRole});
                break;
            }
        }
    }
}

// One row per sub-agent, updated in place. True when this one is new.
static bool mergeAgent(QVariantList &agents, const QJsonObject &obj) {
    QVariantMap row = obj.toVariantMap();
    row.remove(QStringLiteral("event"));
    row.remove(QStringLiteral("runId"));
    const QString id = row.value(QStringLiteral("id")).toString();
    if (row.value(QStringLiteral("status")).toString() != QLatin1String("running"))
        row.insert(QStringLiteral("endedAt"), QDateTime::currentMSecsSinceEpoch());
    for (QVariant &item : agents) {
        QVariantMap map = item.toMap();
        if (map.value(QStringLiteral("id")).toString() == id) {
            row.insert(QStringLiteral("startedAt"), map.value(QStringLiteral("startedAt")));
            item = row;
            return false;
        }
    }
    row.insert(QStringLiteral("startedAt"), QDateTime::currentMSecsSinceEpoch());
    agents.append(row);
    return true;
}

// Text for a new round of the same reply: keep rounds apart with a blank line.
static void nextRound(TurnModel *model) {
    int row = -1;
    if (Turn *turn = model->lastAssistant(&row)) {
        if (!turn->raw.trimmed().isEmpty())
            turn->raw += QStringLiteral("\n\n");
        if (!turn->reasoning.trimmed().isEmpty())
            turn->reasoning += QStringLiteral("\n\n");
        turn->content = TurnModel::stripToolFences(turn->raw);
        model->changed(row, {TurnModel::ContentRole, TurnModel::ReasoningRole});
    }
}

static void appendStream(TurnModel *model, const QString &event, const QString &text) {
    int row = -1;
    Turn *turn = model->lastAssistant(&row);
    if (!turn)
        return;
    if (event == QLatin1String("chunk")) {
        const int before = int(turn->content.size());
        turn->raw += text;
        turn->content = TurnModel::stripToolFences(turn->raw);
        // A closed tool fence can make the text shorter: show that at once.
        if (turn->content.size() < before)
            turn->shown = -1;
        if (turn->content.size() > before)
            model->reveal(row, before);
        else
            model->changed(row, {TurnModel::ContentRole});
    } else {
        turn->reasoning += text;
        model->changed(row, {TurnModel::ReasoningRole});
    }
}

static void setToolOutput(TurnModel *model, const QJsonObject &obj) {
    int row = -1;
    Turn *turn = model->lastAssistant(&row);
    if (!turn)
        return;
    const QString id = obj.value(QStringLiteral("id")).toString();
    for (int i = 0; i < turn->tools.size(); ++i) {
        QVariantMap tool = turn->tools.at(i).toMap();
        if (tool.value(QStringLiteral("id")).toString() == id) {
            tool.insert(QStringLiteral("output"), obj.value(QStringLiteral("text")).toString());
            turn->tools[i] = tool;
            model->changed(row, {TurnModel::ToolsRole});
            return;
        }
    }
}

static void insertHandoff(TurnModel *model, const QJsonObject &obj) {
    Turn marker;
    marker.role = QStringLiteral("switch");
    marker.switchFrom = obj.value(QStringLiteral("from")).toString();
    marker.switchTo = obj.value(QStringLiteral("to")).toString();
    marker.content = QString::number(obj.value(QStringLiteral("messages")).toInt());
    int row = -1;
    Turn *reply = model->lastAssistant(&row);
    if (reply && reply->streaming)
        model->insert(row, marker);
    else
        model->append(marker);
}

void IpcClient::handleEvent(const QJsonObject &obj) {
    const QString event = obj.value(QStringLiteral("event")).toString();
    if (event == QLatin1String("runs")) {
        emit liveChatsChanged();
        return;
    }
    // Fresh lists that arrive after a quick answer from the saved ones.
    if (event == QLatin1String("models")) {
        if (!m_demo)
            applyModels(obj);
        return;
    }
    if (event == QLatin1String("gitAccounts")) {
        m_gitAccounts = obj.toVariantMap();
        m_gitAccounts.remove(QStringLiteral("event"));
        emit gitAccountsChanged();
        return;
    }
    // Events of a chat that is not on screen go to that chat's own state.
    static const QSet<QString> perChat{QStringLiteral("started"), QStringLiteral("status"), QStringLiteral("chunk"),
                                       QStringLiteral("reasoning"), QStringLiteral("tool"), QStringLiteral("toolOutput"),
                                       QStringLiteral("approval"), QStringLiteral("diff"), QStringLiteral("subagent"),
                                       QStringLiteral("handoff"), QStringLiteral("completed"), QStringLiteral("failed"),
                                       QStringLiteral("cancelled")};
    const QString runId = obj.value(QStringLiteral("runId")).toString();
    if (!runId.isEmpty() && runId != m_viewRun && perChat.contains(event)) {
        auto it = m_runs.find(runId);
        if (it != m_runs.end())
            handleBackgroundEvent(*it, event, obj);
        else if (event == QLatin1String("completed") || event == QLatin1String("failed") || event == QLatin1String("cancelled"))
            request(QStringLiteral("sessions.list"), {}, QStringLiteral("sessions"));
        return;
    }
    if (event == QLatin1String("started")) {
        m_activeSession = obj.value(QStringLiteral("sessionId")).toString();
        if (m_runs.contains(m_viewRun))
            m_runs[m_viewRun].sessionId = m_activeSession;
        emit activeSessionChanged();
        emit liveChatsChanged();
        request(QStringLiteral("sessions.list"), {}, QStringLiteral("sessions"));
        m_roundsSeen = 0;
        setRunning(true);
        beginAssistantTurn();
    } else if (event == QLatin1String("status")) {
        const QString text = obj.value(QStringLiteral("text")).toString();
        if (text.startsWith(QLatin1String("turn "))) {
            ++m_roundsSeen;
            if (m_roundsSeen > 1)
                nextRound(m_view);
        } else if (text.contains(QLatin1String("needed approval"))) {
            setNotice(text);
        }
    } else if (event == QLatin1String("chunk") || event == QLatin1String("reasoning")) {
        appendStream(m_view, event, obj.value(QStringLiteral("text")).toString());
    } else if (event == QLatin1String("tool")) {
        updateTool(obj, m_view);
    } else if (event == QLatin1String("toolOutput")) {
        setToolOutput(m_view, obj);
    } else if (event == QLatin1String("approval")) {
        m_approval = obj.toVariantMap();
        emit approvalChanged();
        emit liveChatsChanged();
    } else if (event == QLatin1String("diff")) {
        mergeDiff(m_diffs, m_view, obj);
        emit diffsChanged();
    } else if (event == QLatin1String("browser")) {
        applyFrame(obj);
        emit browserActivity();
    } else if (event == QLatin1String("browserImage")) {
        setBrowserFrame(obj.value(QStringLiteral("image")).toString());
    } else if (event == QLatin1String("terminal")) {
        const QString id = obj.value(QStringLiteral("id")).toString();
        const QString kind = obj.value(QStringLiteral("kind")).toString();
        if (kind == QLatin1String("output")) {
            const QString text = obj.value(QStringLiteral("text")).toString();
            QString &buffer = m_termText[id];
            buffer.append(text);
            if (buffer.size() > 400000)
                buffer.remove(0, buffer.size() - 300000);
            emit terminalOutput(id, text, obj.value(QStringLiteral("error")).toBool());
        } else if (kind == QLatin1String("done")) {
            updateTerminal(id, {{QStringLiteral("running"), false}, {QStringLiteral("cwd"), obj.value(QStringLiteral("cwd")).toString()},
                                {QStringLiteral("lastCode"), obj.value(QStringLiteral("code")).toInt()}});
            emit terminalDone(id, obj.value(QStringLiteral("code")).toInt());
        } else if (kind == QLatin1String("exit")) {
            updateTerminal(id, {{QStringLiteral("running"), false}, {QStringLiteral("exited"), true}});
        }
    } else if (event == QLatin1String("subagent")) {
        if (mergeAgent(m_agents, obj))
            emit panelRequested(QStringLiteral("agents"));
        emit agentsChanged();
    } else if (event == QLatin1String("handoff")) {
        // Show where the chat moved to another model, just above the reply it is writing.
        insertHandoff(m_view, obj);
    } else if (event == QLatin1String("providerUsage")) {
        const QString provider = obj.value(QStringLiteral("provider")).toString();
        for (QVariant &row : m_providers) {
            QVariantMap map = row.toMap();
            if (map.value(QStringLiteral("id")).toString() == provider) {
                map.insert(QStringLiteral("usage"), obj.value(QStringLiteral("usage")).toObject().toVariantMap());
                row = map;
                break;
            }
        }
        emit providersChanged();
    } else if (event == QLatin1String("completed") || event == QLatin1String("failed") || event == QLatin1String("cancelled")) {
        if (event == QLatin1String("completed")) {
            const QString id = obj.value(QStringLiteral("sessionId")).toString();
            if (!id.isEmpty() && id != m_activeSession) {
                m_activeSession = id;
                emit activeSessionChanged();
            }
            setStatus(QString());
        } else if (event == QLatin1String("failed")) {
            setStatus(obj.value(QStringLiteral("message")).toString());
        }
        setRunning(false);
        if (!m_viewRun.isEmpty()) {
            m_runs.remove(m_viewRun);   // its model stays on screen
            m_viewRun.clear();
            emit liveChatsChanged();
        }
        // Queued messages go out after a reply; a failure or Stop holds them until you resume.
        if (!m_queue.isEmpty()) {
            if (event == QLatin1String("completed") || m_sendNowPending)
                QTimer::singleShot(200, this, &IpcClient::dispatchQueue);
            else
                pauseQueue();
        }
        m_sendNowPending = false;
        if (!m_approval.isEmpty()) {
            m_approval.clear();
            emit approvalChanged();
        }
        request(QStringLiteral("sessions.list"), {}, QStringLiteral("sessions"));
        request(QStringLiteral("projects.list"), {}, QStringLiteral("projects"));
        request(QStringLiteral("account"), {}, QStringLiteral("account"));
    }
}

void IpcClient::beginAssistantTurn() {
    Turn turn;
    turn.role = QStringLiteral("assistant");
    turn.model = m_selectedModel;
    turn.streaming = true;
    m_view->append(turn);
}

void IpcClient::setRunning(bool on) {
    if (!on) {
        int row = -1;
        if (Turn *turn = m_view->lastAssistant(&row)) {
            if (turn->streaming) {
                turn->streaming = false;
                m_view->changed(row, {TurnModel::StreamingRole});
            }
        }
    }
    if (m_running == on)
        return;
    m_running = on;
    emit runningChanged();
}

void IpcClient::setStatus(const QString &text) {
    if (m_status == text)
        return;
    m_status = text;
    emit statusChanged();
}

void IpcClient::setNotice(const QString &text) {
    m_notice = text;
    emit noticeChanged();
}

void IpcClient::updateTool(const QJsonObject &obj, TurnModel *model) {
    int row = -1;
    Turn *turn = model->lastAssistant(&row);
    if (!turn)
        return;
    const QString id = obj.value(QStringLiteral("id")).toString();
    const QString phase = obj.value(QStringLiteral("phase")).toString();
    const QString name = obj.value(QStringLiteral("name")).toString();
    int index = -1;
    for (int i = 0; i < turn->tools.size(); ++i) {
        if (turn->tools.at(i).toMap().value(QStringLiteral("id")).toString() == id) {
            index = i;
            break;
        }
    }
    QVariantMap tool = index >= 0 ? turn->tools.at(index).toMap() : QVariantMap{{QStringLiteral("id"), id}};
    tool.insert(QStringLiteral("name"), name);
    if (phase == QLatin1String("requested")) {
        if (!tool.contains(QStringLiteral("state")))
            tool.insert(QStringLiteral("state"), QStringLiteral("pending"));
        const QString detail = toolDetail(obj.value(QStringLiteral("arguments")).toString());
        if (!detail.isEmpty())
            tool.insert(QStringLiteral("summary"), detail);
    } else if (phase == QLatin1String("start")) {
        tool.insert(QStringLiteral("state"), QStringLiteral("running"));
        if (name.startsWith(QLatin1String("browser_")))
            emit browserActivity();
    } else if (phase == QLatin1String("done")) {
        tool.insert(QStringLiteral("state"), obj.value(QStringLiteral("ok")).toBool() ? QStringLiteral("ok") : QStringLiteral("failed"));
        const QString result = obj.value(QStringLiteral("summary")).toString().section(QLatin1Char('\n'), 0, 0);
        if (!result.isEmpty())
            tool.insert(QStringLiteral("result"), result);
    }
    if (index >= 0)
        turn->tools[index] = tool;
    else
        turn->tools.append(tool);
    model->changed(row, {TurnModel::ToolsRole});
}

void IpcClient::applyAccount(const QJsonObject &result) {
    m_accountLoaded = true;
    m_signedIn = result.value(QStringLiteral("signedIn")).toBool();
    m_username = result.value(QStringLiteral("username")).toString();
    m_email = result.value(QStringLiteral("email")).toString();
    m_plan = result.value(QStringLiteral("plan")).toString();
    m_isPremium = result.value(QStringLiteral("isPremium")).toBool();
    m_periodEnd = result.value(QStringLiteral("periodEnd")).toString();
    m_avatarUrl = result.value(QStringLiteral("avatar")).toString();
    m_sessionPercent = result.value(QStringLiteral("sessionPercent")).toDouble();
    m_sessionResetsAt = result.value(QStringLiteral("sessionResetsAt")).toString();
    m_weeklyPercent = result.value(QStringLiteral("weeklyPercent")).toDouble();
    m_sessionUsed = result.value(QStringLiteral("sessionUsed")).toDouble(-1);
    m_sessionLimit = result.value(QStringLiteral("sessionLimit")).toDouble(-1);
    m_weeklyUsed = result.value(QStringLiteral("weeklyUsed")).toDouble(-1);
    m_weeklyLimit = result.value(QStringLiteral("weeklyLimit")).toDouble(-1);
    m_weeklyResetsAt = result.value(QStringLiteral("weeklyResetsAt")).toString();
    if (!m_signedIn) {
        m_view->clear();
        m_sessions.clear();
        m_projects.clear();
        m_diffs.clear();
        m_activeSession.clear();
        emit sessionsChanged();
        emit projectsChanged();
        emit diffsChanged();
        emit activeSessionChanged();
    }
    emit accountChanged();
}

void IpcClient::applyModels(const QJsonObject &result) {
    m_freeModels = result.value(QStringLiteral("free")).toArray().toVariantList();
    m_premiumModels = result.value(QStringLiteral("premium")).toArray().toVariantList();
    if (result.contains(QStringLiteral("providers"))) {
        m_providers = result.value(QStringLiteral("providers")).toArray().toVariantList();
        emit providersChanged();
    }
    const QString selected = result.value(QStringLiteral("selected")).toString();
    if (!selected.isEmpty() && !(m_demo && qEnvironmentVariableIsSet("SERIKA_DEMO_MODEL")))
        m_selectedModel = selected;
    emit modelsChanged();
}

void IpcClient::applySettings(const QJsonObject &result) {
    if (result.isEmpty())
        return;
    const QString oldProject = m_projectPath;
    m_settings = result.toVariantMap();
    const QString locale = result.value(QStringLiteral("locale")).toString(QStringLiteral("en"));
    const bool localeChanged = locale != m_locale || m_strings.isEmpty();
    m_locale = locale;
    m_theme = result.value(QStringLiteral("theme")).toString(QStringLiteral("dark"));
    m_projectName = result.value(QStringLiteral("projectName")).toString();
    m_projectPath = result.value(QStringLiteral("project")).toString();
    m_version = result.value(QStringLiteral("version")).toString();
    const QString model = result.value(QStringLiteral("model")).toString();
    if (!model.isEmpty() && model != m_selectedModel && !(m_demo && qEnvironmentVariableIsSet("SERIKA_DEMO_MODEL"))) {
        m_selectedModel = model;
        emit modelsChanged();
    }
    emit settingsChanged();
    if (localeChanged)
        request(QStringLiteral("i18n"), {{QStringLiteral("locale"), locale}}, QStringLiteral("i18n"));
    if (oldProject != m_projectPath && !m_demo)
        request(QStringLiteral("projects.list"), {}, QStringLiteral("projects"));
}

void IpcClient::applyDictionary(const QJsonObject &result) {
    m_strings = result.value(QStringLiteral("strings")).toObject().toVariantMap();
    m_locales = result.value(QStringLiteral("locales")).toArray().toVariantList();
    emit stringsChanged();
}

QString IpcClient::modelName(const QString &id) const {
    if (id.isEmpty())
        return {};
    const auto scan = [&id](const QVariantList &rows) -> QString {
        for (const QVariant &row : rows) {
            const QVariantMap map = row.toMap();
            if (map.value(QStringLiteral("id")).toString() == id)
                return map.value(QStringLiteral("name")).toString();
        }
        return {};
    };
    QString name = scan(m_freeModels);
    if (name.isEmpty())
        name = scan(m_premiumModels);
    for (const QVariant &provider : m_providers) {
        if (!name.isEmpty())
            break;
        const QVariantMap map = provider.toMap();
        name = scan(map.value(QStringLiteral("models")).toList());
        if (name.startsWith(QLatin1String("Default")))
            name = map.value(QStringLiteral("name")).toString();
    }
    if (!name.isEmpty())
        return name;
    // claude-code/sonnet → Claude Code · sonnet
    if (id.startsWith(QLatin1String("claude-code/")))
        return QStringLiteral("Claude Code · ") + id.mid(12);
    if (id.startsWith(QLatin1String("codex/")))
        return QStringLiteral("Codex · ") + id.mid(6);
    return id.section(QLatin1Char('/'), -1);
}

/// Show a file in the system file manager, selected where the platform allows it.
void IpcClient::revealFile(const QString &path) {
    const QFileInfo info(path);
#ifdef Q_OS_WIN
    QProcess::startDetached(QStringLiteral("explorer.exe"), {QStringLiteral("/select,") + QDir::toNativeSeparators(info.absoluteFilePath())});
#elif defined(Q_OS_MACOS)
    QProcess::startDetached(QStringLiteral("open"), {QStringLiteral("-R"), info.absoluteFilePath()});
#else
    QDesktopServices::openUrl(QUrl::fromLocalFile(info.absolutePath()));
#endif
}

QString IpcClient::lastReplyModel() const {
    const QList<Turn> &turns = m_view->turns();
    for (int i = int(turns.size()) - 1; i >= 0; --i) {
        if (turns[i].role == QLatin1String("assistant") && !turns[i].model.isEmpty())
            return turns[i].model;
    }
    return {};
}

QString IpcClient::selectedModelName() const {
    const auto scan = [this](const QVariantList &rows) -> QString {
        for (const QVariant &row : rows) {
            const QVariantMap map = row.toMap();
            if (map.value(QStringLiteral("id")).toString() == m_selectedModel)
                return map.value(QStringLiteral("name")).toString();
        }
        return {};
    };
    QString name = scan(m_freeModels);
    if (name.isEmpty())
        name = scan(m_premiumModels);
    if (name.isEmpty()) {
        for (const QVariant &provider : m_providers) {
            name = scan(provider.toMap().value(QStringLiteral("models")).toList());
            if (!name.isEmpty())
                break;
        }
    }
    if (!name.isEmpty())
        return name;
    return m_selectedModel == QLatin1String("openrouter/free") ? QStringLiteral("openrouter Free") : m_selectedModel;
}

QString IpcClient::selectedProvider() const {
    if (m_selectedModel.startsWith(QLatin1String("claude-code/")))
        return QStringLiteral("claude");
    if (m_selectedModel.startsWith(QLatin1String("codex/")))
        return QStringLiteral("codex");
    return QStringLiteral("serika");
}

QString IpcClient::permissionMode() const {
    const QString mode = m_settings.value(QStringLiteral("permission")).toString();
    return mode.isEmpty() ? QStringLiteral("standard") : mode;
}

void IpcClient::setPlanMode(bool on) {
    if (m_planMode == on)
        return;
    m_planMode = on;
    emit planModeChanged();
}

void IpcClient::login(const QString &email, const QString &password, const QString &twoFactor) {
    request(QStringLiteral("auth.login"),
            {{QStringLiteral("email"), email}, {QStringLiteral("password"), password}, {QStringLiteral("twoFactor"), twoFactor}},
            QStringLiteral("auth.login"));
}

void IpcClient::registerAccount(const QString &username, const QString &email, const QString &password, const QString &dateOfBirth) {
    request(QStringLiteral("auth.register"),
            {{QStringLiteral("username"), username},
             {QStringLiteral("email"), email},
             {QStringLiteral("password"), password},
             {QStringLiteral("dateOfBirth"), dateOfBirth}},
            QStringLiteral("auth.register"));
}

void IpcClient::requestMagicLink(const QString &email) {
    request(QStringLiteral("auth.magic"), {{QStringLiteral("email"), email}}, QStringLiteral("auth.magic"));
}

void IpcClient::consumeMagicLink(const QString &token, const QString &twoFactor) {
    request(QStringLiteral("auth.magicConsume"),
            {{QStringLiteral("token"), token}, {QStringLiteral("twoFactor"), twoFactor}},
            QStringLiteral("auth.magicConsume"));
}

void IpcClient::logout() {
    if (m_demo) {
        m_signedIn = false;
        emit accountChanged();
        return;
    }
    m_view->clear();
    m_activeSession.clear();
    emit activeSessionChanged();
    request(QStringLiteral("auth.logout"), {}, QStringLiteral("auth.logout"));
}

void IpcClient::refresh() {
    request(QStringLiteral("account"), {}, QStringLiteral("account"));
    request(QStringLiteral("models"), {}, QStringLiteral("models"));
    request(QStringLiteral("sessions.list"), {}, QStringLiteral("sessions"));
    request(QStringLiteral("projects.list"), {}, QStringLiteral("projects"));
}

void IpcClient::refreshProviders(bool force) {
    m_providersLoading = true;
    emit providersChanged();
    request(QStringLiteral("providers.status"), {{QStringLiteral("refresh"), force}}, QStringLiteral("providers"));
}

void IpcClient::probeClaudeUsage() {
    m_providersLoading = true;
    emit providersChanged();
    request(QStringLiteral("providers.probe"), {}, QStringLiteral("probe"));
}

void IpcClient::checkUpdates() {
    request(QStringLiteral("update.check"), {}, QStringLiteral("update"));
}

void IpcClient::send(const QString &prompt) {
    const QString trimmed = prompt.trimmed();
    if (trimmed.isEmpty() && m_attachments.isEmpty())
        return;
    const QVariantList files = m_attachments;
    if (!m_attachments.isEmpty()) {
        m_attachments.clear();
        emit attachmentsChanged();
    }
    if (m_running || (!m_queue.isEmpty() && m_queuePaused)) {
        m_queue.append(QVariantMap{{QStringLiteral("text"), trimmed}, {QStringLiteral("attachments"), files}});
        emit queueChanged();
        return;
    }
    startSend(trimmed, files);
}

void IpcClient::startSend(const QString &trimmed, const QVariantList &files) {
    // What was typed is now a message; the draft (and a new chat's draft row) goes away.
    if (m_drafts.remove(currentDraftKey()) > 0)
        saveDrafts();
    if (!m_draftKey.isEmpty()) {
        m_draftKey.clear();
    }
    emit draftsChanged();
    Turn user;
    user.role = QStringLiteral("user");
    user.raw = trimmed;
    user.content = trimmed;
    user.attachments = files;
    m_view->append(user);
    setStatus(QString());
    setRunning(true);
    QJsonObject params{{QStringLiteral("prompt"), trimmed},
                       {QStringLiteral("model"), m_selectedModel},
                       {QStringLiteral("permission"), permissionMode()}};
    if (m_planMode)
        params.insert(QStringLiteral("mode"), QStringLiteral("plan"));
    if (!files.isEmpty()) {
        QJsonArray list;
        for (const QVariant &item : files)
            list.append(QJsonObject::fromVariantMap(item.toMap()));
        params.insert(QStringLiteral("attachments"), list);
    }
    if (!m_activeSession.isEmpty())
        params.insert(QStringLiteral("sessionId"), m_activeSession);
    // This chat runs on its own: it keeps going if you open another chat or project.
    LiveRun run;
    run.runId = QUuid::createUuid().toString(QUuid::WithoutBraces);
    run.sessionId = m_activeSession;
    run.project = m_projectPath;
    run.model = m_view;
    m_runs.insert(run.runId, run);
    m_viewRun = run.runId;
    params.insert(QStringLiteral("runId"), run.runId);
    params.insert(QStringLiteral("project"), m_projectPath);
    request(QStringLiteral("chat.send"), params, QStringLiteral("chat"));
    emit liveChatsChanged();
}

void IpcClient::detachView() {
    if (m_viewRun.isEmpty())
        return;
    auto it = m_runs.find(m_viewRun);
    if (it != m_runs.end()) {
        it->diffs = m_diffs;
        it->agents = m_agents;
        it->approval = m_approval;
        it->rounds = m_roundsSeen;
        it->queue = m_queue;
        it->sessionId = m_activeSession.isEmpty() ? it->sessionId : m_activeSession;
    }
    m_viewRun.clear();
    // The running chat keeps its model; the screen gets a fresh one.
    m_view = new TurnModel(this);
    emit turnsModelChanged();
    m_queue.clear();
    emit queueChanged();
    if (!m_approval.isEmpty()) {
        m_approval.clear();
        emit approvalChanged();
    }
    if (m_running) {
        m_running = false;
        emit runningChanged();
    }
    emit liveChatsChanged();
}

void IpcClient::attachRun(const QString &runId) {
    if (runId == m_viewRun || !m_runs.contains(runId))
        return;
    detachView();
    TurnModel *old = m_view;
    LiveRun &run = m_runs[runId];
    m_view = run.model;
    m_viewRun = runId;
    bool owned = false;
    for (const LiveRun &other : std::as_const(m_runs))
        owned = owned || other.model == old;
    if (!owned && old != m_view)
        old->deleteLater();
    m_activeSession = run.sessionId;
    m_diffs = run.diffs;
    m_agents = run.agents;
    m_approval = run.approval;
    m_roundsSeen = run.rounds;
    m_queue = run.queue;
    m_unread.remove(run.sessionId);
    emit turnsModelChanged();
    emit activeSessionChanged();
    emit diffsChanged();
    emit agentsChanged();
    emit approvalChanged();
    emit queueChanged();
    emit turnsReloaded();
    setRunning(true);
    emit liveChatsChanged();
    // Bring its project and settings along (the turns on screen are already live).
    if (!run.sessionId.isEmpty())
        request(QStringLiteral("sessions.open"), {{QStringLiteral("id"), run.sessionId}}, QStringLiteral("sessionSettings"));
}

void IpcClient::handleBackgroundEvent(LiveRun &run, const QString &event, const QJsonObject &obj) {
    TurnModel *model = run.model;
    if (event == QLatin1String("started")) {
        run.sessionId = obj.value(QStringLiteral("sessionId")).toString();
        run.rounds = 0;
        request(QStringLiteral("sessions.list"), {}, QStringLiteral("sessions"));
        emit liveChatsChanged();
    } else if (event == QLatin1String("status")) {
        if (obj.value(QStringLiteral("text")).toString().startsWith(QLatin1String("turn ")) && ++run.rounds > 1)
            nextRound(model);
    } else if (event == QLatin1String("chunk") || event == QLatin1String("reasoning")) {
        appendStream(model, event, obj.value(QStringLiteral("text")).toString());
    } else if (event == QLatin1String("tool")) {
        updateTool(obj, model);
    } else if (event == QLatin1String("toolOutput")) {
        setToolOutput(model, obj);
    } else if (event == QLatin1String("diff")) {
        mergeDiff(run.diffs, model, obj);
    } else if (event == QLatin1String("subagent")) {
        mergeAgent(run.agents, obj);
    } else if (event == QLatin1String("handoff")) {
        insertHandoff(model, obj);
    } else if (event == QLatin1String("approval")) {
        run.approval = obj.toVariantMap();
        setNotice(QStringLiteral("A chat running in the background is waiting for your approval."));
        emit liveChatsChanged();
    } else {
        // completed, failed or cancelled
        if (!obj.value(QStringLiteral("sessionId")).toString().isEmpty())
            run.sessionId = obj.value(QStringLiteral("sessionId")).toString();
        finishRun(run.runId);
    }
}

void IpcClient::finishRun(const QString &runId) {
    auto it = m_runs.find(runId);
    if (it == m_runs.end())
        return;
    if (!it->sessionId.isEmpty())
        m_unread.insert(it->sessionId);
    if (it->model && it->model != m_view)
        it->model->deleteLater();
    m_runs.erase(it);
    emit liveChatsChanged();
    request(QStringLiteral("sessions.list"), {}, QStringLiteral("sessions"));
}

QVariantMap IpcClient::liveChats() const {
    // session id -> "running", "waiting" (for an approval) or "done" (finished while away)
    QVariantMap out;
    for (const QString &id : m_unread)
        out.insert(id, QStringLiteral("done"));
    for (const LiveRun &run : m_runs) {
        if (run.sessionId.isEmpty())
            continue;
        const bool onScreen = run.runId == m_viewRun;
        const bool waiting = onScreen ? !m_approval.isEmpty() : !run.approval.isEmpty();
        out.insert(run.sessionId, waiting ? QStringLiteral("waiting") : QStringLiteral("running"));
    }
    return out;
}

void IpcClient::stopChat(const QString &sessionId) {
    request(QStringLiteral("chat.cancel"), {{QStringLiteral("sessionId"), sessionId}}, QStringLiteral("cancel"));
}

void IpcClient::gitRefresh() {
    if (m_demo)
        return;
    request(QStringLiteral("git.overview"), {{QStringLiteral("project"), m_projectPath}}, QStringLiteral("git"));
}

void IpcClient::setGit(const QVariantMap &overview) {
    m_git = overview;
    m_git.remove(QStringLiteral("busy"));
    if (!m_gitBusy.isEmpty())
        m_git.insert(QStringLiteral("busy"), m_gitBusy);
    emit gitChanged();
}

void IpcClient::gitAction(const QString &action, int index, const QString &message, const QString &label, const QStringList &paths) {
    // One git operation at a time: a second click while one runs would race it.
    if (!m_gitBusy.isEmpty()) {
        emit gitMessage(QStringLiteral("Wait for \"%1\" to finish.").arg(m_gitBusy.value(QStringLiteral("label")).toString()), true);
        return;
    }
    QJsonObject params{{QStringLiteral("project"), m_projectPath}, {QStringLiteral("action"), action}, {QStringLiteral("message"), message}};
    if (index >= 0)
        params.insert(QStringLiteral("index"), index);
    if (!paths.isEmpty())
        params.insert(QStringLiteral("paths"), QJsonArray::fromStringList(paths));
    m_gitBusy = {{QStringLiteral("action"), action}, {QStringLiteral("index"), index},
                 {QStringLiteral("label"), label.isEmpty() ? action : label},
                 {QStringLiteral("startedAt"), QDateTime::currentMSecsSinceEpoch()}};
    setGit(m_git);
    request(QStringLiteral("git.action"), params, QStringLiteral("gitAction"));
}

void IpcClient::gitBranchesRefresh() {
    request(QStringLiteral("git.branches"), {{QStringLiteral("project"), m_projectPath}}, QStringLiteral("gitBranches"));
}

void IpcClient::gitAccountsRefresh() {
    request(QStringLiteral("git.accounts"), {{QStringLiteral("project"), m_projectPath}}, QStringLiteral("gitAccounts"));
}

void IpcClient::gitAccountSave(const QVariantMap &account) {
    request(QStringLiteral("git.accountSave"), {{QStringLiteral("project"), m_projectPath}, {QStringLiteral("account"), QJsonObject::fromVariantMap(account)}},
            QStringLiteral("gitAccounts"));
}

void IpcClient::gitAccountRemove(const QString &id) {
    request(QStringLiteral("git.accountRemove"), {{QStringLiteral("project"), m_projectPath}, {QStringLiteral("id"), id}}, QStringLiteral("gitAccounts"));
}

void IpcClient::gitAccountUse(const QString &id, bool makeDefault) {
    if (!m_gitBusy.isEmpty()) {
        emit gitMessage(QStringLiteral("Wait for \"%1\" to finish.").arg(m_gitBusy.value(QStringLiteral("label")).toString()), true);
        return;
    }
    m_gitBusy = {{QStringLiteral("action"), QStringLiteral("account")}, {QStringLiteral("index"), -1},
                 {QStringLiteral("label"), QStringLiteral("Switching git account")},
                 {QStringLiteral("startedAt"), QDateTime::currentMSecsSinceEpoch()}};
    setGit(m_git);
    request(QStringLiteral("git.accountUse"), {{QStringLiteral("project"), m_projectPath}, {QStringLiteral("id"), id}, {QStringLiteral("makeDefault"), makeDefault}},
            QStringLiteral("gitAction"));
}

void IpcClient::projectDetails(const QString &project) {
    request(QStringLiteral("project.details"), {{QStringLiteral("project"), project}}, QStringLiteral("projectDetails:") + project);
}

void IpcClient::projectCreateInstructions(const QString &project) {
    request(QStringLiteral("project.createInstructions"), {{QStringLiteral("project"), project}}, QStringLiteral("projectInstructions:") + project);
}

void IpcClient::gitRemoteAuth(const QString &project) {
    request(QStringLiteral("git.remoteAuth"), {{QStringLiteral("project"), project}}, QStringLiteral("gitRemoteAuth:") + project);
}

void IpcClient::gitSetLogin(const QString &project, const QString &login) {
    request(QStringLiteral("git.setLogin"), {{QStringLiteral("project"), project}, {QStringLiteral("login"), login}}, QStringLiteral("gitRemoteAuth:") + project);
}

void IpcClient::gitApplyAccountIn(const QString &project, const QString &id) {
    // Applies the account and makes it the project's default; the answer reloads the sheet.
    request(QStringLiteral("git.accountUse"), {{QStringLiteral("project"), project}, {QStringLiteral("id"), id}, {QStringLiteral("makeDefault"), true}},
            QStringLiteral("gitAccountIn:") + project);
}

void IpcClient::gitAccountDefault(const QString &project, const QString &id) {
    request(QStringLiteral("git.accountDefault"), {{QStringLiteral("project"), project}, {QStringLiteral("id"), id}}, QStringLiteral("gitAccounts"));
}

void IpcClient::gitStashDiff(int index) {
    request(QStringLiteral("git.stashDiff"), {{QStringLiteral("project"), m_projectPath}, {QStringLiteral("index"), index}}, QStringLiteral("gitDiff"));
}

void IpcClient::gitCommitDiff(const QString &hash) {
    request(QStringLiteral("git.commitDiff"), {{QStringLiteral("project"), m_projectPath}, {QStringLiteral("hash"), hash}}, QStringLiteral("gitDiff"));
}

void IpcClient::gitFileDiff(const QString &path, bool untracked) {
    request(QStringLiteral("git.fileDiff"), {{QStringLiteral("project"), m_projectPath}, {QStringLiteral("path"), path}, {QStringLiteral("untracked"), untracked}},
            QStringLiteral("gitDiff"));
}

void IpcClient::gitConflictFile(const QString &path) {
    request(QStringLiteral("git.conflictFile"), {{QStringLiteral("project"), m_projectPath}, {QStringLiteral("path"), path}},
            QStringLiteral("gitConflict"));
}

void IpcClient::addAttachments(const QList<QUrl> &urls) {
    QJsonArray paths;
    for (const QUrl &url : urls) {
        if (url.isLocalFile())
            paths.append(url.toLocalFile());
    }
    if (paths.isEmpty()) {
        setNotice(QStringLiteral("Only files from your computer can be attached."));
        return;
    }
    QJsonArray existing;
    for (const QVariant &item : m_attachments)
        existing.append(QJsonObject::fromVariantMap(item.toMap()));
    request(QStringLiteral("attachments.add"), {{QStringLiteral("paths"), paths}, {QStringLiteral("existing"), existing}}, QStringLiteral("attachments"));
}

/// Ctrl+V: files copied in Explorer, or an image on the clipboard. Returns false for plain text.
bool IpcClient::pasteAttachments() {
    QClipboard *clipboard = QGuiApplication::clipboard();
    const QMimeData *mime = clipboard ? clipboard->mimeData() : nullptr;
    if (!mime)
        return false;
    if (mime->hasUrls()) {
        QList<QUrl> files;
        for (const QUrl &url : mime->urls()) {
            if (url.isLocalFile())
                files.append(url);
        }
        if (!files.isEmpty()) {
            addAttachments(files);
            return true;
        }
    }
    if (mime->hasImage()) {
        const QImage image = qvariant_cast<QImage>(mime->imageData());
        if (!image.isNull()) {
            const QString path = QDir::temp().filePath(QStringLiteral("pasted-%1.png").arg(QDateTime::currentMSecsSinceEpoch()));
            if (image.save(path, "PNG")) {
                addAttachments({QUrl::fromLocalFile(path)});
                return true;
            }
        }
    }
    return false;
}

void IpcClient::removeAttachment(int index) {
    if (index < 0 || index >= m_attachments.size())
        return;
    m_attachments.removeAt(index);
    emit attachmentsChanged();
}

void IpcClient::clearAttachments() {
    if (m_attachments.isEmpty())
        return;
    m_attachments.clear();
    emit attachmentsChanged();
}

void IpcClient::cancel() {
    if (m_viewRun.isEmpty() && m_activeSession.isEmpty())
        return;
    QJsonObject params;
    if (!m_viewRun.isEmpty())
        params.insert(QStringLiteral("runId"), m_viewRun);
    else
        params.insert(QStringLiteral("sessionId"), m_activeSession);
    request(QStringLiteral("chat.cancel"), params, QStringLiteral("cancel"));
}

// The position of a turn among the messages the user wrote: the user turn itself, or the
// prompt an assistant turn answered.
int IpcClient::userIndexFor(int row) const {
    const QList<Turn> &turns = m_view->turns();
    int index = -1;
    for (int i = 0; i <= row && i < turns.size(); ++i) {
        if (turns[i].role == QLatin1String("user"))
            ++index;
    }
    return index;
}

void IpcClient::turnRequest(const QString &method, int row, int offset, const QString &action) {
    if (m_running || m_activeSession.isEmpty()) {
        setNotice(m_running ? QStringLiteral("Wait for the reply to finish, or stop it first.")
                            : QStringLiteral("This chat has not been saved yet."));
        return;
    }
    const int index = userIndexFor(row) + offset;
    if (index < 0)
        return;
    m_turnAction = action;
    const bool session = method != QLatin1String("sessions.rewind");
    request(method, {{QStringLiteral("id"), m_activeSession}, {QStringLiteral("index"), index}},
            session ? QStringLiteral("session") : QStringLiteral("turnAction"));
}

bool IpcClient::canRewindTo(int row) const {
    // Rewinding to an assistant reply keeps it and drops what came after.
    const QList<Turn> &turns = m_view->turns();
    for (int i = row + 1; i < turns.size(); ++i)
        if (turns[i].role == QLatin1String("user"))
            return true;
    return false;
}

void IpcClient::rewindTo(int row) {
    if (!canRewindTo(row))
        return;
    const bool onUser = row < m_view->count() && m_view->turns()[row].role == QLatin1String("user");
    turnRequest(QStringLiteral("sessions.rewind"), row, onUser ? 0 : 1, QStringLiteral("rewind"));
}

void IpcClient::editMessage(int row) { turnRequest(QStringLiteral("sessions.rewind"), row, 0, QStringLiteral("edit")); }
void IpcClient::retryTurn(int row) { turnRequest(QStringLiteral("sessions.rewind"), row, 0, QStringLiteral("retry")); }
void IpcClient::forkFrom(int row) { turnRequest(QStringLiteral("sessions.fork"), row, 0, QString()); }
void IpcClient::deleteTurn(int row) { turnRequest(QStringLiteral("sessions.deleteTurn"), row, 0, QString()); }

void IpcClient::dispatchQueue() {
    if (m_running || m_queue.isEmpty())
        return;
    const QVariantMap next = m_queue.takeFirst().toMap();
    m_queuePaused = false;
    m_queueResumeAt = 0;
    emit queueChanged();
    startSend(next.value(QStringLiteral("text")).toString(), next.value(QStringLiteral("attachments")).toList());
}

void IpcClient::pauseQueue() {
    m_queuePaused = true;
    emit queueChanged();
}

void IpcClient::removeQueued(int index) {
    if (index < 0 || index >= m_queue.size())
        return;
    m_queue.removeAt(index);
    if (m_queue.isEmpty()) {
        m_queuePaused = false;
        m_queueResumeAt = 0;
        m_queueTimer.stop();
    }
    emit queueChanged();
}

void IpcClient::moveQueued(int index, int delta) {
    const int target = index + delta;
    if (index < 0 || index >= m_queue.size() || target < 0 || target >= m_queue.size())
        return;
    m_queue.move(index, target);
    emit queueChanged();
}

void IpcClient::editQueued(int index) {
    if (index < 0 || index >= m_queue.size())
        return;
    const QVariantMap item = m_queue.at(index).toMap();
    m_attachments.append(item.value(QStringLiteral("attachments")).toList());
    emit attachmentsChanged();
    emit composerFill(item.value(QStringLiteral("text")).toString());
    removeQueued(index);
}

void IpcClient::sendQueuedNow(int index) {
    if (index < 0 || index >= m_queue.size())
        return;
    m_queue.move(index, 0);
    emit queueChanged();
    if (m_running) {
        // Stop the current reply; the queue continues once it has stopped.
        m_sendNowPending = true;
        cancel();
    } else {
        dispatchQueue();
    }
}

void IpcClient::resumeQueue() {
    m_queueTimer.stop();
    m_queuePaused = false;
    m_queueResumeAt = 0;
    emit queueChanged();
    dispatchQueue();
}

void IpcClient::resumeQueueAt(qint64 epochSecs) {
    const qint64 wait = epochSecs - QDateTime::currentSecsSinceEpoch();
    if (wait <= 0) {
        resumeQueue();
        return;
    }
    m_queueResumeAt = epochSecs;
    // Timers top out near 24 days; limits reset well within that.
    m_queueTimer.start(int(qMin<qint64>(wait * 1000 + 5000, 0x7fffffff)));
    emit queueChanged();
}

void IpcClient::clearQueue() {
    m_queue.clear();
    m_queuePaused = false;
    m_queueResumeAt = 0;
    m_queueTimer.stop();
    emit queueChanged();
}

void IpcClient::openSession(const QString &id) {
    if (id == m_activeSession && m_view->count() > 0)
        return;
    if (id.startsWith(QLatin1String("draft:"))) {
        if (id == m_draftKey)
            return;
        const QVariantMap draft = m_drafts.value(id).toMap();
        const QString folder = draft.value(QStringLiteral("project")).toString();
        detachView();
        leaveDraft();
        m_activeSession.clear();
        m_view->clear();
        m_diffs.clear();
        m_agents.clear();
        setStatus(QString());
        m_draftKey = id;
        emit activeSessionChanged();
        emit diffsChanged();
        emit agentsChanged();
        emit draftsChanged();
        emit composerFill(draft.value(QStringLiteral("text")).toString());
        if (!folder.isEmpty() && folder != m_projectPath)
            request(QStringLiteral("project.set"), {{QStringLiteral("path"), folder}}, QStringLiteral("settings"));
        return;
    }
    leaveDraft();
    // A chat that is still running opens live, exactly where it is.
    for (const LiveRun &run : std::as_const(m_runs)) {
        if (run.sessionId == id) {
            attachRun(run.runId);
            return;
        }
    }
    if (m_unread.remove(id))
        emit liveChatsChanged();
    request(QStringLiteral("sessions.open"), {{QStringLiteral("id"), id}}, QStringLiteral("session"));
}

QVariantList IpcClient::drafts() const {
    // "fresh" drafts are new chats (their own sidebar row); the others mark unsent text in
    // an existing chat.
    QVariantList out;
    for (auto it = m_drafts.cbegin(); it != m_drafts.cend(); ++it) {
        QVariantMap row = it.value().toMap();
        row.insert(QStringLiteral("id"), it.key());
        row.insert(QStringLiteral("fresh"), it.key().startsWith(QLatin1String("draft:")));
        out.append(row);
    }
    return out;
}

QString IpcClient::draftText() const {
    return m_drafts.value(currentDraftKey()).toMap().value(QStringLiteral("text")).toString();
}

void IpcClient::setDraftText(const QString &text) {
    const QString key = currentDraftKey();
    if (key.isEmpty())
        return;
    QVariantMap draft = m_drafts.value(key).toMap();
    if (draft.value(QStringLiteral("text")).toString() == text && m_drafts.contains(key))
        return;
    const bool newChat = key.startsWith(QLatin1String("draft:"));
    if (text.trimmed().isEmpty() && !newChat) {
        if (m_drafts.remove(key) > 0) {
            emit draftsChanged();
            saveDrafts();
        }
        return;
    }
    const qint64 now = QDateTime::currentSecsSinceEpoch();
    draft.insert(QStringLiteral("text"), text);
    draft.insert(QStringLiteral("updatedAt"), now);
    if (!draft.contains(QStringLiteral("createdAt")))
        draft.insert(QStringLiteral("createdAt"), now);
    if (!draft.contains(QStringLiteral("project")))
        draft.insert(QStringLiteral("project"), m_projectPath);
    m_drafts.insert(key, draft);
    emit draftsChanged();
    saveDrafts();
}

void IpcClient::saveDrafts() {
    // Only drafts with text are worth keeping across restarts.
    if (!m_draftSave.isSingleShot()) {
        m_draftSave.setSingleShot(true);
        m_draftSave.setInterval(600);
        connect(&m_draftSave, &QTimer::timeout, this, [this] {
            QJsonObject keep;
            for (auto it = m_drafts.cbegin(); it != m_drafts.cend(); ++it)
                if (!it.value().toMap().value(QStringLiteral("text")).toString().trimmed().isEmpty())
                    keep.insert(it.key(), QJsonObject::fromVariantMap(it.value().toMap()));
            setUi(QStringLiteral("drafts"), QString::fromUtf8(QJsonDocument(keep).toJson(QJsonDocument::Compact)));
        });
    }
    m_draftSave.start();
}

void IpcClient::leaveDraft() {
    if (m_draftKey.isEmpty())
        return;
    if (m_drafts.value(m_draftKey).toMap().value(QStringLiteral("text")).toString().trimmed().isEmpty()) {
        m_drafts.remove(m_draftKey);
        saveDrafts();
    }
    m_draftKey.clear();
    emit draftsChanged();
}

void IpcClient::newChat() {
    detachView();
    leaveDraft();
    m_activeSession.clear();
    // The new chat shows in the sidebar right away, as a draft, until its first message.
    if (!m_demo) {
        m_draftKey = QStringLiteral("draft:") + QUuid::createUuid().toString(QUuid::WithoutBraces);
        const qint64 now = QDateTime::currentSecsSinceEpoch();
        m_drafts.insert(m_draftKey, QVariantMap{{QStringLiteral("text"), QString()}, {QStringLiteral("project"), m_projectPath},
                                                {QStringLiteral("createdAt"), now}, {QStringLiteral("updatedAt"), now}});
        emit draftsChanged();
        emit composerFill(QString());
    }
    m_view->clear();
    m_diffs.clear();
    m_agents.clear();
    setStatus(QString());
    emit activeSessionChanged();
    emit diffsChanged();
    emit agentsChanged();
}

void IpcClient::newChatIn(const QString &folder) {
    newChat();
    if (!m_draftKey.isEmpty()) {
        QVariantMap draft = m_drafts.value(m_draftKey).toMap();
        draft.insert(QStringLiteral("project"), folder);
        m_drafts.insert(m_draftKey, draft);
        emit draftsChanged();
    }
    if (folder != m_projectPath) {
        request(QStringLiteral("project.set"), {{QStringLiteral("path"), folder}}, QStringLiteral("settings"));
    }
}

void IpcClient::renameSession(const QString &id, const QString &title) {
    if (title.trimmed().isEmpty())
        return;
    request(QStringLiteral("sessions.rename"), {{QStringLiteral("id"), id}, {QStringLiteral("title"), title.trimmed()}}, QStringLiteral("mutated"));
}

void IpcClient::deleteSession(const QString &id) {
    if (id.startsWith(QLatin1String("draft:"))) {
        const bool current = id == m_draftKey;
        m_drafts.remove(id);
        if (current)
            m_draftKey.clear();
        saveDrafts();
        emit draftsChanged();
        if (current)
            newChat();
        return;
    }
    for (const LiveRun &run : std::as_const(m_runs))
        if (run.sessionId == id)
            stopChat(id);
    if (id == m_activeSession)
        newChat();
    request(QStringLiteral("sessions.delete"), {{QStringLiteral("id"), id}}, QStringLiteral("mutated"));
}

void IpcClient::chooseModel(const QString &id) {
    if (id.isEmpty() || id == m_selectedModel)
        return;
    m_selectedModel = id;
    emit modelsChanged();
    request(QStringLiteral("settings.set"), {{QStringLiteral("key"), QStringLiteral("model")}, {QStringLiteral("value"), id}}, QStringLiteral("settings"));
}

void IpcClient::setLocale(const QString &code) {
    request(QStringLiteral("settings.set"), {{QStringLiteral("key"), QStringLiteral("locale")}, {QStringLiteral("value"), code}}, QStringLiteral("settings"));
}

void IpcClient::setTheme(const QString &name) {
    m_theme = name;
    emit settingsChanged();
    request(QStringLiteral("settings.set"), {{QStringLiteral("key"), QStringLiteral("theme")}, {QStringLiteral("value"), name}}, QStringLiteral("settings"));
}

void IpcClient::setSetting(const QString &key, const QVariant &value) {
    request(QStringLiteral("settings.set"),
            {{QStringLiteral("key"), key}, {QStringLiteral("value"), toStringValue(value)}},
            QStringLiteral("settings"));
}

void IpcClient::setUi(const QString &key, const QVariant &value) {
    m_ui.insert(key, value);
    emit uiChanged();
    request(QStringLiteral("ui.set"), {{QStringLiteral("key"), key}, {QStringLiteral("value"), QJsonValue::fromVariant(value)}}, QStringLiteral("uiSet"));
}

void IpcClient::addProject(const QUrl &folder) {
    const QString path = folder.isLocalFile() ? folder.toLocalFile() : folder.toString();
    if (path.isEmpty())
        return;
    newChat();
    request(QStringLiteral("project.set"), {{QStringLiteral("path"), path}}, QStringLiteral("settings"));
    request(QStringLiteral("projects.list"), {}, QStringLiteral("projects"));
}

void IpcClient::removeProject(const QString &path) {
    request(QStringLiteral("projects.remove"), {{QStringLiteral("path"), path}}, QStringLiteral("projects"));
}

void IpcClient::resolveApproval(bool allow) {
    const QString id = m_approval.value(QStringLiteral("id")).toString();
    if (id.isEmpty())
        return;
    m_approval.clear();
    emit approvalChanged();
    request(QStringLiteral("approval.resolve"), {{QStringLiteral("id"), id}, {QStringLiteral("allow"), allow}}, QStringLiteral("approval"));
}

bool IpcClient::exportMarkdown(const QUrl &file) {
    QFile out(file.isLocalFile() ? file.toLocalFile() : file.toString());
    if (!out.open(QIODevice::WriteOnly | QIODevice::Text))
        return false;
    QTextStream stream(&out);
    stream.setEncoding(QStringConverter::Utf8);
    stream << "# Serika Code\n\n";
    for (const Turn &turn : m_view->turns()) {
        stream << "## " << (turn.role == QLatin1String("user") ? "You" : "Serika") << "\n\n"
               << turn.content.trimmed() << "\n\n";
    }
    return true;
}

void IpcClient::copyText(const QString &text) {
    if (QClipboard *clipboard = QGuiApplication::clipboard())
        clipboard->setText(text);
}

QString IpcClient::folderName(const QString &path) const {
    const QString name = QFileInfo(path).fileName();
    return name.isEmpty() ? path : name;
}

void IpcClient::browserOpen(const QString &url) {
    m_browserBusy = true;
    m_browserError.clear();
    emit browserChanged();
    browserAction(QStringLiteral("browser.open"), {{QStringLiteral("url"), url}});
}

void IpcClient::browserBack() {
    m_browserBusy = true;
    emit browserChanged();
    browserAction(QStringLiteral("browser.back"), {});
}

void IpcClient::browserForward() {
    m_browserBusy = true;
    emit browserChanged();
    browserAction(QStringLiteral("browser.forward"), {});
}

void IpcClient::browserReload() {
    m_browserBusy = true;
    emit browserChanged();
    browserAction(QStringLiteral("browser.reload"), {});
}

void IpcClient::browserClick(double x, double y) {
    request(QStringLiteral("browser.click"), {{QStringLiteral("x"), x}, {QStringLiteral("y"), y}}, QStringLiteral("browser"));
}

void IpcClient::browserWheel(double x, double y, double dy) {
    // While streaming, the picture follows by itself, so skip the screenshot round trip.
    if (m_browserStreaming)
        request(QStringLiteral("browser.scroll"), {{QStringLiteral("x"), x}, {QStringLiteral("y"), y}, {QStringLiteral("dy"), dy}}, QStringLiteral("browserMouse"));
    else
        request(QStringLiteral("browser.wheel"), {{QStringLiteral("x"), x}, {QStringLiteral("y"), y}, {QStringLiteral("dy"), dy}}, QStringLiteral("browser"));
}

void IpcClient::browserInput(const QString &key, const QString &text) {
    request(QStringLiteral("browser.input"), {{QStringLiteral("key"), key}, {QStringLiteral("text"), text}}, QStringLiteral("browser"));
}

void IpcClient::browserNewTab(const QString &url) {
    m_browserBusy = true;
    m_browserError.clear();
    emit browserChanged();
    browserAction(QStringLiteral("browser.newTab"), {{QStringLiteral("url"), url}});
}

void IpcClient::browserAction(const QString &method, const QJsonObject &params) {
    if (m_demo)
        return;
    // Only the last of several quick switches matters.
    if (method == QLatin1String("browser.switchTab") && !m_browserQueue.isEmpty() &&
        m_browserQueue.last().first == method)
        m_browserQueue.removeLast();
    m_browserQueue.append({method, params});
    ++m_browserEpoch;
    if (!m_browserInFlight)
        browserNext();
}

void IpcClient::browserNext() {
    if (m_browserInFlight || m_browserQueue.isEmpty())
        return;
    const auto next = m_browserQueue.takeFirst();
    m_browserInFlight = true;
    request(next.first, next.second, QStringLiteral("browserAction"));
}

void IpcClient::browserSwitchTab(const QString &id) {
    // Show the switch at once; the browser's answer confirms it.
    QVariantList tabs = m_browser.value(QStringLiteral("tabs")).toList();
    for (QVariant &item : tabs) {
        QVariantMap tab = item.toMap();
        const bool active = tab.value(QStringLiteral("id")).toString() == id;
        tab.insert(QStringLiteral("active"), active);
        if (active) {
            m_browser.insert(QStringLiteral("url"), tab.value(QStringLiteral("url")));
            m_browser.insert(QStringLiteral("title"), tab.value(QStringLiteral("title")));
            m_browser.insert(QStringLiteral("activeTab"), id);
        }
        item = tab;
    }
    m_browser.insert(QStringLiteral("tabs"), tabs);
    emit browserChanged();
    browserAction(QStringLiteral("browser.switchTab"), {{QStringLiteral("id"), id}});
}

void IpcClient::browserCloseTab(const QString &id) {
    QVariantList tabs = m_browser.value(QStringLiteral("tabs")).toList();
    int index = -1;
    bool wasActive = false;
    for (int i = 0; i < tabs.size(); ++i) {
        const QVariantMap tab = tabs[i].toMap();
        if (tab.value(QStringLiteral("id")).toString() == id) {
            index = i;
            wasActive = tab.value(QStringLiteral("active")).toBool();
            break;
        }
    }
    if (index >= 0) {
        tabs.removeAt(index);
        if (tabs.isEmpty()) {
            m_browser.clear();
            m_browserImage.clear();
            emit browserImageChanged();
        } else {
            if (wasActive) {
                QVariantMap next = tabs[qMin(index, int(tabs.size()) - 1)].toMap();
                next.insert(QStringLiteral("active"), true);
                tabs[qMin(index, int(tabs.size()) - 1)] = next;
                m_browser.insert(QStringLiteral("url"), next.value(QStringLiteral("url")));
                m_browser.insert(QStringLiteral("title"), next.value(QStringLiteral("title")));
            }
            m_browser.insert(QStringLiteral("tabs"), tabs);
        }
        emit browserChanged();
    }
    browserAction(QStringLiteral("browser.closeTab"), {{QStringLiteral("id"), id}});
}

void IpcClient::browserResize(int width, int height) {
    if (m_demo)
        return;
    request(QStringLiteral("browser.resize"), {{QStringLiteral("width"), width}, {QStringLiteral("height"), height}}, QStringLiteral("browserPoll#%1").arg(m_browserEpoch));
}

void IpcClient::browserPoll() {
    if (m_demo || m_browserBusy || m_browserInFlight || !m_browserQueue.isEmpty())
        return;
    // While streaming, the picture arrives on its own; the poll only refreshes tabs and address.
    QJsonObject params;
    if (m_browserStreaming)
        params.insert(QStringLiteral("meta"), true);
    request(QStringLiteral("browser.frame"), params, QStringLiteral("browserPoll#%1").arg(m_browserEpoch));
}

void IpcClient::browserStream(bool on) {
    if (m_demo)
        return;
    // Panels come and go (moving between docks), so count who wants the picture.
    m_streamHolders = qMax(0, m_streamHolders + (on ? 1 : -1));
    on = m_streamHolders > 0;
    if (m_browserStreaming == on)
        return;
    m_browserStreaming = on;
    request(QStringLiteral("browser.stream"), {{QStringLiteral("on"), on}}, QStringLiteral("browserStream"));
}

void IpcClient::browserMouse(const QString &type, double x, double y, const QString &button, int buttons, int clicks) {
    if (m_demo)
        return;
    request(QStringLiteral("browser.mouse"),
            {{QStringLiteral("type"), type}, {QStringLiteral("x"), x}, {QStringLiteral("y"), y},
             {QStringLiteral("button"), button}, {QStringLiteral("buttons"), buttons}, {QStringLiteral("clicks"), clicks}},
            QStringLiteral("browserMouse"));
}

void IpcClient::browserCopy() { request(QStringLiteral("browser.copy"), {}, QStringLiteral("browserCopy")); }
void IpcClient::browserSelectAll() { request(QStringLiteral("browser.selectAll"), {}, QStringLiteral("browserMouse")); }

void IpcClient::browserPaste() {
    const QClipboard *clipboard = QGuiApplication::clipboard();
    const QString text = clipboard ? clipboard->text() : QString();
    if (!text.isEmpty())
        browserInput(QString(), text);
}

void IpcClient::updateTerminal(const QString &id, const QVariantMap &changes) {
    for (QVariant &row : m_terminals) {
        QVariantMap map = row.toMap();
        if (map.value(QStringLiteral("id")).toString() != id)
            continue;
        for (auto it = changes.begin(); it != changes.end(); ++it)
            map.insert(it.key(), it.value());
        row = map;
        emit terminalsChanged();
        return;
    }
}

void IpcClient::terminalOpen(const QString &cwd) {
    if (m_demo) {
        const QString id = QStringLiteral("demo%1").arg(m_terminals.size() + 1);
        m_terminals.append(QVariantMap{{QStringLiteral("id"), id}, {QStringLiteral("shell"), QStringLiteral("pwsh")},
                                       {QStringLiteral("cwd"), m_projectPath}, {QStringLiteral("running"), false}, {QStringLiteral("exited"), false}});
        m_termText[id] = QStringLiteral("PS> cargo test --workspace\n   Compiling serika-runtime v0.2.0\n    Finished test [unoptimized] target(s) in 4.31s\ntest result: ok. 76 passed; 0 failed\n");
        emit terminalsChanged();
        return;
    }
    QJsonObject params;
    if (!cwd.isEmpty())
        params.insert(QStringLiteral("cwd"), cwd);
    request(QStringLiteral("terminal.open"), params, QStringLiteral("terminalOpen"));
}

void IpcClient::terminalRun(const QString &id, const QString &command) {
    const QString shown = QStringLiteral("❯ ") + command + QLatin1Char('\n');
    m_termText[id].append(shown);
    emit terminalOutput(id, shown, false);
    updateTerminal(id, {{QStringLiteral("running"), true}});
    request(QStringLiteral("terminal.run"), {{QStringLiteral("id"), id}, {QStringLiteral("command"), command}}, QStringLiteral("terminal"));
}

void IpcClient::terminalInterrupt(const QString &id) {
    request(QStringLiteral("terminal.interrupt"), {{QStringLiteral("id"), id}}, QStringLiteral("terminal"));
}

void IpcClient::terminalClose(const QString &id) {
    for (int i = 0; i < m_terminals.size(); ++i) {
        if (m_terminals[i].toMap().value(QStringLiteral("id")).toString() == id) {
            m_terminals.removeAt(i);
            break;
        }
    }
    m_termText.remove(id);
    emit terminalsChanged();
    request(QStringLiteral("terminal.close"), {{QStringLiteral("id"), id}}, QStringLiteral("terminal"));
}

QString IpcClient::terminalText(const QString &id) const { return m_termText.value(id); }

void IpcClient::terminalClear(const QString &id) {
    m_termText[id].clear();
    emit terminalsChanged();
}

void IpcClient::mcpRefresh() { request(QStringLiteral("mcp.list"), {}, QStringLiteral("mcp")); }
void IpcClient::mcpConnect(const QString &name) { request(QStringLiteral("mcp.connect"), {{QStringLiteral("name"), name}}, QStringLiteral("mcp")); }
void IpcClient::mcpToggle(const QString &name, bool enabled) {
    request(QStringLiteral("mcp.toggle"), {{QStringLiteral("name"), name}, {QStringLiteral("enabled"), enabled}}, QStringLiteral("mcp"));
}
void IpcClient::mcpSave(const QString &name, const QString &definition) {
    request(QStringLiteral("mcp.save"), {{QStringLiteral("name"), name}, {QStringLiteral("definition"), definition}}, QStringLiteral("mcp"));
}
void IpcClient::mcpRemove(const QString &name) { request(QStringLiteral("mcp.remove"), {{QStringLiteral("name"), name}}, QStringLiteral("mcp")); }
void IpcClient::skillsRefresh() { request(QStringLiteral("skills.list"), {}, QStringLiteral("skills")); }
void IpcClient::skillsToggle(const QString &name, bool enabled) {
    request(QStringLiteral("skills.toggle"), {{QStringLiteral("name"), name}, {QStringLiteral("enabled"), enabled}}, QStringLiteral("skills"));
}
void IpcClient::deleteProject(const QString &path) {
    request(QStringLiteral("projects.delete"), {{QStringLiteral("path"), path}}, QStringLiteral("projects"));
}

void IpcClient::browserClose() {
    m_browser.clear();
    emit browserChanged();
    request(QStringLiteral("browser.close"), {}, QStringLiteral("browserClosed"));
}

void IpcClient::loadDemo() {
    // SERIKA_DEMO_UI='{"dockSide":"bottom"}' sets interface preferences for screenshots.
    const QJsonDocument demoUi = QJsonDocument::fromJson(qgetenv("SERIKA_DEMO_UI"));
    if (demoUi.isObject()) {
        m_ui = demoUi.object().toVariantMap();
        emit uiChanged();
    }
    // Two sub-agents: one still working, one finished.
    {
        const qint64 nowMs = QDateTime::currentMSecsSinceEpoch();
        m_agents = {
            QVariantMap{{QStringLiteral("id"), QStringLiteral("a1")}, {QStringLiteral("name"), QStringLiteral("Audit provider timeouts")},
                        {QStringLiteral("task"), QStringLiteral("Check every fetch_* function in providers.rs for a missing timeout and list them with line numbers.")},
                        {QStringLiteral("model"), QStringLiteral("claude-code/sonnet")}, {QStringLiteral("status"), QStringLiteral("running")},
                        {QStringLiteral("activity"), QStringLiteral("search_text fetch_.*timeout")}, {QStringLiteral("tools"), 4},
                        {QStringLiteral("startedAt"), nowMs - 41000}},
            QVariantMap{{QStringLiteral("id"), QStringLiteral("a2")}, {QStringLiteral("name"), QStringLiteral("Find duplicate model ids")},
                        {QStringLiteral("task"), QStringLiteral("Look for model ids that two providers both report and say how the picker shows them.")},
                        {QStringLiteral("model"), QStringLiteral("z-ai/glm-5.3-flash")}, {QStringLiteral("status"), QStringLiteral("done")},
                        {QStringLiteral("activity"), QStringLiteral("Finished")}, {QStringLiteral("tools"), 7},
                        {QStringLiteral("summary"), QStringLiteral("Two ids collide: `openai/gpt-4o` (OpenAI API and Serika) and `anthropic/claude-haiku-5.5`. The picker shows both rows with the same name; prefixing the provider fixes it.")},
                        {QStringLiteral("startedAt"), nowMs - 95000}, {QStringLiteral("endedAt"), nowMs - 62000}}};
    }
    // SERIKA_DEMO_QUEUE=1 shows two queued messages, paused by a limit.
    if (qEnvironmentVariableIntValue("SERIKA_DEMO_QUEUE") == 1) {
        m_queue = {QVariantMap{{QStringLiteral("text"), QStringLiteral("Now add tests for the model cache")}, {QStringLiteral("attachments"), QVariantList{}}},
                   QVariantMap{{QStringLiteral("text"), QStringLiteral("Then update the README with the new provider setup steps")}, {QStringLiteral("attachments"), QVariantList{}}}};
        m_queuePaused = true;
    }
    // SERIKA_DEMO_MODEL picks the selected model, e.g. claude-code/opus.
    if (qEnvironmentVariableIsSet("SERIKA_DEMO_MODEL"))
        m_selectedModel = qEnvironmentVariable("SERIKA_DEMO_MODEL");
    m_signedIn = true;
    m_accountLoaded = true;
    m_username = QStringLiteral("alex");
    m_email = QStringLiteral("alex@example.com");
    m_plan = QStringLiteral("Serika+ AI Max");
    m_isPremium = true;
    m_periodEnd = QStringLiteral("2026-11-30T00:00:00Z");
    m_avatarUrl = qEnvironmentVariable("SERIKA_DEMO_AVATAR");
    m_sessionPercent = 0.16;
    m_weeklyPercent = 0.36;
    m_sessionUsed = 72022;
    m_sessionLimit = 45000000;
    m_weeklyUsed = 872549;
    m_weeklyLimit = 240000000;
    m_sessionResetsAt = QDateTime::currentDateTimeUtc().addSecs(4 * 3600 + 57 * 60).toString(Qt::ISODate);
    m_weeklyResetsAt = QDateTime::currentDateTimeUtc().addDays(3).toString(Qt::ISODate);
    emit accountChanged();

    const auto model = [](const char *id, const char *name, bool premium, double mult, const char *provider) {
        return QVariantMap{{QStringLiteral("id"), s(id)},
                           {QStringLiteral("name"), s(name)},
                           {QStringLiteral("premium"), premium},
                           {QStringLiteral("usageMultiplier"), mult},
                           {QStringLiteral("provider"), s(provider)}};
    };
    m_freeModels = {model("openrouter/free", "openrouter Free", false, 1, "openrouter"),
                    model("deepseek/deepseek-v4.1-flash", "DeepSeek V4.1 Flash", false, 1, "deepseek"),
                    model("anthropic/claude-haiku-5.5", "Claude Haiku 5.5", false, 1, "anthropic")};
    m_premiumModels = {model("anthropic/claude-3.5-sonnet", "Claude 3.5 Sonnet", true, 5, "anthropic"),
                       model("openai/gpt-4o", "GPT-4o", true, 5, "openai"),
                       model("google/gemini-2.5-pro", "Gemini 2.5 Pro", true, 11, "google"),
                       model("google/gemini-3-flash", "Gemini 3 Flash", true, 1.5, "google")};
    const qint64 now = QDateTime::currentSecsSinceEpoch();
    const auto agentModel = [](const char *id, const char *name, const char *description, bool effort) {
        QVariantList efforts;
        if (effort)
            efforts = {s("low"), s("medium"), s("high"), s("xhigh"), s("max")};
        return QVariantMap{{QStringLiteral("id"), s(id)}, {QStringLiteral("name"), s(name)}, {QStringLiteral("description"), s(description)},
                           {QStringLiteral("provider"), s("claude-code")}, {QStringLiteral("efforts"), efforts}};
    };
    // SERIKA_DEMO_LIMIT=1 shows Claude Code with its weekly limit reached.
    const bool demoLimit = qEnvironmentVariableIntValue("SERIKA_DEMO_LIMIT") == 1;
    QVariantMap claudeUsage{
        {QStringLiteral("updated_at"), now},
        {QStringLiteral("limited"), demoLimit},
        {QStringLiteral("windows"),
         QVariantList{QVariantMap{{QStringLiteral("key"), s("five_hour")}, {QStringLiteral("label"), s("5-hour session")}, {QStringLiteral("used"), 0.18}, {QStringLiteral("resets_at"), now + 3 * 3600}, {QStringLiteral("status"), s("allowed")}},
                      QVariantMap{{QStringLiteral("key"), s("seven_day")}, {QStringLiteral("label"), s("Weekly")}, {QStringLiteral("used"), demoLimit ? 1.0 : 0.62}, {QStringLiteral("resets_at"), now + 3 * 86400}, {QStringLiteral("status"), s(demoLimit ? "rejected" : "allowed")}}}}};
    m_providers = {
        QVariantMap{{QStringLiteral("id"), s("claude")}, {QStringLiteral("name"), s("Claude Code")}, {QStringLiteral("installed"), true},
                    {QStringLiteral("version"), s("2.1.314 (Claude Code)")}, {QStringLiteral("signed_in"), true}, {QStringLiteral("account"), s("you@example.com")},
                    {QStringLiteral("plan"), s("Max")}, {QStringLiteral("detail"), s("Signed in with your Claude subscription.")},
                    {QStringLiteral("binary"), s("C:\\Users\\you\\.local\\bin\\claude.exe")}, {QStringLiteral("install_hint"), s("npm install -g @anthropic-ai/claude-code")},
                    {QStringLiteral("login_hint"), s("claude auth login")}, {QStringLiteral("docs_url"), s("https://claude.com/product/claude-code")},
                    {QStringLiteral("usage_url"), s("https://claude.ai/settings/usage")}, {QStringLiteral("usage"), claudeUsage},
                    {QStringLiteral("models_live"), true},
                    {QStringLiteral("models"), QVariantList{agentModel("claude-code/default", "Default (recommended)", "Opus 5.5 · Best for everyday, complex tasks", true),
                                                            agentModel("claude-code/opus", "Opus 5.5", "Best for everyday, complex tasks", true),
                                                            agentModel("claude-code/fable", "Fable 5.1", "Most capable for your hardest and longest-running tasks", true),
                                                            agentModel("claude-code/sonnet", "Sonnet 5.5", "Efficient for routine tasks", true),
                                                            agentModel("claude-code/haiku", "Haiku 5.5", "Fastest for quick answers", true),
                                                            agentModel("claude-code/claude-haiku-4-5-20251001", "Haiku 4.5", "Fastest for quick answers", false)}}},
        QVariantMap{{QStringLiteral("id"), s("codex")}, {QStringLiteral("name"), s("Codex")}, {QStringLiteral("installed"), false},
                    {QStringLiteral("detail"), s("Codex is not installed.")}, {QStringLiteral("install_hint"), s("npm install -g @openai/codex")},
                    {QStringLiteral("login_hint"), s("codex login")}, {QStringLiteral("docs_url"), s("https://developers.openai.com/codex/cli")},
                    {QStringLiteral("usage_url"), s("https://chatgpt.com/codex/settings/usage")},
                    {QStringLiteral("models"), QVariantList{model("codex/default", "Codex", false, 1, "codex")}}}};
    emit modelsChanged();
    emit providersChanged();

    const auto session = [now](const char *id, const char *title, qint64 ago, const char *project) {
        return QVariantMap{{QStringLiteral("id"), s(id)}, {QStringLiteral("title"), s(title)}, {QStringLiteral("updatedAt"), now - ago}, {QStringLiteral("project"), s(project)}};
    };
    const QString here = QDir::currentPath();
    m_sessions = {session("a", "Wire live /models discovery", 3600 * 2, "C:/Code/acme-api"),
                  session("b", "Add rate limiting to uploads", 3600 * 20, "C:/Code/acme-api"),
                  session("c", "Why is the checkout test flaky?", 86400 * 3, "C:/Code/storefront"),
                  session("d", "Build Pomodoro timer in React", 86400 * 12, "C:/Code/pomodoro"),
                  session("e", "OAuth 2.0 flow diagram", 86400 * 40, "C:/Code/storefront")};
    emit sessionsChanged();
    m_projects = {QVariantMap{{QStringLiteral("path"), s("C:/Code/acme-api")}, {QStringLiteral("name"), s("acme-api")}, {QStringLiteral("exists"), true}, {QStringLiteral("sessionCount"), 2}, {QStringLiteral("current"), true}},
                  QVariantMap{{QStringLiteral("path"), s("C:/Code/storefront")}, {QStringLiteral("name"), s("storefront")}, {QStringLiteral("exists"), true}, {QStringLiteral("sessionCount"), 2}, {QStringLiteral("current"), false}},
                  QVariantMap{{QStringLiteral("path"), s("C:/Code/pomodoro")}, {QStringLiteral("name"), s("pomodoro")}, {QStringLiteral("exists"), true}, {QStringLiteral("sessionCount"), 1}, {QStringLiteral("current"), false}},
                  QVariantMap{{QStringLiteral("path"), s("C:/Code/old-prototype")}, {QStringLiteral("name"), s("old-prototype")}, {QStringLiteral("exists"), false}, {QStringLiteral("sessionCount"), 0}, {QStringLiteral("current"), false}}};
    emit projectsChanged();
    m_projectName = QStringLiteral("acme-api");
    m_projectPath = s("C:/Code/acme-api");
    emit settingsChanged();
    Q_UNUSED(here)

    QVariantList turns;
    turns.append(QVariantMap{{QStringLiteral("role"), QStringLiteral("user")},
                             {QStringLiteral("content"), QStringLiteral("wire live /models discovery and check every provider call has a timeout")}});
    turns.append(QVariantMap{
        {QStringLiteral("role"), QStringLiteral("assistant")},
        {QStringLiteral("reasoning"),
         QStringLiteral("1. Searching the workspace for model discovery and provider calls.\n"
                        "2. Inspected providers.rs and client.rs.\n"
                        "3. Verified the chat client streams reasoning and tool calls separately.\n"
                        "4. Ran the workspace tests to confirm everything passes.")},
        {QStringLiteral("tools"),
         QVariantList{QVariantMap{{QStringLiteral("name"), QStringLiteral("search_text")}, {QStringLiteral("state"), QStringLiteral("ok")}, {QStringLiteral("summary"), QStringLiteral("timeout")}, {QStringLiteral("result"), QStringLiteral("14 matches in 5 files")}, {QStringLiteral("output"), QStringLiteral("src/providers.rs:212: // no timeout here\nsrc/client.rs:88: fn with_timeout")}},
                      QVariantMap{{QStringLiteral("name"), QStringLiteral("read_file")}, {QStringLiteral("state"), QStringLiteral("ok")}, {QStringLiteral("summary"), QStringLiteral("src/providers.rs")}, {QStringLiteral("result"), QStringLiteral("Read 142 lines")}},
                      QVariantMap{{QStringLiteral("name"), QStringLiteral("replace_in_file")}, {QStringLiteral("state"), QStringLiteral("ok")}, {QStringLiteral("summary"), QStringLiteral("src/providers.rs")}, {QStringLiteral("result"), QStringLiteral("Updated providers.rs (+28 -4)")}, {QStringLiteral("diffPath"), QStringLiteral("src/providers.rs")}},
                      QVariantMap{{QStringLiteral("name"), QStringLiteral("browser_open")}, {QStringLiteral("state"), QStringLiteral("ok")}, {QStringLiteral("summary"), QStringLiteral("https://docs.rs/tokio")}, {QStringLiteral("result"), QStringLiteral("Title: tokio - Rust")}},
                      QVariantMap{{QStringLiteral("name"), QStringLiteral("run_command")}, {QStringLiteral("state"), QStringLiteral("ok")}, {QStringLiteral("summary"), QStringLiteral("cargo test --workspace")}, {QStringLiteral("result"), QStringLiteral("test result: ok. 37 passed")}, {QStringLiteral("output"), QStringLiteral("running 37 tests\n...\ntest result: ok. 37 passed; 0 failed; finished in 1.74s")}},
                      QVariantMap{{QStringLiteral("name"), QStringLiteral("run_command")}, {QStringLiteral("state"), QStringLiteral("failed")}, {QStringLiteral("summary"), QStringLiteral("cargo clippy -- -D warnings")}, {QStringLiteral("result"), QStringLiteral("error: unused variable")}}}},
        {QStringLiteral("content"),
         QStringLiteral("### Live `/models` discovery\n\n"
                        "I traced the code and updated the runtime:\n\n"
                        "- **Reasoning traces** are captured from streaming deltas and stored with the session.\n"
                        "- **Live `/models`** now queries every provider you have a key for, then merges the Serika catalog.\n\n"
                        "```rust\npub async fn discover_all_models(http: &Client, cfg: &Config) -> Report {\n"
                        "    let (ollama, openai, claude) = tokio::join!(\n        fetch_ollama(http, cfg),\n        fetch_openai(http, cfg),\n        fetch_anthropic(http, cfg),\n    );\n"
                        "    Report::merge(ollama, openai, claude)\n}\n```\n\nAll tests pass.")},
        {QStringLiteral("model"), QStringLiteral("z-ai/glm-5.3-flash")}});
    // The chat then moved to Claude Code mid-conversation.
    turns.append(QVariantMap{{QStringLiteral("role"), QStringLiteral("user")},
                             {QStringLiteral("content"), QStringLiteral("Now review that change for edge cases.")}});
    turns.append(QVariantMap{{QStringLiteral("role"), QStringLiteral("switch")},
                             {QStringLiteral("from"), QStringLiteral("z-ai/glm-5.3-flash")},
                             {QStringLiteral("to"), QStringLiteral("claude-code/opus")},
                             {QStringLiteral("content"), QStringLiteral("4")}});
    turns.append(QVariantMap{{QStringLiteral("role"), QStringLiteral("assistant")},
                             {QStringLiteral("model"), QStringLiteral("claude-code/opus")},
                             {QStringLiteral("content"),
                              QStringLiteral("Two edge cases worth handling:\n\n1. A provider that times out should not block the others; `join!` already runs them together, but each fetch needs its own timeout.\n"
                                             "2. Duplicate model ids from different providers should keep the provider prefix so the picker can tell them apart.")}});
    m_view->replaceAll(turns);
    m_activeSession = QStringLiteral("a");
    emit activeSessionChanged();
    m_diffs = {QVariantMap{{QStringLiteral("path"), QStringLiteral("src/providers.rs")},
                           {QStringLiteral("diff"), QStringLiteral("@@ -172,6 +172,12 @@ pub async fn discover_all_models(\n-    let models = builtin_static_models();\n+    let (ollama, openai, claude) = tokio::join!(\n+        fetch_ollama(http, cfg),\n+        fetch_openai(http, cfg),\n+        fetch_anthropic(http, cfg),\n+    );\n     report\n }")}}};
    emit diffsChanged();

    // A sample page for the browser panel.
    QImage page(1280, 800, QImage::Format_RGB32);
    page.fill(QColor(QStringLiteral("#ffffff")));
    {
        QPainter painter(&page);
        painter.setRenderHint(QPainter::Antialiasing);
        painter.fillRect(QRect(0, 0, 1280, 72), QColor(QStringLiteral("#f4f4f5")));
        QFont title(QStringLiteral("Segoe UI"), 34, QFont::DemiBold);
        painter.setFont(title);
        painter.setPen(QColor(QStringLiteral("#18181b")));
        painter.drawText(QRect(80, 130, 1100, 60), Qt::AlignLeft | Qt::AlignVCenter, QStringLiteral("tokio — Rust async runtime"));
        QFont body(QStringLiteral("Segoe UI"), 15);
        painter.setFont(body);
        painter.setPen(QColor(QStringLiteral("#52525b")));
        painter.drawText(QRect(80, 210, 1000, 300), Qt::AlignLeft | Qt::TextWordWrap,
                         QStringLiteral("A runtime for writing reliable asynchronous applications with Rust. It provides I/O, networking, scheduling, timers, and more."));
        painter.setPen(QColor(QStringLiteral("#8b5cf6")));
        painter.drawText(QRect(80, 330, 600, 40), Qt::AlignLeft, QStringLiteral("Getting started   ·   Tutorial   ·   API docs"));
    }
    QByteArray bytes;
    QBuffer buffer(&bytes);
    buffer.open(QIODevice::WriteOnly);
    page.save(&buffer, "JPEG", 80);
    m_browser = QVariantMap{{QStringLiteral("url"), QStringLiteral("https://docs.rs/tokio")},
                            {QStringLiteral("title"), QStringLiteral("tokio - Rust")},
                            {QStringLiteral("imageUrl"), dataUrl(QString::fromLatin1(bytes.toBase64()))},
                            {QStringLiteral("width"), 1280},
                            {QStringLiteral("height"), 800},
                            {QStringLiteral("canGoBack"), true},
                            {QStringLiteral("canGoForward"), false},
                            {QStringLiteral("activeTab"), QStringLiteral("t1")},
                            {QStringLiteral("agentAt"), QDateTime::currentMSecsSinceEpoch() + 60000},
                            {QStringLiteral("tabs"), QVariantList{
                                QVariantMap{{QStringLiteral("id"), QStringLiteral("t1")}, {QStringLiteral("title"), QStringLiteral("tokio - Rust")}, {QStringLiteral("url"), QStringLiteral("https://docs.rs/tokio")}, {QStringLiteral("active"), true}},
                                QVariantMap{{QStringLiteral("id"), QStringLiteral("t2")}, {QStringLiteral("title"), QStringLiteral("tokio-rs/axum: Ergonomic web framework")}, {QStringLiteral("url"), QStringLiteral("https://github.com/tokio-rs/axum")}, {QStringLiteral("active"), false}},
                                QVariantMap{{QStringLiteral("id"), QStringLiteral("t3")}, {QStringLiteral("title"), QStringLiteral("localhost:5173")}, {QStringLiteral("url"), QStringLiteral("http://localhost:5173/")}, {QStringLiteral("active"), false}}}}};
    emit browserChanged();

    // A repository with stashes per ticket, an incoming commit and two to push.
    const auto commit = [now](const char *hash, const char *subject, const char *author, qint64 ago) {
        return QVariantMap{{QStringLiteral("hash"), s(hash)}, {QStringLiteral("short"), s(hash).left(7)}, {QStringLiteral("subject"), s(subject)},
                           {QStringLiteral("author"), s(author)}, {QStringLiteral("time"), now - ago}};
    };
    const auto stash = [now](int index, const char *message, const char *branch, qint64 ago) {
        return QVariantMap{{QStringLiteral("index"), index}, {QStringLiteral("name"), QStringLiteral("stash@{%1}").arg(index)},
                           {QStringLiteral("message"), s(message)}, {QStringLiteral("branch"), s(branch)}, {QStringLiteral("time"), now - ago}};
    };
    const auto change = [](const char *path, const char *code, bool untracked) {
        return QVariantMap{{QStringLiteral("path"), s(path)}, {QStringLiteral("code"), s(code)}, {QStringLiteral("staged"), false}, {QStringLiteral("untracked"), untracked}};
    };
    m_git = QVariantMap{
        {QStringLiteral("repo"), true}, {QStringLiteral("root"), s("C:/Code/acme-api")}, {QStringLiteral("branch"), s("feature/live-models")},
        {QStringLiteral("detached"), false}, {QStringLiteral("upstream"), s("origin/feature/live-models")},
        {QStringLiteral("ahead"), 2}, {QStringLiteral("behind"), 1}, {QStringLiteral("fetchedAt"), now - 240},
        {QStringLiteral("changes"), QVariantList{change("src/providers.rs", ".M", false), change("src/client.rs", ".M", false), change("tests/models.rs", "??", true)}},
        {QStringLiteral("stashes"), QVariantList{stash(0, "ACME-142 retry uploads on 503", "main", 3600), stash(1, "ACME-157 dark mode for settings", "main", 86400 * 2),
                                                 stash(2, "ACME-163 export CSV button", "feature/reports", 86400 * 5), stash(3, "spike: streaming JSON parser", "main", 86400 * 9)}},
        {QStringLiteral("incoming"), QVariantList{commit("9f2c1aa31b", "Bump axum to 0.8", "Sam Lee", 1800)}},
        {QStringLiteral("outgoing"), QVariantList{commit("4b7e90d2c1", "Query every provider for live models", "Alex Rivera", 2400),
                                                  commit("1c0d5e7f88", "Give each provider fetch its own timeout", "Alex Rivera", 5400)}},
        {QStringLiteral("history"), QVariantList{commit("4b7e90d2c1", "Query every provider for live models", "Alex Rivera", 2400),
                                                 commit("1c0d5e7f88", "Give each provider fetch its own timeout", "Alex Rivera", 5400),
                                                 commit("77aa21c903", "Merge pull request #48 from acme/uploads", "Sam Lee", 86400),
                                                 commit("e01f9b4d22", "Add rate limiting to uploads", "Alex Rivera", 86400 * 2)}},
        {QStringLiteral("identity"), QVariantMap{{QStringLiteral("name"), s("Alex Rivera")}, {QStringLiteral("email"), s("alex@example.com")},
                                                 {QStringLiteral("account"), s("work")}}}};
    m_gitAccounts = QVariantMap{
        {QStringLiteral("accounts"), QVariantList{
            QVariantMap{{QStringLiteral("id"), s("work")}, {QStringLiteral("label"), s("Work")}, {QStringLiteral("name"), s("Alex Rivera")},
                        {QStringLiteral("email"), s("alex@example.com")}, {QStringLiteral("username"), s("alex-acme")}},
            QVariantMap{{QStringLiteral("id"), s("personal")}, {QStringLiteral("label"), s("Personal")}, {QStringLiteral("name"), s("Alex R.")},
                        {QStringLiteral("email"), s("alex.r@example.net")}, {QStringLiteral("username"), s("alexr")}}}},
        {QStringLiteral("projectDefault"), s("work")}};
    emit gitChanged();
    emit gitAccountsChanged();
}

void IpcClient::focusDiff(const QString &path) {
    emit diffFocus(path);
    emit panelRequested(QStringLiteral("changes"));
}

void IpcClient::requestPanel(const QString &which) {
    emit panelRequested(which);
}

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
        m_bridgeError = QStringLiteral("Could not find serika.exe. Build the CLI with cargo first.");
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
    const QString beside = QDir(QCoreApplication::applicationDirPath()).filePath(QStringLiteral("serika.exe"));
    if (QFileInfo::exists(beside))
        return beside;
    // Running from a source checkout: use whichever cargo build is newest.
    QDir dir(QCoreApplication::applicationDirPath());
    for (int i = 0; i < 7; ++i) {
        const QFileInfo debug(dir.filePath(QStringLiteral("target/debug/serika.exe")));
        const QFileInfo release(dir.filePath(QStringLiteral("target/release/serika.exe")));
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
    const QString tag = m_tags.take(id);
    if (!obj.value(QStringLiteral("ok")).toBool()) {
        const QString error = obj.value(QStringLiteral("error")).toString();
        if (tag.startsWith(QLatin1String("auth.")))
            emit authFinished(false, error, false);
        else if (tag == QLatin1String("chat")) {
            setRunning(false);
            setStatus(error);
        } else if (tag == QLatin1String("browser")) {
            m_browserBusy = false;
            m_browserError = error;
            emit browserChanged();
        } else if (tag == QLatin1String("providers")) {
            m_providersLoading = false;
            emit providersChanged();
            setNotice(error);
        } else if (tag != QLatin1String("background") && tag != QLatin1String("account") && tag != QLatin1String("models") &&
                   tag != QLatin1String("browserPoll")) {
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
    } else if (tag == QLatin1String("i18n")) {
        applyDictionary(result);
    } else if (tag == QLatin1String("projects")) {
        if (!m_demo) {
            m_projects = result.value(QStringLiteral("projects")).toArray().toVariantList();
            emit projectsChanged();
        }
    } else if (tag == QLatin1String("sessions")) {
        if (!m_demo) {
            m_sessions = result.value(QStringLiteral("sessions")).toArray().toVariantList();
            emit sessionsChanged();
        }
    } else if (tag == QLatin1String("session")) {
        applySettings(result.value(QStringLiteral("settings")).toObject());
        m_turns.replaceAll(result.value(QStringLiteral("turns")).toArray().toVariantList());
        m_diffs = result.value(QStringLiteral("diffs")).toArray().toVariantList();
        m_activeSession = result.value(QStringLiteral("id")).toString();
        emit activeSessionChanged();
        emit diffsChanged();
        emit turnsReloaded();
        request(QStringLiteral("projects.list"), {}, QStringLiteral("projects"));
        request(QStringLiteral("sessions.list"), {}, QStringLiteral("sessions"));
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
        if (!m_browser.isEmpty()) {
            m_browser.clear();
            emit browserChanged();
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
    map.insert(QStringLiteral("imageUrl"), image.isEmpty() ? QString() : dataUrl(image));
    map.remove(QStringLiteral("image"));
    m_browser = map;
    m_browserBusy = false;
    m_browserError.clear();
    emit browserChanged();
}

void IpcClient::handleEvent(const QJsonObject &obj) {
    const QString event = obj.value(QStringLiteral("event")).toString();
    if (event == QLatin1String("started")) {
        m_activeSession = obj.value(QStringLiteral("sessionId")).toString();
        emit activeSessionChanged();
        m_roundsSeen = 0;
        setRunning(true);
        beginAssistantTurn();
    } else if (event == QLatin1String("status")) {
        const QString text = obj.value(QStringLiteral("text")).toString();
        if (text.startsWith(QLatin1String("turn "))) {
            ++m_roundsSeen;
            if (m_roundsSeen > 1) {
                int row = -1;
                if (Turn *turn = m_turns.lastAssistant(&row)) {
                    if (!turn->raw.trimmed().isEmpty())
                        turn->raw += QStringLiteral("\n\n");
                    if (!turn->reasoning.trimmed().isEmpty())
                        turn->reasoning += QStringLiteral("\n\n");
                    turn->content = TurnModel::stripToolFences(turn->raw);
                    m_turns.changed(row, {TurnModel::ContentRole, TurnModel::ReasoningRole});
                }
            }
        } else if (text.contains(QLatin1String("needed approval"))) {
            setNotice(text);
        }
    } else if (event == QLatin1String("chunk") || event == QLatin1String("reasoning")) {
        int row = -1;
        Turn *turn = m_turns.lastAssistant(&row);
        if (!turn)
            return;
        const QString text = obj.value(QStringLiteral("text")).toString();
        if (event == QLatin1String("chunk")) {
            turn->raw += text;
            turn->content = TurnModel::stripToolFences(turn->raw);
            m_turns.changed(row, {TurnModel::ContentRole});
        } else {
            turn->reasoning += text;
            m_turns.changed(row, {TurnModel::ReasoningRole});
        }
    } else if (event == QLatin1String("tool")) {
        updateTool(obj);
    } else if (event == QLatin1String("toolOutput")) {
        int row = -1;
        Turn *turn = m_turns.lastAssistant(&row);
        if (!turn)
            return;
        const QString id = obj.value(QStringLiteral("id")).toString();
        for (int i = 0; i < turn->tools.size(); ++i) {
            QVariantMap tool = turn->tools.at(i).toMap();
            if (tool.value(QStringLiteral("id")).toString() == id) {
                tool.insert(QStringLiteral("output"), obj.value(QStringLiteral("text")).toString());
                turn->tools[i] = tool;
                m_turns.changed(row, {TurnModel::ToolsRole});
                break;
            }
        }
    } else if (event == QLatin1String("approval")) {
        m_approval = obj.toVariantMap();
        emit approvalChanged();
    } else if (event == QLatin1String("diff")) {
        const QString path = obj.value(QStringLiteral("path")).toString();
        const QVariantMap item{{QStringLiteral("path"), path}, {QStringLiteral("diff"), obj.value(QStringLiteral("diff")).toString()}};
        bool replaced = false;
        for (QVariant &existing : m_diffs) {
            if (existing.toMap().value(QStringLiteral("path")).toString() == path) {
                existing = item;
                replaced = true;
                break;
            }
        }
        if (!replaced)
            m_diffs.append(item);
        emit diffsChanged();
        // Link the change to the edit tool that made it, so its card can open the diff.
        int row = -1;
        if (Turn *turn = m_turns.lastAssistant(&row)) {
            for (int i = int(turn->tools.size()) - 1; i >= 0; --i) {
                QVariantMap tool = turn->tools.at(i).toMap();
                if (isEditTool(tool.value(QStringLiteral("name")).toString()) && !tool.contains(QStringLiteral("diffPath"))) {
                    tool.insert(QStringLiteral("diffPath"), path);
                    turn->tools[i] = tool;
                    m_turns.changed(row, {TurnModel::ToolsRole});
                    break;
                }
            }
        }
    } else if (event == QLatin1String("browser")) {
        applyFrame(obj);
        emit browserActivity();
    } else if (event == QLatin1String("handoff")) {
        // Show where the chat moved to another model, just above the reply it is writing.
        Turn marker;
        marker.role = QStringLiteral("switch");
        marker.switchFrom = obj.value(QStringLiteral("from")).toString();
        marker.switchTo = obj.value(QStringLiteral("to")).toString();
        marker.content = QString::number(obj.value(QStringLiteral("messages")).toInt());
        int row = -1;
        Turn *reply = m_turns.lastAssistant(&row);
        if (reply && reply->streaming)
            m_turns.insert(row, marker);
        else
            m_turns.append(marker);
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
    m_turns.append(turn);
}

void IpcClient::setRunning(bool on) {
    if (!on) {
        int row = -1;
        if (Turn *turn = m_turns.lastAssistant(&row)) {
            if (turn->streaming) {
                turn->streaming = false;
                m_turns.changed(row, {TurnModel::StreamingRole});
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

void IpcClient::updateTool(const QJsonObject &obj) {
    int row = -1;
    Turn *turn = m_turns.lastAssistant(&row);
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
    m_turns.changed(row, {TurnModel::ToolsRole});
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
        m_turns.clear();
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
    const QList<Turn> &turns = m_turns.turns();
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
    m_turns.clear();
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
    Turn user;
    user.role = QStringLiteral("user");
    user.raw = trimmed;
    user.content = trimmed;
    user.attachments = files;
    m_turns.append(user);
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
    request(QStringLiteral("chat.send"), params, QStringLiteral("chat"));
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

void IpcClient::cancel() { request(QStringLiteral("chat.cancel"), {}, QStringLiteral("cancel")); }

// The position of a turn among the messages the user wrote: the user turn itself, or the
// prompt an assistant turn answered.
int IpcClient::userIndexFor(int row) const {
    const QList<Turn> &turns = m_turns.turns();
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
    const QList<Turn> &turns = m_turns.turns();
    for (int i = row + 1; i < turns.size(); ++i)
        if (turns[i].role == QLatin1String("user"))
            return true;
    return false;
}

void IpcClient::rewindTo(int row) {
    if (!canRewindTo(row))
        return;
    const bool onUser = row < m_turns.count() && m_turns.turns()[row].role == QLatin1String("user");
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
    if (id == m_activeSession && m_turns.count() > 0)
        return;
    request(QStringLiteral("sessions.open"), {{QStringLiteral("id"), id}}, QStringLiteral("session"));
}

void IpcClient::newChat() {
    m_activeSession.clear();
    m_turns.clear();
    m_diffs.clear();
    setStatus(QString());
    emit activeSessionChanged();
    emit diffsChanged();
}

void IpcClient::newChatIn(const QString &folder) {
    newChat();
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
    request(QStringLiteral("ui.set"), {{QStringLiteral("key"), key}, {QStringLiteral("value"), QJsonValue::fromVariant(value)}}, QStringLiteral("ui"));
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
    for (const Turn &turn : m_turns.turns()) {
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
    request(QStringLiteral("browser.open"), {{QStringLiteral("url"), url}}, QStringLiteral("browser"));
}

void IpcClient::browserBack() {
    m_browserBusy = true;
    emit browserChanged();
    request(QStringLiteral("browser.back"), {}, QStringLiteral("browser"));
}

void IpcClient::browserForward() {
    m_browserBusy = true;
    emit browserChanged();
    request(QStringLiteral("browser.forward"), {}, QStringLiteral("browser"));
}

void IpcClient::browserReload() {
    m_browserBusy = true;
    emit browserChanged();
    request(QStringLiteral("browser.reload"), {}, QStringLiteral("browser"));
}

void IpcClient::browserClick(double x, double y) {
    request(QStringLiteral("browser.click"), {{QStringLiteral("x"), x}, {QStringLiteral("y"), y}}, QStringLiteral("browser"));
}

void IpcClient::browserWheel(double x, double y, double dy) {
    request(QStringLiteral("browser.wheel"), {{QStringLiteral("x"), x}, {QStringLiteral("y"), y}, {QStringLiteral("dy"), dy}}, QStringLiteral("browser"));
}

void IpcClient::browserInput(const QString &key, const QString &text) {
    request(QStringLiteral("browser.input"), {{QStringLiteral("key"), key}, {QStringLiteral("text"), text}}, QStringLiteral("browser"));
}

void IpcClient::browserNewTab(const QString &url) {
    m_browserBusy = true;
    m_browserError.clear();
    emit browserChanged();
    request(QStringLiteral("browser.newTab"), {{QStringLiteral("url"), url}}, QStringLiteral("browser"));
}

void IpcClient::browserSwitchTab(const QString &id) {
    request(QStringLiteral("browser.switchTab"), {{QStringLiteral("id"), id}}, QStringLiteral("browser"));
}

void IpcClient::browserCloseTab(const QString &id) {
    request(QStringLiteral("browser.closeTab"), {{QStringLiteral("id"), id}}, QStringLiteral("browser"));
}

void IpcClient::browserResize(int width, int height) {
    if (m_demo)
        return;
    request(QStringLiteral("browser.resize"), {{QStringLiteral("width"), width}, {QStringLiteral("height"), height}}, QStringLiteral("browserPoll"));
}

void IpcClient::browserPoll() {
    if (m_demo || m_browserBusy)
        return;
    request(QStringLiteral("browser.frame"), {}, QStringLiteral("browserPoll"));
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
    m_username = QStringLiteral("pikachubolk");
    m_email = QStringLiteral("pikachubolk@example.com");
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
    m_sessions = {session("a", "Wire live /models discovery", 3600 * 2, "C:/Code/SerikaCode"),
                  session("b", "Yo, how's it going?", 3600 * 20, "C:/Code/SerikaCode"),
                  session("c", "can you check where tickets are handled?", 86400 * 3, "C:/Code/neoforce-app"),
                  session("d", "Build Pomodoro timer in React", 86400 * 12, "C:/Code/pomodoro"),
                  session("e", "OAuth 2.0 flow diagram", 86400 * 40, "C:/Code/neoforce-app")};
    emit sessionsChanged();
    m_projects = {QVariantMap{{QStringLiteral("path"), s("C:/Code/SerikaCode")}, {QStringLiteral("name"), s("SerikaCode")}, {QStringLiteral("exists"), true}, {QStringLiteral("sessionCount"), 2}, {QStringLiteral("current"), true}},
                  QVariantMap{{QStringLiteral("path"), s("C:/Code/neoforce-app")}, {QStringLiteral("name"), s("neoforce-app")}, {QStringLiteral("exists"), true}, {QStringLiteral("sessionCount"), 2}, {QStringLiteral("current"), false}},
                  QVariantMap{{QStringLiteral("path"), s("C:/Code/pomodoro")}, {QStringLiteral("name"), s("pomodoro")}, {QStringLiteral("exists"), true}, {QStringLiteral("sessionCount"), 1}, {QStringLiteral("current"), false}},
                  QVariantMap{{QStringLiteral("path"), s("C:/Code/old-prototype")}, {QStringLiteral("name"), s("old-prototype")}, {QStringLiteral("exists"), false}, {QStringLiteral("sessionCount"), 0}, {QStringLiteral("current"), false}}};
    emit projectsChanged();
    m_projectName = QStringLiteral("SerikaCode");
    m_projectPath = s("C:/Code/SerikaCode");
    emit settingsChanged();
    Q_UNUSED(here)

    QVariantList turns;
    turns.append(QVariantMap{{QStringLiteral("role"), QStringLiteral("user")},
                             {QStringLiteral("content"), QStringLiteral("can you check where tickets are handled and wire live /models discovery?")}});
    turns.append(QVariantMap{
        {QStringLiteral("role"), QStringLiteral("assistant")},
        {QStringLiteral("reasoning"),
         QStringLiteral("1. Searching the workspace for ticket handlers and model discovery entry points.\n"
                        "2. Inspected providers.rs and client.rs.\n"
                        "3. Verified the chat client streams reasoning and tool calls separately.\n"
                        "4. Ran the workspace tests to confirm everything passes.")},
        {QStringLiteral("tools"),
         QVariantList{QVariantMap{{QStringLiteral("name"), QStringLiteral("search_text")}, {QStringLiteral("state"), QStringLiteral("ok")}, {QStringLiteral("summary"), QStringLiteral("ticket")}, {QStringLiteral("result"), QStringLiteral("14 matches in 5 files")}, {QStringLiteral("output"), QStringLiteral("crates/serika-runtime/src/providers.rs:212: // tickets\ncrates/serika-runtime/src/client.rs:88: fn ticket_url")}},
                      QVariantMap{{QStringLiteral("name"), QStringLiteral("read_file")}, {QStringLiteral("state"), QStringLiteral("ok")}, {QStringLiteral("summary"), QStringLiteral("crates/serika-runtime/src/providers.rs")}, {QStringLiteral("result"), QStringLiteral("Read 142 lines")}},
                      QVariantMap{{QStringLiteral("name"), QStringLiteral("replace_in_file")}, {QStringLiteral("state"), QStringLiteral("ok")}, {QStringLiteral("summary"), QStringLiteral("crates/serika-runtime/src/providers.rs")}, {QStringLiteral("result"), QStringLiteral("Updated providers.rs (+28 -4)")}, {QStringLiteral("diffPath"), QStringLiteral("crates/serika-runtime/src/providers.rs")}},
                      QVariantMap{{QStringLiteral("name"), QStringLiteral("browser_open")}, {QStringLiteral("state"), QStringLiteral("ok")}, {QStringLiteral("summary"), QStringLiteral("https://docs.rs/tokio")}, {QStringLiteral("result"), QStringLiteral("Title: tokio - Rust")}},
                      QVariantMap{{QStringLiteral("name"), QStringLiteral("run_command")}, {QStringLiteral("state"), QStringLiteral("ok")}, {QStringLiteral("summary"), QStringLiteral("cargo test --workspace")}, {QStringLiteral("result"), QStringLiteral("test result: ok. 37 passed")}, {QStringLiteral("output"), QStringLiteral("running 37 tests\n...\ntest result: ok. 37 passed; 0 failed; finished in 1.74s")}},
                      QVariantMap{{QStringLiteral("name"), QStringLiteral("run_command")}, {QStringLiteral("state"), QStringLiteral("failed")}, {QStringLiteral("summary"), QStringLiteral("cargo clippy -- -D warnings")}, {QStringLiteral("result"), QStringLiteral("error: unused variable")}}}},
        {QStringLiteral("content"),
         QStringLiteral("### Ticket handling and live `/models` discovery\n\n"
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
    m_turns.replaceAll(turns);
    m_activeSession = QStringLiteral("a");
    emit activeSessionChanged();
    m_diffs = {QVariantMap{{QStringLiteral("path"), QStringLiteral("crates/serika-runtime/src/providers.rs")},
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
                                QVariantMap{{QStringLiteral("id"), QStringLiteral("t2")}, {QStringLiteral("title"), QStringLiteral("serika-dev/chat-web: SerikaChat web app")}, {QStringLiteral("url"), QStringLiteral("https://github.com/serika-dev/chat-web")}, {QStringLiteral("active"), false}},
                                QVariantMap{{QStringLiteral("id"), QStringLiteral("t3")}, {QStringLiteral("title"), QStringLiteral("localhost:5173")}, {QStringLiteral("url"), QStringLiteral("http://localhost:5173/")}, {QStringLiteral("active"), false}}}}};
    emit browserChanged();
}

void IpcClient::focusDiff(const QString &path) {
    emit diffFocus(path);
    emit panelRequested(QStringLiteral("changes"));
}

void IpcClient::requestPanel(const QString &which) {
    emit panelRequested(which);
}

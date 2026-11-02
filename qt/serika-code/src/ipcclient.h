#pragma once

#include <QHash>
#include <QJsonObject>
#include <QObject>
#include <QProcess>
#include <QTimer>
#include <QUrl>
#include <QVariantList>
#include <QVariantMap>

#include "turnmodel.h"

/// Bridge between the QML interface and `serika ipc`, the Rust agent runtime.
class IpcClient : public QObject {
    Q_OBJECT
    // Account
    Q_PROPERTY(bool signedIn READ signedIn NOTIFY accountChanged)
    Q_PROPERTY(QString username READ username NOTIFY accountChanged)
    Q_PROPERTY(QString email READ email NOTIFY accountChanged)
    Q_PROPERTY(QString plan READ plan NOTIFY accountChanged)
    Q_PROPERTY(bool isPremium READ isPremium NOTIFY accountChanged)
    Q_PROPERTY(QString periodEnd READ periodEnd NOTIFY accountChanged)
    Q_PROPERTY(QString avatarUrl READ avatarUrl NOTIFY accountChanged)
    Q_PROPERTY(double sessionPercent READ sessionPercent NOTIFY accountChanged)
    Q_PROPERTY(QString sessionResetsAt READ sessionResetsAt NOTIFY accountChanged)
    Q_PROPERTY(double weeklyPercent READ weeklyPercent NOTIFY accountChanged)
    Q_PROPERTY(QString weeklyResetsAt READ weeklyResetsAt NOTIFY accountChanged)
    Q_PROPERTY(double sessionUsed READ sessionUsed NOTIFY accountChanged)
    Q_PROPERTY(double sessionLimit READ sessionLimit NOTIFY accountChanged)
    Q_PROPERTY(double weeklyUsed READ weeklyUsed NOTIFY accountChanged)
    Q_PROPERTY(double weeklyLimit READ weeklyLimit NOTIFY accountChanged)
    Q_PROPERTY(bool accountLoaded READ accountLoaded NOTIFY accountChanged)
    // Login artwork
    Q_PROPERTY(QString artUrl READ artUrl NOTIFY artChanged)
    Q_PROPERTY(QString artArtist READ artArtist NOTIFY artChanged)
    // Models and providers
    Q_PROPERTY(QString selectedModel READ selectedModel NOTIFY modelsChanged)
    Q_PROPERTY(QString selectedModelName READ selectedModelName NOTIFY modelsChanged)
    Q_PROPERTY(QString selectedProvider READ selectedProvider NOTIFY modelsChanged)
    Q_PROPERTY(QVariantList freeModels READ freeModels NOTIFY modelsChanged)
    Q_PROPERTY(QVariantList premiumModels READ premiumModels NOTIFY modelsChanged)
    Q_PROPERTY(QVariantList providers READ providers NOTIFY providersChanged)
    Q_PROPERTY(bool providersLoading READ providersLoading NOTIFY providersChanged)
    // Settings and language
    Q_PROPERTY(QVariantMap settings READ settings NOTIFY settingsChanged)
    Q_PROPERTY(QVariantMap ui READ ui NOTIFY uiChanged)
    Q_PROPERTY(QString locale READ locale NOTIFY settingsChanged)
    Q_PROPERTY(QString theme READ theme NOTIFY settingsChanged)
    Q_PROPERTY(QString projectName READ projectName NOTIFY settingsChanged)
    Q_PROPERTY(QString projectPath READ projectPath NOTIFY settingsChanged)
    Q_PROPERTY(QString version READ version NOTIFY settingsChanged)
    Q_PROPERTY(QString permissionMode READ permissionMode NOTIFY settingsChanged)
    Q_PROPERTY(QVariantMap strings READ strings NOTIFY stringsChanged)
    Q_PROPERTY(QVariantList locales READ locales NOTIFY stringsChanged)
    Q_PROPERTY(QVariantMap updateInfo READ updateInfo NOTIFY updateInfoChanged)
    // Chat
    Q_PROPERTY(QVariantList projects READ projects NOTIFY projectsChanged)
    Q_PROPERTY(QVariantList sessions READ sessions NOTIFY sessionsChanged)
    Q_PROPERTY(QString activeSession READ activeSession NOTIFY activeSessionChanged)
    Q_PROPERTY(QObject *turns READ turns CONSTANT)
    Q_PROPERTY(QVariantList diffs READ diffs NOTIFY diffsChanged)
    Q_PROPERTY(QVariantList attachments READ attachments NOTIFY attachmentsChanged)
    Q_PROPERTY(bool running READ running NOTIFY runningChanged)
    Q_PROPERTY(QVariantList queue READ queue NOTIFY queueChanged)
    Q_PROPERTY(bool queuePaused READ queuePaused NOTIFY queueChanged)
    Q_PROPERTY(qint64 queueResumeAt READ queueResumeAt NOTIFY queueChanged)
    Q_PROPERTY(QString status READ status NOTIFY statusChanged)
    Q_PROPERTY(QString notice READ notice NOTIFY noticeChanged)
    Q_PROPERTY(QVariantMap approval READ approval NOTIFY approvalChanged)
    Q_PROPERTY(bool planMode READ planMode WRITE setPlanMode NOTIFY planModeChanged)
    Q_PROPERTY(QString bridgeError READ bridgeError NOTIFY bridgeErrorChanged)
    // Browser
    Q_PROPERTY(QVariantMap browser READ browser NOTIFY browserChanged)
    Q_PROPERTY(bool browserBusy READ browserBusy NOTIFY browserChanged)
    Q_PROPERTY(QString browserError READ browserError NOTIFY browserChanged)

public:
    explicit IpcClient(bool demo = false, QObject *parent = nullptr);
    ~IpcClient() override;

    bool signedIn() const { return m_signedIn; }
    QString username() const { return m_username; }
    QString email() const { return m_email; }
    QString plan() const { return m_plan; }
    bool isPremium() const { return m_isPremium; }
    QString periodEnd() const { return m_periodEnd; }
    QString avatarUrl() const { return m_avatarUrl; }
    double sessionPercent() const { return m_sessionPercent; }
    QString sessionResetsAt() const { return m_sessionResetsAt; }
    double weeklyPercent() const { return m_weeklyPercent; }
    double sessionUsed() const { return m_sessionUsed; }
    double sessionLimit() const { return m_sessionLimit; }
    double weeklyUsed() const { return m_weeklyUsed; }
    double weeklyLimit() const { return m_weeklyLimit; }
    QString weeklyResetsAt() const { return m_weeklyResetsAt; }
    bool accountLoaded() const { return m_accountLoaded; }
    QString artUrl() const { return m_artUrl; }
    QString artArtist() const { return m_artArtist; }
    QString selectedModel() const { return m_selectedModel; }
    QString selectedModelName() const;
    QString selectedProvider() const;
    QVariantList freeModels() const { return m_freeModels; }
    QVariantList premiumModels() const { return m_premiumModels; }
    QVariantList providers() const { return m_providers; }
    bool providersLoading() const { return m_providersLoading; }
    QVariantMap settings() const { return m_settings; }
    QVariantMap ui() const { return m_ui; }
    QString locale() const { return m_locale; }
    QString theme() const { return m_theme; }
    QString projectName() const { return m_projectName; }
    QString projectPath() const { return m_projectPath; }
    QString version() const { return m_version; }
    QString permissionMode() const;
    QVariantMap strings() const { return m_strings; }
    QVariantList locales() const { return m_locales; }
    QVariantMap updateInfo() const { return m_updateInfo; }
    QVariantList projects() const { return m_projects; }
    QVariantList sessions() const { return m_sessions; }
    QString activeSession() const { return m_activeSession; }
    QObject *turns() { return &m_turns; }
    QVariantList diffs() const { return m_diffs; }
    QVariantList attachments() const { return m_attachments; }
    QVariantList queue() const { return m_queue; }
    bool queuePaused() const { return m_queuePaused; }
    qint64 queueResumeAt() const { return m_queueResumeAt; }
    bool running() const { return m_running; }
    QString status() const { return m_status; }
    QString notice() const { return m_notice; }
    QVariantMap approval() const { return m_approval; }
    bool planMode() const { return m_planMode; }
    QString bridgeError() const { return m_bridgeError; }
    QVariantMap browser() const { return m_browser; }
    bool browserBusy() const { return m_browserBusy; }
    QString browserError() const { return m_browserError; }

    void setPlanMode(bool on);

    Q_INVOKABLE void login(const QString &email, const QString &password, const QString &twoFactor);
    Q_INVOKABLE void registerAccount(const QString &username, const QString &email, const QString &password, const QString &dateOfBirth);
    Q_INVOKABLE void requestMagicLink(const QString &email);
    Q_INVOKABLE void consumeMagicLink(const QString &token, const QString &twoFactor);
    Q_INVOKABLE void logout();
    Q_INVOKABLE void refresh();
    Q_INVOKABLE void send(const QString &prompt);
    Q_INVOKABLE void addAttachments(const QList<QUrl> &urls);
    Q_INVOKABLE bool pasteAttachments();
    Q_INVOKABLE void removeAttachment(int index);
    Q_INVOKABLE void clearAttachments();
    Q_INVOKABLE void cancel();
    Q_INVOKABLE void openSession(const QString &id);
    Q_INVOKABLE void newChat();
    Q_INVOKABLE void newChatIn(const QString &folder);
    Q_INVOKABLE void renameSession(const QString &id, const QString &title);
    Q_INVOKABLE void deleteSession(const QString &id);
    Q_INVOKABLE void chooseModel(const QString &id);
    Q_INVOKABLE void setLocale(const QString &code);
    Q_INVOKABLE void setTheme(const QString &name);
    Q_INVOKABLE void setSetting(const QString &key, const QVariant &value);
    Q_INVOKABLE void setUi(const QString &key, const QVariant &value);
    Q_INVOKABLE void addProject(const QUrl &folder);
    Q_INVOKABLE void removeProject(const QString &path);
    Q_INVOKABLE void refreshProviders(bool force = false);
    Q_INVOKABLE void probeClaudeUsage();
    Q_INVOKABLE void checkUpdates();
    Q_INVOKABLE void resolveApproval(bool allow);
    Q_INVOKABLE bool exportMarkdown(const QUrl &file);
    Q_INVOKABLE void copyText(const QString &text);
    Q_INVOKABLE void browserOpen(const QString &url);
    Q_INVOKABLE void browserBack();
    Q_INVOKABLE void browserForward();
    Q_INVOKABLE void browserReload();
    Q_INVOKABLE void browserClick(double x, double y);
    Q_INVOKABLE void browserWheel(double x, double y, double dy);
    Q_INVOKABLE void browserInput(const QString &key, const QString &text);
    Q_INVOKABLE void browserClose();
    // Turn actions. `row` is the turn's row in the thread.
    Q_INVOKABLE void rewindTo(int row);
    Q_INVOKABLE void editMessage(int row);
    Q_INVOKABLE void retryTurn(int row);
    Q_INVOKABLE void forkFrom(int row);
    Q_INVOKABLE void deleteTurn(int row);
    Q_INVOKABLE bool canRewindTo(int row) const;
    // Messages typed while a reply is running wait here and go out one by one.
    Q_INVOKABLE void removeQueued(int index);
    Q_INVOKABLE void moveQueued(int index, int delta);
    Q_INVOKABLE void editQueued(int index);
    Q_INVOKABLE void sendQueuedNow(int index);
    Q_INVOKABLE void resumeQueue();
    Q_INVOKABLE void resumeQueueAt(qint64 epochSecs);
    Q_INVOKABLE void clearQueue();
    Q_INVOKABLE QString modelName(const QString &id) const;
    Q_INVOKABLE QString lastReplyModel() const;
    Q_INVOKABLE void browserNewTab(const QString &url = QString());
    Q_INVOKABLE void browserSwitchTab(const QString &id);
    Q_INVOKABLE void browserCloseTab(const QString &id);
    Q_INVOKABLE void browserResize(int width, int height);
    Q_INVOKABLE void browserPoll();
    Q_INVOKABLE QString folderName(const QString &path) const;
    Q_INVOKABLE void focusDiff(const QString &path);
    Q_INVOKABLE void requestPanel(const QString &which);

signals:
    void accountChanged();
    void artChanged();
    void modelsChanged();
    void providersChanged();
    void settingsChanged();
    void uiChanged();
    void stringsChanged();
    void updateInfoChanged();
    void projectsChanged();
    void sessionsChanged();
    void activeSessionChanged();
    void diffsChanged();
    void attachmentsChanged();
    void runningChanged();
    void statusChanged();
    void noticeChanged();
    void approvalChanged();
    void planModeChanged();
    void bridgeErrorChanged();
    void browserChanged();
    void browserActivity();
    void queueChanged();
    // Put text back in the message box (edit a sent or queued message).
    void composerFill(const QString &text);
    void diffFocus(const QString &path);
    void panelRequested(const QString &which);
    void authFinished(bool ok, const QString &message, bool needsSecondFactor);
    void turnsReloaded();

private:
    void startProcess();
    void loadDemo();
    void request(const QString &method, const QJsonObject &params, const QString &tag);
    void onReadyRead();
    void handleLine(const QByteArray &line);
    void handleEvent(const QJsonObject &obj);
    void handleResponse(const QString &tag, const QJsonObject &result);
    void applyAccount(const QJsonObject &result);
    void applyModels(const QJsonObject &result);
    void applySettings(const QJsonObject &result);
    void applyDictionary(const QJsonObject &result);
    void applyFrame(const QJsonObject &frame);
    void beginAssistantTurn();
    void setRunning(bool on);
    void setStatus(const QString &text);
    void setNotice(const QString &text);
    void updateTool(const QJsonObject &obj);
    QString findSerika() const;

    QProcess m_process;
    QByteArray m_buffer;
    int m_nextId = 1;
    QHash<int, QString> m_tags;
    bool m_demo = false;

    bool m_signedIn = false;
    bool m_accountLoaded = false;
    QString m_username, m_email, m_plan, m_periodEnd, m_avatarUrl;
    QString m_sessionResetsAt, m_weeklyResetsAt;
    bool m_isPremium = false;
    double m_sessionPercent = 0;
    double m_weeklyPercent = 0;
    double m_sessionUsed = -1, m_sessionLimit = -1, m_weeklyUsed = -1, m_weeklyLimit = -1;
    QString m_artUrl, m_artArtist;

    QString m_selectedModel = QStringLiteral("openrouter/free");
    QVariantList m_freeModels, m_premiumModels, m_providers;
    bool m_providersLoading = false;

    QVariantMap m_settings, m_ui, m_updateInfo;
    QString m_locale = QStringLiteral("en");
    QString m_theme = QStringLiteral("dark");
    QString m_projectName, m_projectPath, m_version;
    QVariantMap m_strings;
    QVariantList m_locales;

    QVariantList m_projects;
    QVariantList m_sessions;
    QString m_activeSession;
    TurnModel m_turns;
    QVariantList m_diffs;
    QVariantList m_attachments;
    QVariantList m_queue;
    bool m_queuePaused = false;
    qint64 m_queueResumeAt = 0;
    QTimer m_queueTimer;
    bool m_sendNowPending = false;
    QString m_turnAction;
    void startSend(const QString &prompt, const QVariantList &attachments);
    void dispatchQueue();
    void pauseQueue();
    int userIndexFor(int row) const;
    void turnRequest(const QString &method, int row, int offset, const QString &action);
    bool m_running = false;
    bool m_planMode = false;
    int m_roundsSeen = 0;
    QString m_status, m_notice;
    QVariantMap m_approval;
    QString m_bridgeError;

    QVariantMap m_browser;
    bool m_browserBusy = false;
    QString m_browserError;
};

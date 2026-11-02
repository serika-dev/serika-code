import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

ColumnLayout {
    id: section
    spacing: 24
    readonly property var dict: ipc.strings
    function t(key, fallback, values) { return Format.tr(dict, key, fallback, values) }

    Component.onCompleted: ipc.refreshProviders()

    Text {
        Layout.fillWidth: true
        wrapMode: Text.WordWrap
        text: section.t("ProvidersIntro", "Chat with Serika's cloud models, or use the Claude Code or Codex subscription you already pay for. Your Serika account is always required to sign in. Claude Code and Codex run on your own login and limits, in the folder of your project.")
        color: Theme.textSecondary
        font.family: Theme.sans
        font.pixelSize: 14
        lineHeight: 1.2
    }

    // Serika (always on)
    SettingsCard {
        RowLayout {
            Layout.fillWidth: true
            Layout.margins: 16
            spacing: 14
            ProviderLogo { provider: "serika"; size: 40 }
            ColumnLayout {
                Layout.fillWidth: true
                spacing: 2
                RowLayout {
                    spacing: 8
                    Text { text: "Serika Cloud"; color: Theme.text; font.family: Theme.sans; font.pixelSize: 16; font.weight: Font.DemiBold }
                    Rectangle {
                        implicitWidth: sBadge.implicitWidth + 16
                        implicitHeight: 20
                        radius: 10
                        color: Theme.successSoft
                        Text { id: sBadge; anchors.centerIn: parent; text: section.t("Connected", "Connected"); color: Theme.success; font.family: Theme.sans; font.pixelSize: 11; font.weight: Font.Medium }
                    }
                }
                Text {
                    Layout.fillWidth: true
                    text: ipc.username + (ipc.plan.length > 0 ? " · " + ipc.plan : " · " + section.t("FreePlan", "Free"))
                    elide: Text.ElideRight
                    color: Theme.textDim
                    font.family: Theme.sans
                    font.pixelSize: 13
                }
            }
            Avatar { size: 36; source: ipc.avatarUrl; name: ipc.username }
        }
        Item {
            Layout.fillWidth: true
            implicitHeight: serikaMeters.implicitHeight + 28
            Rectangle { anchors.top: parent.top; anchors.left: parent.left; anchors.right: parent.right; anchors.leftMargin: 16; anchors.rightMargin: 16; height: 1; color: Theme.borderSubtle }
            ColumnLayout {
                id: serikaMeters
                anchors.left: parent.left
                anchors.right: parent.right
                anchors.verticalCenter: parent.verticalCenter
                anchors.leftMargin: 16
                anchors.rightMargin: 16
                spacing: 14
                UsageBar {
                    label: section.t("SessionLabel", "Session")
                    percent: ipc.sessionPercent
                    detail: [Format.tokens(ipc.sessionUsed, ipc.sessionLimit), ipc.sessionResetsAt.length > 0 ? Format.t(dict, "ResetsIn", { n: Format.countdown(ipc.sessionResetsAt) }) : ""].filter(function (x) { return x.length > 0 }).join("  ·  ")
                }
                UsageBar {
                    label: section.t("WeeklyLabel", "Weekly")
                    percent: ipc.weeklyPercent
                    detail: [Format.tokens(ipc.weeklyUsed, ipc.weeklyLimit), ipc.weeklyResetsAt.length > 0 ? Format.t(dict, "ResetsIn", { n: Format.countdown(ipc.weeklyResetsAt) }) : ""].filter(function (x) { return x.length > 0 }).join("  ·  ")
                }
            }
        }
    }

    Repeater {
        model: ipc.providers
        delegate: SettingsCard {
            id: card
            required property var modelData
            readonly property var p: modelData
            readonly property bool installed: p.installed === true
            readonly property bool signedIn: p.signed_in === true
            readonly property var usage: p.usage || ({})
            readonly property var windows: usage.windows || []
            readonly property bool isClaude: p.id === "claude"
            readonly property var limit: Format.providerLimit(p)
            property bool advanced: false

            // Header
            RowLayout {
                Layout.fillWidth: true
                Layout.margins: 16
                spacing: 14
                ProviderLogo { provider: card.p.id; size: 40 }
                ColumnLayout {
                    Layout.fillWidth: true
                    spacing: 2
                    RowLayout {
                        spacing: 8
                        Text { text: card.p.name; color: Theme.text; font.family: Theme.sans; font.pixelSize: 16; font.weight: Font.DemiBold }
                        Rectangle {
                            implicitWidth: badge.implicitWidth + 16
                            implicitHeight: 20
                            radius: 10
                            color: !card.installed ? Theme.active : (card.signedIn ? Theme.successSoft : Theme.warningSoft)
                            Text {
                                id: badge
                                anchors.centerIn: parent
                                text: !card.installed ? section.t("NotInstalledBadge", "Not installed")
                                      : (card.signedIn ? section.t("Connected", "Connected") : section.t("SignInNeeded", "Sign-in needed"))
                                color: !card.installed ? Theme.textSecondary : (card.signedIn ? Theme.success : Theme.warning)
                                font.family: Theme.sans
                                font.pixelSize: 11
                                font.weight: Font.Medium
                            }
                        }
                        Rectangle {
                            visible: card.limit !== null
                            implicitWidth: limRow.implicitWidth + 16
                            implicitHeight: 20
                            radius: 10
                            color: Qt.rgba(Theme.limit.r, Theme.limit.g, Theme.limit.b, 0.14)
                            Row {
                                id: limRow
                                anchors.centerIn: parent
                                spacing: 4
                                LimitBadge { limit: card.limit; size: 11; anchors.verticalCenter: parent.verticalCenter }
                                Text { text: section.t("LimitReached", "Limit reached"); color: Theme.limit; font.family: Theme.sans; font.pixelSize: 11; font.weight: Font.Medium }
                            }
                        }
                    }
                    Text {
                        Layout.fillWidth: true
                        text: card.p.detail || ""
                        elide: Text.ElideRight
                        color: Theme.textDim
                        font.family: Theme.sans
                        font.pixelSize: 13
                    }
                }
                IconButton {
                    icon: "refresh-cw"
                    iconSize: 15
                    tip: section.t("RefreshStatus", "Check again")
                    onClicked: ipc.refreshProviders(true)
                }
            }

            // Account and command
            SettingRow {
                visible: card.installed && card.signedIn
                heading: section.t("ProviderAccount", "Account")
                help: card.p.plan ? card.p.plan + " " + section.t("PlanWord", "plan") : ""
                Text {
                    text: card.p.account || section.t("Signed in", "Signed in")
                    color: Theme.text
                    font.family: Theme.sans
                    font.pixelSize: 14
                }
            }
            SettingRow {
                visible: card.installed
                heading: section.t("ProviderCommand", "Command")
                help: card.p.version || ""
                Text {
                    width: 280
                    text: card.p.binary || ""
                    elide: Text.ElideMiddle
                    horizontalAlignment: Text.AlignRight
                    color: Theme.textDim
                    font.family: Theme.mono
                    font.pixelSize: 11
                }
            }

            // Usage
            Item {
                visible: card.installed && card.signedIn
                Layout.fillWidth: true
                implicitHeight: usageCol.implicitHeight + 32
                Rectangle { anchors.top: parent.top; anchors.left: parent.left; anchors.right: parent.right; anchors.leftMargin: 16; anchors.rightMargin: 16; height: 1; color: Theme.borderSubtle }
                ColumnLayout {
                    id: usageCol
                    anchors.left: parent.left
                    anchors.right: parent.right
                    anchors.verticalCenter: parent.verticalCenter
                    anchors.leftMargin: 16
                    anchors.rightMargin: 16
                    spacing: 14
                    RowLayout {
                        Layout.fillWidth: true
                        Text {
                            text: section.t("SubscriptionUsage", "Subscription usage")
                            color: Theme.text
                            font.family: Theme.sans
                            font.pixelSize: 14
                            font.weight: Font.Medium
                        }
                        Item { Layout.fillWidth: true }
                        Text {
                            visible: card.windows.length > 0
                            text: section.t("UpdatedAgo", "Updated {t} ago", { t: Format.relTime(card.usage.updated_at) })
                            color: Theme.textDim
                            font.family: Theme.sans
                            font.pixelSize: 12
                        }
                    }
                    Rectangle {
                        visible: card.limit !== null && (card.usage.message || "").length > 0
                        Layout.fillWidth: true
                        implicitHeight: limText.implicitHeight + 20
                        radius: 10
                        color: Theme.dangerSoft
                        RowLayout {
                            anchors.fill: parent
                            anchors.margins: 10
                            spacing: 8
                            Icon { name: "alert-triangle"; size: 15; color: Theme.danger }
                            Text {
                                id: limText
                                Layout.fillWidth: true
                                wrapMode: Text.WordWrap
                                text: card.usage.message || ""
                                color: Theme.danger
                                font.family: Theme.sans
                                font.pixelSize: 13
                            }
                        }
                    }
                    Repeater {
                        model: card.windows
                        delegate: UsageBar {
                            required property var modelData
                            label: modelData.label
                            percent: modelData.used * 100
                            status: modelData.status
                            detail: modelData.resets_at ? section.t("ResetsInAt", "Resets in {n} ({time})", { n: Format.untilSeconds(modelData.resets_at), time: Format.clockTime(modelData.resets_at) }) : ""
                        }
                    }
                    Text {
                        visible: card.windows.length === 0
                        Layout.fillWidth: true
                        wrapMode: Text.WordWrap
                        text: card.isClaude ? section.t("ClaudeNoUsage", "Claude reports its limits while it works. Send a message, or read them now with a tiny request that uses a few tokens.")
                                            : section.t("CodexNoUsage", "Codex shows its limits on the ChatGPT usage page.")
                        color: Theme.textDim
                        font.family: Theme.sans
                        font.pixelSize: 13
                    }
                    RowLayout {
                        spacing: 8
                        ChipButton {
                            visible: card.isClaude
                            text: section.t("ReadLimits", "Read limits now")
                            iconName: "activity"
                            enabled: !ipc.providersLoading
                            onClicked: ipc.probeClaudeUsage()
                        }
                        ChipButton {
                            text: section.t("ManageUsage", "Manage usage")
                            iconName: "external-link"
                            onClicked: Qt.openUrlExternally(card.p.usage_url)
                        }
                    }
                }
            }

            // Set up / sign in
            Item {
                visible: !card.installed || !card.signedIn
                Layout.fillWidth: true
                implicitHeight: setupCol.implicitHeight + 32
                Rectangle { anchors.top: parent.top; anchors.left: parent.left; anchors.right: parent.right; anchors.leftMargin: 16; anchors.rightMargin: 16; height: 1; color: Theme.borderSubtle }
                ColumnLayout {
                    id: setupCol
                    anchors.left: parent.left
                    anchors.right: parent.right
                    anchors.verticalCenter: parent.verticalCenter
                    anchors.leftMargin: 16
                    anchors.rightMargin: 16
                    spacing: 10
                    Text {
                        Layout.fillWidth: true
                        wrapMode: Text.WordWrap
                        text: !card.installed ? section.t("StepInstall", "1. Install {name}, then sign in once.", { name: card.p.name })
                                              : section.t("StepSignIn", "Sign in once in a terminal. Serika uses that login.")
                        color: Theme.textSecondary
                        font.family: Theme.sans
                        font.pixelSize: 13
                    }
                    Rectangle {
                        Layout.fillWidth: true
                        implicitHeight: 38
                        radius: 8
                        color: Theme.code
                        border.width: 1
                        border.color: Theme.borderSubtle
                        RowLayout {
                            anchors.fill: parent
                            anchors.leftMargin: 12
                            anchors.rightMargin: 4
                            Text {
                                Layout.fillWidth: true
                                text: "$ " + (!card.installed ? card.p.install_hint : card.p.login_hint)
                                elide: Text.ElideRight
                                color: Theme.text
                                font.family: Theme.mono
                                font.pixelSize: 12
                            }
                            IconButton {
                                icon: "copy"
                                iconSize: 14
                                implicitWidth: 28
                                implicitHeight: 28
                                tip: section.t("Copy", "Copy")
                                onClicked: ipc.copyText(!card.installed ? card.p.install_hint : card.p.login_hint)
                            }
                        }
                    }
                    Text {
                        visible: !card.installed
                        Layout.fillWidth: true
                        wrapMode: Text.WordWrap
                        text: section.t("StepLogin", "2. Run {cmd} and finish signing in.", { cmd: "`" + card.p.login_hint + "`" })
                        color: Theme.textSecondary
                        font.family: Theme.sans
                        font.pixelSize: 13
                    }
                    RowLayout {
                        spacing: 8
                        ChipButton {
                            text: section.t("Docs", "Setup guide")
                            iconName: "external-link"
                            onClicked: Qt.openUrlExternally(card.p.docs_url)
                        }
                        ChipButton {
                            primary: true
                            text: section.t("RefreshStatus", "Check again")
                            iconName: "refresh-cw"
                            onClicked: ipc.refreshProviders(true)
                        }
                    }
                }
            }

            // Advanced
            Item {
                Layout.fillWidth: true
                implicitHeight: 44
                Rectangle { anchors.top: parent.top; anchors.left: parent.left; anchors.right: parent.right; anchors.leftMargin: 16; anchors.rightMargin: 16; height: 1; color: Theme.borderSubtle }
                Row {
                    anchors.verticalCenter: parent.verticalCenter
                    anchors.left: parent.left
                    anchors.leftMargin: 16
                    spacing: 8
                    Icon { anchors.verticalCenter: parent.verticalCenter; name: "chevron-right"; size: 14; color: Theme.textDim; rotation: card.advanced ? 90 : 0; Behavior on rotation { NumberAnimation { duration: 130 } } }
                    Text { anchors.verticalCenter: parent.verticalCenter; text: section.t("Advanced", "Advanced"); color: Theme.textSecondary; font.family: Theme.sans; font.pixelSize: 13 }
                }
                MouseArea { anchors.fill: parent; cursorShape: Qt.PointingHandCursor; onClicked: card.advanced = !card.advanced }
            }
            SettingRow {
                visible: card.advanced
                heading: section.t("BinaryPath", "Binary path")
                help: section.t("BinaryPathHelp", "Only needed when it is not on your PATH, for example with a version manager.")
                stacked: true
                PathField {
                    width: parent.width
                    mono: true
                    placeholder: card.isClaude ? "claude" : "codex"
                    value: card.isClaude ? (ipc.settings.claudePath || "") : (ipc.settings.codexPath || "")
                    onCommitted: (text) => { ipc.setSetting(card.isClaude ? "claude_path" : "codex_path", text); ipc.refreshProviders() }
                }
            }
            SettingRow {
                visible: card.advanced
                heading: card.isClaude ? "CLAUDE_CONFIG_DIR" : "CODEX_HOME"
                help: card.isClaude ? section.t("ClaudeDirHelp", "Use a separate folder to keep a second Claude account. Leave empty for your normal login.")
                                    : section.t("CodexHomeHelp", "Use a separate folder to keep a second Codex login. Leave empty for ~/.codex.")
                stacked: true
                PathField {
                    width: parent.width
                    mono: true
                    placeholder: card.isClaude ? "~/.claude_work" : "~/.codex_work"
                    value: card.isClaude ? (ipc.settings.claudeConfigDir || "") : (ipc.settings.codexHome || "")
                    onCommitted: (text) => { ipc.setSetting(card.isClaude ? "claude_config_dir" : "codex_home", text); ipc.refreshProviders() }
                }
            }
        }
    }
}

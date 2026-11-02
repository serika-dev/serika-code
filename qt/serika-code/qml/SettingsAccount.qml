import QtQuick
import QtQuick.Layouts

ColumnLayout {
    id: section
    spacing: 24
    readonly property var dict: ipc.strings
    function t(key, fallback) { return Format.tr(dict, key, fallback) }

    SettingsCard {
        RowLayout {
            Layout.fillWidth: true
            Layout.margins: 18
            spacing: 16
            Avatar { size: 56; source: ipc.avatarUrl; name: ipc.username }
            ColumnLayout {
                Layout.fillWidth: true
                spacing: 2
                Text { text: ipc.username; color: Theme.text; font.family: Theme.sans; font.pixelSize: 18; font.weight: Font.DemiBold }
                Text { text: ipc.email; color: Theme.textDim; font.family: Theme.sans; font.pixelSize: 13 }
            }
            ChipButton {
                text: section.t("ManageAccount", "Manage on serika.dev")
                iconName: "external-link"
                onClicked: Qt.openUrlExternally("https://serika.dev/options")
            }
        }
        SettingRow {
            heading: section.t("Subscription", "Subscription")
            help: {
                const end = Format.expiry(ipc.periodEnd)
                return end.length > 0 && ipc.isPremium ? section.t("RenewsOn", "Renews") + " " + end : ""
            }
            RowLayout {
                spacing: 10
                Text {
                    text: ipc.plan.length > 0 ? ipc.plan : section.t("FreePlan", "Free")
                    color: Theme.accentSecondary
                    font.family: Theme.sans
                    font.pixelSize: 14
                    font.weight: Font.DemiBold
                }
                ChipButton {
                    visible: !ipc.isPremium
                    primary: true
                    text: section.t("Upgrade", "Upgrade")
                    onClicked: Qt.openUrlExternally("https://serika.dev/premium")
                }
            }
        }
    }

    SettingsCard {
        title: section.t("UsageSection", "Serika usage")
        Item {
            Layout.fillWidth: true
            implicitHeight: meters.implicitHeight + 36
            ColumnLayout {
                id: meters
                anchors.left: parent.left
                anchors.right: parent.right
                anchors.verticalCenter: parent.verticalCenter
                anchors.leftMargin: 16
                anchors.rightMargin: 16
                spacing: 16
                UsageBar {
                    label: section.t("SessionLabel", "Session")
                    percent: ipc.sessionPercent
                    detail: [Format.tokens(ipc.sessionUsed, ipc.sessionLimit), ipc.sessionResetsAt.length > 0 ? Format.t(section.dict, "ResetsIn", { n: Format.countdown(ipc.sessionResetsAt) }) : ""].filter(function (x) { return x.length > 0 }).join("  ·  ")
                }
                UsageBar {
                    label: section.t("WeeklyLabel", "Weekly")
                    percent: ipc.weeklyPercent
                    detail: [Format.tokens(ipc.weeklyUsed, ipc.weeklyLimit), ipc.weeklyResetsAt.length > 0 ? Format.t(section.dict, "ResetsIn", { n: Format.countdown(ipc.weeklyResetsAt) }) : ""].filter(function (x) { return x.length > 0 }).join("  ·  ")
                }
            }
        }
    }

    SettingsCard {
        SettingRow {
            heading: section.t("SignOut", "Sign out")
            help: section.t("SignOutHelp", "Signs out of Serika on this computer. Claude Code and Codex keep their own logins, but Serika Code needs your Serika account to open.")
            ChipButton {
                text: section.t("SignOut", "Sign out")
                iconName: "log-out"
                danger: true
                onClicked: ipc.logout()
            }
        }
    }
}

import QtQuick
import QtQuick.Layouts

ColumnLayout {
    id: section
    spacing: 24
    readonly property var dict: ipc.strings
    function t(key, fallback) { return Format.tr(dict, key, fallback) }
    readonly property var update: ipc.updateInfo

    SettingsCard {
        RowLayout {
            Layout.fillWidth: true
            Layout.margins: 18
            spacing: 16
            Rectangle {
                Layout.preferredWidth: 52
                Layout.preferredHeight: 52
                radius: 15
                color: Theme.accent
                Text { anchors.centerIn: parent; text: "s"; color: "white"; font.family: Theme.brand; font.pixelSize: 28; font.weight: Font.Bold }
            }
            ColumnLayout {
                Layout.fillWidth: true
                spacing: 2
                Row {
                    spacing: 0
                    Text { text: "serika"; color: Theme.text; font.family: Theme.brand; font.pixelSize: 22; font.weight: Font.DemiBold }
                    Text { text: " code"; color: Theme.textDim; font.family: Theme.brand; font.pixelSize: 22 }
                }
                Text { text: section.t("Version", "Version") + " " + ipc.version; color: Theme.textDim; font.family: Theme.sans; font.pixelSize: 13 }
            }
        }
        SettingRow {
            heading: section.t("Updates", "Updates")
            help: {
                if (section.update.latest === undefined) return section.t("UpdatesHelp", "Check whether a newer version is available.")
                if (section.update.available) return section.t("UpdateAvailable", "Version {v} is available.").replace("{v}", section.update.latest)
                return section.t("UpToDate", "You are on the latest version.")
            }
            RowLayout {
                spacing: 8
                ChipButton {
                    visible: section.update.available === true
                    primary: true
                    text: section.t("Download", "Download")
                    onClicked: Qt.openUrlExternally(section.update.url)
                }
                ChipButton {
                    text: section.t("CheckUpdates", "Check for updates")
                    iconName: "refresh-cw"
                    onClicked: ipc.checkUpdates()
                }
            }
        }
    }

    SettingsCard {
        title: section.t("AboutRuntime", "This computer")
        SettingRow {
            heading: "Claude Code"
            help: {
                for (var i = 0; i < ipc.providers.length; ++i)
                    if (ipc.providers[i].id === "claude")
                        return ipc.providers[i].installed ? (ipc.providers[i].version || section.t("Installed", "Installed")) : section.t("NotInstalledBadge", "Not installed")
                return ""
            }
            Text { text: ""; }
        }
        SettingRow {
            heading: "Codex"
            help: {
                for (var i = 0; i < ipc.providers.length; ++i)
                    if (ipc.providers[i].id === "codex")
                        return ipc.providers[i].installed ? (ipc.providers[i].version || section.t("Installed", "Installed")) : section.t("NotInstalledBadge", "Not installed")
                return ""
            }
            Text { text: ""; }
        }
        SettingRow {
            heading: section.t("BrowserSection", "Agent browser")
            help: ipc.settings.browserDetected ? ipc.settings.browserDetected : section.t("BrowserMissing", "No browser found")
            Text { text: ""; }
        }
        SettingRow {
            heading: section.t("ProjectFolder", "Current project")
            help: (ipc.projectPath || "").replace(/^\\\\\?\\/, "")
            Text { text: ""; }
        }
    }

    SettingsCard {
        title: section.t("LinksSection", "Links")
        SettingRow {
            heading: "serika.dev"
            ChipButton { text: section.t("Open", "Open"); iconName: "external-link"; onClicked: Qt.openUrlExternally("https://serika.dev") }
        }
        SettingRow {
            heading: section.t("Terms", "Terms")
            ChipButton { text: section.t("Open", "Open"); iconName: "external-link"; onClicked: Qt.openUrlExternally("https://serika.dev/terms") }
        }
        SettingRow {
            heading: section.t("Privacy", "Privacy")
            ChipButton { text: section.t("Open", "Open"); iconName: "external-link"; onClicked: Qt.openUrlExternally("https://serika.dev/privacy") }
        }
    }

    Text {
        Layout.fillWidth: true
        Layout.leftMargin: 4
        wrapMode: Text.WordWrap
        text: section.t("BuiltWith", "Built with Qt 6 and a Rust agent runtime. Claude Code is a product of Anthropic; Codex is a product of OpenAI. Serika Code runs them on your own login and does not resell either.")
        color: Theme.textDim
        font.family: Theme.sans
        font.pixelSize: 12
    }
}

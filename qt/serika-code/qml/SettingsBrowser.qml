import QtQuick
import QtQuick.Layouts

ColumnLayout {
    id: section
    spacing: 24
    readonly property var dict: ipc.strings
    function t(key, fallback) { return Format.tr(dict, key, fallback) }
    readonly property string detected: ipc.settings.browserDetected || ""

    SettingsCard {
        title: section.t("BrowserSection", "Agent browser")
        description: section.t("BrowserSectionHelp", "Serika can open pages, read them, click, type, and scroll in a real browser, the same way you would. You can watch and take over in the Browser panel.")
        SettingRow {
            heading: section.t("BrowserEnable", "Let Serika use the browser")
            help: section.t("BrowserEnableHelp", "Applies to chats that use Serika models. Navigation is allowed in every permission mode; clicking and typing follow the permission mode.")
            Toggle {
                checked: ipc.settings.browserEnabled !== false
                onToggled: (v) => ipc.setSetting("browser_enabled", v)
            }
        }
        SettingRow {
            heading: section.t("BrowserHome", "Start page")
            help: section.t("BrowserHomeHelp", "Opened from the empty Browser panel.")
            stacked: true
            PathField {
                width: parent.width
                value: ipc.settings.browserHome || ""
                placeholder: "https://duckduckgo.com"
                onCommitted: (text) => ipc.setSetting("browser_home", text)
            }
        }
        SettingRow {
            heading: section.t("BrowserBinary", "Browser program")
            help: section.detected.length > 0
                  ? section.t("BrowserDetected", "Found:") + " " + section.detected + ". " + section.t("BrowserBinaryHelp", "Leave empty to use it. Takes effect the next time Serika Code starts.")
                  : section.t("BrowserMissingHelp", "Install Microsoft Edge or Google Chrome, or set a browser path in Settings → Browser.")
            stacked: true
            PathField {
                width: parent.width
                mono: true
                value: ipc.settings.browserPath || ""
                placeholder: "C:\\Program Files (x86)\\Microsoft\\Edge\\Application\\msedge.exe"
                onCommitted: (text) => ipc.setSetting("browser_path", text)
            }
        }
    }

    SettingsCard {
        title: section.t("BrowserPrivacy", "Privacy")
        SettingRow {
            heading: section.t("BrowserProfile", "Separate profile")
            help: section.t("BrowserProfileHelp", "The agent browser has its own cookies and history, apart from your everyday browser. Sites you sign in to stay signed in between sessions.")
            ChipButton {
                text: section.t("StopBrowser", "Close browser")
                iconName: "x"
                onClicked: ipc.browserClose()
            }
        }
        SettingRow {
            heading: section.t("BrowserOpenData", "Browser data")
            help: ipc.settings.dataDir ? ipc.settings.dataDir + "\\browser-profile" : ""
            ChipButton {
                text: section.t("OpenFolder", "Open folder")
                iconName: "folder"
                onClicked: Qt.openUrlExternally("file:///" + (ipc.settings.dataDir || "").replace(/\\/g, "/") + "/browser-profile")
            }
        }
    }
}

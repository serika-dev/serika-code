import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

// Opens above the account card in the sidebar (same entries as SerikaChat's AccountMenu).
Popover {
    id: menu
    signal settingsRequested(string tab)
    readonly property var dict: ipc.strings
    function t(key) { return Format.t(dict, key) }

    contentItem: ColumnLayout {
        spacing: 0
        MenuRow {
            icon: "sliders"
            label: menu.t("Settings")
            kbd: "Ctrl ,"
            onTriggered: { menu.close(); menu.settingsRequested("general") }
        }
        MenuDivider {}
        MenuRow {
            icon: "pie-chart"
            label: "Serika+ / " + menu.t("Usage")
            onTriggered: { menu.close(); menu.settingsRequested("account") }
        }
        MenuRow {
            icon: "user"
            label: "serika.dev " + menu.t("Account").toLowerCase()
            external: true
            onTriggered: { menu.close(); Qt.openUrlExternally("https://serika.dev/options") }
        }
        MenuDivider {}
        MenuRow {
            icon: "file-text"
            label: menu.t("Terms")
            external: true
            onTriggered: { menu.close(); Qt.openUrlExternally("https://serika.dev/terms") }
        }
        MenuRow {
            icon: "shield"
            label: menu.t("Privacy")
            external: true
            onTriggered: { menu.close(); Qt.openUrlExternally("https://serika.dev/privacy") }
        }
        MenuDivider {}
        MenuRow {
            icon: "log-out"
            label: menu.t("SignOut")
            danger: true
            onTriggered: { menu.close(); ipc.logout() }
        }
    }
}

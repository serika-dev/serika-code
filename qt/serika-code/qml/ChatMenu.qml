import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

// The `···` menu in the chat header.
Popover {
    id: menu
    signal exportRequested()
    signal inspectorRequested()
    signal shortcutsRequested()
    property bool inspectorOpen: false
    readonly property var dict: ipc.strings
    function t(key) { return Format.t(dict, key) }

    width: 256
    contentItem: ColumnLayout {
        spacing: 0
        MenuRow {
            icon: "code"
            label: menu.t("DiffsInspector")
            kbd: "Ctrl B"
            onTriggered: { menu.close(); menu.inspectorRequested() }
        }
        MenuRow {
            icon: "download"
            label: menu.t("ExportMarkdown")
            enabledRow: ipc.turns.count > 0
            onTriggered: { menu.close(); menu.exportRequested() }
        }
        MenuDivider {}
        MenuRow {
            icon: "command"
            label: menu.t("KeyboardShortcuts")
            kbd: "Ctrl /"
            onTriggered: { menu.close(); menu.shortcutsRequested() }
        }
    }
}

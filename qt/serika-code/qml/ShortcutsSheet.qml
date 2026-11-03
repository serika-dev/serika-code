import QtQuick
import QtQuick.Layouts

Sheet {
    id: sheet
    readonly property var dict: ipc.strings
    function t(key) { return Format.t(dict, key) }

    title: t("KeyboardShortcuts")
    maxWidth: 480

    function tr(key, fallback) { return Format.tr(dict, key, fallback) }
    readonly property var entries: [
        { label: t("NewChat"), keys: ["Ctrl", "N"] },
        { label: tr("CommandPalette", "Command palette"), keys: ["Ctrl", "K"] },
        { label: tr("SwitchChat", "Open chat 1 to 9 in the sidebar"), keys: ["Ctrl", "1–9"] },
        { label: tr("NextChat", "Next / previous chat"), keys: ["Ctrl", "Tab"] },
        { label: tr("SwitchModel", "Switch model"), keys: ["Ctrl", "Shift", "M"] },
        { label: t("ToggleInspector"), keys: ["Ctrl", "B"] },
        { label: tr("ShowBrowser", "Browser"), keys: ["Ctrl", "Shift", "B"] },
        { label: tr("ShowGit", "Git"), keys: ["Ctrl", "Shift", "G"] },
        { label: tr("ShowTerminal", "Terminal"), keys: ["Ctrl", "`"] },
        { label: tr("ShowSubagents", "Sub-agents"), keys: ["Ctrl", "Shift", "A"] },
        { label: tr("ToggleSidebar", "Sidebar"), keys: ["Ctrl", "Shift", "S"] },
        { label: t("Settings"), keys: ["Ctrl", ","] },
        { label: t("KeyboardShortcuts"), keys: ["Ctrl", "/"] },
        { label: t("SendMessage"), keys: ["Enter"] },
        { label: t("ShortcutClose"), keys: ["Esc"] }
    ]

    Item {
        width: parent.width
        height: list.implicitHeight + 28
        ColumnLayout {
            id: list
            anchors.left: parent.left
            anchors.right: parent.right
            anchors.top: parent.top
            anchors.margins: 20
            anchors.topMargin: 14
            spacing: 0
            Repeater {
                model: sheet.entries
                delegate: RowLayout {
                    required property var modelData
                    Layout.fillWidth: true
                    Layout.preferredHeight: 38
                    Text {
                        Layout.fillWidth: true
                        text: modelData.label
                        color: Theme.text
                        font.family: Theme.sans
                        font.pixelSize: 14
                    }
                    Row {
                        spacing: 4
                        Repeater {
                            model: modelData.keys
                            delegate: Kbd { required property string modelData; label: modelData }
                        }
                    }
                }
            }
        }
    }
}

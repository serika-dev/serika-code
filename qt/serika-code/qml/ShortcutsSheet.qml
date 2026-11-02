import QtQuick
import QtQuick.Layouts

Sheet {
    id: sheet
    readonly property var dict: ipc.strings
    function t(key) { return Format.t(dict, key) }

    title: t("KeyboardShortcuts")
    maxWidth: 440

    readonly property var entries: [
        { label: t("NewChat"), keys: ["Ctrl", "K"] },
        { label: t("ToggleInspector"), keys: ["Ctrl", "B"] },
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

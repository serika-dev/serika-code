import QtQuick
import QtQuick.Layouts

ColumnLayout {
    id: section
    spacing: 24
    readonly property var dict: ipc.strings
    function t(key, fallback) { return Format.tr(dict, key, fallback) }

    readonly property var groups: [
        { title: t("KeysChat", "Chat"), rows: [
            { label: t("KeySend", "Send message"), keys: [ipc.ui.sendKey === "ctrlenter" ? "Ctrl" : "Enter", ipc.ui.sendKey === "ctrlenter" ? "Enter" : ""] },
            { label: t("KeyNewline", "New line"), keys: [ipc.ui.sendKey === "ctrlenter" ? "Enter" : "Shift", ipc.ui.sendKey === "ctrlenter" ? "" : "Enter"] },
            { label: t("KeyNewChat", "New chat"), keys: ["Ctrl", "K"] },
            { label: t("KeyNewChat2", "New chat (alternative)"), keys: ["Ctrl", "N"] }
        ] },
        { title: t("KeysPanels", "Panels"), rows: [
            { label: t("KeySidebar", "Show or hide the sidebar"), keys: ["Ctrl", "Shift", "S"] },
            { label: t("KeyChanges", "Show or hide changes"), keys: ["Ctrl", "B"] },
            { label: t("KeyBrowser", "Show or hide the browser"), keys: ["Ctrl", "Shift", "B"] }
        ] },
        { title: t("KeysApp", "App"), rows: [
            { label: t("KeySettings", "Open settings"), keys: ["Ctrl", ","] },
            { label: t("KeyShortcuts", "Show keyboard shortcuts"), keys: ["Ctrl", "/"] },
            { label: t("KeyClose", "Close settings or a menu"), keys: ["Esc"] }
        ] }
    ]

    Repeater {
        model: section.groups
        delegate: SettingsCard {
            required property var modelData
            title: modelData.title
            Repeater {
                model: modelData.rows
                delegate: SettingRow {
                    required property var modelData
                    heading: modelData.label
                    Row {
                        spacing: 4
                        Repeater {
                            model: modelData.keys.filter(function (k) { return k.length > 0 })
                            delegate: Rectangle {
                                required property string modelData
                                implicitWidth: Math.max(26, keyText.implicitWidth + 14)
                                implicitHeight: 24
                                radius: 6
                                color: Theme.hover
                                border.width: 1
                                border.color: Theme.border
                                Text {
                                    id: keyText
                                    anchors.centerIn: parent
                                    text: modelData
                                    color: Theme.textSecondary
                                    font.family: Theme.sans
                                    font.pixelSize: 12
                                    font.weight: Font.Medium
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

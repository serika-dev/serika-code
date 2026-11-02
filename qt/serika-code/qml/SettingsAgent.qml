import QtQuick
import QtQuick.Layouts

ColumnLayout {
    id: section
    spacing: 24
    readonly property var dict: ipc.strings
    function t(key, fallback) { return Format.tr(dict, key, fallback) }

    readonly property var modeOptions: [
        { value: "restricted", label: t("ModeSupervised", "Supervised") },
        { value: "standard", label: t("ModeAutoEdits", "Auto-accept edits") },
        { value: "full", label: t("ModeFull", "Full access") },
        { value: "read_only", label: t("ModeReadOnly", "Read-only") }
    ]
    readonly property var timeoutOptions: [
        { value: "30", label: "30 seconds" }, { value: "60", label: "1 minute" }, { value: "120", label: "2 minutes" },
        { value: "300", label: "5 minutes" }, { value: "600", label: "10 minutes" }, { value: "1800", label: "30 minutes" }
    ]
    readonly property var roundOptions: [
        { value: "6", label: "6 steps" }, { value: "12", label: "12 steps" }, { value: "20", label: "20 steps" },
        { value: "30", label: "30 steps" }, { value: "40", label: "40 steps" }
    ]

    SettingsCard {
        title: section.t("PermissionModes", "Permission modes")
        description: section.t("PermissionModesHelp", "Choose when an agent needs your approval. Claude Code and Codex enforce these through their own sandboxes.")
        Repeater {
            model: section.modeOptions
            delegate: SettingRow {
                required property var modelData
                required property int index
                heading: modelData.label
                help: ["Asks before every edit and command. Approvals show up in the chat.",
                       "Edits and everyday dev commands run. Risky or destructive actions still ask.",
                       "Works without asking, except for destructive actions such as deleting files or force pushing.",
                       "Can read files and search, but cannot change anything."][index]
                Rectangle {
                    implicitWidth: 78
                    implicitHeight: 26
                    radius: 13
                    color: modelData.value === ipc.permissionMode ? Theme.accentSoft : Theme.hover
                    border.width: 1
                    border.color: modelData.value === ipc.permissionMode ? Theme.accent : Theme.borderSubtle
                    Text {
                        anchors.centerIn: parent
                        text: modelData.value === ipc.permissionMode ? section.t("Default", "Default") : section.t("SetDefault", "Make default")
                        color: modelData.value === ipc.permissionMode ? Theme.accentSecondary : Theme.textSecondary
                        font.family: Theme.sans
                        font.pixelSize: 12
                    }
                    MouseArea { anchors.fill: parent; cursorShape: Qt.PointingHandCursor; onClicked: ipc.setSetting("permission", modelData.value) }
                }
            }
        }
    }

    SettingsCard {
        title: section.t("SerikaAgent", "Serika agent limits")
        description: section.t("SerikaAgentHelp", "These apply to chats that use Serika models. Claude Code and Codex manage their own limits.")
        SettingRow {
            heading: section.t("CommandTimeout", "Command timeout")
            help: section.t("CommandTimeoutHelp", "A command that runs longer than this is stopped.")
            Select {
                options: section.timeoutOptions
                value: String(ipc.settings.commandTimeout || 120)
                implicitWidth: 180
                onPicked: (v) => ipc.setSetting("command_timeout_secs", v)
            }
        }
        SettingRow {
            heading: section.t("MaxRounds", "Steps per message")
            help: section.t("MaxRoundsHelp", "How many rounds of tool use Serika may take to answer one message.")
            Select {
                options: section.roundOptions
                value: String(ipc.settings.maxToolRounds || 12)
                implicitWidth: 180
                onPicked: (v) => ipc.setSetting("max_tool_rounds", v)
            }
        }
    }
}

import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

// Skills: SKILL.md folders from the project, Serika, Claude Code (and its plugins) and Codex.
// Serika models see each skill's name and description and load the full instructions on demand.
ColumnLayout {
    id: section
    spacing: 24
    readonly property var dict: ipc.strings
    function t(key, fallback) { return Format.tr(dict, key, fallback) }
    property string filter: ""
    readonly property var shown: ipc.skills.filter(function (s) {
        const q = section.filter.trim().toLowerCase()
        return q.length === 0 || s.name.toLowerCase().indexOf(q) >= 0 || (s.description || "").toLowerCase().indexOf(q) >= 0
    })
    Component.onCompleted: ipc.skillsRefresh()

    function sourceLabel(source) {
        const s = (source || "").toLowerCase()
        return s === "project" ? section.t("McpProject", "This project")
             : s === "serika" ? "Serika"
             : s.indexOf("claude") === 0 ? (s.indexOf("plugin") >= 0 ? section.t("SkillsClaudePlugin", "Claude Code plugin") : "Claude Code")
             : s === "codex" ? "Codex"
             : s === "agents" ? "~/.agents" : source
    }

    SettingsCard {
        title: section.t("SkillsTitle", "Skills")
        description: section.t("SkillsHelp", "Folders with a SKILL.md, found in the project (.serika, .claude, .agents, .codex), in Serika's data folder, and in your Claude Code and Codex skills. Serika uses a skill when the task matches its description.")
        SettingRow {
            heading: section.t("SkillsFound", "{n} skills found").replace("{n}", ipc.skills.length)
            help: section.t("SkillsAddHelp", "To add one, create skills/<name>/SKILL.md with a name and description in its front matter.")
            RowLayout {
                spacing: 8
                ChipButton {
                    text: section.t("OpenFolder", "Open folder")
                    iconName: "folder"
                    onClicked: Qt.openUrlExternally("file:///" + (ipc.settings.dataDir || "").replace(/\\/g, "/") + "/skills")
                }
                ChipButton {
                    text: section.t("Refresh", "Refresh")
                    iconName: "refresh-cw"
                    onClicked: ipc.skillsRefresh()
                }
            }
        }
        Field {
            visible: ipc.skills.length > 6
            Layout.fillWidth: true
            Layout.leftMargin: 12
            Layout.rightMargin: 12
            icon: "search"
            placeholder: section.t("SkillsSearch", "Search skills")
            onEdited: section.filter = text
        }
        Repeater {
            model: section.shown
            delegate: Rectangle {
                id: skillRow
                required property var modelData
                Layout.fillWidth: true
                implicitHeight: skillColumn.implicitHeight + 22
                radius: 10
                color: skillArea.containsMouse ? Theme.hover : "transparent"
                HoverHandler { id: skillArea }
                RowLayout {
                    anchors.fill: parent
                    anchors.leftMargin: 12
                    anchors.rightMargin: 8
                    spacing: 12
                    Rectangle {
                        Layout.preferredWidth: 32
                        Layout.preferredHeight: 32
                        Layout.alignment: Qt.AlignTop
                        Layout.topMargin: 11
                        radius: 9
                        color: Theme.accentSoft
                        Icon { anchors.centerIn: parent; name: "package"; size: 15; color: Theme.accentSecondary }
                    }
                    ColumnLayout {
                        id: skillColumn
                        Layout.fillWidth: true
                        spacing: 3
                        RowLayout {
                            spacing: 8
                            Text { text: skillRow.modelData.name; color: skillRow.modelData.enabled ? Theme.text : Theme.textDim; font.family: Theme.sans; font.pixelSize: 14; font.weight: Font.Medium }
                            Tag { text: section.sourceLabel(skillRow.modelData.source) }
                        }
                        Text {
                            Layout.fillWidth: true
                            text: skillRow.modelData.description || section.t("SkillsNoDescription", "No description")
                            wrapMode: Text.WordWrap
                            maximumLineCount: 3
                            elide: Text.ElideRight
                            color: Theme.textSecondary
                            font.family: Theme.sans
                            font.pixelSize: 12
                        }
                        Text {
                            Layout.fillWidth: true
                            text: skillRow.modelData.path
                            elide: Text.ElideMiddle
                            color: Theme.textDim
                            font.family: Theme.mono
                            font.pixelSize: 10
                        }
                    }
                    IconButton {
                        icon: "folder"
                        iconSize: 14
                        tip: section.t("OpenFolder", "Open folder")
                        onClicked: Qt.openUrlExternally("file:///" + skillRow.modelData.path.replace(/\\/g, "/"))
                    }
                    Toggle {
                        checked: skillRow.modelData.enabled
                        onToggled: (v) => ipc.skillsToggle(skillRow.modelData.name, v)
                    }
                }
            }
        }
        Text {
            visible: ipc.skills.length === 0
            Layout.leftMargin: 12
            Layout.bottomMargin: 8
            text: section.t("SkillsNone", "No skills yet.")
            color: Theme.textDim
            font.family: Theme.sans
            font.pixelSize: 13
        }
    }
}

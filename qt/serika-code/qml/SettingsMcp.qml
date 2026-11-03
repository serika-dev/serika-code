import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

// MCP servers: everything Serika found (its own list, the project's .mcp.json, Claude Code,
// Claude Desktop and Codex), whether each is connected, which protocol it speaks, and its tools.
ColumnLayout {
    id: section
    spacing: 24
    readonly property var dict: ipc.strings
    function t(key, fallback) { return Format.tr(dict, key, fallback) }
    readonly property var servers: ipc.mcp.servers || []
    property string expanded: ""
    property string editing: ""          // name of the Serika server being edited, or "+" for a new one
    Component.onCompleted: ipc.mcpRefresh()

    function sourceLabel(source) {
        return source === "serika" ? "Serika"
             : source === "project" ? section.t("McpProject", "This project")
             : source.indexOf("claude-code") === 0 ? "Claude Code"
             : source === "claude-desktop" ? "Claude Desktop"
             : source === "codex" ? "Codex" : source
    }
    function stateColor(s) {
        return s.state === "ready" ? "#22c55e" : (s.state === "error" ? Theme.danger : (s.state === "connecting" ? Theme.accent : Theme.textDim))
    }
    function stateLabel(s) {
        if (!s.enabled) return section.t("McpOff", "Off")
        if (s.state === "ready") return section.t("McpConnected", "Connected") + (s.tools ? " · " + s.tools.length + " " + section.t("McpTools", "tools") : "")
        if (s.state === "error") return section.t("McpFailed", "Failed")
        if (s.state === "connecting") return section.t("McpConnecting", "Connecting…")
        return section.t("McpIdle", "Connects on the next message")
    }

    SettingsCard {
        title: section.t("McpTitle", "MCP servers")
        description: section.t("McpHelp", "Tools from Model Context Protocol servers, available to Serika models in every chat. Servers from Claude Code, Claude Desktop, Codex and the project's .mcp.json are picked up automatically.")

        SettingRow {
            heading: section.t("McpProtocol", "Protocol")
            help: section.t("McpProtocolHelp", "Speaks the latest revision ({v}) and falls back to 2025-11-25, 2025-06-18, 2025-03-26 and 2024-11-05 for older servers, over stdio, Streamable HTTP or HTTP+SSE.").replace("{v}", ipc.mcp.latestVersion || "2026-07-28")
            ChipButton {
                text: section.t("Refresh", "Refresh")
                iconName: "refresh-cw"
                onClicked: ipc.mcpRefresh()
            }
        }
        SettingRow {
            heading: section.t("McpOwn", "Serika's browser for Claude Code and Codex")
            help: (ipc.mcp.ownEndpoint || "").length > 0
                  ? section.t("McpOwnHelp", "Claude Code and Codex chats get Serika's browser as MCP tools, served at") + " " + ipc.mcp.ownEndpoint
                  : section.t("McpOwnOff", "Not running. Restart Serika Code to start it.")
            Rectangle {
                width: 10
                height: 10
                radius: 5
                color: (ipc.mcp.ownEndpoint || "").length > 0 ? "#22c55e" : Theme.textDim
            }
        }
    }

    SettingsCard {
        title: section.t("McpServers", "Servers")
        description: section.servers.length === 0 ? section.t("McpNone", "No servers yet. Add one below, or configure one in Claude Code (claude mcp add) or Codex.") : ""

        Repeater {
            model: section.servers
            delegate: ColumnLayout {
                id: row
                required property var modelData
                readonly property bool open: section.expanded === modelData.name
                Layout.fillWidth: true
                spacing: 0
                Rectangle {
                    Layout.fillWidth: true
                    implicitHeight: 58
                    radius: 10
                    color: rowArea.containsMouse ? Theme.hover : "transparent"
                    MouseArea {
                        id: rowArea
                        anchors.fill: parent
                        hoverEnabled: true
                        cursorShape: Qt.PointingHandCursor
                        onClicked: section.expanded = row.open ? "" : row.modelData.name
                    }
                    RowLayout {
                        anchors.fill: parent
                        anchors.leftMargin: 12
                        anchors.rightMargin: 8
                        spacing: 12
                        Rectangle {
                            Layout.preferredWidth: 32
                            Layout.preferredHeight: 32
                            radius: 9
                            color: Theme.accentSoft
                            Icon { anchors.centerIn: parent; name: row.modelData.transport === "stdio" ? "terminal" : "server"; size: 15; color: Theme.accentSecondary }
                            Rectangle {
                                anchors.right: parent.right
                                anchors.bottom: parent.bottom
                                anchors.margins: -2
                                width: 10
                                height: 10
                                radius: 5
                                border.width: 2
                                border.color: Theme.card
                                color: row.modelData.enabled ? section.stateColor(row.modelData) : Theme.textDim
                            }
                        }
                        ColumnLayout {
                            Layout.fillWidth: true
                            spacing: 2
                            RowLayout {
                                spacing: 8
                                Text { text: row.modelData.name; color: Theme.text; font.family: Theme.sans; font.pixelSize: 14; font.weight: Font.Medium }
                                Tag { text: section.sourceLabel(row.modelData.source) }
                                Tag {
                                    accent: true
                                    visible: (row.modelData.protocol || "").length > 0
                                    text: (row.modelData.era === "legacy" ? section.t("McpLegacy", "legacy ") : "") + (row.modelData.protocol || "")
                                }
                            }
                            Text {
                                Layout.fillWidth: true
                                text: row.modelData.state === "error" && row.modelData.enabled ? (row.modelData.error || "") : section.stateLabel(row.modelData) + "  ·  " + row.modelData.target
                                elide: Text.ElideRight
                                color: row.modelData.state === "error" && row.modelData.enabled ? Theme.danger : Theme.textDim
                                font.family: Theme.sans
                                font.pixelSize: 12
                            }
                        }
                        IconButton {
                            visible: row.modelData.enabled
                            icon: "refresh-cw"
                            iconSize: 14
                            tip: section.t("McpReconnect", "Reconnect")
                            onClicked: ipc.mcpConnect(row.modelData.name)
                        }
                        Toggle {
                            checked: row.modelData.enabled
                            onToggled: (v) => ipc.mcpToggle(row.modelData.name, v)
                        }
                        Icon {
                            name: "chevron-down"
                            size: 14
                            color: Theme.textDim
                            rotation: row.open ? 180 : 0
                            Behavior on rotation { NumberAnimation { duration: Theme.motionFast } }
                        }
                    }
                }
                // Details: tools, definition, edit/remove for Serika's own servers
                ColumnLayout {
                    visible: row.open
                    Layout.fillWidth: true
                    Layout.leftMargin: 56
                    Layout.rightMargin: 12
                    Layout.bottomMargin: 12
                    spacing: 8
                    Text {
                        visible: (row.modelData.serverName || "").length > 0
                        text: section.t("McpServerName", "Reports itself as") + " " + row.modelData.serverName
                        color: Theme.textSecondary
                        font.family: Theme.sans
                        font.pixelSize: 12
                    }
                    Flow {
                        Layout.fillWidth: true
                        spacing: 6
                        Repeater {
                            model: row.modelData.tools || []
                            delegate: Rectangle {
                                required property var modelData
                                width: toolText.implicitWidth + 16
                                height: 24
                                radius: 7
                                color: Theme.hover
                                border.width: 1
                                border.color: Theme.borderSubtle
                                Text { id: toolText; anchors.centerIn: parent; text: modelData.name; color: Theme.textSecondary; font.family: Theme.mono; font.pixelSize: 11 }
                                HoverHandler { id: toolHover }
                                ToolTip.visible: toolHover.hovered && (modelData.description || "").length > 0
                                ToolTip.delay: 400
                                ToolTip.text: modelData.description || ""
                            }
                        }
                    }
                    Rectangle {
                        Layout.fillWidth: true
                        implicitHeight: defText.implicitHeight + 16
                        radius: 8
                        color: Theme.input
                        border.width: 1
                        border.color: Theme.borderSubtle
                        Text {
                            id: defText
                            anchors.fill: parent
                            anchors.margins: 8
                            text: JSON.stringify(row.modelData.definition, null, 2)
                            wrapMode: Text.WrapAnywhere
                            color: Theme.textSecondary
                            font.family: Theme.mono
                            font.pixelSize: 11
                        }
                    }
                    RowLayout {
                        spacing: 8
                        ChipButton {
                            visible: row.modelData.editable
                            text: section.t("Edit", "Edit")
                            iconName: "edit-2"
                            onClicked: { section.editing = row.modelData.name; editorName.text = row.modelData.name; editorBody.text = JSON.stringify(row.modelData.definition, null, 2) }
                        }
                        ChipButton {
                            visible: row.modelData.editable
                            text: section.t("Remove", "Remove")
                            iconName: "trash-2"
                            onClicked: ipc.mcpRemove(row.modelData.name)
                        }
                        Text {
                            visible: !row.modelData.editable
                            text: section.t("McpImported", "Imported. Change it where it is defined; you can still turn it off here.")
                            color: Theme.textDim
                            font.family: Theme.sans
                            font.pixelSize: 12
                        }
                    }
                }
            }
        }

        SettingRow {
            heading: section.t("McpAdd", "Add a server")
            help: section.t("McpAddHelp", "Same JSON as Claude Code's .mcp.json: a command with args and env, or a url with headers.")
            ChipButton {
                text: section.t("McpAddButton", "Add server")
                iconName: "plus"
                onClicked: {
                    section.editing = "+"
                    editorName.text = ""
                    editorBody.text = '{\n  "command": "npx",\n  "args": ["-y", "@modelcontextprotocol/server-filesystem", "."]\n}'
                }
            }
        }

        // Editor for Serika's own servers
        ColumnLayout {
            visible: section.editing.length > 0
            Layout.fillWidth: true
            Layout.leftMargin: 12
            Layout.rightMargin: 12
            Layout.bottomMargin: 12
            spacing: 8
            Field {
                id: editorName
                icon: 'server'
                Layout.fillWidth: true
                placeholder: section.t("McpName", "Name, e.g. filesystem")
                enabled: section.editing === "+"
            }
            Rectangle {
                Layout.fillWidth: true
                Layout.preferredHeight: 150
                radius: 10
                color: Theme.input
                border.width: 1
                border.color: editorBody.activeFocus ? Theme.accent : Theme.border
                ScrollView {
                    anchors.fill: parent
                    anchors.margins: 8
                    TextArea {
                        id: editorBody
                        background: null
                        color: Theme.text
                        font.family: Theme.mono
                        font.pixelSize: 12
                        selectByMouse: true
                        wrapMode: TextEdit.NoWrap
                    }
                }
            }
            RowLayout {
                spacing: 8
                AccentButton {
                    text: section.t("Save", "Save")
                    onClicked: {
                        const name = editorName.text.trim()
                        if (name.length === 0) return
                        ipc.mcpSave(name, editorBody.text)
                        section.editing = ""
                        section.expanded = name
                    }
                }
                GhostButton {
                    text: section.t("Cancel", "Cancel")
                    onClicked: section.editing = ""
                }
            }
        }
    }

    // Refresh while something is connecting so the state moves on by itself.
    Timer {
        interval: 1500
        repeat: true
        running: section.visible && section.servers.some(function (s) { return s.state === "connecting" })
        onTriggered: ipc.mcpRefresh()
    }
}

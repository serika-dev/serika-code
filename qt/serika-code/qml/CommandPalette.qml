import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

// Ctrl+K: jump to any chat or run any action by typing. ↑↓ to move, Enter to run, Esc to close.
Popup {
    id: cmdPalette
    signal actionTriggered(string id)
    readonly property var dict: ipc.strings
    property int selected: 0

    readonly property var actions: [
        { id: "new", label: Format.tr(dict, "NewChat", "New chat"), icon: "edit", keys: "Ctrl N" },
        { id: "model", label: Format.tr(dict, "SwitchModel", "Switch model"), icon: "cpu", keys: "Ctrl Shift M" },
        { id: "changes", label: Format.tr(dict, "ShowChanges", "Show changes"), icon: "git-branch", keys: "Ctrl B" },
        { id: "browser", label: Format.tr(dict, "ShowBrowser", "Show browser"), icon: "globe", keys: "Ctrl Shift B" },
        { id: "agents", label: Format.tr(dict, "ShowSubagents", "Show sub-agents"), icon: "users", keys: "Ctrl Shift A" },
        { id: "terminal", label: Format.tr(dict, "ShowTerminal", "Show terminal"), icon: "terminal", keys: "Ctrl `" },
        { id: "git", label: Format.tr(dict, "ShowGit", "Git: stashes, incoming, history"), icon: "git-commit", keys: "Ctrl Shift G" },
        { id: "mcp", label: Format.tr(dict, "ManageMcp", "MCP servers"), icon: "server", keys: "" },
        { id: "skills", label: Format.tr(dict, "ManageSkills", "Skills"), icon: "package", keys: "" },
        { id: "sidebar", label: Format.tr(dict, "ToggleSidebar", "Toggle sidebar"), icon: "sidebar", keys: "Ctrl Shift S" },
        { id: "project", label: Format.tr(dict, "AddProject", "Add project folder"), icon: "folder-plus", keys: "" },
        { id: "export", label: Format.tr(dict, "ExportMarkdown", "Export chat as Markdown"), icon: "download", keys: "" },
        { id: "settings", label: Format.tr(dict, "Settings", "Settings"), icon: "settings", keys: "Ctrl ," },
        { id: "appearance", label: Format.tr(dict, "Appearance", "Appearance and layout"), icon: "droplet", keys: "" },
        { id: "shortcuts", label: Format.tr(dict, "KeyboardShortcuts", "Keyboard shortcuts"), icon: "keyboard", keys: "Ctrl /" }
    ]
    readonly property var rows: {
        const q = query.text.trim().toLowerCase()
        const out = []
        const acts = actions.filter(function (a) { return !q || a.label.toLowerCase().indexOf(q) >= 0 })
        if (acts.length > 0) {
            out.push({ kind: "label", label: Format.tr(dict, "Actions", "Actions") })
            acts.forEach(function (a) { out.push({ kind: "action", id: a.id, label: a.label, icon: a.icon, keys: a.keys }) })
        }
        const chats = (ipc.sessions || [])
            .filter(function (s) { return !q || (s.title || "").toLowerCase().indexOf(q) >= 0 })
            .slice()
            .sort(function (a, b) { return b.updatedAt - a.updatedAt })
            .slice(0, q ? 30 : 8)
        if (chats.length > 0) {
            out.push({ kind: "label", label: q ? Format.tr(dict, "Chats", "Chats") : Format.tr(dict, "RecentChats", "Recent chats") })
            chats.forEach(function (s) {
                out.push({ kind: "chat", id: s.id, label: s.title || Format.tr(dict, "NewChat", "New chat"),
                           icon: "message", detail: Format.relTime(s.updatedAt) + " · " + ipc.folderName(s.project || "") })
            })
        }
        return out
    }
    readonly property var picks: rows.filter(function (r) { return r.kind !== "label" })
    function run(row) {
        if (!row) return
        close()
        if (row.kind === "chat") ipc.openSession(row.id)
        else cmdPalette.actionTriggered(row.id)
    }
    function move(step) {
        if (picks.length === 0) return
        selected = (selected + step + picks.length) % picks.length
        const target = picks[selected]
        for (let i = 0; i < rows.length; ++i) {
            if (rows[i] === target) { list.positionViewAtIndex(i, ListView.Contain); break }
        }
    }

    parent: Overlay.overlay
    modal: true
    focus: true
    x: (parent.width - width) / 2
    y: Math.max(48, parent.height * 0.16)
    width: Math.min(600, parent.width - 48)
    height: Math.min(460, 64 + list.contentHeight + 12)
    padding: 0
    closePolicy: Popup.CloseOnEscape | Popup.CloseOnPressOutside
    onOpened: { query.text = ""; selected = 0; query.forceActiveFocus() }

    Overlay.modal: Rectangle {
        color: Qt.rgba(0, 0, 0, 0.42)
        Behavior on opacity { NumberAnimation { duration: Theme.motionBase } }
    }
    background: Rectangle {
        radius: 16
        color: Theme.raised
        border.width: 1
        border.color: Theme.border
    }
    enter: Transition {
        NumberAnimation { property: "opacity"; from: 0; to: 1; duration: Theme.motionBase; easing.type: Theme.easeOut }
        NumberAnimation { property: "scale"; from: 0.96; to: 1; duration: Theme.motionBase; easing.type: Theme.easeEmphasis }
    }
    exit: Transition {
        NumberAnimation { property: "opacity"; to: 0; duration: Theme.motionFast }
        NumberAnimation { property: "scale"; to: 0.98; duration: Theme.motionFast }
    }

    contentItem: ColumnLayout {
        spacing: 0
        Item {
            Layout.fillWidth: true
            Layout.preferredHeight: 56
            Icon { anchors.left: parent.left; anchors.leftMargin: 18; anchors.verticalCenter: parent.verticalCenter; name: "search"; size: 18; color: Theme.textDim }
            TextField {
                id: query
                anchors.fill: parent
                anchors.leftMargin: 46
                anchors.rightMargin: 16
                background: null
                color: Theme.text
                placeholderText: Format.tr(cmdPalette.dict, "PaletteHint", "Search chats or type a command…")
                placeholderTextColor: Theme.textDim
                selectionColor: Theme.accent
                font.family: Theme.sans
                font.pixelSize: 16
                verticalAlignment: TextInput.AlignVCenter
                onTextChanged: cmdPalette.selected = 0
                Keys.onUpPressed: cmdPalette.move(-1)
                Keys.onDownPressed: cmdPalette.move(1)
                Keys.onTabPressed: cmdPalette.move(1)
                Keys.onReturnPressed: cmdPalette.run(cmdPalette.picks[cmdPalette.selected])
                Keys.onEnterPressed: cmdPalette.run(cmdPalette.picks[cmdPalette.selected])
            }
        }
        Rectangle { Layout.fillWidth: true; Layout.preferredHeight: 1; color: Theme.borderSubtle }
        ListView {
            id: list
            SmoothWheel { flickable: list }
            Layout.fillWidth: true
            Layout.fillHeight: true
            Layout.margins: 6
            clip: true
            model: cmdPalette.rows
            boundsBehavior: Flickable.StopAtBounds
            ScrollBar.vertical: ScrollBar { policy: ScrollBar.AsNeeded; width: 6 }
            delegate: Item {
                id: row
                required property var modelData
                readonly property bool isLabel: modelData.kind === "label"
                readonly property var pick: cmdPalette.picks[cmdPalette.selected]
                readonly property bool current: !isLabel && pick !== undefined && pick.kind === modelData.kind && pick.id === modelData.id
                width: ListView.view.width
                height: isLabel ? 28 : 40
                Text {
                    visible: row.isLabel
                    anchors.left: parent.left
                    anchors.leftMargin: 12
                    anchors.bottom: parent.bottom
                    anchors.bottomMargin: 4
                    text: row.modelData.label
                    color: Theme.textDim
                    font.family: Theme.sans
                    font.pixelSize: 11
                    font.weight: Font.Medium
                    font.capitalization: Font.AllUppercase
                    font.letterSpacing: 0.6
                }
                Rectangle {
                    visible: !row.isLabel
                    anchors.fill: parent
                    radius: 9
                    color: row.current ? Theme.active : (rowArea.containsMouse ? Theme.hover : "transparent")
                    Behavior on color { ColorAnimation { duration: Theme.motionFast } }
                    // Accent bar on the selected row
                    Rectangle {
                        anchors.left: parent.left
                        anchors.verticalCenter: parent.verticalCenter
                        width: 3
                        height: row.current ? 18 : 0
                        radius: 1.5
                        color: Theme.accent
                        Behavior on height { NumberAnimation { duration: Theme.motionFast; easing.type: Theme.easeOut } }
                    }
                    RowLayout {
                        anchors.fill: parent
                        anchors.leftMargin: 12
                        anchors.rightMargin: 12
                        spacing: 12
                        Icon {
                            name: row.modelData.kind === "chat" ? "message-square" : (row.modelData.icon || "command")
                            size: 16
                            color: row.current ? Theme.text : Theme.textSecondary
                        }
                        Text {
                            Layout.fillWidth: true
                            text: row.modelData.label || ""
                            elide: Text.ElideRight
                            color: Theme.text
                            font.family: Theme.sans
                            font.pixelSize: 14
                        }
                        Text {
                            visible: (row.modelData.detail || "").length > 0
                            text: row.modelData.detail || ""
                            color: Theme.textDim
                            font.family: Theme.sans
                            font.pixelSize: 12
                        }
                        Row {
                            visible: (row.modelData.keys || "").length > 0
                            spacing: 4
                            Repeater {
                                model: (row.modelData.keys || "").split(" ").filter(function (k) { return k.length > 0 })
                                delegate: Kbd { required property string modelData; label: modelData }
                            }
                        }
                    }
                    MouseArea {
                        id: rowArea
                        anchors.fill: parent
                        hoverEnabled: true
                        cursorShape: Qt.PointingHandCursor
                        onClicked: cmdPalette.run(row.modelData)
                    }
                }
            }
            Text {
                visible: cmdPalette.rows.length === 0
                anchors.centerIn: parent
                text: Format.tr(cmdPalette.dict, "NothingMatches", "Nothing matches “{q}”", { q: query.text })
                color: Theme.textDim
                font.family: Theme.sans
                font.pixelSize: 14
            }
        }
    }
}

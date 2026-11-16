import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

Item {
    id: header
    property bool browserShown: false
    property bool changesShown: false
    property bool agentsShown: false
    property bool terminalShown: false
    property bool gitShown: false
    signal gitRequested()
    signal agentsRequested()
    signal terminalRequested()
    readonly property bool inspectorOpen: browserShown || changesShown || agentsShown
    property real rightInset: 12
    signal exportRequested()
    signal inspectorRequested()
    signal browserRequested()
    signal providerSetupRequested(string provider)
    signal shortcutsRequested()
    readonly property var dict: ipc.strings

    implicitHeight: 52
    // A snapped chat column cannot hold the model name, the title and every panel button.
    readonly property bool toolsCollapsed: width < 640
    function openModels() { modelPopover.open() }

    // Limit of the provider behind the selected model, rechecked every 30 s so it clears at reset.
    property real now: Date.now()
    Timer { interval: 30000; running: true; repeat: true; onTriggered: header.now = Date.now() }
    readonly property var limit: {
        const provider = ipc.selectedProvider
        if (provider === "serika")
            return Format.serikaLimit(ipc.weeklyPercent, ipc.weeklyResetsAt, ipc.sessionPercent, ipc.sessionResetsAt)
        for (let i = 0; i < ipc.providers.length; ++i)
            if (ipc.providers[i].id === provider)
                return Format.providerLimit(ipc.providers[i], header.now)
        return null
    }
    function openChatMenu() { chatMenu.open() }

    // Model selector (frameless text button, as in SerikaChat)
    Rectangle {
        id: modelButton
        anchors.left: parent.left
        anchors.leftMargin: 12
        anchors.verticalCenter: parent.verticalCenter
        height: 36
        width: Math.min(modelRow.implicitWidth + 24, header.toolsCollapsed ? Math.max(108, header.width - header.rightInset - 132) : 320)
        radius: 8
        color: (modelArea.containsMouse || modelPopover.opened) ? Theme.hover : "transparent"

        RowLayout {
            id: modelRow
            anchors.fill: parent
            anchors.leftMargin: 10
            anchors.rightMargin: 8
            spacing: 6
            ProviderLogo {
                visible: ipc.selectedProvider !== "serika"
                provider: ipc.selectedProvider
                size: 18
            }
            Text {
                Layout.fillWidth: true
                text: ipc.selectedModelName
                elide: Text.ElideRight
                color: Theme.text
                font.family: Theme.sans
                font.pixelSize: 15
                font.weight: Font.Medium
            }
            LimitBadge { limit: header.limit; size: 15 }
            Icon {
                name: "chevron-down"
                size: 16
                color: Theme.textDim
                rotation: modelPopover.opened ? 180 : 0
                Behavior on rotation { NumberAnimation { duration: 150 } }
            }
        }
        MouseArea {
            id: modelArea
            anchors.fill: parent
            hoverEnabled: true
            cursorShape: Qt.PointingHandCursor
            onClicked: modelPopover.opened ? modelPopover.close() : modelPopover.open()
        }
        ModelPopover {
            id: modelPopover
            y: modelButton.height + 6
            onProviderSetupRequested: (provider) => header.providerSetupRequested(provider)
        }
    }

    // Chat title, centered between the controls
    Text {
        id: title
        readonly property real room: header.width - modelButton.width - header.rightInset - (header.toolsCollapsed ? 96 : 280)
        visible: ipc.activeSession.length > 0 && room > 72
        anchors.centerIn: parent
        width: Math.min(Math.max(0, room), 420)
        horizontalAlignment: Text.AlignHCenter
        elide: Text.ElideRight
        text: {
            for (var i = 0; i < ipc.sessions.length; ++i)
                if (ipc.sessions[i].id === ipc.activeSession) return ipc.sessions[i].title
            return ""
        }
        color: Theme.textSecondary
        font.family: Theme.sans
        font.pixelSize: 14
    }

    Row {
        anchors.right: parent.right
        anchors.rightMargin: header.rightInset
        anchors.verticalCenter: parent.verticalCenter
        spacing: 4

        // Sub-agents, once this chat has any
        IconButton {
            id: panelsButton
            visible: header.toolsCollapsed
            icon: "layout"
            tip: Format.tr(header.dict, "Panels", "Panels")
            active: panelsMenu.opened || header.inspectorOpen || header.gitShown || header.terminalShown
            onClicked: panelsMenu.opened ? panelsMenu.close() : panelsMenu.open()
            Rectangle {
                visible: ipc.agents.some(function (a) { return a.status === "running" })
                anchors.right: parent.right
                anchors.top: parent.top
                anchors.margins: 5
                width: 7
                height: 7
                radius: 3.5
                color: Theme.accent
            }
            Popover {
                id: panelsMenu
                x: panelsButton.width - width
                y: panelsButton.height + 6
                width: 220
                contentItem: ColumnLayout {
                    spacing: 0
                    MenuRow { icon: "git-commit"; label: Format.tr(header.dict, "Git", "Git"); kbd: "Ctrl Shift G"; onTriggered: { panelsMenu.close(); header.gitRequested() } }
                    MenuRow { icon: "terminal"; label: Format.tr(header.dict, "Terminal", "Terminal"); kbd: "Ctrl `"; onTriggered: { panelsMenu.close(); header.terminalRequested() } }
                    MenuRow { icon: "globe"; label: Format.tr(header.dict, "Browser", "Browser"); kbd: "Ctrl Shift B"; onTriggered: { panelsMenu.close(); header.browserRequested() } }
                    MenuRow { icon: "code"; label: Format.t(header.dict, "DiffsInspector"); kbd: "Ctrl B"; onTriggered: { panelsMenu.close(); header.inspectorRequested() } }
                    MenuRow {
                        visible: ipc.agents.length > 0
                        icon: "users"
                        label: Format.tr(header.dict, "Subagents", "Sub-agents")
                        kbd: "Ctrl Shift A"
                        onTriggered: { panelsMenu.close(); header.agentsRequested() }
                    }
                }
            }
        }
        IconButton {
            visible: !header.toolsCollapsed && ipc.agents.length > 0
            icon: "users"
            tip: Format.tr(header.dict, "Subagents", "Sub-agents") + "  (Ctrl Shift A)"
            active: header.agentsShown
            onClicked: header.agentsRequested()
            Rectangle {
                visible: ipc.agents.some(function (a) { return a.status === "running" })
                anchors.right: parent.right
                anchors.top: parent.top
                anchors.margins: 5
                width: 7
                height: 7
                radius: 3.5
                color: Theme.accent
            }
        }
        IconButton {
            visible: !header.toolsCollapsed
            icon: "git-commit"
            tip: Format.tr(header.dict, "Git", "Git") + "  (Ctrl Shift G)"
            active: header.gitShown
            onClicked: header.gitRequested()
            // Incoming commits waiting
            Rectangle {
                visible: (ipc.git.behind || 0) > 0
                anchors.right: parent.right
                anchors.top: parent.top
                anchors.margins: 5
                width: 7
                height: 7
                radius: 3.5
                color: Theme.accent
            }
        }
        IconButton {
            visible: !header.toolsCollapsed
            icon: "terminal"
            tip: Format.tr(header.dict, "Terminal", "Terminal") + "  (Ctrl `)"
            active: header.terminalShown
            onClicked: header.terminalRequested()
        }
        IconButton {
            visible: !header.toolsCollapsed
            icon: "globe"
            tip: Format.tr(header.dict, "Browser", "Browser") + "  (Ctrl Shift B)"
            active: header.browserShown
            onClicked: header.browserRequested()
        }
        IconButton {
            visible: !header.toolsCollapsed
            icon: "code"
            tip: Format.t(header.dict, "DiffsInspector") + "  (Ctrl B)"
            active: header.changesShown
            onClicked: header.inspectorRequested()
        }
        IconButton {
            id: moreButton
            icon: "more-horizontal"
            iconSize: 20
            tip: Format.t(header.dict, "ChatOptions")
            active: chatMenu.opened
            onClicked: chatMenu.opened ? chatMenu.close() : chatMenu.open()

            ChatMenu {
                id: chatMenu
                x: moreButton.width - width
                y: moreButton.height + 6
                inspectorOpen: header.inspectorOpen
                onExportRequested: header.exportRequested()
                onInspectorRequested: header.inspectorRequested()
                onShortcutsRequested: header.shortcutsRequested()
            }
        }
    }
}

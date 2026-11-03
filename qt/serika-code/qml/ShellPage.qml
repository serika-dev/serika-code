import QtQuick
import QtQuick.Controls
import QtQuick.Dialogs
import QtQuick.Layouts

// The workspace: sidebar, chat, and two docks (right and bottom) for changes, browser, sub-agents and terminal.
Item {
    id: shell
    property bool sidebarOpen: true
    property bool settingsOpen: false
    // Two docks like VS Code: one on the right, one at the bottom. Each holds panes as tabs; any
    // pane can sit in either, and the layout (tabs, sizes, which is open) is kept in ui.json.
    readonly property var paneIds: ["changes", "browser", "agents", "terminal"]
    function tabList(text) {
        return text.split(",").filter(function (t) { return paneIds.indexOf(t) >= 0 })
    }
    // Settings can rearrange these too, so they always come from ui.json. The lists hang off plain
    // strings, which only notify when they really change, so tab rows aren't rebuilt by unrelated settings.
    readonly property string rightKey: ipc.ui.rightTabs !== undefined ? String(ipc.ui.rightTabs)
                                     : (ipc.ui.dockTabs !== undefined ? String(ipc.ui.dockTabs) : "changes,browser")
    readonly property string bottomKey: ipc.ui.bottomTabs !== undefined ? String(ipc.ui.bottomTabs) : "terminal"
    readonly property var rightTabs: tabList(rightKey)
    readonly property var bottomTabs: tabList(bottomKey).filter(function (t) { return rightTabs.indexOf(t) < 0 })
    property string rightPick: ""
    property string bottomPick: ""
    readonly property string rightTab: rightTabs.indexOf(rightPick) >= 0 ? rightPick : (rightTabs.length > 0 ? rightTabs[0] : "")
    readonly property string bottomTab: bottomTabs.indexOf(bottomPick) >= 0 ? bottomPick : (bottomTabs.length > 0 ? bottomTabs[0] : "")
    property bool rightOpen: false
    property bool bottomOpen: ipc.ui.bottomOpen === true
    property string maximizedDock: ""            // "", "right" or "bottom"
    property real rightWidthLive: -1
    property real bottomHeightLive: -1
    readonly property real rightWidth: {
        if (rightWidthLive > 0) return rightWidthLive
        const saved = Number(ipc.ui.rightWidth || ipc.ui.dockWidth)
        return saved > 0 ? saved : 480
    }
    readonly property real bottomHeight: {
        if (bottomHeightLive > 0) return bottomHeightLive
        const saved = Number(ipc.ui.bottomHeight)
        return saved > 0 ? saved : 280
    }
    // Pane drag between docks
    property string dragPane: ""
    property point dragPos: Qt.point(0, 0)
    readonly property string dropDock: {
        if (dragPane.length === 0) return ""
        const p = dragPos
        if (rightDock.open && p.x >= rightDock.x) return "right"
        if (bottomDock.open && p.y >= bottomDock.y && p.x >= bottomDock.x && p.x <= bottomDock.x + bottomDock.width) return "bottom"
        if (p.x > shell.width - 240) return "right"
        if (p.y > shell.height - 180 && p.x > sidebar.width) return "bottom"
        return ""
    }
    readonly property bool hero: ipc.turns.count === 0 && !ipc.running
    readonly property var dict: ipc.strings
    function t(key, values) { return Format.t(dict, key, values) }
    function newChat() {
        ipc.newChat()
        Qt.callLater(composer.focusInput)
    }
    function openSettings(tab) {
        settingsPage.tab = tab
        shell.settingsOpen = true
    }
    function dockOf(id) {
        if (rightTabs.indexOf(id) >= 0) return "right"
        if (bottomTabs.indexOf(id) >= 0) return "bottom"
        return ""
    }
    function saveLayout(right, bottom) {
        ipc.setUi("rightTabs", right.join(","))
        ipc.setUi("bottomTabs", bottom.join(","))
    }
    function front(where, id) {
        if (where === "right") rightPick = id
        else bottomPick = id
    }
    function setOpen(where, value) {
        if (where === "right") rightOpen = value
        else { bottomOpen = value; ipc.setUi("bottomOpen", value) }
        if (!value && maximizedDock === where) maximizedDock = ""
    }
    function showPanel(id) {
        let where = dockOf(id)
        if (where === "") {
            where = id === "terminal" ? "bottom" : "right"
            if (where === "right") saveLayout(rightTabs.concat([id]), bottomTabs)
            else saveLayout(rightTabs, bottomTabs.concat([id]))
        }
        front(where, id)
        setOpen(where, true)
    }
    function panelShown(id) {
        const where = dockOf(id)
        if (where === "right") return rightOpen && rightTab === id
        if (where === "bottom") return bottomOpen && bottomTab === id
        return false
    }
    function togglePanel(id) {
        if (panelShown(id)) setOpen(dockOf(id), false)
        else showPanel(id)
    }
    function closeTab(id) {
        const where = dockOf(id)
        if (where === "") return
        const rest = (where === "right" ? rightTabs : bottomTabs).filter(function (t) { return t !== id })
        const wasFront = (where === "right" ? rightTab : bottomTab) === id
        if (where === "right") saveLayout(rest, bottomTabs)
        else saveLayout(rightTabs, rest)
        if (rest.length === 0) { setOpen(where, false); return }
        if (wasFront) front(where, rest[rest.length - 1])
    }
    // Move a pane to a dock (and a spot in its tabs), opening that dock with the pane in front.
    function movePane(id, target, index) {
        const from = dockOf(id)
        let right = rightTabs.filter(function (t) { return t !== id })
        let bottom = bottomTabs.filter(function (t) { return t !== id })
        const list = target === "right" ? right : bottom
        const at = index === undefined || index < 0 ? list.length : Math.min(index, list.length)
        list.splice(at, 0, id)
        saveLayout(right, bottom)
        if (from !== "" && from !== target && (from === "right" ? right : bottom).length === 0)
            setOpen(from, false)
        front(target, id)
        setOpen(target, true)
    }
    function finishDrag() {
        const target = dropDock
        const id = dragPane
        dragPane = ""
        if (id.length === 0 || target.length === 0) return
        const dock = target === "right" ? rightDock : bottomDock
        let index = dock.open ? dock.indexAt(dragPos) : -1
        // Dropping inside its own dock past itself shifts the target left by one.
        const own = target === "right" ? rightTabs : bottomTabs
        const was = own.indexOf(id)
        if (was >= 0 && index > was) index -= 1
        movePane(id, target, index)
    }
    function clampRight(size) {
        return Math.max(300, Math.min(size, Math.max(300, shell.width - sidebar.width - 380)))
    }
    function clampBottom(size) {
        return Math.max(140, Math.min(size, Math.max(140, shell.height - 260)))
    }
    Rectangle { anchors.fill: parent; color: Theme.bg }

    Sidebar {
        id: sidebar
        anchors.left: parent.left
        anchors.top: parent.top
        anchors.bottom: parent.bottom
        open: shell.sidebarOpen
        expandedWidth: Number(ipc.ui.sidebarWidth) >= 220 ? Math.min(Number(ipc.ui.sidebarWidth), 440) : 272
        onToggleRequested: shell.sidebarOpen = !shell.sidebarOpen
        onSettingsRequested: (tab) => shell.openSettings(tab)
        onAddProjectRequested: folderDialog.open()
        onNewChatRequested: shell.newChat()
        onNewChatInRequested: (path) => { ipc.newChatIn(path); Qt.callLater(composer.focusInput) }
    }

    // Drag the sidebar edge to resize it
    MouseArea {
        z: 20
        visible: shell.sidebarOpen
        x: sidebar.width - 3
        width: 6
        anchors.top: parent.top
        anchors.bottom: parent.bottom
        cursorShape: Qt.SizeHorCursor
        hoverEnabled: true
        property real startX: 0
        property real startWidth: 0
        onPressed: (mouse) => { startX = mapToItem(shell, mouse.x, 0).x; startWidth = sidebar.expandedWidth; sidebar.resizing = true }
        onPositionChanged: (mouse) => {
            if (!pressed) return
            const next = Math.max(220, Math.min(440, startWidth + mapToItem(shell, mouse.x, 0).x - startX))
            sidebar.expandedWidth = next
        }
        onReleased: { sidebar.resizing = false; ipc.setUi("sidebarWidth", Math.round(sidebar.expandedWidth)) }
        onDoubleClicked: { sidebar.resizing = false; ipc.setUi("sidebarWidth", 272) }
    }

    Item {
        id: main
        anchors.left: sidebar.right
        anchors.right: shell.rightOpen ? rightDock.left : parent.right
        anchors.rightMargin: shell.rightOpen ? 4 : 0
        anchors.top: parent.top
        anchors.bottom: shell.bottomOpen ? bottomDock.top : parent.bottom
        anchors.bottomMargin: shell.bottomOpen ? 4 : 0
        visible: shell.maximizedDock === ""

        ChatHeader {
            id: header
            anchors.top: parent.top
            anchors.left: parent.left
            anchors.right: parent.right
            rightInset: shell.rightOpen ? 12 : 146
            browserShown: shell.panelShown("browser")
            changesShown: shell.panelShown("changes")
            agentsShown: shell.panelShown("agents")
            terminalShown: shell.panelShown("terminal")
            onAgentsRequested: shell.togglePanel("agents")
            onTerminalRequested: shell.togglePanel("terminal")
            onInspectorRequested: shell.togglePanel("changes")
            onBrowserRequested: shell.togglePanel("browser")
            onProviderSetupRequested: shell.openSettings("providers")
            onExportRequested: exportDialog.open()
            onShortcutsRequested: shortcuts.open()
        }

        ThreadView {
            id: thread
            anchors.top: header.bottom
            anchors.left: parent.left
            anchors.right: parent.right
            anchors.bottom: composer.top
            anchors.bottomMargin: 8
            opacity: shell.hero ? 0 : 1
            visible: opacity > 0
            Behavior on opacity { NumberAnimation { duration: 160 } }
        }

        // Greeting and starters (empty chat)
        Item {
            id: heroBlock
            anchors.fill: parent
            opacity: shell.hero ? 1 : 0
            visible: opacity > 0
            Behavior on opacity { NumberAnimation { duration: 160 } }

            Text {
                id: greeting
                anchors.horizontalCenter: parent.horizontalCenter
                y: composer.y - height - 28
                text: shell.t(Format.greetingKey(new Date().getHours()), { name: ipc.username })
                color: Theme.text
                font.family: Theme.sans
                font.pixelSize: 30
                font.weight: Font.DemiBold
                font.letterSpacing: -0.5
            }

            ColumnLayout {
                id: starters
                x: (parent.width - width) / 2
                y: composer.y + composer.height + 52
                width: composer.width
                spacing: 0
                Repeater {
                    model: [
                        { title: "StarterTimerTitle", sub: "StarterTimerSub", prompt: "StarterTimerPrompt" },
                        { title: "StarterOauthTitle", sub: "StarterOauthSub", prompt: "StarterOauthPrompt" },
                        { title: "StarterReadmeTitle", sub: "StarterReadmeSub", prompt: "StarterReadmePrompt" },
                        { title: "StarterExplainTitle", sub: "StarterExplainSub", prompt: "StarterExplainPrompt" }
                    ]
                    delegate: Item {
                        required property var modelData
                        required property int index
                        Layout.fillWidth: true
                        Layout.preferredHeight: 46
                        Rectangle {
                            visible: index > 0
                            anchors.top: parent.top
                            width: parent.width
                            height: 1
                            color: Theme.borderSubtle
                        }
                        Row {
                            anchors.verticalCenter: parent.verticalCenter
                            anchors.left: parent.left
                            anchors.leftMargin: 8
                            spacing: 8
                            Text {
                                text: shell.t(parent.parent.modelData.title)
                                color: starterArea.containsMouse ? Theme.text : Theme.textSecondary
                                font.family: Theme.sans
                                font.pixelSize: 14
                                font.weight: Font.Medium
                            }
                            Text {
                                text: shell.t(parent.parent.modelData.sub)
                                color: Theme.textDim
                                font.family: Theme.sans
                                font.pixelSize: 14
                            }
                        }
                        MouseArea {
                            id: starterArea
                            anchors.fill: parent
                            hoverEnabled: true
                            cursorShape: Qt.PointingHandCursor
                            onClicked: ipc.send(shell.t(parent.modelData.prompt))
                        }
                    }
                }
            }
        }

        // Error line above the composer
        Rectangle {
            id: errorBar
            visible: ipc.status.length > 0
            anchors.horizontalCenter: parent.horizontalCenter
            anchors.bottom: composer.top
            anchors.bottomMargin: 10
            width: composer.width
            height: errorText.implicitHeight + 20
            radius: 12
            color: Theme.dangerSoft
            border.width: 1
            border.color: "#33f43f5e"
            RowLayout {
                anchors.fill: parent
                anchors.leftMargin: 12
                anchors.rightMargin: 6
                spacing: 8
                Icon { name: "alert-circle"; size: 16; color: "#f43f5e" }
                Text {
                    id: errorText
                    Layout.fillWidth: true
                    text: ipc.status
                    wrapMode: Text.WordWrap
                    color: Theme.danger
                    font.family: Theme.sans
                    font.pixelSize: 13
                }
            }
        }

        Composer {
            id: composer
            width: Math.min(parent.width - 48, Theme.laneMax)
            x: (parent.width - width) / 2
            readonly property real heroY: Math.max(header.height + 150, parent.height * 0.34)
            y: shell.hero ? heroY : parent.height - height - 44
            Behavior on y { NumberAnimation { duration: 260; easing.type: Easing.OutCubic } }
            onFolderRequested: folderDialog.open()
            onBrowserRequested: shell.showPanel("browser")
        }

        Text {
            anchors.horizontalCenter: parent.horizontalCenter
            anchors.top: composer.bottom
            anchors.topMargin: 10
            text: shell.t("Disclaimer")
            color: Theme.textDim
            font.family: Theme.sans
            font.pixelSize: 12
        }
    }

    // Right dock: full height under the title row, resized from its left edge.
    Inspector {
        id: rightDock
        where: "right"
        readonly property bool full: shell.maximizedDock === "right"
        readonly property real gap: 8
        readonly property real titleRow: 52
        x: full ? sidebar.width + gap : shell.width - width - gap
        y: titleRow
        width: full ? shell.width - sidebar.width - 2 * gap : shell.clampRight(shell.rightWidth)
        height: shell.height - titleRow - gap
        z: full ? 3 : 1
        open: shell.rightOpen && (shell.maximizedDock === "" || full)
        tabs: shell.rightTabs
        tab: shell.rightTab
        maximized: full
        dragging: shell.dragPane
        dropTarget: shell.dropDock === "right"
        onTabRequested: (id) => shell.dockOf(id) === "right" ? shell.front("right", id) : shell.movePane(id, "right")
        onTabClosed: (id) => shell.closeTab(id)
        onMoveRequested: (id, target) => shell.movePane(id, target)
        onCloseRequested: shell.setOpen("right", false)
        onMaximizeRequested: shell.maximizedDock = full ? "" : "right"
        onDragStarted: (id) => shell.dragPane = id
        onDragMoved: (p) => shell.dragPos = p
        onDragFinished: shell.finishDrag()

        MouseArea {
            z: 10
            visible: !rightDock.full
            x: -7
            width: 8
            height: parent.height
            cursorShape: Qt.SizeHorCursor
            hoverEnabled: true
            Rectangle {
                anchors.centerIn: parent
                width: 4
                height: 44
                radius: 2
                color: parent.pressed ? Theme.accent : Theme.textDim
                opacity: parent.pressed ? 1 : (parent.containsMouse ? 0.8 : 0)
                Behavior on opacity { NumberAnimation { duration: Theme.motionFast } }
            }
            property real startX: 0
            property real startSize: 0
            onPressed: (mouse) => { startX = mapToItem(shell, mouse.x, 0).x; startSize = shell.clampRight(shell.rightWidth) }
            onPositionChanged: (mouse) => {
                if (pressed) shell.rightWidthLive = shell.clampRight(startSize + startX - mapToItem(shell, mouse.x, 0).x)
            }
            onReleased: if (shell.rightWidthLive > 0) ipc.setUi("rightWidth", Math.round(shell.rightWidthLive))
            onDoubleClicked: { shell.rightWidthLive = -1; ipc.setUi("rightWidth", 0) }
        }
    }

    // Bottom dock: under the chat, between the sidebar and the right dock, resized from its top edge.
    Inspector {
        id: bottomDock
        where: "bottom"
        readonly property bool full: shell.maximizedDock === "bottom"
        readonly property real gap: 8
        x: sidebar.width + gap
        y: full ? 52 : shell.height - height - gap
        width: (shell.rightOpen && !full ? rightDock.x - 4 : shell.width - gap) - x
        height: full ? shell.height - 52 - gap : shell.clampBottom(shell.bottomHeight)
        z: full ? 3 : 1
        open: shell.bottomOpen && (shell.maximizedDock === "" || full)
        tabs: shell.bottomTabs
        tab: shell.bottomTab
        maximized: full
        dragging: shell.dragPane
        dropTarget: shell.dropDock === "bottom"
        onTabRequested: (id) => shell.dockOf(id) === "bottom" ? shell.front("bottom", id) : shell.movePane(id, "bottom")
        onTabClosed: (id) => shell.closeTab(id)
        onMoveRequested: (id, target) => shell.movePane(id, target)
        onCloseRequested: shell.setOpen("bottom", false)
        onMaximizeRequested: shell.maximizedDock = full ? "" : "bottom"
        onDragStarted: (id) => shell.dragPane = id
        onDragMoved: (p) => shell.dragPos = p
        onDragFinished: shell.finishDrag()

        MouseArea {
            z: 10
            visible: !bottomDock.full
            y: -7
            width: parent.width
            height: 8
            cursorShape: Qt.SizeVerCursor
            hoverEnabled: true
            Rectangle {
                anchors.centerIn: parent
                width: 44
                height: 4
                radius: 2
                color: parent.pressed ? Theme.accent : Theme.textDim
                opacity: parent.pressed ? 1 : (parent.containsMouse ? 0.8 : 0)
                Behavior on opacity { NumberAnimation { duration: Theme.motionFast } }
            }
            property real startY: 0
            property real startSize: 0
            onPressed: (mouse) => { startY = mapToItem(shell, 0, mouse.y).y; startSize = shell.clampBottom(shell.bottomHeight) }
            onPositionChanged: (mouse) => {
                if (pressed) shell.bottomHeightLive = shell.clampBottom(startSize + startY - mapToItem(shell, 0, mouse.y).y)
            }
            onReleased: if (shell.bottomHeightLive > 0) ipc.setUi("bottomHeight", Math.round(shell.bottomHeightLive))
            onDoubleClicked: { shell.bottomHeightLive = -1; ipc.setUi("bottomHeight", 0) }
        }
    }

    // Where a dragged tab will land when the target dock is closed: a glowing edge zone.
    Rectangle {
        z: 40
        readonly property bool showRight: shell.dropDock === "right" && !rightDock.open
        readonly property bool showBottom: shell.dropDock === "bottom" && !bottomDock.open
        visible: opacity > 0
        opacity: showRight || showBottom ? 1 : 0
        Behavior on opacity { NumberAnimation { duration: Theme.motionFast } }
        x: showRight ? shell.width - shell.clampRight(shell.rightWidth) - 8 : sidebar.width + 8
        y: showRight ? 52 : shell.height - shell.clampBottom(shell.bottomHeight) - 8
        width: showRight ? shell.clampRight(shell.rightWidth) : shell.width - sidebar.width - 16
        height: showRight ? shell.height - 60 : shell.clampBottom(shell.bottomHeight)
        radius: 14
        color: Qt.rgba(Theme.accent.r, Theme.accent.g, Theme.accent.b, 0.10)
        border.width: 2
        border.color: Theme.accent
    }
    // The tab following the pointer while dragging
    Rectangle {
        z: 41
        visible: shell.dragPane.length > 0
        x: shell.dragPos.x + 12
        y: shell.dragPos.y + 10
        width: ghostRow.implicitWidth + 24
        height: 30
        radius: 9
        color: Theme.card
        border.width: 1
        border.color: Theme.accent
        Row {
            id: ghostRow
            anchors.centerIn: parent
            spacing: 7
            readonly property var pane: rightDock.catalog[shell.dragPane] || null
            Icon { anchors.verticalCenter: parent.verticalCenter; name: ghostRow.pane ? ghostRow.pane.icon : "square"; size: 14; color: Theme.text }
            Text {
                anchors.verticalCenter: parent.verticalCenter
                text: ghostRow.pane ? ghostRow.pane.label : ""
                color: Theme.text
                font.family: Theme.sans
                font.pixelSize: 13
                font.weight: Font.Medium
            }
        }
    }
    SettingsPage {
        id: settingsPage
        anchors.fill: parent
        z: 50
        visible: opacity > 0
        opacity: shell.settingsOpen ? 1 : 0
        scale: shell.settingsOpen ? 1 : 0.985
        Behavior on opacity { NumberAnimation { duration: Theme.motionBase; easing.type: Theme.easeOut } }
        Behavior on scale { NumberAnimation { duration: Theme.motionBase; easing.type: Theme.easeEmphasis } }
        onCloseRequested: shell.settingsOpen = false

        // The window has no title bar, so the top edge doubles as a drag handle.
        Item {
            z: 5
            anchors.top: parent.top
            anchors.left: parent.left
            anchors.right: parent.right
            height: 10
            DragHandler { target: null; onActiveChanged: if (active) settingsPage.Window.window.startSystemMove() }
        }
        Item {
            z: 5
            anchors.top: parent.top
            anchors.left: parent.left
            anchors.leftMargin: 264
            anchors.right: parent.right
            height: 36
            DragHandler { target: null; onActiveChanged: if (active) settingsPage.Window.window.startSystemMove() }
        }
    }

    Connections {
        target: ipc
        function onBrowserActivity() { if (ipc.ui.autoBrowser !== false) shell.showPanel("browser") }
        function onPanelRequested(which) {
            if (which === "addProject") folderDialog.open()
            else shell.showPanel(which)
            shell.settingsOpen = false
        }
        function onDiffsChanged() { if (ipc.ui.autoDiff === true && ipc.running && ipc.diffs.length > 0) shell.showPanel("changes") }
    }

    // Drop files or images anywhere in the window to attach them to the next message.
    DropArea {
        id: dropZone
        anchors.fill: parent
        z: 60
        enabled: !shell.settingsOpen
        onDropped: (drop) => {
            if (drop.hasUrls) ipc.addAttachments(drop.urls)
            composer.focusInput()
        }
    }
    Rectangle {
        id: dropOverlay
        z: 61
        anchors.fill: parent
        anchors.margins: 14
        radius: 20
        visible: opacity > 0
        opacity: dropZone.containsDrag ? 1 : 0
        color: Qt.rgba(Theme.accent.r, Theme.accent.g, Theme.accent.b, 0.10)
        border.width: 2
        border.color: Theme.accent
        Behavior on opacity { NumberAnimation { duration: 120 } }
        Column {
            anchors.centerIn: parent
            spacing: 10
            Rectangle {
                anchors.horizontalCenter: parent.horizontalCenter
                width: 56
                height: 56
                radius: 18
                color: Theme.accent
                Icon { anchors.centerIn: parent; name: "paperclip"; size: 26; color: "white" }
            }
            Text {
                anchors.horizontalCenter: parent.horizontalCenter
                text: Format.tr(shell.dict, "DropToAttach", "Drop to attach")
                color: Theme.text
                font.family: Theme.sans
                font.pixelSize: 18
                font.weight: Font.DemiBold
            }
            Text {
                anchors.horizontalCenter: parent.horizontalCenter
                text: Format.tr(shell.dict, "DropHelp", "Images and GIFs up to 10 MB, text and code files")
                color: Theme.textSecondary
                font.family: Theme.sans
                font.pixelSize: 13
            }
        }
    }

    ShortcutsSheet { id: shortcuts }
    ApprovalSheet { id: approval }

    FolderDialog {
        id: folderDialog
        title: shell.t("PaletteFolder")
        onAccepted: { ipc.addProject(selectedFolder); Qt.callLater(composer.focusInput) }
    }
    FileDialog {
        id: exportDialog
        title: shell.t("ExportMarkdown")
        fileMode: FileDialog.SaveFile
        defaultSuffix: "md"
        nameFilters: ["Markdown (*.md)"]
        onAccepted: ipc.exportMarkdown(selectedFile)
    }

    Shortcut { sequence: "Ctrl+N"; onActivated: shell.newChat() }
    Timer { id: paletteTimer; interval: 400; onTriggered: commandPalette.open() }
    Shortcut { sequences: ["Ctrl+K", "Ctrl+P"]; onActivated: commandPalette.opened ? commandPalette.close() : commandPalette.open() }

    CommandPalette {
        id: commandPalette
        onActionTriggered: (id) => {
            if (id === "new") shell.newChat()
            else if (id === "model") header.openModels()
            else if (id === "changes" || id === "browser" || id === "agents" || id === "terminal") shell.showPanel(id)
            else if (id === "mcp") shell.openSettings("mcp")
            else if (id === "skills") shell.openSettings("skills")
            else if (id === "sidebar") shell.sidebarOpen = !shell.sidebarOpen
            else if (id === "project") folderDialog.open()
            else if (id === "export") exportDialog.open()
            else if (id === "settings") shell.openSettings("general")
            else if (id === "appearance") shell.openSettings("appearance")
            else if (id === "shortcuts") shortcuts.open()
        }
    }
    Shortcut { sequence: "Ctrl+Shift+S"; onActivated: shell.sidebarOpen = !shell.sidebarOpen }
    Shortcut { sequence: "Ctrl+B"; onActivated: shell.togglePanel("changes") }
    Shortcut { sequence: "Ctrl+Shift+B"; onActivated: shell.togglePanel("browser") }
    Shortcut { sequence: "Ctrl+,"; onActivated: shell.openSettings("general") }
    Shortcut { sequence: "Ctrl+Shift+M"; onActivated: header.openModels() }
    Shortcut { sequence: "Ctrl+Shift+A"; onActivated: shell.togglePanel("agents") }
    Shortcut { sequences: ["Ctrl+`", "Ctrl+J"]; onActivated: { shell.togglePanel("terminal") } }
    Shortcut { sequence: "Ctrl+/"; onActivated: shortcuts.open() }

    Component.onCompleted: Qt.callLater(function () {
        if (startPopup === "models") header.openModels()
        else if (startPopup === "account") sidebar.openAccount()
        else if (startPopup === "chat") header.openChatMenu()
        else if (startPopup === "settings") shell.openSettings(startTab.length > 0 ? startTab : "general")
        else if (startPopup === "account-settings") shell.openSettings("account")
        else if (startPopup === "shortcuts") shortcuts.open()
        else if (startPopup === "palette") paletteTimer.start()
        else if (startPopup === "inspector") shell.showPanel("changes")
        else if (startPopup === "hero") ipc.newChat()
        if (startPanel.length > 0) shell.showPanel(startPanel)
        composer.focusInput()
    })
}

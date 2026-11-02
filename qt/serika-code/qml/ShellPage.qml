import QtQuick
import QtQuick.Controls
import QtQuick.Dialogs
import QtQuick.Layouts

// The workspace: sidebar, chat, and the optional diff inspector.
Item {
    id: shell
    property bool sidebarOpen: true
    property bool inspectorOpen: false
    property string inspectorTab: "changes"
    property bool settingsOpen: false
    // Dock layout, kept in ui.json: tabs or split, right or bottom, and its size.
    readonly property string dockMode: ipc.ui.dockMode === "split" ? "split" : "tabs"
    readonly property string dockSide: ipc.ui.dockSide === "bottom" ? "bottom" : "right"
    property var splitPanes: ["browser", "changes"]
    property bool dockMaximized: false
    property real dockSizeLive: -1
    readonly property real dockSize: {
        if (dockSizeLive > 0) return dockSizeLive
        const saved = Number(dockSide === "bottom" ? ipc.ui.dockHeight : ipc.ui.dockWidth)
        return saved > 0 ? saved : (dockSide === "bottom" ? 340 : 480)
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
    function showPanel(tab) {
        inspectorTab = tab
        if (splitPanes.indexOf(tab) < 0) splitPanes = splitPanes.concat([tab])
        inspectorOpen = true
    }
    function togglePanel(tab) {
        if (!inspectorOpen) { showPanel(tab); return }
        if (dockMode === "split") {
            if (splitPanes.indexOf(tab) < 0) { showPanel(tab); return }
            const rest = splitPanes.filter(function (p) { return p !== tab })
            if (rest.length === 0) inspectorOpen = false
            else splitPanes = rest
            return
        }
        if (inspectorTab === tab) inspectorOpen = false
        else showPanel(tab)
    }
    function panelShown(tab) {
        return inspectorOpen && (dockMode === "split" ? splitPanes.indexOf(tab) >= 0 : inspectorTab === tab)
    }
    function clampDock(size) {
        const limit = dockSide === "bottom" ? shell.height - 220 : shell.width - sidebar.width - 360
        return Math.max(dockSide === "bottom" ? 180 : 320, Math.min(size, Math.max(320, limit)))
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
        anchors.right: shell.inspectorOpen && shell.dockSide === "right" ? inspector.left : parent.right
        anchors.rightMargin: shell.inspectorOpen && shell.dockSide === "right" ? 4 : 0
        anchors.top: parent.top
        anchors.bottom: shell.inspectorOpen && shell.dockSide === "bottom" ? inspector.top : parent.bottom
        anchors.bottomMargin: shell.inspectorOpen && shell.dockSide === "bottom" ? 4 : 0
        visible: !(shell.inspectorOpen && shell.dockMaximized)

        ChatHeader {
            id: header
            anchors.top: parent.top
            anchors.left: parent.left
            anchors.right: parent.right
            rightInset: shell.inspectorOpen && shell.dockSide === "right" ? 12 : 146
            browserShown: shell.panelShown("browser")
            changesShown: shell.panelShown("changes")
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

    Inspector {
        id: inspector
        // A floating card: 8 px from the edges, under the title row so the window buttons stay clear.
        readonly property bool full: shell.dockMaximized
        readonly property real gap: 8
        readonly property real titleRow: 52
        x: full || shell.dockSide === "bottom" ? sidebar.width + gap : shell.width - width - gap
        y: full || shell.dockSide === "right" ? titleRow : shell.height - height - gap
        width: full || shell.dockSide === "bottom" ? shell.width - sidebar.width - 2 * gap : shell.clampDock(shell.dockSize)
        height: full || shell.dockSide === "right" ? shell.height - titleRow - gap : shell.clampDock(shell.dockSize)
        open: shell.inspectorOpen
        tab: shell.inspectorTab
        mode: shell.dockMode
        side: shell.dockSide
        panes: shell.splitPanes
        maximized: shell.dockMaximized
        onTabRequested: (id) => {
            if (shell.dockMode === "split") shell.togglePanel(id)
            else shell.inspectorTab = id
        }
        onCloseRequested: { shell.inspectorOpen = false; shell.dockMaximized = false }
        onMaximizeRequested: shell.dockMaximized = !shell.dockMaximized
        onModeRequested: (mode) => {
            ipc.setUi("dockMode", mode)
            if (mode === "split") shell.splitPanes = ["browser", "changes"]
        }
        onSideRequested: (side) => { shell.dockSizeLive = -1; ipc.setUi("dockSide", side) }

        // Drag the inner edge to resize the dock
        MouseArea {
            z: 10
            visible: !shell.dockMaximized
            x: shell.dockSide === "right" ? -7 : 0
            y: shell.dockSide === "bottom" ? -7 : 0
            width: shell.dockSide === "right" ? 8 : parent.width
            height: shell.dockSide === "bottom" ? 8 : parent.height
            cursorShape: shell.dockSide === "right" ? Qt.SizeHorCursor : Qt.SizeVerCursor
            hoverEnabled: true
            property point start
            property real startSize: 0
            onPressed: (mouse) => { start = mapToItem(shell, mouse.x, mouse.y); startSize = shell.clampDock(shell.dockSize) }
            onPositionChanged: (mouse) => {
                if (!pressed) return
                const p = mapToItem(shell, mouse.x, mouse.y)
                shell.dockSizeLive = shell.clampDock(startSize + (shell.dockSide === "right" ? start.x - p.x : start.y - p.y))
            }
            onReleased: {
                if (shell.dockSizeLive > 0)
                    ipc.setUi(shell.dockSide === "bottom" ? "dockHeight" : "dockWidth", Math.round(shell.dockSizeLive))
            }
            onDoubleClicked: { shell.dockSizeLive = -1; ipc.setUi(shell.dockSide === "bottom" ? "dockHeight" : "dockWidth", 0) }
        }
    }

    SettingsPage {
        id: settingsPage
        anchors.fill: parent
        z: 50
        visible: opacity > 0
        opacity: shell.settingsOpen ? 1 : 0
        Behavior on opacity { NumberAnimation { duration: 140 } }
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

    Shortcut { sequences: ["Ctrl+K", "Ctrl+N"]; onActivated: shell.newChat() }
    Shortcut { sequence: "Ctrl+Shift+S"; onActivated: shell.sidebarOpen = !shell.sidebarOpen }
    Shortcut { sequence: "Ctrl+B"; onActivated: shell.togglePanel("changes") }
    Shortcut { sequence: "Ctrl+Shift+B"; onActivated: shell.togglePanel("browser") }
    Shortcut { sequence: "Ctrl+,"; onActivated: shell.openSettings("general") }
    Shortcut { sequence: "Ctrl+Shift+M"; onActivated: header.openModels() }
    Shortcut { sequence: "Ctrl+/"; onActivated: shortcuts.open() }

    Component.onCompleted: Qt.callLater(function () {
        if (startPopup === "models") header.openModels()
        else if (startPopup === "account") sidebar.openAccount()
        else if (startPopup === "chat") header.openChatMenu()
        else if (startPopup === "settings") shell.openSettings(startTab.length > 0 ? startTab : "general")
        else if (startPopup === "account-settings") shell.openSettings("account")
        else if (startPopup === "shortcuts") shortcuts.open()
        else if (startPopup === "inspector") shell.showPanel("changes")
        else if (startPopup === "hero") ipc.newChat()
        if (startPanel.length > 0) shell.showPanel(startPanel)
        composer.focusInput()
    })
}

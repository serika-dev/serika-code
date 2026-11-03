import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

// One dock (right or bottom) holding panes as tabs, like VS Code's panels: one pane in front,
// tabs dragged to the other dock or reordered, and a + menu to pull any pane in here.
// ShellPage owns all state; this asks for changes through signals.
Item {
    id: dock
    property string where: "right"               // "right" or "bottom"
    property bool open: false
    property var tabs: []                        // panes in this dock, in tab order
    property string tab: ""                      // the one in front
    property bool maximized: false
    property bool dropTarget: false              // a dragged tab would land here
    property string dragging: ""                 // id of the tab being dragged anywhere
    signal tabRequested(string id)
    signal tabClosed(string id)
    signal moveRequested(string id, string target)
    signal closeRequested()
    signal maximizeRequested()
    signal dragStarted(string id)
    signal dragMoved(point scenePos)
    signal dragFinished()
    readonly property var dict: ipc.strings
    readonly property string other: where === "right" ? "bottom" : "right"
    readonly property var catalog: ({
        changes: { id: "changes", label: Format.tr(dict, "Changes", "Changes"), icon: "git-branch", count: ipc.diffs.length, live: false },
        browser: { id: "browser", label: Format.tr(dict, "Browser", "Browser"), icon: "globe", count: (ipc.browser.tabs || []).length, live: false },
        agents: { id: "agents", label: Format.tr(dict, "Subagents", "Sub-agents"), icon: "users", count: ipc.agents.length,
                  live: ipc.agents.some(function (a) { return a.status === "running" }) },
        terminal: { id: "terminal", label: Format.tr(dict, "Terminal", "Terminal"), icon: "terminal", count: ipc.terminals.length,
                    live: ipc.terminals.some(function (t) { return t.running === true }) },
        git: { id: "git", label: Format.tr(dict, "Git", "Git"), icon: "git-commit", count: (ipc.git.stashes || []).length,
               live: (ipc.git.behind || 0) > 0 }
    })
    readonly property var allIds: ["changes", "git", "browser", "agents", "terminal"]

    // Where in this dock's tab row a tab dropped at a scene point would go.
    function indexAt(scenePos) {
        const p = tabRow.mapFromItem(null, scenePos.x, scenePos.y)
        for (let i = 0; i < tabRepeater.count; i++) {
            const item = tabRepeater.itemAt(i)
            if (item && p.x < item.x + item.width / 2) return i
        }
        return tabs.length
    }

    // Slide and fade in from the edge it is docked to.
    property real slide: open ? 0 : 28
    Behavior on slide { NumberAnimation { duration: Theme.motionBase; easing.type: Theme.easeEmphasis } }
    opacity: open ? 1 : 0
    Behavior on opacity { NumberAnimation { duration: Theme.motionBase; easing.type: Theme.easeOut } }
    visible: opacity > 0.01
    transform: Translate {
        x: dock.where === "right" && !dock.maximized ? dock.slide : 0
        y: dock.where === "bottom" && !dock.maximized ? dock.slide : 0
    }

    Rectangle {
        id: card
        anchors.fill: parent
        radius: 14
        color: Theme.card
        border.width: dock.dropTarget ? 2 : 1
        border.color: dock.dropTarget ? Theme.accent : Theme.borderSubtle
        Behavior on border.color { ColorAnimation { duration: Theme.motionFast } }
        clip: true

        ColumnLayout {
            anchors.fill: parent
            anchors.margins: 1
            spacing: 0

            // Tab bar
            Item {
                id: tabBar
                Layout.fillWidth: true
                Layout.preferredHeight: dock.where === "bottom" ? 38 : 44
                readonly property real room: width - controls.width - 18
                readonly property bool compact: dock.tabs.length * 122 + 34 > room

                Item {
                    anchors.left: parent.left
                    anchors.leftMargin: 6
                    anchors.bottom: parent.bottom
                    width: Math.max(0, tabBar.room)
                    height: tabBar.height - 6
                    clip: true
                Row {
                    id: tabRow
                    anchors.left: parent.left
                    anchors.bottom: parent.bottom
                    height: parent.height
                    spacing: 2
                    move: Transition { NumberAnimation { properties: "x"; duration: Theme.motionBase; easing.type: Theme.easeEmphasis } }
                    Repeater {
                        id: tabRepeater
                        model: dock.tabs
                        delegate: Item {
                            id: tabItem
                            required property string modelData
                            readonly property var pane: dock.catalog[modelData] || dock.catalog.changes
                            readonly property bool current: dock.tab === modelData
                            width: tabInner.implicitWidth + 26
                            height: tabRow.height
                            opacity: dock.dragging === modelData ? 0.35 : 1
                            Behavior on opacity { NumberAnimation { duration: Theme.motionFast } }
                            HoverHandler { id: tabHover }

                            Rectangle {
                                anchors.fill: parent
                                anchors.bottomMargin: -1
                                topLeftRadius: 9
                                topRightRadius: 9
                                color: tabItem.current ? Theme.bg : (tabHover.hovered ? Theme.hover : "transparent")
                                Behavior on color { ColorAnimation { duration: Theme.motionFast } }
                            }
                            Rectangle {
                                anchors.bottom: parent.bottom
                                anchors.horizontalCenter: parent.horizontalCenter
                                height: 2
                                radius: 1
                                width: tabItem.current ? parent.width - 20 : 0
                                color: Theme.accent
                                Behavior on width { NumberAnimation { duration: Theme.motionBase; easing.type: Theme.easeEmphasis } }
                            }
                            Row {
                                id: tabInner
                                anchors.centerIn: parent
                                anchors.horizontalCenterOffset: -1
                                spacing: 7
                                Item {
                                    anchors.verticalCenter: parent.verticalCenter
                                    width: 15
                                    height: 15
                                    Icon {
                                        anchors.fill: parent
                                        name: tabItem.pane.icon
                                        size: 15
                                        color: tabItem.current ? Theme.text : Theme.textSecondary
                                    }
                                    Rectangle {
                                        visible: tabItem.pane.live
                                        anchors.right: parent.right
                                        anchors.top: parent.top
                                        anchors.rightMargin: -3
                                        anchors.topMargin: -3
                                        width: 7
                                        height: 7
                                        radius: 3.5
                                        color: Theme.accent
                                        border.width: 1.5
                                        border.color: Theme.card
                                        SequentialAnimation on scale {
                                            running: tabItem.pane.live
                                            loops: Animation.Infinite
                                            NumberAnimation { to: 1.3; duration: 520; easing.type: Easing.InOutSine }
                                            NumberAnimation { to: 1.0; duration: 520; easing.type: Easing.InOutSine }
                                        }
                                    }
                                }
                                Text {
                                    visible: !tabBar.compact || tabItem.current
                                    anchors.verticalCenter: parent.verticalCenter
                                    text: tabItem.pane.label
                                    color: tabItem.current ? Theme.text : Theme.textSecondary
                                    font.family: Theme.sans
                                    font.pixelSize: 13
                                    font.weight: tabItem.current ? Font.DemiBold : Font.Medium
                                }
                                Text {
                                    visible: tabItem.pane.count > 0
                                    anchors.verticalCenter: parent.verticalCenter
                                    text: tabItem.pane.count
                                    color: Theme.textDim
                                    font.family: Theme.sans
                                    font.pixelSize: 11
                                }
                                Rectangle {
                                    id: closeButton
                                    anchors.verticalCenter: parent.verticalCenter
                                    width: 18
                                    height: 18
                                    radius: 5
                                    opacity: tabHover.hovered || tabItem.current ? 1 : 0
                                    Behavior on opacity { NumberAnimation { duration: Theme.motionFast } }
                                    color: closeArea.containsMouse ? Theme.active : "transparent"
                                    Icon { anchors.centerIn: parent; name: "x"; size: 11; color: Theme.textSecondary }
                                    MouseArea {
                                        id: closeArea
                                        anchors.fill: parent
                                        hoverEnabled: true
                                        cursorShape: Qt.PointingHandCursor
                                        onClicked: dock.tabClosed(tabItem.modelData)
                                    }
                                }
                            }
                            TapHandler {
                                acceptedButtons: Qt.LeftButton | Qt.MiddleButton | Qt.RightButton
                                onTapped: (point, button) => {
                                    // The close button handles its own click; don't also bring the tab back.
                                    const p = closeButton.mapFromItem(tabItem, point.position.x, point.position.y)
                                    if (closeButton.contains(p)) return
                                    if (button === Qt.MiddleButton) dock.tabClosed(tabItem.modelData)
                                    else if (button === Qt.RightButton) { tabMenu.paneId = tabItem.modelData; tabMenu.parent = tabItem; tabMenu.y = tabItem.height + 4; tabMenu.open() }
                                    else dock.tabRequested(tabItem.modelData)
                                }
                            }
                            // Drag a tab to the other dock or to a new spot in this one.
                            DragHandler {
                                target: null
                                cursorShape: active ? Qt.ClosedHandCursor : Qt.ArrowCursor
                                onActiveChanged: active ? dock.dragStarted(tabItem.modelData) : dock.dragFinished()
                                onCentroidChanged: if (active) dock.dragMoved(tabItem.mapToItem(null, centroid.position.x, centroid.position.y))
                            }
                            ToolTip.visible: tabBar.compact && tabHover.hovered && !tabItem.current
                            ToolTip.delay: 400
                            ToolTip.text: tabItem.pane.label
                        }
                    }
                    // + : bring any pane into this dock
                    IconButton {
                        id: addButton
                        anchors.verticalCenter: parent.verticalCenter
                        implicitWidth: 28
                        implicitHeight: 28
                        icon: "plus"
                        iconSize: 15
                        active: addMenu.opened
                        tip: Format.tr(dock.dict, "AddPane", "Open a panel")
                        onClicked: addMenu.opened ? addMenu.close() : addMenu.open()
                        Popover {
                            id: addMenu
                            y: addButton.height + 6
                            width: 230
                            contentItem: ColumnLayout {
                                spacing: 0
                                Repeater {
                                    model: dock.allIds
                                    delegate: MenuRow {
                                        required property string modelData
                                        readonly property var pane: dock.catalog[modelData]
                                        icon: pane.icon
                                        label: pane.label + (dock.tabs.indexOf(modelData) >= 0 ? "  ✓" : "")
                                        onTriggered: { addMenu.close(); dock.tabRequested(modelData) }
                                    }
                                }
                            }
                        }
                    }
                }
                }

                Row {
                    id: controls
                    anchors.right: parent.right
                    anchors.rightMargin: 6
                    anchors.verticalCenter: parent.verticalCenter
                    spacing: 0
                    IconButton {
                        icon: dock.where === "right" ? "panel-bottom" : "panel-right"
                        iconSize: 15
                        tip: dock.where === "right" ? Format.tr(dock.dict, "MoveToBottom", "Move to the bottom panel")
                                                    : Format.tr(dock.dict, "MoveToRight", "Move to the right panel")
                        onClicked: if (dock.tab.length > 0) dock.moveRequested(dock.tab, dock.other)
                    }
                    IconButton {
                        icon: dock.maximized ? "minimize-2" : "maximize-2"
                        iconSize: 14
                        tip: dock.maximized ? Format.tr(dock.dict, "Restore", "Restore") : Format.tr(dock.dict, "Maximize", "Maximize")
                        onClicked: dock.maximizeRequested()
                    }
                    IconButton {
                        icon: dock.where === "bottom" ? "chevron-down" : "x"
                        iconSize: 16
                        tip: Format.tr(dock.dict, "ClosePanel", "Close panel")
                        onClicked: dock.closeRequested()
                    }
                }
            }
            Rectangle { Layout.fillWidth: true; Layout.preferredHeight: 1; color: Theme.borderSubtle }

            // One pane at a time, cross-fading when you switch. Panes stay loaded while in this dock.
            Item {
                Layout.fillWidth: true
                Layout.fillHeight: true
                Repeater {
                    model: dock.allIds
                    delegate: Loader {
                        required property string modelData
                        readonly property bool front: dock.tab === modelData
                        anchors.fill: parent
                        active: dock.tabs.indexOf(modelData) >= 0
                        opacity: front ? 1 : 0
                        visible: opacity > 0.01 && dock.open
                        z: front ? 1 : 0
                        Behavior on opacity { NumberAnimation { duration: Theme.motionBase; easing.type: Theme.easeOut } }
                        sourceComponent: modelData === "browser" ? browserPane
                                       : modelData === "agents" ? agentsPane
                                       : modelData === "terminal" ? terminalPane
                                       : modelData === "git" ? gitPane : changesPane
                    }
                }
                // Drop hint while a tab from elsewhere hovers this dock
                Rectangle {
                    anchors.fill: parent
                    z: 5
                    visible: opacity > 0
                    opacity: dock.dropTarget && dock.tabs.indexOf(dock.dragging) < 0 ? 1 : 0
                    Behavior on opacity { NumberAnimation { duration: Theme.motionFast } }
                    color: Qt.rgba(Theme.accent.r, Theme.accent.g, Theme.accent.b, 0.08)
                }
            }
        }
    }

    Popover {
        id: tabMenu
        property string paneId: ""
        width: 230
        contentItem: ColumnLayout {
            spacing: 0
            MenuRow {
                icon: dock.where === "right" ? "panel-bottom" : "panel-right"
                label: dock.where === "right" ? Format.tr(dock.dict, "MoveToBottom", "Move to the bottom panel")
                                              : Format.tr(dock.dict, "MoveToRight", "Move to the right panel")
                onTriggered: { tabMenu.close(); dock.moveRequested(tabMenu.paneId, dock.other) }
            }
            MenuRow {
                icon: "x"
                label: Format.tr(dock.dict, "CloseTab", "Close")
                onTriggered: { tabMenu.close(); dock.tabClosed(tabMenu.paneId) }
            }
        }
    }

    Component { id: changesPane; DiffPanel {} }
    Component { id: browserPane; BrowserPanel {} }
    Component { id: agentsPane; AgentsPanel {} }
    Component { id: terminalPane; TerminalPanel {} }
    Component { id: gitPane; GitPanel {} }
}

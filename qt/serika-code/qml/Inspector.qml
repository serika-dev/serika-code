import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

// The panel beside or below the chat: Changes, Browser, and Sub-agents as closable tabs
// (or split side by side), with a + menu to bring panes back. A floating card under the
// title row. ShellPage owns all state; this asks for changes through signals.
Item {
    id: dock
    property bool open: false
    property var tabs: ["changes", "browser"]   // open panes, in tab order
    property string tab: "changes"               // the one in front (tabs mode)
    property string mode: "tabs"                 // "tabs" or "split"
    property string side: "right"                // "right" or "bottom"
    property bool maximized: false
    signal tabRequested(string id)
    signal tabClosed(string id)
    signal closeRequested()
    signal maximizeRequested()
    signal modeRequested(string mode)
    signal sideRequested(string side)
    readonly property var dict: ipc.strings
    readonly property var catalog: ({
        changes: { id: "changes", label: Format.tr(dict, "Changes", "Changes"), icon: "git-branch", count: ipc.diffs.length, live: false },
        browser: { id: "browser", label: Format.tr(dict, "Browser", "Browser"), icon: "globe", count: (ipc.browser.tabs || []).length, live: false },
        agents: { id: "agents", label: Format.tr(dict, "Subagents", "Sub-agents"), icon: "users", count: ipc.agents.length,
                  live: ipc.agents.some(function (a) { return a.status === "running" }) }
    })
    readonly property var allIds: ["changes", "browser", "agents"]
    function paneIndex(id) { return Math.max(0, allIds.indexOf(id)) }

    // Slide and fade in from the edge it is docked to.
    property real slide: open ? 0 : 28
    Behavior on slide { NumberAnimation { duration: Theme.motionBase; easing.type: Theme.easeEmphasis } }
    opacity: open ? 1 : 0
    Behavior on opacity { NumberAnimation { duration: Theme.motionBase; easing.type: Theme.easeOut } }
    visible: opacity > 0.01
    transform: Translate {
        x: dock.side === "right" && !dock.maximized ? dock.slide : 0
        y: dock.side === "bottom" && !dock.maximized ? dock.slide : 0
    }

    Rectangle {
        id: card
        anchors.fill: parent
        radius: 14
        color: Theme.card
        border.width: 1
        border.color: Theme.borderSubtle
        clip: true

        ColumnLayout {
            anchors.fill: parent
            anchors.margins: 1
            spacing: 0

            // Tab bar
            Item {
                id: tabBar
                Layout.fillWidth: true
                Layout.preferredHeight: 44
                // Room left of the controls; tabs drop their labels (all but the front one) when it runs out.
                readonly property real room: width - controls.width - 18
                readonly property bool compact: dock.tabs.length * 122 + 34 > room

                Item {
                    anchors.left: parent.left
                    anchors.leftMargin: 6
                    anchors.bottom: parent.bottom
                    width: Math.max(0, tabBar.room)
                    height: 38
                    clip: true
                Row {
                    id: tabRow
                    anchors.left: parent.left
                    anchors.bottom: parent.bottom
                    height: 38
                    spacing: 2
                    Repeater {
                        model: dock.tabs
                        delegate: Item {
                            id: tabItem
                            required property string modelData
                            readonly property var pane: dock.catalog[modelData] || dock.catalog.changes
                            readonly property bool current: dock.mode === "split" || dock.tab === modelData
                            width: tabInner.implicitWidth + 26
                            height: tabRow.height
                            HoverHandler { id: tabHover }

                            Rectangle {
                                anchors.fill: parent
                                anchors.bottomMargin: -1
                                topLeftRadius: 9
                                topRightRadius: 9
                                color: tabItem.current && dock.mode === "tabs" ? Theme.bg : (tabHover.hovered ? Theme.hover : "transparent")
                                Behavior on color { ColorAnimation { duration: Theme.motionFast } }
                            }
                            // Accent line under the tab in front
                            Rectangle {
                                anchors.bottom: parent.bottom
                                anchors.horizontalCenter: parent.horizontalCenter
                                height: 2
                                radius: 1
                                width: tabItem.current && dock.mode === "tabs" ? parent.width - 20 : 0
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
                                    // Something is happening in this pane
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
                                    visible: !tabBar.compact || (tabItem.current && dock.mode === "tabs")
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
                                // Close, on hover or for the tab in front
                                Rectangle {
                                    anchors.verticalCenter: parent.verticalCenter
                                    width: 18
                                    height: 18
                                    radius: 5
                                    opacity: tabHover.hovered || (tabItem.current && dock.mode === "tabs") ? 1 : 0
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
                            MouseArea {
                                anchors.fill: parent
                                z: -1
                                acceptedButtons: Qt.LeftButton | Qt.MiddleButton
                                cursorShape: Qt.PointingHandCursor
                                onClicked: (mouse) => mouse.button === Qt.MiddleButton ? dock.tabClosed(tabItem.modelData) : dock.tabRequested(tabItem.modelData)
                            }
                            ToolTip.visible: tabBar.compact && tabHover.hovered && !(tabItem.current && dock.mode === "tabs")
                            ToolTip.delay: 400
                            ToolTip.text: tabItem.pane.label
                        }
                    }
                    // + : open another pane
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
                        id: layoutButton
                        icon: dock.mode === "split" ? "columns" : "layout"
                        iconSize: 15
                        active: layoutMenu.opened
                        tip: Format.tr(dock.dict, "DockLayout", "Panel layout")
                        onClicked: layoutMenu.opened ? layoutMenu.close() : layoutMenu.open()
                        Popover {
                            id: layoutMenu
                            x: layoutButton.width - width
                            y: layoutButton.height + 6
                            width: 252
                            contentItem: ColumnLayout {
                                spacing: 8
                                Text { Layout.leftMargin: 6; Layout.topMargin: 4; text: Format.tr(dock.dict, "DockShow", "Show panels"); color: Theme.textDim; font.family: Theme.sans; font.pixelSize: 12 }
                                Segmented {
                                    Layout.leftMargin: 4
                                    options: [
                                        { value: "tabs", label: Format.tr(dock.dict, "DockTabsShort", "As tabs"), icon: "square" },
                                        { value: "split", label: Format.tr(dock.dict, "DockSplitShort", "Side by side"), icon: "columns" }
                                    ]
                                    value: dock.mode
                                    onPicked: (v) => dock.modeRequested(v)
                                }
                                Text { Layout.leftMargin: 6; text: Format.tr(dock.dict, "DockPlace", "Position"); color: Theme.textDim; font.family: Theme.sans; font.pixelSize: 12 }
                                Segmented {
                                    Layout.leftMargin: 4
                                    Layout.bottomMargin: 4
                                    options: [
                                        { value: "right", label: Format.tr(dock.dict, "DockRightShort", "Right") },
                                        { value: "bottom", label: Format.tr(dock.dict, "DockBottomShort", "Bottom") }
                                    ]
                                    value: dock.side
                                    onPicked: (v) => dock.sideRequested(v)
                                }
                            }
                        }
                    }
                    IconButton {
                        icon: dock.maximized ? "minimize-2" : "maximize-2"
                        iconSize: 14
                        tip: dock.maximized ? Format.tr(dock.dict, "Restore", "Restore") : Format.tr(dock.dict, "Maximize", "Maximize")
                        onClicked: dock.maximizeRequested()
                    }
                    IconButton {
                        icon: "x"
                        iconSize: 16
                        tip: Format.tr(dock.dict, "ClosePanel", "Close panel")
                        onClicked: dock.closeRequested()
                    }
                }
            }
            Rectangle { Layout.fillWidth: true; Layout.preferredHeight: 1; color: Theme.borderSubtle }

            // One pane at a time, cross-fading when you switch
            Item {
                visible: dock.mode === "tabs"
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
                        visible: opacity > 0.01
                        z: front ? 1 : 0
                        Behavior on opacity { NumberAnimation { duration: Theme.motionBase; easing.type: Theme.easeOut } }
                        sourceComponent: modelData === "browser" ? browserPane : (modelData === "agents" ? agentsPane : changesPane)
                    }
                }
            }

            // Open panes side by side: stacked when docked right, in a row below
            SplitView {
                id: split
                visible: dock.mode === "split"
                Layout.fillWidth: true
                Layout.fillHeight: true
                orientation: dock.side === "right" && !dock.maximized ? Qt.Vertical : Qt.Horizontal
                handle: Rectangle {
                    implicitWidth: split.orientation === Qt.Horizontal ? 6 : split.width
                    implicitHeight: split.orientation === Qt.Vertical ? 6 : split.height
                    color: SplitHandle.pressed ? Theme.accent : (SplitHandle.hovered ? Theme.border : Theme.borderSubtle)
                    Behavior on color { ColorAnimation { duration: Theme.motionFast } }
                }
                Repeater {
                    model: dock.mode === "split" ? dock.tabs : []
                    delegate: Item {
                        required property string modelData
                        SplitView.fillWidth: true
                        SplitView.fillHeight: true
                        SplitView.minimumWidth: 220
                        SplitView.minimumHeight: 140
                        Loader {
                            anchors.fill: parent
                            sourceComponent: parent.modelData === "browser" ? browserPane : (parent.modelData === "agents" ? agentsPane : changesPane)
                        }
                    }
                }
            }
        }
    }

    Component { id: changesPane; DiffPanel {} }
    Component { id: browserPane; BrowserPanel {} }
    Component { id: agentsPane; AgentsPanel {} }
}

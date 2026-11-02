import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

// The dock beside or below the chat: Changes and Browser, either as tabs or split side by side.
// ShellPage owns every piece of state; this only draws it and asks for changes through signals.
Rectangle {
    id: dock
    property bool open: false
    property string tab: "changes"          // active pane in tabs mode
    property string mode: "tabs"            // "tabs" or "split"
    property string side: "right"           // "right" or "bottom"
    property var panes: ["changes", "browser"]   // panes shown in split mode
    property bool maximized: false
    property real headerInset: 146          // room for the window controls when the dock reaches the top edge
    signal tabRequested(string id)
    signal closeRequested()
    signal paneCloseRequested(string id)
    signal maximizeRequested()
    signal modeRequested(string mode)
    signal sideRequested(string side)
    readonly property var dict: ipc.strings
    readonly property var allPanes: [
        { id: "changes", label: Format.t(dict, "Diffs") || "Changes", icon: "git-branch", count: ipc.diffs.length },
        { id: "browser", label: Format.tr(dict, "Browser", "Browser"), icon: "globe", count: (ipc.browser.tabs || []).length }
    ]
    function pane(id) {
        for (let i = 0; i < allPanes.length; ++i)
            if (allPanes[i].id === id) return allPanes[i]
        return allPanes[0]
    }
    readonly property var splitPanes: allPanes.filter(function (p) { return dock.panes.indexOf(p.id) >= 0 })

    visible: open
    color: Theme.card
    clip: true

    // Border toward the chat
    Rectangle {
        visible: dock.side === "right" && !dock.maximized
        anchors.left: parent.left
        width: 1
        height: parent.height
        color: Theme.borderSubtle
    }
    Rectangle {
        visible: dock.side === "bottom" && !dock.maximized
        anchors.top: parent.top
        width: parent.width
        height: 1
        color: Theme.borderSubtle
    }

    component PaneButton: Rectangle {
        id: pb
        property var entry: ({})
        property bool current: false
        signal picked()
        implicitHeight: 32
        implicitWidth: pbRow.implicitWidth + 22
        radius: 8
        color: current ? Theme.active : (pbArea.containsMouse ? Theme.hover : "transparent")
        Row {
            id: pbRow
            anchors.centerIn: parent
            spacing: 7
            Icon { anchors.verticalCenter: parent.verticalCenter; name: pb.entry.icon || "layers"; size: 15; color: pb.current ? Theme.text : Theme.textSecondary }
            Text {
                anchors.verticalCenter: parent.verticalCenter
                text: pb.entry.label || ""
                color: pb.current ? Theme.text : Theme.textSecondary
                font.family: Theme.sans
                font.pixelSize: 13
                font.weight: Font.Medium
            }
            Rectangle {
                visible: (pb.entry.count || 0) > 0
                anchors.verticalCenter: parent.verticalCenter
                width: countText.implicitWidth + 10
                height: 16
                radius: 8
                color: Theme.accentSoft
                Text {
                    id: countText
                    anchors.centerIn: parent
                    text: pb.entry.count || 0
                    color: Theme.accentSecondary
                    font.family: Theme.sans
                    font.pixelSize: 10
                    font.weight: Font.DemiBold
                }
            }
        }
        MouseArea { id: pbArea; anchors.fill: parent; hoverEnabled: true; cursorShape: Qt.PointingHandCursor; onClicked: pb.picked() }
    }

    // Header controls shared by both modes
    component DockControls: Row {
        spacing: 2
        IconButton {
            id: layoutButton
            icon: "layout"
            iconSize: 15
            active: layoutMenu.opened
            tip: Format.tr(dock.dict, "DockLayout", "Panel layout")
            onClicked: layoutMenu.opened ? layoutMenu.close() : layoutMenu.open()
            Popover {
                id: layoutMenu
                x: layoutButton.width - width
                y: layoutButton.height + 6
                width: 250
                contentItem: ColumnLayout {
                    spacing: 8
                    Text { Layout.leftMargin: 6; Layout.topMargin: 4; text: Format.tr(dock.dict, "DockShow", "Show panels"); color: Theme.textDim; font.family: Theme.sans; font.pixelSize: 12 }
                    Segmented {
                        Layout.leftMargin: 4
                        options: [
                            { value: "tabs", label: Format.tr(dock.dict, "DockTabsShort", "As tabs"), icon: "square" },
                            { value: "split", label: Format.tr(dock.dict, "DockSplitShort", "Split"), icon: "columns" }
                        ]
                        value: dock.mode
                        onPicked: (v) => dock.modeRequested(v)
                    }
                    Text { Layout.leftMargin: 6; text: Format.tr(dock.dict, "DockPlace", "Dock position"); color: Theme.textDim; font.family: Theme.sans; font.pixelSize: 12 }
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
            iconSize: 15
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

    readonly property bool reachesTop: dock.side === "right" || dock.maximized

    ColumnLayout {
        anchors.fill: parent
        anchors.leftMargin: dock.side === "right" && !dock.maximized ? 1 : 0
        anchors.topMargin: dock.side === "bottom" && !dock.maximized ? 1 : 0
        spacing: 0

        // Tabs mode header
        RowLayout {
            visible: dock.mode === "tabs"
            Layout.fillWidth: true
            Layout.preferredHeight: dock.reachesTop ? 52 : 44
            Layout.leftMargin: 10
            Layout.rightMargin: dock.reachesTop ? dock.headerInset : 8
            spacing: 4
            Repeater {
                model: dock.allPanes
                delegate: PaneButton {
                    required property var modelData
                    entry: modelData
                    current: dock.tab === modelData.id
                    onPicked: dock.tabRequested(modelData.id)
                }
            }
            Item { Layout.fillWidth: true }
            DockControls {}
        }
        Rectangle { visible: dock.mode === "tabs"; Layout.fillWidth: true; Layout.preferredHeight: 1; color: Theme.borderSubtle }

        StackLayout {
            visible: dock.mode === "tabs"
            Layout.fillWidth: true
            Layout.fillHeight: true
            currentIndex: dock.tab === "browser" ? 1 : 0
            DiffPanel {}
            BrowserPanel {}
        }

        // Split mode: a slim title row, then the panes side by side (or stacked when docked right)
        RowLayout {
            visible: dock.mode === "split"
            Layout.fillWidth: true
            Layout.preferredHeight: dock.reachesTop ? 52 : 40
            Layout.leftMargin: 14
            Layout.rightMargin: dock.reachesTop ? dock.headerInset : 8
            spacing: 6
            Repeater {
                model: dock.allPanes
                delegate: PaneButton {
                    required property var modelData
                    entry: modelData
                    current: dock.panes.indexOf(modelData.id) >= 0
                    onPicked: dock.tabRequested(modelData.id)
                }
            }
            Item { Layout.fillWidth: true }
            DockControls {}
        }
        Rectangle { visible: dock.mode === "split"; Layout.fillWidth: true; Layout.preferredHeight: 1; color: Theme.borderSubtle }

        SplitView {
            id: split
            visible: dock.mode === "split"
            Layout.fillWidth: true
            Layout.fillHeight: true
            // Docked right the dock is tall and narrow, so panes stack; docked below they sit side by side.
            orientation: dock.side === "right" && !dock.maximized ? Qt.Vertical : Qt.Horizontal
            handle: Rectangle {
                implicitWidth: split.orientation === Qt.Horizontal ? 5 : split.width
                implicitHeight: split.orientation === Qt.Vertical ? 5 : split.height
                color: SplitHandle.pressed ? Theme.accent : (SplitHandle.hovered ? Theme.border : Theme.borderSubtle)
            }
            Item {
                visible: dock.panes.indexOf("browser") >= 0
                SplitView.fillWidth: true
                SplitView.fillHeight: true
                SplitView.minimumWidth: 220
                SplitView.minimumHeight: 160
                BrowserPanel { anchors.fill: parent; visible: dock.mode === "split" }
            }
            Item {
                visible: dock.panes.indexOf("changes") >= 0
                SplitView.preferredWidth: split.width * 0.42
                SplitView.preferredHeight: split.height * 0.4
                SplitView.minimumWidth: 220
                SplitView.minimumHeight: 120
                DiffPanel { anchors.fill: parent }
            }
        }
    }
}

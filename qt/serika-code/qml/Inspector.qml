import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

// The panel beside or below the chat with Changes and Browser, as tabs or split.
// A floating card under the title row. ShellPage owns all state; this asks through signals.
Item {
    id: dock
    property bool open: false
    property string tab: "changes"          // active pane in tabs mode
    property string mode: "tabs"            // "tabs" or "split"
    property string side: "right"           // "right" or "bottom"
    property var panes: ["changes", "browser"]   // panes shown in split mode
    property bool maximized: false
    signal tabRequested(string id)
    signal closeRequested()
    signal maximizeRequested()
    signal modeRequested(string mode)
    signal sideRequested(string side)
    readonly property var dict: ipc.strings
    readonly property var allPanes: [
        { id: "changes", label: Format.tr(dict, "Changes", "Changes"), icon: "git-branch", count: ipc.diffs.length },
        { id: "browser", label: Format.tr(dict, "Browser", "Browser"), icon: "globe", count: (ipc.browser.tabs || []).length }
    ]
    function shown(id) { return mode === "split" ? panes.indexOf(id) >= 0 : tab === id }

    visible: open
    opacity: open ? 1 : 0
    Behavior on opacity { NumberAnimation { duration: 140 } }

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

            // Header: pane switcher and window-style controls
            Item {
                Layout.fillWidth: true
                Layout.preferredHeight: 46

                Rectangle {
                    id: switcher
                    anchors.left: parent.left
                    anchors.leftMargin: 8
                    anchors.verticalCenter: parent.verticalCenter
                    height: 32
                    width: switchRow.implicitWidth + 6
                    radius: 10
                    color: Theme.bg
                    border.width: 1
                    border.color: Theme.borderSubtle
                    Row {
                        id: switchRow
                        anchors.centerIn: parent
                        spacing: 2
                        Repeater {
                            model: dock.allPanes
                            delegate: Rectangle {
                                id: pill
                                required property var modelData
                                readonly property bool on: dock.shown(modelData.id)
                                height: 26
                                width: pillRow.implicitWidth + 20
                                radius: 7
                                color: on ? Theme.raised : (pillArea.containsMouse ? Theme.hover : "transparent")
                                border.width: on ? 1 : 0
                                border.color: Theme.border
                                Behavior on color { ColorAnimation { duration: 120 } }
                                Row {
                                    id: pillRow
                                    anchors.centerIn: parent
                                    spacing: 6
                                    Icon {
                                        anchors.verticalCenter: parent.verticalCenter
                                        name: pill.modelData.icon
                                        size: 14
                                        color: pill.on ? Theme.text : Theme.textSecondary
                                    }
                                    Text {
                                        anchors.verticalCenter: parent.verticalCenter
                                        text: pill.modelData.label
                                        color: pill.on ? Theme.text : Theme.textSecondary
                                        font.family: Theme.sans
                                        font.pixelSize: 13
                                        font.weight: pill.on ? Font.DemiBold : Font.Medium
                                    }
                                    Rectangle {
                                        visible: pill.modelData.count > 0
                                        anchors.verticalCenter: parent.verticalCenter
                                        width: Math.max(18, countText.implicitWidth + 10)
                                        height: 17
                                        radius: 8.5
                                        color: pill.on ? Theme.accentSoft : Theme.active
                                        Text {
                                            id: countText
                                            anchors.centerIn: parent
                                            text: pill.modelData.count
                                            color: pill.on ? Theme.accentSecondary : Theme.textDim
                                            font.family: Theme.sans
                                            font.pixelSize: 10
                                            font.weight: Font.DemiBold
                                        }
                                    }
                                }
                                MouseArea {
                                    id: pillArea
                                    anchors.fill: parent
                                    hoverEnabled: true
                                    cursorShape: Qt.PointingHandCursor
                                    onClicked: dock.tabRequested(pill.modelData.id)
                                }
                            }
                        }
                    }
                }

                Row {
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
                            width: 248
                            contentItem: ColumnLayout {
                                spacing: 8
                                Text { Layout.leftMargin: 6; Layout.topMargin: 4; text: Format.tr(dock.dict, "DockShow", "Show panels"); color: Theme.textDim; font.family: Theme.sans; font.pixelSize: 12 }
                                Segmented {
                                    Layout.leftMargin: 4
                                    options: [
                                        { value: "tabs", label: Format.tr(dock.dict, "DockTabsShort", "One at a time"), icon: "square" },
                                        { value: "split", label: Format.tr(dock.dict, "DockSplitShort", "Split"), icon: "columns" }
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

            // One pane at a time
            StackLayout {
                visible: dock.mode === "tabs"
                Layout.fillWidth: true
                Layout.fillHeight: true
                currentIndex: dock.tab === "browser" ? 1 : 0
                DiffPanel {}
                BrowserPanel {}
            }

            // Both panes: stacked when docked right, side by side below
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
                }
                Item {
                    visible: dock.panes.indexOf("browser") >= 0
                    SplitView.fillWidth: true
                    SplitView.fillHeight: true
                    SplitView.minimumWidth: 240
                    SplitView.minimumHeight: 180
                    BrowserPanel { anchors.fill: parent; visible: dock.mode === "split" }
                }
                Item {
                    visible: dock.panes.indexOf("changes") >= 0
                    SplitView.preferredWidth: split.width * 0.42
                    SplitView.preferredHeight: split.height * 0.42
                    SplitView.minimumWidth: 240
                    SplitView.minimumHeight: 140
                    DiffPanel { anchors.fill: parent }
                }
            }
        }
    }
}

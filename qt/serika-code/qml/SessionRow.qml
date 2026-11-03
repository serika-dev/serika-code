import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

// One history row: click to open, double-click or ⋯ to rename, ⋯ to delete.
Rectangle {
    id: row
    property string sessionId: ""
    property string title: ""
    property bool active: false
    property int indent: 0
    property string time: ""
    property bool editing: false
    property bool confirming: false
    property bool draft: false           // a new chat with no message yet
    property bool unsent: false          // text typed but not sent
    readonly property var dict: ipc.strings
    // "running", "waiting" (for your approval), "done" (finished while you were elsewhere) or ""
    readonly property string live: ipc.liveChats[sessionId] || ""
    readonly property bool working: live === "running" || live === "waiting"

    implicitHeight: 36
    radius: 8
    color: confirming ? Theme.dangerSoft : (active ? Theme.active : (area.containsMouse || menuButton.hovered ? Theme.hover : "transparent"))

    function beginEdit() {
        confirming = false
        editing = true
        editor.text = row.title
        editor.forceActiveFocus()
        editor.selectAll()
    }
    function commitEdit() {
        const next = editor.text.trim()
        editing = false
        if (next.length > 0 && next !== row.title)
            ipc.renameSession(row.sessionId, next)
    }

    // Normal row
    Text {
        visible: !row.editing && !row.confirming
        anchors.left: parent.left
        anchors.leftMargin: 10 + row.indent
        anchors.right: parent.right
        anchors.rightMargin: (area.containsMouse || menuButton.hovered || menu.opened) ? (row.working ? 64 : 36)
                           : (row.live.length > 0 ? 34 : (draftTag.visible ? draftTag.width + 14 : (row.time.length > 0 ? timeLabel.implicitWidth + 18 : 10)))
        anchors.verticalCenter: parent.verticalCenter
        text: row.title.length > 0 ? row.title : Format.t(row.dict, "NewChat")
        elide: Text.ElideRight
        color: row.active ? Theme.text : (area.containsMouse ? Theme.text : Theme.textSecondary)
        font.italic: row.draft && row.title.length === 0
        font.family: Theme.sans
        font.pixelSize: 14
    }
    // Draft marker: a tag for a new chat, a pencil for unsent text in an existing chat
    Rectangle {
        id: draftTag
        visible: (row.draft || row.unsent) && !row.editing && !row.confirming && row.live.length === 0 && !(area.containsMouse || menuButton.hovered || menu.opened)
        anchors.right: parent.right
        anchors.rightMargin: 8
        anchors.verticalCenter: parent.verticalCenter
        width: row.draft ? draftText.implicitWidth + 12 : 20
        height: 18
        radius: 6
        color: row.draft ? Theme.accentSoft : "transparent"
        Text {
            id: draftText
            visible: row.draft
            anchors.centerIn: parent
            text: Format.tr(row.dict, "Draft", "Draft")
            color: Theme.accentSecondary
            font.family: Theme.sans
            font.pixelSize: 10
            font.weight: Font.DemiBold
        }
        Icon { visible: !row.draft; anchors.centerIn: parent; name: "edit-2"; size: 12; color: Theme.accentSecondary }
    }
    Text {
        id: timeLabel
        visible: !row.editing && !row.confirming && row.time.length > 0 && row.live.length === 0 && !draftTag.visible && !(area.containsMouse || menuButton.hovered || menu.opened)
        anchors.right: parent.right
        anchors.rightMargin: 10
        anchors.verticalCenter: parent.verticalCenter
        text: row.time
        color: Theme.textDim
        font.family: Theme.sans
        font.pixelSize: 12
    }
    // Live state on the right: spinner while it works, amber when it needs you, a dot when it
    // finished while you were in another chat.
    Item {
        visible: !row.editing && !row.confirming && row.live.length > 0 && !(area.containsMouse || menuButton.hovered || menu.opened || stopButton.hovered)
        anchors.right: parent.right
        anchors.rightMargin: 12
        anchors.verticalCenter: parent.verticalCenter
        width: 16
        height: 16
        Icon {
            id: spinner
            anchors.centerIn: parent
            visible: row.live === "running"
            name: "loader"
            size: 14
            color: Theme.accentSecondary
            RotationAnimator on rotation { running: spinner.visible; from: 0; to: 360; duration: 1000; loops: Animation.Infinite }
        }
        Rectangle {
            anchors.centerIn: parent
            visible: row.live === "waiting"
            width: 9
            height: 9
            radius: 4.5
            color: Theme.warning
            SequentialAnimation on scale {
                running: row.live === "waiting"
                loops: Animation.Infinite
                NumberAnimation { to: 1.35; duration: 560; easing.type: Easing.InOutSine }
                NumberAnimation { to: 1.0; duration: 560; easing.type: Easing.InOutSine }
            }
        }
        Rectangle {
            anchors.centerIn: parent
            visible: row.live === "done"
            width: 8
            height: 8
            radius: 4
            color: Theme.accent
        }
        HoverHandler { id: liveHover }
        ToolTip.visible: liveHover.hovered
        ToolTip.delay: 300
        ToolTip.text: row.live === "running" ? Format.tr(row.dict, "ChatWorking", "Working in the background")
                    : row.live === "waiting" ? Format.tr(row.dict, "ChatWaiting", "Waiting for your approval")
                    : Format.tr(row.dict, "ChatDone", "New reply")
    }
    MouseArea {
        id: area
        anchors.fill: parent
        visible: !row.editing && !row.confirming
        hoverEnabled: true
        cursorShape: Qt.PointingHandCursor
        onClicked: ipc.openSession(row.sessionId)
        onDoubleClicked: if (!row.draft) row.beginEdit()
    }
    // Stop a chat that is running in the background without opening it
    IconButton {
        id: stopButton
        visible: row.working && !row.editing && !row.confirming && (area.containsMouse || hovered || menuButton.hovered || menu.opened)
        anchors.right: menuButton.left
        anchors.verticalCenter: parent.verticalCenter
        implicitWidth: 28
        implicitHeight: 28
        icon: "square"
        iconSize: 12
        tip: Format.tr(row.dict, "StopChat", "Stop")
        onClicked: ipc.stopChat(row.sessionId)
    }
    IconButton {
        id: menuButton
        visible: !row.editing && !row.confirming && (area.containsMouse || hovered || menu.opened || stopButton.hovered)
        anchors.right: parent.right
        anchors.rightMargin: 4
        anchors.verticalCenter: parent.verticalCenter
        implicitWidth: 28
        implicitHeight: 28
        icon: "more-horizontal"
        iconSize: 16
        active: menu.opened
        onClicked: menu.opened ? menu.close() : menu.open()

        Popover {
            id: menu
            x: menuButton.width - width
            y: menuButton.height + 4
            width: 176
            contentItem: ColumnLayout {
                spacing: 0
                MenuRow { visible: !row.draft; icon: "edit-2"; label: Format.t(row.dict, "Rename"); onTriggered: { menu.close(); row.beginEdit() } }
                MenuRow { icon: "trash-2"; label: Format.t(row.dict, "Delete"); danger: true; onTriggered: { menu.close(); row.confirming = true } }
            }
        }
    }

    // Rename
    TextField {
        id: editor
        visible: row.editing
        anchors.fill: parent
        anchors.margins: 3
        color: Theme.text
        selectionColor: Theme.accent
        font.family: Theme.sans
        font.pixelSize: 14
        maximumLength: 100
        leftPadding: 8
        background: Rectangle { radius: 6; color: Theme.input; border.width: 1; border.color: Theme.border }
        onAccepted: row.commitEdit()
        onActiveFocusChanged: if (!activeFocus && row.editing) row.commitEdit()
        Keys.onEscapePressed: row.editing = false
    }

    // Delete confirmation
    RowLayout {
        visible: row.confirming
        anchors.fill: parent
        anchors.leftMargin: 10
        anchors.rightMargin: 4
        spacing: 2
        Text {
            Layout.fillWidth: true
            text: Format.t(row.dict, "DeleteConfirm", { title: row.title })
            elide: Text.ElideRight
            color: Theme.danger
            font.family: Theme.sans
            font.pixelSize: 13
        }
        IconButton {
            implicitWidth: 28
            implicitHeight: 28
            icon: "check"
            iconSize: 16
            danger: true
            onClicked: { row.confirming = false; ipc.deleteSession(row.sessionId) }
        }
        IconButton {
            implicitWidth: 28
            implicitHeight: 28
            icon: "x"
            iconSize: 16
            onClicked: row.confirming = false
        }
    }

    Behavior on color { ColorAnimation { duration: Theme.motionFast } }
}

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
    readonly property var dict: ipc.strings

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
        anchors.rightMargin: (area.containsMouse || menuButton.hovered || menu.opened) ? 36 : (row.time.length > 0 ? 40 : 10)
        anchors.verticalCenter: parent.verticalCenter
        text: row.title.length > 0 ? row.title : Format.t(row.dict, "NewChat")
        elide: Text.ElideRight
        color: row.active ? Theme.text : (area.containsMouse ? Theme.text : Theme.textSecondary)
        font.family: Theme.sans
        font.pixelSize: 14
    }
    Text {
        visible: !row.editing && !row.confirming && row.time.length > 0 && !(area.containsMouse || menuButton.hovered || menu.opened)
        anchors.right: parent.right
        anchors.rightMargin: 10
        anchors.verticalCenter: parent.verticalCenter
        text: row.time
        color: Theme.textDim
        font.family: Theme.sans
        font.pixelSize: 12
    }
    MouseArea {
        id: area
        anchors.fill: parent
        visible: !row.editing && !row.confirming
        hoverEnabled: true
        cursorShape: Qt.PointingHandCursor
        onClicked: ipc.openSession(row.sessionId)
        onDoubleClicked: row.beginEdit()
    }
    IconButton {
        id: menuButton
        visible: !row.editing && !row.confirming && (area.containsMouse || hovered || menu.opened)
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
                MenuRow { icon: "edit-2"; label: Format.t(row.dict, "Rename"); onTriggered: { menu.close(); row.beginEdit() } }
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

import QtQuick

// On/off switch. The parent owns the state: bind `checked` and handle `toggled`.
Rectangle {
    id: sw
    property bool checked: false
    signal toggled(bool value)

    implicitWidth: 40
    implicitHeight: 22
    radius: 11
    color: checked ? Theme.accent : Theme.active
    border.width: checked ? 0 : 1
    border.color: Theme.border
    Behavior on color { ColorAnimation { duration: 120 } }

    Rectangle {
        width: 16
        height: 16
        radius: 8
        y: 3
        x: sw.checked ? sw.width - width - 3 : 3
        color: "white"
        Behavior on x { NumberAnimation { duration: 120; easing.type: Easing.OutCubic } }
    }
    MouseArea {
        anchors.fill: parent
        cursorShape: Qt.PointingHandCursor
        onClicked: sw.toggled(!sw.checked)
    }
}

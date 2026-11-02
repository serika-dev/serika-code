import QtQuick

// Composer action pill (Search, Research).
Rectangle {
    id: pill
    property string icon: "globe"
    property string label: ""
    property bool active: false
    signal clicked()

    implicitHeight: 30
    implicitWidth: row.implicitWidth + 22
    radius: height / 2
    color: active ? Theme.accentSoft : (area.containsMouse ? Theme.hover : "transparent")
    border.width: 1
    border.color: active ? Theme.accent : Theme.border

    Row {
        id: row
        anchors.centerIn: parent
        spacing: 6
        Icon {
            name: pill.icon
            size: 14
            color: pill.active ? Theme.accentSecondary : Theme.textSecondary
            anchors.verticalCenter: parent.verticalCenter
        }
        Text {
            text: pill.label
            color: pill.active ? Theme.accentSecondary : Theme.textSecondary
            font.family: Theme.sans
            font.pixelSize: 13
            anchors.verticalCenter: parent.verticalCenter
        }
    }
    MouseArea {
        id: area
        anchors.fill: parent
        hoverEnabled: true
        cursorShape: Qt.PointingHandCursor
        onClicked: pill.clicked()
    }
}

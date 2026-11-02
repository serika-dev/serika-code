import QtQuick
import QtQuick.Controls

Rectangle {
    id: btn
    property string icon: "x"
    property real iconSize: 18
    property string tip: ""
    property bool active: false
    property bool danger: false
    property alias hovered: area.containsMouse
    signal clicked()

    implicitWidth: 32
    implicitHeight: 32
    radius: 8
    color: (active || area.containsPress) ? Theme.active : (area.containsMouse ? Theme.hover : "transparent")

    Icon {
        anchors.centerIn: parent
        name: btn.icon
        size: btn.iconSize
        color: btn.danger ? Theme.danger : ((btn.active || area.containsMouse) ? Theme.text : Theme.textSecondary)
    }
    MouseArea {
        id: area
        anchors.fill: parent
        hoverEnabled: true
        cursorShape: Qt.PointingHandCursor
        onClicked: btn.clicked()
    }
    ToolTip.visible: btn.tip.length > 0 && area.containsMouse
    ToolTip.delay: 500
    ToolTip.text: btn.tip
}

import QtQuick
import QtQuick.Controls

// Compact button for settings rows and tool cards. `primary` uses the accent fill.
Button {
    id: btn
    property bool danger: false
    property bool primary: false
    property string iconName: ""

    implicitHeight: 30
    leftPadding: 12
    rightPadding: 12
    hoverEnabled: true
    background: Rectangle {
        radius: 8
        color: btn.primary ? (btn.hovered ? Theme.accentHover : Theme.accent)
                           : (btn.danger && btn.hovered ? Theme.dangerSoft : (btn.hovered ? Theme.active : Theme.hover))
        border.width: btn.primary ? 0 : 1
        border.color: btn.danger ? Qt.rgba(0.96, 0.25, 0.37, 0.25) : Theme.borderSubtle
        opacity: btn.enabled ? 1 : 0.5
        Behavior on color { ColorAnimation { duration: Theme.motionFast } }
    }
    contentItem: Row {
        spacing: 6
        Icon {
            visible: btn.iconName.length > 0
            anchors.verticalCenter: parent.verticalCenter
            name: btn.iconName
            size: 14
            color: btn.primary ? "white" : (btn.danger ? Theme.danger : Theme.textSecondary)
        }
        Text {
            anchors.verticalCenter: parent.verticalCenter
            text: btn.text
            color: btn.primary ? "white" : (btn.danger ? Theme.danger : Theme.text)
            font.family: Theme.sans
            font.pixelSize: 13
            font.weight: Font.Medium
        }
    }
    HoverHandler { cursorShape: Qt.PointingHandCursor }

    scale: btn.pressed ? 0.97 : 1
    Behavior on scale { NumberAnimation { duration: Theme.motionFast; easing.type: Theme.easeOut } }
}

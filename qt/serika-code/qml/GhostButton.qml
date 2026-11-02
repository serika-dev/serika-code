import QtQuick
import QtQuick.Controls

// Secondary button (`.btn-ghost`): soft fill, quiet text.
Button {
    id: btn
    property bool danger: false
    implicitHeight: 36
    leftPadding: 14
    rightPadding: 14
    hoverEnabled: true
    background: Rectangle {
        radius: 8
        color: btn.hovered ? Theme.active : Theme.hover
        opacity: btn.enabled ? 1 : 0.5
    }
    contentItem: Text {
        text: btn.text
        color: btn.danger ? Theme.danger : Theme.text
        font.family: Theme.sans
        font.pixelSize: 14
        horizontalAlignment: Text.AlignHCenter
        verticalAlignment: Text.AlignVCenter
    }
    HoverHandler { cursorShape: Qt.PointingHandCursor }
}

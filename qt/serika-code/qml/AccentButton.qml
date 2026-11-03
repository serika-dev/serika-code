import QtQuick
import QtQuick.Controls

// Primary action. Accent fill, used for Sign in, Continue, Approve.
Button {
    id: btn
    property color fill: Theme.accent
    property color fillHover: Theme.accentHover
    property color labelColor: "white"
    implicitHeight: 44
    hoverEnabled: true
    background: Rectangle {
        radius: 12
        color: btn.hovered ? btn.fillHover : btn.fill
        opacity: btn.enabled ? 1 : 0.5
        Behavior on color { ColorAnimation { duration: Theme.motionFast } }
    }
    contentItem: Text {
        text: btn.text
        color: btn.labelColor
        font.family: Theme.sans
        font.pixelSize: 15
        font.weight: Font.DemiBold
        horizontalAlignment: Text.AlignHCenter
        verticalAlignment: Text.AlignVCenter
        elide: Text.ElideRight
    }
    HoverHandler { cursorShape: Qt.PointingHandCursor }

    scale: btn.pressed ? 0.97 : 1
    Behavior on scale { NumberAnimation { duration: Theme.motionFast; easing.type: Theme.easeOut } }
}

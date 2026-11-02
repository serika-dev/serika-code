import QtQuick
import QtQuick.Effects

// Round account picture with an initial as the fallback.
Item {
    id: root
    property url source
    property string name: ""
    property real size: 32
    width: size
    height: size

    Rectangle {
        anchors.fill: parent
        radius: width / 2
        color: Theme.active
        visible: picture.status !== Image.Ready

        Text {
            anchors.centerIn: parent
            text: root.name.length ? root.name.charAt(0).toUpperCase() : ""
            color: Theme.text
            font.family: Theme.sans
            font.pixelSize: Math.round(root.size * 0.44)
            font.weight: Font.Medium
        }
    }

    Image {
        id: picture
        anchors.fill: parent
        source: root.source
        fillMode: Image.PreserveAspectCrop
        asynchronous: true
        smooth: true
        sourceSize: Qt.size(root.size * 3, root.size * 3)
        visible: false
    }
    Rectangle {
        id: mask
        anchors.fill: parent
        radius: width / 2
        visible: false
        layer.enabled: true
    }
    MultiEffect {
        anchors.fill: parent
        source: picture
        maskEnabled: true
        maskSource: mask
        visible: picture.status === Image.Ready
    }
}

import QtQuick
import QtQuick.Effects

// An image cropped to a rounded rectangle. GIFs animate.
Item {
    id: root
    property url source
    property real radius: 10
    property int fillMode: Image.PreserveAspectCrop
    readonly property bool gif: source.toString().toLowerCase().indexOf(".gif") >= 0
    readonly property Item picture: gif ? animated : still
    readonly property bool ready: picture.status === Image.Ready

    Rectangle {
        anchors.fill: parent
        radius: root.radius
        color: Theme.active
        visible: !root.ready
        Icon { anchors.centerIn: parent; name: "image"; size: Math.min(24, root.width / 3); color: Theme.textDim }
    }
    Image {
        id: still
        anchors.fill: parent
        visible: false
        source: root.gif ? "" : root.source
        fillMode: root.fillMode
        asynchronous: true
        smooth: true
        sourceSize: Qt.size(Math.max(64, root.width * 2), Math.max(64, root.height * 2))
    }
    AnimatedImage {
        id: animated
        anchors.fill: parent
        visible: false
        source: root.gif ? root.source : ""
        fillMode: root.fillMode
        playing: true
        smooth: true
    }
    Rectangle {
        id: mask
        anchors.fill: parent
        radius: root.radius
        visible: false
        layer.enabled: true
    }
    MultiEffect {
        anchors.fill: parent
        source: root.picture
        maskEnabled: true
        maskSource: mask
        visible: root.ready
    }
    Rectangle {
        anchors.fill: parent
        radius: root.radius
        color: "transparent"
        border.width: 1
        border.color: Theme.border
    }
}

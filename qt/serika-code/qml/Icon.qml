import QtQuick
import QtQuick.Effects

// Feather icon (the set SerikaChat uses), recolored from the theme.
Item {
    id: root
    property string name: "search"
    property real size: 16
    property color color: Theme.textDim
    implicitWidth: size
    implicitHeight: size
    width: size
    height: size

    Image {
        id: glyph
        anchors.fill: parent
        source: root.name.length > 0 ? Qt.resolvedUrl("icons/" + root.name + ".svg") : ""
        sourceSize: Qt.size(Math.ceil(root.size * 2), Math.ceil(root.size * 2))
        smooth: true
        mipmap: true
        visible: false
    }
    MultiEffect {
        anchors.fill: parent
        source: glyph
        colorization: 1.0
        colorizationColor: root.color
    }
}

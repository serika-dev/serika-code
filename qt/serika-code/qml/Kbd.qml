import QtQuick

Rectangle {
    id: kbd
    property string label: ""
    implicitWidth: Math.max(20, text.implicitWidth + 10)
    implicitHeight: 18
    radius: 4
    color: "transparent"
    border.width: 1
    border.color: Theme.border

    Text {
        id: text
        anchors.centerIn: parent
        text: kbd.label
        color: Theme.textDim
        font.family: Theme.sans
        font.pixelSize: 10
        font.weight: Font.Medium
    }
}

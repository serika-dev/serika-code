import QtQuick

// A small rounded label, e.g. where a server or skill came from.
Rectangle {
    property string text: ""
    property bool accent: false
    implicitWidth: label.implicitWidth + 14
    implicitHeight: 20
    radius: 6
    color: accent ? Theme.accentSoft : Theme.hover
    border.width: 1
    border.color: Theme.borderSubtle
    Text {
        id: label
        anchors.centerIn: parent
        text: parent.text
        color: parent.accent ? Theme.accentSecondary : Theme.textSecondary
        font.family: Theme.sans
        font.pixelSize: 11
        font.weight: Font.Medium
    }
}

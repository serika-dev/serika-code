import QtQuick
import QtQuick.Layouts

// A titled group of settings rows on a rounded surface.
ColumnLayout {
    id: card
    property string title: ""
    property string description: ""
    default property alias rows: body.data
    Layout.fillWidth: true
    spacing: 8

    ColumnLayout {
        visible: card.title.length > 0
        Layout.fillWidth: true
        Layout.leftMargin: 4
        spacing: 3
        Text {
            text: card.title
            color: Theme.text
            font.family: Theme.sans
            font.pixelSize: 14
            font.weight: Font.DemiBold
        }
        Text {
            visible: card.description.length > 0
            Layout.fillWidth: true
            text: card.description
            wrapMode: Text.WordWrap
            color: Theme.textDim
            font.family: Theme.sans
            font.pixelSize: 13
        }
    }
    Rectangle {
        Layout.fillWidth: true
        implicitHeight: body.implicitHeight
        radius: 14
        color: Theme.light ? Theme.raised : Theme.elevated
        border.width: 1
        border.color: Theme.borderSubtle
        clip: true
        ColumnLayout {
            id: body
            width: parent.width
            spacing: 0
        }
    }
}

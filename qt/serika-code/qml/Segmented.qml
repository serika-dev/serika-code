import QtQuick
import QtQuick.Layouts

// A small group of mutually exclusive choices. `options` is a list of { value, label, icon? }.
Rectangle {
    id: seg
    property var options: []
    property string value: ""
    signal picked(string value)

    implicitHeight: 34
    implicitWidth: row.implicitWidth + 6
    radius: 10
    color: Theme.hover
    border.width: 1
    border.color: Theme.borderSubtle

    Row {
        id: row
        anchors.centerIn: parent
        spacing: 2
        Repeater {
            model: seg.options
            delegate: Rectangle {
                required property var modelData
                readonly property bool current: modelData.value === seg.value
                height: seg.height - 6
                width: label.implicitWidth + (modelData.icon ? 40 : 28)
                radius: 7
                color: current ? Theme.raised : (area.containsMouse ? Theme.hover : "transparent")
                border.width: current ? 1 : 0
                border.color: Theme.border
                Row {
                    anchors.centerIn: parent
                    spacing: 6
                    Icon {
                        visible: !!modelData.icon
                        anchors.verticalCenter: parent.verticalCenter
                        name: modelData.icon || ""
                        size: 14
                        color: current ? Theme.text : Theme.textSecondary
                    }
                    Text {
                        id: label
                        anchors.verticalCenter: parent.verticalCenter
                        text: modelData.label
                        color: current ? Theme.text : Theme.textSecondary
                        font.family: Theme.sans
                        font.pixelSize: 13
                        font.weight: current ? Font.Medium : Font.Normal
                    }
                }
                MouseArea {
                    id: area
                    anchors.fill: parent
                    hoverEnabled: true
                    cursorShape: Qt.PointingHandCursor
                    onClicked: seg.picked(modelData.value)
                }
            }
        }
    }
}

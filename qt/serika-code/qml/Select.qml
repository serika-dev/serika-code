import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

// Compact dropdown used in Settings. `options` is a list of { value, label }.
Rectangle {
    id: select
    property var options: []
    property string value: ""
    property real menuWidth: width
    signal picked(string value)

    readonly property string currentLabel: {
        for (var i = 0; i < options.length; ++i)
            if (options[i].value === value) return options[i].label
        return value
    }

    implicitWidth: 220
    implicitHeight: 36
    radius: 8
    color: area.containsMouse || menu.opened ? Theme.active : Theme.hover
    border.width: 1
    border.color: Theme.borderSubtle

    RowLayout {
        anchors.fill: parent
        anchors.leftMargin: 12
        anchors.rightMargin: 10
        Text {
            Layout.fillWidth: true
            text: select.currentLabel
            elide: Text.ElideRight
            color: Theme.text
            font.family: Theme.sans
            font.pixelSize: 14
        }
        Icon { name: "chevron-down"; size: 15; color: Theme.textDim }
    }
    MouseArea {
        id: area
        anchors.fill: parent
        hoverEnabled: true
        cursorShape: Qt.PointingHandCursor
        onClicked: menu.opened ? menu.close() : menu.open()
    }

    Popover {
        id: menu
        y: select.height + 4
        width: Math.max(select.menuWidth, 180)
        height: Math.min(320, list.contentHeight + 8)
        contentItem: ListView {
            id: list
            clip: true
            model: select.options
            boundsBehavior: Flickable.StopAtBounds
            delegate: MenuRow {
                required property var modelData
                width: ListView.view.width
                label: modelData.label
                onTriggered: {
                    select.picked(modelData.value)
                    menu.close()
                }
            }
            ScrollBar.vertical: ScrollBar { policy: ScrollBar.AsNeeded }
        }
    }
}

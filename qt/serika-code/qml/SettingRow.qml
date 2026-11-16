import QtQuick
import QtQuick.Layouts

// One setting: heading and help on the left, the control on the right (or below when `stacked`).
Item {
    id: row
    property string heading: ""
    property string help: ""
    property bool stacked: false
    // A narrow settings page (a snapped window) puts the control under the label.
    readonly property bool narrow: width > 0 && width < 520
    readonly property bool stack: stacked || narrow
    default property alias control: slot.data
    // The first row in a card has no divider above it.
    readonly property bool isFirst: {
        if (!parent) return true
        for (var i = 0; i < parent.children.length; ++i) {
            var sibling = parent.children[i]
            if (sibling.heading !== undefined && sibling.visible)
                return sibling === row
        }
        return true
    }

    Layout.fillWidth: true
    implicitHeight: grid.implicitHeight + 28

    Rectangle {
        visible: !row.isFirst
        anchors.top: parent.top
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.leftMargin: 16
        anchors.rightMargin: 16
        height: 1
        color: Theme.borderSubtle
    }

    GridLayout {
        id: grid
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.verticalCenter: parent.verticalCenter
        anchors.leftMargin: 16
        anchors.rightMargin: 16
        columns: row.stack ? 1 : 2
        columnSpacing: 24
        rowSpacing: 10

        ColumnLayout {
            Layout.fillWidth: true
            Layout.alignment: Qt.AlignVCenter
            spacing: 3
            Text {
                Layout.fillWidth: true
                text: row.heading
                wrapMode: Text.WordWrap
                color: Theme.text
                font.family: Theme.sans
                font.pixelSize: 14
                font.weight: Font.Medium
            }
            Text {
                visible: row.help.length > 0
                Layout.fillWidth: true
                text: row.help
                wrapMode: Text.WordWrap
                color: Theme.textDim
                font.family: Theme.sans
                font.pixelSize: 13
                lineHeight: 1.15
            }
        }
        Item {
            id: slot
            Layout.fillWidth: row.stack
            Layout.alignment: row.stack ? Qt.AlignLeft : (Qt.AlignRight | Qt.AlignVCenter)
            Layout.preferredWidth: row.stack ? -1 : childrenRect.width
            implicitHeight: childrenRect.height
            implicitWidth: childrenRect.width
        }
    }
}

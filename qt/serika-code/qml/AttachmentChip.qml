import QtQuick
import QtQuick.Layouts

// One attachment: a thumbnail for images, a small card for files. With `removable` it shows an x.
Item {
    id: chip
    property var item: ({})
    property bool removable: false
    property real thumb: 72
    signal removeRequested()

    readonly property bool isImage: item.kind === "image"
    implicitWidth: isImage ? thumb : Math.min(260, fileRow.implicitWidth + 28)
    implicitHeight: isImage ? thumb : 46

    // Image
    RoundedImage {
        visible: chip.isImage
        anchors.fill: parent
        source: chip.item.fileUrl || ""
        radius: 12
    }

    // File
    Rectangle {
        visible: !chip.isImage
        anchors.fill: parent
        radius: 12
        color: Theme.hover
        border.width: 1
        border.color: Theme.borderSubtle
        RowLayout {
            id: fileRow
            anchors.verticalCenter: parent.verticalCenter
            anchors.left: parent.left
            anchors.leftMargin: 12
            anchors.right: parent.right
            anchors.rightMargin: chip.removable ? 30 : 12
            spacing: 10
            Rectangle {
                Layout.preferredWidth: 28
                Layout.preferredHeight: 28
                radius: 8
                color: Theme.accentSoft
                Icon { anchors.centerIn: parent; name: chip.item.kind === "text" ? "file-text" : "paperclip"; size: 15; color: Theme.accentSecondary }
            }
            ColumnLayout {
                Layout.fillWidth: true
                spacing: 0
                Text {
                    Layout.fillWidth: true
                    text: chip.item.name || ""
                    elide: Text.ElideMiddle
                    color: Theme.text
                    font.family: Theme.sans
                    font.pixelSize: 13
                }
                Text {
                    text: Format.bytes(chip.item.size || 0)
                    color: Theme.textDim
                    font.family: Theme.sans
                    font.pixelSize: 11
                }
            }
        }
    }

    MouseArea {
        anchors.fill: parent
        enabled: !chip.removable
        cursorShape: Qt.PointingHandCursor
        onClicked: Qt.openUrlExternally(chip.item.fileUrl)
    }

    // Remove
    Rectangle {
        visible: chip.removable
        width: 20
        height: 20
        radius: 10
        anchors.top: parent.top
        anchors.right: parent.right
        anchors.topMargin: chip.isImage ? -6 : 4
        anchors.rightMargin: chip.isImage ? -6 : 4
        color: removeArea.containsMouse ? Theme.danger : Theme.raised
        border.width: 1
        border.color: Theme.border
        Icon { anchors.centerIn: parent; name: "x"; size: 12; color: removeArea.containsMouse ? "white" : Theme.textSecondary }
        MouseArea {
            id: removeArea
            anchors.fill: parent
            hoverEnabled: true
            cursorShape: Qt.PointingHandCursor
            onClicked: chip.removeRequested()
        }
    }
}

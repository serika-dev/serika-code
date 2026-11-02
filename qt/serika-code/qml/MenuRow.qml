import QtQuick
import QtQuick.Layouts

Rectangle {
    id: row
    property string icon: ""
    property string label: ""
    property string kbd: ""
    property bool danger: false
    property bool external: false
    property bool enabledRow: true
    signal triggered()

    implicitHeight: 36
    Layout.fillWidth: true
    radius: 8
    opacity: enabledRow ? 1 : 0.4
    color: (area.containsMouse && enabledRow) ? Theme.active : "transparent"

    RowLayout {
        anchors.fill: parent
        anchors.leftMargin: 10
        anchors.rightMargin: 10
        spacing: 12
        Icon {
            visible: row.icon.length > 0
            name: row.icon
            size: 16
            color: row.danger ? Theme.danger : Theme.textSecondary
        }
        Text {
            Layout.fillWidth: true
            text: row.label
            elide: Text.ElideRight
            color: row.danger ? Theme.danger : Theme.text
            font.family: Theme.sans
            font.pixelSize: 14
        }
        Kbd { visible: row.kbd.length > 0; label: row.kbd }
        Icon { visible: row.external; name: "external-link"; size: 13; color: Theme.textDim }
    }
    MouseArea {
        id: area
        anchors.fill: parent
        hoverEnabled: true
        cursorShape: row.enabledRow ? Qt.PointingHandCursor : Qt.ArrowCursor
        onClicked: if (row.enabledRow) row.triggered()
    }
}

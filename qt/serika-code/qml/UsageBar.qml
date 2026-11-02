import QtQuick
import QtQuick.Layouts

// A usage window: label, percent used, a bar that turns amber and red near the limit, and when it resets.
ColumnLayout {
    id: bar
    property string label: ""
    property real percent: 0
    property string detail: ""
    property string status: ""
    readonly property real clamped: Math.min(100, Math.max(0, percent))
    readonly property color tone: status === "rejected" || clamped >= 95 ? Theme.danger : (clamped >= 75 ? Theme.warning : Theme.accent)
    Layout.fillWidth: true
    spacing: 6

    RowLayout {
        Layout.fillWidth: true
        Text {
            text: bar.label
            color: Theme.textSecondary
            font.family: Theme.sans
            font.pixelSize: 13
        }
        Item { Layout.fillWidth: true }
        Text {
            text: Format.pct(bar.clamped) + "%"
            color: bar.status === "rejected" ? Theme.danger : Theme.text
            font.family: Theme.sans
            font.pixelSize: 13
            font.weight: Font.Medium
        }
    }
    Rectangle {
        Layout.fillWidth: true
        Layout.preferredHeight: 6
        radius: 3
        color: Theme.active
        Rectangle {
            width: Math.max(parent.width * bar.clamped / 100, bar.clamped > 0 ? 6 : 0)
            height: parent.height
            radius: 3
            color: bar.tone
            Behavior on width { NumberAnimation { duration: 300 } }
        }
    }
    Text {
        visible: bar.detail.length > 0
        Layout.fillWidth: true
        text: bar.detail
        color: Theme.textDim
        font.family: Theme.sans
        font.pixelSize: 12
    }
}

import QtQuick
import QtQuick.Controls

// Orange triangle shown wherever a provider's usage limit is in force, with the reset time on hover.
Item {
    id: badge
    property var limit: null
    property real size: 15
    readonly property bool active: limit !== null && limit !== undefined

    visible: active
    implicitWidth: active ? size : 0
    implicitHeight: size
    width: implicitWidth
    height: implicitHeight

    Icon {
        anchors.centerIn: parent
        name: "alert-triangle"
        size: badge.size
        color: Theme.limit
    }
    HoverHandler { id: hover }
    ToolTip.visible: badge.active && hover.hovered
    ToolTip.delay: 250
    ToolTip.text: Format.limitText(badge.limit, ipc.strings)
}

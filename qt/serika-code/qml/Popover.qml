import QtQuick
import QtQuick.Controls

// Floating menu surface (the `.menu-surface` look from SerikaChat).
Popup {
    id: pop
    property real radius: 12
    padding: 4
    margins: 8
    closePolicy: Popup.CloseOnEscape | Popup.CloseOnPressOutside

    background: Rectangle {
        radius: pop.radius
        color: Theme.raised
        border.width: 1
        border.color: Theme.border
    }
    enter: Transition {
        NumberAnimation { property: "opacity"; from: 0; to: 1; duration: 110; easing.type: Easing.OutCubic }
        NumberAnimation { property: "scale"; from: 0.97; to: 1; duration: 110; easing.type: Easing.OutCubic }
    }
    exit: Transition {
        NumberAnimation { property: "opacity"; to: 0; duration: 80 }
    }
}

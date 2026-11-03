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
    // Grows out of the corner it opens from, then settles.
    transformOrigin: Popup.Top
    enter: Transition {
        NumberAnimation { property: "opacity"; from: 0; to: 1; duration: Theme.motionBase; easing.type: Theme.easeOut }
        NumberAnimation { property: "scale"; from: 0.94; to: 1; duration: Theme.motionBase; easing.type: Theme.easeEmphasis }
    }
    exit: Transition {
        NumberAnimation { property: "opacity"; to: 0; duration: Theme.motionFast; easing.type: Easing.InCubic }
        NumberAnimation { property: "scale"; to: 0.97; duration: Theme.motionFast; easing.type: Easing.InCubic }
    }
}

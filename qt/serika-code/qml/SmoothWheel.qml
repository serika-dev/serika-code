import QtQuick

// Smooth, responsive mouse-wheel scrolling for a Flickable or ListView.
// Quick notches add up (and speed up) instead of being dropped, each step glides with an
// ease-out, touchpads scroll pixel for pixel, and at either end the wheel passes to the
// list around it. Use: SmoothWheel { flickable: list }  (it attaches itself on top of it).
Item {
    id: root
    property Flickable flickable: null
    property real step: 110                 // pixels per wheel notch
    property real goal: 0
    property double lastAt: 0
    property real speed: 1
    parent: flickable
    anchors.fill: parent
    z: 1000

    function bounds() {
        const f = flickable
        const top = f.originY - f.topMargin
        const bottom = f.originY + f.contentHeight + f.bottomMargin - f.height
        return { top: top, bottom: Math.max(top, bottom) }
    }

    WheelHandler {
        acceptedDevices: PointerDevice.Mouse | PointerDevice.TouchPad
        onWheel: (event) => {
            const f = root.flickable
            const dy = event.pixelDelta.y !== 0 ? event.pixelDelta.y : event.angleDelta.y
            if (!f || !f.visible || dy === 0) { event.accepted = false; return }
            const b = root.bounds()
            const from = glide.running ? root.goal : f.contentY
            // Already at the end this way: let the surrounding list take it.
            if ((dy > 0 && from <= b.top + 0.5) || (dy < 0 && from >= b.bottom - 0.5)) {
                event.accepted = false
                return
            }
            f.cancelFlick()
            if (event.pixelDelta.y !== 0) {
                glide.stop()
                f.contentY = Math.max(b.top, Math.min(b.bottom, f.contentY - event.pixelDelta.y))
                return
            }
            // Notches close together scroll further, up to 3x.
            const now = Date.now()
            root.speed = now - root.lastAt < 60 ? Math.min(3, root.speed + 0.35) : 1
            root.lastAt = now
            root.goal = Math.max(b.top, Math.min(b.bottom, from - dy / 120 * root.step * root.speed))
            glide.to = root.goal
            glide.restart()
        }
    }

    NumberAnimation {
        id: glide
        target: root.flickable
        property: "contentY"
        duration: 260
        easing.type: Easing.OutCubic
    }

    // A drag or flick by hand takes over from a glide in progress.
    Connections {
        target: root.flickable
        function onMovementStarted() { glide.stop() }
    }
}

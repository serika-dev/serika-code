import QtQuick
import QtQuick.Controls

ListView {
    id: thread
    property bool stick: true
    clip: true
    spacing: 0
    model: ipc.turns
    boundsBehavior: Flickable.StopAtBounds
    cacheBuffer: 2000
    topMargin: 8
    bottomMargin: 24

    delegate: TurnItem {}

    ScrollBar.vertical: ScrollBar { policy: ScrollBar.AsNeeded; width: 8 }

    onContentHeightChanged: if (stick) Qt.callLater(positionViewAtEnd)
    onMovementEnded: stick = atYEnd
    onCountChanged: if (count > 0 && ipc.running) { stick = true }

    Connections {
        target: ipc
        function onTurnsReloaded() { thread.stick = true; Qt.callLater(thread.positionViewAtEnd) }
        function onRunningChanged() { if (ipc.running) thread.stick = true }
    }

    // Wheel up leaves the bottom; the next streamed token will not drag the view back.
    WheelHandler {
        acceptedDevices: PointerDevice.Mouse | PointerDevice.TouchPad
        onWheel: (event) => { if (event.angleDelta.y > 0) thread.stick = false }
    }
}

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
    // Wheel scrolling glides instead of jumping a fixed step.
    flickDeceleration: 3200
    maximumFlickVelocity: 4200

    delegate: TurnItem {}

    // New messages rise into place; the rest glide out of the way.
    add: Transition {
        ParallelAnimation {
            NumberAnimation { property: "opacity"; from: 0; to: 1; duration: Theme.motionSlow; easing.type: Theme.easeOut }
            NumberAnimation { property: "y"; from: ViewTransition.destination.y + 14; duration: Theme.motionSlow; easing.type: Theme.easeEmphasis }
        }
    }
    displaced: Transition { NumberAnimation { properties: "y"; duration: Theme.motionBase; easing.type: Theme.easeOut } }

    ScrollBar.vertical: ScrollBar { policy: ScrollBar.AsNeeded; width: 8 }

    onContentHeightChanged: if (stick) Qt.callLater(positionViewAtEnd)
    onMovementEnded: stick = atYEnd
    onCountChanged: if (count > 0 && ipc.running) { stick = true }

    function glideToEnd() {
        const from = contentY
        positionViewAtEnd()
        const to = contentY
        if (Math.abs(to - from) < 2) return
        contentY = from
        glide.from = from
        glide.to = to
        glide.restart()
        stick = true
    }
    NumberAnimation {
        id: glide
        target: thread
        property: "contentY"
        duration: Theme.motionSlow
        easing.type: Theme.easeEmphasis
        onFinished: thread.positionViewAtEnd()
    }

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

    // Back to the newest message
    Rectangle {
        id: jump
        readonly property bool wanted: !thread.stick && !thread.atYEnd && thread.contentHeight > thread.height
        parent: thread
        anchors.horizontalCenter: parent.horizontalCenter
        y: parent.height - height - (wanted ? 16 : -height)
        z: 10
        width: jumpRow.implicitWidth + 26
        height: 34
        radius: 17
        opacity: wanted ? 1 : 0
        visible: opacity > 0.01
        color: jumpArea.containsMouse ? Theme.active : Theme.raised
        border.width: 1
        border.color: Theme.border
        Behavior on opacity { NumberAnimation { duration: Theme.motionBase; easing.type: Theme.easeOut } }
        Behavior on y { NumberAnimation { duration: Theme.motionBase; easing.type: Theme.easeEmphasis } }
        Behavior on color { ColorAnimation { duration: Theme.motionFast } }
        Row {
            id: jumpRow
            anchors.centerIn: parent
            spacing: 6
            Icon { anchors.verticalCenter: parent.verticalCenter; name: "arrow-down"; size: 14; color: Theme.text }
            Text {
                anchors.verticalCenter: parent.verticalCenter
                text: ipc.running ? Format.tr(ipc.strings, "FollowReply", "Follow reply") : Format.tr(ipc.strings, "JumpToLatest", "Jump to latest")
                color: Theme.text
                font.family: Theme.sans
                font.pixelSize: 12
                font.weight: Font.Medium
            }
        }
        MouseArea {
            id: jumpArea
            anchors.fill: parent
            hoverEnabled: true
            cursorShape: Qt.PointingHandCursor
            onClicked: thread.glideToEnd()
        }
    }
}

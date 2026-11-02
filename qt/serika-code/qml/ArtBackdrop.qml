import QtQuick

// Random serika.art artwork behind sign-in and welcome, with the same violet glow
// SerikaChat uses while the picture loads (or if the host is unreachable).
Item {
    id: root
    property bool dimBottom: true
    clip: true

    Rectangle { anchors.fill: parent; color: "black" }
    Rectangle {
        x: -96
        y: parent.height * 0.25
        width: 384
        height: 384
        radius: 192
        color: "#408b5cf6"
        opacity: 0.55
    }
    Rectangle {
        x: parent.width - 288
        y: parent.height - 288
        width: 384
        height: 384
        radius: 192
        color: "#30f43f5e"
        opacity: 0.5
    }

    Image {
        id: art
        anchors.fill: parent
        source: Format.sizedArt(ipc.artUrl, 1800, 1200)
        fillMode: Image.PreserveAspectCrop
        asynchronous: true
        cache: true
        smooth: true
        opacity: status === Image.Ready ? 1 : 0
        Behavior on opacity { NumberAnimation { duration: 700 } }
    }

    Rectangle {
        anchors.fill: parent
        gradient: Gradient {
            GradientStop { position: 0.0; color: "#66000000" }
            GradientStop { position: 0.55; color: "#33000000" }
            GradientStop { position: 1.0; color: root.dimBottom ? "#cc000000" : "#40000000" }
        }
    }

    Rectangle {
        visible: ipc.artArtist.length > 0 && art.status === Image.Ready
        anchors.left: parent.left
        anchors.bottom: parent.bottom
        anchors.margins: 20
        height: 26
        width: credit.implicitWidth + 24
        radius: 13
        color: "#80000000"
        border.width: 1
        border.color: "#1affffff"

        Text {
            id: credit
            anchors.centerIn: parent
            text: Format.t(ipc.strings, "ArtBy", { name: ipc.artArtist })
            color: "#d4d4d8"
            font.family: Theme.sans
            font.pixelSize: 11
        }
    }
}

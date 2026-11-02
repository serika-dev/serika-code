import QtQuick

// Window controls (minimize, maximize, close). Dragging happens on the strip in Main.qml.
Row {
    id: controls
    required property var host
    readonly property bool maximized: host.visibility === Window.Maximized
    height: 36
    spacing: 0

    component ChromeButton: Rectangle {
        id: button
        property string glyph: "x"
        property bool danger: false
        signal clicked()
        width: 46
        height: 36
        color: area.containsMouse ? (danger ? "#e11d48" : Theme.active) : "transparent"

        Icon {
            anchors.centerIn: parent
            name: button.glyph
            size: 14
            color: (button.danger && area.containsMouse) ? "white" : Theme.textSecondary
        }
        MouseArea {
            id: area
            anchors.fill: parent
            hoverEnabled: true
            onClicked: button.clicked()
        }
    }

    ChromeButton { glyph: "minus"; onClicked: controls.host.showMinimized() }
    ChromeButton {
        glyph: controls.maximized ? "copy" : "square"
        onClicked: controls.maximized ? controls.host.showNormal() : controls.host.showMaximized()
    }
    ChromeButton { glyph: "x"; danger: true; onClicked: controls.host.close() }
}

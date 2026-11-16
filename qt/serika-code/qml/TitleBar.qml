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
        property string role: ""
        property bool danger: false
        signal clicked()
        // 46 by 36, three across the top-right. windowchrome.cpp hit-tests these same sizes.
        width: 46
        height: 36
        readonly property bool hot: winChrome !== null && winChrome.hot === role
        color: (area.containsMouse || hot) ? (danger ? "#e11d48" : Theme.active) : "transparent"

        Icon {
            anchors.centerIn: parent
            name: button.glyph
            size: 14
            color: (button.danger && (area.containsMouse || button.hot)) ? "white" : Theme.textSecondary
        }
        MouseArea {
            id: area
            anchors.fill: parent
            hoverEnabled: true
            // On Windows the caption buttons are real window buttons, so the system
            // handles the click (that is what opens snap layouts). Elsewhere, we do.
            onClicked: if (winChrome === null || !winChrome.active) button.clicked()
        }
    }

    ChromeButton { role: "min"; glyph: "minus"; onClicked: controls.host.showMinimized() }
    ChromeButton {
        role: "max"
        glyph: controls.maximized ? "copy" : "square"
        onClicked: controls.maximized ? controls.host.showNormal() : controls.host.showMaximized()
    }
    ChromeButton { role: "close"; glyph: "x"; danger: true; onClicked: controls.host.close() }
}

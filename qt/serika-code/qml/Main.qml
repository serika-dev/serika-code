import QtQuick
import QtQuick.Controls

ApplicationWindow {
    id: win
    visible: true
    width: 1420
    height: 880
    minimumWidth: 940
    minimumHeight: 620
    flags: Qt.Window | Qt.FramelessWindowHint
    color: Theme.bg
    title: "Serika Code"

    readonly property bool maximized: visibility === Window.Maximized
    readonly property var prefs: ipc.ui

    // Saved appearance preferences drive the theme. --theme and --preset are for screenshots.
    readonly property string presetId: {
        if (startPreset.length > 0) return startPreset
        if (forcedTheme.length > 0) return forcedTheme === "light" ? "serika-light" : "serika-dark"
        if (prefs.themePreset) return prefs.themePreset
        return ipc.theme === "light" ? "serika-light" : "serika-dark"
    }
    Binding { target: Theme; property: "preset"; value: win.presetId }
    Binding { target: Theme; property: "accentOverride"; value: win.prefs.accent ? win.prefs.accent : "" }
    Binding { target: Theme; property: "chatSize"; value: win.prefs.chatSize ? win.prefs.chatSize : "medium" }
    Binding { target: Theme; property: "density"; value: win.prefs.density ? win.prefs.density : "comfortable" }
    Binding { target: Theme; property: "chatWidth"; value: win.prefs.chatWidth ? win.prefs.chatWidth : "default" }
    Binding { target: Theme; property: "reduceMotion"; value: win.prefs.reduceMotion === true }
    Binding { target: Theme; property: "systemDark"; value: Qt.styleHints.colorScheme !== Qt.ColorScheme.Light }

    readonly property string page: {
        if (startPage === "login") return "login"
        if (!ipc.accountLoaded) return ipc.bridgeError.length > 0 ? "error" : "loading"
        if (!ipc.signedIn) return "login"
        return "app"
    }

    // Drag strip. It sits under the pages, so buttons and menus on top still win.
    Item {
        id: dragStrip
        z: -1
        anchors.top: parent.top
        anchors.left: parent.left
        anchors.right: parent.right
        height: 36
        DragHandler {
            target: null
            onActiveChanged: if (active) win.startSystemMove()
        }
        TapHandler {
            acceptedButtons: Qt.LeftButton
            onDoubleTapped: win.maximized ? win.showNormal() : win.showMaximized()
        }
    }

    Loader {
        anchors.fill: parent
        active: win.page === "login"
        sourceComponent: Component { LoginPage {} }
    }
    Loader {
        anchors.fill: parent
        active: win.page === "app"
        sourceComponent: Component { ShellPage {} }
    }

    Item {
        anchors.fill: parent
        visible: win.page === "loading" || win.page === "error"
        Column {
            anchors.centerIn: parent
            spacing: 10
            Text {
                anchors.horizontalCenter: parent.horizontalCenter
                text: "serika"
                color: Theme.text
                font.family: Theme.brand
                font.pixelSize: 30
                font.weight: Font.DemiBold
            }
            Text {
                anchors.horizontalCenter: parent.horizontalCenter
                width: Math.min(440, win.width - 80)
                horizontalAlignment: Text.AlignHCenter
                wrapMode: Text.WordWrap
                visible: win.page === "error"
                text: ipc.bridgeError
                color: Theme.danger
                font.family: Theme.sans
                font.pixelSize: 13
            }
        }
    }

    // Short messages that are not part of a chat (a failed refresh, a provider hint).
    Rectangle {
        id: toast
        property string message: ""
        z: 200
        anchors.horizontalCenter: parent.horizontalCenter
        anchors.bottom: parent.bottom
        anchors.bottomMargin: 28
        width: Math.min(520, parent.width - 48)
        height: toastText.implicitHeight + 24
        radius: 12
        color: Theme.raised
        border.width: 1
        border.color: Theme.border
        opacity: message.length > 0 ? 1 : 0
        visible: opacity > 0
        Behavior on opacity { NumberAnimation { duration: 160 } }
        Row {
            anchors.fill: parent
            anchors.margins: 12
            spacing: 10
            Icon { name: "info"; size: 16; color: Theme.accentSecondary; anchors.verticalCenter: parent.verticalCenter }
            Text {
                id: toastText
                width: parent.width - 30
                anchors.verticalCenter: parent.verticalCenter
                wrapMode: Text.WordWrap
                text: toast.message
                color: Theme.text
                font.family: Theme.sans
                font.pixelSize: 13
            }
        }
        MouseArea { anchors.fill: parent; onClicked: toast.message = "" }
        Timer { id: toastTimer; interval: 6000; onTriggered: toast.message = "" }
        Connections {
            target: ipc
            function onNoticeChanged() {
                toast.message = ipc.notice
                if (ipc.notice.length > 0) toastTimer.restart()
            }
        }
    }

    TitleBar {
        z: 100
        anchors.top: parent.top
        anchors.right: parent.right
        host: win
    }

    // Resize handles around the frameless window.
    component Edge: MouseArea {
        property int edges: Qt.LeftEdge
        property int shape: Qt.SizeHorCursor
        z: 1000
        hoverEnabled: true
        visible: !win.maximized
        cursorShape: shape
        onPressed: win.startSystemResize(edges)
    }
    Edge { edges: Qt.LeftEdge; shape: Qt.SizeHorCursor; anchors.left: parent.left; anchors.top: parent.top; anchors.bottom: parent.bottom; anchors.topMargin: 8; anchors.bottomMargin: 8; width: 5 }
    Edge { edges: Qt.RightEdge; shape: Qt.SizeHorCursor; anchors.right: parent.right; anchors.top: parent.top; anchors.bottom: parent.bottom; anchors.topMargin: 8; anchors.bottomMargin: 8; width: 5 }
    Edge { edges: Qt.TopEdge; shape: Qt.SizeVerCursor; anchors.top: parent.top; anchors.left: parent.left; anchors.right: parent.right; anchors.leftMargin: 8; anchors.rightMargin: 8; height: 4 }
    Edge { edges: Qt.BottomEdge; shape: Qt.SizeVerCursor; anchors.bottom: parent.bottom; anchors.left: parent.left; anchors.right: parent.right; anchors.leftMargin: 8; anchors.rightMargin: 8; height: 5 }
    Edge { edges: Qt.LeftEdge | Qt.TopEdge; shape: Qt.SizeFDiagCursor; anchors.left: parent.left; anchors.top: parent.top; width: 8; height: 8 }
    Edge { edges: Qt.RightEdge | Qt.TopEdge; shape: Qt.SizeBDiagCursor; anchors.right: parent.right; anchors.top: parent.top; width: 8; height: 8 }
    Edge { edges: Qt.LeftEdge | Qt.BottomEdge; shape: Qt.SizeBDiagCursor; anchors.left: parent.left; anchors.bottom: parent.bottom; width: 8; height: 8 }
    Edge { edges: Qt.RightEdge | Qt.BottomEdge; shape: Qt.SizeFDiagCursor; anchors.right: parent.right; anchors.bottom: parent.bottom; width: 8; height: 8 }
}

import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

Item {
    id: header
    property bool browserShown: false
    property bool changesShown: false
    readonly property bool inspectorOpen: browserShown || changesShown
    property real rightInset: 12
    signal exportRequested()
    signal inspectorRequested()
    signal browserRequested()
    signal providerSetupRequested(string provider)
    signal shortcutsRequested()
    readonly property var dict: ipc.strings

    implicitHeight: 52
    function openModels() { modelPopover.open() }

    // Limit of the provider behind the selected model, rechecked every 30 s so it clears at reset.
    property real now: Date.now()
    Timer { interval: 30000; running: true; repeat: true; onTriggered: header.now = Date.now() }
    readonly property var limit: {
        const provider = ipc.selectedProvider
        if (provider === "serika")
            return Format.serikaLimit(ipc.weeklyPercent, ipc.weeklyResetsAt, ipc.sessionPercent, ipc.sessionResetsAt)
        for (let i = 0; i < ipc.providers.length; ++i)
            if (ipc.providers[i].id === provider)
                return Format.providerLimit(ipc.providers[i], header.now)
        return null
    }
    function openChatMenu() { chatMenu.open() }

    // Model selector (frameless text button, as in SerikaChat)
    Rectangle {
        id: modelButton
        anchors.left: parent.left
        anchors.leftMargin: 12
        anchors.verticalCenter: parent.verticalCenter
        height: 36
        width: Math.min(modelRow.implicitWidth + 24, 320)
        radius: 8
        color: (modelArea.containsMouse || modelPopover.opened) ? Theme.hover : "transparent"

        RowLayout {
            id: modelRow
            anchors.fill: parent
            anchors.leftMargin: 10
            anchors.rightMargin: 8
            spacing: 6
            ProviderLogo {
                visible: ipc.selectedProvider !== "serika"
                provider: ipc.selectedProvider
                size: 18
            }
            Text {
                Layout.fillWidth: true
                text: ipc.selectedModelName
                elide: Text.ElideRight
                color: Theme.text
                font.family: Theme.sans
                font.pixelSize: 15
                font.weight: Font.Medium
            }
            LimitBadge { limit: header.limit; size: 15 }
            Icon {
                name: "chevron-down"
                size: 16
                color: Theme.textDim
                rotation: modelPopover.opened ? 180 : 0
                Behavior on rotation { NumberAnimation { duration: 150 } }
            }
        }
        MouseArea {
            id: modelArea
            anchors.fill: parent
            hoverEnabled: true
            cursorShape: Qt.PointingHandCursor
            onClicked: modelPopover.opened ? modelPopover.close() : modelPopover.open()
        }
        ModelPopover {
            id: modelPopover
            y: modelButton.height + 6
            onProviderSetupRequested: (provider) => header.providerSetupRequested(provider)
        }
    }

    // Chat title, centered between the controls
    Text {
        id: title
        visible: ipc.activeSession.length > 0
        anchors.centerIn: parent
        width: Math.min(parent.width - modelButton.width - header.rightInset - 260, 420)
        horizontalAlignment: Text.AlignHCenter
        elide: Text.ElideRight
        text: {
            for (var i = 0; i < ipc.sessions.length; ++i)
                if (ipc.sessions[i].id === ipc.activeSession) return ipc.sessions[i].title
            return ""
        }
        color: Theme.textSecondary
        font.family: Theme.sans
        font.pixelSize: 14
    }

    Row {
        anchors.right: parent.right
        anchors.rightMargin: header.rightInset
        anchors.verticalCenter: parent.verticalCenter
        spacing: 4

        IconButton {
            icon: "globe"
            tip: Format.tr(header.dict, "Browser", "Browser") + "  (Ctrl Shift B)"
            active: header.browserShown
            onClicked: header.browserRequested()
        }
        IconButton {
            icon: "code"
            tip: Format.t(header.dict, "DiffsInspector") + "  (Ctrl B)"
            active: header.changesShown
            onClicked: header.inspectorRequested()
        }
        IconButton {
            id: moreButton
            icon: "more-horizontal"
            iconSize: 20
            tip: Format.t(header.dict, "ChatOptions")
            active: chatMenu.opened
            onClicked: chatMenu.opened ? chatMenu.close() : chatMenu.open()

            ChatMenu {
                id: chatMenu
                x: moreButton.width - width
                y: moreButton.height + 6
                inspectorOpen: header.inspectorOpen
                onExportRequested: header.exportRequested()
                onInspectorRequested: header.inspectorRequested()
                onShortcutsRequested: header.shortcutsRequested()
            }
        }
    }
}

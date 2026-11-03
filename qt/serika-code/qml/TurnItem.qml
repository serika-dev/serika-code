import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

// One turn in the thread: the user's bubble, or an unboxed assistant reply.
Item {
    id: item
    required property int index
    required property string turn
    required property string content
    required property string reasoning
    required property var tools
    required property bool streaming
    required property var attachments
    required property string replyModel
    required property string switchFrom
    required property string switchTo

    readonly property bool isUser: turn === "user"
    readonly property bool isSwitch: turn === "switch"
    function providerOf(id) {
        if (id.indexOf("claude-code/") === 0) return "claude"
        if (id.indexOf("codex/") === 0) return "codex"
        return "serika"
    }
    readonly property real laneWidth: Math.min(width - 48, Theme.laneMax)
    readonly property bool waiting: streaming && content.length === 0 && reasoning.length === 0 && tools.length === 0
    readonly property bool hovered: hover.hovered || menu.opened
    readonly property bool idle: !ipc.running && !streaming
    readonly property var dict: ipc.strings
    property bool confirmDelete: false

    width: ListView.view ? ListView.view.width : 600
    implicitHeight: isSwitch ? switchRow.implicitHeight + 28 : lane.implicitHeight + (index === 0 ? 8 : 22)

    // Where the chat moved to another model
    RowLayout {
        id: switchRow
        visible: item.isSwitch
        x: (item.width - width) / 2
        y: 20
        width: item.laneWidth
        spacing: 10
        Rectangle { Layout.fillWidth: true; Layout.preferredHeight: 1; color: Theme.borderSubtle }
        ProviderLogo { provider: item.providerOf(item.switchFrom); size: 14; opacity: 0.7 }
        Icon { name: "arrow-right"; size: 12; color: Theme.textDim }
        ProviderLogo { provider: item.providerOf(item.switchTo); size: 14 }
        Text {
            text: Format.tr(item.dict, "SwitchedTo", "Switched to {to}", { to: ipc.modelName(item.switchTo) })
            color: Theme.textSecondary
            font.family: Theme.sans
            font.pixelSize: 12
            font.weight: Font.Medium
        }
        Text {
            visible: Number(item.content) > 0
            text: "· " + Format.tr(item.dict, "HandedOver", "{n} earlier messages handed over", { n: item.content })
            color: Theme.textDim
            font.family: Theme.sans
            font.pixelSize: 12
        }
        Rectangle { Layout.fillWidth: true; Layout.preferredHeight: 1; color: Theme.borderSubtle }
    }

    HoverHandler { id: hover }
    TapHandler {
        acceptedButtons: Qt.RightButton
        onTapped: (point) => {
            if (item.streaming) return
            menu.x = Math.min(point.position.x, item.width - menu.width - 8)
            menu.y = point.position.y
            menu.open()
        }
    }
    Timer { id: deleteReset; interval: 3000; onTriggered: item.confirmDelete = false }
    function askDelete() {
        if (item.confirmDelete) { item.confirmDelete = false; ipc.deleteTurn(item.index) }
        else { item.confirmDelete = true; deleteReset.restart() }
    }

    component ActionButton: IconButton {
        implicitWidth: 28
        implicitHeight: 28
        iconSize: 15
    }

    // Right-click menu with every action for this turn
    Popover {
        id: menu
        width: 230
        contentItem: ColumnLayout {
            spacing: 0
            MenuRow {
                visible: item.isUser
                icon: "edit-2"
                label: Format.tr(item.dict, "EditMessage", "Edit and resend")
                enabledRow: item.idle
                onTriggered: { menu.close(); ipc.editMessage(item.index) }
            }
            MenuRow {
                visible: !item.isUser
                icon: "refresh-cw"
                label: Format.tr(item.dict, "Retry", "Retry")
                enabledRow: item.idle
                onTriggered: { menu.close(); ipc.retryTurn(item.index) }
            }
            MenuRow {
                visible: !item.isUser
                icon: "rotate-ccw"
                label: Format.tr(item.dict, "RewindHere", "Rewind to here")
                enabledRow: item.idle && ipc.canRewindTo(item.index)
                onTriggered: { menu.close(); ipc.rewindTo(item.index) }
            }
            MenuRow {
                icon: "git-branch"
                label: Format.tr(item.dict, "ForkHere", "Fork chat from here")
                enabledRow: item.idle
                onTriggered: { menu.close(); ipc.forkFrom(item.index) }
            }
            MenuRow {
                icon: "copy"
                label: Format.tr(item.dict, "CopyMarkdown", "Copy as Markdown")
                enabledRow: item.content.length > 0
                onTriggered: { menu.close(); ipc.copyText(item.content) }
            }
            MenuDivider {}
            MenuRow {
                icon: "trash-2"
                danger: true
                label: item.isUser ? Format.tr(item.dict, "DeleteExchange", "Delete message and reply")
                                   : Format.tr(item.dict, "DeleteReply", "Delete reply and its prompt")
                enabledRow: item.idle
                onTriggered: { menu.close(); ipc.deleteTurn(item.index) }
            }
        }
    }

    Item {
        id: lane
        visible: !item.isSwitch
        x: (item.width - width) / 2
        y: index === 0 ? 0 : 22
        width: item.laneWidth
        implicitHeight: item.isUser ? userBlock.implicitHeight : assistant.implicitHeight

        // User message: attachments above the bubble
        ColumnLayout {
            id: userBlock
            visible: item.isUser
            anchors.right: parent.right
            width: lane.width
            spacing: 8

            Flow {
                visible: item.attachments.length > 0
                Layout.fillWidth: true
                layoutDirection: Qt.RightToLeft
                spacing: 8
                Repeater {
                    model: item.attachments
                    delegate: AttachmentChip {
                        required property var modelData
                        item: modelData
                        thumb: 168
                    }
                }
            }

        RowLayout {
            Layout.alignment: Qt.AlignRight
            spacing: 6
            // Actions for your message, shown on hover beside the bubble
            Row {
                Layout.alignment: Qt.AlignBottom
                spacing: 0
                opacity: item.hovered && !item.streaming ? 1 : 0
                visible: opacity > 0
                Behavior on opacity { NumberAnimation { duration: 120 } }
                ActionButton {
                    icon: "edit-2"
                    tip: Format.tr(item.dict, "EditMessage", "Edit and resend")
                    opacity: item.idle ? 1 : 0.4
                    onClicked: if (item.idle) ipc.editMessage(item.index)
                }
                ActionButton {
                    id: userCopy
                    property bool copied: false
                    icon: copied ? "check" : "copy"
                    tip: Format.t(item.dict, copied ? "Copied" : "Copy")
                    onClicked: { ipc.copyText(item.content); copied = true; userCopyReset.restart() }
                    Timer { id: userCopyReset; interval: 1500; onTriggered: userCopy.copied = false }
                }
                ActionButton {
                    icon: "more-horizontal"
                    tip: Format.tr(item.dict, "MoreActions", "More")
                    onClicked: { menu.x = Math.max(8, lane.x + lane.width - menu.width); menu.y = lane.y + lane.height + 4; menu.open() }
                }
            }
        Rectangle {
            id: bubble
            visible: item.content.length > 0
            Layout.alignment: Qt.AlignRight
            // Long messages (a pasted log or email) show a preview until you open them.
            readonly property real foldedHeight: Theme.chatPx * 1.45 * 14
            readonly property bool isLong: userText.implicitHeight > foldedHeight + Theme.chatPx * 4
            readonly property bool expanded: item.ListView.view ? !!item.ListView.view.openTurns[item.index] : false
            readonly property real textHeight: isLong && !expanded ? foldedHeight : userText.implicitHeight
            implicitWidth: Math.min(lane.width * 0.85, userText.implicitWidth + 32)
            implicitHeight: textHeight + 24 + (isLong ? 36 : 0)
            Behavior on implicitHeight { enabled: bubble.isLong; NumberAnimation { duration: Theme.motionBase; easing.type: Theme.easeEmphasis } }
            radius: 18
            color: Theme.raised
            border.width: Theme.light ? 1 : 0
            border.color: Theme.border

            Item {
                id: textClip
                x: 16
                y: 12
                width: userText.width
                height: bubble.textHeight
                clip: bubble.isLong

            TextEdit {
                id: userText
                width: Math.min(lane.width * 0.85 - 32, implicitWidth)
                text: item.isUser ? item.content : ""
                // Always plain: an email header like "Name <a@b.c>" must not be read as HTML.
                textFormat: TextEdit.PlainText
                readOnly: true
                selectByMouse: true
                selectionColor: Theme.accent
                wrapMode: TextEdit.Wrap
                color: Theme.text
                font.family: Theme.sans
                font.pixelSize: Theme.chatPx
            }
            }
            // Fade the cut edge of a folded message
            Rectangle {
                visible: bubble.isLong && !bubble.expanded
                x: 1
                y: 12 + bubble.foldedHeight - height
                width: parent.width - 2
                height: Theme.chatPx * 4
                gradient: Gradient {
                    GradientStop { position: 0; color: Qt.rgba(Theme.raised.r, Theme.raised.g, Theme.raised.b, 0) }
                    GradientStop { position: 1; color: Theme.raised }
                }
            }
            Rectangle {
                visible: bubble.isLong
                anchors.horizontalCenter: parent.horizontalCenter
                anchors.bottom: parent.bottom
                anchors.bottomMargin: 10
                width: foldRow.implicitWidth + 22
                height: 26
                radius: 13
                color: foldArea.containsMouse ? Theme.active : Theme.hover
                border.width: 1
                border.color: Theme.borderSubtle
                Behavior on color { ColorAnimation { duration: Theme.motionFast } }
                Row {
                    id: foldRow
                    anchors.centerIn: parent
                    spacing: 6
                    Icon {
                        anchors.verticalCenter: parent.verticalCenter
                        name: "chevron-down"
                        size: 12
                        color: Theme.textSecondary
                        rotation: bubble.expanded ? 180 : 0
                        Behavior on rotation { NumberAnimation { duration: Theme.motionBase; easing.type: Theme.easeEmphasis } }
                    }
                    Text {
                        anchors.verticalCenter: parent.verticalCenter
                        text: bubble.expanded ? Format.tr(item.dict, "ShowLess", "Show less")
                                              : Format.tr(item.dict, "ShowAllLines", "Show all {n} lines", { n: userText.lineCount })
                        color: Theme.textSecondary
                        font.family: Theme.sans
                        font.pixelSize: 12
                        font.weight: Font.Medium
                    }
                }
                MouseArea {
                    id: foldArea
                    anchors.fill: parent
                    hoverEnabled: true
                    cursorShape: Qt.PointingHandCursor
                    onClicked: item.ListView.view.toggleTurn(item.index)
                }
            }
        }
        }
        }

        // Assistant message
        ColumnLayout {
            id: assistant
            visible: !item.isUser
            width: parent.width
            spacing: 12

            ReasoningTrace {
                Layout.fillWidth: true
                visible: item.reasoning.length > 0
                text: item.reasoning
                streaming: item.streaming
                autoOpen: item.streaming && item.content.length === 0
            }
            ToolList {
                Layout.fillWidth: true
                visible: item.tools.length > 0
                tools: item.tools
            }
            MarkdownView {
                Layout.fillWidth: true
                visible: item.content.length > 0
                text: item.content
            }

            // Waiting for the first token
            Row {
                visible: item.waiting
                spacing: 5
                Repeater {
                    model: 3
                    delegate: Rectangle {
                        required property int index
                        width: 7
                        height: 7
                        radius: 3.5
                        color: Theme.accentSecondary
                        opacity: 0.3
                        SequentialAnimation on opacity {
                            running: item.waiting
                            loops: Animation.Infinite
                            PauseAnimation { duration: index * 160 }
                            NumberAnimation { to: 1; duration: 320 }
                            NumberAnimation { to: 0.3; duration: 320 }
                            PauseAnimation { duration: (2 - index) * 160 }
                        }
                    }
                }
            }

            Row {
                visible: !item.streaming && (item.content.length > 0 || item.tools.length > 0)
                // Room between the reply and its model / copy row.
                Layout.topMargin: 6
                spacing: 2
                // Which model wrote this reply
                Row {
                    visible: item.replyModel.length > 0
                    anchors.verticalCenter: parent.verticalCenter
                    rightPadding: 6
                    spacing: 5
                    ProviderLogo { anchors.verticalCenter: parent.verticalCenter; provider: item.providerOf(item.replyModel); size: 13 }
                    Text {
                        anchors.verticalCenter: parent.verticalCenter
                        text: ipc.modelName(item.replyModel)
                        color: Theme.textDim
                        font.family: Theme.sans
                        font.pixelSize: 12
                    }
                }
                ActionButton {
                    id: copyButton
                    property bool copied: false
                    visible: item.content.length > 0
                    icon: copied ? "check" : "copy"
                    tip: Format.t(ipc.strings, copied ? "Copied" : "Copy")
                    onClicked: {
                        ipc.copyText(item.content)
                        copied = true
                        resetCopy.restart()
                    }
                    Timer { id: resetCopy; interval: 1500; onTriggered: copyButton.copied = false }
                }
                // The rest appear on hover
                Row {
                    spacing: 2
                    opacity: item.hovered ? 1 : 0
                    Behavior on opacity { NumberAnimation { duration: 120 } }
                    ActionButton {
                        icon: "refresh-cw"
                        tip: Format.tr(item.dict, "Retry", "Retry")
                        opacity: item.idle ? 1 : 0.4
                        onClicked: if (item.idle) ipc.retryTurn(item.index)
                    }
                    ActionButton {
                        visible: ipc.canRewindTo(item.index) || ipc.turns.count > 0
                        icon: "rotate-ccw"
                        tip: Format.tr(item.dict, "RewindHereHelp", "Rewind to here: undo later messages and their file changes")
                        opacity: item.idle && ipc.canRewindTo(item.index) ? 1 : 0.4
                        onClicked: if (item.idle && ipc.canRewindTo(item.index)) ipc.rewindTo(item.index)
                    }
                    ActionButton {
                        icon: "git-branch"
                        tip: Format.tr(item.dict, "ForkHere", "Fork chat from here")
                        opacity: item.idle ? 1 : 0.4
                        onClicked: if (item.idle) ipc.forkFrom(item.index)
                    }
                    ActionButton {
                        icon: "trash-2"
                        danger: item.confirmDelete
                        tip: item.confirmDelete ? Format.tr(item.dict, "ClickToConfirm", "Click again to delete")
                                                : Format.tr(item.dict, "DeleteReply", "Delete reply and its prompt")
                        opacity: item.idle ? 1 : 0.4
                        onClicked: if (item.idle) item.askDelete()
                    }
                }
            }
        }
    }
}

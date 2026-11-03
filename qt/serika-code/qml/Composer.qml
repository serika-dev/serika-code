import QtQuick
import QtQuick.Controls
import QtQuick.Dialogs
import QtQuick.Layouts

// Message box with the Search and Research pills and a round send button.
Rectangle {
    id: composer
    property alias text: input.text
    signal folderRequested()
    signal browserRequested()
    readonly property var dict: ipc.strings
    readonly property bool ctrlToSend: ipc.ui.sendKey === "ctrlenter"
    readonly property var modes: [
        { id: "restricted", label: Format.tr(dict, "ModeSupervised", "Supervised"), icon: "shield", help: Format.tr(dict, "ModeSupervisedHelp", "Asks before every edit and command") },
        { id: "standard", label: Format.tr(dict, "ModeAutoEdits", "Auto-accept edits"), icon: "edit-2", help: Format.tr(dict, "ModeAutoEditsHelp", "Edits and dev commands run, risky ones ask") },
        { id: "full", label: Format.tr(dict, "ModeFull", "Full access"), icon: "zap", help: Format.tr(dict, "ModeFullHelp", "Runs without asking, except destructive actions") },
        { id: "read_only", label: Format.tr(dict, "ModeReadOnly", "Read-only"), icon: "eye", help: Format.tr(dict, "ModeReadOnlyHelp", "Can look around, cannot change anything") }
    ]
    readonly property var currentMode: {
        for (var i = 0; i < modes.length; ++i)
            if (modes[i].id === ipc.permissionMode) return modes[i]
        return modes[1]
    }
    // Effort levels of the selected Claude Code or Codex model, as its CLI reports them.
    readonly property var agentModel: {
        for (let i = 0; i < ipc.providers.length; ++i) {
            const models = ipc.providers[i].models || []
            for (let j = 0; j < models.length; ++j)
                if (models[j].id === ipc.selectedModel) return models[j]
        }
        return null
    }
    readonly property var efforts: agentModel ? (agentModel.efforts || []) : []
    readonly property string effortSetting: ipc.selectedProvider === "codex" ? "codex_effort" : "claude_effort"
    readonly property string effort: {
        const saved = ipc.settings[ipc.selectedProvider === "codex" ? "codexEffort" : "claudeEffort"] || ""
        return efforts.indexOf(saved) >= 0 ? saved : ""
    }
    function effortLabel(level) {
        if (!level) return Format.tr(dict, "EffortDefault", "Default")
        if (level === "xhigh") return Format.tr(dict, "EffortXHigh", "Extra high")
        return Format.tr(dict, "Effort_" + level, level.charAt(0).toUpperCase() + level.substring(1))
    }
    readonly property bool hasText: input.text.trim().length > 0 || ipc.attachments.length > 0
    function t(key) { return Format.t(dict, key) }
    function focusInput() { input.forceActiveFocus() }
    // Unsent text is kept per chat (and makes a new chat a draft in the sidebar).
    property bool filling: false
    Timer { id: draftTimer; interval: 250; onTriggered: ipc.setDraftText(input.text) }
    // While a reply runs, sending adds the message to the queue.
    function submit() {
        if (!hasText) return
        ipc.send(input.text)
        input.text = ""
    }
    // The limit that stopped the queue, if one is in force, so it can resume at the reset.
    readonly property var queueLimit: {
        if (ipc.selectedProvider === "serika")
            return Format.serikaLimit(ipc.weeklyPercent, ipc.weeklyResetsAt, ipc.sessionPercent, ipc.sessionResetsAt)
        for (let i = 0; i < ipc.providers.length; ++i)
            if (ipc.providers[i].id === ipc.selectedProvider) return Format.providerLimit(ipc.providers[i])
        return null
    }

    Connections {
        target: ipc
        function onComposerFill(text) {
            composer.filling = true
            input.text = text
            composer.filling = false
            input.cursorPosition = text.length
            input.forceActiveFocus()
        }
    }

    FileDialog {
        id: attachDialog
        title: Format.tr(composer.dict, "AttachFiles", "Attach files or images")
        fileMode: FileDialog.OpenFiles
        onAccepted: { ipc.addAttachments(selectedFiles); composer.focusInput() }
    }

    implicitHeight: column.implicitHeight + 24
    radius: 24
    color: Theme.composer
    border.width: 1
    border.color: ipc.running ? Theme.accent : (input.activeFocus ? Theme.border : Theme.borderSubtle)
    Behavior on border.color { ColorAnimation { duration: Theme.motionBase } }

    // A soft accent halo while you type, and a slow pulse while a reply runs.
    Rectangle {
        z: -1
        anchors.fill: parent
        anchors.margins: -4
        radius: parent.radius + 4
        color: "transparent"
        border.width: 4
        border.color: Qt.rgba(Theme.accent.r, Theme.accent.g, Theme.accent.b, ipc.running ? 0.22 : 0.14)
        opacity: input.activeFocus || ipc.running ? 1 : 0
        Behavior on opacity { NumberAnimation { duration: Theme.motionBase; easing.type: Theme.easeOut } }
        SequentialAnimation on border.width {
            running: ipc.running
            loops: Animation.Infinite
            NumberAnimation { to: 6; duration: 900; easing.type: Easing.InOutSine }
            NumberAnimation { to: 3; duration: 900; easing.type: Easing.InOutSine }
        }
    }

    ColumnLayout {
        id: column
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.top: parent.top
        anchors.margins: 12
        anchors.leftMargin: 14
        anchors.rightMargin: 12
        spacing: 6

        // The chat continues on another model: say so before the message goes out.
        RowLayout {
            id: switchHint
            readonly property string previous: { ipc.turns.count; ipc.running; return ipc.lastReplyModel() }
            visible: previous.length > 0 && previous !== ipc.selectedModel && !ipc.running
            Layout.fillWidth: true
            Layout.topMargin: 4
            spacing: 6
            ProviderLogo { provider: ipc.selectedProvider; size: 14 }
            Text {
                Layout.fillWidth: true
                text: Format.tr(composer.dict, "SwitchHint", "Next reply from {to}. It gets this conversation as context.", { to: ipc.selectedModelName })
                elide: Text.ElideRight
                color: Theme.textDim
                font.family: Theme.sans
                font.pixelSize: 12
            }
            Text {
                text: Format.tr(composer.dict, "SwitchBack", "Use {from}", { from: ipc.modelName(switchHint.previous) })
                color: Theme.accentSecondary
                font.family: Theme.sans
                font.pixelSize: 12
                MouseArea { anchors.fill: parent; cursorShape: Qt.PointingHandCursor; onClicked: ipc.chooseModel(switchHint.previous) }
            }
        }

        // Messages waiting for the current reply to finish
        ColumnLayout {
            visible: ipc.queue.length > 0
            Layout.fillWidth: true
            Layout.topMargin: 4
            spacing: 4
            RowLayout {
                Layout.fillWidth: true
                spacing: 8
                Icon { name: ipc.queuePaused ? "pause" : "clock"; size: 13; color: ipc.queuePaused ? Theme.warning : Theme.textDim }
                Text {
                    Layout.fillWidth: true
                    text: ipc.queuePaused
                          ? (ipc.queueResumeAt > 0
                             ? Format.tr(composer.dict, "QueueResumesIn", "Queue resumes in {n}", { n: Format.countdown(new Date(ipc.queueResumeAt * 1000).toISOString()) })
                             : Format.tr(composer.dict, "QueuePaused", "Queue paused"))
                          : Format.tr(composer.dict, "QueuedCount", "{n} queued, sent when the reply finishes", { n: ipc.queue.length })
                    elide: Text.ElideRight
                    color: Theme.textDim
                    font.family: Theme.sans
                    font.pixelSize: 12
                }
                ChipButton {
                    visible: ipc.queuePaused && composer.queueLimit !== null && composer.queueLimit.resetsAt > 0 && ipc.queueResumeAt === 0
                    text: Format.tr(composer.dict, "ResumeAtReset", "Resume at reset")
                    iconName: "clock"
                    onClicked: ipc.resumeQueueAt(composer.queueLimit.resetsAt)
                }
                ChipButton {
                    visible: ipc.queuePaused
                    primary: true
                    text: Format.tr(composer.dict, "Resume", "Resume")
                    iconName: "play"
                    onClicked: ipc.resumeQueue()
                }
                IconButton {
                    implicitWidth: 26
                    implicitHeight: 26
                    icon: "x"
                    iconSize: 13
                    tip: Format.tr(composer.dict, "ClearQueue", "Clear queue")
                    onClicked: ipc.clearQueue()
                }
            }
            Repeater {
                model: ipc.queue
                delegate: Rectangle {
                    id: queued
                    required property var modelData
                    required property int index
                    Layout.fillWidth: true
                    implicitHeight: 34
                    radius: 10
                    color: queuedHover.hovered ? Theme.hover : Theme.input
                    border.width: 1
                    border.color: Theme.borderSubtle
                    HoverHandler { id: queuedHover }
                    RowLayout {
                        anchors.fill: parent
                        anchors.leftMargin: 10
                        anchors.rightMargin: 4
                        spacing: 6
                        Text {
                            text: (queued.index + 1) + "."
                            color: Theme.textDim
                            font.family: Theme.sans
                            font.pixelSize: 12
                        }
                        Icon {
                            visible: (queued.modelData.attachments || []).length > 0
                            name: "paperclip"
                            size: 12
                            color: Theme.textDim
                        }
                        Text {
                            Layout.fillWidth: true
                            text: (queued.modelData.text || "").replace(/\s+/g, " ")
                            elide: Text.ElideRight
                            color: Theme.textSecondary
                            font.family: Theme.sans
                            font.pixelSize: 13
                        }
                        Row {
                            opacity: queuedHover.hovered ? 1 : 0.0
                            spacing: 0
                            IconButton { implicitWidth: 26; implicitHeight: 26; icon: "chevron-up"; iconSize: 14; tip: Format.tr(composer.dict, "MoveUp", "Move up"); opacity: queued.index > 0 ? 1 : 0.3; onClicked: ipc.moveQueued(queued.index, -1) }
                            IconButton { implicitWidth: 26; implicitHeight: 26; icon: "chevron-down"; iconSize: 14; tip: Format.tr(composer.dict, "MoveDown", "Move down"); opacity: queued.index < ipc.queue.length - 1 ? 1 : 0.3; onClicked: ipc.moveQueued(queued.index, 1) }
                            IconButton { implicitWidth: 26; implicitHeight: 26; icon: "edit-2"; iconSize: 13; tip: Format.tr(composer.dict, "Edit", "Edit"); onClicked: ipc.editQueued(queued.index) }
                            IconButton { implicitWidth: 26; implicitHeight: 26; icon: "skip-forward"; iconSize: 13; tip: Format.tr(composer.dict, "SendNow", "Send now (stops the current reply)"); onClicked: ipc.sendQueuedNow(queued.index) }
                            IconButton { implicitWidth: 26; implicitHeight: 26; icon: "trash-2"; iconSize: 13; tip: Format.tr(composer.dict, "Remove", "Remove"); onClicked: ipc.removeQueued(queued.index) }
                        }
                    }
                }
            }
        }

        // Attached files and images, removable until you send
        Flow {
            visible: ipc.attachments.length > 0
            Layout.fillWidth: true
            Layout.topMargin: 6
            Layout.bottomMargin: 2
            spacing: 10
            Repeater {
                model: ipc.attachments
                delegate: AttachmentChip {
                    required property var modelData
                    required property int index
                    item: modelData
                    removable: true
                    onRemoveRequested: ipc.removeAttachment(index)
                }
            }
        }

        ScrollView {
            Layout.fillWidth: true
            Layout.preferredHeight: Math.min(190, Math.max(46, input.contentHeight + 20))
            clip: true
            TextArea {
                id: input
                onTextChanged: if (!composer.filling) draftTimer.restart()
                placeholderText: composer.t("MessageSerika")
                placeholderTextColor: Theme.textDim
                color: Theme.text
                selectionColor: Theme.accent
                selectedTextColor: "white"
                font.family: Theme.sans
                font.pixelSize: Theme.chatPx
                wrapMode: TextArea.Wrap
                background: null
                leftPadding: 4
                topPadding: 8
                bottomPadding: 8
                Keys.onPressed: (event) => {
                    // Ctrl+V with files or an image on the clipboard attaches them; text pastes as usual.
                    if (event.matches(StandardKey.Paste) && ipc.pasteAttachments()) {
                        event.accepted = true
                        return
                    }
                    if (event.key === Qt.Key_Escape && ipc.running) {
                        ipc.cancel()
                        event.accepted = true
                        return
                    }
                    if (event.key === Qt.Key_Return || event.key === Qt.Key_Enter) {
                        const ctrl = (event.modifiers & Qt.ControlModifier) !== 0
                        const shift = (event.modifiers & Qt.ShiftModifier) !== 0
                        if (composer.ctrlToSend ? ctrl : !shift) {
                            composer.submit()
                            event.accepted = true
                        }
                    }
                }
            }
        }

        RowLayout {
            Layout.fillWidth: true
            spacing: 6

            IconButton {
                icon: "paperclip"
                iconSize: 17
                tip: Format.tr(composer.dict, "AttachFiles", "Attach files or images")
                onClicked: attachDialog.open()
            }
            IconButton {
                icon: "folder-plus"
                iconSize: 17
                tip: Format.tr(composer.dict, "AddProject", "Add project folder")
                onClicked: composer.folderRequested()
            }
            Pill {
                id: modePill
                icon: composer.currentMode.icon
                label: composer.currentMode.label
                active: ipc.permissionMode === "full"
                onClicked: modeMenu.opened ? modeMenu.close() : modeMenu.open()
                Popover {
                    id: modeMenu
                    y: -height - 8
                    width: 300
                    contentItem: ColumnLayout {
                        spacing: 0
                        Text {
                            Layout.leftMargin: 10
                            Layout.topMargin: 6
                            Layout.bottomMargin: 4
                            text: Format.tr(composer.dict, "Permissions", "Permissions")
                            color: Theme.textDim
                            font.family: Theme.sans
                            font.pixelSize: 12
                        }
                        Repeater {
                            model: composer.modes
                            delegate: Rectangle {
                                required property var modelData
                                readonly property bool current: modelData.id === ipc.permissionMode
                                Layout.fillWidth: true
                                implicitHeight: 48
                                radius: 8
                                color: current ? Theme.active : (modeArea.containsMouse ? Theme.hover : "transparent")
                                RowLayout {
                                    anchors.fill: parent
                                    anchors.leftMargin: 10
                                    anchors.rightMargin: 10
                                    spacing: 12
                                    Icon { name: modelData.icon; size: 17; color: Theme.textSecondary }
                                    ColumnLayout {
                                        Layout.fillWidth: true
                                        spacing: 1
                                        Text { text: modelData.label; color: Theme.text; font.family: Theme.sans; font.pixelSize: 14 }
                                        Text { Layout.fillWidth: true; text: modelData.help; elide: Text.ElideRight; color: Theme.textDim; font.family: Theme.sans; font.pixelSize: 12 }
                                    }
                                    Icon { visible: current; name: "check"; size: 16; color: Theme.text }
                                }
                                MouseArea {
                                    id: modeArea
                                    anchors.fill: parent
                                    hoverEnabled: true
                                    cursorShape: Qt.PointingHandCursor
                                    onClicked: { ipc.setSetting("permission", modelData.id); modeMenu.close() }
                                }
                            }
                        }
                    }
                }
            }
            Pill {
                id: effortPill
                visible: composer.efforts.length > 0
                icon: "cpu"
                label: composer.effort ? composer.effortLabel(composer.effort) : Format.tr(composer.dict, "Effort", "Effort")
                active: composer.effort === "max" || composer.effort === "xhigh"
                onClicked: effortMenu.opened ? effortMenu.close() : effortMenu.open()
                Popover {
                    id: effortMenu
                    y: -height - 8
                    width: 220
                    contentItem: ColumnLayout {
                        spacing: 0
                        Text {
                            Layout.leftMargin: 10
                            Layout.topMargin: 6
                            Layout.bottomMargin: 4
                            text: Format.tr(composer.dict, "ReasoningEffort", "Reasoning effort")
                            color: Theme.textDim
                            font.family: Theme.sans
                            font.pixelSize: 12
                        }
                        Repeater {
                            model: [""].concat(composer.efforts)
                            delegate: Rectangle {
                                required property var modelData
                                readonly property bool current: modelData === composer.effort
                                Layout.fillWidth: true
                                implicitHeight: 36
                                radius: 8
                                color: current ? Theme.active : (effortArea.containsMouse ? Theme.hover : "transparent")
                                RowLayout {
                                    anchors.fill: parent
                                    anchors.leftMargin: 10
                                    anchors.rightMargin: 10
                                    Text { Layout.fillWidth: true; text: composer.effortLabel(modelData); color: Theme.text; font.family: Theme.sans; font.pixelSize: 14 }
                                    Icon { visible: current; name: "check"; size: 16; color: Theme.text }
                                }
                                MouseArea {
                                    id: effortArea
                                    anchors.fill: parent
                                    hoverEnabled: true
                                    cursorShape: Qt.PointingHandCursor
                                    onClicked: { ipc.setSetting(composer.effortSetting, modelData || "default"); effortMenu.close() }
                                }
                            }
                        }
                    }
                }
            }
            Pill {
                icon: "layers"
                label: composer.t("PlanMode")
                active: ipc.planMode
                onClicked: ipc.planMode = !ipc.planMode
            }
            Pill {
                icon: "globe"
                label: Format.tr(composer.dict, "Browser", "Browser")
                onClicked: composer.browserRequested()
            }

            Item { Layout.fillWidth: true }

            Rectangle {
                id: send
                Layout.preferredWidth: 34
                Layout.preferredHeight: 34
                radius: 17
                // Running with text typed: the button queues it. Running without: it stops.
                readonly property bool stops: ipc.running && !composer.hasText
                color: stops ? Theme.text
                       : (composer.hasText ? (sendArea.containsMouse ? Theme.accentHover : Theme.accent) : Theme.active)
                Behavior on color { ColorAnimation { duration: 120 } }

                Icon {
                    anchors.centerIn: parent
                    name: send.stops ? "square" : (ipc.running ? "clock" : "arrow-up")
                    size: send.stops ? 13 : (ipc.running ? 16 : 18)
                    color: send.stops ? Theme.bg : (composer.hasText ? "white" : Theme.textDim)
                }
                MouseArea {
                    id: sendArea
                    anchors.fill: parent
                    hoverEnabled: true
                    cursorShape: (ipc.running || composer.hasText) ? Qt.PointingHandCursor : Qt.ArrowCursor
                    onClicked: send.stops ? ipc.cancel() : composer.submit()
                }
                ToolTip.visible: sendArea.containsMouse
                ToolTip.delay: 500
                ToolTip.text: send.stops ? composer.t("StopGeneration") + "  (Esc)"
                              : (ipc.running ? Format.tr(composer.dict, "AddToQueue", "Add to queue") : composer.t("SendMessage"))
            }
        }
    }
}

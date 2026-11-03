import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

// A terminal per tab in the project folder: run a command, watch its output, Ctrl+C to stop it.
// The shell lives in the runtime and keeps its state (cd, variables) between commands.
Item {
    id: panel
    readonly property var dict: ipc.strings
    readonly property var terms: ipc.terminals
    property string current: ""
    readonly property var term: {
        for (let i = 0; i < terms.length; i++)
            if (terms[i].id === current) return terms[i]
        return null
    }
    readonly property bool busy: term !== null && term.running === true
    property var history: []
    property int historyAt: -1
    property bool requested: false

    function tr(key, fallback, values) { return Format.tr(dict, key, fallback, values) }
    function shortPath(path) {
        const p = (path || "").replace(/\\/g, "/")
        const parts = p.split("/").filter(function (s) { return s.length > 0 })
        return parts.length > 2 ? "…/" + parts.slice(-2).join("/") : p
    }
    function reload() {
        output.text = current.length > 0 ? ipc.terminalText(current) : ""
        Qt.callLater(scrollDown)
    }
    function scrollDown() { flick.contentY = Math.max(0, flick.contentHeight - flick.height) }
    function run() {
        const command = input.text
        if (!term || term.exited || busy) return
        if (command.trim().length > 0) {
            history = history.filter(function (h) { return h !== command }).concat([command]).slice(-200)
        }
        historyAt = -1
        input.text = ""
        if (command.trim() === "clear" || command.trim() === "cls") {
            ipc.terminalClear(current)
            reload()
            return
        }
        ipc.terminalRun(current, command)
    }
    function focusInput() { input.forceActiveFocus() }

    // Open a first terminal when the panel shows up empty, and pick a tab when one goes away.
    onVisibleChanged: ensure()
    onTermsChanged: {
        if (terms.length > 0 && term === null) { current = terms[terms.length - 1].id; requested = false; reload() }
        else if (terms.length === 0) { current = ""; reload(); ensure() }
    }
    onCurrentChanged: reload()
    Component.onCompleted: {
        if (terms.length > 0) current = terms[terms.length - 1].id
        ensure()
    }
    function ensure() {
        if (visible && terms.length === 0 && !requested) {
            requested = true
            ipc.terminalOpen("")
        }
    }

    Connections {
        target: ipc
        function onTerminalOutput(id, text, error) {
            if (id !== panel.current) return
            const atEnd = flick.contentY >= flick.contentHeight - flick.height - 24
            output.insert(output.length, text)
            if (output.length > 300000) output.remove(0, output.length - 240000)
            if (atEnd) Qt.callLater(panel.scrollDown)
        }
    }

    ColumnLayout {
        anchors.fill: parent
        spacing: 0

        // Terminal tabs
        Rectangle {
            Layout.fillWidth: true
            Layout.preferredHeight: 34
            color: Theme.bg
            RowLayout {
                anchors.fill: parent
                anchors.leftMargin: 8
                anchors.rightMargin: 6
                spacing: 2
                Repeater {
                    model: panel.terms
                    delegate: Rectangle {
                        id: termTab
                        required property var modelData
                        required property int index
                        readonly property bool selected: modelData.id === panel.current
                        Layout.preferredHeight: 26
                        Layout.preferredWidth: termRow.implicitWidth + 18
                        radius: 7
                        color: selected ? Theme.card : (termArea.containsMouse ? Theme.hover : "transparent")
                        border.width: selected ? 1 : 0
                        border.color: Theme.borderSubtle
                        Behavior on color { ColorAnimation { duration: Theme.motionFast } }
                        Row {
                            id: termRow
                            anchors.centerIn: parent
                            spacing: 6
                            Rectangle {
                                anchors.verticalCenter: parent.verticalCenter
                                width: 6
                                height: 6
                                radius: 3
                                color: termTab.modelData.exited ? Theme.textDim : (termTab.modelData.running ? Theme.accent : "#22c55e")
                            }
                            Text {
                                anchors.verticalCenter: parent.verticalCenter
                                text: (termTab.modelData.shell || "shell") + " " + (termTab.index + 1)
                                color: termTab.selected ? Theme.text : Theme.textSecondary
                                font.family: Theme.sans
                                font.pixelSize: 12
                            }
                            Rectangle {
                                anchors.verticalCenter: parent.verticalCenter
                                width: 16
                                height: 16
                                radius: 4
                                opacity: termArea.containsMouse || termTab.selected ? 1 : 0
                                color: termClose.containsMouse ? Theme.active : "transparent"
                                Icon { anchors.centerIn: parent; name: "x"; size: 10; color: Theme.textSecondary }
                                MouseArea {
                                    id: termClose
                                    anchors.fill: parent
                                    hoverEnabled: true
                                    cursorShape: Qt.PointingHandCursor
                                    onClicked: ipc.terminalClose(termTab.modelData.id)
                                }
                            }
                        }
                        MouseArea {
                            id: termArea
                            anchors.fill: parent
                            z: -1
                            hoverEnabled: true
                            acceptedButtons: Qt.LeftButton | Qt.MiddleButton
                            cursorShape: Qt.PointingHandCursor
                            onClicked: (mouse) => {
                                if (mouse.button === Qt.MiddleButton) ipc.terminalClose(termTab.modelData.id)
                                else { panel.current = termTab.modelData.id; panel.focusInput() }
                            }
                        }
                    }
                }
                IconButton {
                    implicitWidth: 26
                    implicitHeight: 26
                    icon: "plus"
                    iconSize: 14
                    tip: panel.tr("NewTerminal", "New terminal")
                    onClicked: ipc.terminalOpen("")
                }
                Item { Layout.fillWidth: true }
                IconButton {
                    implicitWidth: 26
                    implicitHeight: 26
                    visible: panel.busy
                    icon: "square"
                    iconSize: 12
                    tip: panel.tr("StopCommand", "Stop (Ctrl C)")
                    onClicked: ipc.terminalInterrupt(panel.current)
                }
                IconButton {
                    implicitWidth: 26
                    implicitHeight: 26
                    icon: "copy"
                    iconSize: 13
                    tip: panel.tr("CopyOutput", "Copy output")
                    onClicked: ipc.copyText(output.selectedText.length > 0 ? output.selectedText : output.text)
                }
                IconButton {
                    implicitWidth: 26
                    implicitHeight: 26
                    icon: "trash-2"
                    iconSize: 13
                    tip: panel.tr("ClearTerminal", "Clear (Ctrl L)")
                    onClicked: { ipc.terminalClear(panel.current); panel.reload() }
                }
            }
            Rectangle { anchors.bottom: parent.bottom; width: parent.width; height: 1; color: Theme.borderSubtle }
        }

        // Output
        Rectangle {
            Layout.fillWidth: true
            Layout.fillHeight: true
            color: Theme.card
            Flickable {
                id: flick
                SmoothWheel { flickable: flick }
                anchors.fill: parent
                anchors.leftMargin: 12
                anchors.rightMargin: 4
                anchors.topMargin: 8
                contentWidth: width
                contentHeight: output.implicitHeight
                clip: true
                boundsBehavior: Flickable.StopAtBounds
                ScrollBar.vertical: ScrollBar { policy: ScrollBar.AsNeeded }
                TextEdit {
                    id: output
                    width: flick.width - 12
                    readOnly: true
                    selectByMouse: true
                    wrapMode: TextEdit.WrapAnywhere
                    textFormat: TextEdit.PlainText
                    color: Theme.text
                    selectionColor: Theme.accent
                    selectedTextColor: "white"
                    font.family: Theme.mono
                    font.pixelSize: 12
                }
            }
            Text {
                visible: output.length === 0
                anchors.centerIn: parent
                text: panel.term && panel.term.exited ? panel.tr("TerminalExited", "This shell has exited. Open a new terminal with +.")
                                                      : panel.tr("TerminalEmpty", "Run a command in {path}", { path: panel.shortPath(panel.term ? panel.term.cwd : ipc.projectPath) })
                color: Theme.textDim
                font.family: Theme.sans
                font.pixelSize: 12
            }
            MouseArea {
                anchors.fill: parent
                acceptedButtons: Qt.NoButton
                onWheel: (wheel) => wheel.accepted = false
            }
        }

        // Prompt
        Rectangle {
            Layout.fillWidth: true
            Layout.preferredHeight: 38
            color: Theme.card
            Rectangle { anchors.top: parent.top; width: parent.width; height: 1; color: Theme.borderSubtle }
            RowLayout {
                anchors.fill: parent
                anchors.leftMargin: 12
                anchors.rightMargin: 8
                spacing: 8
                Text {
                    text: panel.shortPath(panel.term ? panel.term.cwd : "")
                    color: Theme.accentSecondary
                    font.family: Theme.mono
                    font.pixelSize: 12
                    Layout.maximumWidth: panel.width * 0.4
                    elide: Text.ElideLeft
                }
                Text {
                    text: panel.busy ? "…" : "❯"
                    color: panel.term && panel.term.lastCode !== undefined && panel.term.lastCode !== 0 ? Theme.danger : Theme.textSecondary
                    font.family: Theme.mono
                    font.pixelSize: 13
                }
                TextField {
                    id: input
                    Layout.fillWidth: true
                    background: null
                    enabled: panel.term !== null && panel.term.exited !== true
                    color: Theme.text
                    placeholderText: panel.busy ? panel.tr("TerminalBusy", "Running… Ctrl C to stop")
                                                : panel.tr("TerminalPlaceholder", "Type a command")
                    placeholderTextColor: Theme.textDim
                    selectionColor: Theme.accent
                    selectedTextColor: "white"
                    font.family: Theme.mono
                    font.pixelSize: 12
                    verticalAlignment: TextInput.AlignVCenter
                    onAccepted: panel.run()
                    Keys.onPressed: (event) => {
                        if ((event.modifiers & Qt.ControlModifier) && event.key === Qt.Key_C && selectedText.length === 0) {
                            if (panel.busy) ipc.terminalInterrupt(panel.current)
                            else text = ""
                            event.accepted = true
                        } else if ((event.modifiers & Qt.ControlModifier) && event.key === Qt.Key_L) {
                            ipc.terminalClear(panel.current)
                            panel.reload()
                            event.accepted = true
                        } else if (event.key === Qt.Key_Up && panel.history.length > 0) {
                            panel.historyAt = panel.historyAt < 0 ? panel.history.length - 1 : Math.max(0, panel.historyAt - 1)
                            text = panel.history[panel.historyAt]
                            event.accepted = true
                        } else if (event.key === Qt.Key_Down && panel.historyAt >= 0) {
                            panel.historyAt += 1
                            if (panel.historyAt >= panel.history.length) { panel.historyAt = -1; text = "" }
                            else text = panel.history[panel.historyAt]
                            event.accepted = true
                        }
                    }
                }
            }
        }
    }
}

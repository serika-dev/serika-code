import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

// Sub-agents this chat delegated to: what each is doing now, and what it reported.
Item {
    id: panel
    readonly property var dict: ipc.strings
    readonly property int running: ipc.agents.filter(function (a) { return a.status === "running" }).length
    property real now: Date.now()
    property var openMap: ({})

    function providerOf(id) {
        id = id || ""
        if (id.indexOf("claude-code/") === 0) return "claude"
        if (id.indexOf("codex/") === 0) return "codex"
        return "serika"
    }
    function elapsed(agent) {
        const start = Number(agent.startedAt || 0)
        if (start <= 0) return ""
        const end = Number(agent.endedAt || 0) > 0 ? Number(agent.endedAt) : panel.now
        const s = Math.max(0, Math.round((end - start) / 1000))
        return s >= 60 ? Math.floor(s / 60) + "m " + (s % 60) + "s" : s + "s"
    }
    function setOpen(id, value) {
        const next = Object.assign({}, openMap)
        next[id] = value
        openMap = next
    }

    // Live clock for running agents
    Timer { interval: 1000; repeat: true; running: panel.visible && panel.running > 0; onTriggered: panel.now = Date.now() }

    // Summary
    RowLayout {
        id: header
        visible: ipc.agents.length > 0
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.top: parent.top
        anchors.leftMargin: 14
        anchors.rightMargin: 14
        height: visible ? 44 : 0
        spacing: 8
        Text {
            text: ipc.agents.length === 1 ? Format.tr(panel.dict, "OneSubagent", "1 sub-agent") : Format.tr(panel.dict, "Subagents", "{n} sub-agents", { n: ipc.agents.length })
            color: Theme.text
            font.family: Theme.sans
            font.pixelSize: 13
            font.weight: Font.DemiBold
        }
        Rectangle {
            visible: panel.running > 0
            implicitWidth: runRow.implicitWidth + 14
            implicitHeight: 20
            radius: 10
            color: Theme.accentSoft
            Row {
                id: runRow
                anchors.centerIn: parent
                spacing: 5
                Rectangle {
                    anchors.verticalCenter: parent.verticalCenter
                    width: 6
                    height: 6
                    radius: 3
                    color: Theme.accent
                    SequentialAnimation on opacity {
                        running: panel.running > 0
                        loops: Animation.Infinite
                        NumberAnimation { to: 0.3; duration: 600; easing.type: Easing.InOutSine }
                        NumberAnimation { to: 1; duration: 600; easing.type: Easing.InOutSine }
                    }
                }
                Text {
                    anchors.verticalCenter: parent.verticalCenter
                    text: Format.tr(panel.dict, "RunningCount", "{n} working", { n: panel.running })
                    color: Theme.accentSecondary
                    font.family: Theme.sans
                    font.pixelSize: 11
                    font.weight: Font.Medium
                }
            }
        }
        Item { Layout.fillWidth: true }
    }
    Rectangle { visible: header.visible; anchors.top: header.bottom; width: parent.width; height: 1; color: Theme.borderSubtle }

    ListView {
        id: list
        visible: ipc.agents.length > 0
        anchors.top: header.bottom
        anchors.topMargin: 1
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.bottom: parent.bottom
        anchors.margins: 10
        spacing: 10
        clip: true
        model: ipc.agents
        boundsBehavior: Flickable.StopAtBounds
        ScrollBar.vertical: ScrollBar { policy: ScrollBar.AsNeeded; width: 8 }
        add: Transition {
            ParallelAnimation {
                NumberAnimation { property: "opacity"; from: 0; to: 1; duration: Theme.motionBase; easing.type: Theme.easeOut }
                NumberAnimation { property: "scale"; from: 0.97; to: 1; duration: Theme.motionBase; easing.type: Theme.easeOut }
            }
        }
        displaced: Transition { NumberAnimation { properties: "y"; duration: Theme.motionBase; easing.type: Theme.easeOut } }

        delegate: Rectangle {
            id: card
            required property var modelData
            readonly property string status: modelData.status || "done"
            readonly property bool active: status === "running"
            readonly property bool open: panel.openMap[modelData.id] === true || (active && panel.openMap[modelData.id] !== false)
            width: ListView.view.width
            height: body.implicitHeight + 24
            radius: 12
            color: cardHover.hovered ? Theme.hover : Theme.bg
            border.width: 1
            border.color: active ? Qt.rgba(Theme.accent.r, Theme.accent.g, Theme.accent.b, 0.55) : Theme.borderSubtle
            clip: true
            Behavior on color { ColorAnimation { duration: Theme.motionFast } }
            Behavior on border.color { ColorAnimation { duration: Theme.motionBase } }
            Behavior on height { NumberAnimation { duration: Theme.motionBase; easing.type: Theme.easeOut } }
            HoverHandler { id: cardHover }
            TapHandler { onTapped: panel.setOpen(card.modelData.id, !card.open) }

            // Indeterminate progress along the top edge while it works
            Item {
                visible: card.active
                anchors.left: parent.left
                anchors.right: parent.right
                anchors.top: parent.top
                anchors.margins: 1
                height: 2
                clip: true
                Rectangle {
                    id: sweep
                    width: parent.width * 0.4
                    height: 2
                    radius: 1
                    gradient: Gradient {
                        orientation: Gradient.Horizontal
                        GradientStop { position: 0; color: "transparent" }
                        GradientStop { position: 0.5; color: Theme.accent }
                        GradientStop { position: 1; color: "transparent" }
                    }
                    NumberAnimation on x {
                        running: card.active
                        from: -sweep.width
                        to: card.width
                        duration: 1400
                        loops: Animation.Infinite
                        easing.type: Easing.InOutSine
                    }
                }
            }

            ColumnLayout {
                id: body
                anchors.left: parent.left
                anchors.right: parent.right
                anchors.top: parent.top
                anchors.margins: 12
                spacing: 8

                RowLayout {
                    Layout.fillWidth: true
                    spacing: 8
                    // Status
                    Item {
                        Layout.preferredWidth: 18
                        Layout.preferredHeight: 18
                        Icon {
                            id: spin
                            anchors.centerIn: parent
                            visible: card.active
                            name: "loader"
                            size: 16
                            color: Theme.accentSecondary
                            RotationAnimator on rotation { running: spin.visible; from: 0; to: 360; duration: 1000; loops: Animation.Infinite }
                        }
                        Icon {
                            anchors.centerIn: parent
                            visible: !card.active
                            name: card.status === "done" ? "check-circle" : (card.status === "cancelled" ? "square" : "x-circle")
                            size: 16
                            color: card.status === "done" ? Theme.success : (card.status === "cancelled" ? Theme.warning : Theme.danger)
                        }
                    }
                    Text {
                        Layout.fillWidth: true
                        text: card.modelData.name || Format.tr(panel.dict, "Subagent", "Sub-agent")
                        elide: Text.ElideRight
                        color: Theme.text
                        font.family: Theme.sans
                        font.pixelSize: 14
                        font.weight: Font.DemiBold
                    }
                    Text {
                        text: panel.elapsed(card.modelData)
                        color: Theme.textDim
                        font.family: Theme.mono
                        font.pixelSize: 11
                    }
                }

                // Model and tool count
                RowLayout {
                    spacing: 6
                    ProviderLogo { provider: panel.providerOf(card.modelData.model); size: 14 }
                    Text {
                        text: ipc.modelName(card.modelData.model || "")
                        color: Theme.textSecondary
                        font.family: Theme.sans
                        font.pixelSize: 12
                    }
                    Text {
                        visible: Number(card.modelData.tools || 0) > 0
                        text: "· " + Format.tr(panel.dict, "ToolCount", "{n} tools", { n: card.modelData.tools })
                        color: Theme.textDim
                        font.family: Theme.sans
                        font.pixelSize: 12
                    }
                }

                // What it is doing right now
                Text {
                    visible: card.active && (card.modelData.activity || "").length > 0
                    Layout.fillWidth: true
                    text: card.modelData.activity || ""
                    elide: Text.ElideRight
                    color: Theme.accentSecondary
                    font.family: Theme.mono
                    font.pixelSize: 12
                }

                // Task and report, when opened
                ColumnLayout {
                    visible: card.open
                    Layout.fillWidth: true
                    spacing: 6
                    Text { text: Format.tr(panel.dict, "Task", "Task"); color: Theme.textDim; font.family: Theme.sans; font.pixelSize: 11; font.capitalization: Font.AllUppercase; font.letterSpacing: 0.5 }
                    Text {
                        Layout.fillWidth: true
                        text: card.modelData.task || ""
                        wrapMode: Text.WordWrap
                        color: Theme.textSecondary
                        font.family: Theme.sans
                        font.pixelSize: 13
                    }
                    Text {
                        visible: (card.modelData.summary || "").length > 0
                        Layout.topMargin: 4
                        text: Format.tr(panel.dict, "Report", "Report")
                        color: Theme.textDim
                        font.family: Theme.sans
                        font.pixelSize: 11
                        font.capitalization: Font.AllUppercase
                        font.letterSpacing: 0.5
                    }
                    Text {
                        visible: (card.modelData.summary || "").length > 0
                        Layout.fillWidth: true
                        text: card.modelData.summary || ""
                        textFormat: Text.MarkdownText
                        wrapMode: Text.WordWrap
                        color: Theme.text
                        font.family: Theme.sans
                        font.pixelSize: 13
                        onLinkActivated: (link) => Qt.openUrlExternally(link)
                    }
                }

                // Collapsed report preview
                Text {
                    visible: !card.open && !card.active && (card.modelData.summary || "").length > 0
                    Layout.fillWidth: true
                    text: (card.modelData.summary || "").replace(/\s+/g, " ")
                    maximumLineCount: 2
                    wrapMode: Text.WordWrap
                    elide: Text.ElideRight
                    color: Theme.textSecondary
                    font.family: Theme.sans
                    font.pixelSize: 12
                }
            }
        }
    }

    // None yet
    ColumnLayout {
        visible: ipc.agents.length === 0
        anchors.centerIn: parent
        width: Math.min(parent.width - 48, 300)
        spacing: 12
        Rectangle {
            Layout.alignment: Qt.AlignHCenter
            Layout.preferredWidth: 52
            Layout.preferredHeight: 52
            radius: 16
            color: Theme.accentSoft
            Icon { anchors.centerIn: parent; name: "users"; size: 24; color: Theme.accentSecondary }
        }
        Text {
            Layout.fillWidth: true
            horizontalAlignment: Text.AlignHCenter
            text: Format.tr(panel.dict, "NoAgentsTitle", "No sub-agents yet")
            color: Theme.text
            font.family: Theme.brand
            font.pixelSize: 17
            font.weight: Font.DemiBold
        }
        Text {
            Layout.fillWidth: true
            horizontalAlignment: Text.AlignHCenter
            wrapMode: Text.WordWrap
            text: Format.tr(panel.dict, "NoAgentsHelp", "Serika can hand separate pieces of work to sub-agents that run side by side. Ask it to split a task up, and they show here.")
            color: Theme.textDim
            font.family: Theme.sans
            font.pixelSize: 13
        }
    }
}

import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

// The tools that ran during a turn, as one grouped card. Each row shows what the tool did and to what;
// click a row to see its output, jump to the diff it produced, or open the page it visited.
ColumnLayout {
    id: list
    property var tools: []
    property var openMap: ({})
    property bool showAll: false

    readonly property int total: tools.length
    readonly property bool trimmed: total > 8 && !showAll
    readonly property var visibleTools: trimmed ? tools.slice(total - 6) : tools
    readonly property int hiddenCount: total - visibleTools.length
    readonly property int failed: tools.filter(function (t) { return t.state === "failed" || t.ok === false }).length
    readonly property int running: tools.filter(function (t) { return t.state === "running" || t.state === "pending" }).length
    readonly property bool expandAll: ipc.ui.toolsExpanded === true
    spacing: 0

    Rectangle {
        Layout.fillWidth: true
        implicitHeight: card.implicitHeight
        radius: 12
        color: Theme.card
        border.width: 1
        border.color: Theme.borderSubtle
        clip: true

        ColumnLayout {
            id: card
            width: parent.width
            spacing: 0

            // Summary header, only worth showing for several calls.
            Item {
                visible: list.total > 2
                Layout.fillWidth: true
                Layout.preferredHeight: visible ? 34 : 0
                RowLayout {
                    anchors.fill: parent
                    anchors.leftMargin: 12
                    anchors.rightMargin: 12
                    spacing: 8
                    Icon { name: "tool"; size: 14; color: Theme.textDim }
                    Text {
                        text: Format.tr(ipc.strings, "ToolCalls", "{n} tool calls", { n: list.total })
                        color: Theme.textSecondary
                        font.family: Theme.sans
                        font.pixelSize: 12
                        font.weight: Font.Medium
                    }
                    Text {
                        visible: list.failed > 0
                        text: "· " + Format.tr(ipc.strings, "ToolsFailed", "{n} failed", { n: list.failed })
                        color: Theme.danger
                        font.family: Theme.sans
                        font.pixelSize: 12
                    }
                    Text {
                        visible: list.running > 0
                        text: "· " + Format.tr(ipc.strings, "ToolsRunning", "{n} running", { n: list.running })
                        color: Theme.accentSecondary
                        font.family: Theme.sans
                        font.pixelSize: 12
                    }
                    Item { Layout.fillWidth: true }
                    Text {
                        visible: list.hiddenCount > 0 || list.showAll && list.total > 8
                        text: list.showAll ? Format.tr(ipc.strings, "ShowFewer", "Show fewer")
                                           : Format.tr(ipc.strings, "ShowEarlier", "Show {n} earlier", { n: list.hiddenCount })
                        color: Theme.accentSecondary
                        font.family: Theme.sans
                        font.pixelSize: 12
                        MouseArea { anchors.fill: parent; cursorShape: Qt.PointingHandCursor; onClicked: list.showAll = !list.showAll }
                    }
                }
            }

            Repeater {
                model: list.visibleTools
                delegate: Item {
                    id: tool
                    required property var modelData
                    required property int index
                    readonly property var meta: Format.toolMeta(modelData.name)
                    readonly property string state: modelData.state || (modelData.ok === false ? "failed" : "ok")
                    readonly property string output: modelData.output || ""
                    readonly property string result: modelData.result || ""
                    readonly property string key: (modelData.id || modelData.name) + ":" + (list.hiddenCount + index)
                    readonly property bool isCommand: meta.icon === "terminal"
                    readonly property bool isBrowser: (modelData.name || "").indexOf("browser_") === 0
                    readonly property bool expandable: output.length > 0 || result.length > 0 || (modelData.diffPath || "").length > 0 || isBrowser
                    readonly property bool open: expandable && (list.openMap[key] === true || (list.expandAll && output.length > 0))

                    Layout.fillWidth: true
                    implicitHeight: head.height + (open ? body.implicitHeight + 10 : 0)

                    Rectangle {
                        visible: tool.index > 0 || list.total > 2
                        anchors.top: parent.top
                        width: parent.width
                        height: 1
                        color: Theme.borderSubtle
                    }

                    Rectangle {
                        id: head
                        width: parent.width
                        height: 40
                        color: headArea.containsMouse && tool.expandable ? Theme.hover : "transparent"

                        RowLayout {
                            anchors.fill: parent
                            anchors.leftMargin: 12
                            anchors.rightMargin: 12
                            spacing: 10

                            Rectangle {
                                Layout.preferredWidth: 24
                                Layout.preferredHeight: 24
                                radius: 7
                                color: tool.state === "failed" ? Theme.dangerSoft : Format.tintSoft(tool.meta.tint)
                                Icon {
                                    id: glyph
                                    anchors.centerIn: parent
                                    visible: tool.state !== "running" && tool.state !== "pending"
                                    name: tool.state === "failed" ? "x" : tool.meta.icon
                                    size: 14
                                    color: tool.state === "failed" ? Theme.danger : Format.tint(tool.meta.tint)
                                }
                                Icon {
                                    id: spinner
                                    anchors.centerIn: parent
                                    visible: !glyph.visible
                                    name: "loader"
                                    size: 14
                                    color: Theme.accentSecondary
                                    RotationAnimator on rotation {
                                        running: spinner.visible
                                        from: 0
                                        to: 360
                                        duration: 1100
                                        loops: Animation.Infinite
                                    }
                                }
                            }
                            Text {
                                text: tool.meta.label
                                color: Theme.text
                                font.family: Theme.sans
                                font.pixelSize: 13
                                font.weight: Font.DemiBold
                            }
                            Text {
                                Layout.fillWidth: true
                                text: tool.modelData.summary || ""
                                elide: Text.ElideMiddle
                                color: Theme.textDim
                                font.family: Theme.mono
                                font.pixelSize: 12
                            }
                            Text {
                                visible: tool.result.length > 0 && !tool.open
                                Layout.maximumWidth: 190
                                text: tool.result
                                elide: Text.ElideRight
                                color: tool.state === "failed" ? Theme.danger : Theme.textSecondary
                                font.family: Theme.sans
                                font.pixelSize: 12
                            }
                            Icon {
                                visible: tool.expandable
                                name: "chevron-right"
                                size: 14
                                color: Theme.textDim
                                rotation: tool.open ? 90 : 0
                                Behavior on rotation { NumberAnimation { duration: 130 } }
                            }
                        }
                        MouseArea {
                            id: headArea
                            anchors.fill: parent
                            hoverEnabled: true
                            cursorShape: tool.expandable ? Qt.PointingHandCursor : Qt.ArrowCursor
                            onClicked: {
                                if (!tool.expandable) return
                                const next = Object.assign({}, list.openMap)
                                next[tool.key] = !tool.open
                                list.openMap = next
                            }
                        }
                    }

                    // Expanded: what the tool printed, plus shortcuts to what it changed
                    ColumnLayout {
                        id: body
                        visible: tool.open
                        anchors.top: head.bottom
                        anchors.left: parent.left
                        anchors.right: parent.right
                        anchors.leftMargin: 12
                        anchors.rightMargin: 12
                        spacing: 8

                        Rectangle {
                            visible: tool.output.length > 0 || tool.isCommand
                            Layout.fillWidth: true
                            Layout.preferredHeight: Math.min(220, outputText.implicitHeight + 20)
                            radius: 8
                            color: Theme.code
                            border.width: 1
                            border.color: Theme.borderSubtle
                            ScrollView {
                                anchors.fill: parent
                                anchors.margins: 10
                                clip: true
                                contentWidth: availableWidth
                                TextEdit {
                                    id: outputText
                                    width: parent.width
                                    readOnly: true
                                    selectByMouse: true
                                    selectionColor: Theme.accent
                                    wrapMode: TextEdit.WrapAnywhere
                                    color: tool.state === "failed" ? Theme.danger : Theme.textSecondary
                                    font.family: Theme.mono
                                    font.pixelSize: 12
                                    text: (tool.isCommand ? "$ " + (tool.modelData.summary || "") + (tool.output.length > 0 ? "\n" : "") : "") + tool.output
                                }
                            }
                        }
                        Text {
                            visible: tool.output.length === 0 && tool.result.length > 0
                            Layout.fillWidth: true
                            wrapMode: Text.WordWrap
                            text: tool.result
                            color: tool.state === "failed" ? Theme.danger : Theme.textSecondary
                            font.family: Theme.sans
                            font.pixelSize: 12
                        }
                        RowLayout {
                            spacing: 6
                            ChipButton {
                                visible: (tool.modelData.diffPath || "").length > 0
                                text: Format.tr(ipc.strings, "ViewDiff", "View diff")
                                onClicked: ipc.focusDiff(tool.modelData.diffPath)
                            }
                            ChipButton {
                                visible: tool.isBrowser
                                text: Format.tr(ipc.strings, "OpenBrowser", "Open browser")
                                onClicked: ipc.requestPanel("browser")
                            }
                            ChipButton {
                                visible: tool.output.length > 0
                                text: Format.tr(ipc.strings, "Copy", "Copy")
                                onClicked: ipc.copyText(tool.output)
                            }
                            Item { Layout.fillWidth: true }
                        }
                    }
                }
            }
        }
    }
}

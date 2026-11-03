import QtQuick
import QtQuick.Layouts

// A reply from the model: fenced code in code cards, and the prose drawn block by block,
// with headings, lists (numbered, bulleted, tasks), quotes, GitHub-style callouts, tables,
// rules, and inline code, links and emphasis.
ColumnLayout {
    id: view
    property string text: ""
    spacing: 14

    // Colors the inline renderer bakes into rich text.
    readonly property var inlineColors: ({
        code: Theme.light ? "#0f000000" : "#1affffff",
        codeText: Theme.light ? "#be185d" : "#f9a8d4",
        link: Theme.accentSecondary,
        mono: Theme.mono
    })
    // A touch more leading than the default, like a reading view.
    function rich(s) { return "<div style=\"line-height:138%\">" + Format.mdInline(s, inlineColors) + "</div>" }

    // Code fences first, then each prose part split into its own blocks.
    readonly property var blocks: {
        const out = []
        Format.splitBlocks(view.text).forEach(function (part) {
            if (part.code) out.push({ type: "code", lang: part.lang, text: part.text })
            else Format.mdBlocks(part.text).forEach(function (b) { out.push(b) })
        })
        return out
    }

    // Selectable rich text that opens links.
    component Prose: TextEdit {
        readOnly: true
        selectByMouse: true
        selectionColor: Theme.accent
        selectedTextColor: "white"
        wrapMode: TextEdit.Wrap
        textFormat: TextEdit.RichText
        color: Theme.text

        font.family: Theme.sans
        font.pixelSize: Theme.chatPx
        onLinkActivated: (link) => Qt.openUrlExternally(link)
        HoverHandler { cursorShape: parent.hoveredLink.length > 0 ? Qt.PointingHandCursor : Qt.IBeamCursor }
    }

    Repeater {
        model: view.blocks
        delegate: Loader {
            id: block
            required property var modelData
            required property int index
            Layout.fillWidth: true
            // Headings breathe a little more above, except at the very top.
            Layout.topMargin: modelData.type === "heading" && index > 0 ? (modelData.level <= 2 ? 10 : 4) : 0
            sourceComponent: {
                switch (modelData.type) {
                case "code": return codeBlock
                case "heading": return heading
                case "list": return list
                case "quote": return quote
                case "callout": return callout
                case "table": return table
                case "hr": return rule
                default: return para
                }
            }
            onLoaded: item.width = Qt.binding(function () { return block.width })

            Component {
                id: para
                Prose { text: view.rich(block.modelData.text) }
            }

            Component {
                id: heading
                Column {
                    spacing: 6
                    Prose {
                        width: parent.width
                        text: view.rich(block.modelData.text)
                        font.pixelSize: [0, Math.round(Theme.chatPx * 1.6), Math.round(Theme.chatPx * 1.35), Math.round(Theme.chatPx * 1.15),
                                         Theme.chatPx + 1, Theme.chatPx, Theme.chatPx][block.modelData.level]
                        font.weight: block.modelData.level <= 2 ? Font.Bold : Font.DemiBold
                        font.letterSpacing: block.modelData.level <= 2 ? -0.3 : 0
                        color: block.modelData.level >= 5 ? Theme.textSecondary : Theme.text
                    }
                    Rectangle {
                        visible: block.modelData.level <= 2
                        width: parent.width
                        height: 1
                        color: Theme.borderSubtle
                    }
                }
            }

            Component {
                id: list
                Column {
                    spacing: 5
                    Repeater {
                        model: block.modelData.items
                        delegate: Item {
                            id: entry
                            required property var modelData
                            required property int index
                            width: parent.width
                            height: Math.max(entryText.implicitHeight, 20)
                            readonly property real indent: modelData.depth * 22
                            // Bullet, number, or checkbox
                            Item {
                                x: entry.indent
                                width: 22
                                height: Theme.chatPx * 1.45
                                Rectangle {
                                    visible: entry.modelData.checked === null && !entry.modelData.ordered
                                    anchors.centerIn: parent
                                    anchors.horizontalCenterOffset: -3
                                    width: entry.modelData.depth === 0 ? 6 : 5
                                    height: width
                                    radius: entry.modelData.depth % 2 === 0 ? width / 2 : 1
                                    color: entry.modelData.depth === 0 ? Theme.accentSecondary : Theme.textDim
                                }
                                Text {
                                    visible: entry.modelData.checked === null && entry.modelData.ordered
                                    anchors.verticalCenter: parent.verticalCenter
                                    anchors.right: parent.right
                                    anchors.rightMargin: 6
                                    text: entry.modelData.num + "."
                                    color: Theme.accentSecondary
                                    font.family: Theme.sans
                                    font.pixelSize: Theme.chatPx
                                    font.weight: Font.DemiBold
                                }
                                Rectangle {
                                    visible: entry.modelData.checked !== null
                                    anchors.centerIn: parent
                                    anchors.horizontalCenterOffset: -3
                                    width: 14
                                    height: 14
                                    radius: 4
                                    color: entry.modelData.checked ? Theme.accent : "transparent"
                                    border.width: entry.modelData.checked ? 0 : 1.5
                                    border.color: Theme.textDim
                                    Icon { anchors.centerIn: parent; visible: entry.modelData.checked === true; name: "check"; size: 10; color: "white" }
                                }
                            }
                            Prose {
                                id: entryText
                                x: entry.indent + 24
                                width: parent.width - x
                                text: view.rich(entry.modelData.text)
                                color: entry.modelData.checked === true ? Theme.textDim : Theme.text
                            }
                        }
                    }
                }
            }

            Component {
                id: quote
                Item {
                    implicitHeight: quoteText.implicitHeight + 8
                    Rectangle { width: 3; height: parent.height; radius: 1.5; color: Theme.accentSoft }
                    Prose {
                        id: quoteText
                        x: 16
                        y: 4
                        width: parent.width - 16
                        text: view.rich(block.modelData.text)
                        color: Theme.textSecondary
                    }
                }
            }

            Component {
                id: callout
                Rectangle {
                    id: calloutCard
                    readonly property var look: ({
                        note: { color: Theme.info, icon: "info", label: "Note" },
                        tip: { color: Theme.success, icon: "zap", label: "Tip" },
                        important: { color: Theme.accentSecondary, icon: "alert-circle", label: "Important" },
                        warning: { color: Theme.warning, icon: "alert-triangle", label: "Warning" },
                        caution: { color: Theme.danger, icon: "alert-octagon", label: "Caution" }
                    })[block.modelData.kind] || ({ color: Theme.info, icon: "info", label: "Note" })
                    implicitHeight: calloutColumn.implicitHeight + 24
                    radius: 10
                    color: Qt.rgba(look.color.r, look.color.g, look.color.b, 0.08)
                    border.width: 1
                    border.color: Qt.rgba(look.color.r, look.color.g, look.color.b, 0.30)
                    Rectangle { width: 3; height: parent.height - 16; y: 8; x: 0; radius: 1.5; color: parent.look.color }
                    Column {
                        id: calloutColumn
                        x: 16
                        y: 12
                        width: parent.width - 30
                        spacing: 6
                        Row {
                            spacing: 7
                            Icon { anchors.verticalCenter: parent.verticalCenter; name: calloutCard.look.icon; size: 15; color: calloutCard.look.color }
                            Text {
                                anchors.verticalCenter: parent.verticalCenter
                                text: calloutCard.look.label
                                color: calloutCard.look.color
                                font.family: Theme.sans
                                font.pixelSize: Theme.chatPx - 1
                                font.weight: Font.DemiBold
                            }
                        }
                        Prose {
                            visible: block.modelData.text.length > 0
                            width: parent.width
                            text: view.rich(block.modelData.text)
                        }
                    }
                }
            }

            Component {
                id: table
                Rectangle {
                    id: tableCard
                    readonly property int columns: block.modelData.header.length
                    // Each column gets room by how much text it holds (within limits), so a short
                    // "Name" column does not take half the table.
                    readonly property var widths: {
                        const all = [block.modelData.header].concat(block.modelData.rows)
                        const weights = []
                        for (let c = 0; c < columns; c++) {
                            let longest = 3
                            all.forEach(function (row) { longest = Math.max(longest, String(row[c] || "").length) })
                            weights.push(Math.min(60, longest))
                        }
                        const sum = weights.reduce(function (a, b) { return a + b }, 0)
                        return weights.map(function (w) { return w / sum })
                    }
                    function xAt(i) {
                        let x = 0
                        for (let c = 0; c < i; c++) x += widths[c] * tableColumn.width
                        return x
                    }
                    implicitHeight: tableColumn.implicitHeight + 2
                    radius: 10
                    color: "transparent"
                    border.width: 1
                    border.color: Theme.border
                    clip: true
                    Column {
                        id: tableColumn
                        x: 1
                        y: 1
                        width: parent.width - 2
                        Repeater {
                            model: [block.modelData.header].concat(block.modelData.rows)
                            delegate: Rectangle {
                                id: tableRow
                                required property var modelData
                                required property int index
                                readonly property bool head: index === 0
                                width: tableColumn.width
                                height: rowCells.implicitHeight
                                color: head ? (Theme.light ? "#08000000" : "#0affffff")
                                     : (index % 2 === 0 ? (Theme.light ? "#04000000" : "#05ffffff") : "transparent")
                                Rectangle { visible: tableRow.index > 0; width: parent.width; height: 1; color: Theme.borderSubtle }
                                // Column lines run the full height of the row, whichever cell is tallest.
                                Repeater {
                                    model: Math.max(0, tableCard.columns - 1)
                                    delegate: Rectangle {
                                        required property int index
                                        x: tableCard.xAt(index + 1)
                                        width: 1
                                        height: tableRow.height
                                        color: Theme.borderSubtle
                                    }
                                }
                                Row {
                                    id: rowCells
                                    width: parent.width
                                    Repeater {
                                        model: tableRow.modelData
                                        delegate: Item {
                                            required property var modelData
                                            required property int index
                                            width: rowCells.width * (tableCard.widths[index] || 0)
                                            height: cellText.implicitHeight + 16
                                            Prose {
                                                id: cellText
                                                x: 10
                                                y: 8
                                                width: parent.width - 20
                                                text: view.rich(modelData)
                                                font.pixelSize: Theme.chatPx - 1
                                                font.weight: tableRow.head ? Font.DemiBold : Font.Normal
                                                horizontalAlignment: block.modelData.align[index] === "right" ? TextEdit.AlignRight
                                                                   : block.modelData.align[index] === "center" ? TextEdit.AlignHCenter : TextEdit.AlignLeft
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }

            Component {
                id: rule
                Item {
                    implicitHeight: 17
                    Rectangle { anchors.verticalCenter: parent.verticalCenter; width: parent.width; height: 1; color: Theme.border }
                }
            }

            Component {
                id: codeBlock
                CodeBlock {
                    language: block.modelData.lang
                    code: block.modelData.text
                }
            }
        }
    }
}

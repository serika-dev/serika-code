import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

// Files of a diff (from git), each with a header and line-numbered changes. Used by the Git
// panel for stashes, commits and working changes. `files` is a list of { path, diff }.
ColumnLayout {
    id: view
    property var files: []
    property int maxLines: 600
    property var closed: ({})
    spacing: 6

    Repeater {
        model: view.files
        delegate: Rectangle {
            id: card
            required property var modelData
            readonly property bool open: view.closed[modelData.path] !== true
            readonly property var stats: Format.diffStats(modelData.diff)
            readonly property var rows: open ? Format.diffLines(modelData.diff) : []
            Layout.fillWidth: true
            implicitHeight: head.height + (open ? body.implicitHeight : 0)
            radius: 8
            color: Theme.bg
            border.width: 1
            border.color: Theme.borderSubtle
            clip: true

            Rectangle {
                id: head
                width: parent.width
                height: 32
                color: headArea.containsMouse ? Theme.hover : "transparent"
                radius: 8
                RowLayout {
                    anchors.fill: parent
                    anchors.leftMargin: 8
                    anchors.rightMargin: 10
                    spacing: 6
                    Icon {
                        name: "chevron-right"
                        size: 12
                        color: Theme.textDim
                        rotation: card.open ? 90 : 0
                        Behavior on rotation { NumberAnimation { duration: Theme.motionFast } }
                    }
                    Text {
                        Layout.fillWidth: true
                        text: card.modelData.path
                        elide: Text.ElideMiddle
                        color: Theme.text
                        font.family: Theme.mono
                        font.pixelSize: 12
                    }
                    Text { text: "+" + card.stats.add; color: Theme.success; font.family: Theme.mono; font.pixelSize: 11 }
                    Text { text: "−" + card.stats.del; color: Theme.danger; font.family: Theme.mono; font.pixelSize: 11 }
                }
                MouseArea {
                    id: headArea
                    anchors.fill: parent
                    hoverEnabled: true
                    cursorShape: Qt.PointingHandCursor
                    onClicked: {
                        const next = Object.assign({}, view.closed)
                        next[card.modelData.path] = card.open
                        view.closed = next
                    }
                }
            }

            Column {
                id: body
                y: head.height
                width: parent.width
                visible: card.open
                Rectangle { width: parent.width; height: 1; color: Theme.borderSubtle }
                Flickable {
                    id: hscroll
                    width: parent.width
                    height: lines.height
                    contentWidth: Math.max(width, lines.width)
                    flickableDirection: Flickable.HorizontalFlick
                    boundsBehavior: Flickable.StopAtBounds
                    interactive: contentWidth > width
                    clip: true
                    ScrollBar.horizontal: ScrollBar { policy: ScrollBar.AsNeeded; height: 6 }
                    Column {
                        id: lines
                        width: Math.max(hscroll.width, implicitWidth)
                        Repeater {
                            model: card.rows.slice(0, view.maxLines)
                            delegate: Rectangle {
                                id: lineRow
                                required property var modelData
                                readonly property string kind: modelData.kind
                                width: Math.max(lines.width, lineText.implicitWidth + 92)
                                implicitWidth: lineText.implicitWidth + 92
                                height: kind === "hunk" ? 22 : 19
                                color: kind === "add" ? (Theme.light ? "#e6f6ec" : "#0f2a1b")
                                     : kind === "del" ? (Theme.light ? "#fdecec" : "#2f1416")
                                     : kind === "hunk" ? Theme.accentSoft : "transparent"
                                Text {
                                    width: 36
                                    anchors.verticalCenter: parent.verticalCenter
                                    horizontalAlignment: Text.AlignRight
                                    text: lineRow.modelData.old
                                    color: Theme.textDim
                                    font.family: Theme.mono
                                    font.pixelSize: 11
                                }
                                Text {
                                    x: 40
                                    width: 36
                                    anchors.verticalCenter: parent.verticalCenter
                                    horizontalAlignment: Text.AlignRight
                                    text: lineRow.modelData.new
                                    color: Theme.textDim
                                    font.family: Theme.mono
                                    font.pixelSize: 11
                                }
                                Text {
                                    x: 82
                                    anchors.verticalCenter: parent.verticalCenter
                                    text: lineRow.kind === "add" ? "+" : (lineRow.kind === "del" ? "−" : "")
                                    color: lineRow.kind === "add" ? Theme.success : Theme.danger
                                    font.family: Theme.mono
                                    font.pixelSize: 12
                                }
                                Text {
                                    id: lineText
                                    x: 92
                                    anchors.verticalCenter: parent.verticalCenter
                                    text: lineRow.kind === "hunk" ? (lineRow.modelData.text.length > 0 ? "⋯  " + lineRow.modelData.text : "⋯") : lineRow.modelData.text
                                    textFormat: Text.PlainText
                                    color: lineRow.kind === "hunk" ? Theme.accentSecondary : Theme.text
                                    font.family: Theme.mono
                                    font.pixelSize: 12
                                }
                            }
                        }
                    }
                }
                Text {
                    visible: card.rows.length > view.maxLines
                    leftPadding: 12
                    topPadding: 6
                    bottomPadding: 8
                    text: Format.tr(ipc.strings, "MoreDiffLines", "{n} more lines. Copy the diff to see all of it.", { n: card.rows.length - view.maxLines })
                    color: Theme.textDim
                    font.family: Theme.sans
                    font.pixelSize: 12
                }
            }
        }
    }
    Text {
        visible: view.files.length === 0
        text: Format.tr(ipc.strings, "GitNoFiles", "No file changes.")
        color: Theme.textDim
        font.family: Theme.sans
        font.pixelSize: 12
    }
}

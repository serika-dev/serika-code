import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

// Files the agent changed in this chat: a summary, then one collapsible card per file
// with a line-numbered diff.
Item {
    id: panel
    readonly property var dict: ipc.strings
    // Paths the user collapsed. Everything starts open.
    property var collapsed: ({})
    property string focusPath: ""
    readonly property var totals: {
        let add = 0, del = 0
        for (let i = 0; i < ipc.diffs.length; ++i) {
            const s = Format.diffStats(ipc.diffs[i].diff)
            add += s.add
            del += s.del
        }
        return { add: add, del: del }
    }
    readonly property int maxLines: 400

    function fileName(path) { return path.split("/").pop().split("\\").pop() }
    function folderOf(path) {
        const parts = path.replace(/\\/g, "/").split("/")
        parts.pop()
        return parts.join("/")
    }
    function absolute(path) {
        if (/^[A-Za-z]:[\\/]/.test(path) || path.indexOf("/") === 0) return path
        return ipc.projectPath.replace(/[\\/]+$/, "") + "/" + path
    }
    function toggle(path) {
        const next = Object.assign({}, collapsed)
        next[path] = !(collapsed[path] === true)
        collapsed = next
    }
    function setAll(closed) {
        const next = {}
        for (let i = 0; i < ipc.diffs.length; ++i) next[ipc.diffs[i].path] = closed
        collapsed = next
    }
    function allDiffs() {
        return ipc.diffs.map(function (d) { return d.diff }).join("\n")
    }

    Connections {
        target: ipc
        function onDiffFocus(path) {
            const next = Object.assign({}, panel.collapsed)
            next[path] = false
            panel.collapsed = next
            panel.focusPath = path
            focusTimer.restart()
        }
    }
    // Scroll after the cards have laid out.
    Timer {
        id: focusTimer
        interval: 60
        onTriggered: {
            for (let i = 0; i < files.count; ++i) {
                const card = files.itemAt(i)
                if (card && card.path === panel.focusPath) {
                    flick.contentY = Math.max(0, Math.min(card.y, flick.contentHeight - flick.height))
                    break
                }
            }
        }
    }

    // Summary
    Rectangle {
        id: summary
        visible: ipc.diffs.length > 0
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.top: parent.top
        height: visible ? 44 : 0
        color: "transparent"
        RowLayout {
            anchors.fill: parent
            anchors.leftMargin: 14
            anchors.rightMargin: 8
            spacing: 8
            Text {
                text: ipc.diffs.length === 1 ? Format.tr(panel.dict, "OneFileChanged", "1 file changed") : Format.tr(panel.dict, "FilesChanged", "{n} files changed", { n: ipc.diffs.length })
                color: Theme.text
                font.family: Theme.sans
                font.pixelSize: 13
                font.weight: Font.DemiBold
            }
            Text { text: "+" + panel.totals.add; color: Theme.success; font.family: Theme.mono; font.pixelSize: 12 }
            Text { text: "−" + panel.totals.del; color: Theme.danger; font.family: Theme.mono; font.pixelSize: 12 }
            Item { Layout.fillWidth: true }
            IconButton {
                implicitWidth: 28
                implicitHeight: 28
                icon: "chevron-up"
                iconSize: 15
                tip: Format.tr(panel.dict, "CollapseAll", "Collapse all")
                onClicked: panel.setAll(true)
            }
            IconButton {
                implicitWidth: 28
                implicitHeight: 28
                icon: "chevron-down"
                iconSize: 15
                tip: Format.tr(panel.dict, "ExpandAll", "Expand all")
                onClicked: panel.setAll(false)
            }
            IconButton {
                id: copyAll
                property bool copied: false
                implicitWidth: 28
                implicitHeight: 28
                icon: copied ? "check" : "copy"
                iconSize: 14
                tip: Format.tr(panel.dict, "CopyAllDiffs", "Copy all changes as a patch")
                onClicked: { ipc.copyText(panel.allDiffs()); copied = true; copyAllReset.restart() }
                Timer { id: copyAllReset; interval: 1500; onTriggered: copyAll.copied = false }
            }
        }
        Rectangle { anchors.bottom: parent.bottom; width: parent.width; height: 1; color: Theme.borderSubtle }
    }

    Flickable {
        id: flick
        visible: ipc.diffs.length > 0
        anchors.top: summary.bottom
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.bottom: parent.bottom
        contentHeight: column.implicitHeight + 24
        boundsBehavior: Flickable.StopAtBounds
        clip: true
        ScrollBar.vertical: ScrollBar { policy: ScrollBar.AsNeeded; width: 8 }

        Column {
            id: column
            x: 10
            y: 10
            width: flick.width - 20
            spacing: 10

            Repeater {
                id: files
                model: ipc.diffs
                delegate: Rectangle {
                    id: fileCard
                    required property var modelData
                    required property int index
                    readonly property string path: modelData.path
                    readonly property bool open: panel.collapsed[path] !== true
                    readonly property var stats: Format.diffStats(modelData.diff)
                    readonly property var rows: open ? Format.diffLines(modelData.diff) : []
                    readonly property bool focused: panel.focusPath === path
                    width: column.width
                    height: head.height + (open ? body.height : 0)
                    radius: 10
                    color: Theme.bg
                    border.width: 1
                    border.color: focused ? Theme.accent : Theme.borderSubtle
                    clip: true

                    // File header
                    Rectangle {
                        id: head
                        width: parent.width
                        height: 40
                        color: headHover.hovered ? Theme.hover : "transparent"
                        radius: 10
                        HoverHandler { id: headHover }
                        TapHandler { onTapped: panel.toggle(fileCard.path) }
                        RowLayout {
                            anchors.fill: parent
                            anchors.leftMargin: 10
                            anchors.rightMargin: 6
                            spacing: 8
                            Icon {
                                name: "chevron-right"
                                size: 14
                                color: Theme.textDim
                                rotation: fileCard.open ? 90 : 0
                                Behavior on rotation { NumberAnimation { duration: 120 } }
                            }
                            Icon { name: "file-text"; size: 14; color: Theme.textSecondary }
                            Text {
                                text: panel.fileName(fileCard.path)
                                color: Theme.text
                                font.family: Theme.sans
                                font.pixelSize: 13
                                font.weight: Font.DemiBold
                            }
                            Text {
                                Layout.fillWidth: true
                                text: panel.folderOf(fileCard.path)
                                elide: Text.ElideMiddle
                                color: Theme.textDim
                                font.family: Theme.sans
                                font.pixelSize: 12
                            }
                            // Hover actions
                            Row {
                                visible: headHover.hovered
                                spacing: 0
                                IconButton {
                                    id: copyOne
                                    property bool copied: false
                                    implicitWidth: 26
                                    implicitHeight: 26
                                    icon: copied ? "check" : "copy"
                                    iconSize: 13
                                    tip: Format.tr(panel.dict, "CopyDiff", "Copy diff")
                                    onClicked: { ipc.copyText(fileCard.modelData.diff); copied = true; copyOneReset.restart() }
                                    Timer { id: copyOneReset; interval: 1500; onTriggered: copyOne.copied = false }
                                }
                                IconButton {
                                    implicitWidth: 26
                                    implicitHeight: 26
                                    icon: "external-link"
                                    iconSize: 13
                                    tip: Format.tr(panel.dict, "OpenFile", "Open file")
                                    onClicked: Qt.openUrlExternally("file:///" + panel.absolute(fileCard.path).replace(/\\/g, "/"))
                                }
                                IconButton {
                                    implicitWidth: 26
                                    implicitHeight: 26
                                    icon: "folder"
                                    iconSize: 13
                                    tip: Format.tr(panel.dict, "RevealFile", "Show in folder")
                                    onClicked: ipc.revealFile(panel.absolute(fileCard.path))
                                }
                            }
                            Text { text: "+" + fileCard.stats.add; color: Theme.success; font.family: Theme.mono; font.pixelSize: 12 }
                            Text { text: "−" + fileCard.stats.del; color: Theme.danger; font.family: Theme.mono; font.pixelSize: 12 }
                            // Five-block change bar, as on GitHub
                            Row {
                                spacing: 2
                                Repeater {
                                    model: 5
                                    delegate: Rectangle {
                                        required property int index
                                        readonly property int total: fileCard.stats.add + fileCard.stats.del
                                        readonly property int adds: total === 0 ? 0 : Math.round(5 * fileCard.stats.add / total)
                                        width: 7
                                        height: 7
                                        radius: 1.5
                                        color: total === 0 ? Theme.active : (index < adds ? Theme.success : Theme.danger)
                                    }
                                }
                            }
                        }
                    }

                    // Line-numbered diff
                    Column {
                        id: body
                        y: head.height
                        width: parent.width
                        visible: fileCard.open
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
                                    model: fileCard.rows.slice(0, panel.maxLines)
                                    delegate: Rectangle {
                                        id: lineRow
                                        required property var modelData
                                        readonly property string kind: modelData.kind
                                        width: Math.max(lines.width, lineText.implicitWidth + 92)
                                        implicitWidth: lineText.implicitWidth + 92
                                        height: kind === "hunk" ? 24 : 20
                                        color: kind === "add" ? (Theme.light ? "#e6f6ec" : "#0f2a1b")
                                             : kind === "del" ? (Theme.light ? "#fdecec" : "#2f1416")
                                             : kind === "hunk" ? Theme.accentSoft : "transparent"
                                        Text {
                                            x: 0
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
                            visible: fileCard.rows.length > panel.maxLines
                            leftPadding: 12
                            topPadding: 6
                            bottomPadding: 8
                            text: Format.tr(panel.dict, "MoreDiffLines", "{n} more lines. Copy the diff to see all of it.", { n: fileCard.rows.length - panel.maxLines })
                            color: Theme.textDim
                            font.family: Theme.sans
                            font.pixelSize: 12
                        }
                    }
                }
            }
        }
    }

    // Nothing changed yet
    ColumnLayout {
        visible: ipc.diffs.length === 0
        anchors.centerIn: parent
        width: Math.min(parent.width - 48, 300)
        spacing: 12
        Rectangle {
            Layout.alignment: Qt.AlignHCenter
            Layout.preferredWidth: 52
            Layout.preferredHeight: 52
            radius: 16
            color: Theme.accentSoft
            Icon { anchors.centerIn: parent; name: "git-branch"; size: 24; color: Theme.accentSecondary }
        }
        Text {
            Layout.fillWidth: true
            horizontalAlignment: Text.AlignHCenter
            text: Format.tr(panel.dict, "NoChangesTitle", "No changes yet")
            color: Theme.text
            font.family: Theme.brand
            font.pixelSize: 17
            font.weight: Font.DemiBold
        }
        Text {
            Layout.fillWidth: true
            horizontalAlignment: Text.AlignHCenter
            wrapMode: Text.WordWrap
            text: Format.tr(panel.dict, "NoChangesHelp", "Files Serika edits in this chat show up here, with every line it added or removed.")
            color: Theme.textDim
            font.family: Theme.sans
            font.pixelSize: 13
        }
    }
}

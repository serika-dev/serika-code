import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

// A stash, merge or rebase that stopped on conflicts, worked through file by file. Each place
// the two sides differ shows both versions with "keep this one / that one / both"; a file is
// saved as soon as every place in it is decided. Undo puts everything back as it was, Finish
// wraps up (a popped stash is only removed then), and Serika can be asked to do it instead.
Rectangle {
    id: card
    property var info: ({})
    property string root: ""
    property bool busy: false
    readonly property var dict: ipc.strings
    function tr(key, fallback, values) { return Format.tr(dict, key, fallback, values) }

    readonly property var pending: info.files || []
    readonly property var all: (info.all && info.all.length > 0) ? info.all : pending
    readonly property int done: all.length - pending.length
    readonly property bool finished: pending.length === 0
    readonly property string kind: info.kind || "stash"
    readonly property bool mineIncoming: info.theirs === "Your changes"
    function brief(text) { return text.length > 30 ? text.substring(0, 29) + "…" : text }
    readonly property string oursName: brief(info.ours || tr("GitYourBranch", "Your branch"))
    readonly property string theirsName: brief(info.theirs || tr("GitIncoming", "Incoming"))
    // One word for each side, for the buttons
    readonly property string oursWord: kind === "rebase" ? tr("GitWordUpstream", "upstream") : mineIncoming ? tr("GitWordStash", "stash") : tr("GitWordYours", "yours")
    readonly property string theirsWord: mineIncoming || kind === "rebase" ? tr("GitWordYours", "yours") : kind === "stash" ? tr("GitWordStash", "stash") : tr("GitWordIncoming", "incoming")

    property string openPath: ""
    property var files: ({})      // path -> {segments, whole}
    property var picks: ({})      // path -> ["ours" | "theirs" | "both" | ""] per conflict
    property bool undoArmed: false
    Timer { id: undoDisarm; interval: 4000; onTriggered: card.undoArmed = false }

    // One side of a conflict: its name and its lines.
    component Side: Rectangle {
        id: side
        property string label: ""
        property string code: ""
        property color tint: Theme.accent
        property bool picked: false
        property bool dimmed: false
        Layout.fillWidth: true
        implicitHeight: sideCol.implicitHeight + 12
        radius: 7
        opacity: dimmed ? 0.45 : 1
        color: Qt.rgba(tint.r, tint.g, tint.b, picked ? 0.16 : 0.08)
        border.width: picked ? 1 : 0
        border.color: Qt.rgba(tint.r, tint.g, tint.b, 0.5)
        Behavior on opacity { NumberAnimation { duration: Theme.motionFast } }
        Rectangle { width: 3; height: parent.height - 12; y: 6; x: 0; radius: 1.5; color: side.tint }
        Column {
            id: sideCol
            x: 12
            y: 6
            width: parent.width - 20
            spacing: 3
            Text {
                text: side.label
                color: side.tint
                font.family: Theme.sans
                font.pixelSize: 11
                font.weight: Font.DemiBold
            }
            Text {
                width: parent.width
                text: side.code.length > 0 ? side.code.replace(/\n$/, "") : card.tr("GitNothingHere", "(nothing)")
                color: side.code.length > 0 ? Theme.text : Theme.textDim
                font.family: Theme.mono
                font.pixelSize: 12
                font.italic: side.code.length === 0
                wrapMode: Text.WrapAnywhere
                maximumLineCount: 16
                elide: Text.ElideRight
            }
        }
    }

    function isLeft(path) { return pending.indexOf(path) >= 0 }
    function open(path) {
        if (openPath === path) { openPath = ""; return }
        expand(path)
    }
    function expand(path) {
        openPath = path
        if (isLeft(path)) ipc.gitConflictFile(path)
    }
    function conflictsOf(path) {
        const f = files[path]
        return f ? f.segments.filter(function (s) { return s.kind === "conflict" }) : []
    }
    function pick(path, i, side) {
        const list = (picks[path] || []).slice()
        list[i] = side
        const next = Object.assign({}, picks)
        next[path] = list
        picks = next
        const total = conflictsOf(path).length
        for (let k = 0; k < total; k++) if (!list[k]) return
        ipc.gitAction("conflict.resolve", -1, path + "\t" + JSON.stringify(list), tr("GitSavingFile", "Saving {f}", { f: path }))
    }
    function whole(path, side) {
        ipc.gitAction(side === "ours" ? "conflict.ours" : "conflict.theirs", -1, path, tr("GitSavingFile", "Saving {f}", { f: path }))
    }
    function askSerika() {
        const list = pending.map(function (f) { return "- " + f }).join("\n")
        ipc.fillComposer(tr("GitAskResolve",
            "Resolve the git conflicts in this project: \"{a}\" against \"{b}\". These files still have conflict markers:\n{files}\n\n"
            + "For each one, read both sides and keep what each side meant to do, then remove the markers. "
            + "Where they really contradict each other, keep \"{b}\" and tell me. Stage each file with git add when it is done, and do not commit.",
            { a: info.ours || "ours", b: info.theirs || "theirs", files: list }))
    }

    // Lines of a stretch both sides agree on, shortened to the ones next to a conflict.
    function context(seg, index, count) {
        const lines = seg.text.replace(/\n$/, "").split("\n")
        const head = index === 0 ? 0 : 2
        const tail = index === count - 1 ? 0 : 2
        if (lines.length <= head + tail + 1) return { top: lines.join("\n"), hidden: 0, bottom: "" }
        return { top: lines.slice(0, head).join("\n"), hidden: lines.length - head - tail, bottom: lines.slice(lines.length - tail).join("\n") }
    }

    Connections {
        target: ipc
        function onGitConflictReady(path, file) {
            const old = card.files[path]
            const next = Object.assign({}, card.files)
            next[path] = file
            card.files = next
            // Keep choices unless the file itself changed.
            if (!old || JSON.stringify(old.segments) !== JSON.stringify(file.segments)) {
                const p = Object.assign({}, card.picks)
                p[path] = []
                card.picks = p
            }
        }
    }
    onPendingChanged: {
        // The file just finished folds away and the next one opens.
        if (openPath && !isLeft(openPath)) openPath = ""
        if (!openPath && pending.length > 0) expand(pending[0])
        else if (openPath) ipc.gitConflictFile(openPath)
    }
    Component.onCompleted: if (!openPath && pending.length > 0) expand(pending[0])

    implicitHeight: body.implicitHeight + 28
    radius: 12
    color: finished ? Theme.successSoft : Theme.warningSoft
    border.width: 1
    border.color: finished ? Qt.rgba(Theme.success.r, Theme.success.g, Theme.success.b, 0.4)
                           : Qt.rgba(Theme.warning.r, Theme.warning.g, Theme.warning.b, 0.4)
    Behavior on color { ColorAnimation { duration: Theme.motionBase } }

    ColumnLayout {
        id: body
        x: 14
        y: 14
        width: parent.width - 28
        spacing: 10

        // What happened
        RowLayout {
            Layout.fillWidth: true
            spacing: 10
            Rectangle {
                Layout.alignment: Qt.AlignTop
                Layout.preferredWidth: 34
                Layout.preferredHeight: 34
                radius: 10
                color: card.finished ? Theme.successSoft : Theme.warningSoft
                Icon {
                    anchors.centerIn: parent
                    name: card.finished ? "check-circle" : "git-merge"
                    size: 17
                    color: card.finished ? Theme.success : Theme.warning
                }
            }
            ColumnLayout {
                Layout.fillWidth: true
                spacing: 2
                Text {
                    Layout.fillWidth: true
                    wrapMode: Text.Wrap
                    color: Theme.text
                    font.family: Theme.sans
                    font.pixelSize: 14
                    font.weight: Font.DemiBold
                    text: card.finished ? card.tr("GitConflictSorted", "All conflicts sorted")
                        : card.kind === "merge" ? card.tr("GitConflictMerge", "Merging {s} stopped on conflicts", { s: card.info.source || card.tr("GitABranch", "a branch") })
                        : card.kind === "rebase" ? card.tr("GitConflictRebase", "The rebase stopped on conflicts")
                        : card.kind === "cherry-pick" ? card.tr("GitConflictPick", "The cherry-pick stopped on conflicts")
                        : card.mineIncoming ? card.tr("GitConflictMine", "Your changes clash with {s}", { s: card.brief(card.info.source || "") })
                        : card.tr("GitConflictStash", "{s} clashes with your branch", { s: card.brief(card.info.source || card.tr("GitTheStash", "The stash")) })
                }
                Text {
                    Layout.fillWidth: true
                    wrapMode: Text.Wrap
                    color: Theme.textSecondary
                    font.family: Theme.sans
                    font.pixelSize: 12
                    text: card.finished ? (card.kind === "stash" && card.info.drop && !card.mineIncoming
                                           ? card.tr("GitConflictDoneDrop", "Finish to keep the result and remove the stash, or Undo to put everything back.")
                                           : card.tr("GitConflictAllDone", "Finish to keep the result, or Undo to put everything back."))
                        : card.tr("GitConflictLeft", "{n} of {t} files need a decision. Pick a side for each highlighted part.", { n: card.pending.length, t: card.all.length })
                }
            }
        }

        // Progress
        Rectangle {
            Layout.fillWidth: true
            Layout.preferredHeight: 4
            radius: 2
            color: Theme.hover
            visible: card.all.length > 1
            Rectangle {
                width: parent.width * (card.all.length ? card.done / card.all.length : 0)
                height: parent.height
                radius: 2
                color: card.finished ? Theme.success : Theme.accent
                Behavior on width { NumberAnimation { duration: Theme.motionSlow; easing.type: Theme.easeEmphasis } }
            }
        }

        // Files
        Repeater {
            model: card.all
            delegate: Rectangle {
                id: fileCard
                required property string modelData
                readonly property string path: modelData
                readonly property bool resolved: !card.isLeft(path)
                readonly property bool open: card.openPath === path && !resolved
                readonly property var file: card.files[path]
                readonly property var blocks: card.conflictsOf(path)
                Layout.fillWidth: true
                implicitHeight: fileCol.implicitHeight + 2
                radius: 10
                color: Theme.bg
                border.width: 1
                border.color: open ? Theme.border : Theme.borderSubtle
                clip: true

                ColumnLayout {
                    id: fileCol
                    x: 1
                    y: 1
                    width: parent.width - 2
                    spacing: 0

                    // Row: state, name, folder
                    Item {
                        Layout.fillWidth: true
                        Layout.preferredHeight: 40
                        MouseArea {
                            id: fileArea
                            anchors.fill: parent
                            hoverEnabled: true
                            cursorShape: fileCard.resolved ? Qt.ArrowCursor : Qt.PointingHandCursor
                            onClicked: if (!fileCard.resolved) card.open(fileCard.path)
                        }
                        RowLayout {
                            anchors.fill: parent
                            anchors.leftMargin: 12
                            anchors.rightMargin: 10
                            spacing: 9
                            Icon {
                                name: fileCard.resolved ? "check-circle" : "alert-triangle"
                                size: 15
                                color: fileCard.resolved ? Theme.success : Theme.warning
                            }
                            Text {
                                text: fileCard.path.split("/").pop()
                                color: fileCard.resolved ? Theme.textSecondary : Theme.text
                                font.family: Theme.sans
                                font.pixelSize: 13
                                font.weight: Font.Medium
                                font.strikeout: false
                            }
                            Text {
                                Layout.fillWidth: true
                                text: fileCard.path.indexOf("/") >= 0 ? fileCard.path.substring(0, fileCard.path.lastIndexOf("/")) : ""
                                elide: Text.ElideMiddle
                                color: Theme.textDim
                                font.family: Theme.sans
                                font.pixelSize: 11
                            }
                            Tag {
                                visible: fileCard.resolved
                                text: card.tr("GitResolved", "Resolved")
                            }
                            Text {
                                visible: !fileCard.resolved && fileCard.blocks.length > 0
                                text: fileCard.blocks.length === 1 ? card.tr("GitOneSpot", "1 place") : card.tr("GitNSpots", "{n} places", { n: fileCard.blocks.length })
                                color: Theme.textDim
                                font.family: Theme.sans
                                font.pixelSize: 11
                            }
                            Icon {
                                visible: !fileCard.resolved
                                name: "chevron-down"
                                size: 14
                                color: Theme.textDim
                                rotation: fileCard.open ? 180 : 0
                                Behavior on rotation { NumberAnimation { duration: Theme.motionBase; easing.type: Theme.easeEmphasis } }
                            }
                        }
                    }

                    // The file, place by place
                    ColumnLayout {
                        visible: fileCard.open
                        Layout.fillWidth: true
                        Layout.leftMargin: 10
                        Layout.rightMargin: 10
                        Layout.bottomMargin: 10
                        spacing: 8

                        Text {
                            visible: !fileCard.file
                            text: card.tr("GitLoading", "Loading…")
                            color: Theme.textDim
                            font.family: Theme.sans
                            font.pixelSize: 12
                        }
                        Text {
                            visible: !!fileCard.file && fileCard.file.whole
                            Layout.fillWidth: true
                            wrapMode: Text.Wrap
                            text: card.tr("GitWholeOnly", "This file cannot be compared line by line (it is binary, or one side deleted it). Keep one side of it.")
                            color: Theme.textSecondary
                            font.family: Theme.sans
                            font.pixelSize: 12
                        }

                        Repeater {
                            model: fileCard.file ? fileCard.file.segments : []
                            delegate: Loader {
                                id: seg
                                required property var modelData
                                required property int index
                                readonly property int conflictIndex: {
                                    let n = 0
                                    const list = fileCard.file.segments
                                    for (let i = 0; i < index; i++) if (list[i].kind === "conflict") n++
                                    return n
                                }
                                Layout.fillWidth: true
                                sourceComponent: modelData.kind === "conflict" ? conflictBlock : sameBlock

                                Component {
                                    id: sameBlock
                                    Column {
                                        readonly property var c: card.context(seg.modelData, seg.index, fileCard.file.segments.length)
                                        spacing: 2
                                        Text {
                                            visible: text.length > 0
                                            width: parent.width
                                            text: parent.c.top
                                            color: Theme.textDim
                                            font.family: Theme.mono
                                            font.pixelSize: 11
                                            wrapMode: Text.WrapAnywhere
                                            maximumLineCount: 10
                                            elide: Text.ElideRight
                                        }
                                        Text {
                                            visible: parent.c.hidden > 0
                                            text: "⋯  " + card.tr("GitSameLines", "{n} lines the same on both sides", { n: parent.c.hidden })
                                            color: Theme.textDim
                                            font.family: Theme.sans
                                            font.pixelSize: 11
                                            font.italic: true
                                        }
                                        Text {
                                            visible: text.length > 0
                                            width: parent.width
                                            text: parent.c.bottom
                                            color: Theme.textDim
                                            font.family: Theme.mono
                                            font.pixelSize: 11
                                            wrapMode: Text.WrapAnywhere
                                            maximumLineCount: 10
                                            elide: Text.ElideRight
                                        }
                                    }
                                }

                                Component {
                                    id: conflictBlock
                                    Rectangle {
                                        id: block
                                        readonly property string chosen: (card.picks[fileCard.path] || [])[seg.conflictIndex] || ""
                                        implicitHeight: blockCol.implicitHeight + 16
                                        radius: 9
                                        color: Theme.card
                                        border.width: 1
                                        border.color: chosen ? Qt.rgba(Theme.success.r, Theme.success.g, Theme.success.b, 0.45) : Theme.border
                                        ColumnLayout {
                                            id: blockCol
                                            x: 8
                                            y: 8
                                            width: parent.width - 16
                                            spacing: 6

                                            Side {
                                                label: card.oursName
                                                code: seg.modelData.ours
                                                tint: Theme.success
                                                picked: block.chosen === "ours" || block.chosen === "both"
                                                dimmed: block.chosen === "theirs"
                                            }
                                            Side {
                                                label: card.theirsName
                                                code: seg.modelData.theirs
                                                tint: Theme.accentSecondary
                                                picked: block.chosen === "theirs" || block.chosen === "both"
                                                dimmed: block.chosen === "ours"
                                            }
                                            Flow {
                                                Layout.fillWidth: true
                                                spacing: 6
                                                ChipButton {
                                                    text: card.tr("GitKeepSide", "Keep {s}", { s: card.oursWord })
                                                    primary: block.chosen === "ours"
                                                    enabled: !card.busy
                                                    onClicked: card.pick(fileCard.path, seg.conflictIndex, "ours")
                                                }
                                                ChipButton {
                                                    text: card.tr("GitKeepSide", "Keep {s}", { s: card.theirsWord })
                                                    primary: block.chosen === "theirs"
                                                    enabled: !card.busy
                                                    onClicked: card.pick(fileCard.path, seg.conflictIndex, "theirs")
                                                }
                                                ChipButton {
                                                    text: card.tr("GitKeepBoth", "Keep both")
                                                    primary: block.chosen === "both"
                                                    enabled: !card.busy
                                                    onClicked: card.pick(fileCard.path, seg.conflictIndex, "both")
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }

                        // The whole file at once
                        Flow {
                            Layout.fillWidth: true
                            Layout.topMargin: 2
                            spacing: 6
                            ChipButton {
                                text: card.tr("GitAllSide", "Whole file: {s}", { s: card.oursWord })
                                iconName: "check"
                                enabled: !card.busy
                                onClicked: card.whole(fileCard.path, "ours")
                            }
                            ChipButton {
                                text: card.tr("GitAllSide", "Whole file: {s}", { s: card.theirsWord })
                                iconName: "check"
                                enabled: !card.busy
                                onClicked: card.whole(fileCard.path, "theirs")
                            }
                            ChipButton {
                                text: card.tr("GitOpenFile", "Open in editor")
                                iconName: "external-link"
                                onClicked: Qt.openUrlExternally("file:///" + (card.root + "/" + fileCard.path).replace(/^\/+/, ""))
                            }
                            ChipButton {
                                visible: !!fileCard.file && !fileCard.file.whole
                                text: card.tr("GitMarkResolved", "I fixed it myself")
                                enabled: !card.busy
                                onClicked: ipc.gitAction("conflict.mark", -1, fileCard.path, card.tr("GitSavingFile", "Saving {f}", { f: fileCard.path }))
                            }
                        }
                    }
                }
            }
        }

        // Undo, ask Serika, finish
        RowLayout {
            Layout.fillWidth: true
            Layout.topMargin: 2
            spacing: 6
            ChipButton {
                visible: !card.finished
                text: card.tr("GitAskSerika", "Ask Serika")
                iconName: "zap"
                onClicked: card.askSerika()
            }
            Item { Layout.fillWidth: true }
            ChipButton {
                danger: card.undoArmed
                text: card.undoArmed ? card.tr("GitUndoConfirm", "Click again to undo") : card.tr("GitUndo", "Undo")
                iconName: "rotate-ccw"
                enabled: !card.busy
                onClicked: {
                    if (!card.undoArmed) { card.undoArmed = true; undoDisarm.restart(); return }
                    card.undoArmed = false
                    ipc.gitAction("conflict.abort", -1, "", card.tr("GitUndoing", "Putting things back"))
                }
            }
            AccentButton {
                implicitHeight: 30
                enabled: card.finished && !card.busy
                opacity: enabled ? 1 : 0.5
                text: card.kind === "merge" ? card.tr("GitCommitMerge", "Commit merge")
                    : card.kind === "rebase" || card.kind === "cherry-pick" ? card.tr("GitContinue", "Continue")
                    : card.info.drop && !card.mineIncoming ? card.tr("GitFinishDrop", "Finish and remove stash")
                    : card.tr("GitFinish", "Finish")
                onClicked: ipc.gitAction("conflict.finish", -1, "", card.tr("GitFinishing", "Finishing"))
            }
        }
    }
}

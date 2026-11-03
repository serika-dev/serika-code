import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

// Git for the open project: branch and upstream, incoming and outgoing commits, working
// changes, history, and stashes. Stashes are listed by message, so a stash per ticket can be
// found by its number, looked at file by file, and applied, popped, or dropped.
Item {
    id: panel
    readonly property var dict: ipc.strings
    readonly property var g: ipc.git
    readonly property bool repo: g.repo === true
    readonly property string busy: g.busy || ""
    property string view: "stashes"
    property string filter: ""
    property string openKey: ""
    property var diffs: ({})
    property string loadingKey: ""
    property string note: ""
    property bool noteError: false
    property int dropArmed: -1
    property bool stashing: false
    property string stashSig: ""

    function tr(key, fallback, values) { return Format.tr(dict, key, fallback, values) }
    // A ticket reference in a stash message: PROJ-123, #123 or a bare number of 3+ digits.
    function ticketOf(text) {
        const m = (text || "").match(/[A-Z][A-Z0-9]+-\d+|#\d+|\b\d{3,7}\b/)
        return m ? m[0] : ""
    }
    function toggle(key, load) {
        if (openKey === key) { openKey = ""; return }
        openKey = key
        if (diffs[key] === undefined) { loadingKey = key; load() }
    }
    function say(text, error) {
        note = text
        noteError = error
        noteTimer.restart()
    }
    readonly property var stashes: (g.stashes || []).filter(function (s) {
        const q = panel.filter.trim().toLowerCase()
        return q.length === 0 || s.message.toLowerCase().indexOf(q) >= 0 || s.branch.toLowerCase().indexOf(q) >= 0
               || s.name.indexOf(q) >= 0
    })

    // A different project: start over with its repository.
    readonly property string project: ipc.projectPath
    onProjectChanged: { diffs = ({}); openKey = ""; filter = ""; ipc.gitRefresh() }
    Component.onCompleted: ipc.gitRefresh()
    onVisibleChanged: if (visible) ipc.gitRefresh()
    Timer {
        interval: 8000
        repeat: true
        running: panel.visible && Qt.application.state === Qt.ApplicationActive
        onTriggered: {
            ipc.gitRefresh()
            if (panel.openKey.indexOf("file:") === 0) {
                const path = panel.openKey.substring(5)
                const change = (panel.g.changes || []).filter(function (c) { return c.path === path })[0]
                if (change) ipc.gitFileDiff(change.path, change.untracked)
            }
        }
    }
    Timer { id: noteTimer; interval: 6000; onTriggered: panel.note = "" }
    Timer { id: disarm; interval: 4000; onTriggered: panel.dropArmed = -1 }
    Connections {
        target: ipc
        function onGitDiffReady(key, files) {
            const next = Object.assign({}, panel.diffs)
            next[key] = files
            panel.diffs = next
            if (panel.loadingKey === key) panel.loadingKey = ""
        }
        function onGitMessage(text, error) { panel.loadingKey = ""; panel.say(text, error) }
        function onGitChanged() {
            // Stash numbers shift after a pop, drop or new stash; only then do cached stash
            // diffs go stale. The open one is fetched again so it never sits on "Loading".
            const sig = (panel.g.stashes || []).map(function (s) { return s.name + "|" + s.time + "|" + s.message }).join("\n")
            if (sig === panel.stashSig) return
            panel.stashSig = sig
            const next = {}
            for (const k in panel.diffs) if (k.indexOf("stash:") !== 0) next[k] = panel.diffs[k]
            panel.diffs = next
            if (panel.openKey.indexOf("stash:") === 0) {
                const index = parseInt(panel.openKey.substring(6))
                if (index < (panel.g.stashes || []).length) { panel.loadingKey = panel.openKey; ipc.gitStashDiff(index) }
                else panel.openKey = ""
            }
        }
    }

    ColumnLayout {
        anchors.fill: parent
        spacing: 0

        // Branch, upstream and sync
        Rectangle {
            Layout.fillWidth: true
            Layout.preferredHeight: 52
            color: Theme.bg
            visible: panel.repo
            RowLayout {
                anchors.fill: parent
                anchors.leftMargin: 12
                anchors.rightMargin: 8
                spacing: 8
                Icon { name: "git-branch"; size: 16; color: Theme.accentSecondary }
                ColumnLayout {
                    Layout.fillWidth: true
                    spacing: 1
                    Text {
                        Layout.fillWidth: true
                        text: panel.g.detached ? panel.tr("GitDetached", "Detached HEAD") : (panel.g.branch || "")
                        elide: Text.ElideRight
                        color: Theme.text
                        font.family: Theme.sans
                        font.pixelSize: 14
                        font.weight: Font.DemiBold
                    }
                    Text {
                        Layout.fillWidth: true
                        text: panel.g.upstream ? panel.g.upstream + (panel.g.fetchedAt ? "  ·  " + panel.tr("GitFetched", "fetched {t} ago", { t: Format.relTime(panel.g.fetchedAt) }) : "")
                                               : panel.tr("GitNoUpstream", "No upstream branch")
                        elide: Text.ElideRight
                        color: Theme.textDim
                        font.family: Theme.sans
                        font.pixelSize: 11
                    }
                }
                // Behind / ahead
                Repeater {
                    model: [
                        { icon: "arrow-down", n: panel.g.behind || 0, tip: panel.tr("GitBehind", "Incoming commits") },
                        { icon: "arrow-up", n: panel.g.ahead || 0, tip: panel.tr("GitAhead", "Commits to push") }
                    ]
                    delegate: Rectangle {
                        required property var modelData
                        visible: !!panel.g.upstream
                        implicitWidth: syncRow.implicitWidth + 14
                        implicitHeight: 24
                        radius: 12
                        color: modelData.n > 0 ? Theme.accentSoft : Theme.hover
                        Row {
                            id: syncRow
                            anchors.centerIn: parent
                            spacing: 3
                            Icon { anchors.verticalCenter: parent.verticalCenter; name: modelData.icon; size: 12; color: modelData.n > 0 ? Theme.accentSecondary : Theme.textDim }
                            Text { anchors.verticalCenter: parent.verticalCenter; text: modelData.n; color: modelData.n > 0 ? Theme.accentSecondary : Theme.textDim; font.family: Theme.sans; font.pixelSize: 12; font.weight: Font.Medium }
                        }
                        HoverHandler { id: syncHover }
                        ToolTip.visible: syncHover.hovered
                        ToolTip.delay: 300
                        ToolTip.text: modelData.tip
                    }
                }
                IconButton {
                    icon: panel.busy === "fetch" ? "loader" : "download"
                    iconSize: 15
                    tip: panel.tr("GitFetch", "Fetch")
                    enabled: panel.busy.length === 0
                    onClicked: ipc.gitAction("fetch")
                }
                IconButton {
                    icon: panel.busy === "pull" ? "loader" : "arrow-down"
                    iconSize: 15
                    tip: panel.tr("GitPull", "Pull (fast-forward only)")
                    enabled: panel.busy.length === 0 && (panel.g.behind || 0) > 0
                    opacity: enabled ? 1 : 0.4
                    onClicked: ipc.gitAction("pull")
                }
                IconButton {
                    icon: panel.busy === "push" ? "loader" : "arrow-up"
                    iconSize: 15
                    tip: panel.tr("GitPush", "Push")
                    enabled: panel.busy.length === 0 && (panel.g.ahead || 0) > 0
                    opacity: enabled ? 1 : 0.4
                    onClicked: ipc.gitAction("push")
                }
                IconButton {
                    icon: "refresh-cw"
                    iconSize: 14
                    tip: panel.tr("Refresh", "Refresh")
                    onClicked: ipc.gitRefresh()
                }
            }
            Rectangle { anchors.bottom: parent.bottom; width: parent.width; height: 1; color: Theme.borderSubtle }
        }

        // Views
        Item {
            Layout.fillWidth: true
            Layout.preferredHeight: 46
            visible: panel.repo
            Segmented {
                anchors.left: parent.left
                anchors.leftMargin: 10
                anchors.verticalCenter: parent.verticalCenter
                options: [
                    { value: "stashes", label: panel.tr("GitStashes", "Stashes") + "  " + (panel.g.stashes || []).length },
                    { value: "changes", label: panel.tr("GitChanges", "Changes") + "  " + (panel.g.changes || []).length },
                    { value: "sync", label: panel.tr("GitSync", "Incoming") + ((panel.g.behind || 0) + (panel.g.ahead || 0) > 0 ? "  " + ((panel.g.behind || 0) + (panel.g.ahead || 0)) : "") },
                    { value: "history", label: panel.tr("GitHistory", "History") }
                ]
                value: panel.view
                onPicked: (v) => { panel.view = v; panel.openKey = "" }
            }
        }

        // Message from the last action
        Rectangle {
            Layout.fillWidth: true
            Layout.leftMargin: 10
            Layout.rightMargin: 10
            Layout.preferredHeight: noteText.implicitHeight + 14
            visible: panel.note.length > 0
            radius: 8
            color: panel.noteError ? Theme.dangerSoft : Theme.accentSoft
            Text {
                id: noteText
                anchors.fill: parent
                anchors.margins: 7
                anchors.leftMargin: 10
                text: panel.note
                wrapMode: Text.Wrap
                maximumLineCount: 6
                elide: Text.ElideRight
                color: panel.noteError ? Theme.danger : Theme.text
                font.family: Theme.mono
                font.pixelSize: 11
            }
        }

        Flickable {
            id: flick
            Layout.fillWidth: true
            Layout.fillHeight: true
            contentWidth: width
            contentHeight: content.implicitHeight + 24
            clip: true
            boundsBehavior: Flickable.StopAtBounds
            ScrollBar.vertical: ScrollBar { policy: ScrollBar.AsNeeded; width: 8 }
            SmoothWheel { flickable: flick }

            ColumnLayout {
                id: content
                x: 10
                y: 6
                width: flick.width - 20
                spacing: 4

                // Not a repository
                ColumnLayout {
                    visible: !panel.repo
                    Layout.fillWidth: true
                    Layout.topMargin: 60
                    spacing: 10
                    Rectangle {
                        Layout.alignment: Qt.AlignHCenter
                        Layout.preferredWidth: 48
                        Layout.preferredHeight: 48
                        radius: 14
                        color: Theme.accentSoft
                        Icon { anchors.centerIn: parent; name: "git-branch"; size: 22; color: Theme.accentSecondary }
                    }
                    Text {
                        Layout.alignment: Qt.AlignHCenter
                        text: panel.tr("GitNotRepo", "Not a git repository")
                        color: Theme.text
                        font.family: Theme.sans
                        font.pixelSize: 15
                        font.weight: Font.DemiBold
                    }
                    Text {
                        Layout.alignment: Qt.AlignHCenter
                        Layout.maximumWidth: 320
                        horizontalAlignment: Text.AlignHCenter
                        wrapMode: Text.WordWrap
                        text: ipc.projectPath
                        color: Theme.textDim
                        font.family: Theme.mono
                        font.pixelSize: 11
                    }
                }

                // ---- Stashes ----
                RowLayout {
                    visible: panel.repo && panel.view === "stashes"
                    Layout.fillWidth: true
                    Layout.bottomMargin: 4
                    spacing: 6
                    Field {
                        Layout.fillWidth: true
                        icon: "search"
                        placeholder: panel.tr("GitFindStash", "Find a ticket or stash")
                        onEdited: panel.filter = text
                    }
                    ChipButton {
                        text: panel.tr("GitStashNow", "Stash")
                        iconName: "archive"
                        enabled: (panel.g.changes || []).length > 0
                        onClicked: { panel.stashing = !panel.stashing; if (panel.stashing) Qt.callLater(stashName.forceActiveFocus) }
                    }
                }
                // New stash, named after the ticket
                RowLayout {
                    visible: panel.repo && panel.view === "stashes" && panel.stashing
                    Layout.fillWidth: true
                    spacing: 6
                    Field {
                        id: stashName
                        Layout.fillWidth: true
                        icon: "edit-2"
                        placeholder: panel.tr("GitStashName", "Ticket number and a note, e.g. 4821 login timeout")
                        onAccepted: { ipc.gitAction("stash.push", -1, text); text = ""; panel.stashing = false }
                    }
                    AccentButton {
                        text: panel.tr("GitStashSave", "Save stash")
                        onClicked: { ipc.gitAction("stash.push", -1, stashName.text); stashName.text = ""; panel.stashing = false }
                    }
                }
                Repeater {
                    model: panel.repo && panel.view === "stashes" ? panel.stashes : []
                    delegate: Rectangle {
                        id: stashCard
                        required property var modelData
                        readonly property string key: "stash:" + modelData.index
                        readonly property bool open: panel.openKey === key
                        readonly property string ticket: panel.ticketOf(modelData.message)
                        Layout.fillWidth: true
                        implicitHeight: stashColumn.implicitHeight + 16
                        radius: 10
                        color: open ? Theme.card : (stashArea.containsMouse ? Theme.hover : "transparent")
                        border.width: open ? 1 : 0
                        border.color: Theme.borderSubtle
                        Behavior on color { ColorAnimation { duration: Theme.motionFast } }
                        MouseArea {
                            id: stashArea
                            width: parent.width
                            height: 46
                            hoverEnabled: true
                            cursorShape: Qt.PointingHandCursor
                            onClicked: panel.toggle(stashCard.key, function () { ipc.gitStashDiff(stashCard.modelData.index) })
                        }
                        ColumnLayout {
                            id: stashColumn
                            x: 10
                            y: 8
                            width: parent.width - 20
                            spacing: 8
                            RowLayout {
                                Layout.fillWidth: true
                                spacing: 8
                                Rectangle {
                                    visible: stashCard.ticket.length > 0
                                    implicitWidth: ticketText.implicitWidth + 14
                                    implicitHeight: 22
                                    radius: 6
                                    color: Theme.accent
                                    Text { id: ticketText; anchors.centerIn: parent; text: stashCard.ticket; color: "white"; font.family: Theme.mono; font.pixelSize: 11; font.weight: Font.DemiBold }
                                }
                                ColumnLayout {
                                    Layout.fillWidth: true
                                    spacing: 1
                                    Text {
                                        Layout.fillWidth: true
                                        text: stashCard.modelData.message || stashCard.modelData.name
                                        elide: Text.ElideRight
                                        color: Theme.text
                                        font.family: Theme.sans
                                        font.pixelSize: 13
                                        font.weight: Font.Medium
                                    }
                                    Text {
                                        Layout.fillWidth: true
                                        text: stashCard.modelData.name + (stashCard.modelData.branch ? "  ·  " + stashCard.modelData.branch : "") + "  ·  " + Format.relTime(stashCard.modelData.time)
                                        elide: Text.ElideRight
                                        color: Theme.textDim
                                        font.family: Theme.sans
                                        font.pixelSize: 11
                                    }
                                }
                                // Actions, on hover or when open
                                Row {
                                    visible: stashArea.containsMouse || stashCard.open || applyBtn.hovered || popBtn.hovered || dropBtn.hovered
                                    spacing: 2
                                    IconButton {
                                        id: applyBtn
                                        icon: "copy"
                                        iconSize: 13
                                        implicitWidth: 28
                                        implicitHeight: 28
                                        tip: panel.tr("GitApply", "Apply (keep the stash)")
                                        enabled: panel.busy.length === 0
                                        onClicked: ipc.gitAction("stash.apply", stashCard.modelData.index)
                                    }
                                    IconButton {
                                        id: popBtn
                                        icon: "download"
                                        iconSize: 13
                                        implicitWidth: 28
                                        implicitHeight: 28
                                        tip: panel.tr("GitPop", "Pop (apply and remove)")
                                        enabled: panel.busy.length === 0
                                        onClicked: ipc.gitAction("stash.pop", stashCard.modelData.index)
                                    }
                                    IconButton {
                                        id: dropBtn
                                        icon: panel.dropArmed === stashCard.modelData.index ? "check" : "trash-2"
                                        iconSize: 13
                                        implicitWidth: 28
                                        implicitHeight: 28
                                        danger: true
                                        tip: panel.dropArmed === stashCard.modelData.index ? panel.tr("GitDropConfirm", "Click again to delete this stash")
                                                                                          : panel.tr("GitDrop", "Drop")
                                        enabled: panel.busy.length === 0
                                        onClicked: {
                                            if (panel.dropArmed !== stashCard.modelData.index) { panel.dropArmed = stashCard.modelData.index; disarm.restart(); return }
                                            panel.dropArmed = -1
                                            ipc.gitAction("stash.drop", stashCard.modelData.index)
                                        }
                                    }
                                }
                            }
                            // Files and changes
                            Loader {
                                Layout.fillWidth: true
                                active: stashCard.open
                                visible: active
                                sourceComponent: panel.diffs[stashCard.key] === undefined ? loadingRow : stashFiles
                                Component {
                                    id: stashFiles
                                    DiffView { width: parent ? parent.width : 0; files: panel.diffs[stashCard.key] || [] }
                                }
                            }
                        }
                    }
                }
                Text {
                    visible: panel.repo && panel.view === "stashes" && panel.stashes.length === 0
                    Layout.topMargin: 20
                    Layout.alignment: Qt.AlignHCenter
                    text: (panel.g.stashes || []).length === 0 ? panel.tr("GitNoStashes", "No stashes in this repository.") : panel.tr("GitNoMatch", "No stash matches.")
                    color: Theme.textDim
                    font.family: Theme.sans
                    font.pixelSize: 13
                }

                // ---- Working changes ----
                Repeater {
                    model: panel.repo && panel.view === "changes" ? (panel.g.changes || []) : []
                    delegate: Rectangle {
                        id: changeCard
                        required property var modelData
                        readonly property string key: "file:" + modelData.path
                        readonly property bool open: panel.openKey === key
                        readonly property string letter: modelData.untracked ? "U" : (modelData.code.replace(/\./g, "").charAt(0) || "M")
                        Layout.fillWidth: true
                        implicitHeight: changeColumn.implicitHeight + 12
                        radius: 8
                        color: open ? Theme.card : (changeArea.containsMouse ? Theme.hover : "transparent")
                        MouseArea {
                            id: changeArea
                            width: parent.width
                            height: 34
                            hoverEnabled: true
                            cursorShape: Qt.PointingHandCursor
                            onClicked: panel.toggle(changeCard.key, function () { ipc.gitFileDiff(changeCard.modelData.path, changeCard.modelData.untracked) })
                        }
                        ColumnLayout {
                            id: changeColumn
                            x: 8
                            y: 6
                            width: parent.width - 16
                            spacing: 6
                            RowLayout {
                                Layout.fillWidth: true
                                spacing: 8
                                Rectangle {
                                    implicitWidth: 20
                                    implicitHeight: 20
                                    radius: 5
                                    color: changeCard.letter === "D" ? Theme.dangerSoft : (changeCard.letter === "A" || changeCard.letter === "U" ? Qt.rgba(0.13, 0.77, 0.37, 0.16) : Theme.accentSoft)
                                    Text {
                                        anchors.centerIn: parent
                                        text: changeCard.letter
                                        color: changeCard.letter === "D" ? Theme.danger : (changeCard.letter === "A" || changeCard.letter === "U" ? Theme.success : Theme.accentSecondary)
                                        font.family: Theme.mono
                                        font.pixelSize: 11
                                        font.weight: Font.DemiBold
                                    }
                                }
                                Text {
                                    Layout.fillWidth: true
                                    text: changeCard.modelData.path
                                    elide: Text.ElideMiddle
                                    color: Theme.text
                                    font.family: Theme.mono
                                    font.pixelSize: 12
                                }
                                Text {
                                    visible: changeCard.modelData.staged
                                    text: panel.tr("GitStaged", "staged")
                                    color: Theme.textDim
                                    font.family: Theme.sans
                                    font.pixelSize: 11
                                }
                            }
                            Loader {
                                Layout.fillWidth: true
                                active: changeCard.open
                                visible: active
                                sourceComponent: panel.diffs[changeCard.key] === undefined ? loadingRow : fileDiff
                                Component {
                                    id: fileDiff
                                    DiffView { width: parent ? parent.width : 0; files: panel.diffs[changeCard.key] || [] }
                                }
                            }
                        }
                    }
                }
                Text {
                    visible: panel.repo && panel.view === "changes" && (panel.g.changes || []).length === 0
                    Layout.topMargin: 20
                    Layout.alignment: Qt.AlignHCenter
                    text: panel.tr("GitClean", "Working tree clean.")
                    color: Theme.textDim
                    font.family: Theme.sans
                    font.pixelSize: 13
                }

                // ---- Incoming / outgoing and history ----
                Repeater {
                    model: !panel.repo ? []
                         : panel.view === "sync" ? [
                               { title: panel.tr("GitIncoming", "Incoming from {u}", { u: panel.g.upstream || "upstream" }), commits: panel.g.incoming || [], empty: panel.g.upstream ? panel.tr("GitUpToDate", "Up to date. Fetch to check for new commits.") : panel.tr("GitNoUpstream", "No upstream branch") },
                               { title: panel.tr("GitOutgoing", "Not pushed yet"), commits: panel.g.outgoing || [], empty: panel.tr("GitNothingToPush", "Nothing to push.") }
                           ]
                         : panel.view === "history" ? [{ title: "", commits: panel.g.history || [], empty: panel.tr("GitNoCommits", "No commits yet.") }]
                         : []
                    delegate: ColumnLayout {
                        id: section
                        required property var modelData
                        Layout.fillWidth: true
                        Layout.bottomMargin: 10
                        spacing: 2
                        Text {
                            visible: section.modelData.title.length > 0
                            Layout.topMargin: 4
                            Layout.bottomMargin: 4
                            text: section.modelData.title
                            color: Theme.textDim
                            font.family: Theme.sans
                            font.pixelSize: 11
                            font.weight: Font.Medium
                            font.capitalization: Font.AllUppercase
                            font.letterSpacing: 0.6
                        }
                        Text {
                            visible: section.modelData.commits.length === 0
                            Layout.leftMargin: 2
                            text: section.modelData.empty
                            color: Theme.textDim
                            font.family: Theme.sans
                            font.pixelSize: 12
                        }
                        Repeater {
                            model: section.modelData.commits
                            delegate: Rectangle {
                                id: commitCard
                                required property var modelData
                                readonly property bool open: panel.openKey === modelData.hash
                                Layout.fillWidth: true
                                implicitHeight: commitColumn.implicitHeight + 12
                                radius: 8
                                color: open ? Theme.card : (commitArea.containsMouse ? Theme.hover : "transparent")
                                MouseArea {
                                    id: commitArea
                                    width: parent.width
                                    height: 40
                                    hoverEnabled: true
                                    cursorShape: Qt.PointingHandCursor
                                    onClicked: panel.toggle(commitCard.modelData.hash, function () { ipc.gitCommitDiff(commitCard.modelData.hash) })
                                }
                                ColumnLayout {
                                    id: commitColumn
                                    x: 8
                                    y: 6
                                    width: parent.width - 16
                                    spacing: 6
                                    RowLayout {
                                        Layout.fillWidth: true
                                        spacing: 8
                                        Icon { name: "git-commit"; size: 14; color: Theme.textDim }
                                        ColumnLayout {
                                            Layout.fillWidth: true
                                            spacing: 1
                                            Text {
                                                Layout.fillWidth: true
                                                text: commitCard.modelData.subject
                                                elide: Text.ElideRight
                                                color: Theme.text
                                                font.family: Theme.sans
                                                font.pixelSize: 13
                                            }
                                            Text {
                                                Layout.fillWidth: true
                                                text: commitCard.modelData.short + "  ·  " + commitCard.modelData.author + "  ·  " + Format.relTime(commitCard.modelData.time)
                                                elide: Text.ElideRight
                                                color: Theme.textDim
                                                font.family: Theme.sans
                                                font.pixelSize: 11
                                            }
                                        }
                                        IconButton {
                                            visible: commitArea.containsMouse || commitCard.open || hovered
                                            icon: "copy"
                                            iconSize: 12
                                            implicitWidth: 26
                                            implicitHeight: 26
                                            tip: panel.tr("GitCopyHash", "Copy commit id")
                                            onClicked: ipc.copyText(commitCard.modelData.hash)
                                        }
                                    }
                                    Loader {
                                        Layout.fillWidth: true
                                        active: commitCard.open
                                        visible: active
                                        sourceComponent: panel.diffs[commitCard.modelData.hash] === undefined ? loadingRow : commitDiff
                                        Component {
                                            id: commitDiff
                                            DiffView { width: parent ? parent.width : 0; files: panel.diffs[commitCard.modelData.hash] || [] }
                                        }
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
        id: loadingRow
        Row {
            spacing: 8
            Icon {
                id: spin
                name: "loader"
                size: 14
                color: Theme.accentSecondary
                RotationAnimator on rotation { running: spin.visible; from: 0; to: 360; duration: 900; loops: Animation.Infinite }
            }
            Text { text: panel.tr("Loading", "Loading…"); color: Theme.textDim; font.family: Theme.sans; font.pixelSize: 12 }
        }
    }
}

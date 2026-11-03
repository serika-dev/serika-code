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
    // The git operation running now ({action, index, label, startedAt}); everything that
    // changes the repository waits until it is done.
    readonly property var busyInfo: g.busy || null
    readonly property string busy: busyInfo ? busyInfo.action : ""
    readonly property bool isBusy: busy.length > 0
    property int elapsed: 0
    Timer {
        interval: 1000
        repeat: true
        running: panel.isBusy
        onTriggered: panel.elapsed = Math.round((Date.now() - panel.busyInfo.startedAt) / 1000)
        onRunningChanged: panel.elapsed = 0
    }
    function commits(n) { return n === 1 ? panel.tr("GitOneCommit", "1 commit") : panel.tr("GitNCommits", "{n} commits", { n: n }) }
    function initials(name) {
        const parts = (name || "?").trim().split(/\s+/)
        return ((parts[0] || "?").charAt(0) + (parts.length > 1 ? parts[parts.length - 1].charAt(0) : "")).toUpperCase()
    }
    function accountById(id) {
        const list = ipc.gitAccounts.accounts || []
        for (let i = 0; i < list.length; i++) if (list[i].id === id) return list[i]
        return null
    }
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
    // Success fades on its own; an error stays until you close it.
    function say(text, error) {
        note = text
        noteError = error
        if (error) noteTimer.stop()
        else noteTimer.restart()
    }
    readonly property var stashes: (g.stashes || []).filter(function (s) {
        const q = panel.filter.trim().toLowerCase()
        return q.length === 0 || s.message.toLowerCase().indexOf(q) >= 0 || s.branch.toLowerCase().indexOf(q) >= 0
               || s.name.indexOf(q) >= 0
    })

    // A different project: start over with its repository.
    readonly property string project: ipc.projectPath
    onProjectChanged: { diffs = ({}); openKey = ""; filter = ""; ipc.gitRefresh(); ipc.gitAccountsRefresh() }
    Component.onCompleted: { ipc.gitRefresh(); ipc.gitAccountsRefresh() }
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

        // Branch switcher, account switcher, refresh
        Rectangle {
            Layout.fillWidth: true
            Layout.preferredHeight: 50
            color: Theme.bg
            visible: panel.repo
            RowLayout {
                anchors.fill: parent
                anchors.leftMargin: 8
                anchors.rightMargin: 8
                spacing: 6
                // Current branch; click to switch or create one
                Rectangle {
                    id: branchButton
                    Layout.fillWidth: true
                    Layout.maximumWidth: implicitWidth
                    implicitWidth: branchRow.implicitWidth + 20
                    implicitHeight: 34
                    radius: 9
                    color: branchArea.containsMouse || branchMenu.opened ? Theme.active : Theme.hover
                    border.width: 1
                    border.color: branchMenu.opened ? Theme.accent : Theme.borderSubtle
                    Behavior on color { ColorAnimation { duration: Theme.motionFast } }
                    RowLayout {
                        id: branchRow
                        anchors.fill: parent
                        anchors.leftMargin: 10
                        anchors.rightMargin: 8
                        spacing: 7
                        Icon { name: "git-branch"; size: 15; color: Theme.accentSecondary }
                        Text {
                            Layout.fillWidth: true
                            Layout.maximumWidth: 220
                            text: panel.g.detached ? panel.tr("GitDetached", "Detached HEAD") : (panel.g.branch || "")
                            elide: Text.ElideMiddle
                            color: Theme.text
                            font.family: Theme.sans
                            font.pixelSize: 13
                            font.weight: Font.DemiBold
                        }
                        Icon {
                            name: "chevron-down"
                            size: 13
                            color: Theme.textDim
                            rotation: branchMenu.opened ? 180 : 0
                            Behavior on rotation { NumberAnimation { duration: Theme.motionFast } }
                        }
                    }
                    MouseArea {
                        id: branchArea
                        anchors.fill: parent
                        hoverEnabled: true
                        cursorShape: Qt.PointingHandCursor
                        onClicked: {
                            if (branchMenu.opened) { branchMenu.close(); return }
                            ipc.gitBranchesRefresh()
                            branchSearch.text = ""
                            branchMenu.open()
                            Qt.callLater(branchSearch.focusInput)
                        }
                    }
                    ToolTip.visible: branchArea.containsMouse && !branchMenu.opened
                    ToolTip.delay: 500
                    ToolTip.text: panel.tr("GitSwitchBranch", "Switch or create a branch")

                    Popover {
                        id: branchMenu
                        focus: true
                        y: branchButton.height + 6
                        width: Math.min(340, panel.width - 24)
                        readonly property string query: branchSearch.text.trim()
                        readonly property var matches: ipc.gitBranches.filter(function (b) {
                            return branchMenu.query.length === 0 || b.name.toLowerCase().indexOf(branchMenu.query.toLowerCase()) >= 0
                        })
                        readonly property bool exists: ipc.gitBranches.some(function (b) { return b.name === branchMenu.query })
                        function pick(name) {
                            branchMenu.close()
                            ipc.gitAction("switch", -1, name, panel.tr("GitSwitching", "Switching to {b}", { b: name }))
                        }
                        contentItem: ColumnLayout {
                            spacing: 4
                            Field {
                                id: branchSearch
                                Layout.fillWidth: true
                                icon: "search"
                                placeholder: panel.tr("GitFindBranch", "Find or create a branch")
                                onAccepted: {
                                    if (branchMenu.matches.length > 0 && branchMenu.matches[0].name.toLowerCase() === branchMenu.query.toLowerCase()) branchMenu.pick(branchMenu.matches[0].name)
                                    else if (branchMenu.query.length > 0 && !branchMenu.exists) createRow.triggered()
                                }
                            }
                            MenuRow {
                                id: createRow
                                visible: branchMenu.query.length > 0 && !branchMenu.exists
                                icon: "plus"
                                label: panel.tr("GitCreateBranch", "Create branch \"{b}\" from {c}", { b: branchMenu.query, c: panel.g.branch || "HEAD" })
                                onTriggered: {
                                    const name = branchMenu.query
                                    branchMenu.close()
                                    ipc.gitAction("branch.create", -1, name, panel.tr("GitCreating", "Creating {b}", { b: name }))
                                }
                            }
                            Flickable {
                                id: branchFlick
                                Layout.fillWidth: true
                                Layout.preferredHeight: Math.min(branchColumn.implicitHeight, 320)
                                contentHeight: branchColumn.implicitHeight
                                clip: true
                                boundsBehavior: Flickable.StopAtBounds
                                SmoothWheel { flickable: branchFlick }
                                ColumnLayout {
                                    id: branchColumn
                                    width: branchFlick.width
                                    spacing: 0
                                    Repeater {
                                        model: branchMenu.matches
                                        delegate: ColumnLayout {
                                            id: branchItem
                                            required property var modelData
                                            required property int index
                                            readonly property bool firstRemote: modelData.remote && (index === 0 || !branchMenu.matches[index - 1].remote)
                                            Layout.fillWidth: true
                                            spacing: 0
                                            Text {
                                                visible: branchItem.firstRemote
                                                Layout.leftMargin: 10
                                                Layout.topMargin: 6
                                                Layout.bottomMargin: 2
                                                text: panel.tr("GitRemoteBranches", "Remote branches")
                                                color: Theme.textDim
                                                font.family: Theme.sans
                                                font.pixelSize: 11
                                                font.weight: Font.Medium
                                                font.capitalization: Font.AllUppercase
                                                font.letterSpacing: 0.6
                                            }
                                            Rectangle {
                                                Layout.fillWidth: true
                                                implicitHeight: 44
                                                radius: 8
                                                color: rowArea.containsMouse ? Theme.active : "transparent"
                                                RowLayout {
                                                    anchors.fill: parent
                                                    anchors.leftMargin: 10
                                                    anchors.rightMargin: 10
                                                    spacing: 10
                                                    Icon {
                                                        name: branchItem.modelData.current ? "check" : (branchItem.modelData.remote ? "cloud" : "git-branch")
                                                        size: 14
                                                        color: branchItem.modelData.current ? Theme.accentSecondary : Theme.textDim
                                                    }
                                                    ColumnLayout {
                                                        Layout.fillWidth: true
                                                        spacing: 1
                                                        Text {
                                                            Layout.fillWidth: true
                                                            text: branchItem.modelData.name
                                                            elide: Text.ElideMiddle
                                                            color: Theme.text
                                                            font.family: Theme.sans
                                                            font.pixelSize: 13
                                                            font.weight: branchItem.modelData.current ? Font.DemiBold : Font.Normal
                                                        }
                                                        Text {
                                                            Layout.fillWidth: true
                                                            text: Format.relTime(branchItem.modelData.time) + "  ·  " + branchItem.modelData.subject
                                                            elide: Text.ElideRight
                                                            color: Theme.textDim
                                                            font.family: Theme.sans
                                                            font.pixelSize: 11
                                                        }
                                                    }
                                                    Text {
                                                        visible: branchItem.modelData.behind > 0 || branchItem.modelData.ahead > 0
                                                        text: (branchItem.modelData.behind > 0 ? "↓" + branchItem.modelData.behind + " " : "") + (branchItem.modelData.ahead > 0 ? "↑" + branchItem.modelData.ahead : "")
                                                        color: Theme.accentSecondary
                                                        font.family: Theme.sans
                                                        font.pixelSize: 11
                                                    }
                                                }
                                                MouseArea {
                                                    id: rowArea
                                                    anchors.fill: parent
                                                    hoverEnabled: true
                                                    cursorShape: branchItem.modelData.current ? Qt.ArrowCursor : Qt.PointingHandCursor
                                                    onClicked: if (!branchItem.modelData.current) branchMenu.pick(branchItem.modelData.name)
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                            Text {
                                visible: branchMenu.matches.length === 0 && branchMenu.query.length === 0
                                Layout.margins: 10
                                text: panel.tr("Loading", "Loading…")
                                color: Theme.textDim
                                font.family: Theme.sans
                                font.pixelSize: 12
                            }
                        }
                    }
                }
                Item { Layout.fillWidth: true }
                // Who commits here; click to switch git accounts
                Rectangle {
                    id: accountButton
                    readonly property var who: panel.g.identity || {}
                    readonly property var account: panel.accountById(who.account || "")
                    implicitWidth: accountRow.implicitWidth + 16
                    implicitHeight: 34
                    radius: 9
                    color: accountArea.containsMouse || accountMenu.opened ? Theme.active : "transparent"
                    border.width: accountMenu.opened ? 1 : 0
                    border.color: Theme.accent
                    Behavior on color { ColorAnimation { duration: Theme.motionFast } }
                    RowLayout {
                        id: accountRow
                        anchors.centerIn: parent
                        spacing: 6
                        Rectangle {
                            implicitWidth: 22
                            implicitHeight: 22
                            radius: 11
                            color: accountButton.account ? Theme.accent : Theme.active
                            Text {
                                anchors.centerIn: parent
                                text: panel.initials(accountButton.who.name || "?")
                                color: accountButton.account ? "white" : Theme.textSecondary
                                font.family: Theme.sans
                                font.pixelSize: 10
                                font.weight: Font.DemiBold
                            }
                        }
                        Text {
                            Layout.maximumWidth: 120
                            visible: panel.width > 420
                            text: accountButton.account ? accountButton.account.label : (accountButton.who.email || panel.tr("GitNoIdentity", "No identity"))
                            elide: Text.ElideRight
                            color: Theme.textSecondary
                            font.family: Theme.sans
                            font.pixelSize: 12
                        }
                        Icon { name: "chevron-down"; size: 12; color: Theme.textDim }
                    }
                    MouseArea {
                        id: accountArea
                        anchors.fill: parent
                        hoverEnabled: true
                        cursorShape: Qt.PointingHandCursor
                        onClicked: {
                            if (accountMenu.opened) { accountMenu.close(); return }
                            ipc.gitAccountsRefresh()
                            accountMenu.open()
                        }
                    }
                    ToolTip.visible: accountArea.containsMouse && !accountMenu.opened
                    ToolTip.delay: 500
                    ToolTip.text: (accountButton.who.name || "") + " <" + (accountButton.who.email || "") + ">"
                                  + (accountButton.who.username ? "\n" + panel.tr("GitLogin", "Login: {u}", { u: accountButton.who.username }) : "")

                    Popover {
                        id: accountMenu
                        x: accountButton.width - width
                        y: accountButton.height + 6
                        width: 300
                        property bool makeDefault: true
                        function use(id) {
                            accountMenu.close()
                            ipc.gitAccountUse(id, accountMenu.makeDefault)
                        }
                        contentItem: ColumnLayout {
                            spacing: 2
                            Text {
                                Layout.leftMargin: 10
                                Layout.topMargin: 6
                                Layout.bottomMargin: 2
                                text: panel.tr("GitAccountFor", "Git account for this project")
                                color: Theme.textDim
                                font.family: Theme.sans
                                font.pixelSize: 11
                                font.weight: Font.Medium
                                font.capitalization: Font.AllUppercase
                                font.letterSpacing: 0.6
                            }
                            Repeater {
                                model: (ipc.gitAccounts.accounts || [])
                                delegate: Rectangle {
                                    id: accountItem
                                    required property var modelData
                                    readonly property bool current: (panel.g.identity || {}).account === modelData.id
                                    readonly property bool isDefault: ipc.gitAccounts.projectDefault === modelData.id
                                    Layout.fillWidth: true
                                    implicitHeight: 46
                                    radius: 8
                                    color: itemArea.containsMouse ? Theme.active : "transparent"
                                    RowLayout {
                                        anchors.fill: parent
                                        anchors.leftMargin: 10
                                        anchors.rightMargin: 10
                                        spacing: 10
                                        Rectangle {
                                            implicitWidth: 26
                                            implicitHeight: 26
                                            radius: 13
                                            color: accountItem.current ? Theme.accent : Theme.active
                                            Text { anchors.centerIn: parent; text: panel.initials(accountItem.modelData.name); color: accountItem.current ? "white" : Theme.textSecondary; font.family: Theme.sans; font.pixelSize: 11; font.weight: Font.DemiBold }
                                        }
                                        ColumnLayout {
                                            Layout.fillWidth: true
                                            spacing: 1
                                            Text { Layout.fillWidth: true; text: accountItem.modelData.label; elide: Text.ElideRight; color: Theme.text; font.family: Theme.sans; font.pixelSize: 13; font.weight: accountItem.current ? Font.DemiBold : Font.Normal }
                                            Text { Layout.fillWidth: true; text: accountItem.modelData.name + " <" + accountItem.modelData.email + ">"; elide: Text.ElideRight; color: Theme.textDim; font.family: Theme.sans; font.pixelSize: 11 }
                                        }
                                        Tag { visible: accountItem.isDefault; text: panel.tr("GitDefault", "default") }
                                        Icon { visible: accountItem.current; name: "check"; size: 14; color: Theme.accentSecondary }
                                    }
                                    MouseArea {
                                        id: itemArea
                                        anchors.fill: parent
                                        hoverEnabled: true
                                        cursorShape: Qt.PointingHandCursor
                                        onClicked: accountMenu.use(accountItem.modelData.id)
                                    }
                                }
                            }
                            MenuRow {
                                icon: (panel.g.identity || {}).account ? "user" : "check"
                                label: panel.tr("GitGlobalIdentity", "Your global git settings")
                                onTriggered: accountMenu.use("")
                            }
                            Text {
                                visible: (ipc.gitAccounts.accounts || []).length === 0
                                Layout.fillWidth: true
                                Layout.margins: 10
                                wrapMode: Text.WordWrap
                                text: panel.tr("GitNoAccounts", "Add your work and personal git accounts to switch between them per project.")
                                color: Theme.textDim
                                font.family: Theme.sans
                                font.pixelSize: 12
                            }
                            MenuDivider {}
                            RowLayout {
                                Layout.fillWidth: true
                                Layout.leftMargin: 10
                                Layout.rightMargin: 6
                                Layout.preferredHeight: 36
                                Text {
                                    Layout.fillWidth: true
                                    text: panel.tr("GitMakeDefault", "Use it every time I open this project")
                                    wrapMode: Text.WordWrap
                                    color: Theme.textSecondary
                                    font.family: Theme.sans
                                    font.pixelSize: 12
                                }
                                Toggle { checked: accountMenu.makeDefault; onToggled: (v) => accountMenu.makeDefault = v }
                            }
                            MenuRow {
                                icon: "settings"
                                label: panel.tr("GitManageAccounts", "Manage git accounts…")
                                onTriggered: { accountMenu.close(); ipc.panelRequested("settings:gitaccounts") }
                            }
                        }
                    }
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

        // Sync with the upstream: what's waiting either way, and the buttons that move it
        Rectangle {
            id: syncCard
            readonly property int behind: panel.g.behind || 0
            readonly property int ahead: panel.g.ahead || 0
            readonly property bool network: ["fetch", "pull", "push", "publish"].indexOf(panel.busy) >= 0
            Layout.fillWidth: true
            Layout.leftMargin: 10
            Layout.rightMargin: 10
            Layout.topMargin: 10
            Layout.preferredHeight: 58
            visible: panel.repo
            radius: 12
            clip: true
            color: behind > 0 ? Qt.rgba(Theme.accent.r, Theme.accent.g, Theme.accent.b, 0.10) : Theme.card
            border.width: 1
            border.color: behind > 0 ? Qt.rgba(Theme.accent.r, Theme.accent.g, Theme.accent.b, 0.35) : Theme.borderSubtle
            Behavior on color { ColorAnimation { duration: Theme.motionBase } }
            RowLayout {
                anchors.fill: parent
                anchors.leftMargin: 12
                anchors.rightMargin: 10
                spacing: 10
                Rectangle {
                    implicitWidth: 32
                    implicitHeight: 32
                    radius: 10
                    color: syncCard.behind > 0 ? Theme.accent : Theme.active
                    Icon {
                        id: syncIcon
                        anchors.centerIn: parent
                        name: syncCard.network ? "loader" : (syncCard.behind > 0 ? "download-cloud" : (syncCard.ahead > 0 ? "upload-cloud" : (panel.g.upstream ? "check" : "cloud")))
                        size: 16
                        color: syncCard.behind > 0 ? "white" : Theme.textSecondary
                        RotationAnimator on rotation { id: syncSpin; running: syncCard.network; from: 0; to: 360; duration: 900; loops: Animation.Infinite; onRunningChanged: if (!running) syncIcon.rotation = 0 }
                    }
                }
                ColumnLayout {
                    Layout.fillWidth: true
                    spacing: 2
                    Text {
                        Layout.fillWidth: true
                        elide: Text.ElideRight
                        color: Theme.text
                        font.family: Theme.sans
                        font.pixelSize: 13
                        font.weight: Font.DemiBold
                        text: syncCard.network ? panel.busyInfo.label + "…"
                            : !panel.g.upstream ? panel.tr("GitNotPublished", "This branch is not on a remote yet")
                            : syncCard.behind > 0 && syncCard.ahead > 0 ? panel.tr("GitDiverged", "{b} to pull, {a} to push", { b: panel.commits(syncCard.behind), a: panel.commits(syncCard.ahead) })
                            : syncCard.behind > 0 ? panel.tr("GitNewCommits", "{n} on {u}", { n: panel.commits(syncCard.behind), u: panel.g.upstream })
                            : syncCard.ahead > 0 ? panel.tr("GitToPush", "{n} not pushed yet", { n: panel.commits(syncCard.ahead) })
                            : panel.tr("GitUpToDateWith", "Up to date with {u}", { u: panel.g.upstream })
                    }
                    Text {
                        Layout.fillWidth: true
                        elide: Text.ElideRight
                        color: Theme.textDim
                        font.family: Theme.sans
                        font.pixelSize: 11
                        text: syncCard.network ? panel.tr("GitElapsed", "{s}s, this talks to the remote", { s: panel.elapsed })
                            : panel.g.fetchedAt ? panel.tr("GitCheckedAgo", "Checked {t} ago", { t: Format.relTime(panel.g.fetchedAt) })
                            : panel.tr("GitNeverFetched", "Not checked yet. Fetch to see new commits.")
                    }
                }
                ChipButton {
                    text: panel.tr("GitFetch", "Fetch")
                    iconName: "refresh-cw"
                    visible: !!panel.g.upstream
                    enabled: !panel.isBusy
                    opacity: enabled ? 1 : 0.5
                    onClicked: ipc.gitAction("fetch", -1, "", panel.tr("GitFetching", "Checking {r} for new commits", { r: (panel.g.upstream || "origin").split("/")[0] }))
                }
                AccentButton {
                    implicitHeight: 32
                    visible: syncCard.behind > 0
                    text: panel.tr("GitPullN", "Pull {n}", { n: syncCard.behind })
                    enabled: !panel.isBusy
                    opacity: enabled ? 1 : 0.5
                    onClicked: ipc.gitAction("pull", -1, "", panel.tr("GitPulling", "Pulling {n}", { n: panel.commits(syncCard.behind) }))
                }
                ChipButton {
                    visible: !panel.g.upstream && !panel.g.detached
                    primary: true
                    text: panel.tr("GitPublish", "Publish branch")
                    iconName: "upload-cloud"
                    enabled: !panel.isBusy
                    opacity: enabled ? 1 : 0.5
                    onClicked: ipc.gitAction("publish", -1, "", panel.tr("GitPublishing", "Publishing {b}", { b: panel.g.branch }))
                }
                ChipButton {
                    visible: syncCard.ahead > 0
                    primary: syncCard.behind === 0
                    text: panel.tr("GitPushN", "Push {n}", { n: syncCard.ahead })
                    iconName: "upload-cloud"
                    enabled: !panel.isBusy
                    opacity: enabled ? 1 : 0.5
                    onClicked: ipc.gitAction("push", -1, "", panel.tr("GitPushing", "Pushing {n}", { n: panel.commits(syncCard.ahead) }))
                }
            }
            // Indeterminate progress while the remote is busy
            Item {
                anchors.left: parent.left
                anchors.right: parent.right
                anchors.bottom: parent.bottom
                height: 2
                visible: syncCard.network
                clip: true
                Rectangle {
                    id: syncSweep
                    width: parent.width * 0.3
                    height: 2
                    radius: 1
                    color: Theme.accent
                    NumberAnimation on x { running: syncCard.network; from: -syncSweep.width; to: syncSweep.parent.width; duration: 1100; loops: Animation.Infinite; easing.type: Easing.InOutQuad }
                }
            }
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

        // Work in progress (not network, which the sync card shows): stash, switch, account
        Rectangle {
            Layout.fillWidth: true
            Layout.leftMargin: 10
            Layout.rightMargin: 10
            Layout.bottomMargin: 6
            Layout.preferredHeight: panel.isBusy && !syncCard.network ? 40 : 0
            opacity: panel.isBusy && !syncCard.network ? 1 : 0
            visible: Layout.preferredHeight > 1
            Behavior on Layout.preferredHeight { NumberAnimation { duration: Theme.motionBase; easing.type: Theme.easeEmphasis } }
            Behavior on opacity { NumberAnimation { duration: Theme.motionBase } }
            radius: 10
            clip: true
            color: Qt.rgba(Theme.accent.r, Theme.accent.g, Theme.accent.b, 0.12)
            border.width: 1
            border.color: Qt.rgba(Theme.accent.r, Theme.accent.g, Theme.accent.b, 0.35)
            RowLayout {
                anchors.fill: parent
                anchors.leftMargin: 12
                anchors.rightMargin: 12
                spacing: 10
                Icon {
                    id: workSpin
                    name: "loader"
                    size: 15
                    color: Theme.accentSecondary
                    RotationAnimator on rotation { running: panel.isBusy; from: 0; to: 360; duration: 900; loops: Animation.Infinite }
                }
                Text {
                    Layout.fillWidth: true
                    text: panel.isBusy ? panel.busyInfo.label + "…" : ""
                    elide: Text.ElideRight
                    color: Theme.text
                    font.family: Theme.sans
                    font.pixelSize: 13
                    font.weight: Font.Medium
                }
                Text {
                    text: panel.elapsed > 0 ? panel.elapsed + "s" : ""
                    color: Theme.textDim
                    font.family: Theme.sans
                    font.pixelSize: 12
                }
            }
            Item {
                anchors.left: parent.left
                anchors.right: parent.right
                anchors.bottom: parent.bottom
                height: 2
                clip: true
                Rectangle {
                    id: workSweep
                    width: parent.width * 0.3
                    height: 2
                    color: Theme.accent
                    NumberAnimation on x { running: panel.isBusy; from: -workSweep.width; to: workSweep.parent.width; duration: 1100; loops: Animation.Infinite; easing.type: Easing.InOutQuad }
                }
            }
        }

        // Result of the last action: green when it worked, red with git's own words when not
        Rectangle {
            Layout.fillWidth: true
            Layout.leftMargin: 10
            Layout.rightMargin: 10
            Layout.bottomMargin: 6
            Layout.preferredHeight: panel.note.length > 0 ? noteRow.implicitHeight + 18 : 0
            opacity: panel.note.length > 0 ? 1 : 0
            visible: Layout.preferredHeight > 1
            Behavior on Layout.preferredHeight { NumberAnimation { duration: Theme.motionBase; easing.type: Theme.easeEmphasis } }
            Behavior on opacity { NumberAnimation { duration: Theme.motionBase } }
            radius: 10
            clip: true
            color: panel.noteError ? Theme.dangerSoft : Qt.rgba(0.13, 0.77, 0.37, 0.12)
            border.width: 1
            border.color: panel.noteError ? Qt.rgba(0.96, 0.25, 0.37, 0.35) : Qt.rgba(0.13, 0.77, 0.37, 0.35)
            RowLayout {
                id: noteRow
                anchors.left: parent.left
                anchors.right: parent.right
                anchors.top: parent.top
                anchors.margins: 9
                anchors.leftMargin: 12
                spacing: 10
                Icon {
                    Layout.alignment: Qt.AlignTop
                    name: panel.noteError ? "alert-circle" : "check-circle"
                    size: 15
                    color: panel.noteError ? Theme.danger : Theme.success
                }
                Text {
                    id: noteText
                    Layout.fillWidth: true
                    text: panel.note
                    wrapMode: Text.Wrap
                    maximumLineCount: 6
                    elide: Text.ElideRight
                    color: Theme.text
                    font.family: panel.noteError ? Theme.mono : Theme.sans
                    font.pixelSize: panel.noteError ? 11 : 13
                }
                IconButton {
                    Layout.alignment: Qt.AlignTop
                    implicitWidth: 22
                    implicitHeight: 22
                    icon: "x"
                    iconSize: 11
                    onClicked: panel.note = ""
                }
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
                        enabled: (panel.g.changes || []).length > 0 && !panel.isBusy
                        onClicked: { panel.stashing = !panel.stashing; if (panel.stashing) Qt.callLater(stashName.focusInput) }
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
                        onAccepted: { ipc.gitAction("stash.push", -1, text, panel.tr("GitStashing", "Stashing your changes")); text = ""; panel.stashing = false }
                    }
                    AccentButton {
                        text: panel.tr("GitStashSave", "Save stash")
                        enabled: !panel.isBusy
                        onClicked: { ipc.gitAction("stash.push", -1, stashName.text, panel.tr("GitStashing", "Stashing your changes")); stashName.text = ""; panel.stashing = false }
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
                        readonly property string title: modelData.message || modelData.name
                        // This stash is the one being popped, applied or deleted right now.
                        readonly property bool working: panel.isBusy && panel.busyInfo.index === modelData.index && panel.busy.indexOf("stash.") === 0
                        opacity: panel.isBusy && !working ? 0.55 : 1
                        Behavior on opacity { NumberAnimation { duration: Theme.motionBase } }
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
                                // Working on this one: a spinner and what is happening
                                Row {
                                    visible: stashCard.working
                                    spacing: 6
                                    Icon {
                                        id: rowSpin
                                        anchors.verticalCenter: parent.verticalCenter
                                        name: "loader"
                                        size: 14
                                        color: Theme.accentSecondary
                                        RotationAnimator on rotation { running: stashCard.working; from: 0; to: 360; duration: 900; loops: Animation.Infinite }
                                    }
                                    Text {
                                        anchors.verticalCenter: parent.verticalCenter
                                        text: panel.busy === "stash.pop" ? panel.tr("GitPoppingShort", "Popping…")
                                            : panel.busy === "stash.apply" ? panel.tr("GitApplyingShort", "Applying…") : panel.tr("GitDroppingShort", "Deleting…")
                                        color: Theme.accentSecondary
                                        font.family: Theme.sans
                                        font.pixelSize: 12
                                        font.weight: Font.Medium
                                    }
                                }
                                // Actions, on hover or when open
                                Row {
                                    visible: !stashCard.working && !panel.isBusy && (stashArea.containsMouse || stashCard.open || applyBtn.hovered || popBtn.hovered || dropBtn.hovered)
                                    spacing: 2
                                    IconButton {
                                        id: applyBtn
                                        icon: "copy"
                                        iconSize: 13
                                        implicitWidth: 28
                                        implicitHeight: 28
                                        tip: panel.tr("GitApply", "Apply (keep the stash)")
                                        enabled: !panel.isBusy
                                        onClicked: ipc.gitAction("stash.apply", stashCard.modelData.index, "", panel.tr("GitApplying", "Applying {s}", { s: stashCard.title }))
                                    }
                                    IconButton {
                                        id: popBtn
                                        icon: "download"
                                        iconSize: 13
                                        implicitWidth: 28
                                        implicitHeight: 28
                                        tip: panel.tr("GitPop", "Pop (apply and remove)")
                                        enabled: !panel.isBusy
                                        onClicked: ipc.gitAction("stash.pop", stashCard.modelData.index, "", panel.tr("GitPopping", "Popping {s}", { s: stashCard.title }))
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
                                        enabled: !panel.isBusy
                                        onClicked: {
                                            if (panel.dropArmed !== stashCard.modelData.index) { panel.dropArmed = stashCard.modelData.index; disarm.restart(); return }
                                            panel.dropArmed = -1
                                            ipc.gitAction("stash.drop", stashCard.modelData.index, "", panel.tr("GitDropping", "Deleting {s}", { s: stashCard.title }))
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

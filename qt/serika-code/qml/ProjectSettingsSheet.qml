import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

// Settings for one project, from the ⋯ menu of a project in the sidebar. Sections on the
// left (General, Git, Agent, Danger zone), cards on the right. Opens at once with what was
// last known about the project and fills in the fresh details as they arrive.
Popup {
    id: sheet
    readonly property var dict: ipc.strings
    function t(key, fallback, values) { return Format.tr(dict, key, fallback, values) }

    property var project: ({})          // a sidebar project row: { path, name, count, current, ... }
    property var details: ({})
    property bool loading: false
    property string section: "general"
    property bool deleteArmed: false
    // Details by project path, so opening a project again is instant.
    property var cache: ({})

    readonly property var auth: details.auth || ({})
    readonly property var who: details.identity || ({})
    readonly property var accounts: ipc.gitAccounts.accounts || []
    readonly property bool repo: details.repo === true

    // Show a change at once; the answer from the backend confirms (or corrects) it.
    function patch(key, changes) {
        const next = Object.assign({}, details)
        next[key] = Object.assign({}, details[key] || {}, changes)
        details = next
    }
    function show(row) {
        project = row
        section = "general"
        deleteArmed = false
        details = cache[row.path] || ({})
        loading = true
        open()
        ipc.projectDetails(row.path)
    }

    Connections {
        target: ipc
        function onProjectDetailsReady(path, data) {
            const next = Object.assign({}, sheet.cache)
            next[path] = data
            sheet.cache = next
            if (path !== sheet.project.path) return
            sheet.details = data
            sheet.loading = false
        }
    }
    Timer { id: disarm; interval: 4000; onTriggered: sheet.deleteArmed = false }

    parent: Overlay.overlay
    anchors.centerIn: Overlay.overlay
    modal: true
    dim: true
    padding: 0
    width: Math.min(780, (parent ? parent.width : 900) - 48)
    height: Math.min(560, (parent ? parent.height : 700) - 64)
    closePolicy: Popup.CloseOnEscape | Popup.CloseOnPressOutside
    Overlay.modal: Rectangle { color: "#99000000" }
    background: Rectangle {
        radius: 18
        color: Theme.card
        border.width: 1
        border.color: Theme.border
    }
    // A soft spring in, a quick fade out.
    enter: Transition {
        NumberAnimation { property: "opacity"; from: 0; to: 1; duration: Theme.motionBase; easing.type: Theme.easeOut }
        NumberAnimation { property: "scale"; from: 0.94; to: 1; duration: Theme.motionSlow; easing.type: Easing.OutBack; easing.overshoot: 0.6 }
    }
    exit: Transition {
        NumberAnimation { property: "opacity"; to: 0; duration: Theme.motionFast; easing.type: Easing.InCubic }
        NumberAnimation { property: "scale"; to: 0.97; duration: Theme.motionFast; easing.type: Easing.InCubic }
    }

    component NavItem: Rectangle {
        id: nav
        property string key: ""
        property string label: ""
        property string icon: ""
        property bool danger: false
        readonly property bool current: sheet.section === key
        Layout.fillWidth: true
        implicitHeight: 36
        radius: 9
        color: current ? (danger ? Theme.dangerSoft : Theme.active) : (navArea.containsMouse ? Theme.hover : "transparent")
        Behavior on color { ColorAnimation { duration: Theme.motionFast } }
        RowLayout {
            anchors.fill: parent
            anchors.leftMargin: 10
            anchors.rightMargin: 10
            spacing: 10
            Icon { name: nav.icon; size: 15; color: nav.danger ? Theme.danger : (nav.current ? Theme.accentSecondary : Theme.textSecondary) }
            Text {
                Layout.fillWidth: true
                text: nav.label
                color: nav.danger ? Theme.danger : (nav.current ? Theme.text : Theme.textSecondary)
                font.family: Theme.sans
                font.pixelSize: 13
                font.weight: nav.current ? Font.Medium : Font.Normal
            }
        }
        MouseArea { id: navArea; anchors.fill: parent; hoverEnabled: true; cursorShape: Qt.PointingHandCursor; onClicked: sheet.section = nav.key }
    }
    // A card: title and help on top, content below, an optional footer line.
    component Card: Rectangle {
        id: card
        default property alias body: inner.data
        property string title: ""
        property string help: ""
        property string footer: ""
        Layout.fillWidth: true
        implicitHeight: cardColumn.implicitHeight + 32
        radius: 14
        color: Theme.bg
        border.width: 1
        border.color: Theme.borderSubtle
        ColumnLayout {
            id: cardColumn
            x: 16
            y: 16
            width: card.width - 32
            spacing: 10
            ColumnLayout {
                spacing: 3
                Layout.fillWidth: true
                visible: card.title.length > 0
                Text { text: card.title; color: Theme.text; font.family: Theme.sans; font.pixelSize: 14; font.weight: Font.DemiBold }
                Text {
                    Layout.fillWidth: true
                    visible: text.length > 0
                    text: card.help
                    wrapMode: Text.WordWrap
                    color: Theme.textDim
                    font.family: Theme.sans
                    font.pixelSize: 12
                    lineHeight: 1.15
                }
            }
            ColumnLayout { id: inner; Layout.fillWidth: true; spacing: 8 }
            Text {
                visible: text.length > 0
                Layout.fillWidth: true
                text: card.footer
                wrapMode: Text.WrapAnywhere
                color: Theme.textDim
                font.family: Theme.sans
                font.pixelSize: 11
            }
        }
    }
    component Stat: Rectangle {
        id: stat
        property string icon: ""
        property string value: ""
        property string label: ""
        Layout.fillWidth: true
        implicitHeight: 64
        radius: 12
        color: Theme.bg
        border.width: 1
        border.color: Theme.borderSubtle
        ColumnLayout {
            anchors.left: parent.left
            anchors.right: parent.right
            anchors.leftMargin: 14
            anchors.rightMargin: 10
            anchors.verticalCenter: parent.verticalCenter
            spacing: 3
            RowLayout {
                spacing: 6
                Icon { name: stat.icon; size: 13; color: Theme.textDim }
                Text { text: stat.label; color: Theme.textDim; font.family: Theme.sans; font.pixelSize: 11 }
            }
            Text {
                Layout.fillWidth: true
                text: stat.value
                color: Theme.text
                font.family: Theme.sans
                font.pixelSize: 16
                font.weight: Font.DemiBold
                elide: Text.ElideRight
            }
        }
    }

    contentItem: ColumnLayout {
        spacing: 0

        // Header: the project, at a glance
        RowLayout {
            Layout.fillWidth: true
            Layout.preferredHeight: 76
            Layout.leftMargin: 20
            Layout.rightMargin: 12
            spacing: 14
            Rectangle {
                implicitWidth: 44
                implicitHeight: 44
                radius: 13
                gradient: Gradient {
                    GradientStop { position: 0; color: Theme.accent }
                    GradientStop { position: 1; color: Qt.darker(Theme.accent, 1.35) }
                }
                Text {
                    anchors.centerIn: parent
                    text: (sheet.project.name || "?").charAt(0).toUpperCase()
                    color: "white"
                    font.family: Theme.brand
                    font.pixelSize: 21
                    font.weight: Font.Bold
                }
            }
            ColumnLayout {
                Layout.fillWidth: true
                spacing: 3
                RowLayout {
                    spacing: 8
                    Text { text: sheet.project.name || ""; color: Theme.text; font.family: Theme.brand; font.pixelSize: 19; font.weight: Font.DemiBold }
                    Tag { visible: sheet.project.current === true; text: sheet.t("ProjectCurrent", "current"); accent: true }
                    Icon {
                        id: headSpin
                        visible: sheet.loading
                        name: "loader"
                        size: 13
                        color: Theme.textDim
                        RotationAnimator on rotation { running: headSpin.visible; from: 0; to: 360; duration: 900; loops: Animation.Infinite }
                    }
                }
                Text {
                    Layout.fillWidth: true
                    text: sheet.project.path || ""
                    elide: Text.ElideMiddle
                    color: Theme.textDim
                    font.family: Theme.mono
                    font.pixelSize: 11
                }
            }
            IconButton { icon: "copy"; iconSize: 14; tip: sheet.t("CopyPath", "Copy path"); onClicked: ipc.copyText(sheet.project.path) }
            IconButton {
                icon: "folder"
                iconSize: 15
                tip: sheet.t("RevealFolder", "Show in file explorer")
                onClicked: Qt.openUrlExternally("file:///" + (sheet.project.path || "").replace(/\\/g, "/"))
            }
            IconButton { icon: "x"; iconSize: 17; onClicked: sheet.close() }
        }
        Rectangle { Layout.fillWidth: true; Layout.preferredHeight: 1; color: Theme.borderSubtle }

        RowLayout {
            Layout.fillWidth: true
            Layout.fillHeight: true
            spacing: 0

            // Sections
            ColumnLayout {
                Layout.preferredWidth: 188
                Layout.maximumWidth: 188
                Layout.fillHeight: true
                Layout.margins: 10
                spacing: 2
                NavItem { key: "general"; label: sheet.t("General", "General"); icon: "sliders" }
                NavItem { key: "git"; label: sheet.t("Git", "Git"); icon: "git-branch" }
                NavItem { key: "agent"; label: sheet.t("ProjectAgent", "Agent"); icon: "zap" }
                Item { Layout.fillHeight: true }
                NavItem { key: "danger"; label: sheet.t("DangerZone", "Danger zone"); icon: "alert-triangle"; danger: true }
            }
            Rectangle { Layout.preferredWidth: 1; Layout.fillHeight: true; color: Theme.borderSubtle }

            // The chosen section, sliding in
            Item {
                Layout.fillWidth: true
                Layout.fillHeight: true
                clip: true
                Repeater {
                    model: ["general", "git", "agent", "danger"]
                    delegate: Flickable {
                        id: page
                        required property string modelData
                        readonly property bool shown: sheet.section === modelData
                        anchors.fill: parent
                        contentHeight: pageLoader.implicitHeight + 40
                        clip: true
                        boundsBehavior: Flickable.StopAtBounds
                        visible: opacity > 0.01
                        opacity: shown ? 1 : 0
                        property real shift: shown ? 0 : 14
                        Behavior on opacity { NumberAnimation { duration: Theme.motionBase; easing.type: Theme.easeOut } }
                        Behavior on shift { NumberAnimation { duration: Theme.motionSlow; easing.type: Theme.easeEmphasis } }
                        transform: Translate { y: page.shift }
                        SmoothWheel { flickable: page }
                        Loader {
                            id: pageLoader
                            x: 20
                            y: 20
                            width: page.width - 40
                            active: page.shown || item !== null
                            sourceComponent: page.modelData === "general" ? generalPage
                                           : page.modelData === "git" ? gitPage
                                           : page.modelData === "agent" ? agentPage : dangerPage
                        }
                    }
                }
            }
        }
    }

    Component {
        id: generalPage
        ColumnLayout {
            spacing: 14
            RowLayout {
                Layout.fillWidth: true
                spacing: 10
                Stat { icon: "message-square"; label: sheet.t("ProjectChats", "Chats"); value: String(sheet.project.count || 0) }
                Stat { icon: "git-branch"; label: sheet.t("ProjectBranch", "Branch"); value: sheet.repo ? (sheet.details.branch || "—") : "—" }
                Stat { icon: "edit-2"; label: sheet.t("ProjectChanged", "Changed files"); value: sheet.repo ? String(sheet.details.changes || 0) : "—" }
            }
            Card {
                title: sheet.t("ProjectLastCommit", "Last commit")
                visible: sheet.repo && !!sheet.details.lastCommit
                RowLayout {
                    Layout.fillWidth: true
                    spacing: 10
                    Icon { name: "git-commit"; size: 15; color: Theme.textDim }
                    ColumnLayout {
                        Layout.fillWidth: true
                        spacing: 1
                        Text {
                            Layout.fillWidth: true
                            text: sheet.details.lastCommit ? sheet.details.lastCommit.subject : ""
                            elide: Text.ElideRight
                            color: Theme.text
                            font.family: Theme.sans
                            font.pixelSize: 13
                        }
                        Text {
                            text: sheet.details.lastCommit ? sheet.details.lastCommit.short + "  ·  " + sheet.details.lastCommit.author + "  ·  " + Format.relTime(sheet.details.lastCommit.time) : ""
                            color: Theme.textDim
                            font.family: Theme.sans
                            font.pixelSize: 11
                        }
                    }
                }
            }
            Card {
                title: sheet.t("ProjectQuick", "Quick actions")
                Flow {
                    Layout.fillWidth: true
                    spacing: 8
                    ChipButton { text: sheet.t("NewChatHere", "New chat here"); iconName: "edit-2"; onClicked: { sheet.close(); ipc.newChatIn(sheet.project.path) } }
                    ChipButton { text: sheet.t("OpenTerminal", "Open a terminal"); iconName: "terminal"; onClicked: { sheet.close(); ipc.terminalOpen(sheet.project.path); ipc.panelRequested("terminal") } }
                    ChipButton { visible: sheet.repo; text: sheet.t("OpenGitPanel", "Git panel"); iconName: "git-commit"; onClicked: { sheet.close(); if (!sheet.project.current) ipc.newChatIn(sheet.project.path); ipc.panelRequested("git") } }
                }
            }
        }
    }

    Component {
        id: gitPage
        ColumnLayout {
            spacing: 14
            Card {
                visible: !sheet.repo
                title: sheet.t("ProjectNotRepoTitle", "Not a git repository")
                help: sheet.t("ProjectNotRepo", "Run git init in this folder to get branches, stashes and accounts here.")
            }
            Card {
                visible: sheet.repo
                title: sheet.t("ProjectCommitAs", "Commit as")
                help: sheet.t("ProjectCommitAsHelp", "The name and email on commits in this project. Serika sets it again whenever you switch to the project.")
                AccountPicker {
                    Layout.fillWidth: true
                    accounts: sheet.accounts
                    value: sheet.who.account || ""
                    globalLabel: sheet.t("GitGlobalIdentity", "Your global git settings")
                    globalDetail: !sheet.who.account && sheet.who.email ? (sheet.who.name || "") + " <" + sheet.who.email + ">" : ""
                    onPicked: (id) => {
                        const a = sheet.accounts.filter(function (x) { return x.id === id })[0]
                        sheet.patch("identity", a ? { account: id, name: a.name, email: a.email } : { account: "" })
                        sheet.loading = true
                        ipc.gitApplyAccountIn(sheet.project.path, id)
                    }
                }
            }
            Card {
                visible: sheet.repo && (sheet.auth.host || "").length > 0
                title: sheet.t("ProjectSignInAs", "Sign in to {h} as", { h: sheet.auth.host || "" })
                help: sheet.t("ProjectSignInHelp", "Used to fetch, pull and push. If it is refused, Serika tries your other GitHub CLI logins and keeps the one that works.")
                footer: sheet.auth.url || ""
                Flow {
                    Layout.fillWidth: true
                    spacing: 8
                    Repeater {
                        model: {
                            const list = [""].concat((sheet.auth.logins || []).filter(function (l) { return l.length > 0 }))
                            if (sheet.auth.login && list.indexOf(sheet.auth.login) < 0) list.push(sheet.auth.login)
                            return list
                        }
                        delegate: Rectangle {
                            id: chip
                            required property string modelData
                            readonly property bool chosen: (sheet.auth.login || "") === modelData
                            width: chipRow.implicitWidth + 22
                            height: 38
                            radius: 19
                            color: chosen ? Theme.accentSoft : (chipArea.containsMouse ? Theme.hover : "transparent")
                            border.width: 1
                            border.color: chosen ? Theme.accent : Theme.border
                            scale: chipArea.pressed ? 0.96 : 1
                            Behavior on scale { NumberAnimation { duration: Theme.motionFast; easing.type: Theme.easeOut } }
                            Behavior on color { ColorAnimation { duration: Theme.motionFast } }
                            Row {
                                id: chipRow
                                anchors.centerIn: parent
                                spacing: 8
                                Avatar {
                                    visible: chip.modelData.length > 0
                                    anchors.verticalCenter: parent.verticalCenter
                                    size: 24
                                    name: chip.modelData
                                    source: chip.modelData.length > 0 && sheet.auth.host === "github.com" ? "https://github.com/" + chip.modelData + ".png?size=72" : ""
                                }
                                Icon { visible: chip.modelData.length === 0; anchors.verticalCenter: parent.verticalCenter; name: "globe"; size: 14; color: Theme.textSecondary }
                                Text {
                                    anchors.verticalCenter: parent.verticalCenter
                                    text: chip.modelData.length > 0 ? chip.modelData : sheet.t("ProjectSignInDefaultShort", "Git's default")
                                    color: chip.chosen ? Theme.text : Theme.textSecondary
                                    font.family: Theme.sans
                                    font.pixelSize: 13
                                    font.weight: chip.chosen ? Font.DemiBold : Font.Normal
                                }
                                Icon { visible: chip.chosen; anchors.verticalCenter: parent.verticalCenter; name: "check"; size: 13; color: Theme.accentSecondary }
                            }
                            MouseArea {
                                id: chipArea
                                anchors.fill: parent
                                hoverEnabled: true
                                cursorShape: Qt.PointingHandCursor
                                onClicked: if (!chip.chosen) {
                                    sheet.patch("auth", { login: chip.modelData })
                                    sheet.loading = true
                                    ipc.gitSetLogin(sheet.project.path, chip.modelData)
                                }
                            }
                        }
                    }
                }
            }
            Card {
                visible: sheet.repo
                title: sheet.t("GitAccountsTitle", "Git accounts")
                help: sheet.t("ProjectAccountsHelp", "Add or edit the identities you switch between.")
                ChipButton { text: sheet.t("ManageGitAccounts", "Manage git accounts"); iconName: "users"; onClicked: { sheet.close(); ipc.panelRequested("settings:gitaccounts") } }
            }
        }
    }

    Component {
        id: agentPage
        ColumnLayout {
            spacing: 14
            Card {
                title: sheet.t("ProjectInstructions", "Project instructions")
                help: sheet.t("ProjectInstructionsHelp", "Serika reads these files before it starts work here, the same files Claude Code and Codex use, so one file works for all of them.")
                Repeater {
                    model: sheet.details.instructions || []
                    delegate: Rectangle {
                        id: file
                        required property var modelData
                        Layout.fillWidth: true
                        implicitHeight: 44
                        radius: 10
                        color: modelData.exists ? Theme.hover : "transparent"
                        border.width: modelData.exists ? 0 : 1
                        border.color: Theme.borderSubtle
                        RowLayout {
                            anchors.fill: parent
                            anchors.leftMargin: 12
                            anchors.rightMargin: 6
                            spacing: 10
                            Icon { name: "file-text"; size: 15; color: file.modelData.exists ? Theme.accentSecondary : Theme.textDim }
                            Text { Layout.fillWidth: true; text: file.modelData.name; color: file.modelData.exists ? Theme.text : Theme.textDim; font.family: Theme.mono; font.pixelSize: 12 }
                            Text {
                                text: file.modelData.exists ? Math.max(1, Math.round(file.modelData.bytes / 1024)) + " KB" : sheet.t("ProjectMissing", "not there")
                                color: Theme.textDim
                                font.family: Theme.sans
                                font.pixelSize: 11
                            }
                            IconButton {
                                visible: file.modelData.exists
                                icon: "external-link"
                                iconSize: 13
                                implicitWidth: 28
                                implicitHeight: 28
                                tip: sheet.t("Open", "Open")
                                onClicked: Qt.openUrlExternally("file:///" + file.modelData.path.replace(/\\/g, "/"))
                            }
                        }
                    }
                }
                ChipButton {
                    visible: (sheet.details.instructions || []).length > 0 && !(sheet.details.instructions || [])[0].exists
                    primary: true
                    text: sheet.t("ProjectCreateAgents", "Create AGENTS.md")
                    iconName: "plus"
                    onClicked: ipc.projectCreateInstructions(sheet.project.path)
                }
            }
            RowLayout {
                Layout.fillWidth: true
                spacing: 10
                Stat { icon: "package"; label: sheet.t("ProjectSkills", "Project skills"); value: String(sheet.details.skills || 0) }
                Stat { icon: "server"; label: sheet.t("ProjectMcp", "Project MCP servers"); value: String(sheet.details.mcp || 0) }
            }
            Card {
                title: sheet.t("ProjectAgentMore", "Skills and MCP")
                help: sheet.t("ProjectAgentMoreHelp", "Skills in .serika/skills or .claude/skills and servers in .mcp.json load for every chat in this project.")
                Flow {
                    Layout.fillWidth: true
                    spacing: 8
                    ChipButton { text: sheet.t("SkillsNav", "Skills"); iconName: "package"; onClicked: { sheet.close(); ipc.panelRequested("settings:skills") } }
                    ChipButton { text: sheet.t("McpNav", "MCP servers"); iconName: "server"; onClicked: { sheet.close(); ipc.panelRequested("settings:mcp") } }
                }
            }
        }
    }

    Component {
        id: dangerPage
        ColumnLayout {
            spacing: 14
            Card {
                visible: sheet.project.scratch !== true
                title: sheet.t("RemoveProject", "Remove from list")
                help: sheet.t("RemoveProjectHelp", "The folder leaves the sidebar until you open it again. Its chats and files stay.")
                ChipButton {
                    text: sheet.t("RemoveProjectButton", "Remove from list")
                    iconName: "folder-minus"
                    onClicked: { sheet.close(); if (sheet.project.current) ipc.newChat(); ipc.removeProject(sheet.project.path) }
                }
            }
            Card {
                title: sheet.t("DeleteProject", "Delete its chats")
                help: sheet.t("DeleteProjectHelp", "Removes the project and deletes its {n} chats from Serika. Files on disk are never touched.", { n: sheet.project.count || 0 })
                ChipButton {
                    danger: true
                    text: sheet.deleteArmed ? sheet.t("DeleteProjectConfirmShort", "Click again to delete") : sheet.t("DeleteChatsButton", "Delete chats")
                    iconName: "trash-2"
                    onClicked: {
                        if (!sheet.deleteArmed) { sheet.deleteArmed = true; disarm.restart(); return }
                        sheet.close()
                        if (sheet.project.current) ipc.newChat()
                        ipc.deleteProject(sheet.project.path)
                    }
                }
            }
        }
    }
}

import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

// Projects (folders) with their chats, search, plan usage, and the account card.
Rectangle {
    id: side
    property bool open: true
    property var collapsed: ({})

    // Folders come and go (a network share, an unplugged drive): look again while one is
    // missing, and whenever you come back to the app.
    readonly property bool anyMissing: ipc.projects.some(function (p) { return !p.exists })
    Timer {
        interval: 20000
        repeat: true
        running: side.anyMissing && Qt.application.state === Qt.ApplicationActive
        onTriggered: ipc.checkProjects(false)
    }
    Connections {
        target: Qt.application
        function onStateChanged() { if (Qt.application.state === Qt.ApplicationActive) ipc.checkProjects(false) }
    }
    property var expanded: ({})
    readonly property int chatLimit: 6
    signal toggleRequested()
    signal settingsRequested(string tab)
    signal addProjectRequested()
    signal newChatRequested()
    signal newChatInRequested(string path)

    readonly property var dict: ipc.strings
    // How the chat list is arranged, kept in ui.json: grouped by project, by day, or one list;
    // sorted by last activity, newest first, or name; running chats optionally on top.
    readonly property string groupBy: ["projects", "date", "none"].indexOf(ipc.ui.sidebarGroup) >= 0 ? ipc.ui.sidebarGroup : "projects"
    readonly property string sortBy: ["recent", "created", "title"].indexOf(ipc.ui.sidebarSort) >= 0 ? ipc.ui.sidebarSort : "recent"
    readonly property bool runningFirst: ipc.ui.sidebarRunningFirst !== false
    // Chats plus new chats that have no message yet (drafts), which show at once.
    readonly property var allChats: {
        const keyOf = function (path) {
            for (let i = 0; i < ipc.projects.length; i++)
                if (ipc.projects[i].path === path) return ipc.projects[i].key
            return (path || "").replace(/\\/g, "/").toLowerCase()
        }
        const fresh = ipc.drafts.filter(function (d) { return d.fresh }).map(function (d) {
            const firstLine = (d.text || "").trim().split("\n")[0]
            return { id: d.id, title: firstLine.length > 0 ? firstLine : "", updatedAt: d.updatedAt, createdAt: d.createdAt,
                     project: d.project, key: keyOf(d.project), draft: true }
        })
        return fresh.concat(ipc.sessions)
    }
    readonly property var draftIds: {
        const map = {}
        ipc.drafts.forEach(function (d) { if ((d.text || "").trim().length > 0) map[d.id] = true })
        return map
    }
    readonly property var rows: groupBy === "projects"
        ? Format.projectRows(ipc.projects, allChats, filter.text, collapsed, expanded, chatLimit, ipc.liveChats, sortBy, runningFirst)
        : Format.chatRows(allChats, filter.text, collapsed, ipc.liveChats, groupBy, sortBy, runningFirst)
    function t(key, values) { return Format.t(dict, key, values) }

    ListModel { id: rowModel }
    function rowKey(r) { return r.kind + ":" + (r.id || r.key || r.path || "") }
    // Bring rowModel in line with `rows`: move, insert, update or remove only what changed.
    function syncRows() {
        const list = side.rows
        for (let i = 0; i < list.length; i++) {
            const key = rowKey(list[i])
            const json = JSON.stringify(list[i])
            if (i < rowModel.count && rowModel.get(i).key !== key) {
                let found = -1
                for (let j = i + 1; j < rowModel.count; j++)
                    if (rowModel.get(j).key === key) { found = j; break }
                if (found >= 0) rowModel.move(found, i, 1)
                else rowModel.insert(i, { key: key, json: json })
            }
            if (i < rowModel.count) {
                if (rowModel.get(i).json !== json) rowModel.setProperty(i, "json", json)
            } else {
                rowModel.append({ key: key, json: json })
            }
        }
        while (rowModel.count > list.length) rowModel.remove(rowModel.count - 1)
    }
    onRowsChanged: syncRows()
    Component.onCompleted: syncRows()
    function openAccount() { accountMenu.open() }
    function focusSearch() { filter.forceActiveFocus() }
    // Chats in the order the sidebar shows them, for keyboard switching.
    function chatIds() {
        return side.rows.filter(function (r) { return r.kind === "session" }).map(function (r) { return r.id })
    }
    function openNth(n) {
        const ids = chatIds()
        if (n < ids.length) ipc.openSession(ids[n])
    }
    function step(delta) {
        const ids = chatIds()
        if (ids.length === 0) return
        const current = ipc.activeSession.length > 0 ? ipc.activeSession : ipc.currentDraft
        const at = ids.indexOf(current)
        ipc.openSession(ids[(at < 0 ? 0 : at + delta + ids.length) % ids.length])
    }
    // Settings of the project you are in (the first project row otherwise).
    function openProjectSettings() {
        const heads = side.rows.filter(function (r) { return r.kind === "project" })
        const row = heads.filter(function (r) { return r.current })[0] || heads[0]
        if (row) projectSettings.show(row)
    }
    function flip(map, path) {
        const next = Object.assign({}, map)
        next[path] = !(map[path] === true)
        return next
    }

    // Width when open; the shell lets you drag the edge and remembers it.
    property real expandedWidth: 272
    property bool resizing: false
    width: open ? expandedWidth : 56
    color: Theme.card
    clip: true
    Behavior on width { enabled: !side.resizing; NumberAnimation { duration: Theme.motionBase; easing.type: Theme.easeEmphasis } }

    Rectangle { anchors.right: parent.right; width: 1; height: parent.height; color: Theme.borderSubtle }

    component NavRow: Rectangle {
        id: nav
        property string icon: "edit"
        property string label: ""
        property var keys: []
        signal triggered()
        Layout.fillWidth: true
        implicitHeight: 36
        radius: 8
        color: navArea.containsMouse ? Theme.hover : "transparent"
        RowLayout {
            anchors.fill: parent
            anchors.leftMargin: 10
            anchors.rightMargin: 10
            spacing: 12
            Icon { name: nav.icon; size: 18; color: Theme.textSecondary }
            Text {
                Layout.fillWidth: true
                text: nav.label
                elide: Text.ElideRight
                color: Theme.text
                font.family: Theme.sans
                font.pixelSize: 14
            }
            Repeater {
                model: nav.keys
                delegate: Kbd { required property string modelData; label: modelData }
            }
        }
        MouseArea {
            id: navArea
            anchors.fill: parent
            hoverEnabled: true
            cursorShape: Qt.PointingHandCursor
            onClicked: nav.triggered()
        }
    }

    ProjectSettingsSheet { id: projectSettings }

    // Rows of the group-and-sort menu: the chosen one shows a check instead of its icon.
    component Choice: MenuRow {
        property bool chosen: false
        property string glyph: ""
        icon: chosen ? "check" : glyph
    }
    component Heading: Text {
        Layout.leftMargin: 10
        Layout.topMargin: 6
        Layout.bottomMargin: 2
        color: Theme.textDim
        font.family: Theme.sans
        font.pixelSize: 11
        font.weight: Font.Medium
        font.capitalization: Font.AllUppercase
        font.letterSpacing: 0.6
    }

    // Expanded sidebar
    ColumnLayout {
        id: full
        anchors.fill: parent
        anchors.margins: 8
        spacing: 2
        opacity: side.width > 170 ? 1 : 0
        visible: opacity > 0
        Behavior on opacity { NumberAnimation { duration: 120 } }

        RowLayout {
            Layout.fillWidth: true
            Layout.preferredHeight: 40
            Layout.leftMargin: 10
            spacing: 0
            Text {
                text: "serika"
                color: Theme.text
                font.family: Theme.brand
                font.pixelSize: 19
                font.weight: Font.DemiBold
                font.letterSpacing: -0.4
            }
            Text {
                text: " code"
                color: Theme.textDim
                font.family: Theme.brand
                font.pixelSize: 19
                font.letterSpacing: -0.4
            }
            Item { Layout.fillWidth: true }
            IconButton {
                icon: "sidebar"
                tip: Format.tr(side.dict, "ToggleSidebar", "Close sidebar")
                onClicked: side.toggleRequested()
            }
        }

        NavRow {
            icon: "edit"
            label: side.t("NewChat")
            keys: ["Ctrl", "N"]
            onTriggered: side.newChatRequested()
        }

        Rectangle {
            Layout.fillWidth: true
            Layout.topMargin: 6
            Layout.preferredHeight: 36
            radius: 8
            color: Theme.hover
            border.width: 1
            border.color: filter.activeFocus ? Theme.border : "transparent"
            Icon {
                anchors.left: parent.left
                anchors.leftMargin: 12
                anchors.verticalCenter: parent.verticalCenter
                name: "search"
                size: 15
                color: Theme.textDim
            }
            TextField {
                id: filter
                anchors.fill: parent
                anchors.leftMargin: 34
                anchors.rightMargin: 8
                placeholderText: side.t("SearchChats")
                color: Theme.text
                placeholderTextColor: Theme.textDim
                selectionColor: Theme.accent
                font.family: Theme.sans
                font.pixelSize: 14
                background: null
                verticalAlignment: TextInput.AlignVCenter
                Keys.onEscapePressed: text = ""
            }
        }

        // Projects heading
        RowLayout {
            Layout.fillWidth: true
            Layout.topMargin: 10
            Layout.leftMargin: 10
            Layout.rightMargin: 2
            spacing: 6
            Text {
                Layout.fillWidth: true
                elide: Text.ElideRight
                text: side.groupBy === "date" ? Format.tr(side.dict, "ChatsByDate", "Chats by date")
                    : side.groupBy === "none" ? Format.tr(side.dict, "AllChats", "All chats")
                    : Format.tr(side.dict, "Projects", "Projects")
                color: Theme.textDim
                font.family: Theme.sans
                font.pixelSize: 12
                font.weight: Font.Medium
                font.capitalization: Font.AllUppercase
                font.letterSpacing: 0.6
            }
            IconButton {
                id: sortButton
                icon: "sort"
                iconSize: 15
                implicitWidth: 28
                implicitHeight: 28
                active: sortMenu.opened
                tip: Format.tr(side.dict, "SortChats", "Group and sort chats")
                onClicked: sortMenu.opened ? sortMenu.close() : sortMenu.open()
                Popover {
                    id: sortMenu
                    x: 0
                    y: sortButton.height + 4
                    width: 236

                    contentItem: ColumnLayout {
                        spacing: 0
                        Heading { text: Format.tr(side.dict, "GroupBy", "Group by") }
                        Choice { glyph: "folder"; label: Format.tr(side.dict, "GroupProjects", "Projects"); chosen: side.groupBy === "projects"; onTriggered: ipc.setUi("sidebarGroup", "projects") }
                        Choice { glyph: "calendar"; label: Format.tr(side.dict, "GroupDate", "Date"); chosen: side.groupBy === "date"; onTriggered: ipc.setUi("sidebarGroup", "date") }
                        Choice { glyph: "list"; label: Format.tr(side.dict, "GroupNone", "One list"); chosen: side.groupBy === "none"; onTriggered: ipc.setUi("sidebarGroup", "none") }
                        MenuDivider {}
                        Heading { text: Format.tr(side.dict, "SortBy", "Sort by") }
                        Choice { glyph: "clock"; label: Format.tr(side.dict, "SortRecent", "Last activity"); chosen: side.sortBy === "recent"; onTriggered: ipc.setUi("sidebarSort", "recent") }
                        Choice { glyph: "plus"; label: Format.tr(side.dict, "SortCreated", "Newest first"); chosen: side.sortBy === "created"; onTriggered: ipc.setUi("sidebarSort", "created") }
                        Choice { glyph: "type"; label: Format.tr(side.dict, "SortTitle", "Name, A to Z"); chosen: side.sortBy === "title"; onTriggered: ipc.setUi("sidebarSort", "title") }
                        MenuDivider {}
                        Choice {
                            glyph: "loader"
                            label: Format.tr(side.dict, "RunningFirst", "Running chats on top")
                            chosen: side.runningFirst
                            onTriggered: ipc.setUi("sidebarRunningFirst", !side.runningFirst)
                        }
                        Choice {
                            glyph: "minimize-2"
                            label: Format.tr(side.dict, "CollapseAll", "Collapse all")
                            onTriggered: {
                                sortMenu.close()
                                const next = {}
                                side.rows.forEach(function (r) {
                                    if (r.kind === "project") next[r.path] = true
                                    if (r.kind === "group") next["group:" + r.key] = true
                                })
                                side.collapsed = next
                            }
                        }
                        Choice {
                            glyph: "maximize-2"
                            label: Format.tr(side.dict, "ExpandAll", "Expand all")
                            onTriggered: { sortMenu.close(); side.collapsed = ({}) }
                        }
                    }
                }
            }
            IconButton {
                icon: "folder-plus"
                iconSize: 16
                implicitWidth: 28
                implicitHeight: 28
                tip: Format.tr(side.dict, "AddProject", "Add project folder")
                onClicked: side.addProjectRequested()
            }
        }

        ListView {
            id: list
            SmoothWheel { flickable: list }
            Layout.fillWidth: true
            Layout.fillHeight: true
            Layout.topMargin: 2
            clip: true
            spacing: 1
            // Rows live in a model updated in place, so new chats slide in and the rest glide
            // aside instead of the whole list being rebuilt.
            model: rowModel
            add: Transition {
                ParallelAnimation {
                    NumberAnimation { property: "opacity"; from: 0; to: 1; duration: Theme.motionBase; easing.type: Theme.easeOut }
                    NumberAnimation { property: "x"; from: -10; to: 0; duration: Theme.motionSlow; easing.type: Theme.easeEmphasis }
                }
            }
            remove: Transition {
                NumberAnimation { property: "opacity"; to: 0; duration: Theme.motionFast; easing.type: Easing.InCubic }
            }
            displaced: Transition {
                NumberAnimation { properties: "x,y"; duration: Theme.motionBase; easing.type: Theme.easeEmphasis }
                NumberAnimation { property: "opacity"; to: 1; duration: Theme.motionFast }
            }
            boundsBehavior: Flickable.StopAtBounds
            ScrollBar.vertical: ScrollBar { policy: ScrollBar.AsNeeded; width: 6 }

            delegate: Item {
                id: cell
                required property string json
                readonly property var modelData: JSON.parse(json)
                width: ListView.view.width
                height: loader.item ? loader.item.implicitHeight : 34

                Loader {
                    id: loader
                    width: parent.width
                    sourceComponent: cell.modelData.kind === "project" ? projectHead
                                   : cell.modelData.kind === "group" ? groupHead
                                   : cell.modelData.kind === "session" ? sessionRow
                                   : cell.modelData.kind === "more" ? moreRow : emptyRow
                }

                Component {
                    id: projectHead
                    Rectangle {
                        id: head
                        readonly property var r: cell.modelData
                        implicitHeight: 34
                        radius: 8
                        // The project you are in stands out; the others light up on hover.
                        color: head.r.current ? Qt.rgba(Theme.accent.r, Theme.accent.g, Theme.accent.b, headHover.hovered ? 0.16 : 0.10)
                             : (headHover.hovered || headMenu.opened) ? Theme.hover : "transparent"
                        Behavior on color { ColorAnimation { duration: Theme.motionFast } }
                        opacity: r.exists ? 1 : 0.6
                        HoverHandler { id: headHover }
                        Rectangle {
                            visible: head.r.current
                            anchors.left: parent.left
                            anchors.verticalCenter: parent.verticalCenter
                            width: 3
                            height: 16
                            radius: 1.5
                            color: Theme.accent
                        }

                        RowLayout {
                            anchors.fill: parent
                            anchors.leftMargin: 6
                            anchors.rightMargin: 4
                            spacing: 6
                            // The arrow folds the project; the rest of the row switches to it.
                            Item {
                                Layout.preferredWidth: 18
                                Layout.preferredHeight: 26
                                Rectangle {
                                    anchors.centerIn: parent
                                    width: 18
                                    height: 18
                                    radius: 5
                                    color: foldArea.containsMouse ? Theme.active : "transparent"
                                }
                                Icon {
                                    anchors.centerIn: parent
                                    name: "chevron-right"
                                    size: 13
                                    color: foldArea.containsMouse ? Theme.text : Theme.textDim
                                    rotation: head.r.collapsed ? 0 : 90
                                    Behavior on rotation { NumberAnimation { duration: 130 } }
                                }
                                MouseArea {
                                    id: foldArea
                                    anchors.fill: parent
                                    anchors.margins: -2
                                    hoverEnabled: true
                                    cursorShape: Qt.PointingHandCursor
                                    onClicked: side.collapsed = side.flip(side.collapsed, head.r.path)
                                    ToolTip.visible: containsMouse
                                    ToolTip.delay: 500
                                    ToolTip.text: head.r.collapsed ? Format.tr(side.dict, "ExpandProject", "Show chats") : Format.tr(side.dict, "CollapseProject", "Hide chats")
                                }
                            }
                            Icon {
                                name: head.r.exists ? "folder" : "alert-triangle"
                                size: 16
                                color: !head.r.exists ? Theme.warning : (head.r.current ? Theme.accentSecondary : Theme.textSecondary)
                            }
                            Text {
                                Layout.fillWidth: true
                                text: head.r.name
                                elide: Text.ElideRight
                                color: head.r.current ? Theme.text : Theme.textSecondary
                                font.family: Theme.sans
                                font.pixelSize: 14
                                font.weight: head.r.current ? Font.DemiBold : Font.Normal
                            }
                            Rectangle {
                                visible: head.r.current
                                width: 6
                                height: 6
                                radius: 3
                                color: Theme.accent
                            }
                            // Chats of this project working right now (amber when one waits for you)
                            Rectangle {
                                visible: head.r.running > 0 && !(headHover.hovered || headMenu.opened)
                                implicitWidth: liveRow.implicitWidth + 12
                                implicitHeight: 18
                                radius: 9
                                color: head.r.waiting ? Qt.rgba(0.96, 0.62, 0.04, 0.16) : Theme.accentSoft
                                Row {
                                    id: liveRow
                                    anchors.centerIn: parent
                                    spacing: 4
                                    Icon {
                                        id: headSpin
                                        anchors.verticalCenter: parent.verticalCenter
                                        name: head.r.waiting ? "alert-circle" : "loader"
                                        size: 11
                                        color: head.r.waiting ? Theme.warning : Theme.accentSecondary
                                        RotationAnimator on rotation { running: headSpin.visible && !head.r.waiting; from: 0; to: 360; duration: 1000; loops: Animation.Infinite }
                                    }
                                    Text {
                                        anchors.verticalCenter: parent.verticalCenter
                                        text: head.r.running
                                        color: head.r.waiting ? Theme.warning : Theme.accentSecondary
                                        font.family: Theme.sans
                                        font.pixelSize: 11
                                        font.weight: Font.DemiBold
                                    }
                                }
                            }
                            Text {
                                visible: !(headHover.hovered || headMenu.opened) && head.r.count > 0 && head.r.running === 0
                                text: head.r.count
                                color: Theme.textDim
                                font.family: Theme.sans
                                font.pixelSize: 12
                            }
                            IconButton {
                                visible: !head.r.exists && (headHover.hovered || headMenu.opened)
                                icon: "refresh-cw"
                                iconSize: 14
                                implicitWidth: 26
                                implicitHeight: 26
                                tip: Format.tr(side.dict, "CheckAgain", "Check again")
                                onClicked: ipc.checkProjects(true)
                            }
                            IconButton {
                                visible: head.r.exists && (headHover.hovered || headMenu.opened)
                                icon: "edit-2"
                                iconSize: 15
                                implicitWidth: 26
                                implicitHeight: 26
                                tip: Format.tr(side.dict, "NewChatHere", "New chat in this project")
                                onClicked: side.newChatInRequested(head.r.path)
                            }
                            IconButton {
                                id: headMenuButton
                                visible: headHover.hovered || headMenu.opened
                                icon: "more-horizontal"
                                iconSize: 16
                                implicitWidth: 26
                                implicitHeight: 26
                                active: headMenu.opened
                                onClicked: headMenu.opened ? headMenu.close() : headMenu.open()
                                Popover {
                                    id: headMenu
                                    x: headMenuButton.width - width
                                    y: headMenuButton.height + 4
                                    width: head.r.exists ? 220 : 270
                                    contentItem: ColumnLayout {
                                        spacing: 0
                                        // The folder is gone: say which, and how to get it back.
                                        ColumnLayout {
                                            visible: !head.r.exists
                                            Layout.fillWidth: true
                                            Layout.margins: 8
                                            Layout.bottomMargin: 6
                                            spacing: 3
                                            Text {
                                                Layout.fillWidth: true
                                                text: Format.tr(side.dict, "FolderNotFound", "Folder not found")
                                                color: Theme.warning
                                                font.family: Theme.sans
                                                font.pixelSize: 13
                                                font.weight: Font.DemiBold
                                            }
                                            Text {
                                                Layout.fillWidth: true
                                                text: head.r.path.replace(/^\\\\\?\\/, "")
                                                wrapMode: Text.WrapAnywhere
                                                maximumLineCount: 3
                                                elide: Text.ElideMiddle
                                                color: Theme.textDim
                                                font.family: Theme.mono
                                                font.pixelSize: 11
                                            }
                                        }
                                        MenuRow {
                                            visible: !head.r.exists
                                            icon: "folder"
                                            label: Format.tr(side.dict, "LocateFolder", "Locate folder…")
                                            onTriggered: { headMenu.close(); ipc.requestPanel("locate:" + head.r.path) }
                                        }
                                        MenuRow {
                                            visible: !head.r.exists
                                            icon: "refresh-cw"
                                            label: Format.tr(side.dict, "CheckAgain", "Check again")
                                            onTriggered: { headMenu.close(); ipc.checkProjects(true) }
                                        }
                                        MenuRow {
                                            visible: head.r.exists
                                            icon: "edit-2"
                                            label: Format.tr(side.dict, "NewChatHere", "New chat in this project")
                                            onTriggered: { headMenu.close(); side.newChatInRequested(head.r.path) }
                                        }
                                        MenuRow {
                                            visible: head.r.exists
                                            icon: "folder"
                                            label: Format.tr(side.dict, "RevealFolder", "Show in file explorer")
                                            onTriggered: { headMenu.close(); Qt.openUrlExternally("file:///" + head.r.path.replace(/\\/g, "/")) }
                                        }
                                        MenuRow {
                                            visible: head.r.exists
                                            icon: "sliders"
                                            label: Format.tr(side.dict, "ProjectSettings", "Project settings")
                                            onTriggered: { headMenu.close(); projectSettings.show(head.r) }
                                        }
                                        MenuRow {
                                            icon: "copy"
                                            label: Format.tr(side.dict, "CopyPath", "Copy path")
                                            onTriggered: { headMenu.close(); ipc.copyText(head.r.path) }
                                        }
                                        MenuDivider { visible: !head.r.scratch || head.r.count > 0 }
                                        // Chats stay; the folder leaves the list until you open it again.
                                        MenuRow {
                                            visible: !head.r.scratch
                                            icon: "folder-minus"
                                            label: Format.tr(side.dict, "RemoveProject", "Remove from list")
                                            onTriggered: {
                                                headMenu.close()
                                                if (head.r.current) ipc.newChat()
                                                ipc.removeProject(head.r.path)
                                            }
                                        }
                                        // Deletes the chats too (files on disk are never touched). Asks twice.
                                        MenuRow {
                                            id: deleteRow
                                            visible: !head.r.scratch || head.r.count > 0
                                            property bool armed: false
                                            icon: "trash-2"
                                            danger: true
                                            label: armed ? Format.tr(side.dict, "DeleteProjectConfirm", "Click again to delete {n} chats").replace("{n}", head.r.count)
                                                         : (head.r.scratch ? Format.tr(side.dict, "DeleteScratchChats", "Delete these chats")
                                                                           : Format.tr(side.dict, "DeleteProject", "Remove and delete its chats"))
                                            onTriggered: {
                                                if (!armed && head.r.count > 0) { armed = true; disarm.restart(); return }
                                                armed = false
                                                headMenu.close()
                                                if (head.r.current) ipc.newChat()
                                                ipc.deleteProject(head.r.path)
                                            }
                                            Timer { id: disarm; interval: 4000; onTriggered: deleteRow.armed = false }
                                        }
                                    }
                                }
                            }
                        }
                        MouseArea {
                            id: headArea
                            anchors.fill: parent
                            hoverEnabled: true
                            cursorShape: Qt.PointingHandCursor
                            z: -1
                            // Switch to this project: a fresh chat there, with its chats shown.
                            onClicked: {
                                if (head.r.collapsed) side.collapsed = side.flip(side.collapsed, head.r.path)
                                if (!head.r.exists) {
                                    // Maybe it is back (a share that reconnected); either way, show the options.
                                    ipc.checkProjects(false)
                                    headMenu.open()
                                } else if (!head.r.current) {
                                    side.newChatInRequested(head.r.path)
                                }
                            }
                            ToolTip.visible: containsMouse
                            ToolTip.delay: 600
                            ToolTip.text: head.r.path.replace(/^\\\\\?\\/, "")
                                          + (!head.r.exists ? "\n" + Format.tr(side.dict, "FolderNotFoundTip", "Folder not found. Click to locate it or check again.")
                                             : head.r.current ? "\n" + Format.tr(side.dict, "CurrentProject", "You are working in this project")
                                             : "\n" + Format.tr(side.dict, "SwitchProject", "Click to switch to this project"))
                        }
                    }
                }

                Component {
                    id: sessionRow
                    SessionRow {
                        sessionId: cell.modelData.id
                        title: cell.modelData.title
                        draft: cell.modelData.id.indexOf("draft:") === 0
                        unsent: side.draftIds[cell.modelData.id] === true
                        indent: cell.modelData.flat ? 8 : 22
                        // Without project groups, say which folder the chat belongs to.
                        time: cell.modelData.flat ? Format.ellipsize(cell.modelData.folder, 12) + " · " + Format.relTime(cell.modelData.updatedAt)
                                                  : Format.relTime(cell.modelData.updatedAt)
                        active: cell.modelData.id === ipc.activeSession || (cell.modelData.id === ipc.currentDraft && ipc.activeSession.length === 0)
                    }
                }

                // Day (or "Running now") heading when chats are grouped by date
                Component {
                    id: groupHead
                    Rectangle {
                        id: group
                        readonly property var r: cell.modelData
                        implicitHeight: 30
                        radius: 8
                        color: groupArea.containsMouse ? Theme.hover : "transparent"
                        RowLayout {
                            anchors.fill: parent
                            anchors.leftMargin: 6
                            anchors.rightMargin: 8
                            spacing: 6
                            Icon {
                                name: "chevron-right"
                                size: 13
                                color: Theme.textDim
                                rotation: group.r.collapsed ? 0 : 90
                                Behavior on rotation { NumberAnimation { duration: 130 } }
                            }
                            Icon {
                                id: groupSpin
                                visible: group.r.key === "running"
                                name: group.r.waiting ? "alert-circle" : "loader"
                                size: 13
                                color: group.r.waiting ? Theme.warning : Theme.accentSecondary
                                RotationAnimator on rotation { running: groupSpin.visible && !group.r.waiting; from: 0; to: 360; duration: 1000; loops: Animation.Infinite }
                            }
                            Text {
                                Layout.fillWidth: true
                                text: group.r.label
                                elide: Text.ElideRight
                                color: group.r.key === "running" ? Theme.accentSecondary : Theme.textSecondary
                                font.family: Theme.sans
                                font.pixelSize: 13
                                font.weight: Font.DemiBold
                            }
                            Text {
                                text: group.r.count
                                color: Theme.textDim
                                font.family: Theme.sans
                                font.pixelSize: 12
                            }
                        }
                        MouseArea {
                            id: groupArea
                            anchors.fill: parent
                            hoverEnabled: true
                            cursorShape: Qt.PointingHandCursor
                            onClicked: side.collapsed = side.flip(side.collapsed, "group:" + group.r.key)
                        }
                    }
                }

                Component {
                    id: moreRow
                    Item {
                        implicitHeight: 28
                        Text {
                            anchors.left: parent.left
                            anchors.leftMargin: 32
                            anchors.verticalCenter: parent.verticalCenter
                            text: Format.tr(side.dict, "ShowMore", "Show {n} more", { n: cell.modelData.hidden })
                            color: moreArea.containsMouse ? Theme.text : Theme.textDim
                            font.family: Theme.sans
                            font.pixelSize: 13
                        }
                        MouseArea {
                            id: moreArea
                            anchors.fill: parent
                            hoverEnabled: true
                            cursorShape: Qt.PointingHandCursor
                            onClicked: side.expanded = side.flip(side.expanded, cell.modelData.path)
                        }
                    }
                }

                Component {
                    id: emptyRow
                    Item {
                        implicitHeight: 28
                        Text {
                            anchors.left: parent.left
                            anchors.leftMargin: 32
                            anchors.verticalCenter: parent.verticalCenter
                            text: Format.tr(side.dict, "NoChatsYet", "No chats yet")
                            color: Theme.textDim
                            font.family: Theme.sans
                            font.pixelSize: 13
                        }
                    }
                }
            }

            Text {
                visible: side.rows.length === 0
                anchors.top: parent.top
                anchors.topMargin: 16
                anchors.left: parent.left
                anchors.right: parent.right
                anchors.leftMargin: 10
                anchors.rightMargin: 10
                wrapMode: Text.WordWrap
                text: filter.text.length > 0 ? side.t("ChatsNoMatch").replace("{q}", filter.text) : side.t("ChatsEmpty")
                color: Theme.textDim
                font.family: Theme.sans
                font.pixelSize: 14
            }
        }

        Rectangle { Layout.fillWidth: true; Layout.preferredHeight: 1; color: Theme.borderSubtle }

        // Usage meter
        ColumnLayout {
            Layout.fillWidth: true
            Layout.leftMargin: 6
            Layout.rightMargin: 6
            Layout.topMargin: 6
            spacing: 5
            Rectangle {
                Layout.fillWidth: true
                Layout.preferredHeight: 3
                radius: 2
                color: Theme.active
                Rectangle {
                    width: ipc.sessionPercent > 0 ? Math.max(3, parent.width * Math.min(1, ipc.sessionPercent / 100)) : 0
                    height: parent.height
                    radius: 2
                    color: Theme.accent
                    Behavior on width { NumberAnimation { duration: 300 } }
                }
            }
            RowLayout {
                Layout.fillWidth: true
                Text {
                    text: side.t("SessionUsage").replace("{n}", Format.pct(ipc.sessionPercent))
                    color: Theme.textSecondary
                    font.family: Theme.sans
                    font.pixelSize: 12
                }
                Item { Layout.fillWidth: true }
                Text {
                    visible: ipc.sessionResetsAt.length > 0
                    text: side.t("ResetsIn").replace("{n}", Format.countdown(ipc.sessionResetsAt))
                    color: Theme.textDim
                    font.family: Theme.sans
                    font.pixelSize: 12
                }
            }
        }

        // Account card
        Rectangle {
            id: accountCard
            Layout.fillWidth: true
            Layout.preferredHeight: 52
            radius: 8
            color: (cardArea.containsMouse || accountMenu.opened) ? Theme.hover : "transparent"
            RowLayout {
                anchors.fill: parent
                anchors.leftMargin: 8
                anchors.rightMargin: 8
                spacing: 10
                Avatar { size: 32; source: ipc.avatarUrl; name: ipc.username }
                ColumnLayout {
                    Layout.fillWidth: true
                    spacing: 0
                    Text {
                        Layout.fillWidth: true
                        text: ipc.username
                        elide: Text.ElideRight
                        color: Theme.text
                        font.family: Theme.sans
                        font.pixelSize: 14
                    }
                    Text {
                        Layout.fillWidth: true
                        text: {
                            const plan = ipc.plan.length > 0 ? ipc.plan : side.t("FreePlan")
                            const end = Format.expiry(ipc.periodEnd)
                            return end.length > 0 && ipc.isPremium ? plan + " · " + end : plan
                        }
                        elide: Text.ElideRight
                        color: Theme.textDim
                        font.family: Theme.sans
                        font.pixelSize: 12
                    }
                }
                Icon { name: "chevron-up"; size: 16; color: Theme.textDim }
            }
            MouseArea {
                id: cardArea
                anchors.fill: parent
                hoverEnabled: true
                cursorShape: Qt.PointingHandCursor
                onClicked: accountMenu.opened ? accountMenu.close() : accountMenu.open()
            }
            AccountMenu {
                id: accountMenu
                y: -height - 6
                width: accountCard.width
                onSettingsRequested: (tab) => side.settingsRequested(tab)
            }
        }

        RowLayout {
            Layout.alignment: Qt.AlignHCenter
            spacing: 16
            Text {
                text: side.t("Terms")
                color: Theme.textDim
                font.family: Theme.sans
                font.pixelSize: 11
                MouseArea { anchors.fill: parent; cursorShape: Qt.PointingHandCursor; onClicked: Qt.openUrlExternally("https://serika.dev/terms") }
            }
            Text {
                text: side.t("Privacy")
                color: Theme.textDim
                font.family: Theme.sans
                font.pixelSize: 11
                MouseArea { anchors.fill: parent; cursorShape: Qt.PointingHandCursor; onClicked: Qt.openUrlExternally("https://serika.dev/privacy") }
            }
        }
    }

    // Collapsed rail
    ColumnLayout {
        anchors.fill: parent
        anchors.topMargin: 10
        anchors.bottomMargin: 10
        spacing: 4
        opacity: side.width < 120 ? 1 : 0
        visible: opacity > 0
        Behavior on opacity { NumberAnimation { duration: 120 } }

        IconButton { Layout.alignment: Qt.AlignHCenter; icon: "sidebar"; onClicked: side.toggleRequested() }
        IconButton { Layout.alignment: Qt.AlignHCenter; icon: "edit"; onClicked: side.newChatRequested() }
        IconButton {
            Layout.alignment: Qt.AlignHCenter
            icon: "search"
            onClicked: { side.toggleRequested(); Qt.callLater(side.focusSearch) }
        }
        IconButton { Layout.alignment: Qt.AlignHCenter; icon: "folder-plus"; onClicked: side.addProjectRequested() }
        Item { Layout.fillHeight: true }
        IconButton { Layout.alignment: Qt.AlignHCenter; icon: "pie-chart"; onClicked: side.settingsRequested("account") }
        Avatar {
            Layout.alignment: Qt.AlignHCenter
            Layout.topMargin: 4
            size: 32
            source: ipc.avatarUrl
            name: ipc.username
            MouseArea { anchors.fill: parent; cursorShape: Qt.PointingHandCursor; onClicked: side.toggleRequested() }
        }
    }
}

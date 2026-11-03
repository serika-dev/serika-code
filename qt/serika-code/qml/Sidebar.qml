import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

// Projects (folders) with their chats, search, plan usage, and the account card.
Rectangle {
    id: side
    property bool open: true
    property var collapsed: ({})
    property var expanded: ({})
    readonly property int chatLimit: 6
    signal toggleRequested()
    signal settingsRequested(string tab)
    signal addProjectRequested()
    signal newChatRequested()
    signal newChatInRequested(string path)

    readonly property var dict: ipc.strings
    readonly property var rows: Format.projectRows(ipc.projects, ipc.sessions, filter.text, collapsed, expanded, chatLimit)
    function t(key, values) { return Format.t(dict, key, values) }
    function openAccount() { accountMenu.open() }
    function focusSearch() { filter.forceActiveFocus() }
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
                text: Format.tr(side.dict, "Projects", "Projects")
                color: Theme.textDim
                font.family: Theme.sans
                font.pixelSize: 12
                font.weight: Font.Medium
                font.capitalization: Font.AllUppercase
                font.letterSpacing: 0.6
            }
            Item { Layout.fillWidth: true }
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
            Layout.fillWidth: true
            Layout.fillHeight: true
            Layout.topMargin: 2
            clip: true
            spacing: 1
            model: side.rows
            boundsBehavior: Flickable.StopAtBounds
            ScrollBar.vertical: ScrollBar { policy: ScrollBar.AsNeeded; width: 6 }

            delegate: Item {
                id: cell
                required property var modelData
                width: ListView.view.width
                height: loader.item ? loader.item.implicitHeight : 34

                Loader {
                    id: loader
                    width: parent.width
                    sourceComponent: cell.modelData.kind === "project" ? projectHead
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
                        color: (headArea.containsMouse || headMenuButton.hovered || headMenu.opened) ? Theme.hover : "transparent"
                        opacity: r.exists ? 1 : 0.6

                        RowLayout {
                            anchors.fill: parent
                            anchors.leftMargin: 6
                            anchors.rightMargin: 4
                            spacing: 6
                            Icon {
                                name: "chevron-right"
                                size: 13
                                color: Theme.textDim
                                rotation: head.r.collapsed ? 0 : 90
                                Behavior on rotation { NumberAnimation { duration: 130 } }
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
                            Text {
                                visible: !(headArea.containsMouse || headMenuButton.hovered || headMenu.opened) && head.r.count > 0
                                text: head.r.count
                                color: Theme.textDim
                                font.family: Theme.sans
                                font.pixelSize: 12
                            }
                            IconButton {
                                visible: head.r.exists && (headArea.containsMouse || headMenuButton.hovered || headMenu.opened)
                                icon: "edit-2"
                                iconSize: 15
                                implicitWidth: 26
                                implicitHeight: 26
                                tip: Format.tr(side.dict, "NewChatHere", "New chat in this project")
                                onClicked: side.newChatInRequested(head.r.path)
                            }
                            IconButton {
                                id: headMenuButton
                                visible: headArea.containsMouse || hovered || headMenu.opened
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
                                    width: 220
                                    contentItem: ColumnLayout {
                                        spacing: 0
                                        MenuRow {
                                            icon: "edit-2"
                                            label: Format.tr(side.dict, "NewChatHere", "New chat in this project")
                                            enabledRow: head.r.exists
                                            onTriggered: { headMenu.close(); side.newChatInRequested(head.r.path) }
                                        }
                                        MenuRow {
                                            icon: "folder"
                                            label: Format.tr(side.dict, "RevealFolder", "Show in file explorer")
                                            enabledRow: head.r.exists
                                            onTriggered: { headMenu.close(); Qt.openUrlExternally("file:///" + head.r.path.replace(/\\/g, "/")) }
                                        }
                                        MenuRow {
                                            icon: "copy"
                                            label: Format.tr(side.dict, "CopyPath", "Copy path")
                                            onTriggered: { headMenu.close(); ipc.copyText(head.r.path) }
                                        }
                                        MenuDivider {}
                                        MenuRow {
                                            icon: "x"
                                            label: Format.tr(side.dict, "RemoveProject", "Remove from list")
                                            danger: true
                                            onTriggered: { headMenu.close(); ipc.removeProject(head.r.path) }
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
                            onClicked: side.collapsed = side.flip(side.collapsed, head.r.path)
                            ToolTip.visible: containsMouse
                            ToolTip.delay: 700
                            ToolTip.text: head.r.path.replace(/^\\\\\?\\/, "") + (head.r.exists ? "" : "  (folder not found)")
                        }
                    }
                }

                Component {
                    id: sessionRow
                    SessionRow {
                        sessionId: cell.modelData.id
                        title: cell.modelData.title
                        indent: 22
                        time: Format.relTime(cell.modelData.updatedAt)
                        active: cell.modelData.id === ipc.activeSession
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

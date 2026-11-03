import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

// Settings for one project, opened from the ⋯ menu of a project in the sidebar: who it
// commits as, which login it uses for fetch/pull/push, and quick actions.
Sheet {
    id: sheet
    readonly property var dict: ipc.strings
    function t(key, fallback, values) { return Format.tr(dict, key, fallback, values) }
    property var project: ({})          // a sidebar project row: { path, name, count, ... }
    property var info: ({})             // { auth: {remote, url, host, login, logins}, identity }
    property bool loading: false
    readonly property var auth: info.auth || ({})
    readonly property var who: info.identity || ({})
    readonly property var accounts: ipc.gitAccounts.accounts || []
    readonly property bool isRepo: (auth.url || "").length > 0 || (who.email || "").length > 0

    title: t("ProjectSettings", "Project settings")
    maxWidth: 560

    function show(row) {
        project = row
        info = ({})
        loading = true
        open()
        ipc.gitRemoteAuth(row.path)
    }
    Connections {
        target: ipc
        function onGitRemoteAuthReady(path, data) {
            if (path !== sheet.project.path) return
            sheet.info = data
            sheet.loading = false
        }
    }

    Item {
        width: parent.width
        height: body.implicitHeight + 36

        ColumnLayout {
            id: body
            x: 20
            y: 16
            width: parent.width - 40
            spacing: 16

            // Which folder
            RowLayout {
                Layout.fillWidth: true
                spacing: 12
                Rectangle {
                    implicitWidth: 40
                    implicitHeight: 40
                    radius: 12
                    color: Theme.accentSoft
                    Icon { anchors.centerIn: parent; name: "folder"; size: 19; color: Theme.accentSecondary }
                }
                ColumnLayout {
                    Layout.fillWidth: true
                    spacing: 2
                    Text { text: sheet.project.name || ""; color: Theme.text; font.family: Theme.sans; font.pixelSize: 16; font.weight: Font.DemiBold }
                    Text {
                        Layout.fillWidth: true
                        text: sheet.project.path || ""
                        elide: Text.ElideMiddle
                        color: Theme.textDim
                        font.family: Theme.mono
                        font.pixelSize: 11
                    }
                }
                IconButton {
                    icon: "folder"
                    iconSize: 15
                    tip: sheet.t("RevealFolder", "Show in file explorer")
                    onClicked: Qt.openUrlExternally("file:///" + (sheet.project.path || "").replace(/\\/g, "/"))
                }
            }

            Rectangle { Layout.fillWidth: true; Layout.preferredHeight: 1; color: Theme.borderSubtle }

            Row {
                visible: sheet.loading
                spacing: 8
                Icon {
                    id: spin
                    name: "loader"
                    size: 14
                    color: Theme.accentSecondary
                    RotationAnimator on rotation { running: sheet.loading; from: 0; to: 360; duration: 900; loops: Animation.Infinite }
                }
                Text { text: sheet.t("Loading", "Loading…"); color: Theme.textDim; font.family: Theme.sans; font.pixelSize: 13 }
            }

            Text {
                visible: !sheet.loading && !sheet.isRepo
                Layout.fillWidth: true
                wrapMode: Text.WordWrap
                text: sheet.t("ProjectNotRepo", "This folder is not a git repository, so there are no git settings for it.")
                color: Theme.textSecondary
                font.family: Theme.sans
                font.pixelSize: 13
            }

            // Commit identity
            ColumnLayout {
                visible: !sheet.loading && sheet.isRepo
                Layout.fillWidth: true
                spacing: 6
                Text { text: sheet.t("ProjectCommitAs", "Commit as"); color: Theme.text; font.family: Theme.sans; font.pixelSize: 14; font.weight: Font.Medium }
                Text {
                    Layout.fillWidth: true
                    wrapMode: Text.WordWrap
                    text: sheet.t("ProjectCommitAsHelp", "The name and email on commits in this project. Serika sets it again whenever you switch to the project.")
                    color: Theme.textDim
                    font.family: Theme.sans
                    font.pixelSize: 12
                }
                Select {
                    Layout.fillWidth: true
                    options: [{ value: "", label: sheet.t("GitGlobalIdentity", "Your global git settings") }].concat(
                        sheet.accounts.map(function (a) { return { value: a.id, label: a.label + "  ·  " + a.email } }))
                    value: sheet.who.account || ""
                    onPicked: (v) => { sheet.loading = true; ipc.gitApplyAccountIn(sheet.project.path, v) }
                }
                RowLayout {
                    visible: (sheet.who.email || "").length > 0
                    spacing: 8
                    Avatar {
                        size: 20
                        name: sheet.who.name || "?"
                        source: {
                            const a = sheet.accounts.filter(function (x) { return x.id === sheet.who.account })[0]
                            return a ? (a.avatar || "") : ""
                        }
                    }
                    Text {
                        text: sheet.t("ProjectCommitsNow", "Commits now: {n} <{e}>", { n: sheet.who.name || "?", e: sheet.who.email || "" })
                        color: Theme.textSecondary
                        font.family: Theme.sans
                        font.pixelSize: 12
                    }
                }
            }

            // Sign-in for fetch, pull and push
            ColumnLayout {
                visible: !sheet.loading && sheet.isRepo && (sheet.auth.host || "").length > 0
                Layout.fillWidth: true
                spacing: 6
                Text {
                    text: sheet.t("ProjectSignInAs", "Sign in to {h} as", { h: sheet.auth.host || "" })
                    color: Theme.text
                    font.family: Theme.sans
                    font.pixelSize: 14
                    font.weight: Font.Medium
                }
                Text {
                    Layout.fillWidth: true
                    wrapMode: Text.WordWrap
                    text: sheet.t("ProjectSignInHelp", "The login used to fetch, pull and push {r}. If it is refused, Serika tries your other GitHub CLI logins and keeps the one that works.", { r: sheet.auth.url || "" })
                    color: Theme.textDim
                    font.family: Theme.sans
                    font.pixelSize: 12
                }
                // Logins to pick from, with their GitHub pictures
                Flow {
                    Layout.fillWidth: true
                    spacing: 6
                    Repeater {
                        // The login set now is always shown, even when the GitHub CLI does not hold it.
                        model: {
                            const list = [""].concat((sheet.auth.logins || []).filter(function (l) { return l.length > 0 }))
                            if (sheet.auth.login && list.indexOf(sheet.auth.login) < 0) list.push(sheet.auth.login)
                            return list
                        }
                        delegate: Rectangle {
                            id: loginChip
                            required property string modelData
                            readonly property bool chosen: (sheet.auth.login || "") === modelData
                            width: chipRow.implicitWidth + 20
                            height: 34
                            radius: 17
                            color: chosen ? Theme.accentSoft : (chipArea.containsMouse ? Theme.hover : "transparent")
                            border.width: 1
                            border.color: chosen ? Theme.accent : Theme.border
                            Behavior on color { ColorAnimation { duration: Theme.motionFast } }
                            Row {
                                id: chipRow
                                anchors.centerIn: parent
                                spacing: 7
                                Avatar {
                                    visible: loginChip.modelData.length > 0
                                    anchors.verticalCenter: parent.verticalCenter
                                    size: 22
                                    name: loginChip.modelData
                                    source: loginChip.modelData.length > 0 && sheet.auth.host === "github.com"
                                            ? "https://github.com/" + loginChip.modelData + ".png?size=64" : ""
                                }
                                Text {
                                    anchors.verticalCenter: parent.verticalCenter
                                    text: loginChip.modelData.length > 0 ? loginChip.modelData : sheet.t("ProjectSignInDefaultShort", "Git's default")
                                    color: loginChip.chosen ? Theme.text : Theme.textSecondary
                                    font.family: Theme.sans
                                    font.pixelSize: 13
                                    font.weight: loginChip.chosen ? Font.DemiBold : Font.Normal
                                }
                                Icon { visible: loginChip.chosen; anchors.verticalCenter: parent.verticalCenter; name: "check"; size: 13; color: Theme.accentSecondary }
                            }
                            MouseArea {
                                id: chipArea
                                anchors.fill: parent
                                hoverEnabled: true
                                cursorShape: Qt.PointingHandCursor
                                onClicked: if (!loginChip.chosen) { sheet.loading = true; ipc.gitSetLogin(sheet.project.path, loginChip.modelData) }
                            }
                        }
                    }
                }
                Text {
                    visible: (sheet.auth.logins || []).length === 0
                    Layout.fillWidth: true
                    wrapMode: Text.WordWrap
                    text: sheet.t("ProjectNoGh", "Sign in with the GitHub CLI (gh auth login) to choose between accounts here.")
                    color: Theme.textDim
                    font.family: Theme.sans
                    font.pixelSize: 12
                }
            }

            Rectangle { Layout.fillWidth: true; Layout.preferredHeight: 1; color: Theme.borderSubtle }

            RowLayout {
                Layout.fillWidth: true
                spacing: 8
                ChipButton {
                    text: sheet.t("NewChatHere", "New chat in this project")
                    iconName: "edit-2"
                    onClicked: { sheet.close(); ipc.newChatIn(sheet.project.path) }
                }
                ChipButton {
                    text: sheet.t("ManageGitAccounts", "Git accounts…")
                    iconName: "users"
                    onClicked: { sheet.close(); ipc.panelRequested("settings:gitaccounts") }
                }
                Item { Layout.fillWidth: true }
                AccentButton {
                    implicitHeight: 36
                    text: sheet.t("Done", "Done")
                    onClicked: sheet.close()
                }
            }
        }
    }
}

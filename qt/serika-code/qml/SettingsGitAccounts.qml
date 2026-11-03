import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

// Git accounts: identities to switch between per project (name and email for commits, the
// login the credential manager should use, an SSH key), and which one each project uses by
// default. Serika never stores passwords or tokens here.
ColumnLayout {
    id: section
    spacing: 24
    readonly property var dict: ipc.strings
    function t(key, fallback) { return Format.tr(dict, key, fallback) }
    readonly property var accounts: ipc.gitAccounts.accounts || []
    readonly property var defaults: ipc.gitAccounts.defaults || ({})
    property string editing: ""          // account id being edited, "+" for a new one
    property string error: ""
    Component.onCompleted: ipc.gitAccountsRefresh()
    Connections {
        target: ipc
        function onGitMessage(text, isError) { if (isError) section.error = text }
        function onGitAccountsChanged() { if (section.error.length === 0) section.editing = "" }
    }
    function edit(account) {
        error = ""
        editing = account ? account.id : "+"
        labelField.text = account ? account.label : ""
        nameField.text = account ? account.name : ""
        emailField.text = account ? account.email : ""
        userField.text = account ? account.username : ""
        keyField.text = account ? account.sshKey : ""
    }
    function initials(name) {
        const parts = (name || "?").trim().split(/\s+/)
        return ((parts[0] || "?").charAt(0) + (parts.length > 1 ? parts[parts.length - 1].charAt(0) : "")).toUpperCase()
    }

    SettingsCard {
        title: section.t("GitAccountsTitle", "Git accounts")
        description: section.t("GitAccountsHelp", "Switch who you commit as, and which login and SSH key git uses, per project. Pick one from the account button in the Git panel. Passwords and tokens stay in your credential manager; Serika only tells git which login to use.")

        Repeater {
            model: section.accounts
            delegate: Rectangle {
                id: card
                required property var modelData
                Layout.fillWidth: true
                implicitHeight: 58
                radius: 10
                color: cardArea.containsMouse ? Theme.hover : "transparent"
                HoverHandler { id: cardArea }
                RowLayout {
                    anchors.fill: parent
                    anchors.leftMargin: 12
                    anchors.rightMargin: 8
                    spacing: 12
                    Rectangle {
                        implicitWidth: 32
                        implicitHeight: 32
                        radius: 16
                        color: Theme.accent
                        Text { anchors.centerIn: parent; text: section.initials(card.modelData.name); color: "white"; font.family: Theme.sans; font.pixelSize: 12; font.weight: Font.DemiBold }
                    }
                    ColumnLayout {
                        Layout.fillWidth: true
                        spacing: 2
                        RowLayout {
                            spacing: 8
                            Text { text: card.modelData.label; color: Theme.text; font.family: Theme.sans; font.pixelSize: 14; font.weight: Font.Medium }
                            Tag {
                                visible: (card.modelData.source || "").length > 0
                                text: card.modelData.source === "gh" ? section.t("GitFromGh", "from GitHub CLI") : section.t("GitFromGcm", "from Credential Manager")
                            }
                        }
                        Text {
                            Layout.fillWidth: true
                            elide: Text.ElideRight
                            text: card.modelData.name + " <" + card.modelData.email + ">"
                                  + (card.modelData.username ? "  ·  " + section.t("GitLoginShort", "login") + " " + card.modelData.username : "")
                                  + (card.modelData.sshKey ? "  ·  SSH key" : "")
                            color: Theme.textDim
                            font.family: Theme.sans
                            font.pixelSize: 12
                        }
                    }
                    IconButton { icon: "edit-2"; iconSize: 14; tip: section.t("Edit", "Edit"); onClicked: section.edit(card.modelData) }
                    IconButton { icon: "trash-2"; iconSize: 14; danger: true; tip: section.t("Remove", "Remove"); onClicked: ipc.gitAccountRemove(card.modelData.id) }
                }
            }
        }

        SettingRow {
            heading: section.t("GitAddAccount", "Add an account")
            help: section.t("GitAddAccountHelp", "For example your work and personal identities.")
            ChipButton { text: section.t("GitAddAccountButton", "Add account"); iconName: "plus"; onClicked: section.edit(null) }
        }

        // Editor
        ColumnLayout {
            visible: section.editing.length > 0
            Layout.fillWidth: true
            Layout.leftMargin: 12
            Layout.rightMargin: 12
            Layout.bottomMargin: 12
            spacing: 8
            Field { id: labelField; Layout.fillWidth: true; icon: "edit-2"; placeholder: section.t("GitAccountLabel", "Label, e.g. Work") }
            RowLayout {
                Layout.fillWidth: true
                spacing: 8
                Field { id: nameField; Layout.fillWidth: true; icon: "user"; placeholder: section.t("GitAccountName", "Name on commits") }
                Field { id: emailField; Layout.fillWidth: true; icon: "mail"; placeholder: section.t("GitAccountEmail", "Email on commits") }
            }
            Field { id: userField; Layout.fillWidth: true; icon: "globe"; placeholder: section.t("GitAccountLogin", "Login for HTTPS remotes (optional), e.g. your GitHub username") }
            Field { id: keyField; Layout.fillWidth: true; icon: "key"; placeholder: section.t("GitAccountKey", "SSH private key for SSH remotes (optional), e.g. C:\\Users\\you\\.ssh\\id_work") }
            Text {
                visible: section.error.length > 0
                Layout.fillWidth: true
                wrapMode: Text.WordWrap
                text: section.error
                color: Theme.danger
                font.family: Theme.sans
                font.pixelSize: 12
            }
            RowLayout {
                spacing: 8
                AccentButton {
                    text: section.t("Save", "Save")
                    onClicked: {
                        section.error = ""
                        ipc.gitAccountSave({ id: section.editing === "+" ? "" : section.editing, label: labelField.text, name: nameField.text,
                                             email: emailField.text, username: userField.text, sshKey: keyField.text })
                    }
                }
                GhostButton { text: section.t("Cancel", "Cancel"); onClicked: { section.editing = ""; section.error = "" } }
            }
        }
    }

    // Which account each project uses when you open it
    SettingsCard {
        title: section.t("GitProjectDefaults", "Project defaults")
        description: section.t("GitProjectDefaultsHelp", "When you switch to a project, Serika sets its repository to commit and sign in as this account.")
        Repeater {
            model: ipc.projects
            delegate: SettingRow {
                required property var modelData
                heading: modelData.name
                help: modelData.path
                Select {
                    implicitWidth: 220
                    options: [{ value: "", label: section.t("GitGlobalIdentity", "Your global git settings") }].concat(
                        section.accounts.map(function (a) { return { value: a.id, label: a.label } }))
                    value: section.defaults[modelData.key] || ""
                    onPicked: (v) => ipc.gitAccountDefault(modelData.path, v)
                }
            }
        }
    }
}

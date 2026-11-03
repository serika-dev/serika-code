import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

// Serika accounts sign-in: artwork on the left, form panel on the right.
Item {
    id: page
    property bool signingIn: true
    property bool busy: false
    property bool secondFactor: false
    property bool magicSent: false
    property bool agreed: false
    property string notice: ""
    property bool noticeIsError: true
    readonly property var dict: ipc.strings
    readonly property real panelWidth: width >= 1100 ? 520 : 440

    function t(key) { return Format.t(dict, key) }

    ArtBackdrop {
        anchors.left: parent.left
        anchors.top: parent.top
        anchors.bottom: parent.bottom
        width: page.width - page.panelWidth
    }

    Rectangle {
        id: panel
        anchors.right: parent.right
        anchors.top: parent.top
        anchors.bottom: parent.bottom
        width: page.panelWidth
        color: Theme.card

        Rectangle { anchors.left: parent.left; width: 1; height: parent.height; color: Theme.border }

        Flickable {
            id: scroller
            SmoothWheel { flickable: scroller }
            anchors.fill: parent
            anchors.topMargin: 36
            contentWidth: width
            contentHeight: Math.max(height, form.implicitHeight + 64)
            clip: true
            boundsBehavior: Flickable.StopAtBounds
            ScrollBar.vertical: ScrollBar { policy: ScrollBar.AsNeeded }

            ColumnLayout {
                id: form
                width: Math.min(380, panel.width - 80)
                x: (scroller.width - width) / 2
                y: Math.max(32, (scroller.height - implicitHeight) / 2)
                spacing: 0

                ColumnLayout {
                    Layout.alignment: Qt.AlignHCenter
                    spacing: 6
                    Row {
                        Layout.alignment: Qt.AlignHCenter
                        spacing: 0
                        Text {
                            text: "serika"
                            color: Theme.text
                            font.family: Theme.brand
                            font.pixelSize: 38
                            font.weight: Font.DemiBold
                            font.letterSpacing: -0.8
                        }
                        Text {
                            text: " code"
                            color: Theme.textDim
                            font.family: Theme.brand
                            font.pixelSize: 38
                            font.weight: Font.Normal
                            font.letterSpacing: -0.8
                        }
                    }
                    Text {
                        Layout.alignment: Qt.AlignHCenter
                        text: "AI coding agent · Powered by Serika"
                        color: Theme.textDim
                        font.family: Theme.sans
                        font.pixelSize: 13
                    }
                }

                ColumnLayout {
                    Layout.topMargin: 32
                    Layout.alignment: Qt.AlignHCenter
                    spacing: 8
                    Text {
                        Layout.alignment: Qt.AlignHCenter
                        text: page.signingIn ? page.t("WelcomeBack") : page.t("CreateAccount")
                        color: Theme.text
                        font.family: Theme.sans
                        font.pixelSize: 28
                        font.weight: Font.Bold
                    }
                    Text {
                        Layout.alignment: Qt.AlignHCenter
                        text: page.signingIn ? page.t("SignInSubtitle") : page.t("CreateSubtitle")
                        color: Theme.textSecondary
                        font.family: Theme.sans
                        font.pixelSize: 14
                    }
                }

                Rectangle {
                    Layout.fillWidth: true
                    Layout.topMargin: 24
                    visible: page.notice.length > 0
                    implicitHeight: noticeText.implicitHeight + 24
                    radius: 12
                    color: page.noticeIsError ? Theme.dangerSoft : "#1a34d399"
                    border.width: 1
                    border.color: page.noticeIsError ? "#33f43f5e" : "#3334d399"
                    RowLayout {
                        anchors.fill: parent
                        anchors.margins: 12
                        spacing: 10
                        Icon {
                            Layout.alignment: Qt.AlignTop
                            name: page.noticeIsError ? "alert-circle" : "check"
                            size: 17
                            color: page.noticeIsError ? "#f43f5e" : Theme.success
                        }
                        Text {
                            id: noticeText
                            Layout.fillWidth: true
                            text: page.notice
                            wrapMode: Text.WordWrap
                            color: page.noticeIsError ? "#fca5a5" : "#6ee7b7"
                            font.family: Theme.sans
                            font.pixelSize: 13
                        }
                    }
                }

                ColumnLayout {
                    Layout.fillWidth: true
                    Layout.topMargin: 24
                    spacing: 16

                    Field {
                        id: username
                        visible: !page.signingIn
                        Layout.fillWidth: true
                        icon: "user"
                        placeholder: page.t("UsernameLabel")
                    }
                    Field {
                        id: email
                        Layout.fillWidth: true
                        icon: "mail"
                        placeholder: page.t("EmailAddress")
                        inputMethodHints: Qt.ImhEmailCharactersOnly | Qt.ImhNoAutoUppercase
                        onAccepted: page.submit()
                    }
                    Field {
                        id: password
                        Layout.fillWidth: true
                        icon: "lock"
                        secret: true
                        placeholder: page.t("Password")
                        onAccepted: page.submit()
                    }
                    ColumnLayout {
                        visible: !page.signingIn
                        Layout.fillWidth: true
                        spacing: 6
                        Text {
                            text: page.t("DateOfBirth")
                            color: Theme.textDim
                            font.family: Theme.sans
                            font.pixelSize: 12
                        }
                        Field {
                            id: dob
                            Layout.fillWidth: true
                            icon: "calendar"
                            placeholder: "YYYY-MM-DD"
                            onAccepted: page.submit()
                        }
                    }
                    Field {
                        id: factor
                        visible: page.secondFactor
                        Layout.fillWidth: true
                        icon: "shield"
                        placeholder: page.t("AuthCode")
                        onAccepted: page.submit()
                    }

                    CheckBox {
                        id: terms
                        visible: !page.signingIn
                        Layout.fillWidth: true
                        checked: page.agreed
                        onToggled: page.agreed = checked
                        indicator: Rectangle {
                            x: terms.leftPadding
                            y: (terms.height - height) / 2
                            width: 18
                            height: 18
                            radius: 5
                            color: terms.checked ? Theme.accent : Theme.input
                            border.width: 1
                            border.color: terms.checked ? Theme.accent : Theme.border
                            Icon { anchors.centerIn: parent; visible: terms.checked; name: "check"; size: 13; color: "white" }
                        }
                        contentItem: Text {
                            leftPadding: 28
                            text: page.t("AgreeTerms")
                            wrapMode: Text.WordWrap
                            color: Theme.textSecondary
                            font.family: Theme.sans
                            font.pixelSize: 13
                            verticalAlignment: Text.AlignVCenter
                        }
                    }
                    CheckBox {
                        id: remember
                        visible: page.signingIn
                        Layout.fillWidth: true
                        checked: true
                        indicator: Rectangle {
                            x: remember.leftPadding
                            y: (remember.height - height) / 2
                            width: 18
                            height: 18
                            radius: 5
                            color: remember.checked ? Theme.accent : Theme.input
                            border.width: 1
                            border.color: remember.checked ? Theme.accent : Theme.border
                            Icon { anchors.centerIn: parent; visible: remember.checked; name: "check"; size: 13; color: "white" }
                        }
                        contentItem: Text {
                            leftPadding: 28
                            text: page.t("RememberMe")
                            color: Theme.textSecondary
                            font.family: Theme.sans
                            font.pixelSize: 13
                            verticalAlignment: Text.AlignVCenter
                        }
                    }

                    AccentButton {
                        Layout.fillWidth: true
                        text: page.busy ? "…" : (page.signingIn ? page.t("SignIn") : page.t("CreateAccount"))
                        enabled: !page.busy && (page.signingIn || page.agreed)
                        onClicked: page.submit()
                    }
                }

                // Email sign-in link: send it, then paste the token from the email.
                ColumnLayout {
                    visible: page.signingIn
                    Layout.fillWidth: true
                    Layout.topMargin: 20
                    spacing: 12

                    RowLayout {
                        Layout.fillWidth: true
                        spacing: 12
                        Rectangle { Layout.fillWidth: true; Layout.preferredHeight: 1; color: Theme.border }
                        Text { text: page.t("Or"); color: Theme.textDim; font.family: Theme.sans; font.pixelSize: 13 }
                        Rectangle { Layout.fillWidth: true; Layout.preferredHeight: 1; color: Theme.border }
                    }
                    GhostButton {
                        Layout.fillWidth: true
                        implicitHeight: 44
                        text: page.t("SendLink")
                        enabled: !page.busy
                        onClicked: page.sendLink()
                    }
                    Field {
                        id: magic
                        visible: page.magicSent
                        Layout.fillWidth: true
                        icon: "link"
                        placeholder: page.t("LinkToken")
                        onAccepted: page.consumeLink()
                    }
                    AccentButton {
                        visible: page.magicSent
                        Layout.fillWidth: true
                        text: page.t("ContinueLink")
                        enabled: !page.busy
                        onClicked: page.consumeLink()
                    }
                    Text {
                        Layout.alignment: Qt.AlignHCenter
                        text: page.t("ForgotPassword")
                        color: Theme.accentSecondary
                        font.family: Theme.sans
                        font.pixelSize: 13
                        MouseArea {
                            anchors.fill: parent
                            cursorShape: Qt.PointingHandCursor
                            onClicked: Qt.openUrlExternally("https://accounts.serika.dev/forgot-password")
                        }
                    }
                }

                Text {
                    Layout.alignment: Qt.AlignHCenter
                    Layout.topMargin: 16
                    text: page.signingIn ? page.t("NeedAccount") : page.t("HaveAccount")
                    color: Theme.textSecondary
                    font.family: Theme.sans
                    font.pixelSize: 14
                    MouseArea {
                        anchors.fill: parent
                        cursorShape: Qt.PointingHandCursor
                        onClicked: {
                            page.signingIn = !page.signingIn
                            page.notice = ""
                            page.secondFactor = false
                            page.magicSent = false
                        }
                    }
                }

                RowLayout {
                    Layout.alignment: Qt.AlignHCenter
                    Layout.topMargin: 28
                    spacing: 6
                    Text { text: "© 2026 Serika.dev"; color: Theme.textDim; font.family: Theme.sans; font.pixelSize: 11 }
                    Text { text: "·"; color: Theme.textDim; font.pixelSize: 11 }
                    Text {
                        text: page.t("Privacy")
                        color: Theme.textDim
                        font.family: Theme.sans
                        font.pixelSize: 11
                        MouseArea {
                            anchors.fill: parent
                            cursorShape: Qt.PointingHandCursor
                            onClicked: Qt.openUrlExternally("https://serika.dev/privacy")
                        }
                    }
                }
            }
        }
    }

    function show(message, isError) {
        page.notice = message
        page.noticeIsError = isError
    }

    function submit() {
        if (page.busy) return
        page.notice = ""
        if (email.text.trim().length === 0 || password.text.length === 0) {
            show(page.t("EnterCredentials"), true)
            return
        }
        page.busy = true
        if (page.signingIn)
            ipc.login(email.text, password.text, factor.text)
        else
            ipc.registerAccount(username.text, email.text, password.text, dob.text)
    }

    function sendLink() {
        if (email.text.trim().length === 0) {
            show(page.t("EnterCredentials"), true)
            return
        }
        page.busy = true
        page.notice = ""
        ipc.requestMagicLink(email.text)
    }

    function consumeLink() {
        if (magic.text.trim().length === 0) return
        page.busy = true
        page.notice = ""
        ipc.consumeMagicLink(magic.text, factor.text)
    }

    Connections {
        target: ipc
        function onAuthFinished(ok, message, needsSecondFactor) {
            const wasMagicRequest = page.busy && message.length > 0 && ok && !needsSecondFactor && !ipc.signedIn
            page.busy = false
            page.secondFactor = needsSecondFactor
            if (needsSecondFactor) {
                show(message.length > 0 ? message : page.t("SecondFactorHint"), true)
            } else if (!ok) {
                show(message, true)
            } else if (wasMagicRequest) {
                page.magicSent = true
                show(message, false)
            }
        }
        function onBridgeErrorChanged() {
            if (ipc.bridgeError.length > 0)
                page.show(ipc.bridgeError, true)
        }
    }
}

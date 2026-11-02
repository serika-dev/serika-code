import QtQuick
import QtQuick.Controls

// Text input with a leading Feather icon (the sign-in form field).
Rectangle {
    id: field
    property alias text: input.text
    property alias placeholder: input.placeholderText
    property alias echoMode: input.echoMode
    property alias inputMethodHints: input.inputMethodHints
    property string icon: "mail"
    property bool secret: false
    property bool revealed: false
    signal accepted()
    signal edited()

    implicitHeight: 46
    radius: 12
    color: Theme.input
    border.width: 1
    border.color: input.activeFocus ? Theme.accent : Theme.border

    Icon {
        id: lead
        anchors.left: parent.left
        anchors.leftMargin: 14
        anchors.verticalCenter: parent.verticalCenter
        name: field.icon
        size: 18
        color: Theme.textDim
    }
    TextField {
        id: input
        anchors.left: lead.right
        anchors.leftMargin: 10
        anchors.right: eye.visible ? eye.left : parent.right
        anchors.rightMargin: 8
        anchors.verticalCenter: parent.verticalCenter
        color: Theme.text
        placeholderTextColor: Theme.textDim
        selectionColor: Theme.accent
        selectedTextColor: "white"
        font.family: Theme.sans
        font.pixelSize: 14
        echoMode: field.secret && !field.revealed ? TextInput.Password : TextInput.Normal
        background: null
        leftPadding: 0
        onAccepted: field.accepted()
        onTextEdited: field.edited()
    }
    IconButton {
        id: eye
        visible: field.secret
        anchors.right: parent.right
        anchors.rightMargin: 6
        anchors.verticalCenter: parent.verticalCenter
        icon: field.revealed ? "eye-off" : "eye"
        iconSize: 17
        onClicked: field.revealed = !field.revealed
    }
}

import QtQuick
import QtQuick.Controls

// Text input for settings. Saves when you press Enter or click away.
Rectangle {
    id: field
    property string value: ""
    property string placeholder: ""
    property bool mono: false
    signal committed(string text)

    implicitWidth: 320
    implicitHeight: 36
    radius: 8
    color: Theme.input
    border.width: 1
    border.color: input.activeFocus ? Theme.accent : Theme.borderSubtle

    TextField {
        id: input
        anchors.fill: parent
        anchors.leftMargin: 6
        anchors.rightMargin: 6
        background: null
        color: Theme.text
        placeholderText: field.placeholder
        placeholderTextColor: Theme.textDim
        selectionColor: Theme.accent
        selectedTextColor: "white"
        selectByMouse: true
        font.family: field.mono ? Theme.mono : Theme.sans
        font.pixelSize: field.mono ? 12 : 14
        verticalAlignment: TextInput.AlignVCenter
        text: field.value
        onEditingFinished: if (text !== field.value) field.committed(text)
    }
    onValueChanged: if (!input.activeFocus) input.text = value
}

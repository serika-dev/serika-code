import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

// Fenced code with the language and a copy button.
Rectangle {
    id: card
    property string language: ""
    property string code: ""
    property bool copied: false

    implicitHeight: column.implicitHeight
    radius: 12
    color: Theme.code
    border.width: 1
    border.color: Theme.borderSubtle

    ColumnLayout {
        id: column
        anchors.left: parent.left
        anchors.right: parent.right
        spacing: 0

        RowLayout {
            Layout.fillWidth: true
            Layout.preferredHeight: 34
            Layout.leftMargin: 14
            Layout.rightMargin: 6
            Text {
                Layout.fillWidth: true
                text: card.language.length > 0 ? card.language : "text"
                color: Theme.textDim
                font.family: Theme.mono
                font.pixelSize: 12
            }
            Item {
                implicitWidth: copyRow.implicitWidth + 12
                implicitHeight: 26
                Row {
                    id: copyRow
                    anchors.centerIn: parent
                    spacing: 4
                    Icon {
                        anchors.verticalCenter: parent.verticalCenter
                        name: card.copied ? "check" : "copy"
                        size: 13
                        color: card.copied ? Theme.success : Theme.textDim
                    }
                    Text {
                        anchors.verticalCenter: parent.verticalCenter
                        text: Format.t(ipc.strings, card.copied ? "Copied" : "Copy")
                        color: card.copied ? Theme.success : Theme.textDim
                        font.family: Theme.sans
                        font.pixelSize: 12
                    }
                }
                MouseArea {
                    anchors.fill: parent
                    cursorShape: Qt.PointingHandCursor
                    onClicked: { ipc.copyText(card.code); card.copied = true; reset.restart() }
                }
                Timer { id: reset; interval: 1500; onTriggered: card.copied = false }
            }
            Item { Layout.preferredWidth: 8 }
        }

        Rectangle { Layout.fillWidth: true; Layout.preferredHeight: 1; color: Theme.borderSubtle }

        Flickable {
            id: scroller
            Layout.fillWidth: true
            Layout.preferredHeight: codeText.implicitHeight + 24
            contentWidth: Math.max(width, codeText.implicitWidth + 28)
            contentHeight: height
            clip: true
            boundsBehavior: Flickable.StopAtBounds
            flickableDirection: Flickable.HorizontalFlick
            ScrollBar.horizontal: ScrollBar { policy: ScrollBar.AsNeeded; height: 6 }

            TextEdit {
                id: codeText
                x: 14
                y: 12
                text: card.code
                readOnly: true
                selectByMouse: true
                selectionColor: Theme.accent
                wrapMode: TextEdit.NoWrap
                color: Theme.text
                font.family: Theme.mono
                font.pixelSize: 13
            }
        }
    }
}

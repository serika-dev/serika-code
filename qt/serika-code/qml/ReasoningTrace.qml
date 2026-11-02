import QtQuick
import QtQuick.Layouts

// Frameless "Thought process · N steps" with a hairline on the left edge, as in SerikaChat.
ColumnLayout {
    id: trace
    property string text: ""
    property bool streaming: false
    property bool autoOpen: false
    property bool openByDefault: ipc.ui.reasoningOpen === true
    property int userState: -1
    readonly property bool open: userState >= 0 ? userState === 1 : (autoOpen || openByDefault)
    spacing: 6

    Item {
        Layout.preferredHeight: 26
        Layout.preferredWidth: header.implicitWidth + 8

        Row {
            id: header
            anchors.verticalCenter: parent.verticalCenter
            spacing: 6
            Icon {
                name: "chevron-right"
                size: 14
                color: Theme.textDim
                anchors.verticalCenter: parent.verticalCenter
                rotation: trace.open ? 90 : 0
                Behavior on rotation { NumberAnimation { duration: 140 } }
            }
            Text {
                anchors.verticalCenter: parent.verticalCenter
                text: trace.streaming && trace.autoOpen
                      ? Format.t(ipc.strings, "Thinking")
                      : Format.t(ipc.strings, "ThoughtProcess", { n: Format.steps(trace.text) })
                color: Theme.textDim
                font.family: Theme.sans
                font.pixelSize: 13
            }
            Rectangle {
                visible: trace.streaming && trace.autoOpen
                anchors.verticalCenter: parent.verticalCenter
                width: 6
                height: 6
                radius: 3
                color: Theme.accentSecondary
                SequentialAnimation on opacity {
                    running: trace.streaming && trace.autoOpen
                    loops: Animation.Infinite
                    NumberAnimation { to: 0.25; duration: 600 }
                    NumberAnimation { to: 1; duration: 600 }
                }
            }
        }
        MouseArea {
            anchors.fill: parent
            cursorShape: Qt.PointingHandCursor
            onClicked: trace.userState = trace.open ? 0 : 1
        }
    }

    RowLayout {
        visible: trace.open
        Layout.fillWidth: true
        spacing: 12
        Rectangle {
            Layout.fillHeight: true
            Layout.preferredWidth: 1.5
            Layout.leftMargin: 6
            color: Theme.border
        }
        TextEdit {
            Layout.fillWidth: true
            text: trace.text
            readOnly: true
            selectByMouse: true
            selectionColor: Theme.accent
            wrapMode: TextEdit.Wrap
            textFormat: TextEdit.MarkdownText
            color: Theme.textSecondary
            font.family: Theme.sans
            font.pixelSize: 13
        }
    }
}

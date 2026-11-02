import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

// The agent asks before running a command or touching files.
Sheet {
    id: sheet
    readonly property var dict: ipc.strings
    readonly property bool destructive: ipc.approval.destructive === true
    function t(key) { return Format.t(dict, key) }

    title: ipc.approval.title || t("ApproveTitle")
    maxWidth: 560
    closable: false

    Connections {
        target: ipc
        function onApprovalChanged() {
            if (ipc.approval.id) sheet.open()
            else sheet.close()
        }
    }

    Item {
        width: parent.width
        height: body.implicitHeight + 40

        ColumnLayout {
            id: body
            anchors.left: parent.left
            anchors.right: parent.right
            anchors.top: parent.top
            anchors.margins: 20
            spacing: 14

            Text {
                Layout.fillWidth: true
                text: sheet.t("ApprovalQuestion")
                wrapMode: Text.WordWrap
                color: sheet.destructive ? Theme.danger : Theme.textSecondary
                font.family: Theme.sans
                font.pixelSize: 14
            }
            Rectangle {
                Layout.fillWidth: true
                implicitHeight: Math.min(220, detail.implicitHeight + 24)
                radius: 10
                color: Theme.code
                border.width: 1
                border.color: Theme.borderSubtle
                clip: true
                Flickable {
                    anchors.fill: parent
                    anchors.margins: 12
                    contentWidth: width
                    contentHeight: detail.implicitHeight
                    boundsBehavior: Flickable.StopAtBounds
                    TextEdit {
                        id: detail
                        width: parent.width
                        text: ipc.approval.detail || ""
                        readOnly: true
                        selectByMouse: true
                        wrapMode: TextEdit.Wrap
                        color: Theme.text
                        font.family: Theme.mono
                        font.pixelSize: 13
                    }
                }
            }
            RowLayout {
                Layout.fillWidth: true
                spacing: 10
                Item { Layout.fillWidth: true }
                GhostButton {
                    text: sheet.t("Deny")
                    implicitHeight: 40
                    onClicked: ipc.resolveApproval(false)
                }
                AccentButton {
                    text: sheet.t("Approve")
                    implicitHeight: 40
                    implicitWidth: 120
                    fill: sheet.destructive ? "#e11d48" : Theme.accent
                    fillHover: sheet.destructive ? "#be123c" : Theme.accentHover
                    onClicked: ipc.resolveApproval(true)
                }
            }
        }
    }
}

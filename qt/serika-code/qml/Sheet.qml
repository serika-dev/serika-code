import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

// Centered modal dialog with a title bar and close button.
Popup {
    id: sheet
    property string title: ""
    property real maxWidth: 672
    property bool closable: true
    default property alias content: body.data

    parent: Overlay.overlay
    anchors.centerIn: Overlay.overlay
    modal: true
    dim: true
    padding: 0
    width: Math.min(maxWidth, (parent ? parent.width : 800) - 32)
    closePolicy: closable ? (Popup.CloseOnEscape | Popup.CloseOnPressOutside) : Popup.NoAutoClose

    Overlay.modal: Rectangle { color: "#b3000000" }
    background: Rectangle {
        radius: 16
        color: Theme.card
        border.width: 1
        border.color: Theme.border
    }
    enter: Transition {
        NumberAnimation { property: "opacity"; from: 0; to: 1; duration: 130; easing.type: Easing.OutCubic }
        NumberAnimation { property: "scale"; from: 0.97; to: 1; duration: 130; easing.type: Easing.OutCubic }
    }
    exit: Transition {
        NumberAnimation { property: "opacity"; to: 0; duration: 90 }
    }

    contentItem: ColumnLayout {
        spacing: 0
        RowLayout {
            Layout.fillWidth: true
            Layout.preferredHeight: 52
            Layout.leftMargin: 20
            Layout.rightMargin: 10
            Text {
                Layout.fillWidth: true
                text: sheet.title
                color: Theme.text
                font.family: Theme.sans
                font.pixelSize: 15
                font.weight: Font.DemiBold
            }
            IconButton {
                visible: sheet.closable
                icon: "x"
                iconSize: 18
                onClicked: sheet.close()
            }
        }
        Rectangle { Layout.fillWidth: true; Layout.preferredHeight: 1; color: Theme.borderSubtle }
        Item {
            id: body
            Layout.fillWidth: true
            Layout.preferredHeight: childrenRect.height
        }
    }
}

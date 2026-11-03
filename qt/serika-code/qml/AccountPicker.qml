import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

// Pick a git account. The closed control shows the chosen account the way the list does:
// picture, label and email. `accounts` are git accounts ({ id, label, name, email, avatar });
// "" stands for your global git settings.
Rectangle {
    id: picker
    property var accounts: []
    property string value: ""
    property string globalLabel: "Your global git settings"
    property string globalDetail: ""
    signal picked(string id)

    readonly property var current: {
        for (let i = 0; i < accounts.length; i++) if (accounts[i].id === value) return accounts[i]
        return null
    }

    implicitWidth: 320
    implicitHeight: 52
    radius: 12
    color: area.containsMouse || menu.opened ? Theme.active : Theme.hover
    border.width: 1
    border.color: menu.opened ? Theme.accent : Theme.borderSubtle
    Behavior on color { ColorAnimation { duration: Theme.motionFast } }
    Behavior on border.color { ColorAnimation { duration: Theme.motionFast } }

    component Row_: RowLayout {
        id: entry
        property var account: null
        property string fallback: ""
        property string detail: ""
        spacing: 11
        Item {
            implicitWidth: 30
            implicitHeight: 30
            Avatar {
                visible: entry.account !== null
                anchors.fill: parent
                size: 30
                source: entry.account ? (entry.account.avatar || "") : ""
                name: entry.account ? entry.account.name : ""
            }
            Rectangle {
                visible: entry.account === null
                anchors.fill: parent
                radius: 15
                color: Theme.active
                Icon { anchors.centerIn: parent; name: "globe"; size: 15; color: Theme.textSecondary }
            }
        }
        ColumnLayout {
            Layout.fillWidth: true
            spacing: 1
            Text {
                Layout.fillWidth: true
                text: entry.account ? entry.account.label : entry.fallback
                elide: Text.ElideRight
                color: Theme.text
                font.family: Theme.sans
                font.pixelSize: 13
                font.weight: Font.Medium
            }
            Text {
                Layout.fillWidth: true
                visible: text.length > 0
                text: entry.account ? entry.account.name + " <" + entry.account.email + ">" : entry.detail
                elide: Text.ElideRight
                color: Theme.textDim
                font.family: Theme.sans
                font.pixelSize: 11
            }
        }
    }

    RowLayout {
        anchors.fill: parent
        anchors.leftMargin: 11
        anchors.rightMargin: 12
        spacing: 8
        Row_ {
            Layout.fillWidth: true
            account: picker.current
            fallback: picker.globalLabel
            detail: picker.globalDetail
        }
        Icon {
            name: "chevron-down"
            size: 15
            color: Theme.textDim
            rotation: menu.opened ? 180 : 0
            Behavior on rotation { NumberAnimation { duration: Theme.motionBase; easing.type: Theme.easeEmphasis } }
        }
    }
    MouseArea {
        id: area
        anchors.fill: parent
        hoverEnabled: true
        cursorShape: Qt.PointingHandCursor
        onClicked: menu.opened ? menu.close() : menu.open()
    }

    Popover {
        id: menu
        y: picker.height + 6
        width: picker.width
        contentItem: ColumnLayout {
            spacing: 2
            Repeater {
                model: [null].concat(picker.accounts)
                delegate: Rectangle {
                    id: option
                    required property var modelData
                    readonly property string id_: modelData ? modelData.id : ""
                    readonly property bool chosen: id_ === picker.value
                    Layout.fillWidth: true
                    implicitHeight: 50
                    radius: 9
                    color: optionArea.containsMouse ? Theme.active : (chosen ? Theme.accentSoft : "transparent")
                    Behavior on color { ColorAnimation { duration: Theme.motionFast } }
                    RowLayout {
                        anchors.fill: parent
                        anchors.leftMargin: 9
                        anchors.rightMargin: 10
                        spacing: 8
                        Row_ {
                            Layout.fillWidth: true
                            account: option.modelData
                            fallback: picker.globalLabel
                            detail: picker.globalDetail
                        }
                        Icon { visible: option.chosen; name: "check"; size: 14; color: Theme.accentSecondary }
                    }
                    MouseArea {
                        id: optionArea
                        anchors.fill: parent
                        hoverEnabled: true
                        cursorShape: Qt.PointingHandCursor
                        onClicked: { menu.close(); if (!option.chosen) picker.picked(option.id_) }
                    }
                }
            }
        }
    }
}

import QtQuick
import QtQuick.Layouts

// A section label, a provider setup prompt, or a model entry in the picker.
Item {
    id: row
    property var entry: ({})
    readonly property bool isLabel: entry.kind === "label"
    readonly property bool isSetup: entry.kind === "setup"
    readonly property bool isModel: entry.kind === "model"
    readonly property var model: entry.model || ({})
    readonly property bool locked: entry.locked === true
    readonly property bool selected: isModel && model.id === ipc.selectedModel
    readonly property real multiplier: Number(model.usageMultiplier)
    signal chosen(string id)
    signal lockedChosen()
    signal setupChosen(string provider)

    readonly property var limit: entry.limit || null

    implicitHeight: isLabel ? 34 : (isSetup ? 44 : 48)

    // Section label
    RowLayout {
        visible: row.isLabel
        anchors.fill: parent
        anchors.leftMargin: 10
        anchors.rightMargin: 10
        anchors.topMargin: 8
        spacing: 7
        ProviderLogo {
            provider: row.entry.mark || "serika"
            size: 16
        }
        Text {
            Layout.maximumWidth: 210
            text: row.entry.text || ""
            elide: Text.ElideRight
            color: Theme.textDim
            font.family: Theme.sans
            font.pixelSize: 12
        }
        LimitBadge { limit: row.limit; size: 13 }
        Rectangle {
            visible: (row.entry.note || "").length > 0
            Layout.preferredWidth: noteText.implicitWidth + 12
            Layout.preferredHeight: 16
            radius: 8
            color: Qt.rgba(Theme.limit.r, Theme.limit.g, Theme.limit.b, 0.14)
            Text {
                id: noteText
                anchors.centerIn: parent
                text: row.entry.note || ""
                color: Theme.limit
                font.family: Theme.sans
                font.pixelSize: 10
                font.weight: Font.Medium
            }
        }
        Item { Layout.fillWidth: true }
        Text {
            visible: row.entry.upgrade === true
            text: Format.t(ipc.strings, "Upgrade")
            color: Theme.textSecondary
            font.family: Theme.sans
            font.pixelSize: 12
            MouseArea {
                anchors.fill: parent
                cursorShape: Qt.PointingHandCursor
                onClicked: row.lockedChosen()
            }
        }
    }

    // Provider that still needs installing or signing in
    Rectangle {
        visible: row.isSetup
        anchors.fill: parent
        anchors.leftMargin: 2
        anchors.rightMargin: 2
        radius: 8
        color: setupArea.containsMouse ? Theme.hover : "transparent"
        border.width: 1
        border.color: Theme.borderSubtle
        RowLayout {
            anchors.fill: parent
            anchors.leftMargin: 10
            anchors.rightMargin: 10
            spacing: 10
            Icon { name: "alert-circle"; size: 16; color: Theme.warning }
            Text {
                Layout.fillWidth: true
                text: row.entry.text || ""
                wrapMode: Text.WordWrap
                color: Theme.textSecondary
                font.family: Theme.sans
                font.pixelSize: 12
            }
            Text {
                text: Format.tr(ipc.strings, "SetUp", "Set up")
                color: Theme.accentSecondary
                font.family: Theme.sans
                font.pixelSize: 12
                font.weight: Font.Medium
            }
        }
        MouseArea {
            id: setupArea
            anchors.fill: parent
            hoverEnabled: true
            cursorShape: Qt.PointingHandCursor
            onClicked: row.setupChosen(row.entry.provider)
        }
    }

    // Model
    Rectangle {
        visible: row.isModel
        anchors.fill: parent
        radius: 8
        opacity: row.locked ? 0.55 : 1
        color: row.selected ? Theme.active : (area.containsMouse ? Theme.hover : "transparent")

        RowLayout {
            anchors.fill: parent
            anchors.leftMargin: 10
            anchors.rightMargin: 10
            spacing: 10

            ColumnLayout {
                Layout.fillWidth: true
                spacing: 1
                RowLayout {
                    spacing: 8
                    Text {
                        Layout.maximumWidth: 190
                        text: row.model.name || row.model.id || ""
                        elide: Text.ElideRight
                        color: Theme.text
                        font.family: Theme.sans
                        font.pixelSize: 14
                    }
                    RowLayout {
                        visible: row.model.premium === true
                        spacing: 3
                        Icon { visible: row.locked; name: "lock"; size: 11; color: Theme.textDim }
                        Text {
                            text: "Serika+"
                            color: Theme.textDim
                            font.family: Theme.sans
                            font.pixelSize: 11
                        }
                    }
                    LimitBadge { limit: row.limit; size: 13 }
                    Text {
                        visible: row.multiplier > 1
                        text: (row.multiplier % 1 === 0 ? row.multiplier.toFixed(0) : row.multiplier.toFixed(1)) + "x"
                        color: Theme.textDim
                        font.family: Theme.sans
                        font.pixelSize: 11
                    }
                }
                Text {
                    Layout.fillWidth: true
                    visible: text.length > 0
                    text: row.model.description || row.model.provider || ""
                    elide: Text.ElideRight
                    color: Theme.textDim
                    font.family: Theme.sans
                    font.pixelSize: 12
                }
            }
            Icon {
                visible: row.selected
                name: "check"
                size: 16
                color: Theme.text
            }
        }
        MouseArea {
            id: area
            anchors.fill: parent
            hoverEnabled: true
            cursorShape: Qt.PointingHandCursor
            onClicked: row.locked ? row.lockedChosen() : row.chosen(row.model.id)
        }
    }
}

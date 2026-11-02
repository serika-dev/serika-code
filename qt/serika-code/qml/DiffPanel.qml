import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

// The file changes the agent made in this chat. Lives inside the Inspector.
Item {
    id: panel
    property int selected: 0
    readonly property var dict: ipc.strings

    function stats(diff) {
        var add = 0, del = 0
        var lines = (diff || "").split("\n")
        for (var i = 0; i < lines.length; ++i) {
            var c = lines[i].charAt(0)
            if (c === "+" && lines[i].indexOf("+++") !== 0) add++
            else if (c === "-" && lines[i].indexOf("---") !== 0) del++
        }
        return { add: add, del: del }
    }
    function fileName(path) { return path.split("/").pop().split("\\").pop() }

    Connections {
        target: ipc
        function onDiffsChanged() { if (panel.selected >= ipc.diffs.length) panel.selected = Math.max(0, ipc.diffs.length - 1) }
        function onDiffFocus(path) {
            for (var i = 0; i < ipc.diffs.length; ++i)
                if (ipc.diffs[i].path === path) { panel.selected = i; return }
        }
    }

    ColumnLayout {
        anchors.fill: parent
        spacing: 0

        Flow {
            Layout.fillWidth: true
            Layout.leftMargin: 12
            Layout.rightMargin: 12
            Layout.topMargin: 10
            spacing: 6
            visible: ipc.diffs.length > 0
            Repeater {
                model: ipc.diffs
                delegate: Rectangle {
                    required property var modelData
                    required property int index
                    readonly property var st: panel.stats(modelData.diff)
                    height: 28
                    width: chipRow.implicitWidth + 22
                    radius: 14
                    color: index === panel.selected ? Theme.accentSoft : "transparent"
                    border.width: 1
                    border.color: index === panel.selected ? Theme.accent : Theme.border
                    Row {
                        id: chipRow
                        anchors.centerIn: parent
                        spacing: 6
                        Text {
                            text: panel.fileName(modelData.path)
                            color: index === panel.selected ? Theme.accentSecondary : Theme.textSecondary
                            font.family: Theme.sans
                            font.pixelSize: 12
                        }
                        Text {
                            visible: st.add > 0
                            text: "+" + st.add
                            color: Theme.success
                            font.family: Theme.sans
                            font.pixelSize: 11
                        }
                        Text {
                            visible: st.del > 0
                            text: "−" + st.del
                            color: Theme.danger
                            font.family: Theme.sans
                            font.pixelSize: 11
                        }
                    }
                    MouseArea { anchors.fill: parent; cursorShape: Qt.PointingHandCursor; onClicked: panel.selected = index }
                }
            }
        }

        Text {
            visible: ipc.diffs.length > panel.selected
            Layout.fillWidth: true
            Layout.leftMargin: 14
            Layout.rightMargin: 14
            Layout.topMargin: 8
            elide: Text.ElideMiddle
            text: ipc.diffs.length > panel.selected ? ipc.diffs[panel.selected].path : ""
            color: Theme.textDim
            font.family: Theme.mono
            font.pixelSize: 11
        }

        ScrollView {
            id: scroller
            Layout.fillWidth: true
            Layout.fillHeight: true
            Layout.margins: 12
            visible: ipc.diffs.length > 0
            clip: true
            contentWidth: diffText.implicitWidth
            TextEdit {
                id: diffText
                readOnly: true
                selectByMouse: true
                selectionColor: Theme.accent
                textFormat: TextEdit.RichText
                wrapMode: TextEdit.NoWrap
                text: ipc.diffs.length > panel.selected
                      ? Format.diffHtml(ipc.diffs[panel.selected].diff, Theme.light, Theme.mono)
                      : ""
            }
        }

        ColumnLayout {
            visible: ipc.diffs.length === 0
            Layout.fillWidth: true
            Layout.fillHeight: true
            Layout.margins: 24
            spacing: 10
            Item { Layout.fillHeight: true }
            Icon { Layout.alignment: Qt.AlignHCenter; name: "git-branch"; size: 26; color: Theme.textDim }
            Text {
                Layout.fillWidth: true
                horizontalAlignment: Text.AlignHCenter
                wrapMode: Text.WordWrap
                text: Format.t(panel.dict, "NoDiffs")
                color: Theme.textDim
                font.family: Theme.sans
                font.pixelSize: 14
            }
            Item { Layout.fillHeight: true }
        }
    }
}

import QtQuick
import QtQuick.Layouts

// Prose and fenced code from the model. Prose is selectable Markdown; code gets a card.
ColumnLayout {
    id: view
    property string text: ""
    spacing: 12

    Repeater {
        model: Format.splitBlocks(view.text)
        delegate: Loader {
            id: block
            required property var modelData
            Layout.fillWidth: true
            sourceComponent: modelData.code ? codeBlock : prose
            onLoaded: item.width = Qt.binding(function () { return block.width })

            Component {
                id: prose
                TextEdit {
                    text: block.modelData.text
                    readOnly: true
                    selectByMouse: true
                    selectionColor: Theme.accent
                    wrapMode: TextEdit.Wrap
                    textFormat: TextEdit.MarkdownText
                    color: Theme.text
                    font.family: Theme.sans
                    font.pixelSize: Theme.chatPx
                    onLinkActivated: (link) => Qt.openUrlExternally(link)
                    HoverHandler {
                        cursorShape: parent.hoveredLink.length > 0 ? Qt.PointingHandCursor : Qt.IBeamCursor
                    }
                }
            }
            Component {
                id: codeBlock
                CodeBlock {
                    language: block.modelData.lang
                    code: block.modelData.text
                }
            }
        }
    }
}

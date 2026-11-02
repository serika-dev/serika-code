import QtQuick

// Brand tile of a model provider: Claude (Anthropic orange), Codex (OpenAI), or Serika.
Rectangle {
    id: logo
    property string provider: "serika"
    property real size: 18
    readonly property bool claude: provider === "claude" || provider === "claude-code"
    readonly property bool codex: provider === "codex" || provider === "openai"

    width: size
    height: size
    implicitWidth: size
    implicitHeight: size
    radius: Math.round(size * 0.28)
    color: claude ? "#d97757" : (codex ? "#0d0d0d" : Theme.accent)
    border.width: codex ? 1 : 0
    border.color: "#26ffffff"

    Icon {
        anchors.centerIn: parent
        size: Math.round(logo.size * (logo.claude || logo.codex ? 0.64 : 0.74))
        name: logo.claude ? "logo-claude" : (logo.codex ? "logo-openai" : "wave")
        color: "white"
    }
}

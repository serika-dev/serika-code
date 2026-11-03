pragma Singleton
import QtQuick

// Design tokens. Serika Dark is the canonical look (from serika-dev/chat-web); every other
// preset is a full palette, so switching themes recolors the whole app, not just the accent.
QtObject {
    id: theme

    // Set from Main.qml, which reads the saved interface preferences.
    property string preset: "serika-dark"
    property string accentOverride: ""
    property bool systemDark: true
    property string chatSize: "medium"
    property string density: "comfortable"
    // Width of the chat column: narrow, default, wide, or full.
    property string chatWidth: "default"

    function mk(id, name, mode, bg, card, elevated, raised, input, composer, code, text, secondary, dim, accent) {
        return { id: id, name: name, mode: mode, bg: bg, card: card, elevated: elevated, raised: raised,
                 input: input, composer: composer, code: code, text: text, secondary: secondary, dim: dim, accent: accent }
    }

    readonly property var presets: [
        mk("serika-dark", "Serika Dark", "dark", "#0f0f10", "#0b0b0c", "#18181b", "#1c1c1f", "#171719", "#141416", "#101012", "#ececee", "#a1a1aa", "#71717a", "#8b5cf6"),
        mk("serika-light", "Serika Light", "light", "#f7f7f8", "#ffffff", "#f4f4f5", "#ffffff", "#f4f4f5", "#ffffff", "#f4f4f5", "#18181b", "#52525b", "#8a8a93", "#8b5cf6"),
        mk("midnight", "Midnight", "dark", "#0a0e1a", "#080b15", "#131a2b", "#172036", "#111829", "#0f1626", "#0b1120", "#e6ebf5", "#98a4bf", "#66728e", "#3b82f6"),
        mk("graphite", "Graphite", "dark", "#161616", "#111111", "#202020", "#262626", "#1d1d1d", "#1b1b1b", "#141414", "#eaeaea", "#a3a3a3", "#737373", "#f97316"),
        mk("github-dark", "GitHub Dark", "dark", "#0d1117", "#010409", "#161b22", "#21262d", "#151b23", "#161b22", "#010409", "#e6edf3", "#9198a1", "#6e7681", "#2f81f7"),
        mk("github-light", "GitHub Light", "light", "#ffffff", "#f6f8fa", "#f6f8fa", "#ffffff", "#f6f8fa", "#ffffff", "#f6f8fa", "#1f2328", "#59636e", "#818b98", "#0969da"),
        mk("tokyo-night", "Tokyo Night", "dark", "#1a1b26", "#16161e", "#24283b", "#292e42", "#20212f", "#1f2335", "#15161e", "#c0caf5", "#9aa5ce", "#565f89", "#7aa2f7"),
        mk("catppuccin-mocha", "Catppuccin Mocha", "dark", "#1e1e2e", "#181825", "#313244", "#363a4f", "#2a2b3c", "#28283a", "#11111b", "#cdd6f4", "#a6adc8", "#7f849c", "#8c6ef0"),
        mk("catppuccin-latte", "Catppuccin Latte", "light", "#eff1f5", "#e6e9ef", "#e6e9ef", "#ffffff", "#e6e9ef", "#ffffff", "#e6e9ef", "#4c4f69", "#6c6f85", "#8c8fa1", "#8839ef"),
        mk("rose-pine", "Rosé Pine", "dark", "#191724", "#16141f", "#26233a", "#2a2740", "#1f1d2e", "#1f1d2e", "#12101b", "#e0def4", "#908caa", "#6e6a86", "#eb6f92"),
        mk("rose-pine-dawn", "Rosé Pine Dawn", "light", "#faf4ed", "#fffaf3", "#f2e9e1", "#fffaf3", "#f2e9e1", "#fffaf3", "#f2e9e1", "#575279", "#797593", "#9893a5", "#b4637a"),
        mk("nord", "Nord", "dark", "#2e3440", "#2b303b", "#3b4252", "#434c5e", "#353c4a", "#3b4252", "#292e39", "#eceff4", "#d8dee9", "#8f9bb3", "#5e81ac"),
        mk("dracula", "Dracula", "dark", "#282a36", "#21222c", "#343746", "#3a3d4e", "#303341", "#2f3240", "#1e1f29", "#f8f8f2", "#c4c7d6", "#7f849c", "#9d6cf2"),
        mk("one-dark", "One Dark", "dark", "#282c34", "#21252b", "#2c313a", "#333842", "#2c313a", "#2c313a", "#1e2227", "#abb2bf", "#9da5b4", "#636d83", "#4d78cc"),
        mk("gruvbox", "Gruvbox", "dark", "#282828", "#1d2021", "#3c3836", "#504945", "#32302f", "#32302f", "#1d2021", "#ebdbb2", "#bdae93", "#928374", "#d65d0e"),
        mk("solarized-dark", "Solarized Dark", "dark", "#002b36", "#00212b", "#073642", "#0b3f4c", "#04313c", "#073642", "#001f27", "#eee8d5", "#93a1a1", "#657b83", "#268bd2"),
        mk("solarized-light", "Solarized Light", "light", "#fdf6e3", "#fffbee", "#eee8d5", "#fffbee", "#eee8d5", "#fffbee", "#eee8d5", "#073642", "#586e75", "#93a1a1", "#268bd2"),
        mk("forest", "Forest", "dark", "#0f1512", "#0b110e", "#18211c", "#1d2822", "#151d19", "#141b17", "#0c120f", "#e3ede7", "#9db3a6", "#6a8074", "#10b981")
    ]

    function find(id) {
        for (var i = 0; i < presets.length; ++i)
            if (presets[i].id === id) return presets[i]
        return presets[0]
    }

    readonly property string resolvedId: preset === "system" ? (systemDark ? "serika-dark" : "serika-light") : preset
    readonly property var current: find(resolvedId)
    readonly property bool light: current.mode === "light"

    readonly property color bg: current.bg
    readonly property color card: current.card
    readonly property color elevated: current.elevated
    readonly property color raised: current.raised
    readonly property color input: current.input
    readonly property color composer: current.composer
    readonly property color code: current.code

    readonly property color hover: light ? "#0b000000" : "#0dffffff"
    readonly property color active: light ? "#14000000" : "#12ffffff"
    readonly property color border: light ? "#1f000000" : "#14ffffff"
    readonly property color borderSubtle: light ? "#12000000" : "#0effffff"

    readonly property color text: current.text
    readonly property color textSecondary: current.secondary
    readonly property color textDim: current.dim

    readonly property color accent: accentOverride.length > 0 ? accentOverride : current.accent
    readonly property color accentHover: Qt.darker(accent, 1.15)
    readonly property color accentSecondary: light ? Qt.darker(accent, 1.1) : Qt.lighter(accent, 1.28)
    readonly property color accentSoft: Qt.rgba(accent.r, accent.g, accent.b, 0.16)
    readonly property color danger: light ? "#e11d48" : "#fda4af"
    readonly property color dangerSoft: light ? "#1ae11d48" : "#1af43f5e"
    readonly property color success: light ? "#059669" : "#34d399"
    readonly property color successSoft: light ? "#1a059669" : "#1a34d399"
    readonly property color warning: light ? "#b45309" : "#fbbf24"
    readonly property color warningSoft: light ? "#1ab45309" : "#1afbbf24"
    // Orange used only for "a provider limit is reached".
    readonly property color limit: light ? "#c2410c" : "#fb923c"
    readonly property color info: light ? "#0369a1" : "#38bdf8"
    readonly property color discord: "#5865f2"

    // Interface preferences
    readonly property int chatPx: chatSize === "small" ? 14 : (chatSize === "large" ? 17 : 15)
    readonly property real laneMax: chatWidth === "narrow" ? 640 : (chatWidth === "wide" ? 980 : (chatWidth === "full" ? 100000 : 768))

    // Motion: one set of timings and curves so everything moves the same way.
    property bool reduceMotion: false
    readonly property int motionFast: reduceMotion ? 0 : 120
    readonly property int motionBase: reduceMotion ? 0 : 220
    readonly property int motionSlow: reduceMotion ? 0 : 360
    readonly property int easeOut: Easing.OutCubic
    readonly property int easeEmphasis: Easing.OutQuint
    readonly property int easeSpring: Easing.OutBack
    readonly property int codePx: chatPx - 2
    readonly property real gap: density === "compact" ? 0.7 : 1

    readonly property FontLoader interFont: FontLoader { source: Qt.resolvedUrl("fonts/Inter.ttf") }
    readonly property FontLoader outfitFont: FontLoader { source: Qt.resolvedUrl("fonts/Outfit.ttf") }
    readonly property string sans: interFont.status === FontLoader.Ready ? interFont.name : "Segoe UI"
    readonly property string brand: outfitFont.status === FontLoader.Ready ? outfitFont.name : "Segoe UI"
    readonly property string mono: "Cascadia Mono"
}

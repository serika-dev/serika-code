import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

ColumnLayout {
    id: section
    spacing: 24
    readonly property var dict: ipc.strings
    function t(key, fallback) { return Format.tr(dict, key, fallback) }

    readonly property string currentPreset: ipc.ui.themePreset ? ipc.ui.themePreset : (ipc.theme === "light" ? "serika-light" : "serika-dark")
    readonly property string mode: currentPreset === "system" ? "system" : (Theme.find(currentPreset).mode)
    readonly property var swatches: ["#8b5cf6", "#6366f1", "#3b82f6", "#0ea5e9", "#14b8a6", "#22c55e", "#eab308", "#f97316", "#f43f5e", "#ec4899"]
    readonly property var gallery: Theme.presets

    function pick(id) {
        ipc.setUi("themePreset", id)
        ipc.setTheme(id === "system" ? "dark" : Theme.find(id).mode)
    }

    SettingsCard {
        title: section.t("ThemeSection", "Theme")
        description: section.t("ThemeSectionHelp", "Pick a full color theme. Match your system, or choose one you like.")
        SettingRow {
            heading: section.t("ColorMode", "Color mode")
            help: section.t("ColorModeHelp", "System follows your Windows light or dark setting.")
            Segmented {
                options: [
                    { value: "system", label: section.t("ThemeSystem", "System"), icon: "monitor" },
                    { value: "light", label: section.t("ThemeLight", "Light"), icon: "sun" },
                    { value: "dark", label: section.t("ThemeDark", "Dark"), icon: "moon" }
                ]
                value: section.mode
                onPicked: (v) => section.pick(v === "system" ? "system" : (v === "light" ? "serika-light" : "serika-dark"))
            }
        }
        Item {
            Layout.fillWidth: true
            implicitHeight: grid.implicitHeight + 32
            Rectangle { anchors.top: parent.top; anchors.left: parent.left; anchors.right: parent.right; anchors.leftMargin: 16; anchors.rightMargin: 16; height: 1; color: Theme.borderSubtle }
            Flow {
                id: grid
                anchors.left: parent.left
                anchors.right: parent.right
                anchors.top: parent.top
                anchors.margins: 16
                anchors.topMargin: 18
                spacing: 12
                Repeater {
                    model: section.gallery
                    delegate: Item {
                        id: card
                        required property var modelData
                        readonly property bool selected: section.currentPreset === modelData.id
                        width: 156
                        height: 118

                        Rectangle {
                            anchors.fill: parent
                            radius: 12
                            color: "transparent"
                            border.width: card.selected ? 2 : 1
                            border.color: card.selected ? Theme.accent : (cardArea.containsMouse ? Theme.border : Theme.borderSubtle)
                        }
                        // Miniature of the app in this theme
                        Rectangle {
                            id: mini
                            x: 6
                            y: 6
                            width: parent.width - 12
                            height: 78
                            radius: 8
                            color: card.modelData.bg
                            clip: true
                            Rectangle { x: 0; y: 0; width: 34; height: parent.height; color: card.modelData.card }
                            Repeater {
                                model: 3
                                delegate: Rectangle {
                                    required property int index
                                    x: 7
                                    y: 12 + index * 12
                                    width: 20 - index * 3
                                    height: 4
                                    radius: 2
                                    color: card.modelData.dim
                                    opacity: index === 0 ? 0.9 : 0.5
                                }
                            }
                            Rectangle { x: 44; y: 10; width: 46; height: 14; radius: 7; color: card.modelData.raised }
                            Rectangle { x: 44; y: 32; width: 80; height: 4; radius: 2; color: card.modelData.text; opacity: 0.8 }
                            Rectangle { x: 44; y: 40; width: 62; height: 4; radius: 2; color: card.modelData.secondary; opacity: 0.7 }
                            Rectangle { x: 44; y: 48; width: 70; height: 4; radius: 2; color: card.modelData.secondary; opacity: 0.5 }
                            Rectangle { x: 44; y: parent.height - 18; width: parent.width - 54; height: 12; radius: 6; color: card.modelData.composer; border.width: 1; border.color: Qt.rgba(1, 1, 1, 0.08) }
                            Rectangle { x: parent.width - 20; y: parent.height - 16; width: 8; height: 8; radius: 4; color: ipc.ui.accent ? ipc.ui.accent : card.modelData.accent }
                        }
                        RowLayout {
                            x: 10
                            y: 90
                            width: parent.width - 20
                            spacing: 6
                            Text {
                                Layout.fillWidth: true
                                text: card.modelData.name
                                elide: Text.ElideRight
                                color: Theme.text
                                font.family: Theme.sans
                                font.pixelSize: 12
                                font.weight: card.selected ? Font.DemiBold : Font.Normal
                            }
                            Icon { visible: card.selected; name: "check"; size: 14; color: Theme.accentSecondary }
                        }
                        MouseArea {
                            id: cardArea
                            anchors.fill: parent
                            hoverEnabled: true
                            cursorShape: Qt.PointingHandCursor
                            onClicked: section.pick(card.modelData.id)
                        }
                    }
                }
            }
        }
    }

    SettingsCard {
        title: section.t("AccentSection", "Accent color")
        description: section.t("AccentSectionHelp", "Buttons, highlights, and links. Every theme has its own default.")
        SettingRow {
            heading: section.t("Accent", "Accent")
            stacked: true
            Flow {
                width: parent.width
                spacing: 10
                // Theme default
                Rectangle {
                    width: 30
                    height: 30
                    radius: 15
                    color: Theme.find(section.currentPreset === "system" ? "serika-dark" : section.currentPreset).accent
                    border.width: !ipc.ui.accent ? 2 : 1
                    border.color: !ipc.ui.accent ? Theme.text : Theme.border
                    Icon { anchors.centerIn: parent; visible: !ipc.ui.accent; name: "check"; size: 14; color: "white" }
                    MouseArea { anchors.fill: parent; cursorShape: Qt.PointingHandCursor; onClicked: ipc.setUi("accent", null) }
                    ToolTip.visible: defaultHover.hovered
                    ToolTip.text: section.t("ThemeDefault", "Theme default")
                    HoverHandler { id: defaultHover }
                }
                Repeater {
                    model: section.swatches
                    delegate: Rectangle {
                        required property string modelData
                        readonly property bool selected: (ipc.ui.accent || "").toLowerCase() === modelData
                        width: 30
                        height: 30
                        radius: 15
                        color: modelData
                        border.width: selected ? 2 : 0
                        border.color: Theme.text
                        Icon { anchors.centerIn: parent; visible: parent.selected; name: "check"; size: 14; color: "white" }
                        MouseArea { anchors.fill: parent; cursorShape: Qt.PointingHandCursor; onClicked: ipc.setUi("accent", modelData) }
                    }
                }
                PathField {
                    width: 120
                    height: 30
                    mono: true
                    placeholder: "#8b5cf6"
                    value: ipc.ui.accent ? ipc.ui.accent : ""
                    onCommitted: (text) => {
                        const hex = text.trim()
                        if (/^#([0-9a-fA-F]{6})$/.test(hex)) ipc.setUi("accent", hex.toLowerCase())
                        else if (hex.length === 0) ipc.setUi("accent", null)
                    }
                }
            }
        }
    }

    SettingsCard {
        title: section.t("ChatLook", "Chat")
        SettingRow {
            heading: section.t("TextSize", "Message text size")
            help: section.t("TextSizeHelp", "Applies to messages, the message box, and code.")
            Segmented {
                options: [
                    { value: "small", label: section.t("SizeSmall", "Small") },
                    { value: "medium", label: section.t("SizeMedium", "Medium") },
                    { value: "large", label: section.t("SizeLarge", "Large") }
                ]
                value: ipc.ui.chatSize ? ipc.ui.chatSize : "medium"
                onPicked: (v) => ipc.setUi("chatSize", v)
            }
        }
        SettingRow {
            heading: section.t("Density", "Spacing")
            help: section.t("DensityHelp", "Compact fits more of a long chat on screen.")
            Segmented {
                options: [
                    { value: "comfortable", label: section.t("DensityComfortable", "Comfortable") },
                    { value: "compact", label: section.t("DensityCompact", "Compact") }
                ]
                value: ipc.ui.density ? ipc.ui.density : "comfortable"
                onPicked: (v) => ipc.setUi("density", v)
            }
        }
        SettingRow {
            heading: section.t("ReasoningOpen", "Expand reasoning")
            help: section.t("ReasoningOpenHelp", "Show the thought process open instead of collapsed.")
            Toggle {
                checked: ipc.ui.reasoningOpen === true
                onToggled: (v) => ipc.setUi("reasoningOpen", v)
            }
        }
        SettingRow {
            heading: section.t("ReduceMotion", "Reduce motion")
            help: section.t("ReduceMotionHelp", "Turn off animations and transitions.")
            Toggle {
                checked: ipc.ui.reduceMotion === true
                onToggled: (v) => ipc.setUi("reduceMotion", v)
            }
        }
        SettingRow {
            heading: section.t("ToolsExpanded", "Expand tool output")
            help: section.t("ToolsExpandedHelp", "Show what each command printed without clicking.")
            Toggle {
                checked: ipc.ui.toolsExpanded === true
                onToggled: (v) => ipc.setUi("toolsExpanded", v)
            }
        }
    }

    // Which dock each panel lives in, how big the docks are, and how wide the chat is.
    SettingsCard {
        title: section.t("LayoutTitle", "Layout")
        readonly property var paneIds: ["changes", "git", "browser", "agents", "terminal"]
        function tabsOf(key, fallback) {
            const raw = ipc.ui[key]
            if (raw === undefined || raw === null) return fallback
            return String(raw).split(",").filter(function (t) { return paneIds.indexOf(t) >= 0 })
        }
        readonly property var rightTabs: tabsOf("rightTabs", tabsOf("dockTabs", ["changes", "browser"]))
        readonly property var bottomTabs: tabsOf("bottomTabs", ["terminal"]).filter(function (t) { return rightTabs.indexOf(t) < 0 })
        function place(id, where) {
            const right = rightTabs.filter(function (t) { return t !== id })
            const bottom = bottomTabs.filter(function (t) { return t !== id })
            if (where === "right") right.push(id)
            else if (where === "bottom") bottom.push(id)
            ipc.setUi("rightTabs", right.join(","))
            ipc.setUi("bottomTabs", bottom.join(","))
        }
        id: layoutCard
        Repeater {
            model: [
                { id: "changes", label: section.t("Changes", "Changes"), icon: "git-branch" },
                { id: "git", label: section.t("Git", "Git"), icon: "git-commit" },
                { id: "browser", label: section.t("Browser", "Browser"), icon: "globe" },
                { id: "agents", label: section.t("Subagents", "Sub-agents"), icon: "users" },
                { id: "terminal", label: section.t("Terminal", "Terminal"), icon: "terminal" }
            ]
            delegate: SettingRow {
                required property var modelData
                heading: modelData.label
                help: section.t("PanePlaceHelp", "Drag its tab between docks, or pick a dock here.")
                Segmented {
                    options: [
                        { value: "right", label: section.t("DockRightShort", "Right"), icon: "panel-right" },
                        { value: "bottom", label: section.t("DockBottomShort", "Bottom"), icon: "panel-bottom" },
                        { value: "none", label: section.t("DockHidden", "Hidden") }
                    ]
                    value: layoutCard.rightTabs.indexOf(modelData.id) >= 0 ? "right" : (layoutCard.bottomTabs.indexOf(modelData.id) >= 0 ? "bottom" : "none")
                    onPicked: (v) => layoutCard.place(modelData.id, v)
                }
            }
        }
        SettingRow {
            heading: section.t("RightDockWidth", "Right dock width")
            help: section.t("RightDockWidthHelp", "Or drag its left edge. Double-click the edge to reset.")
            RowLayout {
                spacing: 10
                Slider {
                    id: widthSlider
                    Layout.preferredWidth: 180
                    from: 300
                    to: 1100
                    stepSize: 10
                    value: Number(ipc.ui.rightWidth || ipc.ui.dockWidth) > 0 ? Number(ipc.ui.rightWidth || ipc.ui.dockWidth) : 480
                    onMoved: ipc.setUi("rightWidth", Math.round(value))
                }
                Text { text: Math.round(widthSlider.value) + " px"; color: Theme.textSecondary; font.family: Theme.sans; font.pixelSize: 12; Layout.preferredWidth: 52 }
            }
        }
        SettingRow {
            heading: section.t("BottomDockHeight", "Bottom dock height")
            help: section.t("BottomDockHeightHelp", "Or drag its top edge. Double-click the edge to reset.")
            RowLayout {
                spacing: 10
                Slider {
                    id: heightSlider
                    Layout.preferredWidth: 180
                    from: 140
                    to: 900
                    stepSize: 10
                    value: Number(ipc.ui.bottomHeight) > 0 ? Number(ipc.ui.bottomHeight) : 280
                    onMoved: ipc.setUi("bottomHeight", Math.round(value))
                }
                Text { text: Math.round(heightSlider.value) + " px"; color: Theme.textSecondary; font.family: Theme.sans; font.pixelSize: 12; Layout.preferredWidth: 52 }
            }
        }        SettingRow {
            heading: section.t("ChatWidth", "Chat width")
            help: section.t("ChatWidthHelp", "How wide messages and the message box can grow.")
            Segmented {
                options: [
                    { value: "narrow", label: section.t("WidthNarrow", "Narrow") },
                    { value: "default", label: section.t("WidthDefault", "Default") },
                    { value: "wide", label: section.t("WidthWide", "Wide") },
                    { value: "full", label: section.t("WidthFull", "Full") }
                ]
                value: ipc.ui.chatWidth ? ipc.ui.chatWidth : "default"
                onPicked: (v) => ipc.setUi("chatWidth", v)
            }
        }
        SettingRow {
            heading: section.t("PanelSizes", "Panel sizes")
            help: section.t("PanelSizesHelp", "Drag the edge of the sidebar or a dock to resize. Double-click an edge to reset it.")
            ChipButton {
                text: section.t("ResetSizes", "Reset sizes")
                iconName: "rotate-ccw"
                onClicked: { ipc.setUi("sidebarWidth", 0); ipc.setUi("dockWidth", 0); ipc.setUi("rightWidth", 0); ipc.setUi("bottomHeight", 0) }
            }
        }
    }

    // Live sample so changes are visible right here.
    SettingsCard {
        title: section.t("Preview", "Preview")
        Item {
            Layout.fillWidth: true
            implicitHeight: sample.implicitHeight + 36
            ColumnLayout {
                id: sample
                anchors.left: parent.left
                anchors.right: parent.right
                anchors.verticalCenter: parent.verticalCenter
                anchors.leftMargin: 18
                anchors.rightMargin: 18
                spacing: 12 * Theme.gap
                Rectangle {
                    Layout.alignment: Qt.AlignRight
                    implicitWidth: userSample.implicitWidth + 32
                    implicitHeight: userSample.implicitHeight + 22
                    radius: 16
                    color: Theme.raised
                    border.width: Theme.light ? 1 : 0
                    border.color: Theme.border
                    Text {
                        id: userSample
                        anchors.centerIn: parent
                        text: section.t("PreviewUser", "Can you make the sidebar remember its width?")
                        color: Theme.text
                        font.family: Theme.sans
                        font.pixelSize: Theme.chatPx
                    }
                }
                Text {
                    Layout.fillWidth: true
                    wrapMode: Text.WordWrap
                    text: section.t("PreviewReply", "Sure. I'll store the width in your settings and restore it on launch.")
                    color: Theme.text
                    font.family: Theme.sans
                    font.pixelSize: Theme.chatPx
                }
                Rectangle {
                    Layout.fillWidth: true
                    implicitHeight: 40
                    radius: 10
                    color: Theme.card
                    border.width: 1
                    border.color: Theme.borderSubtle
                    Row {
                        anchors.verticalCenter: parent.verticalCenter
                        anchors.left: parent.left
                        anchors.leftMargin: 12
                        spacing: 10
                        Rectangle {
                            width: 24
                            height: 24
                            radius: 7
                            color: Format.tintSoft("success")
                            Icon { anchors.centerIn: parent; name: "edit-2"; size: 14; color: Theme.success }
                        }
                        Text { anchors.verticalCenter: parent.verticalCenter; text: "Edited"; color: Theme.text; font.family: Theme.sans; font.pixelSize: 13; font.weight: Font.DemiBold }
                        Text { anchors.verticalCenter: parent.verticalCenter; text: "src/sidebar.rs"; color: Theme.textDim; font.family: Theme.mono; font.pixelSize: 12 }
                    }
                }
            }
        }
    }
}

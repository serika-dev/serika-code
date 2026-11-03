import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

// Full-page settings with a section list on the left, like T3 Code.
Rectangle {
    id: page
    property string tab: "general"
    signal closeRequested()
    readonly property var dict: ipc.strings
    function t(key, fallback) { return Format.tr(dict, key, fallback) }

    color: Theme.bg

    readonly property var groups: [
        { title: t("SettingsWorkspace", "Workspace"), items: [
            { id: "general", label: t("General", "General"), icon: "sliders", sub: t("GeneralSub", "Language, model, sending") },
            { id: "providers", label: t("Providers", "Providers"), icon: "cpu", sub: t("ProvidersSub", "Serika, Claude Code, Codex") },
            { id: "agent", label: t("Agent", "Agent"), icon: "zap", sub: t("AgentSub", "Permissions and limits") },
            { id: "browser", label: t("Browser", "Browser"), icon: "globe", sub: t("BrowserSub", "Let Serika browse the web") },
            { id: "mcp", label: t("McpNav", "MCP servers"), icon: "server", sub: t("McpSub", "Tools from Claude Code, Codex and your own servers") },
            { id: "skills", label: t("SkillsNav", "Skills"), icon: "package", sub: t("SkillsSub", "Reusable instructions Serika can load") }
        ] },
        { title: t("SettingsInterface", "Interface"), items: [
            { id: "appearance", label: t("Appearance", "Appearance"), icon: "droplet", sub: t("AppearanceSub", "Themes, accent, text") },
            { id: "keys", label: t("Keybindings", "Keybindings"), icon: "keyboard", sub: t("KeysSub", "Keyboard shortcuts") }
        ] },
        { title: t("SettingsManage", "Manage"), items: [
            { id: "data", label: t("DataStorage", "Data & storage"), icon: "database", sub: t("DataSub", "Projects, history, files") },
            { id: "account", label: t("Account", "Account"), icon: "user", sub: t("AccountSub", "Plan and usage") },
            { id: "about", label: t("About", "About"), icon: "info", sub: t("AboutSub", "Version and updates") }
        ] }
    ]
    readonly property var current: {
        for (var g = 0; g < groups.length; ++g)
            for (var i = 0; i < groups[g].items.length; ++i)
                if (groups[g].items[i].id === tab) return groups[g].items[i]
        return groups[0].items[0]
    }

    Shortcut { sequence: "Escape"; enabled: page.visible; onActivated: page.closeRequested() }
    onTabChanged: flick.contentY = 0

    RowLayout {
        anchors.fill: parent
        spacing: 0

        // Section list
        Rectangle {
            Layout.preferredWidth: 264
            Layout.fillHeight: true
            color: Theme.card
            Rectangle { anchors.right: parent.right; width: 1; height: parent.height; color: Theme.borderSubtle }

            ColumnLayout {
                anchors.fill: parent
                anchors.margins: 10
                spacing: 2

                Rectangle {
                    Layout.fillWidth: true
                    Layout.topMargin: 4
                    implicitHeight: 38
                    radius: 8
                    color: backArea.containsMouse ? Theme.hover : "transparent"
                    RowLayout {
                        anchors.fill: parent
                        anchors.leftMargin: 10
                        anchors.rightMargin: 10
                        spacing: 10
                        Icon { name: "arrow-left"; size: 17; color: Theme.textSecondary }
                        Text {
                            Layout.fillWidth: true
                            text: page.t("BackToApp", "Back to app")
                            color: Theme.text
                            font.family: Theme.sans
                            font.pixelSize: 14
                        }
                        Kbd { label: "Esc" }
                    }
                    MouseArea { id: backArea; anchors.fill: parent; hoverEnabled: true; cursorShape: Qt.PointingHandCursor; onClicked: page.closeRequested() }
                }

                Text {
                    Layout.topMargin: 14
                    Layout.leftMargin: 10
                    text: page.t("Settings", "Settings")
                    color: Theme.text
                    font.family: Theme.brand
                    font.pixelSize: 22
                    font.weight: Font.DemiBold
                }

                Flickable {
                    id: navFlick
                    SmoothWheel { flickable: navFlick }
                    Layout.fillWidth: true
                    Layout.fillHeight: true
                    Layout.topMargin: 4
                    contentHeight: navColumn.implicitHeight
                    clip: true
                    boundsBehavior: Flickable.StopAtBounds
                    ColumnLayout {
                        id: navColumn
                        width: parent.width
                        spacing: 2
                        Repeater {
                            model: page.groups
                            delegate: ColumnLayout {
                                id: group
                                required property var modelData
                                Layout.fillWidth: true
                                spacing: 2
                                Text {
                                    Layout.topMargin: 14
                                    Layout.leftMargin: 10
                                    Layout.bottomMargin: 4
                                    text: group.modelData.title
                                    color: Theme.textDim
                                    font.family: Theme.sans
                                    font.pixelSize: 11
                                    font.weight: Font.Medium
                                    font.capitalization: Font.AllUppercase
                                    font.letterSpacing: 0.7
                                }
                                Repeater {
                                    model: group.modelData.items
                                    delegate: Rectangle {
                                        required property var modelData
                                        readonly property bool current: page.tab === modelData.id
                                        Layout.fillWidth: true
                                        implicitHeight: 38
                                        radius: 8
                                        color: current ? Theme.active : (navArea.containsMouse ? Theme.hover : "transparent")
                                        RowLayout {
                                            anchors.fill: parent
                                            anchors.leftMargin: 10
                                            anchors.rightMargin: 10
                                            spacing: 12
                                            Icon { name: modelData.icon; size: 17; color: current ? Theme.accentSecondary : Theme.textSecondary }
                                            Text {
                                                Layout.fillWidth: true
                                                text: modelData.label
                                                elide: Text.ElideRight
                                                color: current ? Theme.text : Theme.textSecondary
                                                font.family: Theme.sans
                                                font.pixelSize: 14
                                                font.weight: current ? Font.Medium : Font.Normal
                                            }
                                        }
                                        MouseArea { id: navArea; anchors.fill: parent; hoverEnabled: true; cursorShape: Qt.PointingHandCursor; onClicked: page.tab = modelData.id }
                                    }
                                }
                            }
                        }
                    }
                }

                Text {
                    Layout.leftMargin: 10
                    Layout.bottomMargin: 6
                    text: "Serika Code " + ipc.version
                    color: Theme.textDim
                    font.family: Theme.sans
                    font.pixelSize: 11
                }
            }
        }

        // Section content
        Item {
            Layout.fillWidth: true
            Layout.fillHeight: true

            Flickable {
                id: flick
                SmoothWheel { flickable: flick }
                anchors.fill: parent
                contentWidth: width
                contentHeight: content.implicitHeight + 120
                clip: true
                boundsBehavior: Flickable.StopAtBounds
                ScrollBar.vertical: ScrollBar { policy: ScrollBar.AsNeeded; width: 8 }

                ColumnLayout {
                    id: content
                    x: Math.max(32, (flick.width - width) / 2)
                    y: 52
                    width: Math.min(780, flick.width - 64)
                    spacing: 6

                    Text {
                        text: page.current.label
                        color: Theme.text
                        font.family: Theme.brand
                        font.pixelSize: 28
                        font.weight: Font.DemiBold
                        font.letterSpacing: -0.5
                    }
                    Text {
                        Layout.bottomMargin: 18
                        text: page.current.sub
                        color: Theme.textDim
                        font.family: Theme.sans
                        font.pixelSize: 14
                    }

                    Loader {
                        Layout.fillWidth: true
                        onLoaded: flick.contentY = 0
                        sourceComponent: {
                            switch (page.tab) {
                            case "providers": return providers
                            case "appearance": return appearance
                            case "agent": return agent
                            case "browser": return browser
                            case "mcp": return mcpSection
                            case "skills": return skillsSection
                            case "keys": return keys
                            case "data": return dataSection
                            case "account": return account
                            case "about": return about
                            default: return general
                            }
                        }
                    }
                }
            }

            Component { id: general; SettingsGeneral {} }
            Component { id: providers; SettingsProviders {} }
            Component { id: appearance; SettingsAppearance {} }
            Component { id: agent; SettingsAgent {} }
            Component { id: browser; SettingsBrowser {} }
            Component { id: mcpSection; SettingsMcp {} }
            Component { id: skillsSection; SettingsSkills {} }
            Component { id: keys; SettingsKeys {} }
            Component { id: dataSection; SettingsData {} }
            Component { id: account; SettingsAccount {} }
            Component { id: about; SettingsAbout {} }
        }
    }
}

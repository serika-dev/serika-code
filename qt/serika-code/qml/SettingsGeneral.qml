import QtQuick
import QtQuick.Layouts

ColumnLayout {
    id: section
    spacing: 24
    readonly property var dict: ipc.strings
    function t(key, fallback) { return Format.tr(dict, key, fallback) }

    readonly property var localeOptions: ipc.locales.map(function (l) { return { value: l.code, label: l.name } })
    readonly property var modelOptions: {
        var out = []
        ipc.freeModels.concat(ipc.premiumModels).forEach(function (m) { out.push({ value: m.id, label: "Serika · " + (m.name || m.id) }) })
        ipc.providers.forEach(function (p) {
            if (p.installed && p.signed_in)
                (p.models || []).forEach(function (m) { out.push({ value: m.id, label: p.name + " · " + m.name }) })
        })
        return out
    }
    readonly property var modeOptions: [
        { value: "restricted", label: t("ModeSupervised", "Supervised") },
        { value: "standard", label: t("ModeAutoEdits", "Auto-accept edits") },
        { value: "full", label: t("ModeFull", "Full access") },
        { value: "read_only", label: t("ModeReadOnly", "Read-only") }
    ]

    SettingsCard {
        title: section.t("Language", "Language")
        SettingRow {
            heading: section.t("Language", "Language")
            help: section.t("LanguageHelp", "The language of menus, labels, and Serika's greetings.")
            Select {
                options: section.localeOptions
                value: ipc.locale
                implicitWidth: 220
                onPicked: (code) => ipc.setLocale(code)
            }
        }
    }

    SettingsCard {
        title: section.t("ChatSection", "Chat")
        description: section.t("ChatSectionHelp", "How new chats start and how you send messages.")
        SettingRow {
            heading: section.t("DefaultModel", "Default model")
            help: section.t("DefaultModelHelp", "Used for new chats. Serika models need nothing else; Claude Code and Codex appear once they are signed in under Providers.")
            Select {
                options: section.modelOptions
                value: ipc.selectedModel
                implicitWidth: 260
                menuWidth: 320
                onPicked: (id) => ipc.chooseModel(id)
            }
        }
        SettingRow {
            heading: section.t("DefaultPermissions", "Permissions for new chats")
            help: section.t("DefaultPermissionsHelp", "How much Serika may do without asking. You can change it per chat from the message box.")
            Select {
                options: section.modeOptions
                value: ipc.permissionMode
                implicitWidth: 220
                onPicked: (mode) => ipc.setSetting("permission", mode)
            }
        }
        SettingRow {
            heading: section.t("SendWith", "Send messages with")
            help: section.t("SendWithHelp", "The other combination inserts a new line.")
            Segmented {
                options: [{ value: "enter", label: "Enter" }, { value: "ctrlenter", label: "Ctrl + Enter" }]
                value: ipc.ui.sendKey === "ctrlenter" ? "ctrlenter" : "enter"
                onPicked: (v) => ipc.setUi("sendKey", v)
            }
        }
        SettingRow {
            heading: section.t("StreamReplies", "Stream replies")
            help: section.t("StreamRepliesHelp", "Show the answer as it is written. Turn off to see it all at once.")
            Toggle {
                checked: ipc.settings.stream !== false
                onToggled: (v) => ipc.setSetting("stream", v)
            }
        }
    }

    SettingsCard {
        title: section.t("Panels", "Side panels")
        description: section.t("PanelsHelp", "Open the panel on the right by itself when something happens in it.")
        SettingRow {
            heading: section.t("AutoBrowser", "Open the browser when Serika browses")
            help: section.t("AutoBrowserHelp", "Shows the page the agent is looking at while it works.")
            Toggle {
                checked: ipc.ui.autoBrowser !== false
                onToggled: (v) => ipc.setUi("autoBrowser", v)
            }
        }
        SettingRow {
            heading: section.t("AutoDiff", "Open changes when files change")
            help: section.t("AutoDiffHelp", "Opens the Changes tab as soon as the agent edits a file.")
            Toggle {
                checked: ipc.ui.autoDiff === true
                onToggled: (v) => ipc.setUi("autoDiff", v)
            }
        }
    }
}

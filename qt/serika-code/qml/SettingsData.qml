import QtQuick
import QtQuick.Layouts

ColumnLayout {
    id: section
    spacing: 24
    readonly property var dict: ipc.strings
    function t(key, fallback) { return Format.tr(dict, key, fallback) }
    function open(path) { Qt.openUrlExternally("file:///" + (path || "").replace(/\\/g, "/")) }
    function parent_(path) { return (path || "").replace(/[\\\/][^\\\/]*$/, "") }

    readonly property var retentionOptions: [
        { value: "30", label: "30 days" }, { value: "90", label: "90 days" },
        { value: "180", label: "6 months" }, { value: "365", label: "1 year" }, { value: "3650", label: "10 years" }
    ]

    SettingsCard {
        title: section.t("ProjectsSection", "Projects")
        description: section.t("ProjectsSectionHelp", "Folders you have worked in. Removing one only hides it from the sidebar; its chats stay in your history.")
        Repeater {
            model: ipc.projects
            delegate: SettingRow {
                required property var modelData
                heading: modelData.name
                help: modelData.path.replace(/^\\\\\?\\/, "") + (modelData.exists ? "" : "  ·  " + section.t("FolderMissing", "folder not found"))
                RowLayout {
                    spacing: 8
                    Text {
                        text: modelData.sessionCount + " " + section.t("ChatsWord", "chats")
                        color: Theme.textDim
                        font.family: Theme.sans
                        font.pixelSize: 12
                    }
                    ChipButton {
                        visible: modelData.exists
                        text: section.t("OpenFolder", "Open")
                        onClicked: section.open(modelData.path)
                    }
                    ChipButton {
                        text: section.t("RemoveProject", "Remove")
                        danger: true
                        enabled: !modelData.current
                        onClicked: ipc.removeProject(modelData.path)
                    }
                }
            }
        }
        SettingRow {
            visible: ipc.projects.length === 0
            heading: section.t("NoProjects", "No projects yet")
            help: section.t("NoProjectsHelp", "Add a folder from the sidebar to start working in it.")
            ChipButton { text: section.t("AddProject", "Add project folder"); iconName: "folder-plus"; primary: true; onClicked: ipc.requestPanel("addProject") }
        }
    }

    SettingsCard {
        title: section.t("HistorySection", "History")
        SettingRow {
            heading: section.t("Retention", "Keep chats for")
            help: section.t("RetentionHelp", "Older chats are removed when Serika Code starts.")
            Select {
                options: section.retentionOptions
                value: String(ipc.settings.retentionDays || 90)
                implicitWidth: 170
                onPicked: (v) => ipc.setSetting("session_retention_days", v)
            }
        }
    }

    SettingsCard {
        title: section.t("StorageSection", "Where things are stored")
        description: section.t("StorageSectionHelp", "Everything stays on this computer. Your Serika sign-in is kept in Windows Credential Manager.")
        Repeater {
            model: [
                { label: section.t("PathConfig", "Settings file"), path: ipc.settings.configFile, folder: true },
                { label: section.t("PathDatabase", "Chat history"), path: ipc.settings.database, folder: true },
                { label: section.t("PathLogs", "Logs"), path: ipc.settings.logsDir, folder: false },
                { label: section.t("PathData", "App data"), path: ipc.settings.dataDir, folder: false }
            ]
            delegate: SettingRow {
                required property var modelData
                heading: modelData.label
                help: modelData.path || ""
                ChipButton {
                    text: section.t("Reveal", "Show in folder")
                    iconName: "folder"
                    onClicked: section.open(modelData.folder ? section.parent_(modelData.path) : modelData.path)
                }
            }
        }
    }
}

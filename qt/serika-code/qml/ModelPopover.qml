import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

// Model picker: search, Serika models (Included / Serika+), then your Claude Code and Codex subscriptions.
Popover {
    id: pop
    signal providerSetupRequested(string provider)
    readonly property var dict: ipc.strings
    function t(key, values) { return Format.t(dict, key, values) }

    // Re-read on open so a limit whose reset has passed clears.
    property real now: Date.now()
    readonly property var serikaLimit: Format.serikaLimit(ipc.weeklyPercent, ipc.weeklyResetsAt, ipc.sessionPercent, ipc.sessionResetsAt)

    readonly property var rows: {
        const q = query.text.trim().toLowerCase()
        const serikaLimit = pop.serikaLimit
        const match = function (m) {
            return !q || ((m.name || "") + " " + (m.provider || "") + " " + m.id).toLowerCase().indexOf(q) >= 0
        }
        const out = []
        const free = ipc.freeModels.filter(match)
        const premium = ipc.premiumModels.filter(match)
        if (free.length > 0) {
            out.push({ kind: "label", text: "Serika · " + t("Included"), upgrade: false, mark: "serika", limit: serikaLimit })
            free.forEach(function (m) { out.push({ kind: "model", model: m, locked: false, limit: serikaLimit }) })
        }
        if (premium.length > 0) {
            out.push({ kind: "label", text: "Serika · " + (t("SerikaPlus") || "Serika+"), upgrade: !ipc.isPremium, mark: "serika", limit: serikaLimit })
            premium.forEach(function (m) { out.push({ kind: "model", model: m, locked: !ipc.isPremium, limit: serikaLimit }) })
        }
        ipc.providers.forEach(function (p) {
            const models = (p.models || []).filter(match)
            const ready = p.installed === true && p.signed_in === true
            if (q && models.length === 0) return
            const limit = ready ? Format.providerLimit(p, pop.now) : null
            out.push({
                kind: "label",
                text: p.name + (p.plan ? " · " + p.plan : "") + " · " + Format.tr(dict, "Subscription", "subscription"),
                mark: p.id,
                limit: limit,
                note: limit ? Format.tr(dict, "LimitReached", "Limit reached") : ""
            })
            if (!ready) {
                out.push({ kind: "setup", provider: p.id, name: p.name,
                           text: p.installed ? Format.tr(dict, "SignInRequired", "Sign in to use your subscription")
                                             : Format.tr(dict, "NotInstalled", "Not installed. Set up to use your subscription") })
                return
            }
            models.forEach(function (m) { out.push({ kind: "model", model: m, locked: false, limit: limit }) })
        })
        return out
    }

    width: 340
    height: Math.min(520, 60 + list.contentHeight)
    padding: 0
    radius: 16

    onOpened: { now = Date.now(); query.text = ""; query.forceActiveFocus() }

    contentItem: ColumnLayout {
        spacing: 0

        Rectangle {
            Layout.fillWidth: true
            Layout.margins: 6
            Layout.preferredHeight: 40
            radius: 8
            color: Theme.hover
            Icon {
                anchors.left: parent.left
                anchors.leftMargin: 12
                anchors.verticalCenter: parent.verticalCenter
                name: "search"
                size: 15
                color: Theme.textDim
            }
            TextField {
                id: query
                anchors.fill: parent
                anchors.leftMargin: 34
                anchors.rightMargin: 8
                placeholderText: pop.t("SearchModels")
                color: Theme.text
                placeholderTextColor: Theme.textDim
                selectionColor: Theme.accent
                font.family: Theme.sans
                font.pixelSize: 14
                background: null
                verticalAlignment: TextInput.AlignVCenter
            }
        }

        ListView {
            id: list
            Layout.fillWidth: true
            Layout.fillHeight: true
            Layout.leftMargin: 4
            Layout.rightMargin: 4
            Layout.bottomMargin: 4
            clip: true
            model: pop.rows
            boundsBehavior: Flickable.StopAtBounds
            delegate: ModelRow {
                required property var modelData
                width: ListView.view.width
                entry: modelData
                onChosen: (id) => { ipc.chooseModel(id); pop.close() }
                onLockedChosen: { pop.close(); Qt.openUrlExternally("https://serika.dev/premium") }
                onSetupChosen: (provider) => { pop.close(); pop.providerSetupRequested(provider) }
            }
            ScrollBar.vertical: ScrollBar { policy: ScrollBar.AsNeeded; width: 6 }

            Text {
                visible: pop.rows.length === 0
                anchors.centerIn: parent
                text: ipc.freeModels.length + ipc.premiumModels.length === 0 ? pop.t("NoModels") : pop.t("NoModelsMatch", { q: query.text })
                color: Theme.textDim
                font.family: Theme.sans
                font.pixelSize: 14
            }
        }
    }
}

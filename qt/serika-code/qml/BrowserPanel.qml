import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

// The browser Serika and you share: tabs, an address bar, and the live page filling the pane.
// The page renders at the pane's own size (divided by the zoom), so nothing is letterboxed.
Item {
    id: panel
    readonly property var dict: ipc.strings
    readonly property var frame: ipc.browser
    readonly property var tabs: frame.tabs || []
    // Running as soon as the browser reports tabs; a blank new tab has no picture yet.
    readonly property bool running: frame.running === true && tabs.length > 0
    readonly property bool blank: !running || (frame.url || "") === "about:blank" || (frame.url || "") === ""
    readonly property bool wantStream: visible && running && !blank && Qt.application.state === Qt.ApplicationActive
    // Each panel holds the live picture while it shows a page; moving between docks makes a new panel.
    property bool streamHeld: false
    function holdStream(on) {
        if (on === streamHeld) return
        streamHeld = on
        ipc.browserStream(on)
    }
    onWantStreamChanged: holdStream(wantStream)
    Component.onCompleted: { holdStream(wantStream); syncTabs() }
    Component.onDestruction: holdStream(false)

    // Tabs live in a model that is updated in place, so polls never rebuild (and flicker) the strip.
    ListModel { id: tabModel }
    onFrameChanged: syncTabs()
    function syncTabs() {
        // From ipc directly: the panel's own bindings may not have caught up inside this handler.
        const f = ipc.browser
        const live = f.running === true && (f.tabs || []).length > 0
        const list = live ? f.tabs : [{ id: "", title: "", url: "", favicon: "", active: true }]
        for (let i = 0; i < list.length; i++) {
            const t = list[i]
            const row = { tabId: t.id || "", title: t.title || "", url: t.url || "", favicon: t.favicon || "", active: t.active === true }
            if (i < tabModel.count && tabModel.get(i).tabId !== row.tabId) {
                let found = -1
                for (let j = i + 1; j < tabModel.count; j++)
                    if (tabModel.get(j).tabId === row.tabId) { found = j; break }
                if (found >= 0) tabModel.move(found, i, 1)
                else tabModel.insert(i, row)
            }
            if (i < tabModel.count) {
                const old = tabModel.get(i)
                if (old.title !== row.title || old.url !== row.url || old.favicon !== row.favicon || old.active !== row.active)
                    tabModel.set(i, row)
            } else {
                tabModel.append(row)
            }
        }
        while (tabModel.count > list.length) tabModel.remove(tabModel.count - 1)
    }
    readonly property bool loading: ipc.browserBusy || frame.loading === true
    readonly property bool enabledInSettings: ipc.settings.browserEnabled !== false
    readonly property bool detected: (ipc.settings.browserDetected || "").length > 0
    readonly property real zoom: Number(ipc.ui.browserZoom) > 0 ? Number(ipc.ui.browserZoom) : 1
    property bool agentActive: false
    property string dismissedError: ""
    property real wheelAccumulated: 0
    property real wheelX: 0
    property real wheelY: 0

    function tr(key, fallback, values) { return Format.tr(dict, key, fallback, values) }
    function go(text) {
        const value = text.trim()
        if (value.length > 0) ipc.browserOpen(value)
    }
    function setZoom(value) {
        const z = Math.max(0.5, Math.min(2, Math.round(value * 10) / 10))
        ipc.setUi("browserZoom", z)
    }
    function host(url) {
        const m = (url || "").match(/^[a-z]+:\/\/([^\/?#]+)/i)
        return m ? m[1].replace(/^www\./, "") : (url || "")
    }
    function closeTab(id) {
        ipc.browserCloseTab(id)
    }
    function newTab() {
        if (panel.running) ipc.browserNewTab("")
        Qt.callLater(function () { startSearch.forceActiveFocus() })
    }
    function tabTitle(tab) {
        if (!tab.url || tab.url === "about:blank") return panel.tr("NewTab", "New tab")
        return tab.title || panel.host(tab.url)
    }

    Connections {
        target: ipc
        function onBrowserActivity() { panel.agentActive = true; idle.restart() }
        function onRunningChanged() { if (!ipc.running) panel.agentActive = false }
    }
    Timer { id: idle; interval: 4000; onTriggered: panel.agentActive = false }
    Timer {
        id: wheelFlush
        interval: 33
        onTriggered: {
            if (panel.wheelAccumulated !== 0)
                ipc.browserWheel(panel.wheelX, panel.wheelY, panel.wheelAccumulated)
            panel.wheelAccumulated = 0
        }
    }
    // Keep tabs and the address current; while streaming the picture arrives on its own.
    Timer {
        interval: panel.wantStream ? 1200 : (panel.loading ? 450 : 1000)
        repeat: true
        running: panel.visible && panel.running && Qt.application.state === Qt.ApplicationActive
        onTriggered: ipc.browserPoll()
    }
    // Tell the browser how large the pane is, once resizing settles.
    Timer {
        id: resizeTimer
        interval: 220
        onTriggered: {
            if (!panel.visible || viewport.width < 40 || viewport.height < 40) return
            ipc.browserResize(Math.round(viewport.width / panel.zoom), Math.round(viewport.height / panel.zoom))
        }
    }
    onZoomChanged: resizeTimer.restart()
    onVisibleChanged: if (visible) resizeTimer.restart()
    onRunningChanged: if (running) resizeTimer.restart()

    ColumnLayout {
        anchors.fill: parent
        spacing: 0

        // Tab strip
        Rectangle {
            Layout.fillWidth: true
            Layout.preferredHeight: 40
            color: Theme.bg
            RowLayout {
                anchors.fill: parent
                anchors.leftMargin: 8
                anchors.rightMargin: 8
                anchors.topMargin: 6
                spacing: 2
                Repeater {
                    model: tabModel
                    delegate: Rectangle {
                        id: tabItem
                        required property string tabId
                        required property string title
                        required property string url
                        required property string favicon
                        required property bool active
                        readonly property var modelData: ({ id: tabId, title: title, url: url, favicon: favicon, active: active })
                        readonly property bool current: active
                        opacity: 0
                        Component.onCompleted: opacity = 1
                        Behavior on opacity { NumberAnimation { duration: Theme.motionFast } }
                        Layout.fillWidth: true
                        Layout.maximumWidth: 210
                        Layout.minimumWidth: 70
                        Layout.fillHeight: true
                        topLeftRadius: 9
                        topRightRadius: 9
                        color: current ? Theme.card : (tabArea.containsMouse ? Theme.hover : "transparent")
                        RowLayout {
                            anchors.fill: parent
                            anchors.leftMargin: 10
                            anchors.rightMargin: 6
                            spacing: 7
                            Item {
                                Layout.preferredWidth: 14
                                Layout.preferredHeight: 14
                                Icon {
                                    id: tabSpinner
                                    anchors.fill: parent
                                    visible: tabItem.current && panel.loading
                                    name: "loader"
                                    size: 14
                                    color: Theme.accentSecondary
                                    RotationAnimator on rotation { running: tabSpinner.visible; from: 0; to: 360; duration: 900; loops: Animation.Infinite }
                                }
                                Image {
                                    id: faviconImage
                                    anchors.fill: parent
                                    visible: !tabSpinner.visible && status === Image.Ready
                                    source: (tabItem.modelData.favicon || "").indexOf("http") === 0 || (tabItem.modelData.favicon || "").indexOf("data:") === 0 ? tabItem.modelData.favicon : ""
                                    sourceSize: Qt.size(28, 28)
                                    smooth: true
                                    asynchronous: true
                                }
                                Icon {
                                    anchors.fill: parent
                                    visible: !tabSpinner.visible && !faviconImage.visible
                                    name: (tabItem.modelData.url || "").indexOf("localhost") >= 0 || (tabItem.modelData.url || "").indexOf("127.0.0.1") >= 0 ? "terminal" : "globe"
                                    size: 14
                                    color: tabItem.current ? Theme.textSecondary : Theme.textDim
                                }
                            }
                            Text {
                                Layout.fillWidth: true
                                text: panel.tabTitle(tabItem.modelData)
                                elide: Text.ElideRight
                                color: tabItem.current ? Theme.text : Theme.textSecondary
                                font.family: Theme.sans
                                font.pixelSize: 12
                            }
                            Rectangle {
                                visible: panel.running && (tabItem.current || tabArea.containsMouse)
                                Layout.preferredWidth: 18
                                Layout.preferredHeight: 18
                                radius: 5
                                color: closeArea.containsMouse ? Theme.active : "transparent"
                                Icon { anchors.centerIn: parent; name: "x"; size: 12; color: Theme.textSecondary }
                                MouseArea {
                                    id: closeArea
                                    anchors.fill: parent
                                    hoverEnabled: true
                                    cursorShape: Qt.PointingHandCursor
                                    onClicked: panel.closeTab(tabItem.modelData.id)
                                }
                            }
                        }
                        MouseArea {
                            id: tabArea
                            anchors.fill: parent
                            z: -1
                            hoverEnabled: true
                            acceptedButtons: Qt.LeftButton | Qt.MiddleButton
                            cursorShape: Qt.PointingHandCursor
                            onClicked: (mouse) => {
                                if (!panel.running) return
                                if (mouse.button === Qt.MiddleButton) panel.closeTab(tabItem.modelData.id)
                                else if (!tabItem.current) ipc.browserSwitchTab(tabItem.modelData.id)
                            }
                        }
                        ToolTip.visible: tabArea.containsMouse && (tabItem.modelData.url || "").length > 0
                        ToolTip.delay: 700
                        ToolTip.text: (tabItem.modelData.title || "") + "\n" + (tabItem.modelData.url || "")
                    }
                }
                IconButton {
                    Layout.alignment: Qt.AlignVCenter
                    implicitWidth: 28
                    implicitHeight: 28
                    icon: "plus"
                    iconSize: 15
                    tip: panel.tr("NewTab", "New tab") + "  (Ctrl T)"
                    onClicked: panel.newTab()
                }
                Item { Layout.fillWidth: true; Layout.minimumWidth: 0 }
            }
        }

        // Toolbar
        Rectangle {
            Layout.fillWidth: true
            Layout.preferredHeight: 46
            color: Theme.card
            RowLayout {
                anchors.fill: parent
                anchors.leftMargin: 8
                anchors.rightMargin: 8
                spacing: 2
                IconButton {
                    icon: "arrow-left"
                    iconSize: 16
                    opacity: panel.frame.canGoBack ? 1 : 0.35
                    tip: panel.tr("Back", "Back")
                    onClicked: if (panel.frame.canGoBack) ipc.browserBack()
                }
                IconButton {
                    icon: "arrow-right"
                    iconSize: 16
                    opacity: panel.frame.canGoForward ? 1 : 0.35
                    tip: panel.tr("Forward", "Forward")
                    onClicked: if (panel.frame.canGoForward) ipc.browserForward()
                }
                IconButton {
                    icon: "rotate-cw"
                    iconSize: 15
                    opacity: panel.running && !panel.blank ? 1 : 0.35
                    tip: panel.tr("Reload", "Reload")
                    onClicked: if (panel.running && !panel.blank) ipc.browserReload()
                }
                Rectangle {
                    Layout.fillWidth: true
                    Layout.leftMargin: 4
                    Layout.rightMargin: 4
                    Layout.preferredHeight: 32
                    radius: 16
                    color: address.activeFocus ? Theme.input : Theme.hover
                    border.width: 1
                    border.color: address.activeFocus ? Theme.accent : "transparent"
                    Icon {
                        id: addressIcon
                        anchors.left: parent.left
                        anchors.leftMargin: 12
                        anchors.verticalCenter: parent.verticalCenter
                        name: (panel.frame.url || "").indexOf("https://") === 0 ? "lock" : (panel.blank ? "search" : "globe")
                        size: 13
                        color: Theme.textDim
                    }
                    TextField {
                        id: address
                        anchors.fill: parent
                        anchors.leftMargin: 31
                        anchors.rightMargin: zoomChip.visible ? zoomChip.width + 14 : 12
                        background: null
                        color: Theme.text
                        placeholderText: panel.tr("BrowserAddress", "Search or enter a web address")
                        placeholderTextColor: Theme.textDim
                        selectionColor: Theme.accent
                        selectedTextColor: "white"
                        selectByMouse: true
                        font.family: Theme.sans
                        font.pixelSize: 13
                        verticalAlignment: TextInput.AlignVCenter
                        text: (panel.frame.url || "") === "about:blank" ? "" : (panel.frame.url || "")
                        onAccepted: { panel.go(text); viewport.forceActiveFocus() }
                        onActiveFocusChanged: if (activeFocus) selectAll()
                        Keys.onEscapePressed: { text = panel.blank ? "" : (panel.frame.url || ""); viewport.forceActiveFocus() }
                        Connections {
                            target: panel
                            // Read the frame itself: panel.blank may not have caught up with it yet.
                        function onFrameChanged() {
                            if (address.activeFocus) return
                            const url = panel.frame.url || ""
                            address.text = url === "about:blank" ? "" : url
                        }
                        }
                    }
                    // Zoom, shown when it is not 100 %
                    Rectangle {
                        id: zoomChip
                        visible: panel.zoom !== 1
                        anchors.right: parent.right
                        anchors.rightMargin: 6
                        anchors.verticalCenter: parent.verticalCenter
                        width: zoomText.implicitWidth + 14
                        height: 22
                        radius: 11
                        color: zoomArea.containsMouse ? Theme.active : Theme.accentSoft
                        Text {
                            id: zoomText
                            anchors.centerIn: parent
                            text: Math.round(panel.zoom * 100) + "%"
                            color: Theme.accentSecondary
                            font.family: Theme.sans
                            font.pixelSize: 11
                            font.weight: Font.Medium
                        }
                        MouseArea {
                            id: zoomArea
                            anchors.fill: parent
                            hoverEnabled: true
                            cursorShape: Qt.PointingHandCursor
                            onClicked: panel.setZoom(1)
                        }
                        ToolTip.visible: zoomArea.containsMouse
                        ToolTip.delay: 500
                        ToolTip.text: panel.tr("ResetZoom", "Reset zoom")
                    }
                }
                IconButton {
                    id: moreButton
                    icon: "more-horizontal"
                    iconSize: 17
                    active: moreMenu.opened
                    tip: panel.tr("BrowserMenu", "Zoom and more")
                    onClicked: moreMenu.opened ? moreMenu.close() : moreMenu.open()
                    Popover {
                        id: moreMenu
                        x: moreButton.width - width
                        y: moreButton.height + 6
                        width: 230
                        contentItem: ColumnLayout {
                            spacing: 2
                            RowLayout {
                                Layout.fillWidth: true
                                Layout.leftMargin: 10
                                Layout.rightMargin: 4
                                Layout.preferredHeight: 38
                                Text { Layout.fillWidth: true; text: panel.tr("Zoom", "Zoom"); color: Theme.text; font.family: Theme.sans; font.pixelSize: 14 }
                                IconButton { icon: "minus"; iconSize: 14; implicitWidth: 28; implicitHeight: 28; onClicked: panel.setZoom(panel.zoom - 0.1) }
                                Text {
                                    Layout.preferredWidth: 42
                                    horizontalAlignment: Text.AlignHCenter
                                    text: Math.round(panel.zoom * 100) + "%"
                                    color: Theme.textSecondary
                                    font.family: Theme.sans
                                    font.pixelSize: 13
                                }
                                IconButton { icon: "plus"; iconSize: 14; implicitWidth: 28; implicitHeight: 28; onClicked: panel.setZoom(panel.zoom + 0.1) }
                            }
                            MenuDivider {}
                            MenuRow {
                                icon: "external-link"
                                label: panel.tr("OpenInBrowser", "Open in your browser")
                                enabledRow: panel.running && !panel.blank
                                onTriggered: { moreMenu.close(); Qt.openUrlExternally(panel.frame.url) }
                            }
                            MenuRow {
                                icon: "copy"
                                label: panel.tr("CopyAddress", "Copy address")
                                enabledRow: panel.running && !panel.blank
                                onTriggered: { moreMenu.close(); ipc.copyText(panel.frame.url) }
                            }
                            MenuDivider {}
                            MenuRow {
                                icon: "x-circle"
                                label: panel.tr("StopBrowser", "Close browser")
                                enabledRow: panel.running
                                onTriggered: { moreMenu.close(); ipc.browserClose() }
                            }
                        }
                    }
                }
            }
            // Loading bar
            Item {
                anchors.left: parent.left
                anchors.right: parent.right
                anchors.bottom: parent.bottom
                height: 2
                clip: true
                visible: panel.loading
                Rectangle {
                    id: sweep
                    width: parent.width * 0.35
                    height: parent.height
                    radius: 1
                    color: Theme.accent
                    NumberAnimation on x {
                        running: panel.loading
                        from: -sweep.width
                        to: sweep.parent.width
                        duration: 1100
                        loops: Animation.Infinite
                        easing.type: Easing.InOutQuad
                    }
                }
            }
            Rectangle { anchors.bottom: parent.bottom; width: parent.width; height: 1; color: Theme.borderSubtle; visible: !panel.loading }
        }

        // Page
        Item {
            id: viewport
            Layout.fillWidth: true
            Layout.fillHeight: true
            focus: true
            activeFocusOnTab: true
            clip: true
            onWidthChanged: resizeTimer.restart()
            onHeightChanged: resizeTimer.restart()

            Rectangle { anchors.fill: parent; color: panel.running && !panel.blank ? "white" : Theme.card }

            Image {
                id: shot
                visible: panel.running && !panel.blank
                anchors.fill: parent
                source: ipc.browserImage
                fillMode: Image.PreserveAspectFit
                horizontalAlignment: Image.AlignLeft
                verticalAlignment: Image.AlignTop
                cache: false
                smooth: true
                mipmap: panel.zoom < 1
            }
            // The page gets real mouse events: hover menus, drag to select, double-click a word.
            MouseArea {
                id: pageMouse
                anchors.fill: parent
                enabled: shot.visible
                hoverEnabled: true
                acceptedButtons: Qt.LeftButton | Qt.RightButton | Qt.MiddleButton
                cursorShape: Qt.ArrowCursor
                property real lastMoveX: -1
                property real lastMoveY: -1
                property real pendingX: 0
                property real pendingY: 0
                function pageX(x) { return x * (panel.frame.width || 1280) / Math.max(1, shot.paintedWidth) }
                function pageY(y) { return y * (panel.frame.height || 800) / Math.max(1, shot.paintedHeight) }
                function buttonName(b) { return b === Qt.RightButton ? "right" : (b === Qt.MiddleButton ? "middle" : "left") }
                function heldMask() {
                    let mask = 0
                    if (pressedButtons & Qt.LeftButton) mask |= 1
                    if (pressedButtons & Qt.RightButton) mask |= 2
                    if (pressedButtons & Qt.MiddleButton) mask |= 4
                    return mask
                }
                onPressed: (mouse) => {
                    viewport.forceActiveFocus()
                    ipc.browserMouse("down", pageX(mouse.x), pageY(mouse.y), buttonName(mouse.button), heldMask(), 1)
                }
                onReleased: (mouse) => ipc.browserMouse("up", pageX(mouse.x), pageY(mouse.y), buttonName(mouse.button), heldMask(), 1)
                onDoubleClicked: (mouse) => {
                    ipc.browserMouse("down", pageX(mouse.x), pageY(mouse.y), "left", 1, 2)
                    ipc.browserMouse("up", pageX(mouse.x), pageY(mouse.y), "left", 0, 2)
                }
                onPositionChanged: (mouse) => {
                    pendingX = mouse.x
                    pendingY = mouse.y
                    if (!moveThrottle.running) moveThrottle.start()
                }
                Timer {
                    id: moveThrottle
                    interval: 33
                    onTriggered: {
                        if (Math.abs(pageMouse.pendingX - pageMouse.lastMoveX) < 1 && Math.abs(pageMouse.pendingY - pageMouse.lastMoveY) < 1) return
                        pageMouse.lastMoveX = pageMouse.pendingX
                        pageMouse.lastMoveY = pageMouse.pendingY
                        const held = pageMouse.heldMask()
                        ipc.browserMouse("move", pageMouse.pageX(pageMouse.pendingX), pageMouse.pageY(pageMouse.pendingY),
                                         held & 1 ? "left" : "none", held, 0)
                    }
                }
                onWheel: (wheel) => {
                    if (wheel.modifiers & Qt.ControlModifier) {
                        panel.setZoom(panel.zoom + (wheel.angleDelta.y > 0 ? 0.1 : -0.1))
                    } else {
                        // Throttle, not debounce: keep sending while the wheel keeps turning.
                        panel.wheelAccumulated += wheel.pixelDelta.y !== 0 ? -wheel.pixelDelta.y : -wheel.angleDelta.y
                        panel.wheelX = pageX(wheel.x)
                        panel.wheelY = pageY(wheel.y)
                        if (!wheelFlush.running) wheelFlush.start()
                    }
                    wheel.accepted = true
                }
            }
            Keys.onPressed: (event) => {
                if (event.modifiers & Qt.ControlModifier) {
                    let handled = true
                    if (event.key === Qt.Key_L) address.forceActiveFocus()
                    else if (event.key === Qt.Key_C) ipc.browserCopy()
                    else if (event.key === Qt.Key_V) ipc.browserPaste()
                    else if (event.key === Qt.Key_A) ipc.browserSelectAll()
                    else if (event.key === Qt.Key_R) ipc.browserReload()
                    else handled = false
                    if (handled) { event.accepted = true; return }
                }
                const names = {}
                names[Qt.Key_Return] = "enter"; names[Qt.Key_Enter] = "enter"
                names[Qt.Key_Backspace] = "backspace"; names[Qt.Key_Delete] = "delete"
                names[Qt.Key_Tab] = "tab"; names[Qt.Key_Escape] = "escape"
                names[Qt.Key_Up] = "arrowup"; names[Qt.Key_Down] = "arrowdown"
                names[Qt.Key_Left] = "arrowleft"; names[Qt.Key_Right] = "arrowright"
                names[Qt.Key_PageUp] = "pageup"; names[Qt.Key_PageDown] = "pagedown"
                names[Qt.Key_Home] = "home"; names[Qt.Key_End] = "end"; names[Qt.Key_Space] = "space"
                if (names[event.key] !== undefined && !(event.modifiers & Qt.ControlModifier)) {
                    ipc.browserInput(names[event.key], "")
                    event.accepted = true
                } else if (event.text.length > 0 && !(event.modifiers & (Qt.ControlModifier | Qt.AltModifier))) {
                    ipc.browserInput("", event.text)
                    event.accepted = true
                }
            }

            // New tab page, also shown before the browser has started
            Flickable {
                id: start
                SmoothWheel { flickable: start }
                visible: panel.blank && !ipc.browserBusy
                anchors.fill: parent
                contentHeight: startColumn.implicitHeight + 80
                boundsBehavior: Flickable.StopAtBounds
                clip: true
                ColumnLayout {
                    id: startColumn
                    width: Math.min(parent.width - 48, 520)
                    x: (start.width - width) / 2
                    y: Math.max(32, (start.height - implicitHeight) / 2 - 30)
                    spacing: 18
                    Rectangle {
                        Layout.alignment: Qt.AlignHCenter
                        Layout.preferredWidth: 56
                        Layout.preferredHeight: 56
                        radius: 18
                        gradient: Gradient {
                            orientation: Gradient.Vertical
                            GradientStop { position: 0; color: Theme.accent }
                            GradientStop { position: 1; color: Theme.accentHover }
                        }
                        Icon { anchors.centerIn: parent; name: "globe"; size: 26; color: "white" }
                    }
                    Text {
                        Layout.fillWidth: true
                        horizontalAlignment: Text.AlignHCenter
                        text: !panel.enabledInSettings ? panel.tr("BrowserOff", "The browser is turned off")
                              : (!panel.detected ? panel.tr("BrowserMissing", "No browser found") : panel.tr("BrowserReady", "Serika's browser"))
                        color: Theme.text
                        font.family: Theme.brand
                        font.pixelSize: 20
                        font.weight: Font.DemiBold
                    }
                    Text {
                        Layout.fillWidth: true
                        horizontalAlignment: Text.AlignHCenter
                        wrapMode: Text.WordWrap
                        text: !panel.enabledInSettings ? panel.tr("BrowserOffHelp", "Turn it on in Settings → Browser to let Serika read docs, check a dev server, or follow links.")
                              : (!panel.detected ? panel.tr("BrowserMissingHelp", "Install Microsoft Edge or Google Chrome, or set a browser path in Settings → Browser.")
                                                 : panel.tr("BrowserReadyHelp", "A real browser with JavaScript. Ask Serika to look something up, or open a page yourself."))
                        color: Theme.textDim
                        font.family: Theme.sans
                        font.pixelSize: 13
                    }
                    // Search box
                    Rectangle {
                        visible: panel.enabledInSettings && panel.detected
                        Layout.fillWidth: true
                        Layout.preferredHeight: 46
                        radius: 23
                        color: Theme.input
                        border.width: 1
                        border.color: startSearch.activeFocus ? Theme.accent : Theme.border
                        Icon { anchors.left: parent.left; anchors.leftMargin: 16; anchors.verticalCenter: parent.verticalCenter; name: "search"; size: 16; color: Theme.textDim }
                        TextField {
                            id: startSearch
                            anchors.fill: parent
                            anchors.leftMargin: 42
                            anchors.rightMargin: 16
                            background: null
                            color: Theme.text
                            placeholderText: panel.tr("BrowserAddress", "Search or enter a web address")
                            placeholderTextColor: Theme.textDim
                            selectionColor: Theme.accent
                            font.family: Theme.sans
                            font.pixelSize: 14
                            verticalAlignment: TextInput.AlignVCenter
                            onAccepted: { panel.go(text); text = "" }
                        }
                    }
                    // Quick links
                    GridLayout {
                        visible: panel.enabledInSettings && panel.detected
                        Layout.alignment: Qt.AlignHCenter
                        columns: Math.max(2, Math.min(4, Math.floor((startColumn.width + 10) / 120)))
                        columnSpacing: 10
                        rowSpacing: 10
                        Repeater {
                            model: {
                                const home = ipc.settings.browserHome || "https://duckduckgo.com"
                                return [
                                    { label: panel.host(home), url: home, icon: "home" },
                                    { label: "localhost:3000", url: "http://localhost:3000", icon: "terminal" },
                                    { label: "localhost:5173", url: "http://localhost:5173", icon: "terminal" },
                                    { label: "GitHub", url: "https://github.com", icon: "git-branch" },
                                    { label: "MDN", url: "https://developer.mozilla.org", icon: "book-open" },
                                    { label: "docs.rs", url: "https://docs.rs", icon: "file-text" },
                                    { label: "npm", url: "https://www.npmjs.com", icon: "layers" },
                                    { label: "Stack Overflow", url: "https://stackoverflow.com", icon: "list" }
                                ]
                            }
                            delegate: Rectangle {
                                required property var modelData
                                Layout.preferredWidth: 110
                                Layout.preferredHeight: 78
                                radius: 14
                                color: linkArea.containsMouse ? Theme.hover : "transparent"
                                border.width: 1
                                border.color: linkArea.containsMouse ? Theme.border : Theme.borderSubtle
                                ColumnLayout {
                                    anchors.centerIn: parent
                                    spacing: 8
                                    Rectangle {
                                        Layout.alignment: Qt.AlignHCenter
                                        Layout.preferredWidth: 32
                                        Layout.preferredHeight: 32
                                        radius: 10
                                        color: Theme.accentSoft
                                        Icon { anchors.centerIn: parent; name: modelData.icon; size: 16; color: Theme.accentSecondary }
                                    }
                                    Text {
                                        Layout.alignment: Qt.AlignHCenter
                                        Layout.maximumWidth: 96
                                        text: modelData.label
                                        elide: Text.ElideRight
                                        color: Theme.textSecondary
                                        font.family: Theme.sans
                                        font.pixelSize: 12
                                    }
                                }
                                MouseArea {
                                    id: linkArea
                                    anchors.fill: parent
                                    hoverEnabled: true
                                    cursorShape: Qt.PointingHandCursor
                                    onClicked: ipc.browserOpen(modelData.url)
                                }
                            }
                        }
                    }
                }
            }

            // Busy before the first picture
            Icon {
                id: firstLoad
                visible: ipc.browserBusy && !shot.visible
                anchors.centerIn: parent
                name: "loader"
                size: 28
                color: Theme.accentSecondary
                RotationAnimator on rotation { running: firstLoad.visible; from: 0; to: 360; duration: 1000; loops: Animation.Infinite }
            }

            // The agent is driving: a soft accent glow and a badge
            Rectangle {
                anchors.fill: parent
                visible: panel.agentActive
                color: "transparent"
                border.width: 3
                border.color: Theme.accent
                SequentialAnimation on opacity {
                    running: panel.agentActive
                    loops: Animation.Infinite
                    NumberAnimation { from: 0.9; to: 0.35; duration: 900; easing.type: Easing.InOutSine }
                    NumberAnimation { from: 0.35; to: 0.9; duration: 900; easing.type: Easing.InOutSine }
                }
            }
            Rectangle {
                visible: panel.agentActive
                anchors.horizontalCenter: parent.horizontalCenter
                anchors.bottom: parent.bottom
                anchors.bottomMargin: 16
                height: 30
                width: badge.implicitWidth + 30
                radius: 15
                color: Qt.rgba(0.07, 0.06, 0.1, 0.86)
                border.width: 1
                border.color: Qt.rgba(1, 1, 1, 0.12)
                Row {
                    id: badge
                    anchors.centerIn: parent
                    spacing: 8
                    Rectangle {
                        width: 8
                        height: 8
                        radius: 4
                        color: Theme.accent
                        anchors.verticalCenter: parent.verticalCenter
                        SequentialAnimation on opacity {
                            running: panel.agentActive
                            loops: Animation.Infinite
                            NumberAnimation { to: 0.3; duration: 500 }
                            NumberAnimation { to: 1; duration: 500 }
                        }
                    }
                    Text {
                        anchors.verticalCenter: parent.verticalCenter
                        text: panel.tr("AgentBrowsing", "Serika is browsing")
                        color: "white"
                        font.family: Theme.sans
                        font.pixelSize: 12
                        font.weight: Font.Medium
                    }
                }
            }

            // Error for this page
            Rectangle {
                visible: ipc.browserError.length > 0 && ipc.browserError !== panel.dismissedError
                anchors.left: parent.left
                anchors.right: parent.right
                anchors.top: parent.top
                anchors.margins: 10
                implicitHeight: errorRow.implicitHeight + 16
                radius: 10
                color: Theme.card
                border.width: 1
                border.color: Theme.danger
                RowLayout {
                    id: errorRow
                    anchors.fill: parent
                    anchors.margins: 8
                    anchors.leftMargin: 12
                    spacing: 10
                    Icon { name: "alert-circle"; size: 16; color: Theme.danger }
                    Text {
                        Layout.fillWidth: true
                        wrapMode: Text.WordWrap
                        text: ipc.browserError
                        color: Theme.text
                        font.family: Theme.sans
                        font.pixelSize: 12
                    }
                    IconButton { icon: "x"; iconSize: 13; implicitWidth: 24; implicitHeight: 24; onClicked: panel.dismissedError = ipc.browserError }
                }
            }
        }
    }

    Shortcut {
        enabled: panel.visible
        sequence: "Ctrl+T"
        onActivated: panel.newTab()
    }
    Shortcut {
        enabled: panel.visible && panel.running && (viewport.activeFocus || address.activeFocus)
        sequence: "Ctrl+W"
        onActivated: {
            const active = panel.tabs.filter(function (t) { return t.active === true })[0]
            if (active) panel.closeTab(active.id)
        }
    }
}

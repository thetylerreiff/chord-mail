import QtQuick
import QtQuick.Controls
import QtQuick.Controls.Material
import QtQuick.Layouts
import chord

ApplicationWindow {
    id: root
    width: 1120
    height: 780
    visible: true
    title: page === "thread" && threadPage.subject !== "" ? threadPage.subject + " · Chord" : "Chord"
    color: theme.background

    Material.theme: theme.dark ? Material.Dark : Material.Light
    Material.accent: theme.accent
    Material.primary: theme.accent
    Material.background: theme.background
    Material.foreground: theme.foreground
    Material.roundedScale: theme.radius
    font.family: theme.font
    font.pixelSize: 14

    Theme { id: theme }
    Backend {
        id: backend
        active: root.active
    }

    // list | thread. A draft under review takes over either one.
    property string page: "list"
    readonly property bool reviewing: backend.review !== ""
    readonly property bool overlayOpen: picker.active || help.active || searchBar.active
    readonly property bool keysOn: !overlayOpen && backend.setup === "ready"

    Timer { interval: 2000; running: true; repeat: true; onTriggered: theme.poll() }
    Timer { interval: 2000; running: backend.setup !== "ready"; repeat: true; onTriggered: backend.checkSetup() }
    Timer { interval: 300; running: backend.composing; repeat: true; onTriggered: backend.pollCompose() }
    // The service syncs every minute; the open window keeps closer pace.
    Timer {
        interval: 30000
        running: backend.setup === "ready"
        repeat: true
        onTriggered: backend.syncNow()
    }
    onActiveChanged: if (active && backend.setup === "ready" && Date.now() - backend.lastSync > 15000) backend.syncNow()

    // ----- formatting shared by every page ---------------------------------

    function shortDate(ms) {
        if (!ms) return ""
        const d = new Date(ms), now = new Date()
        if (d.toDateString() === now.toDateString())
            return d.toLocaleTimeString(Qt.locale(), Locale.ShortFormat)
        if (d.getFullYear() === now.getFullYear())
            return d.toLocaleDateString(Qt.locale(), "MMM d")
        return d.toLocaleDateString(Qt.locale(), Locale.ShortFormat)
    }

    function longDate(ms) {
        if (!ms) return ""
        const d = new Date(ms)
        return d.toLocaleDateString(Qt.locale(), "ddd, MMM d") + " · " + d.toLocaleTimeString(Qt.locale(), Locale.ShortFormat)
    }

    function noticeText(kind, count, detail) {
        switch (kind) {
        case "archived": return count > 1 ? qsTr("%n conversations done", "", count) : qsTr("Done")
        case "inboxed": return qsTr("Moved to Inbox")
        case "trashed": return count > 1 ? qsTr("%n conversations trashed", "", count) : qsTr("Moved to Trash")
        case "untrashed": return qsTr("Restored from Trash")
        case "spammed": return qsTr("Marked as spam")
        case "starred": return qsTr("Starred")
        case "unstarred": return qsTr("Unstarred")
        case "unread": return qsTr("Marked unread")
        case "labeled": return qsTr("Labeled %1").arg(detail)
        case "unlabeled": return qsTr("Removed %1").arg(detail)
        case "moved": return qsTr("Moved to %1").arg(detail)
        case "snoozed": return qsTr("Snoozed until %1").arg(longDate(Number(detail)))
        case "woken": return qsTr("Back in the Inbox")
        case "undone": return qsTr("Undone")
        case "nothing-to-undo": return qsTr("Nothing to undo")
        case "sent": return qsTr("Sent")
        case "send-undone": return qsTr("Sending cancelled; the draft is back")
        case "scheduled": return qsTr("Scheduled for %1").arg(longDate(Number(detail)))
        case "draft-saved": return qsTr("Saved to Drafts")
        case "kept-local": return qsTr("Draft kept at %1").arg(detail)
        case "discarded": return qsTr("Draft discarded")
        case "already-composing": return qsTr("Finish the draft that's open in Neovim first (Shift+Esc stops waiting)")
        case "signed-out": return qsTr("Your Google sign-in expired; sign in again to keep syncing")
        case "saved": return qsTr("Saved %1 to Downloads").arg(detail)
        case "images-allowed": return qsTr("Images from %1 will always load").arg(detail)
        case "error": return detail
        }
        return kind
    }

    Connections {
        target: backend
        function onNotice(kind, count, detail, undoable) {
            toast.show(root.noticeText(kind, count, detail), undoable, kind === "error")
        }
        function onOpenRequested(threadId) {
            root.openThread(threadId)
        }
        function onQuitReady() {
            Qt.quit()
        }
    }

    // ----- navigation and actions ------------------------------------------

    function targets() {
        if (page === "list") {
            const sel = threadList.selectedIds()
            if (sel.length > 0) return sel
            const id = threadList.currentId()
            return id ? [id] : []
        }
        return threadPage.threadId ? [threadPage.threadId] : []
    }

    function openThread(id) {
        if (!id) return
        threadList.selectId(id)
        page = "thread"
        backend.openThread(id)
        threadPage.focusReader()
    }

    function backToList() {
        page = "list"
        backend.closeThread()
        threadList.forceActiveFocus()
    }

    function goTo(view) {
        if (page === "thread") backToList()
        threadList.clearSelection()
        backend.setViewTo(view)
        threadList.currentIndex = 0
    }

    // Runs an action on the targets. Actions that take a conversation out
    // of the list move on to the next one, so triage never stalls.
    function act(action, arg) {
        const ids = targets()
        if (ids.length === 0) return
        const leaves = ["archive", "trash", "spam", "move", "snooze", "inbox", "untrash"].indexOf(action) >= 0
        const before = page === "thread" ? threadPage.threadId : ""
        backend.act(action, ids.join(","), arg === undefined ? "" : String(arg))
        threadList.clearSelection()
        if (page === "thread" && leaves) {
            let next = threadList.currentId()
            if (next === before) {
                threadList.move(1)
                next = threadList.currentId()
            }
            if (next && next !== before) openThread(next)
            else backToList()
        }
    }

    function step(delta) {
        threadList.move(delta)
        if (page === "thread") openThread(threadList.currentId())
    }

    function compose(kind) {
        if (kind === "new") backend.compose("new", "", "")
        else if (page === "thread") backend.compose(kind, threadPage.currentMessageId(), "")
    }

    function quit() {
        if (backend.composing) {
            toast.show(qsTr("Finish the draft that's open in Neovim first (Shift+Esc stops waiting)"), false, false)
            return
        }
        if (reviewing) backend.saveDraft()
        root.close()
    }

    onClosing: function(close) {
        if (backend.sendCountdown > 0) {
            close.accepted = false
            backend.flushSend()
        }
    }

    // Commands for the palette, in the order it lists them.
    readonly property var commands: [
        { value: "archive", label: qsTr("Done"), hint: "E" },
        { value: "inbox", label: qsTr("Move to Inbox"), hint: "Shift+E" },
        { value: "trash", label: qsTr("Trash"), hint: "#" },
        { value: "spam", label: qsTr("Mark as spam"), hint: "!" },
        { value: "star", label: qsTr("Star or unstar"), hint: "S" },
        { value: "unread", label: qsTr("Mark unread"), hint: "Shift+U" },
        { value: "snooze", label: qsTr("Snooze"), hint: "H" },
        { value: "label", label: qsTr("Label"), hint: "L" },
        { value: "move", label: qsTr("Move to label"), hint: "V" },
        { value: "reply", label: qsTr("Reply"), hint: "R" },
        { value: "replyall", label: qsTr("Reply all"), hint: "A" },
        { value: "forward", label: qsTr("Forward"), hint: "F" },
        { value: "compose", label: qsTr("Compose"), hint: "C" },
        { value: "search", label: qsTr("Search"), hint: "/" },
        { value: "go:inbox", label: qsTr("Go to Inbox"), hint: "G I" },
        { value: "go:starred", label: qsTr("Go to Starred"), hint: "G S" },
        { value: "go:sent", label: qsTr("Go to Sent"), hint: "G T" },
        { value: "go:drafts", label: qsTr("Go to Drafts"), hint: "G D" },
        { value: "go:snoozed", label: qsTr("Go to Snoozed"), hint: "G H" },
        { value: "go:scheduled", label: qsTr("Go to Scheduled"), hint: "" },
        { value: "go:all", label: qsTr("Go to All Mail"), hint: "G A" },
        { value: "go:trash", label: qsTr("Go to Trash"), hint: "" },
        { value: "go:label", label: qsTr("Go to label…"), hint: "G L" },
        { value: "sync", label: qsTr("Sync now"), hint: "" },
        { value: "undo", label: qsTr("Undo"), hint: "Z" },
        { value: "help", label: qsTr("Keyboard shortcuts"), hint: "?" },
        { value: "quit", label: qsTr("Quit"), hint: "Q" }
    ]

    function run(command) {
        if (command.indexOf("go:") === 0) {
            if (command === "go:label") openLabelPicker("goto")
            else goTo(command.slice(3))
            return
        }
        switch (command) {
        case "star": toggleStar(); break
        case "snooze": openSnoozePicker(); break
        case "label": openLabelPicker("label"); break
        case "move": openLabelPicker("move"); break
        case "reply": case "replyall": case "forward": compose(command); break
        case "compose": compose("new"); break
        case "search": searchBar.open(); break
        case "sync": backend.syncNow(); break
        case "undo": backend.undo(); break
        case "help": help.open(); break
        case "quit": quit(); break
        default: act(command)
        }
    }

    function toggleStar() {
        const starred = page === "thread" ? threadPage.starred : threadList.currentStarred()
        act(starred ? "unstar" : "star")
    }

    function openSnoozePicker() {
        if (targets().length === 0) return
        const presets = JSON.parse(backend.snoozePresets())
        picker.open({
            mode: "snooze",
            title: qsTr("Snooze until"),
            placeholder: qsTr("tomorrow, fri 9am, 3d, jan 15…"),
            items: presets.map(p => ({ value: String(p.at), label: qsTr(p.label), hint: longDate(p.at) })),
            custom: text => {
                const at = backend.parseWhen(text)
                return at > 0 ? { value: String(at), label: qsTr("Snooze until %1").arg(longDate(at)) } : null
            }
        })
    }

    function openTimePicker(mode, title) {
        const presets = JSON.parse(backend.snoozePresets())
        picker.open({
            mode: mode,
            title: title,
            placeholder: qsTr("tomorrow 9am, mon, 2h…"),
            items: presets.map(p => ({ value: String(p.at), label: qsTr(p.label), hint: longDate(p.at) })),
            custom: text => {
                const at = backend.parseWhen(text)
                return at > 0 ? { value: String(at), label: longDate(at) } : null
            }
        })
    }

    function openLabelPicker(mode) {
        const labels = JSON.parse(backend.labels || "[]")
        const current = (page === "thread" ? threadPage.labelNames : threadList.currentLabels()).split(", ")
        picker.open({
            mode: mode,
            title: mode === "goto" ? qsTr("Go to label") : mode === "move" ? qsTr("Move to") : qsTr("Label"),
            placeholder: qsTr("Label name"),
            items: labels.map(l => ({
                value: l.id,
                label: l.name,
                hint: mode === "label" && current.indexOf(l.name) >= 0 ? "✓" : ""
            })),
            custom: mode === "goto" ? null : text => (text.trim() === "" || labels.some(l => l.name.toLowerCase() === text.trim().toLowerCase())) ? null
                : { value: "new:" + text.trim(), label: qsTr("Create label “%1”").arg(text.trim()) }
        })
    }

    function picked(mode, value, item) {
        switch (mode) {
        case "palette": run(value); break
        case "snooze": act("snooze", value); break
        case "goto": goTo("label:" + value); break
        case "label":
        case "move":
            if (value.indexOf("new:") === 0) {
                backend.createLabel(value.slice(4), targets().join(","), mode === "move")
                if (mode === "move" && page === "thread") backToList()
            } else if (mode === "label" && item.hint === "✓") act("unlabel", value)
            else act(mode, value)
            break
        case "later": reviewPage.sendLater(Number(value)); break
        case "remind": reviewPage.remindAt = Number(value); break
        case "discard": if (value === "yes") backend.discardDraft(); break
        }
    }

    // ----- layout ----------------------------------------------------------------

    SetupPage {
        anchors.fill: parent
        visible: backend.setup !== "ready"
    }

    ColumnLayout {
        anchors.fill: parent
        spacing: 0
        visible: backend.setup === "ready"

        Item {
            Layout.fillWidth: true
            Layout.fillHeight: true

            ThreadList {
                id: threadList
                anchors.fill: parent
                visible: root.page === "list" && !root.reviewing
                focus: visible
                onOpened: id => root.openThread(id)
            }
            ThreadPage {
                id: threadPage
                anchors.fill: parent
                visible: root.page === "thread" && !root.reviewing
            }
            ReviewPage {
                id: reviewPage
                anchors.fill: parent
                visible: root.reviewing
            }
        }

        StatusBar {
            Layout.fillWidth: true
            visible: !root.reviewing
        }
    }

    SearchBar {
        id: searchBar
        onSubmitted: text => {
            if (root.page === "thread") root.backToList()
            backend.search(text)
            threadList.currentIndex = 0
        }
    }

    Picker {
        id: picker
        onChosen: (mode, value, item) => root.picked(mode, value, item)
        onClosed: if (root.page === "thread") threadPage.focusReader(); else threadList.forceActiveFocus()
    }

    HelpOverlay { id: help }

    Toast { id: toast }

    // ----- keys --------------------------------------------------------------------
    // Every binding is an application-wide Shortcut, so they work even while
    // the message view has focus for scrolling. They switch off while a text
    // box is open so typing goes to the box.

    component Key: Shortcut {
        context: Qt.ApplicationShortcut
        autoRepeat: false
    }

    readonly property bool onList: keysOn && page === "list" && !reviewing
    readonly property bool onThread: keysOn && page === "thread" && !reviewing
    readonly property bool onMail: onList || onThread
    readonly property bool onReview: keysOn && reviewing

    Key { sequences: ["?", "Shift+?"]; enabled: root.keysOn; onActivated: help.open() }
    Key { sequences: ["Ctrl+K"]; enabled: root.keysOn && !root.reviewing; onActivated: picker.open({ mode: "palette", title: qsTr("Command"), placeholder: qsTr("Type a command"), items: root.commands }) }
    Key { sequences: ["/"]; enabled: root.onMail; onActivated: searchBar.open() }
    Key { sequences: ["C"]; enabled: root.onMail; onActivated: root.compose("new") }
    Key { sequences: ["Z", StandardKey.Undo]; enabled: root.keysOn; onActivated: backend.undo() }
    // For when Neovim's exit never comes back: stop waiting, keep the file.
    Key { sequences: ["Shift+Esc"]; enabled: root.keysOn && backend.composing; onActivated: backend.stopComposing() }
    Key { sequences: ["Q"]; enabled: root.onMail; onActivated: root.quit() }

    Key { sequences: ["G,I"]; enabled: root.onMail; onActivated: root.goTo("inbox") }
    Key { sequences: ["G,S"]; enabled: root.onMail; onActivated: root.goTo("starred") }
    Key { sequences: ["G,T"]; enabled: root.onMail; onActivated: root.goTo("sent") }
    Key { sequences: ["G,D"]; enabled: root.onMail; onActivated: root.goTo("drafts") }
    Key { sequences: ["G,H"]; enabled: root.onMail; onActivated: root.goTo("snoozed") }
    Key { sequences: ["G,A"]; enabled: root.onMail; onActivated: root.goTo("all") }
    Key { sequences: ["G,L"]; enabled: root.onMail; onActivated: root.openLabelPicker("goto") }

    Key { sequences: ["J", "Down"]; enabled: root.onList; autoRepeat: true; onActivated: root.step(1) }
    Key { sequences: ["K", "Up"]; enabled: root.onList; autoRepeat: true; onActivated: root.step(-1) }
    Key { sequences: ["J"]; enabled: root.onThread; onActivated: root.step(1) }
    Key { sequences: ["K"]; enabled: root.onThread; onActivated: root.step(-1) }
    Key { sequences: ["Return", "Enter", "O", "Space"]; enabled: root.onList; onActivated: root.openThread(threadList.currentId()) }
    Key { sequences: ["Tab"]; enabled: root.onList && backend.view === "inbox"; onActivated: backend.setSplitTo(backend.split + 1) }
    Key { sequences: ["Shift+Tab", "Backtab"]; enabled: root.onList && backend.view === "inbox"; onActivated: backend.setSplitTo(backend.split - 1) }
    Key { sequences: ["X"]; enabled: root.onList; onActivated: threadList.toggleSelected() }
    Key { sequences: ["Ctrl+A"]; enabled: root.onList; onActivated: threadList.selectAll() }
    Key {
        sequences: ["Escape"]
        enabled: root.onMail
        onActivated: {
            if (root.page === "thread") root.backToList()
            else if (threadList.selectedIds().length > 0) threadList.clearSelection()
            else if (backend.view !== "inbox") root.goTo("inbox")
        }
    }

    Key { sequences: ["E"]; enabled: root.onMail; onActivated: root.act(backend.view === "trash" ? "untrash" : "archive") }
    Key { sequences: ["Shift+E"]; enabled: root.onMail; onActivated: root.act(backend.view === "snoozed" ? "wake" : "inbox") }
    Key { sequences: ["#", "Shift+#", "Shift+3", "Delete"]; enabled: root.onMail; onActivated: root.act("trash") }
    Key { sequences: ["!", "Shift+!", "Shift+1"]; enabled: root.onMail; onActivated: root.act("spam") }
    Key { sequences: ["S"]; enabled: root.onMail; onActivated: root.toggleStar() }
    Key {
        sequences: ["Shift+U"]
        enabled: root.onMail
        onActivated: {
            root.act("unread")
            if (root.page === "thread") root.backToList()
        }
    }
    Key { sequences: ["H"]; enabled: root.onMail; onActivated: root.openSnoozePicker() }
    Key { sequences: ["L"]; enabled: root.onMail; onActivated: root.openLabelPicker("label") }
    Key { sequences: ["V"]; enabled: root.onMail; onActivated: root.openLabelPicker("move") }

    Key { sequences: ["R"]; enabled: root.onThread; onActivated: root.compose("reply") }
    Key { sequences: ["A"]; enabled: root.onThread; onActivated: root.compose("replyall") }
    Key { sequences: ["F"]; enabled: root.onThread; onActivated: root.compose("forward") }
    Key { sequences: ["N"]; enabled: root.onThread; onActivated: threadPage.stepMessage(1) }
    Key { sequences: ["P"]; enabled: root.onThread; onActivated: threadPage.stepMessage(-1) }
    Key { sequences: ["I"]; enabled: root.onThread; onActivated: threadPage.loadImages(false) }
    Key { sequences: ["Shift+I"]; enabled: root.onThread; onActivated: threadPage.loadImages(true) }
    Key { sequences: ["Shift+D"]; enabled: root.onThread; onActivated: threadPage.toggleOriginal() }
    Key { sequences: ["."]; enabled: root.onThread; onActivated: threadPage.toggleQuotes() }
    Key { sequences: ["Shift+S"]; enabled: root.onThread; onActivated: threadPage.saveAttachments() }

    Key { sequences: ["Ctrl+Return", "Ctrl+Enter"]; enabled: root.onReview && reviewPage.canSend; onActivated: reviewPage.send() }
    Key { sequences: ["E"]; enabled: root.onReview; onActivated: backend.editDraft() }
    Key { sequences: ["L"]; enabled: root.onReview && reviewPage.canSend; onActivated: root.openTimePicker("later", qsTr("Send later")) }
    Key {
        sequences: ["R"]
        enabled: root.onReview
        onActivated: reviewPage.remindAt > 0 ? reviewPage.remindAt = 0 : root.openTimePicker("remind", qsTr("Remind me if no reply by"))
    }
    Key { sequences: ["Escape"]; enabled: root.onReview; onActivated: backend.saveDraft() }
    Key {
        sequences: ["Shift+D"]
        enabled: root.onReview
        onActivated: picker.open({
            mode: "discard",
            title: qsTr("Discard this draft?"),
            placeholder: "",
            items: [{ value: "no", label: qsTr("Keep editing") }, { value: "yes", label: qsTr("Discard") }]
        })
    }

    // ----- screenshot tour (CHORD_SNAPSHOT, development only) ---------------------

    property int tourStep: 0
    // Each step presses real keys, then the window is captured.
    readonly property var tour: [
        ["inbox", ""],
        ["select", "J X K"],
        ["thread-html", "Escape Return"],
        ["images", "I"],
        ["conversation", "Escape J J Return"],
        ["quotes", "."],
        ["help", "Escape ?"],
        ["palette", "Escape Ctrl+K"],
        ["palette-filter", "S N O"],
        ["snooze", "Escape H"],
        ["snooze-typed", "F R I Space 9 A M"],
        ["updates", "Escape Tab"],
        ["done", "Shift+Tab E"],
        ["trash", "#"],
        ["undo", "Z"],
        ["starred", "G S"],
        ["labels", "G I L"],
        ["search", "Escape / L U N C H Return"],
        ["compose", "Escape Escape G I C"],
        ["remind", "R T O M Return"],
        ["sending", "Ctrl+Return"],
        ["unsent", "Z"],
        ["zero", "Escape G I E E E E E E"]
    ]
    Timer {
        running: backend.snapshot !== ""
        interval: 1600
        repeat: true
        onTriggered: {
            if (root.tourStep > 0) {
                const name = root.tour[root.tourStep - 1][0]
                backend.grab(backend.snapshot + "/" + String(root.tourStep).padStart(2, "0") + "-" + name + ".png")
            }
            if (root.tourStep >= root.tour.length) {
                Qt.callLater(Qt.quit)
                stop()
                return
            }
            backend.press(root.tour[root.tourStep][1])
            root.tourStep++
        }
    }
}

import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import QtWebEngine

// An open conversation. Every message is a compact row; the selected one
// renders below in a locked-down web view (no JavaScript, remote content
// blocked by the page's policy, links open in the browser).
FocusScope {
    id: page

    readonly property var info: backend.thread !== "" ? JSON.parse(backend.thread) : ({ messages: [] })
    readonly property string threadId: info.id || ""
    readonly property string subject: info.subject || ""
    readonly property bool starred: !!info.starred
    readonly property string labelNames: info.labels || ""
    readonly property var messages: info.messages || []

    property int current: messages.length - 1
    property var message: messages[current] || null
    property var rendered: ({ ready: false })
    // Per-conversation reading choices; they reset when another opens.
    property var remoteAllowed: ({})
    property bool showQuotes: false
    property bool original: false
    property string shownThread: ""

    onThreadIdChanged: {
        if (threadId === shownThread) return
        shownThread = threadId
        remoteAllowed = {}
        showQuotes = false
        original = false
        current = Math.max(0, newest())
        Qt.callLater(load)
    }
    onMessagesChanged: {
        if (current >= messages.length) current = messages.length - 1
        if (!rendered.ready) Qt.callLater(load)
    }
    onCurrentChanged: Qt.callLater(load)

    // The newest message you haven't read, or the newest overall.
    function newest() {
        for (let i = 0; i < messages.length; i++)
            if (messages[i].unread && !messages[i].me) return i
        for (let j = messages.length - 1; j >= 0; j--)
            if (!messages[j].draft) return j
        return messages.length - 1
    }

    function currentMessageId() {
        return message ? message.id : ""
    }

    function stepMessage(delta) {
        current = Math.max(0, Math.min(messages.length - 1, current + delta))
        list.positionViewAtIndex(current, ListView.Contain)
    }

    function focusReader() {
        web.forceActiveFocus()
    }

    function load() {
        if (!message) {
            rendered = { ready: false }
            return
        }
        const style = JSON.stringify({ text: theme.foreground, link: theme.accent, muted: theme.muted, font: theme.font })
        rendered = JSON.parse(backend.messageHtml(message.id, !!remoteAllowed[message.id], showQuotes, style))
        if (!rendered.ready) return
        if (rendered.url) web.url = rendered.url
        else web.loadHtml(rendered.html, "https://chord.invalid/")
    }

    function loadImages(always) {
        if (!message) return
        if (always && rendered.sender) backend.allowImages(rendered.sender)
        const a = Object.assign({}, remoteAllowed)
        a[message.id] = true
        remoteAllowed = a
        load()
    }

    function toggleQuotes() {
        showQuotes = !showQuotes
        load()
    }

    function toggleOriginal() {
        original = !original
    }

    function saveAttachments() {
        if (!message || !rendered.attachments) return
        rendered.attachments.forEach(a => backend.saveAttachment(message.id, a.index))
    }

    Connections {
        target: backend
        function onBodyReady(id) {
            if (page.message && id === page.message.id && !page.rendered.ready) page.load()
        }
    }
    Connections {
        target: theme
        function onForegroundChanged() { page.load() }
    }

    WebEngineProfilePrototype {
        id: readerProfile
        storageName: ""
        httpCacheType: WebEngineProfile.MemoryHttpCache
        persistentCookiesPolicy: WebEngineProfile.NoPersistentCookies
    }

    ColumnLayout {
        anchors.fill: parent
        spacing: 0

        // Subject and labels.
        Item {
            Layout.fillWidth: true
            Layout.preferredHeight: 60

            RowLayout {
                anchors.fill: parent
                anchors.leftMargin: 28
                anchors.rightMargin: 28
                spacing: 12

                Text {
                    Layout.fillWidth: true
                    text: page.subject !== "" ? page.subject : qsTr("(no subject)")
                    color: theme.bright
                    font.family: theme.font
                    font.pixelSize: 19
                    font.weight: Font.DemiBold
                    elide: Text.ElideRight
                }
                Text {
                    visible: page.labelNames !== ""
                    text: page.labelNames
                    color: theme.accent
                    font.family: theme.font
                    font.pixelSize: 12
                }
                Text {
                    visible: page.info.snoozed > 0
                    text: "⏾ " + root.longDate(page.info.snoozed)
                    color: theme.accent
                    font.family: theme.font
                    font.pixelSize: 12
                }
                Text {
                    visible: page.starred
                    text: "★"
                    color: theme.yellow
                    font.pixelSize: 16
                }
            }
        }

        // The conversation's messages; hidden when there's only one.
        ListView {
            id: list
            Layout.fillWidth: true
            Layout.preferredHeight: page.messages.length > 1 ? Math.min(contentHeight, page.height * 0.3) : 0
            visible: page.messages.length > 1
            clip: true
            model: page.messages
            interactive: contentHeight > height
            currentIndex: page.current
            boundsBehavior: Flickable.StopAtBounds

            delegate: Rectangle {
                required property var modelData
                required property int index
                width: list.width
                height: 34
                color: index === page.current ? theme.selection : "transparent"

                Rectangle {
                    width: 3
                    height: parent.height
                    color: theme.accent
                    visible: index === page.current
                }
                RowLayout {
                    anchors.fill: parent
                    anchors.leftMargin: 28
                    anchors.rightMargin: 28
                    spacing: 12
                    Text {
                        Layout.preferredWidth: 180
                        text: modelData.draft ? qsTr("Draft") : modelData.me ? qsTr("me") : modelData.fromName
                        color: modelData.draft ? theme.red : modelData.unread ? theme.bright : theme.foreground
                        font.family: theme.font
                        font.pixelSize: 13
                        font.weight: modelData.unread ? Font.DemiBold : Font.Normal
                        elide: Text.ElideRight
                    }
                    Text {
                        Layout.fillWidth: true
                        text: modelData.snippet
                        color: theme.muted
                        font.family: theme.font
                        font.pixelSize: 13
                        elide: Text.ElideRight
                    }
                    Text {
                        text: root.shortDate(modelData.date)
                        color: theme.muted
                        font.family: theme.font
                        font.pixelSize: 12
                    }
                }
                MouseArea {
                    anchors.fill: parent
                    onClicked: page.current = index
                }
            }
        }

        Rectangle {
            Layout.fillWidth: true
            Layout.preferredHeight: 1
            color: theme.selection
            visible: page.messages.length > 1
        }

        // Who wrote the selected message, to whom, and when.
        Item {
            Layout.fillWidth: true
            Layout.preferredHeight: header.implicitHeight + 24
            visible: page.message !== null

            ColumnLayout {
                id: header
                anchors.left: parent.left
                anchors.right: parent.right
                anchors.top: parent.top
                anchors.topMargin: 14
                anchors.leftMargin: 28
                anchors.rightMargin: 28
                spacing: 3

                RowLayout {
                    Layout.fillWidth: true
                    spacing: 10
                    Text {
                        text: page.message ? page.message.fromName : ""
                        color: theme.bright
                        font.family: theme.font
                        font.pixelSize: 14
                        font.weight: Font.DemiBold
                    }
                    Text {
                        Layout.fillWidth: true
                        text: page.message ? page.message.address : ""
                        color: theme.muted
                        font.family: theme.font
                        font.pixelSize: 12
                        elide: Text.ElideRight
                    }
                    Text {
                        text: page.message ? root.longDate(page.message.date) : ""
                        color: theme.muted
                        font.family: theme.font
                        font.pixelSize: 12
                    }
                }
                Text {
                    Layout.fillWidth: true
                    text: page.message ? qsTr("to %1").arg(page.message.to + (page.message.cc ? ", " + page.message.cc : "")) : ""
                    color: theme.muted
                    font.family: theme.font
                    font.pixelSize: 12
                    elide: Text.ElideRight
                }

                // What the reader is holding back, and the keys that bring it.
                Row {
                    spacing: 18
                    topPadding: 4
                    visible: !!page.rendered.ready && (!!page.rendered.hasRemote && !page.rendered.imagesAllowed || !!page.rendered.hasQuotes)
                    Text {
                        visible: !!page.rendered.hasRemote && !page.rendered.imagesAllowed
                        text: qsTr("Remote images blocked · I load · Shift+I always for this sender")
                        color: theme.yellow
                        font.family: theme.font
                        font.pixelSize: 12
                    }
                    Text {
                        visible: !!page.rendered.hasQuotes
                        text: page.showQuotes ? qsTr(". hide quoted text") : qsTr(". show quoted text")
                        color: theme.muted
                        font.family: theme.font
                        font.pixelSize: 12
                    }
                }
            }
        }

        Item {
            Layout.fillWidth: true
            Layout.fillHeight: true

            WebEngineView {
                id: web
                anchors.fill: parent
                anchors.leftMargin: 24
                anchors.rightMargin: 24
                visible: page.rendered.ready
                backgroundColor: "transparent"
                settings.javascriptEnabled: false
                settings.javascriptCanOpenWindows: false
                settings.localContentCanAccessRemoteUrls: false
                settings.localContentCanAccessFileUrls: false
                settings.pluginsEnabled: false
                settings.focusOnNavigationEnabled: false
                settings.forceDarkMode: theme.dark && !page.original && !page.rendered.plain
                // No storage name means off the record: nothing a message
                // loads is kept on disk. The prototype only has an instance
                // once construction finishes.
                Component.onCompleted: {
                    const p = readerProfile.instance()
                    if (p) profile = p
                }

                // Mail never navigates inside Chord: links go to the browser.
                onNavigationRequested: function(request) {
                    const url = String(request.url)
                    if (request.navigationType === WebEngineNavigationRequest.LinkClickedNavigation) {
                        request.reject()
                        if (url.indexOf("http") === 0 || url.indexOf("mailto:") === 0)
                            Qt.openUrlExternally(request.url)
                        return
                    }
                    if (url.indexOf("https://chord.invalid/") === 0 || url.indexOf("data:") === 0
                        || url.indexOf("file://") === 0 && url.indexOf("/chord-mail/") > 0)
                        request.accept()
                    else
                        request.reject()
                }
                onNewWindowRequested: function(request) {
                    Qt.openUrlExternally(request.requestedUrl)
                }
            }

            Text {
                anchors.centerIn: parent
                visible: !page.rendered.ready && page.message !== null
                text: backend.setup === "ready" ? qsTr("Downloading…") : page.message ? page.message.snippet : ""
                color: theme.muted
                font.family: theme.font
                font.pixelSize: 13
            }
        }

        // Attachments: Shift+S saves them all to Downloads.
        Flow {
            Layout.fillWidth: true
            Layout.leftMargin: 28
            Layout.rightMargin: 28
            Layout.bottomMargin: 10
            Layout.topMargin: 6
            spacing: 10
            visible: page.rendered.ready && page.rendered.attachments && page.rendered.attachments.length > 0

            Text {
                text: qsTr("Shift+S save all")
                color: theme.muted
                font.family: theme.font
                font.pixelSize: 12
                topPadding: 5
            }
            Repeater {
                model: page.rendered.attachments || []
                delegate: Rectangle {
                    required property var modelData
                    width: chip.implicitWidth + 20
                    height: 26
                    radius: Math.min(4, theme.radius)
                    color: theme.raised
                    Text {
                        id: chip
                        anchors.centerIn: parent
                        text: modelData.name + "  " + (modelData.size > 1048576 ? (modelData.size / 1048576).toLocaleString(Qt.locale(), 'f', 1) + " MB"
                                                                                 : Math.max(1, Math.round(modelData.size / 1024)) + " KB")
                        color: theme.foreground
                        font.family: theme.font
                        font.pixelSize: 12
                    }
                    MouseArea {
                        anchors.fill: parent
                        cursorShape: Qt.PointingHandCursor
                        onClicked: backend.saveAttachment(page.message.id, modelData.index)
                    }
                }
            }
        }
    }
}

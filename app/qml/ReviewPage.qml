import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import QtWebEngine

// The draft as it will go out, after Neovim closes. Nothing is sent until
// Ctrl+Enter, and even then it waits out the undo window.
Item {
    id: page

    readonly property var draft: backend.review !== "" ? JSON.parse(backend.review) : ({})
    readonly property bool canSend: !!draft.canSend
    property real remindAt: 0

    onDraftChanged: {
        if (backend.review === "") return
        remindAt = draft.remind || 0
        preview.loadHtml(draft.html || "", "https://chord.invalid/")
    }

    function send() {
        backend.sendDraft(0, remindAt)
    }
    function sendLater(at) {
        backend.sendDraft(at, remindAt)
    }

    component Field: RowLayout {
        property string label
        property string value
        property color valueColor: theme.bright
        Layout.fillWidth: true
        visible: value !== ""
        spacing: 14
        Text {
            Layout.preferredWidth: 70
            text: parent.label
            color: theme.muted
            font.family: theme.font
            font.pixelSize: 13
        }
        Text {
            Layout.fillWidth: true
            textFormat: Text.PlainText
            text: parent.value
            color: parent.valueColor
            font.family: theme.font
            font.pixelSize: 13
            wrapMode: Text.Wrap
        }
    }

    ColumnLayout {
        anchors.fill: parent
        spacing: 0

        ColumnLayout {
            Layout.fillWidth: true
            Layout.margins: 28
            spacing: 6

            Text {
                text: page.draft.kind === "reply" || page.draft.kind === "replyall" ? qsTr("Review reply")
                    : page.draft.kind === "forward" ? qsTr("Review forward") : qsTr("Review message")
                color: theme.bright
                font.family: theme.font
                font.pixelSize: 19
                font.weight: Font.DemiBold
                Layout.bottomMargin: 8
            }
            Field { label: qsTr("From"); value: page.draft.from || ""; valueColor: theme.foreground }
            Field { label: qsTr("To"); value: page.draft.to || "" }
            Field { label: qsTr("Cc"); value: page.draft.cc || "" }
            Field { label: qsTr("Bcc"); value: page.draft.bcc || "" }
            Field { label: qsTr("Subject"); value: page.draft.subject || qsTr("(no subject)") }
            Field {
                label: qsTr("Attached")
                value: (page.draft.attachments || []).map(a => a.missing ? qsTr("%1 (not found)").arg(a.name) : a.name).join(", ")
                valueColor: (page.draft.attachments || []).some(a => a.missing) ? theme.red : theme.foreground
            }
            Field {
                label: qsTr("Remind")
                value: page.remindAt > 0 ? qsTr("Back in the Inbox %1 unless someone replies").arg(root.longDate(page.remindAt)) : ""
                valueColor: theme.accent
            }

            // Problems that keep the draft from sending.
            Text {
                visible: !!page.draft.noRecipients
                text: qsTr("Add a recipient: press E to edit.")
                color: theme.red
                font.family: theme.font
                font.pixelSize: 13
                Layout.topMargin: 6
            }
            Repeater {
                model: page.draft.unresolved || []
                delegate: Text {
                    required property var modelData
                    Layout.topMargin: 6
                    Layout.fillWidth: true
                    wrapMode: Text.Wrap
                    textFormat: Text.PlainText
                    text: modelData.suggestions.length > 0
                          ? qsTr("“%1” matches several people: %2. Press E and pick one.").arg(modelData.entry).arg(modelData.suggestions.join("; "))
                          : qsTr("“%1” isn't an email address. Press E to fix it.").arg(modelData.entry)
                    color: theme.red
                    font.family: theme.font
                    font.pixelSize: 13
                }
            }
        }

        Rectangle {
            Layout.fillWidth: true
            Layout.preferredHeight: 1
            color: theme.selection
        }

        WebEngineView {
            id: preview
            Layout.fillWidth: true
            Layout.fillHeight: true
            Layout.leftMargin: 24
            Layout.rightMargin: 24
            Layout.topMargin: 12
            backgroundColor: "transparent"
            settings.javascriptEnabled: false
            settings.forceDarkMode: theme.dark
            settings.focusOnNavigationEnabled: false
            onNavigationRequested: function(request) {
                // Only the load of the preview itself; nothing the mail asks for.
                if (request.navigationType !== WebEngineNavigationRequest.LinkClickedNavigation
                    && (String(request.url).indexOf("data:") === 0 || String(request.url) === "https://chord.invalid/"))
                    request.accept()
                else
                    request.reject()
            }
        }

        Rectangle {
            Layout.fillWidth: true
            Layout.preferredHeight: 44
            color: theme.surface

            Row {
                anchors.centerIn: parent
                spacing: 26
                Repeater {
                    model: [
                        { key: "Ctrl+Enter", label: qsTr("send"), on: page.canSend },
                        { key: "L", label: qsTr("send later"), on: page.canSend },
                        { key: "R", label: page.remindAt > 0 ? qsTr("no reminder") : qsTr("remind me"), on: true },
                        { key: "E", label: qsTr("edit"), on: true },
                        { key: "Esc", label: qsTr("save draft"), on: true },
                        { key: "Shift+D", label: qsTr("discard"), on: true }
                    ]
                    delegate: Row {
                        required property var modelData
                        spacing: 6
                        opacity: modelData.on ? 1 : 0.35
                        Text { text: modelData.key; color: theme.accent; font.family: theme.font; font.pixelSize: 12 }
                        Text { text: modelData.label; color: theme.foreground; font.family: theme.font; font.pixelSize: 12 }
                    }
                }
            }
        }
    }
}

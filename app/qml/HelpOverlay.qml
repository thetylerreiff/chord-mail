import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

// "?" shows every key, grouped the way you'd reach for them.
Popup {
    id: help

    // True from the moment it starts opening until it starts closing, so
    // keys pressed right after Esc reach the shortcuts again.
    property bool active: false
    onAboutToShow: active = true
    onAboutToHide: active = false
    enter: Transition {}
    exit: Transition {}

    parent: Overlay.overlay
    anchors.centerIn: parent
    width: Math.min(1060, parent.width - 48)
    height: Math.min(grid.implicitHeight + 72, parent.height - 48)
    padding: 28
    modal: true
    focus: true
    closePolicy: Popup.CloseOnEscape | Popup.CloseOnPressOutside
    Overlay.modal: Rectangle { color: Qt.alpha(theme.surface, 0.6) }

    background: Rectangle {
        color: theme.raised
        radius: theme.radius
        border.color: theme.selection
    }

    // Any key closes it, so ? is a quick peek.
    contentItem: Flickable {
        clip: true
        contentHeight: grid.implicitHeight
        focus: true
        Keys.onPressed: function(event) {
            help.close()
            event.accepted = true
        }

        GridLayout {
            id: grid
            width: parent.width
            columns: width > 940 ? 3 : width > 600 ? 2 : 1
            columnSpacing: 36
            rowSpacing: 22

            Repeater {
                model: [
                    { title: qsTr("Move"), keys: [
                        ["J  K", qsTr("Next / previous conversation")],
                        ["Enter", qsTr("Open")],
                        ["Esc", qsTr("Back")],
                        ["N  P", qsTr("Next / previous message")],
                        ["Tab", qsTr("Next split")],
                        ["X", qsTr("Select")],
                        ["Ctrl+A", qsTr("Select all")]
                    ]},
                    { title: qsTr("Triage"), keys: [
                        ["E", qsTr("Done")],
                        ["Shift+E", qsTr("Back to Inbox")],
                        ["H", qsTr("Snooze")],
                        ["S", qsTr("Star")],
                        ["#", qsTr("Trash")],
                        ["!", qsTr("Spam")],
                        ["L", qsTr("Label")],
                        ["V", qsTr("Move to label")],
                        ["Shift+U", qsTr("Mark unread")],
                        ["Z", qsTr("Undo")]
                    ]},
                    { title: qsTr("Write"), keys: [
                        ["C", qsTr("Compose")],
                        ["R", qsTr("Reply")],
                        ["A", qsTr("Reply all")],
                        ["F", qsTr("Forward")],
                        ["Ctrl+Enter", qsTr("Send, after review")],
                        ["L", qsTr("Send later, in review")],
                        ["R", qsTr("Remind me, in review")]
                    ]},
                    { title: qsTr("Go to"), keys: [
                        ["G I", qsTr("Inbox")],
                        ["G S", qsTr("Starred")],
                        ["G T", qsTr("Sent")],
                        ["G D", qsTr("Drafts")],
                        ["G H", qsTr("Snoozed")],
                        ["G A", qsTr("All Mail")],
                        ["G L", qsTr("A label")],
                        ["/", qsTr("Search")]
                    ]},
                    { title: qsTr("Read"), keys: [
                        ["I", qsTr("Load images")],
                        ["Shift+I", qsTr("Always load from sender")],
                        [".", qsTr("Show quoted text")],
                        ["Shift+D", qsTr("Original colors")],
                        ["Shift+S", qsTr("Save attachments")],
                        ["Space", qsTr("Scroll")]
                    ]},
                    { title: qsTr("Chord"), keys: [
                        ["Ctrl+K", qsTr("Command palette")],
                        ["?", qsTr("These keys")],
                        ["Q", qsTr("Quit")]
                    ]}
                ]
                delegate: ColumnLayout {
                    required property var modelData
                    Layout.alignment: Qt.AlignTop
                    Layout.fillWidth: true
                    spacing: 6
                    Text {
                        text: modelData.title
                        color: theme.accent
                        font.family: theme.font
                        font.pixelSize: 13
                        font.weight: Font.DemiBold
                        Layout.bottomMargin: 4
                    }
                    Repeater {
                        model: modelData.keys
                        delegate: RowLayout {
                            required property var modelData
                            spacing: 12
                            Text {
                                Layout.preferredWidth: 84
                                text: modelData[0]
                                color: theme.bright
                                font.family: theme.font
                                font.pixelSize: 13
                            }
                            Text {
                                text: modelData[1]
                                color: theme.foreground
                                font.family: theme.font
                                font.pixelSize: 13
                            }
                        }
                    }
                }
            }
        }
    }
}

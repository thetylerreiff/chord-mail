import QtQuick
import QtQuick.Layouts

// The bottom line: the keys that matter on this page, and whether mail is
// current.
Rectangle {
    id: bar
    height: 30
    color: theme.surface

    readonly property var hints: root.reviewing ? [] : root.page === "thread"
        ? [["E", qsTr("done")], ["R", qsTr("reply")], ["H", qsTr("snooze")], ["J K", qsTr("next")], ["Esc", qsTr("back")], ["?", qsTr("keys")]]
        : [["Enter", qsTr("open")], ["E", qsTr("done")], ["H", qsTr("snooze")], ["C", qsTr("compose")], ["/", qsTr("search")], ["Ctrl+K", qsTr("commands")], ["?", qsTr("keys")]]

    readonly property string status: {
        if (backend.composing) return qsTr("Writing in Neovim…")
        if (backend.syncing) return qsTr("Syncing…")
        if (backend.loading) return qsTr("Loading…")
        const waiting = backend.pending > 0 ? qsTr("%n waiting to sync", "", backend.pending) : ""
        if (backend.syncError !== "") return waiting !== "" ? qsTr("Offline · %1").arg(waiting) : qsTr("Offline")
        if (backend.lastSync > 0) {
            const t = new Date(backend.lastSync).toLocaleTimeString(Qt.locale(), Locale.ShortFormat)
            return waiting !== "" ? qsTr("Synced %1 · %2").arg(t).arg(waiting) : qsTr("Synced %1").arg(t)
        }
        return ""
    }

    RowLayout {
        anchors.fill: parent
        anchors.leftMargin: 28
        anchors.rightMargin: 28
        spacing: 18

        Repeater {
            model: bar.hints
            delegate: Row {
                required property var modelData
                spacing: 6
                Text { text: modelData[0]; color: theme.accent; font.family: theme.font; font.pixelSize: 11 }
                Text { text: modelData[1]; color: theme.muted; font.family: theme.font; font.pixelSize: 11 }
            }
        }

        Item { Layout.fillWidth: true }

        Text {
            text: bar.status
            color: backend.syncError !== "" ? theme.yellow : theme.muted
            font.family: theme.font
            font.pixelSize: 11
        }
    }

    // The full error is a hover away, for when "Offline" isn't the story.
    HoverHandler { id: hover }
    ToolTipText {
        visible: hover.hovered && backend.syncError !== ""
        text: backend.syncError
    }

    component ToolTipText: Rectangle {
        property alias text: label.text
        anchors.bottom: parent.top
        anchors.right: parent.right
        anchors.rightMargin: 20
        width: Math.min(label.implicitWidth + 20, bar.width - 40)
        height: label.implicitHeight + 12
        color: theme.raised
        border.color: theme.selection
        radius: Math.min(4, theme.radius)
        Text {
            id: label
            anchors.centerIn: parent
            width: parent.width - 20
            wrapMode: Text.Wrap
            color: theme.foreground
            font.family: theme.font
            font.pixelSize: 11
        }
    }
}

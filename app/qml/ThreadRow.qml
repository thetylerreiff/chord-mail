import QtQuick
import QtQuick.Layouts

// One conversation in the list: who, what, a line of the latest message, and
// when. Unread rows are bright and marked with the accent.
Rectangle {
    id: row

    required property int index
    required property var model
    property bool current: false
    property bool selected: false

    signal clicked()
    signal doubleClicked()

    height: 46
    color: current ? theme.selection : selected ? Qt.alpha(theme.accent, 0.10) : "transparent"

    Rectangle {
        width: 3
        height: parent.height
        color: theme.accent
        visible: row.current || row.selected
        opacity: row.current ? 1 : 0.5
    }

    RowLayout {
        anchors.fill: parent
        anchors.leftMargin: 18
        anchors.rightMargin: 28
        spacing: 14

        Rectangle {
            Layout.preferredWidth: 6
            Layout.preferredHeight: 6
            radius: 3
            color: theme.accent
            opacity: row.model.unread ? 1 : 0
        }

        Text {
            Layout.preferredWidth: Math.min(220, row.width * 0.22)
            text: row.model.senders + (row.model.count > 1 ? "  " + row.model.count : "")
            color: row.model.unread ? theme.bright : theme.foreground
            font.family: theme.font
            font.pixelSize: 14
            font.weight: row.model.unread ? Font.DemiBold : Font.Normal
            elide: Text.ElideRight
        }

        // Subject, then the snippet trailing off in the muted color.
        Text {
            Layout.fillWidth: true
            textFormat: Text.StyledText
            text: {
                const esc = s => String(s).replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;")
                const subject = row.model.subject !== "" ? row.model.subject : qsTr("(no subject)")
                const draft = row.model.draft ? "<font color='" + theme.red + "'>" + qsTr("Draft") + "</font>  " : ""
                const s = esc(subject)
                return draft + "<font color='" + (row.model.unread ? theme.bright : theme.foreground) + "'>"
                    + (row.model.unread ? "<b>" + s + "</b>" : s) + "</font>"
                    + "&nbsp;&nbsp;<font color='" + theme.muted + "'>" + esc(row.model.snippet) + "</font>"
            }
            font.family: theme.font
            font.pixelSize: 14
            elide: Text.ElideRight
            maximumLineCount: 1
        }

        Text {
            visible: row.model.labels !== ""
            Layout.maximumWidth: 180
            text: row.model.labels
            color: theme.accent
            font.family: theme.font
            font.pixelSize: 11
            elide: Text.ElideRight
        }

        Text {
            visible: row.model.starred
            text: "★"
            color: theme.yellow
            font.pixelSize: 13
        }

        Text {
            Layout.preferredWidth: 78
            horizontalAlignment: Text.AlignRight
            text: row.model.snoozed > 0 ? "⏾ " + root.shortDate(row.model.snoozed) : root.shortDate(row.model.date)
            color: row.model.snoozed > 0 ? theme.accent : row.model.unread ? theme.bright : theme.muted
            font.family: theme.font
            font.pixelSize: 12
        }
    }

    MouseArea {
        anchors.fill: parent
        onClicked: row.clicked()
        onDoubleClicked: row.doubleClicked()
    }
}

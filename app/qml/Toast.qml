import QtQuick
import QtQuick.Controls

// What just happened, with the undo key when there is one. While a sent
// message waits out its undo window, the countdown stays up instead.
Rectangle {
    id: toast

    property string text
    property bool undoable
    property bool error

    function show(message, canUndo, isError) {
        text = message
        undoable = canUndo
        error = isError
        shown = true
        hide.restart()
    }

    property bool shown: false
    readonly property bool sending: backend.sendCountdown > 0
    readonly property string line: sending ? qsTr("Sending in %n s", "", backend.sendCountdown) : text
    readonly property bool canUndo: sending || undoable

    parent: Overlay.overlay
    anchors.horizontalCenter: parent.horizontalCenter
    y: parent.height - height - 48
    width: Math.min(row.implicitWidth + 36, parent.width - 48)
    height: 40
    radius: theme.radius
    color: error ? Qt.darker(theme.red, 2.6) : theme.raised
    border.color: error ? theme.red : theme.selection
    opacity: shown || sending ? 1 : 0
    visible: opacity > 0
    Behavior on opacity { NumberAnimation { duration: 140 } }

    Timer {
        id: hide
        interval: toast.error ? 8000 : 4000
        onTriggered: toast.shown = false
    }

    Row {
        id: row
        anchors.centerIn: parent
        spacing: 16
        Text {
            text: toast.line
            color: toast.error ? theme.bright : theme.bright
            font.family: theme.font
            font.pixelSize: 13
            elide: Text.ElideRight
            width: Math.min(implicitWidth, toast.parent ? toast.parent.width - 180 : 600)
        }
        Text {
            visible: toast.canUndo
            text: qsTr("Z undo")
            color: theme.accent
            font.family: theme.font
            font.pixelSize: 13
        }
    }
}

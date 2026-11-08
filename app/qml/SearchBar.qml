import QtQuick
import QtQuick.Controls
import QtQuick.Controls.Material
import QtQuick.Layouts

// "/" search. Takes Gmail's syntax (from:ada has:attachment older_than:1y);
// cached matches show at once and Gmail's results replace them.
Popup {
    id: bar

    signal submitted(string text)

    function open() {
        field.text = backend.view === "search" ? backend.query : ""
        visible = true
        field.forceActiveFocus()
        field.selectAll()
    }

    // True from the moment it starts opening until it starts closing, so
    // keys pressed right after Esc reach the shortcuts again.
    property bool active: false
    onAboutToShow: active = true
    onAboutToHide: active = false
    enter: Transition {}
    exit: Transition {}

    parent: Overlay.overlay
    x: Math.round((parent.width - width) / 2)
    y: 10
    width: Math.min(680, parent.width - 48)
    padding: 12
    modal: true
    focus: true
    closePolicy: Popup.CloseOnEscape | Popup.CloseOnPressOutside
    Overlay.modal: Rectangle { color: Qt.alpha(theme.surface, 0.4) }

    background: Rectangle {
        color: theme.raised
        radius: theme.radius
        border.color: theme.accent
        border.width: 1
    }

    contentItem: ColumnLayout {
        spacing: 6
        TextField {
            id: field
            Layout.fillWidth: true
            placeholderText: qsTr("Search all mail")
            font.family: theme.font
            font.pixelSize: 16
            color: theme.bright
            Material.roundedScale: Math.min(4, theme.radius)
            Keys.onReturnPressed: {
                bar.visible = false
                bar.submitted(text)
            }
            Keys.onEnterPressed: {
                bar.visible = false
                bar.submitted(text)
            }
        }
        Text {
            text: qsTr("from:  to:  subject:  has:attachment  is:unread  after:2026/01/31  older_than:1y  label:")
            color: theme.muted
            font.family: theme.font
            font.pixelSize: 11
        }
    }
}

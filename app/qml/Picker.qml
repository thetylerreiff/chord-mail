import QtQuick
import QtQuick.Controls
import QtQuick.Controls.Material
import QtQuick.Layouts

// A keyboard picker: type to filter, arrows or Ctrl+J/K to move, Enter to
// choose, Esc to close. It serves the command palette, labels, snooze times,
// and confirmations. A `custom` function can turn typed text into an extra
// first choice, like "Snooze until Fri 9:00" or "Create label".
Popup {
    id: picker

    signal chosen(string mode, string value, var item)

    property string mode
    property string title
    property string placeholder
    property var items: []
    property var custom: null
    property var shown: []

    function open(config) {
        mode = config.mode
        title = config.title
        placeholder = config.placeholder || ""
        items = config.items || []
        custom = config.custom || null
        field.text = ""
        filter()
        list.currentIndex = 0
        visible = true
        if (placeholder !== "") field.forceActiveFocus(); else list.forceActiveFocus()
    }

    // Fuzzy match: every typed character appears in order; earlier and
    // tighter matches rank higher.
    function score(label, query) {
        const l = label.toLowerCase(), q = query.toLowerCase()
        if (q === "") return 1
        const at = l.indexOf(q)
        if (at >= 0) return 1000 - at
        let pos = 0, gaps = 0
        for (const c of q) {
            const i = l.indexOf(c, pos)
            if (i < 0) return 0
            gaps += i - pos
            pos = i + 1
        }
        return 500 - gaps
    }

    function filter() {
        const q = field.text.trim()
        const ranked = items
            .map(item => ({ item: item, s: score(item.label, q) }))
            .filter(x => x.s > 0)
        if (q !== "") ranked.sort((a, b) => b.s - a.s)
        let result = ranked.map(x => x.item)
        if (custom && q !== "") {
            const extra = custom(q)
            if (extra) result = [Object.assign({ hint: "" }, extra)].concat(result)
        }
        shown = result
        list.currentIndex = 0
    }

    function accept() {
        const item = shown[list.currentIndex]
        if (!item) return
        visible = false
        chosen(mode, item.value, item)
    }

    function moveBy(d) {
        if (shown.length === 0) return
        list.currentIndex = (list.currentIndex + d + shown.length) % shown.length
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
    y: Math.round(parent.height * 0.16)
    width: Math.min(560, parent.width - 48)
    padding: 0
    modal: true
    focus: true
    closePolicy: Popup.CloseOnEscape | Popup.CloseOnPressOutside

    Overlay.modal: Rectangle { color: Qt.alpha(theme.surface, 0.55) }

    background: Rectangle {
        color: theme.raised
        radius: theme.radius
        border.color: theme.selection
        border.width: 1
    }

    contentItem: ColumnLayout {
        spacing: 0

        Text {
            Layout.fillWidth: true
            Layout.margins: 16
            Layout.bottomMargin: 6
            text: picker.title
            color: theme.muted
            font.family: theme.font
            font.pixelSize: 12
        }

        TextField {
            id: field
            Layout.fillWidth: true
            Layout.leftMargin: 16
            Layout.rightMargin: 16
            visible: picker.placeholder !== ""
            placeholderText: picker.placeholder
            font.family: theme.font
            font.pixelSize: 15
            color: theme.bright
            Material.roundedScale: Math.min(4, theme.radius)
            onTextChanged: picker.filter()
            Keys.onPressed: function(event) {
                const ctrl = event.modifiers & Qt.ControlModifier
                if (event.key === Qt.Key_Down || ctrl && (event.key === Qt.Key_J || event.key === Qt.Key_N)) { picker.moveBy(1); event.accepted = true }
                else if (event.key === Qt.Key_Up || ctrl && (event.key === Qt.Key_K || event.key === Qt.Key_P)) { picker.moveBy(-1); event.accepted = true }
                else if (event.key === Qt.Key_Return || event.key === Qt.Key_Enter) { picker.accept(); event.accepted = true }
            }
        }

        ListView {
            id: list
            Layout.fillWidth: true
            Layout.preferredHeight: Math.min(contentHeight, 360)
            Layout.topMargin: 8
            Layout.bottomMargin: 8
            clip: true
            model: picker.shown
            highlightMoveDuration: 0
            keyNavigationEnabled: false
            Keys.onPressed: function(event) {
                if (event.key === Qt.Key_Down || event.key === Qt.Key_J) { picker.moveBy(1); event.accepted = true }
                else if (event.key === Qt.Key_Up || event.key === Qt.Key_K) { picker.moveBy(-1); event.accepted = true }
                else if (event.key === Qt.Key_Return || event.key === Qt.Key_Enter) { picker.accept(); event.accepted = true }
            }

            delegate: Rectangle {
                required property var modelData
                required property int index
                width: list.width
                height: 36
                color: ListView.isCurrentItem ? theme.selection : "transparent"
                Rectangle {
                    width: 3
                    height: parent.height
                    color: theme.accent
                    visible: parent.ListView.isCurrentItem
                }
                RowLayout {
                    anchors.fill: parent
                    anchors.leftMargin: 16
                    anchors.rightMargin: 16
                    Text {
                        Layout.fillWidth: true
                        text: modelData.label
                        color: theme.bright
                        font.family: theme.font
                        font.pixelSize: 14
                        elide: Text.ElideRight
                    }
                    Text {
                        text: modelData.hint || ""
                        color: modelData.hint === "✓" ? theme.accent : theme.muted
                        font.family: theme.font
                        font.pixelSize: 12
                    }
                }
                MouseArea {
                    anchors.fill: parent
                    onClicked: {
                        list.currentIndex = index
                        picker.accept()
                    }
                }
            }

            Text {
                visible: picker.shown.length === 0
                anchors.horizontalCenter: parent.horizontalCenter
                text: qsTr("No matches")
                color: theme.muted
                font.family: theme.font
                font.pixelSize: 13
                topPadding: 6
            }
        }
    }
}

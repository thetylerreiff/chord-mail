import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

// The conversation list for whatever view is open, with the split tabs on
// top for the inbox. Rows come from backend.threads as JSON and are diffed
// into the model, so archiving slides a row out instead of redrawing.
FocusScope {
    id: list

    signal opened(string id)

    property alias currentIndex: view.currentIndex
    property var selected: ({})
    property int selectedCount: 0

    function currentId() {
        return view.currentIndex >= 0 && view.currentIndex < rows.count ? rows.get(view.currentIndex).id : ""
    }
    function currentStarred() {
        return view.currentIndex >= 0 && view.currentIndex < rows.count && rows.get(view.currentIndex).starred
    }
    function currentLabels() {
        return view.currentIndex >= 0 && view.currentIndex < rows.count ? rows.get(view.currentIndex).labels : ""
    }
    function move(delta) {
        if (rows.count === 0) return
        view.currentIndex = Math.max(0, Math.min(rows.count - 1, view.currentIndex + delta))
    }
    function selectId(id) {
        for (let i = 0; i < rows.count; i++)
            if (rows.get(i).id === id) { view.currentIndex = i; return }
    }
    function toggleSelected() {
        const id = currentId()
        if (!id) return
        const s = Object.assign({}, selected)
        if (s[id]) delete s[id]; else s[id] = true
        selected = s
        selectedCount = Object.keys(s).length
        move(1)
    }
    function selectAll() {
        const s = {}
        for (let i = 0; i < rows.count; i++) s[rows.get(i).id] = true
        selected = s
        selectedCount = rows.count
    }
    function selectedIds() {
        return Object.keys(selected)
    }
    function clearSelection() {
        selected = {}
        selectedCount = 0
    }

    // Keeps the model in step with the JSON without resetting it.
    function sync(data) {
        const wanted = {}
        data.forEach((r, i) => wanted[r.id] = i)
        for (let i = rows.count - 1; i >= 0; i--)
            if (!(rows.get(i).id in wanted)) rows.remove(i)
        for (let j = 0; j < data.length; j++) {
            const r = data[j]
            if (j < rows.count && rows.get(j).id === r.id) {
                rows.set(j, r)
                continue
            }
            let found = -1
            for (let k = j + 1; k < rows.count; k++)
                if (rows.get(k).id === r.id) { found = k; break }
            if (found >= 0) {
                rows.move(found, j, 1)
                rows.set(j, r)
            } else {
                rows.insert(j, r)
            }
        }
        if (view.currentIndex >= rows.count) view.currentIndex = rows.count - 1
        if (view.currentIndex < 0 && rows.count > 0) view.currentIndex = 0
        // Selections of conversations that left the list go with them.
        const s = {}
        Object.keys(selected).forEach(id => { if (id in wanted) s[id] = true })
        selected = s
        selectedCount = Object.keys(s).length
    }

    ListModel { id: rows }

    Connections {
        target: backend
        function onThreadsChanged() { list.sync(JSON.parse(backend.threads || "[]")) }
    }
    Component.onCompleted: sync(JSON.parse(backend.threads || "[]"))

    readonly property var splitList: JSON.parse(backend.splits || "[]")
    readonly property string viewTitle: {
        const v = backend.view
        if (v === "starred") return qsTr("Starred")
        if (v === "sent") return qsTr("Sent")
        if (v === "drafts") return qsTr("Drafts")
        if (v === "snoozed") return qsTr("Snoozed")
        if (v === "scheduled") return qsTr("Scheduled")
        if (v === "all") return qsTr("All Mail")
        if (v === "trash") return qsTr("Trash")
        if (v === "search") return qsTr("Search: %1").arg(backend.query)
        if (v.indexOf("label:") === 0) {
            const l = JSON.parse(backend.labels || "[]").find(x => x.id === v.slice(6))
            return l ? l.name : qsTr("Label")
        }
        return qsTr("Inbox")
    }

    ColumnLayout {
        anchors.fill: parent
        spacing: 0

        // Split tabs, or the view's name.
        Item {
            Layout.fillWidth: true
            Layout.preferredHeight: 52

            Row {
                anchors.left: parent.left
                anchors.leftMargin: 28
                anchors.verticalCenter: parent.verticalCenter
                spacing: 28
                visible: backend.view === "inbox"

                Repeater {
                    model: list.splitList
                    delegate: Item {
                        required property var modelData
                        required property int index
                        readonly property bool current: index === backend.split
                        width: tab.implicitWidth
                        height: 30

                        Row {
                            id: tab
                            spacing: 7
                            anchors.verticalCenter: parent.verticalCenter
                            Text {
                                text: modelData.name
                                color: current ? theme.bright : theme.muted
                                font.family: theme.font
                                font.pixelSize: 15
                                font.weight: current ? Font.DemiBold : Font.Normal
                            }
                            Text {
                                visible: modelData.unread > 0
                                text: modelData.unread
                                color: current ? theme.accent : theme.muted
                                font.family: theme.font
                                font.pixelSize: 12
                                anchors.baseline: parent.children[0].baseline
                            }
                        }
                        Rectangle {
                            anchors.bottom: parent.bottom
                            width: parent.width
                            height: 2
                            color: theme.accent
                            visible: current
                        }
                        MouseArea {
                            anchors.fill: parent
                            cursorShape: Qt.PointingHandCursor
                            onClicked: backend.setSplitTo(index)
                        }
                    }
                }
            }

            Text {
                visible: backend.view !== "inbox"
                anchors.left: parent.left
                anchors.leftMargin: 28
                anchors.verticalCenter: parent.verticalCenter
                text: list.viewTitle
                color: theme.bright
                font.family: theme.font
                font.pixelSize: 15
                font.weight: Font.DemiBold
                elide: Text.ElideRight
                width: parent.width - 220
            }

            Text {
                anchors.right: parent.right
                anchors.rightMargin: 28
                anchors.verticalCenter: parent.verticalCenter
                text: list.selectedCount > 0 ? qsTr("%n selected", "", list.selectedCount) : backend.account
                color: list.selectedCount > 0 ? theme.accent : theme.muted
                font.family: theme.font
                font.pixelSize: 12
            }
        }

        Rectangle {
            Layout.fillWidth: true
            Layout.preferredHeight: 1
            color: theme.selection
        }

        ListView {
            id: view
            Layout.fillWidth: true
            Layout.fillHeight: true
            visible: rows.count > 0
            clip: true
            model: rows
            currentIndex: 0
            focus: true
            keyNavigationEnabled: false
            highlightMoveDuration: 0
            boundsBehavior: Flickable.StopAtBounds
            ScrollBar.vertical: ScrollBar { policy: ScrollBar.AsNeeded }

            remove: Transition {
                NumberAnimation { property: "opacity"; to: 0; duration: 140; easing.type: Easing.OutQuad }
                NumberAnimation { property: "x"; to: -40; duration: 140; easing.type: Easing.OutQuad }
            }
            displaced: Transition {
                NumberAnimation { property: "y"; duration: 160; easing.type: Easing.OutQuad }
            }

            delegate: ThreadRow {
                width: view.width
                current: ListView.isCurrentItem
                selected: !!list.selected[model.id]
                onClicked: view.currentIndex = index
                onDoubleClicked: list.opened(model.id)
            }
        }

        // Nothing here: in the inbox that's an accomplishment.
        Item {
            Layout.fillWidth: true
            Layout.fillHeight: true
            visible: rows.count === 0
            clip: true

            Image {
                anchors.fill: parent
                source: backend.view === "inbox" ? theme.wallpaper : ""
                fillMode: Image.PreserveAspectCrop
                opacity: 0.55
                asynchronous: true
            }
            Rectangle {
                anchors.fill: parent
                gradient: Gradient {
                    GradientStop { position: 0.0; color: theme.background }
                    GradientStop { position: 0.35; color: "transparent" }
                }
            }
            Column {
                anchors.centerIn: parent
                spacing: 8
                Text {
                    anchors.horizontalCenter: parent.horizontalCenter
                    text: backend.loading || backend.syncing && backend.lastSync === 0 ? qsTr("Fetching mail…")
                        : backend.view === "inbox" ? qsTr("Inbox zero") : qsTr("Nothing here")
                    color: theme.bright
                    font.family: theme.font
                    font.pixelSize: 26
                    font.weight: Font.DemiBold
                    style: Text.Raised
                    styleColor: theme.surface
                }
                Text {
                    anchors.horizontalCenter: parent.horizontalCenter
                    visible: backend.view === "inbox" && !backend.loading
                    text: list.splitList.length > 0 && list.splitList[backend.split]
                          ? qsTr("Nothing left in %1.").arg(list.splitList[backend.split].name) : ""
                    color: theme.foreground
                    font.family: theme.font
                    font.pixelSize: 14
                    style: Text.Raised
                    styleColor: theme.surface
                }
            }
        }
    }
}

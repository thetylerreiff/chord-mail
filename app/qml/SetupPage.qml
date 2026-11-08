import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

// First run: Chord needs your own Google OAuth client, then a sign-in.
// The page notices the client file appear on its own.
Rectangle {
    id: setup
    color: theme.background

    Shortcut {
        sequences: ["Return", "Enter"]
        enabled: setup.visible && backend.setup === "login"
        onActivated: backend.login()
    }
    Shortcut {
        sequences: ["O"]
        enabled: setup.visible && backend.setup === "client"
        onActivated: Qt.openUrlExternally("https://console.cloud.google.com/apis/library/gmail.googleapis.com")
    }
    Shortcut {
        sequences: ["Q"]
        enabled: setup.visible
        onActivated: Qt.quit()
    }

    ColumnLayout {
        anchors.centerIn: parent
        width: Math.min(640, parent.width - 64)
        spacing: 14

        Text {
            text: "Chord"
            color: theme.accent
            font.family: theme.font
            font.pixelSize: 34
            font.weight: Font.Bold
        }

        Text {
            Layout.fillWidth: true
            text: backend.setup === "client"
                  ? qsTr("Connect Chord to Gmail with your own Google OAuth client. No one else ever sees your mail.")
                  : qsTr("One more step: sign in with Google.")
            color: theme.bright
            font.family: theme.font
            font.pixelSize: 16
            wrapMode: Text.Wrap
        }

        Repeater {
            model: backend.setup === "client" ? [
                qsTr("In Google Cloud Console, create a project and enable the Gmail API (press O)."),
                qsTr("Under Google Auth Platform: fill in Branding, choose External under Audience, and add yourself as a test user."),
                qsTr("Under Clients, create a Desktop app client and download its JSON."),
                qsTr("Save it as ~/.config/chord-mail/client_secret.json"),
                qsTr("Back under Audience, publish the app, so sign-ins don't expire after seven days.")
            ] : []
            delegate: RowLayout {
                required property string modelData
                required property int index
                Layout.fillWidth: true
                spacing: 12
                Text {
                    Layout.alignment: Qt.AlignTop
                    text: index + 1
                    color: theme.accent
                    font.family: theme.font
                    font.pixelSize: 14
                }
                Text {
                    Layout.fillWidth: true
                    text: modelData
                    color: theme.foreground
                    font.family: theme.font
                    font.pixelSize: 14
                    wrapMode: Text.Wrap
                }
            }
        }

        Text {
            Layout.topMargin: 10
            text: backend.setup === "client"
                  ? qsTr("Waiting for client_secret.json…    O open Google Cloud    Q quit")
                  : qsTr("Enter sign in with Google    Q quit")
            color: theme.muted
            font.family: theme.font
            font.pixelSize: 13
        }
    }
}

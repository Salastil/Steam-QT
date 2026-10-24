import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

Item {
    id: page
    readonly property string st: lib.authState
    readonly property bool needCode: st === "needCode" || st === "needEmailCode" || st === "needCodeOrMobile"

    Pane {
        anchors.centerIn: parent
        width: Math.min(400, parent.width - 32)
        padding: 24
        background: Rectangle { color: palette.base; radius: 6; border.color: win.frame }

        ColumnLayout {
            anchors.fill: parent
            spacing: 12

            Label {
                text: page.needCode ? qsTr("Steam Guard") : page.st === "needMobile" ? qsTr("Confirm sign-in") : qsTr("Sign in to Steam")
                font.pixelSize: 20
                font.weight: Font.DemiBold
            }

            // Credentials
            ColumnLayout {
                visible: page.st === "loggedOut" || (page.st === "working" && !codeField.visible)
                Layout.fillWidth: true
                spacing: 8
                Label { text: qsTr("Account name"); color: win.subtle }
                TextField {
                    id: account
                    Layout.fillWidth: true
                    enabled: page.st === "loggedOut"
                    text: lib.account
                    onAccepted: password.forceActiveFocus()
                    Component.onCompleted: forceActiveFocus()
                }
                Label { text: qsTr("Password"); color: win.subtle }
                TextField {
                    id: password
                    Layout.fillWidth: true
                    enabled: page.st === "loggedOut"
                    echoMode: TextInput.Password
                    onAccepted: signIn.clicked()
                }
                CheckBox { id: remember; text: qsTr("Remember me on this computer"); checked: true; enabled: page.st === "loggedOut" }
                Button {
                    id: signIn
                    text: qsTr("Sign in")
                    highlighted: true
                    Layout.fillWidth: true
                    enabled: page.st === "loggedOut" && account.text.length > 0 && password.text.length > 0
                    onClicked: lib.login(account.text, password.text, remember.checked)
                }
            }

            // Steam Guard code
            ColumnLayout {
                visible: page.needCode
                Layout.fillWidth: true
                spacing: 8
                Label {
                    Layout.fillWidth: true
                    wrapMode: Text.Wrap
                    color: win.subtle
                    text: page.st === "needEmailCode"
                          ? qsTr("Enter the code Steam sent to your email address.")
                          : page.st === "needCodeOrMobile"
                            ? qsTr("Approve this sign-in in the Steam Mobile app, or enter the code it shows.")
                            : qsTr("Enter the code from the Steam Mobile app.")
                }
                TextField {
                    id: codeField
                    Layout.fillWidth: true
                    font.pixelSize: 20
                    font.letterSpacing: 4
                    horizontalAlignment: Text.AlignHCenter
                    maximumLength: 5
                    inputMethodHints: Qt.ImhUppercaseOnly | Qt.ImhNoPredictiveText
                    onAccepted: submit.clicked()
                    onVisibleChanged: if (visible) { text = ""; forceActiveFocus() }
                }
                Button {
                    id: submit
                    text: qsTr("Continue")
                    highlighted: true
                    Layout.fillWidth: true
                    enabled: codeField.text.length === 5
                    onClicked: lib.submitCode(codeField.text)
                }
            }

            // Mobile confirmation
            RowLayout {
                visible: page.st === "needMobile"
                spacing: 12
                BusyIndicator { running: visible; implicitWidth: 32; implicitHeight: 32 }
                Label {
                    Layout.fillWidth: true
                    wrapMode: Text.Wrap
                    text: qsTr("Approve this sign-in in the Steam Mobile app…")
                }
            }

            BusyIndicator {
                Layout.alignment: Qt.AlignHCenter
                visible: page.st === "working"
                running: visible
            }

            Label {
                visible: lib.authError.length > 0
                text: lib.authError
                color: win.negative
                wrapMode: Text.Wrap
                Layout.fillWidth: true
            }

            Label {
                visible: page.st === "loggedOut"
                text: qsTr("No account? <a href=\"https://store.steampowered.com/join/\">Create one in your browser</a>")
                textFormat: Text.StyledText
                linkColor: palette.highlight
                onLinkActivated: (link) => Qt.openUrlExternally(link)
                wrapMode: Text.Wrap
                Layout.fillWidth: true
            }
        }
    }
}

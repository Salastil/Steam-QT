import QtQuick
import QtQuick.Controls
import QtQuick.Dialogs
import QtQuick.Layouts

ScrollView {
    id: page
    contentWidth: availableWidth
    readonly property var libraries: { try { return JSON.parse(lib.librariesJson) } catch (e) { return [] } }
    function gb(bytes) { return (bytes / 1e9).toFixed(bytes >= 1e11 ? 0 : 1) + " GB" }
    function tb(bytes) { return bytes >= 1e12 ? (bytes / 1e12).toFixed(1) + " TB" : gb(bytes) }

    ColumnLayout {
        width: page.availableWidth
        spacing: 10

        Item { Layout.preferredHeight: 6 }
        Label {
            text: qsTr("Library folders")
            font.pixelSize: 17
            font.weight: Font.DemiBold
            Layout.leftMargin: 16
        }
        Label {
            text: qsTr("New installs go to the default folder unless you pick another in the Install dialog. "
                     + "Libraries of the official Steam client are detected and can be used as-is, without re-downloading.")
            color: win.subtle
            wrapMode: Text.Wrap
            Layout.fillWidth: true
            Layout.leftMargin: 16
            Layout.rightMargin: 16
        }

        Rectangle {
            Layout.fillWidth: true
            Layout.leftMargin: 16
            Layout.rightMargin: 16
            Layout.preferredHeight: folders.implicitHeight + 2
            color: palette.base
            border.color: win.frame
            radius: 5

            ColumnLayout {
                id: folders
                x: 1; y: 1
                width: parent.width - 2
                spacing: 0
                ButtonGroup { id: defaultGroup }
                Repeater {
                    model: page.libraries
                    delegate: ColumnLayout {
                        id: entry
                        required property var modelData
                        required property int index
                        Layout.fillWidth: true
                        spacing: 0
                        RowLayout {
                            Layout.fillWidth: true
                            Layout.margins: 10
                            spacing: 12
                            RadioButton {
                                ButtonGroup.group: defaultGroup
                                checked: entry.modelData.default
                                enabled: !entry.modelData.external
                                onClicked: lib.setDefaultLibrary(entry.index)
                                ToolTip.visible: hovered
                                ToolTip.text: qsTr("Default install location")
                            }
                            ColumnLayout {
                                Layout.fillWidth: true
                                spacing: 3
                                RowLayout {
                                    Label {
                                        text: entry.modelData.path + (entry.modelData.external ? qsTr("  (official Steam client)") : "")
                                        font.weight: Font.Medium
                                        elide: Text.ElideMiddle
                                        Layout.fillWidth: true
                                    }
                                    Label {
                                        visible: entry.modelData.default
                                        text: " " + qsTr("Default") + " "
                                        color: palette.highlightedText
                                        font.pixelSize: 11
                                        background: Rectangle { color: palette.highlight; radius: 3 }
                                    }
                                }
                                Rectangle {
                                    Layout.fillWidth: true
                                    implicitHeight: 6
                                    radius: 3
                                    color: win.frame
                                    visible: entry.modelData.total > 0
                                    Rectangle {
                                        width: parent.width * (1 - entry.modelData.free / Math.max(1, entry.modelData.total))
                                        height: 6
                                        radius: 3
                                        color: palette.highlight
                                    }
                                }
                                Label {
                                    text: qsTr("%1 free of %2 · %3 games")
                                          .arg(page.tb(entry.modelData.free)).arg(page.tb(entry.modelData.total)).arg(entry.modelData.games)
                                    color: win.subtle
                                    font.pixelSize: 12
                                }
                            }
                            Button {
                                visible: entry.modelData.external
                                text: qsTr("Use")
                                onClicked: lib.addLibrary(entry.modelData.path)
                            }
                            ToolButton {
                                id: folderMore
                                visible: !entry.modelData.external
                                text: "⋮"
                                font.pixelSize: 18
                                onClicked: folderMenu.popup(folderMore, folderMore.width - folderMenu.width, folderMore.height)
                                Menu {
                                    id: folderMenu
                                    MenuItem { text: qsTr("Make default"); onTriggered: lib.setDefaultLibrary(entry.index) }
                                    MenuItem { text: qsTr("Open folder"); onTriggered: Qt.openUrlExternally("file://" + entry.modelData.path) }
                                    MenuSeparator {}
                                    MenuItem {
                                        text: qsTr("Remove from list (keeps files)")
                                        onTriggered: lib.removeLibrary(entry.index)
                                    }
                                }
                            }
                        }
                        Rectangle {
                            visible: entry.index < page.libraries.length - 1
                            Layout.fillWidth: true
                            Layout.preferredHeight: 1
                            color: win.frame
                        }
                    }
                }
            }
        }

        RowLayout {
            Layout.leftMargin: 16
            Button { text: "＋  " + qsTr("Add folder…"); onClicked: folderDialog.open() }
        }

        Label {
            text: qsTr("Proton")
            font.pixelSize: 17
            font.weight: Font.DemiBold
            Layout.leftMargin: 16
            Layout.topMargin: 12
        }
        RowLayout {
            Layout.leftMargin: 16
            Layout.rightMargin: 16
            spacing: 12
            readonly property var tools: { try { return JSON.parse(lib.toolsJson) } catch (e) { return [] } }
            Label { text: qsTr("Default for Windows games"); color: win.subtle }
            ComboBox {
                id: protonCombo
                Layout.preferredWidth: 320
                model: parent.tools.map(t => t.name + (t.installed ? "" : qsTr(" (not installed)")))
                currentIndex: parent.tools.findIndex(t => t.key === lib.defaultProton)
                onActivated: (i) => lib.selectDefaultProton(parent.tools[i].key)
            }
        }
        Label {
            Layout.leftMargin: 16
            Layout.rightMargin: 16
            Layout.fillWidth: true
            wrapMode: Text.Wrap
            color: win.subtle
            text: qsTr("Valve's Proton builds download automatically the first time a game needs them. "
                     + "Custom builds (e.g. GE-Proton) placed in compatibilitytools.d are listed too.")
        }

        Label {
            text: qsTr("Updates")
            font.pixelSize: 17
            font.weight: Font.DemiBold
            Layout.leftMargin: 16
            Layout.topMargin: 12
        }
        CheckBox {
            Layout.leftMargin: 12
            text: qsTr("Update games automatically")
            checked: lib.autoUpdate
            onToggled: lib.setAutoUpdateEnabled(checked)
        }

        Label {
            text: qsTr("Account")
            font.pixelSize: 17
            font.weight: Font.DemiBold
            Layout.leftMargin: 16
            Layout.topMargin: 12
        }
        RowLayout {
            Layout.leftMargin: 16
            spacing: 12
            Label { text: qsTr("Signed in as %1").arg(lib.account) }
            Button { text: qsTr("Sign out"); onClicked: lib.logout() }
        }
        Item { Layout.preferredHeight: 16 }
    }

    FolderDialog {
        id: folderDialog
        title: qsTr("Choose a library folder")
        onAccepted: lib.addLibrary(selectedFolder.toString())
    }
}

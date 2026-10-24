// Steam-QT main window. All colours come from the application palette, which on Plasma is
// the active colour scheme (Breeze, Breeze Dark, ...), so the UI follows light/dark switches.
import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import org.steamqt

ApplicationWindow {
    id: win
    width: 820
    height: 900
    minimumWidth: 560
    minimumHeight: 480
    visible: true
    title: "Steam-QT"
    color: palette.window
    background: Rectangle { color: win.palette.window }

    // Bind the window palette to the system palette explicitly so every control inherits the
    // active colour scheme (and follows live light/dark switches on Plasma).
    SystemPalette { id: sys; colorGroup: SystemPalette.Active }
    palette {
        window: sys.window
        windowText: sys.windowText
        base: sys.base
        alternateBase: sys.alternateBase
        text: sys.text
        button: sys.button
        buttonText: sys.buttonText
        highlight: sys.highlight
        highlightedText: sys.highlightedText
        mid: sys.mid
        light: sys.light
        dark: sys.dark
        shadow: sys.shadow
        placeholderText: sys.placeholderText
        toolTipBase: sys.window
        toolTipText: sys.windowText
    }

    // "library" or "settings"
    property string page: "library"
    readonly property bool loggedIn: lib.authState === "ready"

    // Shared colours derived from the palette.
    readonly property color subtle: Qt.rgba(palette.windowText.r, palette.windowText.g, palette.windowText.b, 0.62)
    readonly property color frame: Qt.rgba(palette.windowText.r, palette.windowText.g, palette.windowText.b, 0.18)
    readonly property color hoverTint: Qt.rgba(palette.highlight.r, palette.highlight.g, palette.highlight.b, 0.10)
    readonly property color neutral: "#f67400"
    readonly property color negative: "#da4453"

    Library {
        id: lib
        objectName: "library"
        onMessage: (text) => toast.show(text)
    }

    header: ToolBar {
        visible: win.loggedIn
        background: Rectangle {
            color: palette.window
            Rectangle { anchors.bottom: parent.bottom; width: parent.width; height: 1; color: win.frame }
        }
        RowLayout {
            anchors.fill: parent
            anchors.leftMargin: 6
            anchors.rightMargin: 6
            ToolButton {
                visible: win.page !== "library"
                text: "‹"
                font.pixelSize: 20
                onClicked: win.page = "library"
                ToolTip.visible: hovered
                ToolTip.text: qsTr("Back to library")
            }
            Label {
                text: win.page === "settings" ? qsTr("Settings") : qsTr("Library")
                font.pixelSize: 17
                font.weight: Font.DemiBold
                leftPadding: win.page === "library" ? 8 : 0
                Layout.fillWidth: true
            }
            ToolButton {
                text: "⟳"
                font.pixelSize: 16
                onClicked: lib.refresh()
                ToolTip.visible: hovered
                ToolTip.text: qsTr("Refresh library and check for updates")
            }
            ToolButton {
                text: "⚙"
                font.pixelSize: 16
                checkable: true
                checked: win.page === "settings"
                onClicked: win.page = checked ? "settings" : "library"
                ToolTip.visible: hovered
                ToolTip.text: qsTr("Settings")
            }
            ToolButton {
                id: accountButton
                onClicked: accountMenu.popup(accountButton, 0, accountButton.height)
                contentItem: RowLayout {
                    spacing: 6
                    Rectangle {
                        width: 22; height: 22; radius: 11
                        color: palette.highlight
                        Label {
                            anchors.centerIn: parent
                            text: lib.account.length > 0 ? lib.account[0].toUpperCase() : "?"
                            color: palette.highlightedText
                            font.bold: true
                        }
                    }
                    Label { text: lib.account + " ▾" }
                }
                Menu {
                    id: accountMenu
                    MenuItem {
                        text: qsTr("Account details in browser ↗")
                        onTriggered: Qt.openUrlExternally("https://store.steampowered.com/account/")
                    }
                    MenuSeparator {}
                    MenuItem { text: qsTr("Sign out"); onTriggered: lib.logout() }
                }
            }
        }
    }

    footer: Pane {
        visible: win.loggedIn && lib.footer.length > 0
        padding: 8
        leftPadding: 14
        background: Rectangle {
            color: palette.window
            Rectangle { width: parent.width; height: 1; color: win.frame }
        }
        RowLayout {
            anchors.fill: parent
            Label { text: "⬇"; color: palette.highlight; visible: lib.footer.startsWith("Downloading") }
            Label { text: lib.footer; color: win.subtle; Layout.fillWidth: true; elide: Text.ElideRight }
        }
    }

    StackLayout {
        anchors.fill: parent
        currentIndex: !win.loggedIn ? 0 : (win.page === "settings" ? 2 : 1)
        LoginPage {}
        LibraryPage { onInstallRequested: (row) => installDialog.openFor(row) }
        SettingsPage {}
    }

    InstallDialog { id: installDialog }

    // Transient notices (errors, hints).
    Rectangle {
        id: toast
        property alias text: toastLabel.text
        function show(t) { text = t; opacity = 1; toastTimer.restart() }
        anchors.horizontalCenter: parent.horizontalCenter
        anchors.bottom: parent.bottom
        anchors.bottomMargin: 16
        width: Math.min(parent.width - 32, toastLabel.implicitWidth + 32)
        height: toastLabel.implicitHeight + 18
        radius: 6
        color: palette.toolTipBase
        border.color: win.frame
        opacity: 0
        visible: opacity > 0
        Behavior on opacity { NumberAnimation { duration: 180 } }
        Label {
            id: toastLabel
            anchors.centerIn: parent
            width: Math.min(implicitWidth, toast.parent.width - 64)
            wrapMode: Text.Wrap
            color: palette.toolTipText
        }
        Timer { id: toastTimer; interval: 5000; onTriggered: toast.opacity = 0 }
        MouseArea { anchors.fill: parent; onClicked: toast.opacity = 0 }
    }

    // Development aid: `--screenshot=<file>` saves the window after it settles and quits;
    // `--page=settings` opens a page directly.
    readonly property var devArgs: {
        const o = {}
        for (const a of Qt.application.arguments) {
            const m = a.match(/^--(screenshot|page|delay|expand|play|dlc)=(.*)$/)
            if (m) o[m[1]] = m[2]
        }
        return o
    }
    Component.onCompleted: if (devArgs.page) page = devArgs.page
    Connections {
        target: lib
        enabled: win.devArgs.expand !== undefined || win.devArgs.play !== undefined
        function onFooterChanged() {
            if (!lib.footer.endsWith("in library")) return
            if (win.devArgs.expand) lib.toggleSection(win.devArgs.expand)
            if (win.devArgs.play) lib.play(parseInt(win.devArgs.play))
            enabled = false
        }
    }
    Timer {
        running: win.devArgs.screenshot !== undefined
        interval: parseInt(win.devArgs.delay || "6000")
        onTriggered: win.contentItem.parent.grabToImage(r => { r.saveToFile(win.devArgs.screenshot); Qt.quit() })
    }
}

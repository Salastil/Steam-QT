// One game in the library list, with its runtime toggle, primary action and ⋮ menu.
import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

Rectangle {
    id: root
    property var row
    property bool expanded: false
    signal toggleExpanded()
    signal installRequested()

    readonly property bool installed: row.section === "installed"
    readonly property var dlc: { try { return JSON.parse(row.dlcJson) } catch (e) { return [] } }
    readonly property string storeUrl: "https://store.steampowered.com/app/" + row.appId
    readonly property var tools: { try { return JSON.parse(lib.toolsJson) } catch (e) { return [] } }

    implicitHeight: col.implicitHeight
    color: hover.hovered ? win.hoverTint : palette.base
    border.color: win.frame

    HoverHandler { id: hover }
    // Development aid: `--dlc=<appid>` opens that game's DLC dialog.
    Timer {
        running: win.devArgs.dlc !== undefined && parseInt(win.devArgs.dlc) === root.row.appId
        interval: 800
        onTriggered: { root.toggleExpanded(); dlcDialog.open() }
    }
    TapHandler { onTapped: root.toggleExpanded() }

    ColumnLayout {
        id: col
        width: parent.width
        spacing: 0

        RowLayout {
            Layout.fillWidth: true
            Layout.margins: 10
            spacing: 12

            // Capsule icon, with initials until (or unless) the image loads.
            Rectangle {
                width: 40; height: 40; radius: 4
                color: Qt.hsla((root.row.appId % 360) / 360, 0.45, 0.42, 1)
                Label {
                    anchors.centerIn: parent
                    visible: icon.status !== Image.Ready
                    color: "white"
                    font.bold: true
                    font.pixelSize: 15
                    text: root.row.title.split(/\s+/).filter(w => w.length).map(w => w[0]).join("").slice(0, 2)
                }
                Image {
                    id: icon
                    anchors.fill: parent
                    source: root.row.iconUrl
                    asynchronous: true
                    fillMode: Image.PreserveAspectCrop
                }
            }

            ColumnLayout {
                Layout.fillWidth: true
                spacing: 2
                Label {
                    text: root.row.title
                    font.pixelSize: 15
                    font.weight: Font.Medium
                    elide: Text.ElideRight
                    Layout.fillWidth: true
                }
                Label {
                    text: root.row.size + " · " + root.row.status
                    color: root.row.gameState === "update" || root.row.gameState === "paused" ? win.neutral
                         : root.row.gameState === "downloading" ? palette.highlight : win.subtle
                    font.pixelSize: 12
                    elide: Text.ElideRight
                    Layout.fillWidth: true
                }
            }

            // Which build runs: both available → toggle; otherwise a fixed label.
            Segmented {
                visible: root.row.os === "both"
                small: true
                options: ["Native", "Proton"]
                current: root.row.runtime === "native" ? "Native" : "Proton"
                onPicked: (o) => lib.setRuntime(root.row.appId, o.toLowerCase())
            }
            Label {
                visible: root.row.os === "linux"
                text: qsTr("Native")
                color: win.subtle
                font.pixelSize: 12
                rightPadding: 4
            }
            Button {
                id: protonButton
                visible: root.row.os === "windows" || (root.row.os === "both" && root.row.runtime === "proton")
                flat: true
                font.pixelSize: 12
                implicitHeight: 26
                text: root.row.proton + " ▾"
                onClicked: protonMenu.popup(protonButton, 0, protonButton.height)
                ToolTip.visible: hovered
                ToolTip.text: qsTr("Compatibility tool")
            }

            // Primary action
            Item {
                implicitWidth: 124
                implicitHeight: 32
                Button {
                    anchors.fill: parent
                    visible: root.row.gameState !== "downloading"
                    highlighted: root.row.gameState === "ready" || root.row.gameState === "update"
                    font.pixelSize: 13
                    text: {
                        switch (root.row.gameState) {
                        case "none": return "⬇  " + qsTr("Install")
                        case "update": return "⬇  " + qsTr("Update")
                        case "paused": return "▶  " + qsTr("Resume")
                        case "queued": return qsTr("Queued")
                        case "running": return "■  " + qsTr("Stop")
                        default: return "▶  " + qsTr("Play")
                        }
                    }
                    onClicked: {
                        switch (root.row.gameState) {
                        case "none": root.installRequested(); break
                        case "update":
                        case "paused": lib.update(root.row.appId); break
                        case "queued": lib.pause(root.row.appId); break
                        case "running": lib.stop(root.row.appId); break
                        default: lib.play(root.row.appId)
                        }
                    }
                }
                RowLayout {
                    anchors.fill: parent
                    visible: root.row.gameState === "downloading"
                    Rectangle {
                        Layout.fillWidth: true
                        implicitHeight: 8
                        radius: 4
                        color: win.frame
                        Rectangle {
                            width: parent.width * root.row.progress
                            height: parent.height
                            radius: 4
                            color: palette.highlight
                        }
                    }
                    Label { text: Math.round(root.row.progress * 100) + "%"; font.pixelSize: 12 }
                    ToolButton {
                        text: "❚❚"
                        font.pixelSize: 9
                        implicitWidth: 26
                        implicitHeight: 26
                        padding: 0
                        onClicked: lib.pause(root.row.appId)
                        ToolTip.visible: hovered
                        ToolTip.text: qsTr("Pause")
                    }
                }
            }

            ToolButton {
                id: more
                text: "⋮"
                font.pixelSize: 18
                implicitWidth: 30
                onClicked: menu.popup(more, more.width - menu.width, more.height)
            }
        }

        // Details shown when the row is clicked.
        GridLayout {
            visible: root.expanded
            Layout.fillWidth: true
            Layout.leftMargin: 62
            Layout.rightMargin: 12
            Layout.bottomMargin: 12
            columns: 4
            columnSpacing: 16
            rowSpacing: 6
            Label { text: qsTr("Build"); color: win.subtle; visible: root.installed }
            Label { text: root.row.build; visible: root.installed }
            Label { text: qsTr("Branch"); color: win.subtle; visible: root.installed }
            Label { text: root.row.branch; visible: root.installed }
            Label { text: qsTr("Updated"); color: win.subtle; visible: root.installed }
            Label { text: root.row.updated; visible: root.installed }
            Label { text: qsTr("App ID"); color: win.subtle }
            Label { text: root.row.appId }
            Label { text: qsTr("Launch options"); color: win.subtle; visible: root.installed }
            Label {
                visible: root.installed
                text: root.row.launchOptions.length ? root.row.launchOptions : qsTr("none")
                elide: Text.ElideRight
                Layout.columnSpan: 3
                Layout.fillWidth: true
            }
            Label { text: qsTr("DLC"); color: win.subtle }
            RowLayout {
                Layout.columnSpan: 3
                Layout.fillWidth: true
                spacing: 10
                Label {
                    text: root.dlc.length === 0 ? qsTr("None owned")
                        : qsTr("%1 owned · %2 enabled").arg(root.dlc.length).arg(root.dlc.filter(d => d.enabled).length)
                }
                Button {
                    visible: root.dlc.length > 0
                    flat: true
                    text: qsTr("Manage DLC…")
                    onClicked: dlcDialog.open()
                }
                Item { Layout.fillWidth: true }
            }
            Label { text: qsTr("Location"); color: win.subtle; visible: root.installed }
            Label {
                visible: root.installed
                text: root.row.location
                elide: Text.ElideMiddle
                Layout.columnSpan: 3
                Layout.fillWidth: true
            }
        }
    }

    Menu {
        id: menu
        MenuItem {
            text: root.installed ? "▶  " + qsTr("Play") : "⬇  " + qsTr("Install…")
            onTriggered: root.installed ? lib.play(root.row.appId) : root.installRequested()
        }
        MenuItem {
            text: qsTr("Update now")
            visible: root.installed
            height: visible ? implicitHeight : 0
            onTriggered: lib.update(root.row.appId)
        }
        Menu {
            title: qsTr("Runtime")
            MenuItem {
                text: qsTr("Native (Linux build)")
                checkable: true
                checked: root.row.runtime === "native"
                enabled: root.row.os !== "windows"
                onTriggered: lib.setRuntime(root.row.appId, "native")
            }
            MenuItem {
                text: qsTr("Proton (Windows build)")
                checkable: true
                checked: root.row.runtime === "proton"
                enabled: root.row.os !== "linux"
                onTriggered: lib.setRuntime(root.row.appId, "proton")
            }
        }
        MenuItem {
            text: root.dlc.length > 0 ? qsTr("DLC (%1)…").arg(root.dlc.length) : qsTr("DLC…")
            onTriggered: dlcDialog.open()
        }
        MenuItem {
            text: qsTr("Launch options…")
            visible: root.installed
            height: visible ? implicitHeight : 0
            onTriggered: launchDialog.open()
        }
        MenuItem {
            text: qsTr("Sync workshop items now")
            visible: root.installed
            height: visible ? implicitHeight : 0
            onTriggered: lib.syncWorkshop(root.row.appId)
        }
        MenuItem {
            text: qsTr("Workshop in browser ↗")
            onTriggered: Qt.openUrlExternally("https://steamcommunity.com/app/" + root.row.appId + "/workshop/")
        }
        MenuSeparator {}
        MenuItem {
            text: qsTr("Verify files")
            visible: root.installed
            height: visible ? implicitHeight : 0
            onTriggered: lib.verify(root.row.appId)
        }
        MenuItem {
            text: qsTr("Browse local files")
            visible: root.installed
            height: visible ? implicitHeight : 0
            onTriggered: Qt.openUrlExternally("file://" + root.row.location)
        }
        MenuItem {
            text: qsTr("Open store page ↗")
            onTriggered: Qt.openUrlExternally(root.storeUrl)
        }
        MenuSeparator { visible: root.installed }
        MenuItem {
            text: qsTr("Uninstall")
            visible: root.installed
            height: visible ? implicitHeight : 0
            onTriggered: confirmUninstall.open()
        }
    }

    // Compatibility tool choice for this game.
    Menu {
        id: protonMenu
        MenuItem {
            text: qsTr("Default")
            checkable: true
            checked: root.row.protonKey === ""
            onTriggered: lib.setProtonFor(root.row.appId, "")
        }
        MenuSeparator {}
        Instantiator {
            model: root.tools
            delegate: MenuItem {
                required property var modelData
                text: modelData.name + (modelData.installed ? "" : qsTr(" (downloads on first launch)"))
                checkable: true
                checked: root.row.protonKey === modelData.key
                onTriggered: lib.setProtonFor(root.row.appId, modelData.key)
            }
            onObjectAdded: (i, o) => protonMenu.insertItem(i + 2, o)
            onObjectRemoved: (i, o) => protonMenu.removeItem(o)
        }
    }

    // Owned DLC in a scrollable, filterable list (some games have hundreds).
    Dialog {
        id: dlcDialog
        parent: Overlay.overlay
        anchors.centerIn: parent
        width: Math.min(560, parent.width - 32)
        height: Math.min(640, parent.height - 64)
        modal: true
        title: qsTr("DLC — %1").arg(root.row.title)
        standardButtons: Dialog.Close
        onAboutToShow: dlcFilter.text = ""

        ColumnLayout {
            anchors.fill: parent
            spacing: 8
            TextField {
                id: dlcFilter
                Layout.fillWidth: true
                placeholderText: qsTr("Filter DLC…")
                visible: root.dlc.length > 8
            }
            Label {
                Layout.fillWidth: true
                wrapMode: Text.Wrap
                color: win.subtle
                text: root.installed
                      ? qsTr("Unticked DLC isn't downloaded; changes apply with the next update.")
                      : qsTr("Install the game to choose which DLC content to download.")
            }
            Frame {
                Layout.fillWidth: true
                Layout.fillHeight: true
                padding: 0
                background: Rectangle { color: palette.base; border.color: win.frame; radius: 4 }
                ListView {
                    id: dlcList
                    anchors.fill: parent
                    clip: true
                    boundsBehavior: Flickable.StopAtBounds
                    ScrollBar.vertical: ScrollBar { policy: ScrollBar.AsNeeded }
                    model: {
                        const f = dlcFilter.text.toLowerCase()
                        return f ? root.dlc.filter(d => d.name.toLowerCase().includes(f)) : root.dlc
                    }
                    delegate: CheckDelegate {
                        required property var modelData
                        width: ListView.view.width
                        text: modelData.name
                        checked: modelData.enabled
                        enabled: root.installed && modelData.content
                        ToolTip.visible: hovered && !modelData.content
                        ToolTip.text: qsTr("Unlock only — nothing to download")
                        onToggled: lib.setDlc(root.row.appId, modelData.id, checked)
                    }
                    Label {
                        anchors.centerIn: parent
                        visible: dlcList.count === 0
                        text: qsTr("No matching DLC")
                        color: win.subtle
                    }
                }
            }
            Label {
                text: qsTr("<a href=\"store\">Get more DLC in your browser ↗</a>")
                textFormat: Text.StyledText
                linkColor: palette.highlight
                onLinkActivated: Qt.openUrlExternally("https://store.steampowered.com/dlc/" + root.row.appId)
            }
        }
    }

    Dialog {
        id: launchDialog
        parent: Overlay.overlay
        anchors.centerIn: parent
        width: Math.min(520, parent.width - 32)
        modal: true
        title: qsTr("Launch options — %1").arg(root.row.title)
        standardButtons: Dialog.Cancel | Dialog.Save
        onAboutToShow: optionsField.text = root.row.launchOptions
        onAccepted: lib.setLaunchOptions(root.row.appId, optionsField.text)
        ColumnLayout {
            anchors.fill: parent
            TextField {
                id: optionsField
                Layout.fillWidth: true
                placeholderText: "PROTON_LOG=1 gamemoderun %command% -novid"
                onAccepted: launchDialog.accept()
            }
            Label {
                Layout.fillWidth: true
                wrapMode: Text.Wrap
                color: win.subtle
                text: qsTr("Same syntax as Steam: variables and wrappers before %command%, game arguments after it.")
            }
        }
    }

    Dialog {
        id: confirmUninstall
        parent: Overlay.overlay
        anchors.centerIn: parent
        modal: true
        title: qsTr("Uninstall %1?").arg(root.row.title)
        standardButtons: Dialog.Cancel | Dialog.Ok
        Label { text: qsTr("Game files will be deleted. Proton prefixes (save data) are kept.") }
        onAccepted: lib.uninstall(root.row.appId)
    }
}

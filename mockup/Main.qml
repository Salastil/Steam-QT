// Static UI mockup for Steam-QT. No backend: sample data only.
//
//   qml Main.qml -- theme=dark view=main            (interactive)
//   qml Main.qml -- theme=light view=menu out=x.png (render one state and quit)
//
// Colours come exclusively from `palette`, which mirrors how the real app inherits
// the Plasma colour scheme via Kirigami.Theme / qqc2-desktop-style. The two palettes
// below are Breeze Light and Breeze Dark, standing in for the system scheme.
import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

ApplicationWindow {
    id: win
    width: 760
    height: 860
    visible: true
    title: "Steam-QT"
    color: palette.window
    background: Rectangle { color: win.palette.window }

    readonly property var args: {
        const o = { theme: "light", view: "main", out: "" }
        for (const a of Qt.application.arguments) {
            const i = a.indexOf("=")
            if (i > 0) o[a.slice(0, i)] = a.slice(i + 1)
        }
        return o
    }
    readonly property bool dark: args.theme === "dark"
    property string view: args.view
    property int expandedApp: view === "expanded" ? 1145350 : -1
    property string filterMode: "All"

    palette {
        window:          dark ? "#202326" : "#eff0f1"
        windowText:      dark ? "#fcfcfc" : "#232629"
        base:            dark ? "#141618" : "#ffffff"
        alternateBase:   dark ? "#1d1f22" : "#f7f7f7"
        text:            dark ? "#fcfcfc" : "#232629"
        button:          dark ? "#292c30" : "#fcfcfc"
        buttonText:      dark ? "#fcfcfc" : "#232629"
        highlight:       "#3daee9"
        highlightedText: "#fcfcfc"
        mid:             dark ? "#3b4045" : "#bdc3c7"
        dark:            dark ? "#0f1012" : "#888e93"
        light:           dark ? "#3b4045" : "#ffffff"
        placeholderText: dark ? "#a1a9b1" : "#707d8a"
        toolTipBase:     dark ? "#292c30" : "#f7f7f7"
        toolTipText:     dark ? "#fcfcfc" : "#232629"
    }
    readonly property color subtle: dark ? "#a1a9b1" : "#707d8a"
    readonly property color frame: dark ? "#3b4045" : "#d1d5d9"
    readonly property color positive: dark ? "#27ae60" : "#1e9a53"
    readonly property color neutral: "#f67400"

    // os: "both" | "linux" | "windows"
    // state: "ready" | "update" | "downloading" | "none"
    readonly property var installed: [
        { appid: 1145350, name: "Hades II", os: "both", runtime: "Native", size: "12.4 GB",
          state: "ready", status: "Up to date · 41 h played", tint: "#8e44ad",
          build: "19312088", branch: "public", updated: "2026-10-20",
          dlc: "Soundtrack ☑ installed", workshop: "Not supported" },
        { appid: 1245620, name: "Elden Ring", os: "windows", runtime: "Proton 9.0-4", size: "48.1 GB",
          state: "update", status: "Update queued (1.3 GB) · applies before next launch", tint: "#b8860b",
          build: "18877201", branch: "public", updated: "2026-09-02",
          dlc: "Shadow of the Erdtree ☑ installed", workshop: "Not supported" },
        { appid: 427520, name: "Factorio", os: "both", runtime: "Native", size: "3.1 GB",
          state: "downloading", progress: 0.62, status: "Updating · 21.4 MB/s · 0:42 left", tint: "#c0392b",
          build: "19400112", branch: "experimental", updated: "2026-10-23",
          dlc: "Space Age ☑ installed", workshop: "Mods managed in-game" },
        { appid: 1086940, name: "Baldur's Gate 3", os: "both", runtime: "Proton Experimental", size: "122 GB",
          state: "ready", status: "Up to date · 212 h played", tint: "#16a085",
          build: "18990034", branch: "public", updated: "2026-10-01",
          dlc: "Digital Deluxe ☐ not installed", workshop: "3 mods · synced" }
    ]
    readonly property var notInstalled: [
        { appid: 620, name: "Portal 2", os: "both", runtime: "Native", size: "12.0 GB", state: "none",
          status: "Native · Proton", tint: "#2980b9" },
        { appid: 1091500, name: "Cyberpunk 2077", os: "windows", runtime: "Proton 9.0-4", size: "70.0 GB",
          state: "none", status: "Proton", tint: "#d4ac0d" },
        { appid: 105600, name: "Terraria", os: "both", runtime: "Native", size: "0.5 GB", state: "none",
          status: "Native · Proton", tint: "#27ae60" },
        { appid: 892970, name: "Valheim", os: "both", runtime: "Native", size: "1.4 GB", state: "none",
          status: "Native · Proton", tint: "#5d6d7e" },
        { appid: 1593500, name: "God of War", os: "windows", runtime: "Proton 9.0-4", size: "70.0 GB",
          state: "none", status: "Proton", tint: "#922b21" }
    ]

    function visibleFor(g) {
        if (filterMode === "Native") return g.os !== "windows"
        if (filterMode === "Proton") return g.os !== "linux"
        return true
    }

    // ---------- header ----------
    header: ToolBar {
        visible: ["main","expanded","menu"].indexOf(win.view) >= 0
        background: Rectangle { color: palette.window; Rectangle { anchors.bottom: parent.bottom; width: parent.width; height: 1; color: win.frame } }
        RowLayout {
            anchors.fill: parent
            anchors.leftMargin: 14
            anchors.rightMargin: 6
            Label { text: "Library"; font.pixelSize: 17; font.weight: Font.DemiBold; Layout.fillWidth: true }
            ToolButton { text: "⟳"; font.pixelSize: 16; ToolTip.visible: hovered; ToolTip.text: "Check for updates" }
            ToolButton { text: "⚙"; font.pixelSize: 16; ToolTip.visible: hovered; ToolTip.text: "Settings" }
            ToolButton {
                contentItem: RowLayout {
                    spacing: 6
                    Rectangle { width: 22; height: 22; radius: 11; color: palette.highlight
                        Label { anchors.centerIn: parent; text: "S"; color: "white"; font.bold: true } }
                    Label { text: "salastil ▾" }
                }
            }
        }
    }

    // ---------- footer ----------
    footer: Pane {
        visible: ["main","expanded","menu"].indexOf(win.view) >= 0
        padding: 8
        leftPadding: 14
        background: Rectangle { color: palette.window; Rectangle { width: parent.width; height: 1; color: win.frame } }
        RowLayout {
            anchors.fill: parent
            Label { text: "⬇"; color: palette.highlight }
            Label { text: "1 downloading · 1 queued · 21.4 MB/s"; color: win.subtle; Layout.fillWidth: true }
            Button { text: "Downloads ▸"; flat: true }
        }
    }

    // ---------- library ----------
    ScrollView {
        anchors.fill: parent
        visible: ["main","expanded","menu"].indexOf(win.view) >= 0
        contentWidth: availableWidth

        ColumnLayout {
            width: parent.width
            spacing: 0

            // filter bar
            RowLayout {
                Layout.fillWidth: true
                Layout.margins: 12
                spacing: 10
                TextField { placeholderText: "🔍  Filter games…"; Layout.fillWidth: true }
                Label { text: "Show:"; color: win.subtle }
                Segmented {
                    options: ["All", "Native", "Proton"]
                    current: win.filterMode
                    onPicked: (o) => win.filterMode = o
                }
            }

            Section { title: "Installed"; games: win.installed; extra: "185.7 GB on 2 drives"; open: true }
            Section { title: "Not installed"; games: win.notInstalled; extra: "213 games"
                      open: win.view === "main" || win.view === "menu" }
            Item { Layout.preferredHeight: 12 }
        }
    }

    // ---------- login ----------
    Rectangle {
        anchors.fill: parent
        visible: win.view === "login" || win.view === "guard"
        color: palette.window
        Pane {
            anchors.centerIn: parent
            width: 380
            padding: 24
            background: Rectangle { color: palette.base; radius: 6; border.color: win.frame }
            ColumnLayout {
                anchors.fill: parent
                spacing: 12
                Label { text: "Sign in to Steam"; font.pixelSize: 20; font.weight: Font.DemiBold }
                Label { text: "Account name"; color: win.subtle }
                TextField { text: "salastil"; Layout.fillWidth: true; enabled: win.view === "login" }
                Label { text: "Password"; color: win.subtle; visible: win.view === "login" }
                TextField { text: "hunter2hunter2"; echoMode: TextInput.Password; Layout.fillWidth: true; visible: win.view === "login" }
                CheckBox { text: "Remember me on this computer"; checked: true; visible: win.view === "login" }
                Button { text: "Sign in"; highlighted: true; Layout.fillWidth: true; visible: win.view === "login" }
                Label { text: "No account? Create one in your browser ↗"; color: palette.highlight; visible: win.view === "login" }
            }
        }
    }


    // ---------- settings: library folders ----------
    Rectangle {
        anchors.fill: parent
        visible: win.view === "settings" || win.view === "install"
        color: palette.window
        ColumnLayout {
            anchors.fill: parent
            anchors.margins: 16
            spacing: 10
            Label { text: "Library folders"; font.pixelSize: 17; font.weight: Font.DemiBold }
            Label {
                text: "New installs go to the default folder unless you pick another in the Install dialog. "
                    + "Existing Steam libraries (libraryfolders.vdf) are detected and can be added as-is."
                color: win.subtle; wrapMode: Text.Wrap; Layout.fillWidth: true
            }
            Rectangle {
                Layout.fillWidth: true
                Layout.preferredHeight: folders.implicitHeight + 2
                color: palette.base; border.color: win.frame; radius: 5
                ColumnLayout {
                    id: folders
                    x: 1; y: 1; width: parent.width - 2; spacing: 0
                    Repeater {
                        model: [
                            { path: "~/Games/SteamLibrary", dev: "nvme0n1p3", used: 0.71, free: "412 GB free of 1.4 TB", games: 3, def: true },
                            { path: "/mnt/storage/SteamLibrary", dev: "sda1", used: 0.38, free: "2.2 TB free of 3.6 TB", games: 1, def: false },
                            { path: "~/.local/share/Steam  (official client)", dev: "nvme0n1p3", used: 0.71, free: "detected · 7 games", games: 7, def: false, foreign: true }
                        ]
                        delegate: ColumnLayout {
                            required property var modelData
                            required property int index
                            Layout.fillWidth: true
                            spacing: 0
                            RowLayout {
                                Layout.fillWidth: true; Layout.margins: 10; spacing: 12
                                RadioButton { checked: modelData.def; enabled: !modelData.foreign; ToolTip.visible: hovered; ToolTip.text: "Default install location" }
                                ColumnLayout {
                                    Layout.fillWidth: true; spacing: 3
                                    RowLayout {
                                        Label { text: modelData.path; font.weight: Font.Medium }
                                        Label { visible: modelData.def; text: " Default "; color: win.palette.highlightedText; font.pixelSize: 11
                                                background: Rectangle { color: win.palette.highlight; radius: 3 } }
                                    }
                                    Rectangle { Layout.fillWidth: true; implicitHeight: 6; radius: 3; color: win.frame; visible: !modelData.foreign
                                        Rectangle { width: parent.width * modelData.used; height: 6; radius: 3; color: win.palette.highlight } }
                                    Label { text: modelData.free + (modelData.foreign ? "" : " · " + modelData.games + " games"); color: win.subtle; font.pixelSize: 12 }
                                }
                                Button { visible: !!modelData.foreign; text: "Import" }
                                ToolButton { visible: !modelData.foreign; text: "⋮"; font.pixelSize: 18 }
                            }
                            Rectangle { visible: index < 2; Layout.fillWidth: true; Layout.preferredHeight: 1; color: win.frame; opacity: 0.6 }
                        }
                    }
                }
            }
            RowLayout {
                Button { text: "＋  Add folder…" }
                Item { Layout.fillWidth: true }
            }
            Label { text: "Proton"; font.pixelSize: 17; font.weight: Font.DemiBold; Layout.topMargin: 12 }
            GridLayout {
                columns: 2; columnSpacing: 16; rowSpacing: 8
                Label { text: "Default for Windows-only games"; color: win.subtle }
                ComboBox { model: ["Proton 9.0-4", "Proton Experimental", "GE-Proton (from ~/.steam/compatibilitytools.d)"]; implicitWidth: 300 }
                Label { text: "Downloads"; color: win.subtle }
                RowLayout { CheckBox { text: "Update games automatically"; checked: true } CheckBox { text: "Limit to"; checked: false } SpinBox { value: 50; enabled: false } Label { text: "MB/s"; color: win.subtle } }
            }
            Item { Layout.fillHeight: true }
        }

        // install dialog over the settings page (render only)
        Rectangle { anchors.fill: parent; color: "#80000000"; visible: win.view === "install" }
        Pane {
            visible: win.view === "install"
            anchors.centerIn: parent
            width: 460; padding: 20
            background: Rectangle { color: win.palette.window; radius: 6; border.color: win.frame }
            ColumnLayout {
                anchors.fill: parent; spacing: 10
                Label { text: "Install Portal 2"; font.pixelSize: 18; font.weight: Font.DemiBold }
                Label { text: "Install to"; color: win.subtle }
                ComboBox { Layout.fillWidth: true; model: ["~/Games/SteamLibrary — 412 GB free (default)", "/mnt/storage/SteamLibrary — 2.2 TB free", "Other folder…"] }
                Label { text: "Run with"; color: win.subtle }
                Segmented { options: ["Native", "Proton"]; current: "Native" }
                Label { text: "Download size 12.0 GB · Disk space required 12.4 GB"; color: win.subtle }
                CheckBox { text: "Include DLC: none available"; enabled: false }
                RowLayout {
                    Layout.topMargin: 6
                    Item { Layout.fillWidth: true }
                    Button { text: "Cancel"; flat: true }
                    Button { text: "Install"; highlighted: true }
                }
            }
        }
    }

    // ---------- components ----------
    component Segmented: Row {
        id: seg
        property var options: []
        property string current: ""
        property bool small: false
        property bool locked: false
        signal picked(string o)
        spacing: -1
        Repeater {
            model: seg.options
            delegate: Button {
                required property string modelData
                required property int index
                text: modelData
                enabled: !seg.locked
                checkable: true
                checked: seg.current === modelData
                font.pixelSize: seg.small ? 12 : 13
                implicitHeight: seg.small ? 26 : 32
                leftPadding: 10; rightPadding: 10
                onClicked: seg.picked(modelData)
                palette.buttonText: checked ? win.palette.highlightedText : win.palette.buttonText
                background: Rectangle {
                    color: parent.checked ? win.palette.highlight : (parent.hovered ? Qt.lighter(win.palette.button, win.dark ? 1.25 : 0.96) : win.palette.button)
                    border.color: parent.checked ? Qt.darker(win.palette.highlight, 1.15) : win.frame
                    radius: 3
                }
            }
        }
    }

    component Section: ColumnLayout {
        id: sec
        property string title
        property string extra
        property var games: []
        property bool open: true
        readonly property var shown: games.filter(win.visibleFor)
        Layout.fillWidth: true
        Layout.leftMargin: 12
        Layout.rightMargin: 12
        spacing: 0

        ItemDelegate {
            Layout.fillWidth: true
            Layout.topMargin: 6
            onClicked: sec.open = !sec.open
            background: Item {}
            contentItem: RowLayout {
                Label { text: sec.open ? "▼" : "▶"; color: win.subtle; font.pixelSize: 11 }
                Label { text: sec.title + "  (" + sec.shown.length + ")"; font.weight: Font.DemiBold; font.pixelSize: 15; Layout.fillWidth: true }
                Label { text: sec.extra; color: win.subtle }
            }
        }

        Rectangle {
            visible: sec.open
            Layout.fillWidth: true
            Layout.preferredHeight: rows.implicitHeight + 2
            color: palette.base
            border.color: win.frame
            radius: 5
            ColumnLayout {
                id: rows
                x: 1; y: 1
                width: parent.width - 2
                spacing: 0
                Repeater {
                    model: sec.shown
                    delegate: GameRow {
                        required property var modelData
                        required property int index
                        game: modelData
                        last: index === sec.shown.length - 1
                        Layout.fillWidth: true
                    }
                }
            }
        }
    }

    component GameRow: Rectangle {
        id: row
        property var game
        property bool last: false
        property string runtime: game.os === "linux" ? "Native" : (game.os === "windows" ? "Proton" : (game.runtime === "Native" ? "Native" : "Proton"))
        readonly property bool expanded: win.expandedApp === game.appid
        implicitHeight: col.implicitHeight
        color: hover.hovered && win.args.out === "" ? (win.dark ? "#1e2a33" : "#eaf6fc") : "transparent"
        HoverHandler { id: hover }
        TapHandler { onTapped: win.expandedApp = row.expanded ? -1 : row.game.appid }

        ColumnLayout {
            id: col
            width: parent.width
            spacing: 0
            RowLayout {
                Layout.fillWidth: true
                Layout.margins: 10
                spacing: 12

                Rectangle {   // stand-in for the capsule icon
                    width: 40; height: 40; radius: 4
                    color: row.game.tint
                    Label { anchors.centerIn: parent; color: "white"; font.bold: true; font.pixelSize: 15
                            text: row.game.name.split(" ").map(w => w[0]).join("").slice(0, 2) }
                }
                ColumnLayout {
                    Layout.fillWidth: true
                    spacing: 2
                    Label { text: row.game.name; font.pixelSize: 15; font.weight: Font.Medium; elide: Text.ElideRight; Layout.fillWidth: true }
                    Label {
                        text: row.game.state === "none" ? row.game.size + " · " + row.game.status : row.game.size + " · " + row.game.status
                        color: row.game.state === "update" ? win.neutral : (row.game.state === "downloading" ? palette.highlight : win.subtle)
                        font.pixelSize: 12; elide: Text.ElideRight; Layout.fillWidth: true
                    }
                }

                // runtime selector
                Segmented {
                    visible: row.game.os === "both"
                    small: true
                    options: ["Native", "Proton"]
                    current: row.runtime
                    onPicked: (o) => row.runtime = o
                }
                ComboBox {
                    visible: row.game.os === "windows"
                    implicitHeight: 26
                    font.pixelSize: 12
                    model: ["Proton 9.0-4", "Proton Experimental", "Proton 8.0-5"]
                    currentIndex: Math.max(0, model.indexOf(row.game.runtime))
                }

                // primary action
                Item {
                    implicitWidth: 124
                    implicitHeight: 32
                    Button {
                        anchors.fill: parent
                        visible: row.game.state !== "downloading"
                        highlighted: row.game.state !== "none"
                        text: row.game.state === "none" ? "⬇  Install" : (row.game.state === "update" ? "▶  Update && Play" : "▶  Play")
                        font.pixelSize: row.game.state === "update" ? 11 : 13
                    }
                    RowLayout {
                        anchors.fill: parent
                        visible: row.game.state === "downloading"
                        Rectangle {
                            Layout.fillWidth: true; implicitHeight: 8; radius: 4
                            color: win.frame
                            Rectangle { width: parent.width * (row.game.progress || 0); height: parent.height; radius: 4; color: win.palette.highlight }
                        }
                        Label { text: Math.round((row.game.progress || 0) * 100) + "%"; font.pixelSize: 12 }
                        ToolButton { text: "❚❚"; font.pixelSize: 9; implicitWidth: 26; implicitHeight: 26; padding: 0 }
                    }
                }

                ToolButton {
                    id: more
                    text: "⋮"
                    font.pixelSize: 18
                    implicitWidth: 30
                    onClicked: menu.popup(more, 0, more.height)
                    Component.onCompleted: if (win.view === "menu" && row.game.appid === 1086940) menuTimer.start()
                    Timer { id: menuTimer; interval: 150; onTriggered: menu.popup(more, -menu.width + more.width, more.height) }
                }
            }

            // expanded details
            GridLayout {
                visible: row.expanded
                Layout.fillWidth: true
                Layout.leftMargin: 62
                Layout.rightMargin: 12
                Layout.bottomMargin: 12
                columns: 4
                columnSpacing: 16
                rowSpacing: 6
                Label { text: "Build"; color: win.subtle }
                Label { text: row.game.build || "" }
                Label { text: "Branch"; color: win.subtle }
                ComboBox { model: [row.game.branch || "public", "beta"]; implicitHeight: 26; font.pixelSize: 12 }
                Label { text: "Updated"; color: win.subtle }
                Label { text: row.game.updated || "" }
                Label { text: "Launch options"; color: win.subtle }
                TextField { text: "%command%"; implicitHeight: 26; font.pixelSize: 12; Layout.fillWidth: true }
                Label { text: "DLC"; color: win.subtle }
                Label { text: row.game.dlc || "" }
                Label { text: "Workshop"; color: win.subtle }
                Label { text: row.game.workshop || "" }
                Label { text: "Location"; color: win.subtle }
                Label { text: "~/Games/SteamLibrary"; Layout.columnSpan: 3 }
            }

            Rectangle { visible: !row.last; Layout.fillWidth: true; Layout.preferredHeight: 1; color: win.frame; opacity: 0.6 }
        }

        Menu {
            id: menu
            readonly property bool isInstalled: row.game.state !== "none"
            MenuItem { text: menu.isInstalled ? "▶  Play" : "⬇  Install…" }
            MenuItem { text: "Update now"; visible: menu.isInstalled; height: visible ? implicitHeight : 0 }
            Menu {
                title: "Runtime"
                MenuItem { text: "Native (Linux build)"; checkable: true; checked: row.runtime === "Native"; enabled: row.game.os !== "windows" }
                MenuItem { text: "Proton 9.0-4"; checkable: true; checked: row.runtime === "Proton"; enabled: row.game.os !== "linux" }
                MenuItem { text: "Proton Experimental"; checkable: true; enabled: row.game.os !== "linux" }
            }
            Menu {
                title: "DLC"
                MenuItem { text: "Digital Deluxe Upgrade"; checkable: true; checked: false }
                MenuItem { text: "Soundtrack"; checkable: true; checked: true }
                MenuSeparator {}
                MenuItem { text: "Get more DLC in browser ↗" }
            }
            Menu {
                title: "Workshop"
                MenuItem { text: "3 subscribed items" ; enabled: false }
                MenuItem { text: "Sync now" }
                MenuItem { text: "Browse workshop in browser ↗" }
            }
            MenuSeparator {}
            MenuItem { text: "Launch options…"; visible: menu.isInstalled; height: visible ? implicitHeight : 0 }
            MenuItem { text: "Verify files"; visible: menu.isInstalled; height: visible ? implicitHeight : 0 }
            MenuItem { text: "Browse local files"; visible: menu.isInstalled; height: visible ? implicitHeight : 0 }
            MenuItem { text: "Open store page ↗" }
            MenuSeparator {}
            MenuItem { text: "Move to…"; visible: menu.isInstalled; height: visible ? implicitHeight : 0 }
            MenuItem { text: menu.isInstalled ? "Uninstall" : "Hide" }
        }
    }

    // ---------- render-and-quit ----------
    Timer {
        running: win.args.out !== ""
        interval: 900
        onTriggered: win.contentItem.parent.grabToImage(r => { r.saveToFile(win.args.out); Qt.quit() })
    }
}

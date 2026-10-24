import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

Dialog {
    id: dlg
    property var row: null
    property string runtime: "native"
    readonly property var libraries: {
        try { return JSON.parse(lib.librariesJson).filter(l => !l.external) } catch (e) { return [] }
    }

    function openFor(r) {
        row = r
        runtime = r.runtime
        folder.currentIndex = Math.max(0, libraries.findIndex(l => l.default))
        open()
    }

    function gb(bytes) { return (bytes / 1e9).toFixed(bytes >= 1e11 ? 0 : 1) + " GB" }

    parent: Overlay.overlay
    anchors.centerIn: parent
    width: Math.min(480, parent.width - 32)
    modal: true
    title: row ? qsTr("Install %1").arg(row.title) : ""

    ColumnLayout {
        anchors.fill: parent
        spacing: 10
        Label { text: qsTr("Install to"); color: win.subtle }
        ComboBox {
            id: folder
            Layout.fillWidth: true
            model: dlg.libraries.map(l => l.path + " — " + dlg.gb(l.free) + qsTr(" free") + (l.default ? qsTr(" (default)") : ""))
        }
        Label { text: qsTr("Run with"); color: win.subtle; visible: dlg.row && dlg.row.os === "both" }
        Segmented {
            visible: dlg.row && dlg.row.os === "both"
            options: ["Native", "Proton"]
            current: dlg.runtime === "native" ? "Native" : "Proton"
            onPicked: (o) => dlg.runtime = o.toLowerCase()
        }
        Label {
            text: dlg.row ? qsTr("Size on disk: %1").arg(dlg.row.size) : ""
            color: win.subtle
        }
        Label {
            visible: dlg.libraries.length === 0
            text: qsTr("No library folder yet — add one in Settings.")
            color: win.negative
        }
    }

    footer: DialogButtonBox {
        Button { text: qsTr("Cancel"); flat: true; DialogButtonBox.buttonRole: DialogButtonBox.RejectRole }
        Button {
            text: qsTr("Install")
            highlighted: true
            enabled: dlg.libraries.length > 0
            DialogButtonBox.buttonRole: DialogButtonBox.AcceptRole
        }
    }

    onAccepted: {
        // Indices in the filtered list match settings order: externals are appended last.
        lib.install(row.appId, folder.currentIndex, row.os === "both" ? runtime : "")
    }
}

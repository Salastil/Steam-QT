import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

ItemDelegate {
    id: header
    property string title
    property string extra
    property bool expanded
    property string section
    background: Item {}
    onClicked: lib.toggleSection(section)
    contentItem: RowLayout {
        Label { text: header.expanded ? "▼" : "▶"; color: win.subtle; font.pixelSize: 11 }
        Label {
            text: header.title
            font.weight: Font.DemiBold
            font.pixelSize: 15
            Layout.fillWidth: true
        }
        Label { text: header.extra; color: win.subtle }
    }
}

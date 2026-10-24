import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

Item {
    id: page
    signal installRequested(var row)
    property int expandedApp: -1
    property string filterMode: "All"

    ColumnLayout {
        anchors.fill: parent
        spacing: 0

        RowLayout {
            Layout.fillWidth: true
            Layout.margins: 12
            spacing: 10
            TextField {
                id: filter
                placeholderText: qsTr("Filter games…")
                Layout.fillWidth: true
                onTextChanged: lib.setFilter(text, page.filterMode)
            }
            Label { text: qsTr("Show:"); color: win.subtle }
            Segmented {
                options: ["All", "Native", "Proton"]
                current: page.filterMode
                onPicked: (o) => { page.filterMode = o; lib.setFilter(filter.text, o) }
            }
        }

        ListView {
            id: list
            Layout.fillWidth: true
            Layout.fillHeight: true
            clip: true
            model: lib
            spacing: -1
            boundsBehavior: Flickable.StopAtBounds
            ScrollBar.vertical: ScrollBar {}

            delegate: Loader {
                id: cell
                width: ListView.view.width
                required property bool header
                required property string section
                required property string title
                required property string extra
                required property bool expanded
                required property int appId
                required property string os
                required property string runtime
                required property string gameState
                required property string status
                required property string size
                required property real progress
                required property string iconUrl
                required property string build
                required property string branch
                required property string updated
                required property string location
                required property string dlcJson
                required property string proton
                required property string protonKey
                required property string launchOptions
                sourceComponent: header ? headerComponent : rowComponent

                Component {
                    id: headerComponent
                    SectionHeader {
                        width: cell.width
                        leftPadding: 16
                        rightPadding: 16
                        topPadding: 12
                        title: cell.title
                        extra: cell.extra
                        expanded: cell.expanded
                        section: cell.section
                    }
                }
                Component {
                    id: rowComponent
                    Item {
                        width: cell.width
                        implicitHeight: gameRow.implicitHeight
                        GameRow {
                            id: gameRow
                            x: 12
                            width: parent.width - 24
                            row: cell
                            expanded: page.expandedApp === cell.appId
                            onToggleExpanded: page.expandedApp = expanded ? -1 : cell.appId
                            onInstallRequested: page.installRequested(cell)
                        }
                    }
                }
            }

            Label {
                anchors.centerIn: parent
                visible: list.count <= 2 && lib.footer.length === 0
                text: qsTr("Loading library…")
                color: win.subtle
            }
        }
    }
}

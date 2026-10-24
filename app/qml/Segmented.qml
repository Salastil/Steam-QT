// A row of mutually exclusive buttons (e.g. All / Native / Proton).
import QtQuick
import QtQuick.Controls

Row {
    id: seg
    property var options: []
    property string current: ""
    property bool small: false
    signal picked(string option)
    spacing: -1

    Repeater {
        model: seg.options
        delegate: Button {
            id: btn
            required property string modelData
            text: modelData
            checkable: true
            checked: seg.current === modelData
            font.pixelSize: seg.small ? 12 : 13
            implicitHeight: seg.small ? 26 : 32
            leftPadding: 10
            rightPadding: 10
            palette.buttonText: checked ? win.palette.highlightedText : win.palette.buttonText
            background: Rectangle {
                radius: 3
                color: btn.checked ? win.palette.highlight
                     : btn.hovered ? Qt.lighter(win.palette.button, 1.12) : win.palette.button
                border.color: btn.checked ? Qt.darker(win.palette.highlight, 1.15) : win.frame
                opacity: btn.enabled ? 1 : 0.5
            }
            onClicked: {
                checked = Qt.binding(() => seg.current === modelData)
                seg.picked(modelData)
            }
        }
    }
}

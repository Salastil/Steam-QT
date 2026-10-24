// A row of mutually exclusive buttons (e.g. All / Native / Proton).
//
// Fully custom (label + background) rather than a styled Button: KDE's org.kde.desktop
// style paints a Button's text as part of its background, so replacing the background to
// show the selected state would hide the label.
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
        delegate: AbstractButton {
            id: btn
            required property string modelData
            readonly property bool selected: seg.current === modelData
            text: modelData
            hoverEnabled: true
            focusPolicy: Qt.TabFocus
            implicitHeight: seg.small ? 26 : 32
            implicitWidth: label.implicitWidth + 20
            Accessible.role: Accessible.RadioButton
            Accessible.checked: selected

            contentItem: Label {
                id: label
                text: btn.text
                font.pixelSize: seg.small ? 12 : 13
                horizontalAlignment: Text.AlignHCenter
                verticalAlignment: Text.AlignVCenter
                color: btn.selected ? win.palette.highlightedText : win.palette.buttonText
            }
            background: Rectangle {
                radius: 3
                color: btn.selected ? win.palette.highlight
                     : btn.down ? Qt.darker(win.palette.button, 1.1)
                     : btn.hovered ? Qt.lighter(win.palette.button, 1.12) : win.palette.button
                border.color: btn.visualFocus ? win.palette.highlight
                            : btn.selected ? Qt.darker(win.palette.highlight, 1.15) : win.frame
                opacity: btn.enabled ? 1 : 0.5
            }
            onClicked: if (!selected) seg.picked(modelData)
        }
    }
}

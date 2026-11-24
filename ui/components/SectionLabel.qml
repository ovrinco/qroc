import QtQuick
import ".."

// Section heading: text-base font-semibold text-primary.
Text {
    property string suffix: ''
    font.family: Theme.font
    font.pixelSize: Theme.fsBase
    font.weight: Font.DemiBold
    font.letterSpacing: 0
    color: Theme.textPrimary
    textFormat: Text.PlainText
    elide: Text.ElideRight
}

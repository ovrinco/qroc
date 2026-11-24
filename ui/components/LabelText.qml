import QtQuick
import ".."

// Body text: text-sm text-primary.
Text {
    textFormat: Text.PlainText
    color: Theme.textPrimary
    font.family: Theme.font
    font.pixelSize: Theme.fsSm
    elide: Text.ElideRight
}

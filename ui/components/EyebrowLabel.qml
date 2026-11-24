import QtQuick
import ".."

// text-[10px] uppercase tracking-wider text-muted ("PROJECT BRIEF", "NEXT ACTION").
Text {
    font.family: Theme.font
    font.pixelSize: Theme.fsMicro
    font.capitalization: Font.AllUppercase
    font.letterSpacing: Theme.trackingWider(Theme.fsMicro)
    color: Theme.textMuted
    textFormat: Text.PlainText
    elide: Text.ElideRight
}

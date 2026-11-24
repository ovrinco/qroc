import QtQuick
import ".."

// Status badge / count chip: rounded px-2 py-0.5 text-xs font-medium, tone at 12% fill.
// mono: font-mono text-[10px] chip ("0b 0/16"). neutral: bg-tertiary text-muted. outline: border only.
Rectangle {
    id: pill
    property string text: ''
    property color tone: Theme.textSecondary
    property bool mono: false
    property bool neutral: false
    property bool outline: false
    property real fillAlpha: 0.12
    property int weight: mono ? Font.Normal : Font.Medium
    color: outline ? 'transparent' : neutral ? Theme.bgTertiary : Qt.alpha(tone, fillAlpha)
    border.width: outline ? Theme.borderWidth : 0
    border.color: Theme.border
    radius: Theme.radiusSm
    implicitWidth: label.implicitWidth + (pill.mono ? Theme.s3 : Theme.s4)
    implicitHeight: label.implicitHeight + Theme.s1
    Text {
        id: label
        anchors.centerIn: parent
        text: pill.text
        font.family: pill.mono ? Theme.mono : Theme.font
        font.pixelSize: pill.mono ? Theme.fsMicro : Theme.fsXs
        font.weight: pill.weight
        color: pill.neutral ? Theme.textMuted : pill.outline ? Theme.textSecondary : pill.tone
    }
}

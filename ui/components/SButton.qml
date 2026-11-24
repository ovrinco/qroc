import QtQuick
import ".."
import QtQuick.Controls

// React button classes. px-4 py-2 text-sm rounded-lg = 36 (size 'md'); px-3 py-1.5 = 32 ('sm'); py-2.5 = 40 ('lg').
// primary: bg-accent text-white hover:bg-accent-secondary. default: bg-secondary border hover:bg-tertiary.
// quiet: transparent text-secondary hover:text-primary hover:bg-tertiary. destructive: bg-red-600 text-white.
// disabled: opacity .38. Icon w-4 (16) with gap-2 (8). Focus-visible: 2px accent ring.
Button {
    id: control
    property bool primary: false
    property bool secondary: false
    property bool quiet: false
    property bool destructive: false
    property bool busy: false
    property real restingWidth: 0
    property string iconName: ''
    property string size: 'md'
    property int iconSize: Theme.icon
    property string accessibleLabel: text
    readonly property bool filled: primary || destructive
    readonly property color fillColor: destructive ? Theme.danger : Theme.accentPrimary
    readonly property color fillHover: destructive ? Theme.mixed(Theme.danger, Qt.rgba(0, 0, 0, 1), 0.18) : Theme.accentSecondary
    readonly property color surfaceColor: filled ? (down || hovered ? fillHover : fillColor)
        : (down || hovered) ? Theme.bgTertiary : quiet ? 'transparent' : secondary ? Theme.bgTertiary : Theme.bgSecondary
    readonly property color ink: filled ? Theme.textOnAccent
        : quiet ? ((hovered || down || visualFocus) ? Theme.textPrimary : Theme.textSecondary) : Theme.textPrimary
    readonly property int controlSize: size === 'sm' ? Theme.controlHeightSm : size === 'lg' ? Theme.controlHeightLg : Theme.controlHeight
    readonly property int sidePadding: size === 'sm' ? Theme.s3 : Theme.s4
    implicitHeight: controlSize
    implicitWidth: busy && restingWidth > 0 ? restingWidth
        : (!text && iconName) ? controlSize
        : Math.max(controlSize, label.implicitWidth + (iconName ? iconSize + Theme.s2 : 0) + sidePadding * 2)
    padding: 0
    opacity: enabled ? 1 : 0.38
    font.family: Theme.font
    font.pixelSize: Theme.fsSm
    font.weight: primary ? Font.Medium : Font.Normal
    hoverEnabled: true
    focusPolicy: Qt.StrongFocus
    contentItem: Item {
        Row {
            anchors.centerIn: parent
            spacing: Theme.s2
            visible: !control.busy
            SIcon {
                visible: !!control.iconName
                name: control.iconName
                size: control.iconSize
                color: control.ink
                anchors.verticalCenter: parent.verticalCenter
            }
            Text {
                id: label
                visible: !!control.text
                text: control.text
                font: control.font
                color: control.ink
                verticalAlignment: Text.AlignVCenter
                anchors.verticalCenter: parent.verticalCenter
                width: Math.min(implicitWidth, Math.max(0, control.width - (control.iconName ? control.iconSize + Theme.s2 : 0) - control.sidePadding * 2))
                elide: Text.ElideRight
            }
        }
        Text {
            anchors.centerIn: parent; visible: control.busy; text: '•••'
            font.family: Theme.font; font.pixelSize: Theme.fsSm; font.letterSpacing: 3; color: control.ink
            Accessible.ignored: true
        }
    }
    background: Rectangle {
        color: control.surfaceColor
        radius: Theme.radius
        border.width: control.visualFocus && !control.filled ? 2 : (control.filled || control.quiet) ? 0 : Theme.borderWidth
        border.color: control.visualFocus ? Theme.accentPrimary : Theme.border
        Behavior on color { enabled: !Theme.reducedMotion; ColorAnimation { duration: Theme.motionFast } }
        // Focus on an accent action needs separation, not the same accent edge.
        Rectangle { anchors.fill: parent; anchors.margins: 2; radius: Theme.radiusMd; visible: control.visualFocus && control.filled; color: 'transparent'; border.color: control.ink; border.width: 1 }
    }
    ToolTip.visible: !text && !!accessibleLabel && hovered
    ToolTip.text: accessibleLabel
    ToolTip.delay: 600
    Accessible.role: Accessible.Button
    Accessible.name: control.accessibleLabel + (control.busy ? ', working' : '')
    onWidthChanged: if (!busy && width >= controlSize) restingWidth = width
}

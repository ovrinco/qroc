import QtQuick
import ".."
import QtQuick.Controls

// Sidebar row: px-3 py-2 min-h-9 rounded-lg text-sm gap-3; icon w-4; text-secondary, hover bg-tertiary;
// selected = bg-tertiary text-primary. Collapsed shows the icon only with a tooltip.
AbstractButton {
    id: control
    padding: 0
    property bool selected: false
    property bool collapsed: false
    property string symbol: ''
    readonly property color ink: control.selected || control.hovered ? Theme.textPrimary : Theme.textSecondary
    implicitWidth: 168
    implicitHeight: Theme.rowHeight
    hoverEnabled: true
    focusPolicy: Qt.StrongFocus
    contentItem: Item {
        SIcon {
            x: control.collapsed ? (parent.width - width) / 2 : Theme.s3
            anchors.verticalCenter: parent.verticalCenter
            name: control.symbol
            size: Theme.icon
            color: control.ink
        }
        Text {
            x: Theme.s3 + Theme.icon + Theme.s3; width: Math.max(0, parent.width - x - Theme.s3); height: parent.height
            visible: !control.collapsed
            text: control.text
            font.family: Theme.font; font.pixelSize: Theme.fsSm
            color: control.ink
            verticalAlignment: Text.AlignVCenter
            elide: Text.ElideRight
        }
    }
    background: Rectangle {
        radius: Theme.radius
        color: control.selected || control.hovered ? Theme.bgTertiary : 'transparent'
        Behavior on color { enabled: !Theme.reducedMotion; ColorAnimation { duration: Theme.motionFast } }
        Rectangle { anchors.fill: parent; radius: Theme.radius; color: 'transparent'; border.width: 2; border.color: Theme.accentPrimary; visible: control.visualFocus }
    }
    onActiveFocusChanged: {
        if (!activeFocus) return;
        // Keep keyboard navigation reachable in a short window without
        // shrinking hit targets. Works in the shell and document cascades.
        for (var viewport = parent; viewport; viewport = viewport.parent) {
            if (viewport.contentY !== undefined && viewport.contentItem) {
                var point = mapToItem(viewport.contentItem, 0, 0);
                if (point.y < viewport.contentY) viewport.contentY = point.y;
                else if (point.y + height > viewport.contentY + viewport.height)
                    viewport.contentY = point.y + height - viewport.height;
                break;
            }
        }
    }
    ToolTip.visible: collapsed && hovered
    ToolTip.text: text
    ToolTip.delay: 700
    Accessible.role: Accessible.PageTab
    Accessible.name: text
    Accessible.selected: selected
}

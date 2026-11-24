import QtQuick
import ".."
import QtQuick.Controls
import "Workplane.js" as Workplane

// ::-webkit-scrollbar 8px; thumb --scrollbar-thumb radius 4; hover --scrollbar-thumb-hover; track transparent.
ScrollBar {
    id: control
    objectName: "soliton-scrollbar"
    readonly property bool hasOverflow: Workplane.overflows(size, policy, ScrollBar.AlwaysOff)
    padding: 0
    minimumSize: 0.06
    hoverEnabled: hasOverflow
    interactive: hasOverflow
    implicitWidth: 8
    implicitHeight: 8
    // Retain the attached control's geometry/policy. Only the thumb is painted
    // conditionally: an empty or exact-fit view must not acquire a fake bar.
    contentItem: Rectangle {
        objectName: "soliton-scrollbar-thumb"
        implicitWidth: 8; implicitHeight: 8
        radius: Theme.radiusSm
        visible: control.hasOverflow
        color: control.pressed || control.hovered ? Theme.scrollbarThumbHover : Theme.scrollbarThumb
        opacity: control.hasOverflow ? 1 : 0
        Behavior on color { enabled: !Theme.reducedMotion; ColorAnimation { duration: Theme.motionFast } }
    }
    background: Item { }
}

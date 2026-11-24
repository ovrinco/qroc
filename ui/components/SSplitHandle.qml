import QtQuick
import ".."
import QtQuick.Controls

// The hit region is not the visual separator. Deliberate resizing changes the
// neighboring panes only; this component never animates or adds a layout row.
Rectangle {
    id: handle
    property bool horizontal: true
    // Keep the pointer target close to the pane boundary. The visible line is
    // centered inside this six-pixel target, so there is no adjacent dead gap.
    implicitWidth: horizontal ? 6 : 1
    implicitHeight: horizontal ? 1 : 10
    color: "transparent"
    Rectangle {
        anchors.centerIn: parent
        width: handle.horizontal ? 1 : parent.width
        height: handle.horizontal ? parent.height : 1
        color: handle.SplitHandle.hovered || handle.SplitHandle.pressed ? Theme.accent : Theme.line
        visible: true
    }
    HoverHandler { cursorShape: handle.horizontal ? Qt.SplitHCursor : Qt.SplitVCursor }
}

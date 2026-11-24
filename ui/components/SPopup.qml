import QtQuick
import ".."
import QtQuick.Controls

Popup {
    id: control
    // Menus own keyboard dismissal while open, including nested menus whose
    // callers do not set focus explicitly. Qt restores the previous focus.
    focus: true
    // Set this on anchored menus. Coordinates are recalculated in the overlay
    // so pane and window resizes keep the menu attached to its own control.
    property Item trigger: null
    property bool alignRight: false
    property int anchorGap: 4

    function positionAtTrigger() {
        if (!trigger || !parent)
            return;
        var point = trigger.mapToItem(parent, alignRight ? trigger.width : 0, trigger.height + anchorGap);
        var margin = 8;
        var nextX = alignRight ? point.x - width : point.x;
        if (nextX + width > parent.width - margin)
            nextX = parent.width - width - margin;
        x = Math.max(margin, nextX);
        y = point.y;
        if (y + height > parent.height - margin)
            y = trigger.mapToItem(parent, 0, -height - anchorGap).y;
        y = Math.max(margin, Math.min(y, parent.height - height - margin));
    }

    onOpened: positionAtTrigger()
    onTriggerChanged: if (visible) positionAtTrigger()
    onWidthChanged: if (visible) positionAtTrigger()
    onHeightChanged: if (visible) positionAtTrigger()

    Connections {
        target: control.trigger
        function onXChanged() { control.positionAtTrigger(); }
        function onYChanged() { control.positionAtTrigger(); }
        function onWidthChanged() { control.positionAtTrigger(); }
        function onHeightChanged() { control.positionAtTrigger(); }
    }

    Connections {
        target: control.parent
        function onWidthChanged() { if (control.visible) control.positionAtTrigger(); }
        function onHeightChanged() { if (control.visible) control.positionAtTrigger(); }
    }
    // Ancestors can move without changing trigger.x/y (for example a split pane).
    Timer {
        interval: 16
        running: control.visible && !!control.trigger
        repeat: true
        onTriggered: control.positionAtTrigger()
    }
    // React menu: bg-secondary border rounded-lg shadow-lg py-1.
    palette.base: Theme.bgSecondary
    palette.text: Theme.textPrimary
    palette.window: Theme.bgPrimary
    palette.windowText: Theme.textPrimary
    palette.button: Theme.bgTertiary
    palette.buttonText: Theme.textPrimary
    palette.highlight: Theme.accentPrimary
    palette.highlightedText: Theme.textOnAccent
    font.family: Theme.font
    font.pixelSize: Theme.fsSm
    padding: Theme.s2
    background: Rectangle { color: Theme.bgSecondary; radius: Theme.radius; border.width: Theme.borderWidth; border.color: Theme.border }
}

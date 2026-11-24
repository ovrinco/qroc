import QtQuick
import ".."
import QtQuick.Controls

// React textarea: px-3 py-2 text-sm rounded-lg border bg-primary text-primary focus:border-accent.
TextArea {
    id: control
    property bool invalid: false
    font.family: Theme.font
    font.pixelSize: Theme.fsSm
    color: Theme.textPrimary
    placeholderTextColor: Theme.textMuted
    selectionColor: Theme.accentPrimary
    selectedTextColor: Theme.textOnAccent
    selectByMouse: true
    selectByKeyboard: true
    hoverEnabled: true
    padding: Theme.s3
    background: Rectangle {
        radius: Theme.radius
        color: Theme.bgPrimary
        border.width: Theme.borderWidth
        border.color: control.invalid ? Theme.danger : control.activeFocus ? Theme.accentPrimary : control.hovered ? Theme.borderHover : Theme.border
        Behavior on border.color { enabled: !Theme.reducedMotion; ColorAnimation { duration: Theme.motionFast } }
    }
}

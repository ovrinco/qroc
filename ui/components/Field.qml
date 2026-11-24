import QtQuick
import ".."
import QtQuick.Controls

// React input: px-3 py-2 text-sm rounded-lg border bg-primary text-primary placeholder:text-muted
// focus:border-accent outline-none. 36 high; large (py-2.5) = 40. invalid: border danger.
TextField {
    id: field
    property bool invalid: false
    property bool frameless: false
    property bool large: false
    property string errorText: ''
    implicitHeight: large ? Theme.controlHeightLg : Theme.controlHeight
    leftPadding: Theme.s3
    rightPadding: Theme.s3
    topPadding: 0
    bottomPadding: 0
    hoverEnabled: true
    verticalAlignment: TextInput.AlignVCenter
    color: field.enabled ? Theme.textPrimary : Theme.textMuted
    placeholderTextColor: Theme.textMuted
    font.family: Theme.font
    font.pixelSize: Theme.fsSm
    selectByMouse: true
    selectionColor: Theme.accentPrimary
    selectedTextColor: Theme.textOnAccent
    background: Rectangle {
        radius: Theme.radius
        color: field.frameless ? 'transparent' : field.enabled ? Theme.bgPrimary : Theme.bgSecondary
        border.width: field.frameless && !field.activeFocus && !field.invalid ? 0 : Theme.borderWidth
        border.color: field.invalid ? Theme.danger : field.activeFocus ? Theme.accentPrimary : field.hovered ? Theme.borderHover : Theme.border
        Behavior on border.color { enabled: !Theme.reducedMotion; ColorAnimation { duration: Theme.motionFast } }
    }
    Accessible.name: placeholderText
    Accessible.description: errorText
    Accessible.role: Accessible.EditableText
}

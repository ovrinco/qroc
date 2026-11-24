import QtQuick
import ".."
import QtQuick.Controls as Controls

// React checkbox: w-4 h-4 rounded border; checked = accent fill with a white check; label text-sm text-primary, gap-2.
Controls.CheckBox {
    id: control
    implicitHeight: Math.max(Theme.controlHeight, contentItem.implicitHeight + topPadding + bottomPadding)
    implicitWidth: contentItem.implicitWidth + leftPadding + rightPadding
    padding: 6
    spacing: Theme.s2
    hoverEnabled: true
    focusPolicy: Qt.StrongFocus
    font.family: Theme.font
    font.pixelSize: Theme.fsSm
    opacity: enabled ? 1 : 0.38
    indicator: Rectangle {
        implicitWidth: Theme.icon
        implicitHeight: Theme.icon
        x: control.mirrored ? control.width - width - control.rightPadding : control.leftPadding
        y: (control.height - height) / 2
        radius: Theme.radiusSm
        color: control.checkState === Qt.Unchecked ? Theme.bgPrimary : Theme.accentPrimary
        border.width: control.visualFocus ? 2 : control.checkState === Qt.Unchecked ? Theme.borderWidth : 0
        border.color: control.visualFocus ? Theme.accentPrimary : control.hovered ? Theme.borderHover : Theme.border
        Behavior on color { enabled: !Theme.reducedMotion; ColorAnimation { duration: Theme.motionFast } }
        SIcon { anchors.centerIn: parent; visible: control.checkState === Qt.Checked; name: 'check'; size: Theme.iconXs; strokeWidth: 3; color: Theme.textOnAccent }
        Rectangle { anchors.centerIn: parent; visible: control.checkState === Qt.PartiallyChecked; width: 8; height: 2; radius: 1; color: Theme.textOnAccent }
    }
    contentItem: Text {
        text: control.text
        font: control.font
        color: Theme.textPrimary
        verticalAlignment: Text.AlignVCenter
        horizontalAlignment: Text.AlignLeft
        wrapMode: Text.Wrap
        leftPadding: control.mirrored ? 0 : control.indicator.width + control.spacing
        rightPadding: control.mirrored ? control.indicator.width + control.spacing : 0
        elide: Text.ElideRight
    }
}

import QtQuick
import ".."
import QtQuick.Controls

// React select: same chrome as the input (px-3 py-2 text-sm rounded-lg border bg-primary, focus:border-accent)
// with a 16px chevron; options popup bg-secondary rounded-lg border, rows px-3 py-2 hover:bg-tertiary.
ComboBox {
    id: control
    property var labelMap: ({})
    property bool large: false
    function textFor(value) {
        var label = control.textRole ? value[control.textRole] : value;
        return control.labelMap[label] !== undefined ? control.labelMap[label] : label;
    }
    implicitHeight: large ? Theme.controlHeightLg : Theme.controlHeight
    implicitWidth: 140
    font.family: Theme.font
    font.pixelSize: Theme.fsSm
    hoverEnabled: true
    focusPolicy: Qt.StrongFocus

    contentItem: Text {
        text: control.displayText
        color: control.enabled ? Theme.textPrimary : Theme.textMuted
        font: control.font
        verticalAlignment: Text.AlignVCenter
        leftPadding: Theme.s3
        rightPadding: Theme.s3 + Theme.icon + Theme.s2
        elide: Text.ElideRight
    }
    indicator: SIcon {
        anchors.right: control.right
        anchors.rightMargin: Theme.s3
        anchors.verticalCenter: control.verticalCenter
        name: 'down'
        size: Theme.icon
        color: control.enabled ? Theme.textSecondary : Theme.textMuted
    }
    background: Rectangle {
        radius: Theme.radius
        color: control.enabled ? Theme.bgPrimary : Theme.bgSecondary
        border.width: control.visualFocus ? 2 : Theme.borderWidth
        border.color: control.visualFocus || control.popup.visible ? Theme.accentPrimary : control.hovered ? Theme.borderHover : Theme.border
        Behavior on border.color { enabled: !Theme.reducedMotion; ColorAnimation { duration: Theme.motionFast } }
    }
    popup: Popup {
        y: control.height + Theme.s1
        width: Math.max(control.width, 160)
        padding: Theme.s1
        implicitHeight: Math.min(contentItem.implicitHeight + Theme.s1 * 2, 280)
        background: Rectangle {
            color: Theme.bgSecondary
            border.color: Theme.border
            border.width: Theme.borderWidth
            radius: Theme.radius
        }
        contentItem: ListView {
            clip: true
            implicitHeight: contentHeight
            model: control.popup.visible ? control.delegateModel : null
            currentIndex: control.highlightedIndex
            ScrollBar.vertical: SScrollBar { }
        }
    }
    delegate: ItemDelegate {
        id: row
        required property var modelData
        required property int index
        width: control.popup.width - Theme.s1 * 2
        height: Theme.rowHeight
        contentItem: Text {
            text: control.textFor(row.modelData)
            color: control.currentIndex === row.index ? Theme.accentPrimary : Theme.textPrimary
            font: control.font
            verticalAlignment: Text.AlignVCenter
            leftPadding: Theme.s3
            rightPadding: Theme.s3
            elide: Text.ElideRight
        }
        background: Rectangle {
            radius: Theme.radiusMd
            color: row.highlighted ? Theme.bgTertiary : "transparent"
        }
        highlighted: control.highlightedIndex === index
    }
    Accessible.role: Accessible.ComboBox
}

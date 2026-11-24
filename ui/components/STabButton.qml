import QtQuick
import ".."
import QtQuick.Controls

// .branch-tab: padding 12px 16px, text-sm font-medium text-secondary, hover text-primary, max-width 180 with
// ellipsis; active = accent text + 2px accent underline sitting on the bar's 1px rule.
Button {
    id: control
    padding: 0
    property bool active: false

    function moveTab(delta) {
        var siblings = [];
        var children = parent ? parent.children : [];
        for (var i = 0; i < children.length; ++i) {
            var candidate = children[i];
            if (candidate && candidate.enabled && candidate.active !== undefined && candidate.click)
                siblings.push(candidate);
        }
        var index = siblings.indexOf(control);
        if (index < 0 || !siblings.length)
            return;
        var next = (index + delta + siblings.length) % siblings.length;
        siblings[next].forceActiveFocus(Qt.TabFocusReason);
        siblings[next].click();
    }

    onActiveFocusChanged: {
        if (activeFocus && parent && parent.parent && parent.parent.contentX !== undefined) {
            var viewport = parent.parent;
            if (x < viewport.contentX) viewport.contentX = x;
            else if (x + width > viewport.contentX + viewport.width) viewport.contentX = x + width - viewport.width;
        }
    }
    implicitHeight: Theme.branchTabHeight
    implicitWidth: Math.min(180, Math.max(48, label.implicitWidth + Theme.s4 * 2))
    hoverEnabled: true
    focusPolicy: Qt.StrongFocus

    contentItem: Text {
        id: label
        text: control.text
        font.family: Theme.font
        font.pixelSize: Theme.fsSm
        font.weight: Font.Medium
        opacity: control.enabled ? 1 : 0.38
        color: control.active ? Theme.accentPrimary : (control.hovered ? Theme.textPrimary : Theme.textSecondary)
        leftPadding: Theme.s4
        rightPadding: Theme.s4
        horizontalAlignment: Text.AlignHCenter
        verticalAlignment: Text.AlignVCenter
        elide: Text.ElideRight
        Behavior on color { enabled: !Theme.reducedMotion; ColorAnimation { duration: Theme.motionFast } }
    }

    background: Rectangle {
        color: "transparent"

        Rectangle {
            id: underline
            anchors.bottom: parent.bottom
            anchors.left: parent.left
            anchors.right: parent.right
            height: Theme.tabUnderline
            color: Theme.accentPrimary
            visible: control.active
        }

        Rectangle {
            anchors.fill: parent
            anchors.margins: 2
            color: "transparent"
            border.color: Theme.accentPrimary
            border.width: 2
            visible: control.visualFocus
            radius: Theme.radiusMd
        }
    }

    Accessible.role: Accessible.PageTab
    Accessible.name: control.text
    Accessible.selected: control.active

    Keys.onPressed: event => {
        if (!control.enabled) return;
        if (event.key === Qt.Key_Left || event.key === Qt.Key_Up) {
            control.moveTab(-1);
            event.accepted = true;
        } else if (event.key === Qt.Key_Right || event.key === Qt.Key_Down) {
            control.moveTab(1);
            event.accepted = true;
        }
    }
}

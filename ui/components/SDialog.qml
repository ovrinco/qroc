import QtQuick
import ".."
import QtQuick.Controls

Dialog {
    id: control
    // Dirty consumers set dirty=true and resolve dirtyDismissalRequested with
    // their existing Save / Discard / Cancel flow. Clean dialogs dismiss from
    // outside click or Escape through the same method.
    property bool dirty: false
    signal dirtyDismissalRequested()

    function requestDismiss() {
        if (dirty) {
            dirtyDismissalRequested();
            return;
        }
        close();
    }

    closePolicy: Popup.NoAutoClose
    focus: true
    Shortcut {
        enabled: control.visible && control.activeFocus
        sequences: [StandardKey.Cancel]
        context: Qt.WindowShortcut
        onActivated: control.requestDismiss()
    }
    Overlay.modal: Rectangle {
        color: Theme.overlay
    }
    // Observe pointer presses without adding a hit-test surface or taking
    // an exclusive grab. Popup items retain Qt's normal overlay stacking.
    function outsideTopmostPopup(position) {
        var overlay = control.Overlay.overlay;
        var popupItem = control.contentItem.parent;
        var ownIndex = overlay.children.indexOf(popupItem);
        var local = overlay.mapToItem(popupItem, position.x, position.y);
        if (local.x >= 0 && local.y >= 0 && local.x < popupItem.width && local.y < popupItem.height)
            return false;
        for (var i = 0; i < overlay.children.length; ++i) {
            var item = overlay.children[i];
            if (item === popupItem || item.solitonDismissObserver === true || !item.visible) continue;
            // A higher overlay item owns the topmost dismissal. Do not send
            // its outside press to an underlying editor's discard guard.
            if (item.z > popupItem.z || (item.z === popupItem.z && i > ownIndex))
                return false;
        }
        return true;
    }
    property Item dismissalObserver: Item {
        property bool solitonDismissObserver: true
        // A closed observer must leave the overlay entirely. Qt keeps an
        // overlay with ordinary child items alive even when those children
        // are hidden, which otherwise intercepts unrelated page clicks.
        parent: control.visible ? control.Overlay.overlay : null
        anchors.fill: parent
        z: 100000
        visible: control.visible
        // This item accepts no mouse events. The handler observes passively
        // and never takes an exclusive grab from dialog/child controls.
        PointHandler {
            property bool pressedOutside: false
            acceptedButtons: Qt.LeftButton
            onActiveChanged: {
                if (active) {
                    pressedOutside = control.outsideTopmostPopup(point.position);
                } else {
                    // Decide ownership on press, then dismiss on release. A
                    // closing top dialog cannot expose the same click to the
                    // observer beneath it.
                    var dismiss = pressedOutside;
                    pressedOutside = false;
                    if (dismiss && control.visible) control.requestDismiss();
                }
            }
            onCanceled: pressedOutside = false
        }
    }
    // ModalFrame chrome: bg-secondary rounded-xl border; header px-6 py-4 border-b with a text-xl semibold title; body p-6.
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
    padding: Theme.s6
    background: Rectangle { color: Theme.bgSecondary; radius: Theme.radiusLg; border.width: Theme.borderWidth; border.color: Theme.border }
    header: Item {
        visible: control.title.length > 0
        implicitHeight: visible ? headingText.implicitHeight + Theme.s4 * 2 : 0
        Text {
            id: headingText
            anchors.left: parent.left; anchors.right: parent.right; anchors.verticalCenter: parent.verticalCenter
            anchors.leftMargin: Theme.s6; anchors.rightMargin: Theme.s6
            text: control.title
            font.family: Theme.font; font.pixelSize: Theme.fsXl; font.weight: Font.DemiBold
            color: Theme.textPrimary
            wrapMode: Text.Wrap
        }
        Rectangle { anchors.bottom: parent.bottom; width: parent.width; height: Theme.borderWidth; color: Theme.border }
    }
}

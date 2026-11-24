import QtQuick
import ".."
import QtQuick.Controls
import QtQuick.Layouts

// Toast: fixed bottom-6 right-6, bg-secondary border rounded-lg, padding 12px 16px, shadow-lg (the 1px border
// carries separation on dark), text-sm font-medium with a 16px icon, max-w-md; hugs its content.
// Errors add a danger bar on the left and keep the dismiss button (the message stays until dismissed).
// Popup is not a layout item; safe inside ColumnLayout and StackLayout routes.
SPopup {
    id: notice
    required property Item scope
    property string text: ''
    property bool error: true
    property string iconName: ''
    property bool dismissible: true
    property string dismissedText: ''
    readonly property int maxWidth: Math.max(200, Math.min(Theme.modalMd, parent ? parent.width - Theme.s6 * 2 : Theme.modalMd))
    onTextChanged: dismissedText = ''
    onDismissed: dismissedText = text
    signal dismissed()
    parent: Overlay.overlay
    visible: scope.visible && text.length > 0 && text !== dismissedText
    modal: false; dim: false; focus: false
    closePolicy: Popup.NoAutoClose
    leftPadding: Theme.s4 + (error ? 3 : 0)
    rightPadding: dismissible ? Theme.s2 : Theme.s4
    topPadding: Theme.s3
    bottomPadding: Theme.s3
    width: Math.min(implicitWidth, maxWidth)
    x: parent ? Math.max(Theme.s6, parent.width - width - Theme.s6) : 0
    y: parent ? Math.max(Theme.s6, parent.height - height - Theme.s6) : 0
    background: Rectangle {
        color: Theme.bgSecondary
        radius: Theme.radius
        border.width: Theme.borderWidth
        border.color: Theme.border
        Rectangle { visible: notice.error; x: 1; y: 1; width: 3; height: parent.height - 2; radius: 1; color: Theme.danger }
    }
    contentItem: RowLayout {
        spacing: Theme.s3
        SIcon {
            visible: notice.error || !!notice.iconName
            name: notice.error && !notice.iconName ? 'alert-circle' : notice.iconName
            size: Theme.icon
            color: notice.error ? Theme.danger : Theme.textPrimary
            Layout.alignment: Qt.AlignVCenter
        }
        Text {
            id: noticeText
            Layout.fillWidth: true
            Layout.minimumHeight: 24
            text: notice.text
            wrapMode: Text.Wrap
            maximumLineCount: 10
            elide: Text.ElideRight
            textFormat: Text.PlainText
            verticalAlignment: Text.AlignVCenter
            color: Theme.textPrimary
            font.family: Theme.font; font.pixelSize: Theme.fsSm; font.weight: Font.Medium
            Accessible.role: Accessible.AlertMessage
        }
        SButton {
            visible: notice.dismissible
            width: 24; height: 24; implicitWidth: 24; implicitHeight: 24
            size: 'sm'; text: ''; iconName: 'close'; quiet: true
            accessibleLabel: 'Dismiss message'
            Layout.alignment: Qt.AlignVCenter
            onClicked: notice.dismissed()
        }
    }
}

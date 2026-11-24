import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import ".."

// React modal chrome: fixed inset-0 bg-black/50; panel bg-secondary rounded-xl border;
// header px-6 py-4 border-b (optional icon tile, title text-xl semibold, subtitle text-xs, X);
// body p-6 (scrolls when tall); footer px-6 py-4 border-t right-aligned actions.
Popup {
    id: frame
    property string title: ''
    property string subtitle: ''
    property string icon: ''
    property color iconTone: Theme.accentPrimary
    property int maxWidth: Theme.modal2xl
    property bool showClose: true
    default property alias content: body.data
    property alias footer: footerRow.data
    property alias bodySpacing: body.spacing
    readonly property int hostWidth: parent ? parent.width : maxWidth + 32
    readonly property int hostHeight: parent ? parent.height : 900
    parent: Overlay.overlay
    anchors.centerIn: parent
    modal: true
    dim: true
    focus: true
    padding: 0
    closePolicy: Popup.CloseOnEscape | Popup.CloseOnPressOutside
    width: Math.min(maxWidth, hostWidth - Theme.s8)
    height: Math.min(column.implicitHeight, hostHeight - Theme.s8)
    Overlay.modal: Rectangle { color: Theme.overlay }
    background: Rectangle {
        color: Theme.bgSecondary
        radius: Theme.radiusLg
        border.width: Theme.borderWidth
        border.color: Theme.border
    }
    contentItem: ColumnLayout {
        id: column
        spacing: 0
        Item {
            visible: frame.title.length > 0
            Layout.fillWidth: true
            implicitHeight: headerRow.implicitHeight + Theme.s4 * 2
            RowLayout {
                id: headerRow
                anchors.fill: parent
                anchors.leftMargin: Theme.s6; anchors.rightMargin: Theme.s6
                spacing: Theme.s3
                IconTile { visible: !!frame.icon; icon: frame.icon; tone: frame.iconTone }
                ColumnLayout {
                    Layout.fillWidth: true
                    spacing: 2
                    Text { Layout.fillWidth: true; text: frame.title; font.family: Theme.font; font.pixelSize: Theme.fsXl; font.weight: Font.DemiBold; color: Theme.textPrimary; elide: Text.ElideRight }
                    Text { visible: !!frame.subtitle; Layout.fillWidth: true; text: frame.subtitle; font.family: Theme.font; font.pixelSize: Theme.fsXs; color: Theme.textSecondary; wrapMode: Text.Wrap }
                }
                SButton { visible: frame.showClose; text: ''; iconName: 'close'; quiet: true; accessibleLabel: 'Close'; onClicked: frame.close() }
            }
            Rectangle { anchors.bottom: parent.bottom; width: parent.width; height: Theme.borderWidth; color: Theme.border }
        }
        ScrollView {
            id: scroller
            Layout.fillWidth: true
            Layout.fillHeight: true
            Layout.minimumHeight: 0
            implicitHeight: contentHeight
            contentWidth: availableWidth
            contentHeight: body.implicitHeight + Theme.s6 * 2
            clip: true
            ColumnLayout {
                id: body
                x: Theme.s6
                y: Theme.s6
                width: scroller.availableWidth - Theme.s6 * 2
                spacing: Theme.s4
            }
        }
        Item {
            visible: footerRow.children.length > 0
            Layout.fillWidth: true
            implicitHeight: footerRow.implicitHeight + Theme.s4 * 2
            Rectangle { anchors.top: parent.top; width: parent.width; height: Theme.borderWidth; color: Theme.border }
            RowLayout {
                id: footerRow
                anchors.right: parent.right
                anchors.rightMargin: Theme.s6
                anchors.verticalCenter: parent.verticalCenter
                spacing: Theme.s3
            }
        }
    }
}

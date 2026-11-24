import QtQuick
import QtQuick.Layouts
import ".."

// ProjectView DomainEmptyState / DashboardTab empty / CascadeProgressBar EmptyDocumentState:
// centred card (max-w-xl) with icon tile, eyebrow, title, detail, optional fact tiles and a primary action.
Item {
    id: state
    property string icon: 'layers'
    property color tone: Theme.accentPrimary
    property string eyebrow: ''
    property string title: ''
    property string detail: ''
    property string actionText: ''
    property string actionIcon: 'plus'
    property var facts: []
    property bool card: true
    signal actionRequested()
    implicitHeight: box.implicitHeight + Theme.s8 * 2
    Rectangle {
        id: box
        anchors.centerIn: parent
        width: Math.min(Theme.modalXl, Math.max(240, parent.width - Theme.s8 * 2))
        color: state.card ? Theme.bgSecondary : 'transparent'
        radius: Theme.radius
        border.width: state.card ? Theme.borderWidth : 0
        border.color: Theme.border
        implicitHeight: col.implicitHeight + Theme.s6 * 2
        ColumnLayout {
            id: col
            anchors.fill: parent
            anchors.margins: Theme.s6
            spacing: 0
            IconTile { Layout.alignment: Qt.AlignHCenter; icon: state.icon; tone: state.tone; size: 56; iconSize: Theme.iconLg; tileRadius: Theme.radius }
            EyebrowLabel { visible: !!state.eyebrow; Layout.topMargin: Theme.s5; Layout.alignment: Qt.AlignHCenter; text: state.eyebrow }
            Text { Layout.topMargin: Theme.s2; Layout.fillWidth: true; text: state.title; font.family: Theme.font; font.pixelSize: Theme.fsLg; font.weight: Font.DemiBold; color: Theme.textPrimary; horizontalAlignment: Text.AlignHCenter; wrapMode: Text.Wrap }
            Text { visible: !!state.detail; Layout.topMargin: Theme.s2; Layout.fillWidth: true; text: state.detail; font.family: Theme.font; font.pixelSize: Theme.fsSm; color: Theme.textSecondary; horizontalAlignment: Text.AlignHCenter; wrapMode: Text.Wrap }
            GridLayout {
                visible: state.facts.length > 0
                Layout.topMargin: Theme.s5
                Layout.fillWidth: true
                columns: 2
                columnSpacing: Theme.s2
                rowSpacing: Theme.s2
                Repeater {
                    model: state.facts
                    delegate: Rectangle {
                        required property var modelData
                        Layout.fillWidth: true
                        implicitHeight: 54
                        color: Theme.bgPrimary
                        radius: Theme.radiusSm
                        border.width: Theme.borderWidth
                        border.color: Theme.border
                        ColumnLayout {
                            anchors.centerIn: parent
                            spacing: Theme.s1
                            SIcon { Layout.alignment: Qt.AlignHCenter; name: modelData.icon || 'file'; size: Theme.icon; color: Theme.textMuted }
                            Text { Layout.alignment: Qt.AlignHCenter; text: modelData.label || ''; font.family: Theme.font; font.pixelSize: Theme.fsXs; color: Theme.textSecondary }
                        }
                    }
                }
            }
            SButton { visible: !!state.actionText; Layout.topMargin: Theme.s6; Layout.alignment: Qt.AlignHCenter; text: state.actionText; iconName: state.actionIcon; primary: true; onClicked: state.actionRequested() }
        }
    }
}

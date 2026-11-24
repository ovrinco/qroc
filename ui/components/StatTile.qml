import QtQuick
import QtQuick.Layouts
import ".."

// DashboardView Metric: rounded border bg-primary px-3 py-2; eyebrow row with a 14px icon; mono text-lg semibold value.
Rectangle {
    id: tile
    property string label: ''
    property string value: ''
    property string icon: ''
    property color valueColor: Theme.textPrimary
    property bool centered: false
    color: Theme.bgPrimary
    radius: Theme.radiusSm
    border.width: Theme.borderWidth
    border.color: Theme.border
    implicitWidth: 120
    implicitHeight: col.implicitHeight + Theme.s2 * 2
    ColumnLayout {
        id: col
        anchors.fill: parent
        anchors.leftMargin: Theme.s3; anchors.rightMargin: Theme.s3
        anchors.topMargin: Theme.s2; anchors.bottomMargin: Theme.s2
        spacing: Theme.s2
        RowLayout {
            Layout.fillWidth: true
            Layout.alignment: tile.centered ? Qt.AlignHCenter : Qt.AlignLeft
            spacing: Theme.s2
            visible: !tile.centered
            SIcon { visible: !!tile.icon; name: tile.icon; size: Theme.iconSm; color: Theme.textMuted }
            EyebrowLabel { text: tile.label; Layout.fillWidth: true }
        }
        Text {
            Layout.fillWidth: true
            text: tile.value
            font.family: Theme.mono; font.pixelSize: Theme.fsLg; font.weight: Font.DemiBold
            color: tile.valueColor
            horizontalAlignment: tile.centered ? Text.AlignHCenter : Text.AlignLeft
            elide: Text.ElideRight
        }
        EyebrowLabel { visible: tile.centered; Layout.fillWidth: true; text: tile.label; horizontalAlignment: Text.AlignHCenter; font.letterSpacing: Theme.trackingWide(Theme.fsMicro) }
    }
}

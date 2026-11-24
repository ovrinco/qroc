import QtQuick
import QtQuick.Layouts
import ".."

// h-14 bg-secondary border-b: page title left, actions right.
// large: dashboard header (text-xl bold, px-6); otherwise project header (text-base semibold, px-4).
Rectangle {
    id: header
    property string title: ''
    property bool large: false
    default property alias actions: actionsRow.data
    implicitHeight: Theme.headerHeight
    color: Theme.bgSecondary
    Rectangle { anchors.bottom: parent.bottom; width: parent.width; height: Theme.borderWidth; color: Theme.border }
    RowLayout {
        anchors.fill: parent
        anchors.leftMargin: header.large ? Theme.s6 : Theme.s4
        anchors.rightMargin: Theme.s4
        spacing: Theme.s2
        Text {
            Layout.fillWidth: true
            text: header.title
            font.family: Theme.font
            font.pixelSize: header.large ? Theme.fsXl : Theme.fsBase
            font.weight: header.large ? Font.Bold : Font.DemiBold
            color: Theme.textPrimary
            elide: Text.ElideRight
        }
        RowLayout { id: actionsRow; spacing: Theme.s2 }
    }
}

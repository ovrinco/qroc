import QtQuick
import ".."
import QtQuick.Layouts

// .branch-tab-bar: flex, gap 0, border-bottom 1px, horizontal overflow scrolls without a visible bar.
Item {
    id: bar
    implicitHeight: Theme.branchTabHeight
    implicitWidth: layout.implicitWidth
    default property alias content: layout.data
    property alias spacing: layout.spacing
    Flickable {
        id: viewport
        anchors.fill: parent
        contentWidth: layout.width; contentHeight: height
        clip: true
        boundsBehavior: Flickable.StopAtBounds
        flickableDirection: Flickable.HorizontalFlick
        RowLayout {
            id: layout
            width: Math.max(viewport.width, implicitWidth)
            height: viewport.height
            spacing: 0
        }
    }
    Rectangle { anchors.bottom: parent.bottom; width: parent.width; height: Theme.borderWidth; color: Theme.border; z: -1 }
    Accessible.role: Accessible.PageTabList
}

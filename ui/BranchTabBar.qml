import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

// Branch strip (BranchTabBar.tsx + .branch-tab): Dashboard, one tab per branch (max 180px, ellipsis) and a "+" tab.
// Active = accent text and a 2px accent underline; the strip sits on a 1px border rule and scrolls sideways.
Item {
    id: root
    property var workspace: null
    readonly property string activeTab: workspace ? workspace.branchTab : 'dashboard'
    implicitHeight: Theme.branchTabHeight + 2

    component BranchTab: Button {
        id: tab
        property bool active: false
        height: Theme.branchTabHeight
        padding: 0
        hoverEnabled: true
        focusPolicy: Qt.StrongFocus
        implicitWidth: Math.min(180, label.implicitWidth)
        contentItem: Text {
            id: label
            text: tab.text
            leftPadding: Theme.s4; rightPadding: Theme.s4
            verticalAlignment: Text.AlignVCenter
            font.family: Theme.font; font.pixelSize: Theme.fsSm; font.weight: Font.Medium
            color: tab.active ? Theme.accentPrimary : tab.hovered ? Theme.textPrimary : Theme.textSecondary
            elide: Text.ElideRight
        }
        background: Rectangle {
            color: 'transparent'
            Rectangle { visible: tab.active; anchors.bottom: parent.bottom; width: parent.width; height: Theme.tabUnderline; color: Theme.accentPrimary }
            Rectangle { anchors.fill: parent; visible: tab.visualFocus; color: 'transparent'; border.width: 1; border.color: Theme.accentPrimary }
        }
        ToolTip.visible: hovered && label.truncated
        ToolTip.text: text
        ToolTip.delay: 600
        Accessible.role: Accessible.PageTab
        Accessible.name: text
        Accessible.selected: active
    }

    Flickable {
        anchors.fill: parent
        anchors.bottomMargin: 2
        contentWidth: strip.width
        contentHeight: height
        clip: true
        boundsBehavior: Flickable.StopAtBounds
        flickableDirection: Flickable.HorizontalFlick
        Row {
            id: strip
            height: parent.height
            BranchTab { text: 'Dashboard'; active: root.activeTab === 'dashboard'; onClicked: if (root.workspace) root.workspace.selectBranchTab('dashboard') }
            Repeater {
                model: root.workspace ? root.workspace.branches : []
                delegate: BranchTab {
                    required property var modelData
                    text: modelData.name
                    active: root.activeTab === modelData.id
                    onClicked: root.workspace.selectBranchTab(modelData.id)
                }
            }
            Button {
                id: add
                objectName: 'qroc-new-branch'
                height: Theme.branchTabHeight
                implicitWidth: Theme.s3 * 2 + Theme.icon
                padding: 0
                hoverEnabled: true
                focusPolicy: Qt.StrongFocus
                contentItem: Item { SIcon { anchors.centerIn: parent; name: 'plus'; size: Theme.icon; color: add.hovered ? Theme.accentPrimary : Theme.textMuted } }
                background: Rectangle { color: 'transparent'; border.width: add.visualFocus ? 1 : 0; border.color: Theme.accentPrimary }
                ToolTip.visible: hovered
                ToolTip.text: 'Add branch'
                ToolTip.delay: 600
                Accessible.role: Accessible.Button
                Accessible.name: 'Add branch'
                onClicked: if (root.workspace) root.workspace.openNewBranch()
            }
        }
    }
    Rectangle { anchors.bottom: parent.bottom; width: parent.width; height: Theme.borderWidth; color: Theme.border }
}

import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import ".."

// React Sidebar.tsx: w-64 (w-16 collapsed) bg-secondary border-r; h-14 logo row (pyramid mark + "Q-ROC" in accent,
// collapse chevron); project list (folder icon + left-justified title, active = accent/10 fill + accent text);
// "+ New Project" directly below the last project; footer border-t p-3 with page rows (active = bg-tertiary).
Rectangle {
    id: bar
    property var projects: []
    property string activeProjectId: ''
    property bool projectOpen: true
    property bool collapsed: false
    property string currentPage: 'workspace'
    property var pages: [
        { id: 'phod', label: 'Phod', icon: 'cpu' },
        { id: 'creative', label: 'Creative', icon: 'sparkles' },
        { id: 'settings', label: 'Settings', icon: 'gear' }
    ]
    signal selectProject(string id)
    signal newProject()
    signal goHome()
    signal selectPage(string name)
    signal toggleCollapsed()
    implicitWidth: collapsed ? Theme.sidebarRail : Theme.sidebarWidth
    color: Theme.bgSecondary
    Behavior on implicitWidth { enabled: !Theme.reducedMotion; NumberAnimation { duration: Theme.motionSlow; easing.type: Easing.InOutQuad } }
    Rectangle { anchors.right: parent.right; width: Theme.borderWidth; height: parent.height; color: Theme.border }

    component SidebarRow: AbstractButton {
        id: row
        property string symbol: ''
        property bool active: false
        property bool accentActive: false
        property string avatar: ''
        property int gap: Theme.s3            // gap-3 project and footer rows; gap-2 for "New Project"
        property int railIconSize: Theme.icon // w-5 plus when collapsed
        readonly property bool rail: bar.collapsed
        readonly property color ink: active && accentActive ? Theme.accentPrimary : (active || hovered) ? Theme.textPrimary : Theme.textSecondary
        implicitHeight: Theme.rowHeight
        padding: 0
        hoverEnabled: true
        focusPolicy: Qt.StrongFocus
        background: Rectangle {
            radius: Theme.radius
            // Active project: the spec PNG (react_real_project.png, row box x12-242 y72-107) measures the row at
            // bg-secondary (20,20,20); the state is carried by the accent text + icon, never by an accent-alpha fill
            // (T05 B1: accent/10 = (31,29,34) is outside the compare palette). Footer rows keep bg-tertiary.
            color: (row.active && !row.accentActive) || row.hovered ? Theme.bgTertiary : 'transparent'
            // *:focus-visible { outline: 2px solid var(--accent-primary) }
            Rectangle { visible: row.visualFocus; anchors.fill: parent; color: 'transparent'; radius: Theme.radius; border.width: 2; border.color: Theme.accentPrimary }
            Behavior on color { enabled: !Theme.reducedMotion; ColorAnimation { duration: Theme.motionFast } }
        }
        contentItem: Item {
            SIcon {
                visible: !row.avatar || !row.rail
                name: row.symbol
                size: row.rail ? row.railIconSize : Theme.icon
                color: row.ink
                anchors.verticalCenter: parent.verticalCenter
                x: row.rail ? (parent.width - width) / 2 : Theme.s3
            }
            Rectangle {
                visible: !!row.avatar && row.rail
                width: 24; height: 24; radius: Theme.radiusSm
                anchors.centerIn: parent
                color: Theme.bgTertiary
                Text { anchors.centerIn: parent; text: row.avatar; font.family: Theme.font; font.pixelSize: Theme.fsMicro; font.weight: Font.Bold; color: row.active ? Theme.accentPrimary : Theme.textSecondary }
            }
            Text {
                visible: !row.rail
                x: Theme.s3 + Theme.icon + row.gap
                width: Math.max(0, parent.width - x - Theme.s3)
                anchors.verticalCenter: parent.verticalCenter
                text: row.text
                font.family: Theme.font; font.pixelSize: Theme.fsSm
                color: row.ink
                elide: Text.ElideRight
            }
        }
        ToolTip.visible: row.rail && row.hovered
        ToolTip.text: row.text
        ToolTip.delay: 600
        Accessible.role: Accessible.Button
        Accessible.name: row.text
    }

    ColumnLayout {
        anchors.fill: parent
        anchors.rightMargin: Theme.borderWidth
        spacing: 0
        Item {
            Layout.fillWidth: true
            implicitHeight: Theme.headerHeight
            Rectangle { anchors.bottom: parent.bottom; width: parent.width; height: Theme.borderWidth; color: Theme.border }
            AbstractButton {
                id: logo
                objectName: 'sidebarLogo'
                anchors.verticalCenter: parent.verticalCenter
                anchors.left: parent.left
                anchors.leftMargin: bar.collapsed ? (parent.width - width) / 2 : Theme.s4
                width: bar.collapsed ? 36 : logoRow.implicitWidth + Theme.s2
                height: 36
                hoverEnabled: true
                padding: 0
                onClicked: bar.collapsed ? bar.toggleCollapsed() : bar.goHome()
                background: Rectangle { radius: Theme.radiusMd; color: bar.collapsed && logo.hovered ? Theme.bgTertiary : 'transparent' }
                contentItem: Item {
                    RowLayout {
                        id: logoRow
                        anchors.verticalCenter: parent.verticalCenter
                        x: bar.collapsed ? (parent.width - implicitWidth) / 2 : 0
                        spacing: Theme.s2
                        SIcon { name: bar.collapsed && logo.hovered ? 'right' : 'pyramid'; size: Theme.iconMd; strokeWidth: 1.5; color: Theme.accentPrimary }
                        Text { visible: !bar.collapsed; text: 'QROC'; font.family: Theme.font; font.pixelSize: Theme.fsXl; font.weight: Font.Bold; color: Theme.accentPrimary; opacity: logo.hovered ? 0.85 : 1 }
                    }
                }
                ToolTip.visible: hovered
                ToolTip.text: bar.collapsed ? 'Expand sidebar' : 'Go to Dashboard'
                ToolTip.delay: 600
                Accessible.role: Accessible.Button
                Accessible.name: bar.collapsed ? 'Expand sidebar' : 'Go to the QROC dashboard'
            }
            // p-1 text-secondary hover:text-primary with a w-4 chevron, inside the px-4 header.
            SButton {
                objectName: 'sidebarCollapse'
                visible: !bar.collapsed
                anchors.right: parent.right
                anchors.rightMargin: Theme.s4
                anchors.verticalCenter: parent.verticalCenter
                width: 24; height: 24; implicitWidth: 24; implicitHeight: 24
                text: ''; iconName: 'left'; quiet: true; size: 'sm'
                accessibleLabel: 'Collapse sidebar'
                onClicked: bar.toggleCollapsed()
            }
        }
        ScrollView {
            id: projectScroll
            Layout.fillWidth: true
            Layout.fillHeight: true
            contentWidth: availableWidth
            contentHeight: projectColumn.implicitHeight + Theme.s4 * 2
            clip: true
            ColumnLayout {
                id: projectColumn
                x: Theme.s3
                y: Theme.s4
                width: projectScroll.availableWidth - Theme.s3 * 2
                spacing: Theme.s1
                Repeater {
                    model: bar.projects
                    delegate: SidebarRow {
                        required property var modelData
                        Layout.fillWidth: true
                        objectName: 'sidebarProject:' + (modelData.title || modelData.name || '')
                        text: modelData.title || modelData.name || ''
                        symbol: modelData.kind === 'media' ? 'mic' : 'folder'
                        avatar: String(text).slice(0, 2).toUpperCase()
                        active: bar.projectOpen && bar.currentPage === 'workspace' && bar.activeProjectId === modelData.id
                        accentActive: true
                        onClicked: bar.selectProject(modelData.id)
                    }
                }
                SidebarRow {
                    Layout.fillWidth: true
                    Layout.topMargin: bar.projects.length ? Theme.s1 : 0
                    objectName: 'sidebarNewProject'
                    text: 'New Project'
                    symbol: 'plus'
                    gap: Theme.s2
                    railIconSize: Theme.iconMd
                    onClicked: bar.newProject()
                }
            }
        }
        Rectangle { Layout.fillWidth: true; implicitHeight: Theme.borderWidth; color: Theme.border }
        ColumnLayout {
            Layout.fillWidth: true
            Layout.margins: Theme.s3
            spacing: Theme.s1
            Repeater {
                model: bar.pages
                delegate: SidebarRow {
                    required property var modelData
                    Layout.fillWidth: true
                    objectName: 'sidebarPage:' + modelData.id
                    text: modelData.label
                    symbol: modelData.icon
                    active: bar.currentPage === modelData.id
                    onClicked: bar.selectPage(modelData.id)
                }
            }
        }
    }
    Accessible.role: Accessible.Pane
    Accessible.name: 'Navigation'
}

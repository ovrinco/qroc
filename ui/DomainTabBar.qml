import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

// Project tab strip (DomainHeader.tsx nav): Dashboard and KB fixed width with a right rule, then the four domain
// tabs share the remaining width. Active = label and 2px underline in the domain colour (accent for Dashboard/KB).
// Count chips ("1b 3/16") hide under the xl breakpoint of the window, as the React design's `xl:inline`;
// the chip keeps the tab's medium weight; a failed gate turns it red at a 10 % fill (bg-red-500/10).
Item {
    id: root
    property var workspace: null
    readonly property string view: workspace ? workspace.projectView : 'DASHBOARD'
    readonly property bool showChips: (root.Window.width || width) >= Theme.breakpointXl
    readonly property bool iconsOnly: width < Theme.breakpointSm
    readonly property var domains: ['PRODUCT', 'BUSINESS', 'PRESENCE', 'LEGAL']
    // A media project's form of the strip (design sheet 9.2): Brief · Lyrics · Concepts with the rule, then Versions
    // across the remaining width, all in the accent tone; the version count chip turns red when a file is missing.
    readonly property bool media: !!workspace && workspace.isMedia
    readonly property var domainIcons: ({PRODUCT: 'box', BUSINESS: 'briefcase', PRESENCE: 'globe', LEGAL: 'shield'})
    implicitHeight: Theme.domainTabHeight + 3

    component ProjectTab: Button {
        id: tab
        property string glyph: ''
        property color tone: Theme.accentPrimary
        property bool active: false
        property bool rule: false
        property string chip: ''
        property bool chipDanger: false
        Layout.fillHeight: true
        padding: 0
        hoverEnabled: true
        focusPolicy: Qt.StrongFocus
        implicitWidth: row.implicitWidth + (root.iconsOnly ? Theme.s3 : Theme.s4) * 2
        ToolTip.visible: root.iconsOnly && hovered
        ToolTip.text: text
        ToolTip.delay: 600
        contentItem: Item {
            implicitWidth: row.implicitWidth
            RowLayout {
                id: row
                anchors.centerIn: parent
                spacing: Theme.s2
                SIcon { name: tab.glyph; size: Theme.icon; color: label.color }
                Text {
                    id: label
                    visible: !root.iconsOnly
                    text: tab.text
                    font.family: Theme.font; font.pixelSize: Theme.fsSm; font.weight: Font.Medium
                    color: tab.active ? tab.tone : tab.hovered ? Theme.textPrimary : Theme.textSecondary
                }
                Pill { visible: !!tab.chip; text: tab.chip; mono: true; weight: Font.Medium; neutral: !tab.chipDanger; tone: Theme.danger; fillAlpha: 0.10 }
            }
        }
        background: Rectangle {
            color: 'transparent'
            Rectangle { visible: tab.rule; anchors.right: parent.right; height: parent.height; width: Theme.borderWidth; color: Theme.border }
            Rectangle { visible: tab.active; anchors.bottom: parent.bottom; width: parent.width; height: Theme.tabUnderline; color: tab.tone }
            Rectangle { anchors.fill: parent; visible: tab.visualFocus; color: 'transparent'; border.width: 1; border.color: Theme.accentPrimary }
        }
        Accessible.role: Accessible.PageTab
        Accessible.name: text
        Accessible.selected: active
    }

    Rectangle { anchors.fill: parent; color: Theme.bgSecondary }
    RowLayout {
        anchors.fill: parent
        anchors.bottomMargin: 3
        spacing: 0
        Repeater {
            model: root.media ? [['BRIEF', 'Brief'], ['LYRICS', 'Lyrics'], ['CONCEPTS', 'Concepts']] : []
            delegate: ProjectTab {
                required property var modelData
                text: modelData[1]; glyph: 'edit'; rule: true
                active: root.view === modelData[0]
                onClicked: root.workspace.media.showTab(modelData[0])
            }
        }
        ProjectTab {
            visible: root.media
            Layout.fillWidth: true
            text: 'Versions'; glyph: 'mic'
            active: root.view === 'VERSIONS'
            chip: root.media && root.showChips && root.workspace.media.versionsLoaded ? String(root.workspace.media.versions.length) : ''
            chipDanger: root.media && root.workspace.media.missingCount > 0
            onClicked: root.workspace.media.showTab('VERSIONS')
        }
        ProjectTab { visible: !root.media; text: 'Dashboard'; glyph: 'dashboard'; rule: true; active: root.view === 'DASHBOARD'; onClicked: if (root.workspace) root.workspace.showDashboard() }
        ProjectTab { visible: !root.media; text: 'Pipeline'; glyph: 'layers'; rule: true; active: root.view === 'PIPELINE'; onClicked: if (root.workspace) root.workspace.showPipeline() }
        ProjectTab { visible: !root.media; text: 'KB'; glyph: 'book'; rule: true; active: root.view === 'KB'; onClicked: if (root.workspace) root.workspace.showKnowledge() }
        Repeater {
            model: root.media ? [] : root.domains
            delegate: ProjectTab {
                required property string modelData
                readonly property var summary: root.workspace ? root.workspace.domainSummary(modelData) : null
                Layout.fillWidth: true
                Layout.preferredWidth: 1
                Layout.minimumWidth: 0
                text: Theme.domainLabel(modelData)
                glyph: root.domainIcons[modelData]
                tone: Theme.domainColor(modelData)
                active: root.view === modelData
                chip: root.showChips && summary && !root.workspace.projectLoading ? summary.branches + 'b ' + summary.bestCompleted + '/' + summary.total : ''
                chipDanger: !!summary && summary.blocked
                onClicked: if (root.workspace) root.workspace.showDomain(modelData)
            }
        }
    }
    Rectangle { anchors.bottom: parent.bottom; width: parent.width; height: Theme.borderWidth; color: Theme.border }
}

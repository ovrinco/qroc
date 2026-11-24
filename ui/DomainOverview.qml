import QtQuick
import QtQuick.Controls as QC
import QtQuick.Layouts

// Branch overview for the active domain (DomainDashboard + .branch-card): one card per branch with its gate state,
// document coverage and progress. A card opens that branch's cascade.
Flickable {
    id: root
    property var workspace: null
    readonly property var w: workspace
    readonly property string domain: w ? w.domain : 'PRODUCT'
    readonly property int total: w ? (w.referenceDocuments[domain] || []).length : 0
    readonly property string standing: w ? w.domainStanding(domain) : ''
    readonly property real colWidth: Math.min(Theme.contentMaxWidth, width)
    readonly property int cardColumns: Math.max(1, Math.min(3, Math.floor((colWidth - Theme.s6 * 2 + Theme.s4) / (280 + Theme.s4))))
    clip: true
    contentWidth: width
    contentHeight: column.implicitHeight + Theme.s6 * 2
    boundsBehavior: Flickable.StopAtBounds
    QC.ScrollBar.vertical: SScrollBar {}

    ColumnLayout {
        id: column
        x: Math.max(0, (root.width - root.colWidth) / 2) + Theme.s6
        y: Theme.s6
        width: root.colWidth - Theme.s6 * 2
        spacing: Theme.s4
        RowLayout {
            Layout.fillWidth: true
            spacing: Theme.s2
            DomainDot { domain: root.domain }
            Text { text: Theme.domainLabel(root.domain) + ' branches'; font.family: Theme.font; font.pixelSize: Theme.fsLg; font.weight: Font.DemiBold; color: Theme.textPrimary }
            Pill { visible: root.standing === 'predicted'; neutral: true; text: 'Predicted' }
            Item { Layout.fillWidth: true }
            SButton { text: 'Synthesize'; iconName: 'git-branch'; enabled: !!root.w && root.w.canSynthesize; onClicked: root.w.openSynthesis() }
            SButton { text: 'Add branch'; iconName: 'plus'; onClicked: if (root.w) root.w.openNewBranch() }
        }
        GridLayout {
            Layout.fillWidth: true
            columns: root.cardColumns
            columnSpacing: Theme.s4; rowSpacing: Theme.s4
            uniformCellWidths: true
            Repeater {
                model: root.w ? root.w.branches : []
                delegate: Rectangle {
                    id: card
                    required property var modelData
                    readonly property int count: root.w.branchDocCount(modelData.id, root.domain)
                    readonly property string gate: root.w.branchGateState(modelData.id, root.domain)
                    readonly property bool complete: root.total > 0 && count >= root.total
                    Layout.fillWidth: true
                    implicitHeight: body.implicitHeight + Theme.s4 * 2
                    radius: Theme.radiusLg
                    color: Theme.bgSecondary
                    border.width: Theme.borderWidth
                    border.color: hover.hovered ? Theme.accentPrimary : Theme.border
                    HoverHandler { id: hover }
                    TapHandler { onTapped: root.w.selectBranchTab(card.modelData.id) }
                    ColumnLayout {
                        id: body
                        anchors.fill: parent
                        anchors.margins: Theme.s4
                        spacing: Theme.s3
                        Text { Layout.fillWidth: true; text: card.modelData.name; font.family: Theme.font; font.pixelSize: Theme.fsSm; font.weight: Font.DemiBold; color: Theme.textPrimary; elide: Text.ElideRight }
                        RowLayout {
                            spacing: Theme.s2
                            Text { text: 'Gate:'; font.family: Theme.font; font.pixelSize: Theme.fsXs; color: Theme.textSecondary }
                            Pill {
                                text: card.gate === 'PASSED' ? 'Passed' : card.gate === 'FAILED' ? 'Failed' : root.standing === 'predicted' && card.count === 0 ? 'Predicted' : 'Pending'
                                neutral: card.gate === 'PENDING'
                                tone: card.gate === 'PASSED' ? Theme.success : Theme.danger
                            }
                        }
                        RowLayout {
                            Layout.fillWidth: true
                            Text { Layout.fillWidth: true; text: 'Documents'; font.family: Theme.font; font.pixelSize: Theme.fsXs; color: Theme.textSecondary }
                            Text { text: card.count + '/' + root.total; font.family: Theme.mono; font.pixelSize: Theme.fsXs; color: Theme.textPrimary }
                        }
                        ProgressBar { Layout.fillWidth: true; value: root.total > 0 ? card.count / root.total : 0; tone: card.complete ? Theme.success : Theme.accentPrimary }
                    }
                }
            }
        }
    }
}

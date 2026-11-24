import QtQuick
import QtQuick.Controls as QC
import QtQuick.Layouts

// Project dashboard (DashboardView.tsx): brief + stat tiles, next action, four domain cards, branch comparison and
// gate state. Every number is derived from the workspace root (branches, allDocuments, project.gate_reviews).
Flickable {
    id: root
    property var workspace: null
    readonly property var w: workspace
    readonly property real colWidth: Math.min(Theme.contentMaxWidth, width)
    readonly property bool wide: width >= Theme.breakpointXl
    readonly property int domainColumns: width >= Theme.sidebarAutoCollapse ? 4 : width >= Theme.breakpointSm ? 2 : 1
    readonly property var summaries: w ? ['PRODUCT', 'BUSINESS', 'PRESENCE', 'LEGAL'].map(d => w.domainSummary(d)) : []
    readonly property var gates: w ? w.gateSummary() : ({passed: 0, pending: 0, failed: 0, recent: []})
    readonly property var next: w ? w.nextAction() : ({kind: 'create', domain: 'PRODUCT', title: '', detail: ''})
    readonly property var domainIntent: ({PRODUCT: 'Body and architecture', BUSINESS: 'Market and revenue', PRESENCE: 'Voice and acquisition', LEGAL: 'Protection and terms'})
    readonly property var statusTone: ({EXECUTING: Theme.executing, SUSPENDED: Theme.warning, COMPLETED: Theme.success, FAILED: Theme.danger})
    function statusLabel(status) { var t = String(status || 'pending'); return t.charAt(0).toUpperCase() + t.slice(1).toLowerCase(); }
    function purpose() {
        var text = w && w.project ? Theme.readableStored(w.project.brain_dump || '').replace(/\s+/g, ' ').trim() : '';
        if (!text) return 'No project brief captured yet.';
        return text.length > 260 ? text.slice(0, 257) + '...' : text;
    }
    clip: true
    contentWidth: width
    contentHeight: column.visible ? column.implicitHeight + Theme.s6 * 2 : height
    boundsBehavior: Flickable.StopAtBounds
    QC.ScrollBar.vertical: SScrollBar {}

    // While the project loads the dashboard says so; no count, gate or next action is shown before the store's are in.
    EmptyState {
        visible: !!root.w && root.w.projectLoading
        width: root.width; height: root.height
        card: false
        icon: 'loader'
        tone: Theme.textSecondary
        title: 'Loading the project'
        detail: 'Reading its documents and gates.'
    }
    ColumnLayout {
        id: column
        visible: !root.w || !root.w.projectLoading
        x: Math.max(0, (root.width - root.colWidth) / 2) + Theme.s6
        y: Theme.s6
        width: root.colWidth - Theme.s6 * 2
        spacing: Theme.s5

        GridLayout {
            Layout.fillWidth: true
            columns: root.wide ? 2 : 1
            columnSpacing: Theme.s4
            rowSpacing: Theme.s4
            Card {
                Layout.fillWidth: true
                Layout.fillHeight: true
                padding: Theme.s5
                spacing: 0
                RowLayout {
                    Layout.fillWidth: true
                    spacing: Theme.s4
                    ColumnLayout {
                        Layout.fillWidth: true
                        spacing: 0
                        EyebrowLabel { text: 'Project brief' }
                        Text {
                            Layout.fillWidth: true; Layout.topMargin: Theme.s1
                            text: root.w && root.w.project ? root.w.project.title : ''
                            font.family: Theme.font; font.pixelSize: Theme.fsXl; font.weight: Font.DemiBold
                            color: Theme.textPrimary; elide: Text.ElideRight
                        }
                    }
                    Pill { Layout.alignment: Qt.AlignTop; outline: true; text: root.w && root.w.project ? Theme.domainLabel(root.w.project.current_domain || 'PRODUCT') : '' }
                }
                Text {
                    Layout.fillWidth: true; Layout.topMargin: Theme.s3
                    text: root.purpose()
                    font.family: Theme.font; font.pixelSize: Theme.fsSm
                    lineHeightMode: Text.FixedHeight; lineHeight: 24
                    color: Theme.textSecondary; wrapMode: Text.Wrap
                    maximumLineCount: 4; elide: Text.ElideRight
                }
                Text {
                    visible: !!root.w && root.w.linkedCreativeProjects.length > 0
                    Layout.fillWidth: true; Layout.topMargin: Theme.s2
                    text: root.w ? 'Followed by ' + root.w.linkedCreativeProjects.map(p => p.title).join(', ') : ''
                    font.family: Theme.font; font.pixelSize: Theme.fsSm; color: Theme.textSecondary; wrapMode: Text.Wrap
                }
                GridLayout {
                    Layout.fillWidth: true; Layout.topMargin: Theme.s5
                    columns: root.width >= Theme.breakpointSm ? 4 : 2
                    columnSpacing: Theme.s3; rowSpacing: Theme.s3
                    uniformCellWidths: true
                    StatTile { Layout.fillWidth: true; label: 'Branches'; icon: 'git-branch'; value: root.w ? String(root.w.branches.length) : '0' }
                    StatTile { Layout.fillWidth: true; label: 'Docs'; icon: 'file'; value: root.w ? root.w.projectDocCount() + '/' + root.w.totalDocumentTargets : '0/0' }
                    StatTile { Layout.fillWidth: true; label: 'Gates passed'; icon: 'shield-check'; value: String(root.gates.passed) }
                    StatTile { Layout.fillWidth: true; label: 'KB items'; icon: 'book'; value: root.w && root.w.project ? String((root.w.project.knowledge || []).length) : '0' }
                }
            }
            Card {
                Layout.fillWidth: !root.wide
                Layout.preferredWidth: root.wide ? 320 : -1
                Layout.fillHeight: true
                padding: Theme.s5
                spacing: 0
                EyebrowLabel { text: 'Next action' }
                Text {
                    Layout.fillWidth: true; Layout.topMargin: Theme.s2
                    text: root.next.title
                    font.family: Theme.font; font.pixelSize: Theme.fsLg; font.weight: Font.DemiBold
                    color: Theme.textPrimary; wrapMode: Text.Wrap
                }
                Text {
                    Layout.fillWidth: true; Layout.topMargin: Theme.s2
                    text: root.next.detail
                    font.family: Theme.font; font.pixelSize: Theme.fsSm
                    color: Theme.textSecondary; wrapMode: Text.Wrap
                }
                Flow {
                    Layout.fillWidth: true; Layout.topMargin: Theme.s5
                    spacing: Theme.s2
                    SButton {
                        primary: true
                        text: root.next.kind === 'questions' ? 'Answer questions' : root.next.kind === 'gate' ? 'See the questions' : root.next.kind === 'newround' ? 'Open the gate' : root.next.kind === 'create' ? 'Add branch' : root.next.kind === 'scope' ? 'Open Scope' : 'Open ' + Theme.domainLabel(root.next.domain)
                        iconName: root.next.kind === 'create' ? 'plus' : 'arrow-right'
                        onClicked: root.next.kind === 'questions' ? root.w.openHardening() : root.next.kind === 'gate' || root.next.kind === 'newround' ? root.w.openGate(root.next.gate) : root.next.kind === 'create' ? root.w.openNewBranch() : root.next.kind === 'scope' ? root.w.openScope() : root.w.showDomain(root.next.domain)
                    }
                    SButton { text: 'KB'; iconName: 'book'; onClicked: root.w.showKnowledge() }
                    SButton { text: ''; iconName: 'download'; accessibleLabel: 'Export'; enabled: !!root.w && !!root.w.projectId; onClicked: root.w.exportProject() }
                }
            }
        }

        GridLayout {
            Layout.fillWidth: true
            columns: root.domainColumns
            columnSpacing: Theme.s3; rowSpacing: Theme.s3
            uniformCellWidths: true
            Repeater {
                model: ['PRODUCT', 'BUSINESS', 'PRESENCE', 'LEGAL']
                delegate: Card {
                    id: domainCard
                    required property string modelData
                    required property int index
                    readonly property var summary: root.summaries[index] || ({branches: 0, bestCompleted: 0, total: 0, percent: 0, passed: 0, failed: 0})
                    readonly property string standing: root.w ? root.w.domainStanding(modelData) : ''
                    Layout.fillWidth: true
                    Layout.fillHeight: true
                    spacing: 0
                    RowLayout {
                        Layout.fillWidth: true
                        spacing: Theme.s3
                        ColumnLayout {
                            Layout.fillWidth: true
                            spacing: Theme.s1
                            RowLayout {
                                spacing: Theme.s2
                                DomainDot { domain: domainCard.modelData }
                                Text { text: Theme.domainLabel(domainCard.modelData); font.family: Theme.font; font.pixelSize: Theme.fsBase; font.weight: Font.DemiBold; color: Theme.textPrimary }
                                Pill { visible: domainCard.standing === 'predicted'; neutral: true; text: 'Predicted' }
                            }
                            Text { Layout.fillWidth: true; text: root.domainIntent[domainCard.modelData]; font.family: Theme.font; font.pixelSize: Theme.fsXs; color: Theme.textSecondary; elide: Text.ElideRight }
                        }
                        Text { Layout.alignment: Qt.AlignTop; text: domainCard.summary.branches + 'b'; font.family: Theme.mono; font.pixelSize: Theme.fsXs; color: Theme.textMuted }
                    }
                    RowLayout {
                        Layout.fillWidth: true; Layout.topMargin: Theme.s4
                        ColumnLayout {
                            spacing: Theme.s1
                            EyebrowLabel { text: 'Best branch' }
                            Text { text: domainCard.summary.bestCompleted + '/' + domainCard.summary.total; font.family: Theme.mono; font.pixelSize: Theme.fsLg; font.weight: Font.DemiBold; color: Theme.textPrimary }
                        }
                        Item { Layout.fillWidth: true }
                        ColumnLayout {
                            spacing: 0
                            Text { Layout.alignment: Qt.AlignRight; text: domainCard.summary.passed + ' passed'; font.family: Theme.font; font.pixelSize: Theme.fsXs; color: Theme.textSecondary }
                            Text { Layout.alignment: Qt.AlignRight; text: domainCard.summary.failed + ' failed'; font.family: Theme.font; font.pixelSize: Theme.fsXs; color: Theme.textSecondary }
                        }
                    }
                    ProgressBar { Layout.fillWidth: true; Layout.topMargin: Theme.s3; value: domainCard.summary.percent / 100; tone: Theme.domainColor(domainCard.modelData) }
                    RowLayout {
                        Layout.fillWidth: true; Layout.topMargin: Theme.s4
                        spacing: Theme.s2
                        SButton { Layout.fillWidth: true; implicitHeight: Theme.controlHeightSm; text: 'Open'; onClicked: root.w.showDomain(domainCard.modelData) }
                        SButton { Layout.preferredWidth: 40; implicitHeight: Theme.controlHeightSm; text: ''; iconName: 'plus'; accessibleLabel: 'Add ' + Theme.domainLabel(domainCard.modelData) + ' branch'; onClicked: root.w.openNewBranch() }
                    }
                }
            }
        }

        GridLayout {
            Layout.fillWidth: true
            columns: root.wide ? 2 : 1
            columnSpacing: Theme.s4; rowSpacing: Theme.s4
            Card {
                Layout.fillWidth: true
                Layout.fillHeight: true
                padding: 0
                spacing: 0
                Item {
                    Layout.fillWidth: true
                    implicitHeight: comparisonHead.implicitHeight + Theme.s3 * 2
                    RowLayout {
                        id: comparisonHead
                        anchors.fill: parent
                        anchors.leftMargin: Theme.s4; anchors.rightMargin: Theme.s4
                        ColumnLayout {
                            Layout.fillWidth: true
                            spacing: 0
                            Text { text: 'Branch comparison'; font.family: Theme.font; font.pixelSize: Theme.fsBase; font.weight: Font.DemiBold; color: Theme.textPrimary }
                            Text { Layout.fillWidth: true; text: 'Model branches by domain, status, and document coverage.'; font.family: Theme.font; font.pixelSize: Theme.fsXs; color: Theme.textSecondary; elide: Text.ElideRight }
                        }
                        SIcon { name: 'git-branch'; size: Theme.icon; color: Theme.textMuted }
                    }
                    Rectangle { anchors.bottom: parent.bottom; width: parent.width; height: Theme.borderWidth; color: Theme.border }
                }
                RowLayout {
                    visible: !!root.w && root.w.branches.length === 0
                    Layout.fillWidth: true; Layout.margins: Theme.s4; Layout.topMargin: Theme.s5; Layout.bottomMargin: Theme.s5
                    Text { Layout.fillWidth: true; text: 'No branches created.'; font.family: Theme.font; font.pixelSize: Theme.fsSm; color: Theme.textSecondary }
                    SButton { primary: true; text: 'Product branch'; iconName: 'plus'; onClicked: root.w.openNewBranch() }
                }
                Repeater {
                    model: root.w ? root.w.branches.slice(0, 8) : []
                    delegate: Rectangle {
                        id: branchRow
                        required property var modelData
                        required property int index
                        readonly property string rowDomain: root.w.branchTopDomain(modelData)
                        Layout.fillWidth: true
                        implicitHeight: rowLayout.implicitHeight + Theme.s3 * 2
                        color: rowHover.hovered ? Theme.bgTertiary : 'transparent'
                        Rectangle { visible: branchRow.index > 0; anchors.top: parent.top; width: parent.width; height: Theme.borderWidth; color: Theme.border }
                        HoverHandler { id: rowHover }
                        TapHandler { onTapped: { root.w.showDomain(branchRow.rowDomain); root.w.selectBranchTab(branchRow.modelData.id); } }
                        GridLayout {
                            id: rowLayout
                            anchors.fill: parent
                            anchors.leftMargin: Theme.s4; anchors.rightMargin: Theme.s4
                            anchors.topMargin: Theme.s3; anchors.bottomMargin: Theme.s3
                            columns: 4
                            columnSpacing: Theme.s3
                            RowLayout {
                                Layout.preferredWidth: 110
                                Layout.alignment: Qt.AlignVCenter
                                spacing: Theme.s2
                                DomainDot { domain: branchRow.rowDomain; size: Theme.dotSmall }
                                Text { text: Theme.domainShort(branchRow.rowDomain); font.family: Theme.font; font.pixelSize: Theme.fsXs; font.weight: Font.Medium; color: Theme.textSecondary }
                            }
                            ColumnLayout {
                                Layout.fillWidth: true
                                Layout.alignment: Qt.AlignVCenter
                                spacing: 0
                                Text { Layout.fillWidth: true; text: branchRow.modelData.name; font.family: Theme.font; font.pixelSize: Theme.fsSm; font.weight: Font.Medium; color: Theme.textPrimary; elide: Text.ElideRight }
                                Text { Layout.fillWidth: true; text: root.w.branchModelText(branchRow.modelData); font.family: Theme.mono; font.pixelSize: Theme.fsMono; color: Theme.textMuted; elide: Text.ElideRight }
                            }
                            Text { Layout.preferredWidth: 110; Layout.alignment: Qt.AlignVCenter; text: root.w.branchDocCount(branchRow.modelData.id) + '/' + root.w.totalDocumentTargets + ' docs'; font.family: Theme.mono; font.pixelSize: Theme.fsXs; color: Theme.textSecondary }
                            Pill {
                                Layout.preferredWidth: Math.max(implicitWidth, 90) - Theme.s4
                                Layout.alignment: Qt.AlignLeft | Qt.AlignVCenter
                                text: root.statusLabel(branchRow.modelData.status)
                                neutral: !root.statusTone[String(branchRow.modelData.status).toUpperCase()]
                                tone: root.statusTone[String(branchRow.modelData.status).toUpperCase()] || Theme.textMuted
                            }
                        }
                    }
                }
                Item { Layout.fillWidth: true; Layout.fillHeight: true }
            }
            Card {
                Layout.fillWidth: !root.wide
                Layout.preferredWidth: root.wide ? 320 : -1
                Layout.fillHeight: true
                spacing: 0
                RowLayout {
                    Layout.fillWidth: true
                    Text { Layout.fillWidth: true; text: 'Gate state'; font.family: Theme.font; font.pixelSize: Theme.fsBase; font.weight: Font.DemiBold; color: Theme.textPrimary }
                    SIcon { name: root.gates.failed > 0 ? 'alert' : 'shield-check'; size: Theme.icon; color: root.gates.failed > 0 ? Theme.danger : Theme.textMuted }
                }
                GridLayout {
                    Layout.fillWidth: true; Layout.topMargin: Theme.s4
                    columns: 3
                    columnSpacing: Theme.s2
                    uniformCellWidths: true
                    StatTile { Layout.fillWidth: true; centered: true; label: 'Passed'; value: String(root.gates.passed); valueColor: Theme.success }
                    StatTile { Layout.fillWidth: true; centered: true; label: 'Pending'; value: String(root.gates.pending); valueColor: Theme.warning }
                    StatTile { Layout.fillWidth: true; centered: true; label: 'Failed'; value: String(root.gates.failed); valueColor: Theme.danger }
                }
                Text {
                    visible: root.gates.recent.length === 0
                    Layout.fillWidth: true; Layout.topMargin: Theme.s5
                    text: 'No gate documents yet.'
                    font.family: Theme.font; font.pixelSize: Theme.fsSm; color: Theme.textSecondary
                }
                Repeater {
                    model: root.gates.recent
                    delegate: Rectangle {
                        required property var modelData
                        Layout.fillWidth: true
                        Layout.topMargin: Theme.s2
                        implicitHeight: 38
                        color: 'transparent'
                        radius: Theme.radiusSm
                        border.width: Theme.borderWidth
                        border.color: Theme.border
                        RowLayout {
                            anchors.fill: parent
                            anchors.leftMargin: Theme.s3; anchors.rightMargin: Theme.s3
                            Text { Layout.fillWidth: true; text: root.w.titleFor(modelData.document.document_type); font.family: Theme.font; font.pixelSize: Theme.fsSm; color: Theme.textSecondary; elide: Text.ElideRight }
                            Text { text: modelData.label || modelData.status; font.family: Theme.mono; font.pixelSize: Theme.fsSm; color: Theme.textPrimary }
                        }
                    }
                }
                Item { Layout.fillWidth: true; Layout.fillHeight: true }
            }
        }
    }
}

import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

// Document workbench: cascade list (.doc-row), editor header bar, revision row, DocumentReader, action row and the
// document actions popup. Talks to the workspace root API only; the gate review dialog stays in the workspace (the
// installed journey scans the workspace's direct children for it) and is opened via workspace.openGateReview().
ColumnLayout {
    id: view
    required property var workspace
    spacing: Theme.s3
    readonly property bool wide: workspace.width >= Theme.sidebarAutoCollapse
    readonly property var cascadeTypes: workspace.domainDocuments[workspace.domain] || []

    function isGate(type) { return String(type).indexOf('GATE_') === 0; }
    function stateOf(type) {
        var doc = workspace.latestFor(type);
        if (isGate(type)) {
            if (!doc) return 'QUEUED';
            return 'GATE ' + workspace.gateLabelOf(doc, workspace.gateStatusOf(doc));
        }
        return doc ? 'READY' : 'QUEUED';
    }
    function toneOf(type) {
        var s = stateOf(type);
        return s === 'GATE PASSED' ? Theme.success : s === 'GATE FAILED' ? Theme.danger : isGate(type) ? Theme.warning : Theme.success;
    }

    Choice {
        visible: !view.wide
        Layout.fillWidth: true
        model: view.cascadeTypes
        currentIndex: Math.max(0, model.indexOf(workspace.documentType))
        displayText: workspace.titleFor(workspace.documentType)
        onActivated: workspace.selectDocument(model[currentIndex])
        Accessible.name: 'Documents'
    }
    RowLayout {
        Layout.fillWidth: true; Layout.fillHeight: true; Layout.minimumHeight: 0
        spacing: Theme.s4
        Rectangle {
            visible: view.wide
            Layout.preferredWidth: 300; Layout.fillHeight: true; Layout.minimumHeight: 0
            color: Theme.bgSecondary; border.width: Theme.borderWidth; border.color: Theme.border; radius: Theme.radius
            clip: true
            ListView {
                id: cascade
                anchors.fill: parent; anchors.margins: Theme.borderWidth
                clip: true
                model: view.cascadeTypes
                ScrollBar.vertical: SScrollBar {}
                delegate: Rectangle {
                    id: row
                    required property string modelData
                    required property int index
                    readonly property bool gate: view.isGate(modelData)
                    readonly property bool selected: workspace.documentType === modelData
                    readonly property color tone: view.toneOf(modelData)
                    readonly property string stateText: view.stateOf(modelData)
                    width: cascade.width
                    height: Theme.docRowHeight
                    color: selected ? Qt.alpha(Theme.accentPrimary, 0.10)
                        : gate ? Qt.alpha(tone, 0.05)
                        : rowHover.hovered ? Theme.bgTertiary : 'transparent'
                    Rectangle { visible: row.gate; width: Theme.gateBar; height: parent.height; color: row.stateText === 'QUEUED' ? Qt.alpha(row.tone, 0.3) : row.tone }
                    Rectangle { anchors.bottom: parent.bottom; width: parent.width; height: Theme.borderWidth; color: Theme.border; visible: row.index < cascade.count - 1 }
                    HoverHandler { id: rowHover }
                    TapHandler { onTapped: workspace.selectDocument(row.modelData) }
                    RowLayout {
                        anchors.fill: parent; anchors.leftMargin: Theme.s4; anchors.rightMargin: Theme.s4
                        spacing: Theme.s3
                        Text { text: (row.index + 1 < 10 ? '0' : '') + (row.index + 1); font.family: Theme.mono; font.pixelSize: Theme.fsXs; color: Theme.textMuted; Layout.preferredWidth: 18 }
                        SIcon {
                            size: Theme.icon
                            name: row.gate ? (row.stateText === 'GATE PASSED' ? 'shield-check' : 'shield') : (workspace.latestFor(row.modelData) ? 'check-circle' : 'circle')
                            color: row.gate ? row.tone : (workspace.latestFor(row.modelData) ? Theme.success : Theme.textMuted)
                        }
                        ColumnLayout {
                            Layout.fillWidth: true; spacing: 2
                            Text { Layout.fillWidth: true; text: workspace.titleFor(row.modelData); elide: Text.ElideRight; font.family: Theme.font; font.pixelSize: Theme.fsSm; font.weight: row.gate ? Font.DemiBold : Font.Medium; color: row.selected ? Theme.accentPrimary : Theme.textPrimary }
                            Text { Layout.fillWidth: true; text: (row.index + 1) + '/' + cascade.count + ' · ' + row.stateText; elide: Text.ElideRight; font.family: Theme.font; font.pixelSize: Theme.fsXs; color: row.gate && row.stateText !== 'QUEUED' ? row.tone : Theme.textMuted }
                        }
                    }
                }
            }
        }
        Rectangle {
            Layout.fillWidth: true; Layout.fillHeight: true; Layout.minimumHeight: 0
            color: Theme.bgSecondary; border.width: Theme.borderWidth; border.color: Theme.border; radius: Theme.radius
            ColumnLayout {
                anchors.fill: parent; anchors.margins: workspace.compactWorkspace ? Theme.s3 : Theme.s4
                spacing: Theme.s3
                RowLayout {
                    Layout.fillWidth: true; spacing: Theme.s2
                    Text {
                        Layout.fillWidth: true
                        text: workspace.titleFor(workspace.documentType) || 'Choose a document'
                        font.family: Theme.font; font.pixelSize: Theme.fsLg; font.weight: Font.Bold; color: Theme.textPrimary
                        elide: Text.ElideRight
                    }
                    SButton { objectName: 'qroc-document-metadata-open'; text: 'Details'; quiet: true; onClicked: workspace.openDocumentMetadata() }
                    STabButton { text: 'Edit'; active: workspace.viewMode === 'raw'; onClicked: workspace.viewMode = 'raw' }
                    STabButton { text: 'Read'; active: workspace.viewMode === 'rendered'; onClicked: workspace.viewMode = 'rendered' }
                    SButton { text: ''; iconName: 'more-v'; quiet: true; accessibleLabel: 'More actions'; onClicked: documentActions.open() }
                }
                RowLayout {
                    visible: !workspace.compactWorkspace
                    Layout.fillWidth: true; spacing: Theme.s2
                    Pill { mono: true; neutral: true; text: (workspace.currentDocument ? 'Revision ' + workspace.currentDocument.revision : 'First draft') + ' · ' + Theme.domainLabel(workspace.domainOfType(workspace.documentType) || workspace.domain) }
                    Item { Layout.fillWidth: true }
                    SButton { text: 'Latest saved'; quiet: true; enabled: !!workspace.currentDocument && !workspace.exactSavedRevision; onClicked: workspace.selectDocument(workspace.documentType) }
                    Choice {
                        objectName: 'qroc-history-wide'
                        enabled: workspace.history.length > 0
                        Layout.preferredWidth: 150
                        model: workspace.history.map(d => 'Revision ' + d.revision)
                        currentIndex: workspace.selectedHistoryIndex
                        displayText: workspace.historySelectionText
                        onActivated: { workspace.persistDraft(); workspace.selectHistory(currentIndex); }
                        Accessible.name: 'Saved revision'
                    }
                }
                Flow {
                    Layout.fillWidth: true; spacing: Theme.s2
                    Rectangle {
                        implicitWidth: standing.implicitWidth + Theme.s4; implicitHeight: standing.implicitHeight + Theme.s1
                        radius: Theme.radiusSm
                        color: Qt.alpha(workspace.exactSavedRevision ? Theme.success : Theme.warning, 0.12)
                        Text {
                            id: standing
                            objectName: 'qroc-review-standing'
                            anchors.centerIn: parent
                            text: workspace.reviewStanding
                            font.family: Theme.font; font.pixelSize: Theme.fsXs; font.weight: Font.Medium
                            color: workspace.exactSavedRevision ? Theme.success : Theme.warning
                        }
                    }
                    Pill { visible: !workspace.authoringState.documentIncluded(workspace.documentType); text: 'Excluded by scope'; tone: Theme.danger }
                }
                DocumentReader {
                    id: editor
                    objectName: 'qroc-document-reader'
                    Layout.fillWidth: true; Layout.fillHeight: true; Layout.minimumHeight: workspace.compactWorkspace ? 140 : 180
                    text: workspace.draftText
                    title: workspace.titleFor(workspace.documentType) || 'Project document'
                    positionKey: 'qroc/document/' + workspace.projectId + '/' + workspace.branchId + '/' + workspace.documentType
                    editable: workspace.viewMode === 'raw'
                    textFormat: workspace.viewMode === 'rendered' ? TextEdit.MarkdownText : TextEdit.PlainText
                    onEdited: value => workspace.draftText = value
                }
                RowLayout {
                    Layout.fillWidth: true; spacing: Theme.s2
                    SButton { objectName: 'qroc-save-revision'; text: 'Save revision'; primary: true; enabled: !!workspace.branchId && !!workspace.documentType && !!workspace.draftText.trim() && !workspace.loading; onClicked: workspace.saveDocument() }
                    SButton {
                        busy: !!workspace.jobId
                        text: workspace.jobId ? 'Generating…' : 'Generate locally'
                        Layout.preferredWidth: 150
                        enabled: workspace.explicitModel && workspace.authoringState.generationAllowed(workspace.documentType) && !!workspace.documentType && !workspace.jobId
                        onClicked: workspace.generate()
                    }
                    Item { Layout.fillWidth: true }
                    SButton { text: 'More'; onClicked: documentActions.open() }
                }
            }
        }
    }
    SPopup {
        id: documentActions
        objectName: 'qroc-document-actions'
        parent: Overlay.overlay; anchors.centerIn: parent; modal: true; padding: Theme.s4
        width: Math.min(Theme.modalXl, parent.width - 32)
        background: Rectangle { color: Theme.bgSecondary; border.width: Theme.borderWidth; border.color: Theme.border; radius: Theme.radiusLg }
        contentItem: ColumnLayout {
            spacing: Theme.s3
            Text { text: 'Document actions'; font.family: Theme.font; font.pixelSize: Theme.fsLg; font.weight: Font.DemiBold; color: Theme.textPrimary }
            Flow {
                Layout.fillWidth: true; Layout.preferredHeight: implicitHeight; spacing: Theme.s2
                SButton { text: 'Review gate'; primary: true; enabled: !!workspace.gate; onClicked: { documentActions.close(); workspace.openGateReview(); } }
                SButton { text: 'Cancel'; enabled: !!workspace.jobId; onClicked: workspace.request('cancel', 'qroc-job-cancel', {id: workspace.jobId}, (r, ok) => { if (ok) workspace.noticeText = 'Cancellation requested'; }) }
                SButton {
                    text: 'Stop generating'; enabled: workspace.batchActive
                    onClicked: workspace.batchJobIds.forEach(id => workspace.request('cancel-' + id, 'qroc-job-cancel', {id: id}, (r, ok) => { if (ok) workspace.noticeText = 'Generation stopped · finished documents kept'; }))
                }
                SButton {
                    text: 'Generate all documents'
                    enabled: workspace.explicitModel && workspace.authoringState.generationAllowed(workspace.documentType) && view.cascadeTypes.length > 0 && !workspace.batchActive
                    onClicked: workspace.request('batch', 'qroc-batch-start', {project_id: workspace.projectId, branch_id: workspace.branchId, document_types: workspace.domainDocuments[workspace.domain]}, (r, ok) => {
                        if (ok) { workspace.batchJobIds = (r.jobs || []).map(j => j.id); workspace.batchActive = true; workspace.jobTimer.start(); }
                    })
                }
                SButton { text: 'Compare'; onClicked: { documentActions.close(); workspace.openCompare(); } }
                SButton { text: 'Synthesize…'; onClicked: { documentActions.close(); workspace.openSynthesis(); } }
            }
            Pill {
                visible: workspace.batchActive
                text: 'Generating · ' + workspace.jobs.filter(j => workspace.batchJobIds.indexOf(j.id) >= 0 && j.state === 'succeeded').length + ' done'
                tone: Theme.executing
            }
            SButton { text: 'Done'; quiet: true; Layout.alignment: Qt.AlignRight; onClicked: documentActions.close() }
        }
    }
}

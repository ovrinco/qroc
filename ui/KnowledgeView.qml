import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

// Knowledge base view (KB tab): header actions, 300px list of notes with context checkboxes, reader card with the
// selected context and source rows. Reads and calls the workspace root API only (never sibling ids).
ColumnLayout {
    id: view
    required property var workspace
    spacing: Theme.s4
    readonly property var items: workspace.project ? workspace.project.knowledge || [] : []
    readonly property var sources: workspace.project ? workspace.project.sources || [] : []
    readonly property int selectedCount: items.filter(k => k.selected).length
    // An item's name as people read it: without the engine's content hash, numbered when names then repeat.
    function titleOf(item) {
        if (!item) return '';
        var name = Theme.readableStored(item.title).trim() || 'Note';
        var same = items.filter(k => (Theme.readableStored(k.title).trim() || 'Note') === name);
        return same.length > 1 ? name + ' ' + (same.findIndex(k => k.id === item.id) + 1) : name;
    }

    Flow {
        Layout.fillWidth: true; Layout.preferredHeight: implicitHeight; spacing: Theme.s2
        Text { width: Math.max(160, parent.width - actions.implicitWidth - Theme.s2); height: Theme.controlHeight; verticalAlignment: Text.AlignVCenter; text: 'Knowledge base'; font.family: Theme.font; font.pixelSize: Theme.fsLg; font.weight: Font.DemiBold; color: Theme.textPrimary; visible: view.workspace.width >= Theme.breakpointMd }
        Row {
            id: actions
            spacing: Theme.s2
            SButton { text: 'Project notes'; iconName: 'notes'; onClicked: workspace.openProjectNotes() }
            SButton { text: 'Attach file'; iconName: 'upload'; onClicked: workspace.chooseSource() }
            SButton { text: 'Import skills'; iconName: 'download'; onClicked: workspace.openSkillsImport() }
            SButton { text: 'Add note'; iconName: 'plus'; primary: true; onClicked: workspace.openKnowledgeDialog() }
        }
    }
    SplitView {
        orientation: workspace.width < 760 ? Qt.Vertical : Qt.Horizontal
        Layout.fillWidth: true; Layout.fillHeight: true; Layout.minimumHeight: 0
        handle: SSplitHandle { horizontal: true }
        Item {
            SplitView.preferredWidth: 300; SplitView.preferredHeight: 180; SplitView.minimumWidth: 200
            ListView {
                id: knowledgeList
                anchors.fill: parent; anchors.rightMargin: Theme.s2
                clip: true; spacing: Theme.s2
                model: view.items
                ScrollBar.vertical: SScrollBar {}
                delegate: Rectangle {
                    id: item
                    required property var modelData
                    readonly property bool current: !!workspace.selectedKnowledge && workspace.selectedKnowledge.id === modelData.id
                    width: knowledgeList.width
                    implicitHeight: knowledgeRow.implicitHeight + Theme.s3 * 2
                    radius: Theme.radius
                    color: current ? Qt.alpha(Theme.accentPrimary, 0.10) : itemHover.hovered ? Theme.bgTertiary : Theme.bgSecondary
                    border.width: Theme.borderWidth
                    border.color: current || itemHover.hovered ? Theme.accentPrimary : Theme.border
                    HoverHandler { id: itemHover }
                    TapHandler { onTapped: workspace.selectedKnowledge = item.modelData }
                    RowLayout {
                        id: knowledgeRow
                        anchors.fill: parent; anchors.margins: Theme.s3
                        spacing: Theme.s2
                        SCheckBox { checked: item.modelData.selected; onClicked: workspace.toggleKnowledge(item.modelData); Accessible.name: 'Include ' + view.titleOf(item.modelData) + ' in project context' }
                        Text { Layout.fillWidth: true; text: view.titleOf(item.modelData); wrapMode: Text.Wrap; font.family: Theme.font; font.pixelSize: Theme.fsSm; font.weight: Font.Medium; color: Theme.textPrimary }
                        Pill { text: item.modelData.kind; neutral: true }
                    }
                }
                EmptyState {
                    anchors.fill: parent
                    visible: !knowledgeList.count
                    icon: 'book'; card: false
                    title: 'No notes yet'
                    actionText: 'Add note'
                    onActionRequested: workspace.openKnowledgeDialog()
                }
            }
        }
        Card {
            SplitView.fillWidth: true; SplitView.minimumWidth: 240
            padding: 0
            ScrollView {
                id: knowledgeReader
                Layout.fillWidth: true; Layout.fillHeight: true
                contentWidth: availableWidth; clip: true
                ColumnLayout {
                    x: Theme.s5
                    width: knowledgeReader.availableWidth - Theme.s5 * 2
                    spacing: Theme.s4
                    Item { Layout.preferredHeight: Theme.s1 }
                    RowLayout {
                        Layout.fillWidth: true; spacing: Theme.s2
                        EyebrowLabel { text: 'Selected context' }
                        Pill { text: view.selectedCount + (view.selectedCount === 1 ? ' item selected' : ' items selected'); tone: Theme.accentPrimary }
                        Item { Layout.fillWidth: true }
                    }
                    Text { Layout.fillWidth: true; text: workspace.selectedKnowledge ? view.titleOf(workspace.selectedKnowledge) : 'Source material'; wrapMode: Text.Wrap; font.family: Theme.font; font.pixelSize: Theme.fsXl; font.weight: Font.DemiBold; color: Theme.textPrimary }
                    STextArea {
                        text: Theme.readableStored(workspace.selectedKnowledge ? workspace.selectedKnowledge.content : workspace.project ? workspace.project.brain_dump : '')
                        readOnly: true; selectByMouse: true; wrapMode: TextEdit.Wrap
                        color: Theme.textPrimary; font.family: Theme.font; font.pixelSize: Theme.fsSm
                        Layout.fillWidth: true; background: null; padding: 0
                    }
                    EyebrowLabel { visible: view.sources.length > 0; text: 'Attached files' }
                    Repeater {
                        model: view.sources
                        delegate: Rectangle {
                            required property var modelData
                            Layout.fillWidth: true
                            implicitHeight: sourceRow.implicitHeight + Theme.s3 * 2
                            color: Theme.bgPrimary; radius: Theme.radiusSm; border.width: Theme.borderWidth; border.color: Theme.border
                            RowLayout {
                                id: sourceRow
                                anchors.fill: parent; anchors.margins: Theme.s3; spacing: Theme.s3
                                SIcon { name: 'file'; size: Theme.icon; color: Theme.textMuted }
                                Text { Layout.fillWidth: true; text: modelData.name; elide: Text.ElideMiddle; font.family: Theme.font; font.pixelSize: Theme.fsSm; color: Theme.textPrimary }
                                Pill { text: Math.ceil(modelData.size / 1024) + ' KiB'; neutral: true; mono: true }
                            }
                        }
                    }
                    Item { Layout.preferredHeight: Theme.s4 }
                }
            }
        }
    }
}

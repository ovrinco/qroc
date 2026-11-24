import QtQuick
import QtQuick.Controls as QC
import QtQuick.Layouts

// Recent Projects landing (GlobalDashboard.tsx): empty project queue card with the four domain tiles, or the five most
// recent project rows. "Import" is the QML-only second action next to New Project.
Flickable {
    id: root
    property var workspace: null
    readonly property var w: workspace
    readonly property var recent: w ? w.projects.slice().sort((a, b) => (b.updated_at || 0) - (a.updated_at || 0)).slice(0, 5) : []
    readonly property real colWidth: Math.min(Theme.landingMaxWidth, width)
    readonly property bool stacked: width < Theme.breakpointXl
    readonly property var order: ['PRODUCT', 'BUSINESS', 'PRESENCE', 'LEGAL']
    // A media row reads the first lines of its latest brief: one fetch per media row, five rows at most.
    property var briefs: ({})
    onRecentChanged: recent.forEach(p => {
        if (p.kind !== 'media' || !w) return;
        w.transport.send('qroc-media-document-get', {project_id: p.id, document: 'brief'}, (r, ok) => {
            if (!ok || !r || !r.document) return;
            var next = Object.assign({}, root.briefs); next[p.id] = r.document.content; root.briefs = next;
        });
    })
    function purpose(project) {
        if (project.kind === 'media') project = {brain_dump: briefs[project.id] !== undefined ? briefs[project.id] : project.brain_dump};
        var text = Theme.readableStored(project.brain_dump || '').replace(/\s+/g, ' ').trim();
        if (!text) return 'No project brief captured yet.';
        return text.length > 220 ? text.slice(0, 217) + '...' : text;
    }
    function age(project) {
        var stamp = (project.updated_at || 0) * 1000;
        if (!stamp) return 'Never';
        var days = Math.floor((Date.now() - stamp) / 86400000);
        if (days < 1) return 'Today';
        return days < 30 ? days + 'd ago' : Math.floor(days / 30) + 'mo ago';
    }
    clip: true
    contentWidth: width
    contentHeight: column.implicitHeight + Theme.s8 * 2
    boundsBehavior: Flickable.StopAtBounds
    QC.ScrollBar.vertical: SScrollBar {}

    ColumnLayout {
        id: column
        x: Math.max(0, (root.width - root.colWidth) / 2) + Theme.s6
        y: Theme.s8
        width: root.colWidth - Theme.s6 * 2
        spacing: 0

        Card {
            visible: root.recent.length === 0
            Layout.fillWidth: true
            padding: Theme.s8
            spacing: 0
            GridLayout {
                Layout.fillWidth: true
                columns: root.stacked ? 1 : 2
                columnSpacing: Theme.s6; rowSpacing: Theme.s6
                ColumnLayout {
                    Layout.fillWidth: true
                    spacing: 0
                    EyebrowLabel { text: 'Project queue' }
                    Text { Layout.topMargin: Theme.s2; text: 'No projects yet'; font.family: Theme.font; font.pixelSize: Theme.fsXl; font.weight: Font.DemiBold; color: Theme.textPrimary }
                    Text { Layout.topMargin: Theme.s2; Layout.fillWidth: true; text: 'Create a project to start its Product, Business, Presence and Legal documents.'; font.family: Theme.font; font.pixelSize: Theme.fsSm; color: Theme.textSecondary; wrapMode: Text.Wrap }
                }
                RowLayout {
                    spacing: Theme.s2
                    SButton { text: 'Import'; implicitHeight: Theme.controlHeightLg; iconName: 'upload'; onClicked: root.w.openImportProject() }
                    SButton { objectName: 'qroc-new-project'; primary: true; text: 'Start New Project'; implicitHeight: Theme.controlHeightLg; iconName: 'plus'; onClicked: root.w.openNewProject() }
                }
            }
            GridLayout {
                Layout.fillWidth: true; Layout.topMargin: Theme.s8
                columns: root.width >= Theme.breakpointSm ? 4 : 2
                columnSpacing: Theme.s3; rowSpacing: Theme.s3
                uniformCellWidths: true
                Repeater {
                    model: root.order
                    delegate: Card {
                        required property string modelData
                        required property int index
                        Layout.fillWidth: true
                        nested: true
                        padding: Theme.s3
                        spacing: 0
                        RowLayout {
                            Layout.fillWidth: true
                            DomainDot { domain: modelData }
                            Item { Layout.fillWidth: true }
                            Text { text: (index + 1) + '/4'; font.family: Theme.mono; font.pixelSize: Theme.fsMicro; color: Theme.textMuted }
                        }
                        Text { Layout.topMargin: Theme.s3; text: Theme.domainLabel(modelData); font.family: Theme.font; font.pixelSize: Theme.fsSm; font.weight: Font.Medium; color: Theme.textPrimary }
                    }
                }
            }
        }

        RowLayout {
            visible: root.recent.length > 0
            Layout.fillWidth: true
            Layout.bottomMargin: Theme.s4
            spacing: Theme.s2
            ColumnLayout {
                Layout.fillWidth: true
                spacing: Theme.s1
                EyebrowLabel { text: 'Local queue' }
                Text { text: root.recent.length + (root.recent.length === 1 ? ' recent project' : ' recent projects'); font.family: Theme.font; font.pixelSize: Theme.fsLg; font.weight: Font.DemiBold; color: Theme.textPrimary }
            }
            SButton { text: 'Import'; implicitHeight: Theme.controlHeightSm + Theme.s2; iconName: 'upload'; onClicked: root.w.openImportProject() }
            SButton { objectName: 'qroc-new-project'; text: 'New Project'; implicitHeight: Theme.controlHeightSm + Theme.s2; iconName: 'plus'; onClicked: root.w.openNewProject() }
        }

        Repeater {
            model: root.recent
            delegate: Rectangle {
                id: row
                required property var modelData
                Layout.fillWidth: true
                Layout.bottomMargin: Theme.s3
                implicitHeight: rowGrid.implicitHeight + Theme.s4 * 2
                radius: Theme.radius
                color: hover.hovered ? Theme.bgTertiary : Theme.bgSecondary
                border.width: Theme.borderWidth
                border.color: hover.hovered ? Theme.accentPrimary : Theme.border
                HoverHandler { id: hover }
                TapHandler { onTapped: root.w.showProject(row.modelData.id) }
                GridLayout {
                    id: rowGrid
                    anchors.fill: parent
                    anchors.margins: Theme.s4
                    columns: root.stacked ? 1 : 2
                    columnSpacing: Theme.s4; rowSpacing: Theme.s4
                    ColumnLayout {
                        Layout.fillWidth: true
                        Layout.alignment: Qt.AlignTop
                        spacing: 0
                        RowLayout {
                            spacing: Theme.s3
                            SIcon { name: row.modelData.kind === 'media' ? 'mic' : 'folder'; size: Theme.iconMd; color: Theme.accentPrimary }
                            ColumnLayout {
                                Layout.fillWidth: true
                                spacing: Theme.s1
                                Text { Layout.fillWidth: true; text: row.modelData.title; font.family: Theme.font; font.pixelSize: Theme.fsSm; font.weight: Font.DemiBold; color: Theme.textPrimary; elide: Text.ElideRight }
                                RowLayout {
                                    spacing: Theme.s2
                                    Text { text: row.modelData.kind === 'media' ? 'Media' : Theme.domainLabel(row.modelData.current_domain || 'PRODUCT'); font.family: Theme.font; font.pixelSize: Theme.fsXs; color: Theme.textSecondary }
                                    Rectangle { width: 4; height: 4; radius: 2; color: Theme.textMuted }
                                    SIcon { name: 'clock'; size: 12; color: Theme.textSecondary }
                                    Text { text: root.age(row.modelData); font.family: Theme.font; font.pixelSize: Theme.fsXs; color: Theme.textSecondary }
                                }
                            }
                        }
                        Text {
                            Layout.fillWidth: true; Layout.topMargin: Theme.s3
                            text: root.purpose(row.modelData)
                            font.family: Theme.font; font.pixelSize: Theme.fsSm
                            lineHeightMode: Text.FixedHeight; lineHeight: 24
                            color: Theme.textSecondary; wrapMode: Text.Wrap
                            maximumLineCount: 2; elide: Text.ElideRight
                        }
                    }
                    GridLayout {
                        visible: row.modelData.kind !== 'media'
                        Layout.preferredWidth: root.stacked ? -1 : 320
                        Layout.fillWidth: root.stacked
                        Layout.alignment: Qt.AlignTop
                        columns: 4
                        columnSpacing: Theme.s2
                        uniformCellWidths: true
                        Repeater {
                            model: root.order
                            delegate: Card {
                                required property string modelData
                                Layout.fillWidth: true
                                nested: true
                                padding: Theme.s2
                                spacing: Theme.s2
                                RowLayout {
                                    Layout.fillWidth: true
                                    Text { text: Theme.domainShort(modelData); font.family: Theme.font; font.pixelSize: Theme.fsMicro; font.weight: Font.Medium; color: Theme.textSecondary }
                                    Item { Layout.fillWidth: true }
                                }
                                ProgressBar { Layout.fillWidth: true; value: 0; thickness: Theme.progressThin; tone: Theme.domainColor(modelData) }
                                Text { text: '—/' + (root.w ? (root.w.referenceDocuments[modelData] || []).length : 0); font.family: Theme.mono; font.pixelSize: Theme.fsMicro; color: Theme.textMuted }
                            }
                        }
                    }
                }
            }
        }
    }
}

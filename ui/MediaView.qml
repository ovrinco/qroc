import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import QtQuick.Dialogs

// The media project's own view (design sheet section 9): Brief, Lyrics and Concepts as revisions, and Versions,
// each a file in the media store read as Unchanged, Changed or Missing. Every action is one engine operation
// (qroc-media-*, qroc-creative-status/cancel/list); the list is DocumentWorkbench's 300 px list, the reader
// DocumentReader. Unsaved text lives here while the project stays open and is dropped when another opens.
Item {
    id: view
    required property var workspace
    readonly property string projectId: workspace.isMedia ? workspace.projectId : ''
    readonly property string tab: workspace.projectView
    readonly property bool textTab: tab === 'BRIEF' || tab === 'LYRICS' || tab === 'CONCEPTS'
    readonly property string documentKey: textTab ? tab.toLowerCase() : ''
    readonly property bool wide: workspace.width >= Theme.sidebarAutoCollapse

    // ---- state ----
    property var revisions: ({brief: [], lyrics: [], concepts: []})     // newest first: {revision, created_at, content}
    property var loaded: ({})
    property var selectedRevision: ({brief: 0, lyrics: 0, concepts: 0})
    property var drafts: ({})                                             // document → unsaved text
    property var writing: ({})                                            // document → empty editor opened
    property bool saving: false
    property var versions: []                                             // newest first
    property bool versionsLoaded: false
    property bool checking: false
    property int selectedVersion: 0
    property var outputs: []
    property bool clipToolAvailable: false
    property bool clipStarting: false
    property var activeClip: null
    property string clipStart: '0'
    property string clipLength: '10'
    property string clipVolume: '1'
    property int serial: 0

    readonly property var currentRevisions: textTab ? (revisions[documentKey] || []) : []
    readonly property var latest: currentRevisions.length ? currentRevisions[0] : null
    readonly property int latestNumber: currentRevisions.length ? currentRevisions[0].revision : 0
    readonly property int shownNumber: {
        var n = selectedRevision[documentKey] || 0;
        return currentRevisions.some(r => r.revision === n) ? n : latestNumber;
    }
    readonly property var shownRevision: currentRevisions.find(r => r.revision === shownNumber) || null
    readonly property bool atLatest: shownNumber === latestNumber
    readonly property bool editorOpen: textTab && (!!latest || !!writing[documentKey])
    readonly property string editorText: !textTab ? '' : !atLatest && shownRevision ? shownRevision.content
        : drafts[documentKey] !== undefined ? drafts[documentKey] : latest ? latest.content : ''
    readonly property bool unsaved: textTab && atLatest && editorOpen && editorText !== (latest ? latest.content : '')
    readonly property bool canSave: unsaved && !!editorText.trim() && !saving
    readonly property var version: versions.find(v => v.number === selectedVersion) || null
    readonly property int missingCount: versions.filter(v => v.state === 'missing').length
    readonly property bool clipRunning: outputs.some(o => o.project_id === projectId && o.branch_id === 'media' && o.status === 'running')
    readonly property var versionClips: {
        if (!version) return [];
        var mine = outputs.filter(o => o.project_id === projectId && o.branch_id === 'media' && o.source && o.source.asset_id === 'media-version-' + version.number);
        return mine.map((o, i) => Object.assign({number: i + 1}, o)).reverse();
    }

    function docWord(key) { return key.charAt(0).toUpperCase() + key.slice(1); }
    function with_(map, key, value) { var next = Object.assign({}, map); next[key] = value; return next; }

    // ---- words (section 9.5) ----
    function pad(n) { return (n < 10 ? '0' : '') + n; }
    function timeWords(seconds) {
        if (!seconds) return 'Unknown';
        var d = new Date(seconds * 1000), now = new Date();
        var day = new Date(d.getFullYear(), d.getMonth(), d.getDate()).getTime();
        var today = new Date(now.getFullYear(), now.getMonth(), now.getDate()).getTime();
        var hour = pad(d.getHours()) + ':' + pad(d.getMinutes());
        if (day === today) return 'Today ' + hour;
        if (day === today - 86400000) return 'Yesterday ' + hour;
        var months = ['Jan', 'Feb', 'Mar', 'Apr', 'May', 'Jun', 'Jul', 'Aug', 'Sep', 'Oct', 'Nov', 'Dec'];
        return d.getDate() + ' ' + months[d.getMonth()] + (d.getFullYear() !== now.getFullYear() ? ' ' + d.getFullYear() : '') + ' ' + hour;
    }
    function durationWords(seconds) {
        if (seconds === null || seconds === undefined || isNaN(seconds)) return 'Unknown';
        var t = Math.round(seconds), h = Math.floor(t / 3600), m = Math.floor((t % 3600) / 60), s = t % 60;
        return h > 0 ? h + ':' + pad(m) + ':' + pad(s) : m + ':' + pad(s);
    }
    function sizeWords(bytes) { return (bytes / 1000000).toFixed(1) + ' MB'; }
    function fileName(path) { var parts = String(path || '').split('/'); return parts[parts.length - 1]; }
    function stateWord(state) { return state === 'unchanged' ? 'Unchanged' : state === 'changed' ? 'Changed' : 'Missing'; }
    function stateTone(state) { return state === 'unchanged' ? Theme.success : state === 'changed' ? Theme.warning : Theme.danger; }
    function stateTextTone(state) { return state === 'unchanged' ? Theme.successText : state === 'changed' ? Theme.warning : Theme.dangerText; }
    function stateSentence(state) {
        return state === 'unchanged' ? 'The file is as it was when it was referenced.'
            : state === 'changed' ? 'The file has changed since it was referenced.' : 'The file is not where it was referenced.';
    }
    function clipWord(status) { return status === 'running' ? 'Making clip' : status === 'succeeded' ? 'Clip ready' : status === 'failed' ? 'Failed' : 'Stopped'; }
    function clipTone(status) { return status === 'running' ? Theme.executing : status === 'succeeded' ? Theme.success : status === 'failed' ? Theme.danger : Theme.pending; }
    // An engine sentence never carries a path on screen: a path becomes its last part.
    function plain(sentence) { return String(sentence || '').replace(/\S*\/(\S+)/g, '$1').replace(/^Invalid( input)?:\s*/i, ''); }
    function refusal(error) {
        var e = String(error || '');
        var words = [
            ['starts from its brief', 'Write the brief first.'],
            ['needs a readable name', 'Give the version a name.'],
            ['name already exists', 'A version with this name already exists. Choose another name.'],
            ['must lie in the media store', 'That file is outside the media store. Choose a file inside it.'],
            ['may not contain symlinks', 'That file is reached through a link. Choose the file itself.'],
            ['must reference a regular file', 'That is not a file QROC can reference.'],
            ['must be an absolute path', 'That is not a file QROC can reference.'],
            ['Version limit reached', 'This project has reached its limit of 512 versions.'],
            ['exceeds 1 MiB', 'This text is too long to save (over 1 MB).'],
            ['changed or missing since it was referenced', 'The file has changed or is missing since it was referenced. Check files first.'],
            ['up to 64 MiB', 'Clips are made from files up to 64 MB.'],
            ['owner or digest mismatch', 'This revision could not be read.'],
            ['not a media project', 'This is not a media project.']
        ];
        for (var i = 0; i < words.length; i++) if (e.indexOf(words[i][0]) >= 0) return words[i][1];
        return '';
    }
    function fail(error) {
        var known = refusal(error);
        workspace.noticeText = '';
        workspace.errorText = known || ('QROC could not do that.\n' + plain(error));
    }

    // ---- engine calls; a reply for a project no longer open is dropped ----
    function send(command, args, done) {
        var key = projectId;
        workspace.transport.send(command, args, function(result, ok, error) {
            if (!view || view.projectId !== key) return;
            done(result, ok, error);
        });
    }
    function open() {
        revisions = ({brief: [], lyrics: [], concepts: []}); loaded = ({}); selectedRevision = ({brief: 0, lyrics: 0, concepts: 0});
        drafts = ({}); writing = ({}); versions = []; versionsLoaded = false; selectedVersion = 0; outputs = []; activeClip = null;
        if (!projectId) return;
        workspace.transport.send('bridge-capabilities', {}, (r, ok) => { if (ok && r) view.clipToolAvailable = r.creative === true; });
        loadRevisions('brief');
        loadVersions();
        loadClips();
    }
    function showTab(name) {
        workspace.projectView = name;
        if (name === 'VERSIONS') { if (!versionsLoaded) loadVersions(); }
        else if (!loaded[name.toLowerCase()]) loadRevisions(name.toLowerCase());
        listFocus.forceActiveFocus();
    }
    // The engine lists revision numbers; one read per revision gives each its time and text (section 9.9).
    function loadRevisions(doc, select) {
        var token = ++serial;
        send('qroc-media-document-get', {project_id: projectId, document: doc}, (r, ok, error) => {
            if (!ok) { fail(error); return; }
            var numbers = (r.revisions || []).slice(), rows = [], left = numbers.length;
            if (!left) { view.revisions = with_(view.revisions, doc, []); view.loaded = with_(view.loaded, doc, true); return; }
            numbers.forEach(n => send('qroc-media-document-get', {project_id: projectId, document: doc, revision: n}, (d, got, err) => {
                if (got && d.document) rows.push({revision: d.document.revision, created_at: d.document.created_at, content: d.document.content});
                else if (!got) fail(err);
                if (--left > 0) return;
                rows.sort((a, b) => b.revision - a.revision);
                view.revisions = with_(view.revisions, doc, rows);
                view.loaded = with_(view.loaded, doc, true);
                view.selectedRevision = with_(view.selectedRevision, doc, select || (rows.length ? rows[0].revision : 0));
            }));
        });
    }
    function selectRevision(n) { selectedRevision = with_(selectedRevision, documentKey, n); }
    function edit(text) { if (atLatest) drafts = with_(drafts, documentKey, text); }
    function startWriting() { writing = with_(writing, documentKey, true); }
    function save() {
        if (!canSave) return;
        var doc = documentKey, text = editorText;
        saving = true;
        send('qroc-media-document-save', {project_id: projectId, document: doc, content: text}, (r, ok, error) => {
            view.saving = false;
            if (!ok) { fail(error); return; }
            var next = Object.assign({}, view.drafts); delete next[doc]; view.drafts = next;
            workspace.errorText = ''; workspace.noticeText = 'Revision ' + r.revision + ' saved';
            loadRevisions(doc, r.revision);
        });
    }
    // Read on four occasions only: opening the project, after Add version, after a clip ends, and Check files.
    function loadVersions(selectNumber) {
        checking = true;
        send('qroc-media-version-list', {project_id: projectId}, (r, ok, error) => {
            view.checking = false;
            if (!ok) { fail(error); return; }
            view.versions = (r.versions || []).slice().sort((a, b) => b.number - a.number);
            view.versionsLoaded = true;
            if (selectNumber) view.selectedVersion = selectNumber;
            else if (!view.versions.some(v => v.number === view.selectedVersion)) view.selectedVersion = view.versions.length ? view.versions[0].number : 0;
        });
    }
    function loadClips() {
        send('qroc-creative-list', {}, (r, ok) => { if (ok && r) view.outputs = r.outputs || []; });
    }
    function makeClip() {
        if (!version || version.state !== 'unchanged' || !clipToolAvailable || clipRunning || clipStarting) return;
        var args = {project_id: projectId, version: version.number, start_seconds: Number(clipStart) || 0, duration_seconds: Number(clipLength) || 0, volume: Number(clipVolume)};
        clipStarting = true;
        send('qroc-media-version-trim', args, (r, ok, error) => {
            view.clipStarting = false;
            if (!ok) { fail(error); return; }
            view.activeClip = {artifact_id: r.artifact_id, revision: r.revision};
            loadClips();
            if (r.status === 'running') clipPoll.start(); else clipEnded();
        });
    }
    function cancelClip() {
        if (!activeClip) return;
        send('qroc-creative-cancel', activeClip, (r, ok, error) => { if (!ok) fail(error); });
    }
    function clipEnded() { clipPoll.stop(); activeClip = null; loadClips(); loadVersions(selectedVersion); }
    Timer {
        id: clipPoll
        interval: 1000; repeat: true
        onTriggered: {
            if (!view.activeClip) { stop(); return; }
            view.send('qroc-creative-status', view.activeClip, (r, ok) => {
                if (ok && r && r.status === 'running') { view.loadClips(); return; }
                view.clipEnded();
            });
        }
    }
    onProjectIdChanged: { clipPoll.stop(); addDialog.close(); open(); }

    // ---- keys (section 9.6) ----
    function moveSelection(step) {
        if (textTab) {
            var list = currentRevisions, i = list.findIndex(r => r.revision === (shownRevision ? shownRevision.revision : 0));
            var j = Math.max(0, Math.min(list.length - 1, i + step));
            if (list[j]) selectRevision(list[j].revision);
        } else {
            var k = versions.findIndex(v => v.number === selectedVersion), m = Math.max(0, Math.min(versions.length - 1, k + step));
            if (versions[m]) selectedVersion = versions[m].number;
        }
    }
    Shortcut { sequence: 'Ctrl+S'; enabled: view.visible && view.canSave; context: Qt.WindowShortcut; onActivated: view.save() }
    FocusScope {
        id: listFocus
        anchors.fill: parent
        focus: true
        Keys.onUpPressed: view.moveSelection(-1)
        Keys.onDownPressed: view.moveSelection(1)
        Keys.onReturnPressed: view.focusReader()
        Keys.onEnterPressed: view.focusReader()
    }
    function focusReader() { if (textTab && editorOpen) reader.editor.forceActiveFocus(); else if (!textTab) versionCard.forceActiveFocus(); }
    function focusList() { listFocus.forceActiveFocus(); }

    component RowText: Text { font.family: Theme.font; font.pixelSize: Theme.fsSm; color: Theme.textPrimary; elide: Text.ElideRight }
    component SubText: Text { font.family: Theme.font; font.pixelSize: Theme.fsXs; color: Theme.textMuted; elide: Text.ElideRight }
    component FieldLabel: Text { font.family: Theme.font; font.pixelSize: Theme.fsXs; font.weight: Font.Medium; color: Theme.textSecondary }
    component CardRow: RowLayout {
        property string label: ''
        property string value: ''
        Layout.fillWidth: true
        spacing: Theme.s3
        Text { Layout.preferredWidth: 72; Layout.alignment: Qt.AlignTop; text: parent.label; font.family: Theme.font; font.pixelSize: Theme.fsSm; color: Theme.textSecondary }
        Text { Layout.fillWidth: true; text: parent.value; font.family: Theme.font; font.pixelSize: Theme.fsSm; color: Theme.textPrimary; wrapMode: Text.Wrap }
    }

    RowLayout {
        anchors.fill: parent
        anchors.margins: workspace.compactWorkspace ? Theme.s2 : Theme.s4
        spacing: Theme.s4

        // ---- the list (300 px; full width under the small breakpoint is not needed: it keeps 300 or less) ----
        Rectangle {
            Layout.preferredWidth: Math.min(300, Math.max(180, view.width * 0.36)); Layout.fillHeight: true; Layout.minimumHeight: 0
            color: Theme.bgSecondary; border.width: Theme.borderWidth; border.color: Theme.border; radius: Theme.radius
            clip: true
            ColumnLayout {
                anchors.fill: parent; anchors.margins: Theme.borderWidth
                spacing: 0
                // The product project this piece follows, by title, or none; changed here at any time.
                ColumnLayout {
                    Layout.fillWidth: true; Layout.margins: Theme.s3
                    spacing: Theme.s1
                    FieldLabel { text: 'Follows' }
                    Choice {
                        objectName: 'qroc-media-follows'
                        Layout.fillWidth: true
                        readonly property bool lost: !!workspace.linkedProject && workspace.linkedProject.found === false
                        model: ['On its own'].concat(workspace.productProjects.map(p => p.title))
                        currentIndex: workspace.linkedProject ? workspace.productProjects.findIndex(p => p.id === workspace.linkedProject.id) + 1 : 0
                        displayText: lost ? 'A product project that is gone' : workspace.linkedProject ? workspace.linkedProject.title : 'On its own'
                        onActivated: index => workspace.linkProject(index > 0 ? workspace.productProjects[index - 1].id : '')
                        Accessible.name: 'Follows a product project'
                    }
                }
                Rectangle { Layout.fillWidth: true; height: Theme.borderWidth; color: Theme.border }
                Flow {
                    visible: !view.textTab
                    Layout.fillWidth: true; Layout.margins: Theme.s3
                    spacing: Theme.s2
                    SButton { objectName: 'qroc-media-add-version'; text: 'Add version'; iconName: 'plus'; primary: true; onClicked: addDialog.openFresh() }
                    SButton { objectName: 'qroc-media-check-files'; text: view.checking ? 'Checking…' : 'Check files'; quiet: true; busy: view.checking; enabled: !view.checking; onClicked: view.loadVersions(view.selectedVersion) }
                }
                Rectangle { visible: !view.textTab; Layout.fillWidth: true; height: Theme.borderWidth; color: Theme.border }
                ListView {
                    id: revisionList
                    visible: view.textTab
                    Layout.fillWidth: true; Layout.fillHeight: true
                    clip: true
                    model: view.currentRevisions
                    ScrollBar.vertical: SScrollBar {}
                    delegate: Rectangle {
                        id: revRow
                        required property var modelData
                        required property int index
                        readonly property bool selected: !!view.shownRevision && view.shownRevision.revision === modelData.revision
                        width: revisionList.width; height: Theme.docRowHeight
                        color: selected ? Qt.alpha(Theme.accentPrimary, 0.10) : revHover.hovered ? Theme.bgTertiary : 'transparent'
                        Rectangle { anchors.bottom: parent.bottom; width: parent.width; height: Theme.borderWidth; color: Theme.border; visible: revRow.index < revisionList.count - 1 }
                        HoverHandler { id: revHover }
                        TapHandler { onTapped: { view.selectRevision(revRow.modelData.revision); view.focusList(); } }
                        RowLayout {
                            anchors.fill: parent; anchors.leftMargin: Theme.s4; anchors.rightMargin: Theme.s4
                            spacing: Theme.s3
                            ColumnLayout {
                                Layout.fillWidth: true; spacing: 2
                                RowText { Layout.fillWidth: true; text: 'Revision ' + revRow.modelData.revision; font.weight: Font.Medium; color: revRow.selected ? Theme.accentPrimary : Theme.textPrimary }
                                SubText { Layout.fillWidth: true; text: view.timeWords(revRow.modelData.created_at) }
                            }
                            Pill { visible: revRow.index === 0; text: 'Latest'; tone: Theme.success }
                        }
                    }
                }
                ListView {
                    id: versionList
                    visible: !view.textTab
                    Layout.fillWidth: true; Layout.fillHeight: true
                    clip: true
                    model: view.versions
                    ScrollBar.vertical: SScrollBar {}
                    delegate: Rectangle {
                        id: verRow
                        required property var modelData
                        required property int index
                        readonly property bool selected: view.selectedVersion === modelData.number
                        width: versionList.width; height: Theme.docRowHeight
                        color: selected ? Qt.alpha(Theme.accentPrimary, 0.10) : verHover.hovered ? Theme.bgTertiary : 'transparent'
                        Rectangle { anchors.bottom: parent.bottom; width: parent.width; height: Theme.borderWidth; color: Theme.border; visible: verRow.index < versionList.count - 1 }
                        HoverHandler { id: verHover }
                        TapHandler { onTapped: { view.selectedVersion = verRow.modelData.number; view.focusList(); } }
                        RowLayout {
                            anchors.fill: parent; anchors.leftMargin: Theme.s4; anchors.rightMargin: Theme.s4
                            spacing: Theme.s3
                            ColumnLayout {
                                Layout.fillWidth: true; spacing: 2
                                RowText { Layout.fillWidth: true; text: verRow.modelData.number + ' · ' + verRow.modelData.name; font.weight: Font.Medium; color: verRow.selected ? Theme.accentPrimary : Theme.textPrimary }
                                SubText { Layout.fillWidth: true; text: view.fileName(verRow.modelData.reference) + (verRow.modelData.duration_seconds !== null && verRow.modelData.duration_seconds !== undefined ? ' · ' + view.durationWords(verRow.modelData.duration_seconds) : '') }
                            }
                            Pill { text: view.stateWord(verRow.modelData.state); tone: view.stateTone(verRow.modelData.state) }
                        }
                    }
                }
            }
        }

        // ---- the reader ----
        Rectangle {
            Layout.fillWidth: true; Layout.fillHeight: true; Layout.minimumHeight: 0
            color: Theme.bgSecondary; border.width: Theme.borderWidth; border.color: Theme.border; radius: Theme.radius

            // Text tabs
            ColumnLayout {
                visible: view.textTab && view.editorOpen
                anchors.fill: parent; anchors.margins: workspace.compactWorkspace ? Theme.s3 : Theme.s4
                spacing: Theme.s3
                RowLayout {
                    Layout.fillWidth: true; spacing: Theme.s2
                    Text {
                        Layout.fillWidth: !view.unsaved
                        text: view.docWord(view.documentKey) + (view.shownRevision ? ' · Revision ' + view.shownRevision.revision : '')
                        font.family: Theme.font; font.pixelSize: Theme.fsLg; font.weight: Font.Bold; color: Theme.textPrimary
                        elide: Text.ElideRight
                    }
                    Pill { objectName: 'qroc-media-unsaved'; visible: view.unsaved; text: 'Unsaved changes'; tone: Theme.warning }
                    Item { visible: view.unsaved; Layout.fillWidth: true }
                    SButton { visible: !view.atLatest; text: 'Latest saved'; quiet: true; onClicked: view.selectRevision(view.latest.revision) }
                    STabButton { text: 'Edit'; enabled: view.atLatest; active: view.atLatest && workspace.viewMode === 'raw'; onClicked: workspace.viewMode = 'raw' }
                    STabButton { text: 'Read'; active: !view.atLatest || workspace.viewMode === 'rendered'; onClicked: workspace.viewMode = 'rendered' }
                    SButton { objectName: 'qroc-media-save'; text: 'Save revision'; primary: true; enabled: view.canSave; busy: view.saving; onClicked: view.save() }
                }
                DocumentReader {
                    id: reader
                    objectName: 'qroc-media-reader'
                    Layout.fillWidth: true; Layout.fillHeight: true; Layout.minimumHeight: 140
                    text: view.editorText
                    title: view.docWord(view.documentKey)
                    positionKey: 'qroc/media/' + view.projectId + '/' + view.documentKey + '/' + (view.shownRevision ? view.shownRevision.revision : 0)
                    editable: view.atLatest && workspace.viewMode === 'raw'
                    textFormat: editable ? TextEdit.PlainText : TextEdit.MarkdownText
                    onEdited: value => view.edit(value)
                    Keys.onEscapePressed: view.focusList()
                }
            }
            EmptyState {
                anchors.fill: parent
                visible: view.textTab && !view.editorOpen && !!view.loaded[view.documentKey]
                icon: 'edit'
                title: view.documentKey === 'lyrics' ? 'No lyrics yet' : 'No concepts yet'
                detail: 'Write them here; every save is kept as a revision.'
                actionText: view.documentKey === 'lyrics' ? 'Write lyrics' : 'Write concepts'
                actionIcon: 'edit'
                onActionRequested: { workspace.viewMode = 'raw'; view.startWriting(); }
            }

            // Versions
            EmptyState {
                anchors.fill: parent
                visible: !view.textTab && view.versionsLoaded && view.versions.length === 0
                icon: 'mic'
                title: 'No version yet'
                detail: 'Add a file from the media store as the first version.'
                actionText: 'Add version'
                onActionRequested: addDialog.openFresh()
            }
            Flickable {
                id: versionCard
                objectName: 'qroc-media-version-card'
                visible: !view.textTab && !!view.version
                anchors.fill: parent; anchors.margins: Theme.borderWidth
                clip: true
                contentWidth: width; contentHeight: cardColumn.implicitHeight + Theme.s4 * 2
                boundsBehavior: Flickable.StopAtBounds
                ScrollBar.vertical: SScrollBar {}
                Keys.onEscapePressed: view.focusList()
                Keys.onUpPressed: view.moveSelection(-1)
                Keys.onDownPressed: view.moveSelection(1)
                ColumnLayout {
                    id: cardColumn
                    x: Theme.s4; y: Theme.s4
                    width: versionCard.width - Theme.s4 * 2
                    spacing: Theme.s4
                    Card {
                        Layout.fillWidth: true
                        padding: Theme.s5
                        spacing: Theme.s2
                        RowLayout {
                            Layout.fillWidth: true; spacing: Theme.s2
                            Text { Layout.fillWidth: true; text: view.version ? view.version.name : ''; font.family: Theme.font; font.pixelSize: Theme.fsLg; font.weight: Font.Bold; color: Theme.textPrimary; elide: Text.ElideRight }
                            Pill { text: view.version ? view.stateWord(view.version.state) : ''; tone: view.version ? view.stateTone(view.version.state) : Theme.pending }
                        }
                        CardRow { label: 'File'; value: view.version ? view.fileName(view.version.reference) : '' }
                        CardRow { label: 'Size'; value: !view.version ? '' : view.sizeWords(view.version.now && view.version.now.bytes !== null && view.version.now.bytes !== undefined ? view.version.now.bytes : view.version.bytes) }
                        CardRow { label: 'Duration'; value: view.version ? view.durationWords(view.version.duration_seconds) : '' }
                        CardRow { label: 'Referenced'; value: view.version ? view.timeWords(view.version.referenced_at) : '' }
                        CardRow { visible: !!view.version && !!view.version.note; label: 'Note'; value: view.version ? view.version.note || '' : '' }
                        Text {
                            objectName: 'qroc-media-state-sentence'
                            Layout.fillWidth: true; Layout.topMargin: Theme.s1
                            text: view.version ? view.stateSentence(view.version.state) : ''
                            font.family: Theme.font; font.pixelSize: Theme.fsSm; wrapMode: Text.Wrap
                            color: view.version ? view.stateTextTone(view.version.state) : Theme.textSecondary
                        }
                    }
                    Card {
                        id: clipCard
                        objectName: 'qroc-media-clip'
                        Layout.fillWidth: true
                        padding: Theme.s5
                        spacing: Theme.s3
                        readonly property bool clipUsable: !!view.version && view.version.state === 'unchanged'
                        Text { text: 'Clip'; font.family: Theme.font; font.pixelSize: Theme.fsBase; font.weight: Font.DemiBold; color: Theme.textPrimary }
                        Text {
                            visible: !view.clipToolAvailable
                            Layout.fillWidth: true
                            text: 'Clip making is not available on this machine.'
                            font.family: Theme.font; font.pixelSize: Theme.fsSm; color: Theme.textSecondary; wrapMode: Text.Wrap
                        }
                        GridLayout {
                            visible: view.clipToolAvailable
                            Layout.fillWidth: true
                            columns: cardColumn.width < 560 ? 1 : 3
                            columnSpacing: Theme.s3; rowSpacing: Theme.s2
                            enabled: clipCard.clipUsable && !view.clipRunning
                            ColumnLayout { Layout.fillWidth: true; spacing: Theme.s1
                                FieldLabel { text: 'Start (s)' }
                                Field { Layout.fillWidth: true; text: view.clipStart; validator: DoubleValidator { bottom: 0; top: 3600 } onTextEdited: view.clipStart = text } }
                            ColumnLayout { Layout.fillWidth: true; spacing: Theme.s1
                                FieldLabel { text: 'Length (s, max 30)' }
                                Field { Layout.fillWidth: true; text: view.clipLength; validator: DoubleValidator { bottom: 0.1; top: 30 } onTextEdited: view.clipLength = text } }
                            ColumnLayout { Layout.fillWidth: true; spacing: Theme.s1
                                FieldLabel { text: 'Volume (1 = unchanged)' }
                                Field { Layout.fillWidth: true; text: view.clipVolume; validator: DoubleValidator { bottom: 0; top: 2 } onTextEdited: view.clipVolume = text } }
                        }
                        Text {
                            visible: view.clipToolAvailable && !!view.version && view.version.state !== 'unchanged'
                            Layout.fillWidth: true
                            text: 'Clips are made from the file as it was referenced. Check files, or add the changed file as a new version.'
                            font.family: Theme.font; font.pixelSize: Theme.fsSm; color: Theme.textSecondary; wrapMode: Text.Wrap
                        }
                        Flow {
                            visible: view.clipToolAvailable
                            Layout.fillWidth: true; spacing: Theme.s2
                            SButton {
                                objectName: 'qroc-media-make-clip'
                                text: view.clipRunning || view.clipStarting ? 'Making clip…' : 'Make clip'
                                primary: true; busy: view.clipRunning || view.clipStarting
                                enabled: clipCard.clipUsable && !view.clipRunning && !view.clipStarting && Number(view.clipLength) > 0
                                onClicked: view.makeClip()
                            }
                            SButton { visible: view.clipRunning; text: 'Cancel'; enabled: !!view.activeClip; onClicked: view.cancelClip() }
                        }
                    }
                    Card {
                        visible: view.versionClips.length > 0
                        Layout.fillWidth: true
                        padding: Theme.s5
                        spacing: Theme.s2
                        Text { text: 'Clips'; font.family: Theme.font; font.pixelSize: Theme.fsBase; font.weight: Font.DemiBold; color: Theme.textPrimary }
                        Repeater {
                            model: view.versionClips
                            delegate: ColumnLayout {
                                required property var modelData
                                Layout.fillWidth: true
                                spacing: Theme.s1
                                RowLayout {
                                    Layout.fillWidth: true; spacing: Theme.s2
                                    RowText {
                                        Layout.fillWidth: true
                                        text: 'Clip ' + modelData.number + ' · ' + view.durationWords(modelData.steps ? modelData.steps.duration_seconds : null) + ' from ' + view.durationWords(modelData.steps ? modelData.steps.start_seconds : null)
                                    }
                                    Pill { text: view.clipWord(modelData.status); tone: view.clipTone(modelData.status) }
                                }
                                Text {
                                    visible: modelData.status === 'failed' && !!modelData.error
                                    Layout.fillWidth: true
                                    text: view.plain(modelData.error)
                                    font.family: Theme.font; font.pixelSize: Theme.fsXs; color: Theme.dangerText; wrapMode: Text.Wrap
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    // ---- Add version ----
    FileDialog {
        id: fileDialog
        title: 'Choose file'
        fileMode: FileDialog.OpenFile
        onAccepted: addDialog.choose(selectedFile)
    }
    ModalFrame {
        id: addDialog
        objectName: 'qroc-media-add-dialog'
        title: 'Add version'
        maxWidth: Theme.modalLg
        bodySpacing: Theme.s4
        property string path: ''
        property string name: ''
        property string note: ''
        property string refusalText: ''
        property bool busy: false
        function openFresh() { path = ''; name = ''; note = ''; refusalText = ''; busy = false; open(); }
        function choose(url) {
            path = workspace.pathFromUrl(url);
            var file = view.fileName(path), dot = file.lastIndexOf('.');
            name = dot > 0 ? file.slice(0, dot) : file;
            refusalText = '';
        }
        function submit() {
            if (!path || !name.trim() || busy) return;
            busy = true; refusalText = '';
            var args = {project_id: view.projectId, name: name.trim(), path: path};
            if (note.trim()) args.note = note.trim();
            view.send('qroc-media-version-add', args, (r, ok, error) => {
                addDialog.busy = false;
                if (!ok) { addDialog.refusalText = view.refusal(error) || ('QROC could not do that.\n' + view.plain(error)); return; }
                addDialog.close();
                workspace.errorText = ''; workspace.noticeText = 'Version added';
                view.loadVersions(r.number);
            });
        }
        RowLayout {
            Layout.fillWidth: true; spacing: Theme.s3
            SButton { objectName: 'qroc-media-choose-file'; text: 'Choose file'; onClicked: fileDialog.open() }
            Text {
                Layout.fillWidth: true
                text: addDialog.path ? view.fileName(addDialog.path) : 'No file chosen'
                font.family: Theme.font; font.pixelSize: Theme.fsSm; color: addDialog.path ? Theme.textPrimary : Theme.textMuted; elide: Text.ElideMiddle
            }
        }
        ColumnLayout {
            Layout.fillWidth: true; spacing: Theme.s2
            FieldLabel { text: 'Name' }
            Field { objectName: 'qroc-media-version-name'; Layout.fillWidth: true; text: addDialog.name; onTextEdited: addDialog.name = text; Accessible.name: 'Name' }
        }
        ColumnLayout {
            Layout.fillWidth: true; spacing: Theme.s2
            FieldLabel { text: 'Note' }
            Field { Layout.fillWidth: true; text: addDialog.note; onTextEdited: addDialog.note = text; Accessible.name: 'Note' }
        }
        Text {
            objectName: 'qroc-media-add-refusal'
            visible: !!addDialog.refusalText
            Layout.fillWidth: true
            text: addDialog.refusalText
            font.family: Theme.font; font.pixelSize: Theme.fsSm; color: Theme.dangerText; wrapMode: Text.Wrap
        }
        footer: [
            SButton { text: 'Cancel'; quiet: true; onClicked: addDialog.close() },
            SButton { objectName: 'qroc-media-add-submit'; text: 'Add version'; primary: true; busy: addDialog.busy; enabled: !!addDialog.path && !!addDialog.name.trim() && !addDialog.busy; onClicked: addDialog.submit() }
        ]
    }
    property alias addVersionDialog: addDialog
}

import QtQuick
import QtQuick.Controls
import QtQuick.Dialogs
import QtQuick.Layouts

Item {
    id: root
    property string projectId: ""
    property string branchId: ""
    property string sourcePath: ""
    property real startSeconds: 0
    property real durationSeconds: 10
    property real volume: 1
    property var assets: []
    property var outputs: []
    property var capabilities: []
    property var selectedAsset: null
    property var selectedOutput: null
    property string errorText: ""
    property string noticeText: ""
    property bool busy: false
    property bool initialized: false
    property var lastExport: null
    readonly property bool running: !!selectedOutput && selectedOutput.status === "running"
    readonly property bool canConvert: projectId !== "" && branchId !== "" && !!selectedAsset && !busy && !running
        && capabilities.some(c => c.id === "ffmpeg-audio-wav-v1" && c.supported)
    property string projectTitle: ""
    property string branchName: ""
    signal artifactExported(var record)
    signal openWorkspaceRequested()
    function request(command, args, next) {
        var p = projectId, b = branchId;
        busy = true;
        Backend.send(command, args, function(r, ok, error) {
            busy = false;
            if (p !== projectId || b !== branchId) { refresh(); return; }
            if (!ok) { errorText = error; return; }
            errorText = "";
            if (next) next(r);
        });
    }
    function refresh() {
        request("qroc-creative-list", {}, function(r) {
            assets = (r.assets || []).filter(a => a.project_id === projectId && a.branch_id === branchId);
            outputs = (r.outputs || []).filter(o => o.project_id === projectId && o.branch_id === branchId);
            if (!selectedAsset && assets.length) selectedAsset = assets[assets.length - 1];
            if (outputs.length) {
                var prior = selectedOutput;
                selectedOutput = outputs.find(o => prior && o.artifact_id === prior.artifact_id && o.revision === prior.revision) || outputs[outputs.length - 1];
            }
        });
    }
    function importSource(newVersion) {
        if (!projectId || !branchId) { errorText = "Choose a project and branch first."; return; }
        var args = {project_id: projectId, branch_id: branchId, path: sourcePath};
        if (newVersion && selectedAsset) args.asset_id = selectedAsset.asset_id;
        request("qroc-creative-source-import", args, function(r) {
            selectedAsset = r; noticeText = "Source saved as revision " + r.revision; refresh();
        });
    }
    function convert(newVersion) {
        if (!canConvert) return;
        var args = {adapter_id:"ffmpeg-audio-wav-v1", project_id:projectId, branch_id:branchId,
            asset_id:selectedAsset.asset_id, source_revision:selectedAsset.revision,
            start_seconds:startSeconds, duration_seconds:durationSeconds, volume:volume};
        if (newVersion && selectedOutput) args.artifact_id = selectedOutput.artifact_id;
        request("qroc-creative-start", args, function(r) {
            selectedOutput = r; noticeText = "Converting with ffmpeg…"; refresh();
        });
    }
    function inspect() {
        if (!selectedOutput || busy) return;
        request("qroc-creative-status", {artifact_id:selectedOutput.artifact_id, revision:selectedOutput.revision}, function(r) {
            selectedOutput = r;
            if (r.status !== "running") { noticeText = "Conversion " + r.status; refresh(); }
        });
    }
    function cancel() {
        if (!running || busy) return;
        request("qroc-creative-cancel", {artifact_id:selectedOutput.artifact_id, revision:selectedOutput.revision}, function(r) {
            selectedOutput = r; noticeText = "Cancelled · partial output kept, not exportable"; refresh();
        });
    }
    function exportOutput() {
        if (!selectedOutput || selectedOutput.status !== "succeeded") return;
        request("qroc-creative-export", {artifact_id:selectedOutput.artifact_id, revision:selectedOutput.revision}, function(r) {
            lastExport = r; noticeText = "Exported to " + r.directory;
            artifactExported(r);
        });
    }
    function resetScope() {
        selectedAsset = null; selectedOutput = null; assets = []; outputs = []; lastExport = null; noticeText = "";
        if (initialized && !busy) refresh();
    }
    onProjectIdChanged: resetScope()
    onBranchIdChanged: resetScope()
    Component.onCompleted: {
        initialized = true;
        Backend.send("qroc-creative-capabilities", {}, function(r, ok, error) { if (ok) { capabilities = r.capabilities || []; refresh(); } else errorText = error; });
    }
    function assetIndex() {
        if (!selectedAsset) return -1;
        return assets.findIndex(a => a.asset_id === selectedAsset.asset_id && a.revision === selectedAsset.revision);
    }
    function outputIndex() {
        if (!selectedOutput) return -1;
        return outputs.findIndex(o => o.artifact_id === selectedOutput.artifact_id && o.revision === selectedOutput.revision);
    }
    // Names people read: a file by its own name, a source and an output by their place and revision, a status in a word.
    function fileName(path) { return String(path || "").split("/").pop(); }
    function sourceName(a) { return a ? (fileName(a.original_path) || "Source") + " · revision " + a.revision : ""; }
    function outputName(o) {
        if (!o) return "";
        var clips = outputs.map(x => x.artifact_id).filter((id, i, all) => all.indexOf(id) === i);
        return "Clip " + (clips.indexOf(o.artifact_id) + 1) + " · revision " + o.revision + " · " + statusWord(o.status);
    }
    function statusWord(s) { return ({running: "Running", succeeded: "Ready", failed: "Failed", cancelled: "Cancelled", queued: "Waiting"})[s] || String(s || ""); }
    function outputRows() {
        var o = selectedOutput;
        if (!o) return [];
        var rows = [
            {label: "File", value: fileName(o.file), mono: false},
            {label: "Tool", value: o.tool && o.tool.version ? o.tool.version : "", mono: true}
        ];
        var err = [o.error || "", o.tool_error || ""].filter(s => s.length > 0).join(" · ");
        if (err) rows.push({label: "Error", value: err, mono: false, bad: true});
        return rows;
    }
    // A creative tool by what it makes, never by its engine slug.
    function toolName(c) {
        var known = {"ffmpeg-audio-wav-v1": "Audio clips", "speech-tts": "Speech", "music-composition": "Music", "video-edit": "Video editing", "image-art": "Images", "game-prototype": "Game prototypes"};
        if (known[c.id]) return known[c.id];
        var words = String(c.id || "").replace(/-v\d+$/, "").split(/[-_]/).filter(w => w);
        return words.length ? words[0].charAt(0).toUpperCase() + words[0].slice(1) + (words.length > 1 ? " " + words.slice(1).join(" ") : "") : "Tool";
    }
    function toolReason(c) { var r = String(c.reason || ""); return c.supported ? "" : /adapter|admitted/i.test(r) || !r ? "Not set up on this computer yet" : r; }
    component FieldLabel: Text {
        font.family: Theme.font
        font.pixelSize: Theme.fsXs
        font.weight: Font.Medium
        color: Theme.textSecondary
    }
    component CardTitle: Text {
        font.family: Theme.font
        font.pixelSize: Theme.fsLg
        font.weight: Font.DemiBold
        color: Theme.textPrimary
    }
    component Caption: Text {
        font.family: Theme.font
        font.pixelSize: Theme.fsXs
        color: Theme.textMuted
        wrapMode: Text.Wrap
    }
    Timer { interval:250; repeat:true; running:root.running && !root.busy; onTriggered:root.inspect() }
    ColumnLayout {
        anchors.fill: parent
        spacing: 0
        AppHeader { Layout.fillWidth: true; title: "Creative" }
        ScrollView {
            id: scroll
            Layout.fillWidth: true
            Layout.fillHeight: true
            contentWidth: availableWidth
            contentHeight: body.implicitHeight + Theme.s6 * 2
            clip: true
            ColumnLayout {
                id: body
                x: Math.max(Theme.s6, (scroll.availableWidth - Theme.contentMaxWidth) / 2)
                y: Theme.s6
                width: Math.min(Theme.contentMaxWidth, scroll.availableWidth - Theme.s6 * 2)
                spacing: Theme.s5
                ColumnLayout {
                    Layout.fillWidth: true
                    spacing: Theme.s1
                    EyebrowLabel { text: "Local audio clip" }
                    Text { text: "Creative"; font.family: Theme.font; font.pixelSize: Theme.fsXl; font.weight: Font.DemiBold; color: Theme.textPrimary }
                    Text { Layout.fillWidth: true; text: "Make a listening preview or sound clip from a local audio or video file."; font.family: Theme.font; font.pixelSize: Theme.fsSm; color: Theme.textSecondary; wrapMode: Text.Wrap }
                    Pill {
                        Layout.topMargin: Theme.s2
                        visible: !!root.projectTitle && !!root.branchName
                        text: root.projectTitle + " · " + root.branchName
                        neutral: true
                    }
                }
                EmptyState {
                    Layout.fillWidth: true
                    visible: !root.projectId || !root.branchId
                    icon: "mic"
                    title: "Choose a project and branch"
                    actionText: "Open QROC"
                    actionIcon: "arrow-right"
                    onActionRequested: root.openWorkspaceRequested()
                }
                Card {
                    Layout.fillWidth: true
                    visible: root.errorText !== ""
                    nested: true
                    Text { Layout.fillWidth: true; text: root.errorText; font.family: Theme.font; font.pixelSize: Theme.fsSm; color: Theme.dangerText; wrapMode: Text.WrapAnywhere }
                }
                Card {
                    Layout.fillWidth: true
                    visible: root.noticeText !== ""
                    nested: true
                    Text { Layout.fillWidth: true; text: root.noticeText; font.family: Theme.font; font.pixelSize: Theme.fsSm; color: Theme.textSecondary; wrapMode: Text.WrapAnywhere }
                }
                Card {
                    Layout.fillWidth: true
                    padding: Theme.s5
                    CardTitle { text: "1 · Source" }
                    FieldLabel { text: "Source file" }
                    RowLayout {
                        Layout.fillWidth: true; spacing: Theme.s2
                        Field { Layout.fillWidth: true; placeholderText: "Choose a recording"; text: root.fileName(root.sourcePath); readOnly: true; enabled: !root.busy && !root.running }
                        SButton { text: "Browse"; iconName: "folder"; enabled: !root.busy && !root.running; onClicked: sourceDialog.open() }
                    }
                    FileDialog {
                        id: sourceDialog
                        title: "Choose a recording"
                        fileMode: FileDialog.OpenFile
                        onAccepted: root.sourcePath = decodeURIComponent(String(selectedFile).replace(/^file:\/\//, ""))
                    }
                    Caption { Layout.fillWidth: true; text: "An audio or video file, up to 64 MB" }
                    Flow {
                        Layout.fillWidth: true
                        spacing: Theme.s2
                        SButton { text: "Import"; primary: true; enabled: !root.busy && !root.running && !!root.projectId && !!root.branchId && !!root.sourcePath; onClicked: root.importSource(false) }
                        SButton { text: "Import as revision"; enabled: !root.busy && !root.running && !!root.selectedAsset && !!root.sourcePath; onClicked: root.importSource(true) }
                    }
                    Choice {
                        Layout.fillWidth: true
                        model: root.assets.map(a => root.sourceName(a))
                        currentIndex: root.assetIndex()
                        displayText: root.selectedAsset ? root.sourceName(root.selectedAsset) : "No imported source"
                        onActivated: index => root.selectedAsset = root.assets[index]
                        Accessible.name: "Source"
                    }
                }
                Card {
                    Layout.fillWidth: true
                    padding: Theme.s5
                    CardTitle { text: "2 · Clip" }
                    GridLayout {
                        Layout.fillWidth: true
                        columns: scroll.availableWidth < 560 ? 1 : 3
                        columnSpacing: Theme.s3
                        rowSpacing: Theme.s2
                        ColumnLayout { Layout.fillWidth: true; spacing: Theme.s1
                            FieldLabel { text: "Start (s)" }
                            Field { Layout.fillWidth: true; text: String(root.startSeconds); validator: DoubleValidator { bottom: 0; top: 3600 } onEditingFinished: root.startSeconds = Number(text) } }
                        ColumnLayout { Layout.fillWidth: true; spacing: Theme.s1
                            FieldLabel { text: "Length (s, max 30)" }
                            Field { Layout.fillWidth: true; text: String(root.durationSeconds); validator: DoubleValidator { bottom: 0.1; top: 30 } onEditingFinished: root.durationSeconds = Number(text) } }
                        ColumnLayout { Layout.fillWidth: true; spacing: Theme.s1
                            FieldLabel { text: "Volume (1 = unchanged)" }
                            Field { Layout.fillWidth: true; text: String(root.volume); validator: DoubleValidator { bottom: 0; top: 2 } onEditingFinished: root.volume = Number(text) } }
                    }
                    Caption { Layout.fillWidth: true; text: "Mono WAV · 48 kHz" }
                    Flow {
                        Layout.fillWidth: true
                        spacing: Theme.s2
                        SButton { text: "Create WAV"; primary: true; enabled: root.canConvert; onClicked: root.convert(false) }
                        SButton { text: "Create revision"; enabled: root.canConvert && !!root.selectedOutput; onClicked: root.convert(true) }
                        SButton { text: "Cancel"; enabled: root.running && !root.busy; onClicked: root.cancel() }
                    }
                }
                Card {
                    Layout.fillWidth: true
                    padding: Theme.s5
                    CardTitle { text: "3 · Output" }
                    Choice {
                        Layout.fillWidth: true
                        model: root.outputs.map(o => root.outputName(o))
                        currentIndex: root.outputIndex()
                        displayText: root.selectedOutput ? root.outputName(root.selectedOutput) : "No output yet"
                        onActivated: index => root.selectedOutput = root.outputs[index]
                        Accessible.name: "Output"
                    }
                    EmptyState {
                        Layout.fillWidth: true
                        visible: !root.selectedOutput
                        card: false
                        icon: "play"
                        tone: Theme.textSecondary
                        title: "No output yet"
                    }
                    Repeater {
                        model: root.outputRows()
                        delegate: RowLayout {
                            required property var modelData
                            Layout.fillWidth: true
                            spacing: Theme.s3
                            Text { Layout.preferredWidth: 72; Layout.alignment: Qt.AlignTop; text: modelData.label; font.family: Theme.font; font.pixelSize: Theme.fsSm; color: Theme.textSecondary }
                            Text { Layout.fillWidth: true; text: modelData.value; font.family: modelData.mono ? Theme.mono : Theme.font; font.pixelSize: modelData.mono ? Theme.fsMono : Theme.fsSm; color: modelData.bad ? Theme.dangerText : Theme.textPrimary; wrapMode: Text.WrapAnywhere }
                        }
                    }
                    Caption {
                        Layout.fillWidth: true
                        visible: !!root.selectedOutput
                        text: root.selectedOutput ? "From source revision " + root.selectedOutput.source.revision + " · clip " + root.selectedOutput.steps.start_seconds + "s + " + root.selectedOutput.steps.duration_seconds + "s · volume " + root.selectedOutput.steps.volume : ""
                    }
                    Flow {
                        Layout.fillWidth: true
                        spacing: Theme.s2
                        SButton { text: "Refresh"; enabled: !root.busy; onClicked: root.refresh() }
                        SButton { text: "Export"; primary: true; iconName: "download"; enabled: !root.busy && !!root.selectedOutput && root.selectedOutput.status === "succeeded"; onClicked: root.exportOutput() }
                    }
                    Caption { Layout.fillWidth: true; text: "Saves the clip and a record of how it was made" }
                }
                Card {
                    Layout.fillWidth: true
                    padding: Theme.s5
                    CardTitle { text: "Tools" }
                    Flow {
                        Layout.fillWidth: true
                        spacing: Theme.s2
                        Repeater {
                            model: root.capabilities
                            delegate: Pill {
                                id: toolPill
                                required property var modelData
                                text: root.toolName(modelData) + " · " + (modelData.supported ? "available" : "unavailable")
                                tone: modelData.supported ? Theme.success : Theme.pending
                                HoverHandler { id: toolHover }
                                ToolTip.visible: toolHover.hovered && root.toolReason(modelData).length > 0
                                ToolTip.text: root.toolReason(modelData)
                            }
                        }
                    }
                }
            }
        }
    }
}

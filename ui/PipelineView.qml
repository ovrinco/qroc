import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import "pipeline/PipelineGraph.js" as Graph

// Pipeline tab: the domain map, the selected domain's graph (stage columns left to right, side-by-side
// nodes stacked, gates as joins, the hardening loop at the head of Product) and the reader beside it.
// States are the engine's: documents and revisions, jobs, gate evaluations and reviews, hardening rounds.
FocusScope {
    id: view
    required property var workspace
    property string domain: 'PRODUCT'
    property string selectedKey: 'IDEA'
    property string openedKey: 'IDEA'
    property var jobs: []
    property var gateResults: ({})
    // The engine's revise-from-gate record per gate (Gates 1-3) and the last refusal, in its own words.
    property var revisions: ({})
    property var reviseRefusals: ({})
    // The engine's own graph export for the selected branch (Product); null until it has answered.
    property var productExport: null
    readonly property var graph: Graph.layout(domain, productExport)
    readonly property var docs: workspace.allDocuments.filter(d => d.branch_id === workspace.branchId)
    readonly property var hardening: workspace.hardening
    readonly property var rounds: hardening && hardening.record ? hardening.record.rounds || [] : []
    readonly property var lastRound: rounds.length ? rounds[rounds.length - 1] : null
    readonly property bool productOpen: (!!lastRound && (lastRound.verdict === 'pass' || lastRound.verdict === 'partial'))
        || docs.some(d => workspace.domainOfType(d.document_type) === 'PRODUCT')
    readonly property bool ideaThrough: !!lastRound && (lastRound.verdict === 'pass' || lastRound.verdict === 'partial')
    readonly property bool hasIdea: !!workspace.project && !!String(workspace.project.brain_dump || '').trim()
    // Every state below is recomputed when one of the engine's reports changes.
    readonly property var states: {
        var out = {}, deps = [docs, jobs, gateResults, hardening, workspace.project, workspace.explicitModel, productExport, revisions];
        Graph.DOMAINS.forEach(d => Graph.layout(d, productExport).nodes.forEach(n => out[n.key] = withRound(n, stateOf(n))));
        return out;
    }
    readonly property var opened: graph.byKey[openedKey] || null

    function latestDoc(key) {
        var best = null;
        docs.forEach(d => { if (d.document_type === key && (!best || d.revision > best.revision)) best = d; });
        return best;
    }
    function engineKnows(key) { return workspace.domainOfType(key) !== ''; }
    function tailNode(node) { return !!node && node.fromExport && !engineKnows(node.key) && node.key !== 'IDEA'; }
    function refreshExport() {
        if (!workspace.projectId || !workspace.branchId) { productExport = null; return; }
        var key = workspace.projectId, branch = workspace.branchId;
        workspace.request('pipeline-graph', 'qroc-graph-export', {project_id: key, branch_id: branch}, (r, ok) => {
            if (workspace.projectId === key && workspace.branchId === branch) productExport = ok ? r : null;
        });
    }
    // The tail (contracts, specs, verification, join) and its instances, as the export reports them.
    function tailState(node) {
        var st = node.exportState, reason = productExport && productExport.tail_error ? String(productExport.tail_error) : '';
        if (node.instanceOf === 'PHOD_SPEC' || node.specState) {
            var spec = node.instanceOf === 'PHOD_SPEC' ? st : node.specState;
            if (spec.indexOf('unsupported') === 0) return make('Unsupported', Theme.pending, 'produced', {neutral: true, reason: spec.replace(/^unsupported:\s*/, '')});
        }
        if (st === 'produced' || st === 'verified') return make('Ready', Theme.success, 'produced');
        if (st === 'running') return make('Running', Theme.executing, 'running');
        if (st === 'failed') return make('Failed', Theme.danger, 'outline', {reason: reason});
        return null;
    }
    function jobFor(key) {
        var found = null;
        jobs.forEach(j => { if (j.branch_id === workspace.branchId && j.document_type === key) found = j; });
        return found;
    }
    function domainStarted(d) {
        if (d === 'PRODUCT') return productOpen;
        var standing = workspace.domainStanding(d);
        if (standing) return standing === 'produced';
        var current = workspace.project ? workspace.project.current_domain || 'PRODUCT' : 'PRODUCT';
        return Graph.DOMAINS.indexOf(d) <= Graph.DOMAINS.indexOf(current);
    }
    function reviewOf(doc) {
        var reviews = workspace.project ? workspace.project.gate_reviews || [] : [];
        for (var i = reviews.length - 1; i >= 0; i--)
            if (reviews[i].document_id === doc.id && reviews[i].revision === doc.revision && reviews[i].sha256 === doc.sha256) return reviews[i];
        return null;
    }
    // A domain's first nodes and the advantage facets wait for a gate the engine evaluated as passed,
    // then for the person's ask (design sheet 3.4); every other node waits only for its inputs.
    function gateEvaluatedPassed(n) {
        var doc = latestDoc('GATE_' + n), result = doc ? gateResults[doc.id + '#' + doc.revision] : null;
        return !!result && result.status === 'passed';
    }
    function triggerGate(node) {
        if (!node) return 0;
        if (node.facet) return Graph.FACET_TRIGGER_GATE;
        return node.domainFirst && Graph.DOMAIN_TRIGGER_GATE[node.domain] ? Graph.DOMAIN_TRIGGER_GATE[node.domain] : 0;
    }
    function triggerHolds(node) { var g = triggerGate(node); return !g || gateEvaluatedPassed(g); }
    function present(key) {
        if (key === 'IDEA') return productOpen;
        var n = graph.byKey[key];
        if (tailNode(n)) return n.exportState === 'produced' || n.exportState === 'verified';
        return !!latestDoc(key);
    }
    function missingInputs(node) { return node.needs.filter(k => !present(k)); }
    function triggerOf(node) {
        var g = triggerGate(node);
        return g ? 'Produced when Gate ' + g + ' passes and you ask' : 'Produced when its inputs exist';
    }
    function verdictWord(v) { return ({refuse: 'Refused', clarify: 'Needs answers', partial: 'Partly ready', pass: 'Ready'})[v] || ''; }
    function verdictTone(v) { return v === 'refuse' ? Theme.danger : v === 'pass' ? Theme.success : Theme.warning; }
    function make(word, tone, look, extra) {
        var s = {word: word, tone: tone, look: look, predicted: look === 'predicted', trigger: '', reason: ''};
        if (extra) Object.keys(extra).forEach(k => s[k] = extra[k]);
        return s;
    }
    function stateOf(node) {
        if (node.key === 'IDEA') {
            if (hardening && hardening.running) return make('Running', Theme.executing, 'running');
            if (!lastRound) return make('', Theme.pending, hasIdea ? 'produced' : 'outline');
            return make(verdictWord(lastRound.verdict), verdictTone(lastRound.verdict), 'produced');
        }
        if (tailNode(node)) { var t = tailState(node); if (t) return t; }
        var doc = latestDoc(node.key), job = jobFor(node.key);
        if (job && job.state === 'running') return make('Running', Theme.executing, 'running');
        if (job && job.state === 'queued') return make('Queued', Theme.pending, doc ? 'produced' : 'outline');
        if (job && job.state === 'cancelling') return make('Stopping', Theme.warning, doc ? 'produced' : 'outline');
        if (doc) {
            if (node.gate) {
                var review = reviewOf(doc);
                if (review) return make(review.decision === 'passed' ? 'Reviewed: passed' : 'Reviewed: failed', review.decision === 'passed' ? Theme.success : Theme.danger, 'produced');
                var result = gateResults[doc.id + '#' + doc.revision];
                if (!result) return make('', Theme.pending, 'produced');
                var g = ({passed: ['Pending review', Theme.warning], failed: ['Failed', Theme.danger], pending: ['Pending review', Theme.warning],
                          incomplete: ['Gate incomplete', Theme.warning], not_applicable: ['No gate', Theme.pending]})[result.status] || ['Pending review', Theme.warning];
                return make(g[0], g[1], 'produced', {reason: result.status === 'failed' ? result.reason || '' : ''});
            }
            if (doc.generation_finish_reason && doc.generation_finish_reason !== 'stop') return make('Partly produced', Theme.warning, 'produced');
            return make('Ready', Theme.success, 'produced');
        }
        if (job) {
            var reason = job.error || '';
            if (job.state === 'failed') return make('Failed', Theme.danger, 'outline', {reason: reason});
            if (job.state === 'cancelled') return make('Stopped', Theme.pending, 'outline');
            if (job.state === 'partial') return make('Partly produced', Theme.warning, 'outline', {reason: reason});
            if (String(job.state).indexOf('conflict') === 0) return make('Conflict: newer revision', Theme.warning, 'outline', {reason: reason});
            var blocked = /GATE_(\d+)/.exec(job.state === 'blocked' ? reason : '');
            if (blocked) return make('Blocked by Gate ' + blocked[1], Theme.warning, 'outline', {reason: reason});
        }
        var trigger = triggerOf(node);
        if ((!engineKnows(node.key) && !tailNode(node)) || !domainStarted(node.domain)) return make('Predicted', Theme.pending, 'predicted', {trigger: trigger});
        var missing = missingInputs(node);
        if (missing.length) return make('Needs: ' + missing.map(Graph.titleOf).join(', '), Theme.pending, 'outline', {trigger: trigger});
        return make('Predicted', Theme.pending, 'predicted', {trigger: trigger});
    }
    // While a revision from a gate runs, the gate and the documents it joins carry "round n of 2" in their state words.
    // The graph export carries it per node; without the export, the gate's own record names the round.
    function roundLabelOf(node) {
        if (node.revise) return node.revise.state === 'running' ? node.revise.label || '' : '';
        var label = '';
        Graph.REVISABLE_GATES.forEach(g => {
            var r = revisions[g], gate = Graph.layout('PRODUCT', productExport).byKey[g];
            if (label || !r || !r.running || !r.latest || !gate || (node.key !== g && gate.needs.indexOf(node.key) < 0)) return;
            label = 'round ' + (r.latest.rounds || []).length + ' of ' + (r.max_rounds || 2);
        });
        return label;
    }
    function withRound(node, s) {
        var label = roundLabelOf(node);
        if (!label) return s;
        // The pill is narrow: it carries the round; the state it stands for stays in the tooltip.
        s.under = s.word; s.word = label.charAt(0).toUpperCase() + label.slice(1);
        return s;
    }
    function reviseOf(key) { return revisions[key] || null; }
    function revising(key) { var r = reviseOf(key); return !!r && !!r.running; }
    function refreshRevise(key) {
        if (!workspace.projectId || !workspace.branchId || Graph.REVISABLE_GATES.indexOf(key) < 0) return;
        var project = workspace.projectId, branch = workspace.branchId;
        workspace.transport.send('qroc-gate-revise-get', {project_id: project, branch_id: branch, gate: key}, (r, ok) => {
            if (!ok || workspace.projectId !== project || workspace.branchId !== branch) return;
            var next = Object.assign({}, revisions); next[key] = r; revisions = next;
        });
    }
    // Enabled only where the engine would accept the press: a failed Gate 1, 2 or 3 with feedback, no revision
    // already running and no generation in flight in this branch.
    function canRevise(node) {
        if (!node || Graph.REVISABLE_GATES.indexOf(node.key) < 0 || !workspace.explicitModel) return false;
        var doc = latestDoc(node.key), result = doc ? gateResults[doc.id + '#' + doc.revision] : null;
        if (!result || result.status !== 'failed' || revising(node.key)) return false;
        return !jobs.some(j => j.branch_id === workspace.branchId && ['running', 'queued', 'cancelling'].indexOf(j.state) >= 0);
    }
    function reviseFromGate(node) {
        if (!canRevise(node)) return;
        var key = node.key, project = workspace.projectId, branch = workspace.branchId;
        workspace.transport.send('qroc-gate-revise-start', {project_id: project, branch_id: branch, gate: key}, (r, ok, error) => {
            var refusals = Object.assign({}, reviseRefusals); refusals[key] = ok ? '' : error || ''; reviseRefusals = refusals;
            if (!ok) return;
            refreshRevise(key); refreshJobs(); refreshExport();
        });
    }
    property var answerErrors: ({})
    function answerQuestion(node, question, text) {
        if (!node || !question || !String(text).trim()) return;
        var key = node.key;
        workspace.answerGateQuestion(workspace.branchId, key, question, text, (ok, error) => {
            var errors = Object.assign({}, answerErrors); errors[key] = ok ? '' : error || ''; answerErrors = errors;
            if (ok) refreshRevise(key);
        });
    }
    function canProduceTail(node) {
        if (!node || node.key !== 'COMPONENT_CONTRACTS' || node.exportState !== 'waiting' || !workspace.explicitModel) return false;
        var gate = latestDoc('GATE_3'), result = gate ? gateResults[gate.id + '#' + gate.revision] : null;
        return ideaThrough && !!result && result.status === 'passed';
    }
    function canProduce(node) {
        if (tailNode(node)) return canProduceTail(node);
        if (!node || node.key === 'IDEA' || !engineKnows(node.key) || latestDoc(node.key)) return false;
        var job = jobFor(node.key);
        if (job && ['running', 'queued', 'cancelling'].indexOf(job.state) >= 0) return false;
        if (!workspace.explicitModel || !workspace.authoringState.generationAllowed(node.key) || missingInputs(node).length) return false;
        if (!triggerHolds(node)) return false;
        return triggerGate(node) && !node.facet ? true : domainStarted(node.domain);
    }
    function produce(node) {
        if (!canProduce(node)) return;
        if (tailNode(node)) {
            workspace.request('pipeline-tail', 'qroc-tail-start', {project_id: workspace.projectId, branch_id: workspace.branchId}, (r, ok) => { if (ok) refreshExport(); });
            return;
        }
        workspace.request('pipeline-produce', 'qroc-generate-start', {project_id: workspace.projectId, branch_id: workspace.branchId, document_type: node.key, context: workspace.project ? workspace.project.brain_dump : ''}, (r, ok) => {
            if (ok) { workspace.jobId = r.id; workspace.jobTimer.start(); refreshJobs(); }
        });
    }
    function refreshJobs() {
        if (!workspace.projectId) return;
        workspace.requestAll('pipeline-jobs', 'qroc-job-list', {project_id: workspace.projectId}, 'jobs', (r, ok) => { if (ok) jobs = r.jobs || []; });
    }
    function refreshGates() {
        docs.filter(d => String(d.document_type).indexOf('GATE_') === 0).forEach(d => {
            var doc = latestDoc(d.document_type);
            if (!doc || doc.id !== d.id || gateResults[doc.id + '#' + doc.revision]) return;
            workspace.request('pipeline-gate-' + doc.document_type, 'qroc-gate-evaluate', {project_id: workspace.projectId, branch_id: workspace.branchId, document_type: doc.document_type, revision: doc.revision}, (r, ok) => {
                if (!ok) return;
                var next = Object.assign({}, gateResults); next[doc.id + '#' + doc.revision] = r; gateResults = next;
            });
        });
    }
    function recordReview(node, decision) {
        var doc = latestDoc(node.key);
        if (!doc) return;
        workspace.request('pipeline-review', 'qroc-gate-review', {project_id: workspace.projectId, branch_id: doc.branch_id, document_type: doc.document_type, revision: doc.revision, sha256: doc.sha256, decision: decision}, (r, ok) => {
            if (ok) workspace.project = r.project;
        });
    }
    function selectDomain(d) { domain = d; selectedKey = Graph.layout(d, productExport).nodes[0].key; openedKey = selectedKey; }
    function select(key, open) { selectedKey = key; if (open) openedKey = key; ensureVisible(key); }
    function ensureVisible(key) {
        var n = graph.byKey[key];
        if (!n || flick.width <= 0) return;
        if (n.x < flick.contentX || Graph.COLUMN_WIDTH > flick.width) flick.contentX = Math.max(0, n.x - Theme.s4);
        else if (n.x + Graph.COLUMN_WIDTH > flick.contentX + flick.width) flick.contentX = Math.min(flick.contentWidth - flick.width, n.x + Graph.COLUMN_WIDTH - flick.width + Theme.s4);
    }
    function move(direction) {
        var n = graph.byKey[selectedKey];
        if (!n) { select(graph.nodes[0].key, false); return; }
        var column = graph.nodes.filter(o => o.column === n.column);
        var i = column.indexOf(n);
        if (direction === 'up' && i > 0) select(column[i - 1].key, false);
        else if (direction === 'down' && i < column.length - 1) select(column[i + 1].key, false);
        else if (direction === 'left' && n.inbound.length) select(n.inbound[0], false);
        else if (direction === 'right' && n.dependents.length) select(n.dependents[0], false);
    }

    onVisibleChanged: if (visible) { refreshJobs(); refreshGates(); refreshExport(); workspace.loadHardening(); refreshRevise(openedKey); }
    onOpenedKeyChanged: refreshRevise(openedKey)
    onDocsChanged: { refreshGates(); if (visible) refreshExport(); }
    Connections {
        target: view.workspace
        function onProjectIdChanged() { view.jobs = []; view.gateResults = ({}); view.revisions = ({}); view.reviseRefusals = ({}); view.productExport = null; view.selectDomain('PRODUCT'); }
        function onBranchIdChanged() { if (view.visible) view.refreshExport(); }
        function onHardeningFocusRequested() { view.selectDomain('PRODUCT'); view.select('IDEA', true); }
        function onGateFocusRequested(gate) { view.selectDomain(view.workspace.domainOfType(gate) || 'PRODUCT'); view.select(gate, true); }
    }
    Timer {
        interval: 1500; repeat: true
        running: view.visible && (view.jobs.some(j => ['running', 'queued', 'cancelling'].indexOf(j.state) >= 0)
                 || view.graph.nodes.some(n => n.exportState === 'running')
                 || Object.keys(view.revisions).some(k => view.revising(k)))
        onTriggered: {
            view.refreshJobs(); view.refreshExport(); view.workspace.loadAllDocuments();
            Object.keys(view.revisions).forEach(k => { if (view.revising(k)) view.refreshRevise(k); });
        }
    }

    ColumnLayout {
        anchors.fill: parent
        spacing: 0

        // The map: one band per domain, one dot per node in graph order; a ring is a predicted node.
        Rectangle {
            Layout.fillWidth: true
            implicitHeight: bands.implicitHeight + Theme.s2 * 2
            color: Theme.bgSecondary
            Rectangle { anchors.bottom: parent.bottom; width: parent.width; height: Theme.borderWidth; color: Theme.border }
            Column {
                id: bands
                anchors.fill: parent; anchors.margins: Theme.s2
                anchors.leftMargin: Theme.s4; anchors.rightMargin: Theme.s4
                Repeater {
                    model: Graph.DOMAINS
                    delegate: Rectangle {
                        id: band
                        required property string modelData
                        readonly property bool active: view.domain === modelData
                        width: bands.width; height: 28
                        radius: Theme.radiusSm
                        color: active ? Qt.alpha(Theme.domainColor(modelData), 0.10) : bandHover.hovered ? Theme.bgTertiary : 'transparent'
                        HoverHandler { id: bandHover; cursorShape: Qt.PointingHandCursor }
                        TapHandler { onTapped: view.selectDomain(band.modelData) }
                        activeFocusOnTab: true
                        Keys.onSpacePressed: view.selectDomain(band.modelData)
                        Keys.onReturnPressed: view.selectDomain(band.modelData)
                        border.width: activeFocus ? Theme.borderWidth : 0
                        border.color: Theme.accentPrimary
                        Accessible.role: Accessible.Button
                        Accessible.name: Theme.domainLabel(modelData)
                        RowLayout {
                            anchors.fill: parent; anchors.leftMargin: Theme.s2; anchors.rightMargin: Theme.s2
                            spacing: Theme.s2
                            DomainDot { domain: band.modelData; size: Theme.dotSmall }
                            Text {
                                Layout.preferredWidth: 72
                                text: Theme.domainLabel(band.modelData)
                                font.family: Theme.font; font.pixelSize: Theme.fsXs; font.weight: band.active ? Font.DemiBold : Font.Medium
                                color: band.active ? Theme.textPrimary : Theme.textSecondary
                            }
                            Flow {
                                Layout.fillWidth: true
                                spacing: Theme.s1
                                clip: true
                                Repeater {
                                    model: Graph.layout(band.modelData, view.productExport).nodes
                                    delegate: Rectangle {
                                        required property var modelData
                                        readonly property var st: view.states[modelData.key] || ({})
                                        width: Theme.dotSmall; height: Theme.dotSmall; radius: Theme.dotSmall / 2
                                        color: st.predicted ? 'transparent' : st.word ? st.tone : Theme.textMuted
                                        border.width: st.predicted ? Theme.borderWidth : 0
                                        border.color: Theme.textMuted
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        SplitView {
            Layout.fillWidth: true; Layout.fillHeight: true; Layout.minimumHeight: 0
            orientation: Qt.Horizontal
            handle: SSplitHandle { horizontal: true }

            // The graph of the selected domain. Sideways scroll only: no zoom, pan or minimap.
            FocusScope {
                id: graphPane
                SplitView.fillWidth: true; SplitView.minimumWidth: 160
                focus: true
                activeFocusOnTab: true
                Keys.onLeftPressed: view.move('left')
                Keys.onRightPressed: view.move('right')
                Keys.onUpPressed: view.move('up')
                Keys.onDownPressed: view.move('down')
                Keys.onReturnPressed: view.openedKey = view.selectedKey
                Keys.onEnterPressed: view.openedKey = view.selectedKey
                Flickable {
                    id: flick
                    anchors.fill: parent; anchors.margins: Theme.s4
                    clip: true
                    flickableDirection: Flickable.HorizontalFlick
                    boundsBehavior: Flickable.StopAtBounds
                    onWidthChanged: view.ensureVisible(view.selectedKey)
                    contentWidth: view.graph.width
                    contentHeight: Math.max(height, view.graph.height)
                    ScrollBar.horizontal: SScrollBar { policy: ScrollBar.AsNeeded }
                    Item {
                        width: view.graph.width; height: view.graph.height
                        Canvas {
                            id: edges
                            anchors.fill: parent
                            property var deps: [view.graph, view.selectedKey, view.states, view.ideaThrough, Theme.border, Theme.textSecondary]
                            onDepsChanged: requestPaint()
                            onPaint: {
                                var ctx = getContext('2d');
                                ctx.reset();
                                ctx.lineWidth = 1;
                                var g = view.graph;
                                g.nodes.forEach(function(n) {
                                    n.inbound.forEach(function(k) {
                                        var from = g.byKey[k];
                                        if (!from) return;
                                        // The idea reaches the strategy brief only once hardening lets it through.
                                        if (k === 'IDEA' && !view.ideaThrough) return;
                                        var x1 = from.x + Graph.COLUMN_WIDTH, y1 = from.y + Graph.NODE_HEIGHT / 2;
                                        var x2 = n.x, y2 = n.y + Graph.NODE_HEIGHT / 2, mid = x2 - Graph.COLUMN_GAP / 2;
                                        ctx.strokeStyle = n.key === view.selectedKey ? Theme.textSecondary : Theme.border;
                                        ctx.beginPath();
                                        ctx.moveTo(x1 + 0.5, Math.round(y1) + 0.5);
                                        ctx.lineTo(Math.round(mid) + 0.5, Math.round(y1) + 0.5);
                                        ctx.lineTo(Math.round(mid) + 0.5, Math.round(y2) + 0.5);
                                        ctx.lineTo(x2 + 0.5, Math.round(y2) + 0.5);
                                        ctx.stroke();
                                    });
                                });
                            }
                        }
                        Repeater {
                            model: view.graph.nodes
                            delegate: Rectangle {
                                id: node
                                required property var modelData
                                readonly property var st: view.states[modelData.key] || ({})
                                readonly property bool selected: view.selectedKey === modelData.key
                                readonly property bool isInput: (view.graph.byKey[view.selectedKey] || {inbound: []}).inbound.indexOf(modelData.key) >= 0
                                x: modelData.x; y: modelData.y
                                width: Graph.COLUMN_WIDTH; height: Graph.NODE_HEIGHT
                                radius: Theme.radius
                                color: st.look === 'produced' || st.look === 'running' ? Theme.bgSecondary : Theme.bgPrimary
                                border.width: selected ? 2 : Theme.borderWidth
                                border.color: selected ? Theme.accentPrimary : isInput ? Theme.textSecondary : Theme.border
                                clip: true
                                HoverHandler { id: nodeHover; cursorShape: Qt.PointingHandCursor }
                                TapHandler { onTapped: { graphPane.forceActiveFocus(); view.select(node.modelData.key, true); } }
                                ToolTip.visible: nodeHover.hovered
                                ToolTip.text: modelData.title + (node.st.predicted && node.st.trigger ? '\n' + node.st.trigger : node.st.word ? '\n' + (node.st.under ? node.st.under + ' · ' : '') + node.st.word : '')
                                ToolTip.delay: 300
                                Accessible.role: Accessible.Button
                                Accessible.name: modelData.title + (st.word ? ', ' + st.word : '')
                                Rectangle {
                                    visible: node.modelData.gate > 0 || node.st.look === 'running'
                                    width: node.st.look === 'running' ? 3 : Theme.gateBar; height: parent.height
                                    color: node.st.look === 'running' ? Theme.executing : node.st.word ? node.st.tone : Qt.alpha(Theme.warning, 0.3)
                                }
                                RowLayout {
                                    id: nodeHead
                                    x: Theme.s3; width: parent.width - Theme.s3 - Theme.s2
                                    y: node.st.predicted && node.st.trigger ? Theme.s1 + 2 : (parent.height - height) / 2
                                    height: 24
                                    spacing: Theme.s2
                                    DomainDot { domain: node.modelData.domain; size: Theme.dotSmall }
                                    Text {
                                        Layout.fillWidth: true
                                        text: node.modelData.title
                                        elide: Text.ElideRight
                                        font.family: Theme.font; font.pixelSize: Theme.fsSm
                                        font.weight: node.modelData.gate ? Font.DemiBold : Font.Medium
                                        color: node.st.look === 'produced' || node.st.look === 'running' ? Theme.textPrimary : Theme.textSecondary
                                    }
                                    TextMetrics {
                                        id: pillText
                                        font.family: Theme.font; font.pixelSize: Theme.fsXs; font.weight: Font.Medium
                                        text: node.st.word || ''
                                        elide: Text.ElideRight; elideWidth: Graph.COLUMN_WIDTH / 2 - Theme.s4
                                    }
                                    Pill {
                                        visible: !!node.st.word
                                        text: pillText.elidedText
                                        tone: node.st.tone || Theme.pending
                                        neutral: !!node.st.predicted || !!node.st.neutral
                                    }
                                }
                                // The trigger in words, beneath the name (design sheet 3.4); whole on hover and in the reader.
                                Text {
                                    visible: !!node.st.predicted && !!node.st.trigger
                                    x: Theme.s3; width: parent.width - Theme.s3 - Theme.s2
                                    anchors.top: nodeHead.bottom
                                    text: node.st.trigger || ''
                                    elide: Text.ElideRight
                                    font.family: Theme.font; font.pixelSize: Theme.fsXs
                                    color: Theme.textMuted
                                }
                            }
                        }
                    }
                }
            }

            // The reader: the opened node, as the engine has it.
            Rectangle {
                SplitView.preferredWidth: 360; SplitView.minimumWidth: 240
                color: Theme.bgSecondary
                Rectangle { width: Theme.borderWidth; height: parent.height; color: Theme.border }
                Keys.onEscapePressed: graphPane.forceActiveFocus()
                ScrollView {
                    id: readerScroll
                    anchors.fill: parent; anchors.margins: Theme.s4
                    clip: true
                    contentWidth: availableWidth
                    visible: !!view.opened && view.opened.key === 'IDEA' || !!view.opened && !view.latestDoc(view.opened.key)
                    ColumnLayout {
                        width: readerScroll.availableWidth
                        spacing: Theme.s4
                        RowLayout {
                            Layout.fillWidth: true; spacing: Theme.s2
                            DomainDot { visible: !!view.opened; domain: view.opened ? view.opened.domain : 'PRODUCT' }
                            Text {
                                Layout.fillWidth: true
                                text: view.opened ? view.opened.title : ''
                                font.family: Theme.font; font.pixelSize: Theme.fsLg; font.weight: Font.Bold; color: Theme.textPrimary
                                elide: Text.ElideRight
                            }
                        }
                        Pill {
                            readonly property var st: view.opened ? view.states[view.opened.key] || ({}) : ({})
                            visible: !!st.word && !String(st.word).startsWith('Needs:')
                            text: st.word || ''; tone: st.tone || Theme.pending; neutral: !!st.predicted || !!st.neutral
                        }
                        HardeningPanel {
                            Layout.fillWidth: true
                            visible: !!view.opened && view.opened.key === 'IDEA'
                            workspace: view.workspace
                            hasIdea: view.hasIdea
                        }
                        // Predicted or not yet produced: what it will need and when it would be produced.
                        ColumnLayout {
                            Layout.fillWidth: true
                            visible: !!view.opened && view.opened.key !== 'IDEA'
                            spacing: Theme.s3
                            readonly property var st: view.opened ? view.states[view.opened.key] || ({}) : ({})
                            EyebrowLabel { text: 'Will need' }
                            Repeater {
                                model: view.opened && view.opened.key !== 'IDEA' ? view.opened.needs : []
                                delegate: RowLayout {
                                    required property string modelData
                                    Layout.fillWidth: true; spacing: Theme.s2
                                    DomainDot { domain: Graph.domainOf(modelData) || 'PRODUCT'; size: Theme.dotSmall }
                                    Text {
                                        Layout.fillWidth: true
                                        text: Graph.titleOf(modelData)
                                        font.family: Theme.font; font.pixelSize: Theme.fsSm
                                        color: view.present(modelData) ? Theme.textPrimary : Theme.textSecondary
                                        elide: Text.ElideRight
                                    }
                                    SIcon { name: view.present(modelData) ? 'check-circle' : 'circle'; size: Theme.iconSm; color: view.present(modelData) ? Theme.success : Theme.textMuted }
                                }
                            }
                            EyebrowLabel { visible: parent.st.look !== 'produced'; text: 'Produced when' }
                            Text {
                                Layout.fillWidth: true
                                visible: parent.st.look !== 'produced'
                                text: view.opened ? view.triggerOf(view.opened) : ''
                                wrapMode: Text.Wrap
                                font.family: Theme.font; font.pixelSize: Theme.fsSm; color: Theme.textPrimary
                            }
                            Text {
                                Layout.fillWidth: true
                                visible: !!parent.st.reason
                                text: parent.st.reason || ''
                                wrapMode: Text.Wrap
                                font.family: Theme.font; font.pixelSize: Theme.fsXs; color: parent.st.neutral ? Theme.textSecondary : Theme.dangerText
                            }
                            SButton {
                                visible: !!view.opened && (view.engineKnows(view.opened.key) || view.opened.key === 'COMPONENT_CONTRACTS' && view.opened.exportState === 'waiting')
                                text: 'Produce now'; primary: true
                                enabled: view.canProduce(view.opened)
                                onClicked: view.produce(view.opened)
                            }
                        }
                    }
                }
                // A produced gate: the gate review body as the engine evaluated it.
                ScrollView {
                    id: gateScroll
                    anchors.fill: parent; anchors.margins: Theme.s4
                    clip: true
                    contentWidth: availableWidth
                    readonly property var doc: view.opened && view.opened.gate ? view.latestDoc(view.opened.key) : null
                    visible: !!doc
                    QrocGateReview {
                        width: gateScroll.availableWidth
                        result: gateScroll.doc ? view.gateResults[gateScroll.doc.id + '#' + gateScroll.doc.revision] || null : null
                        review: gateScroll.doc ? view.reviewOf(gateScroll.doc) : null
                        revision: gateScroll.doc ? gateScroll.doc.revision : 0
                        standing: review ? 'Reviewed · ' + review.decision : 'Saved · not reviewed'
                        canReview: !!gateScroll.doc && gateScroll.doc.document_type !== 'GATE_4'
                        revisable: !!view.opened && Graph.REVISABLE_GATES.indexOf(view.opened.key) >= 0
                        reviseEnabled: view.canRevise(view.opened)
                        reviseRecord: view.opened ? view.reviseOf(view.opened.key) : null
                        openQuestions: view.workspace.gateOpenQuestions(result, reviseRecord ? reviseRecord.questions : [])
                        roundLabel: view.opened ? view.roundLabelOf(view.opened) : ''
                        reviseRefusal: view.opened ? view.reviseRefusals[view.opened.key] || '' : ''
                        answerError: view.opened ? view.answerErrors[view.opened.key] || '' : ''
                        onReviewRequested: decision => view.recordReview(view.opened, decision)
                        onReviseRequested: view.reviseFromGate(view.opened)
                        onQuestionAnswerRequested: (question, text) => view.answerQuestion(view.opened, question, text)
                    }
                }
                // A produced document: read as written.
                DocumentReader {
                    readonly property var doc: view.opened && !view.opened.gate && view.opened.key !== 'IDEA' ? view.latestDoc(view.opened.key) : null
                    anchors.fill: parent; anchors.margins: Theme.s4
                    visible: !!doc
                    text: doc ? doc.content : ''
                    title: view.opened ? view.opened.title : ''
                    positionKey: doc ? 'qroc/pipeline/' + doc.id : ''
                    editable: false
                    textFormat: TextEdit.MarkdownText
                }
            }
        }
    }
}

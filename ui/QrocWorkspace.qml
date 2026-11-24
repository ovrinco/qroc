import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import QtQuick.Dialogs
import "authoring"
import "authoring/Semantics.js" as Semantic

Item {
    id: root
    property string projectId: ''
    property string branchId: ''
    property string domain: 'PRODUCT'
    property string documentType: ''
    property string viewMode: 'raw'
    property bool knowledgeMode: false
    // Project tab strip state: 'DASHBOARD' | 'PIPELINE' | 'KB' | a domain. branchTab: 'dashboard' | a branch id.
    property string projectView: 'DASHBOARD'
    property string branchTab: 'dashboard'
    property var allDocuments: []
    property var allDocumentsByBranch: ({})
    // The engine's own evaluation of each stored gate revision (id#revision) and its domain standing (project detail).
    property var gateEvaluations: ({})
    // Each failed gate's revise record as the engine reads it (branch|gate -> its questions and last call); read only.
    property var gateQuestionRecords: ({})
    property var domainStates: []
    property string openViewOnLoad: ''
    onKnowledgeModeChanged: {
        if (knowledgeMode) { if (projectView !== 'KB') projectView = 'KB'; }
        else if (projectView === 'KB') projectView = domain;
    }
    onProjectViewChanged: { var kb = projectView === 'KB'; if (knowledgeMode !== kb) knowledgeMode = kb; }
    onDomainChanged: if (!isMedia && projectView !== 'DASHBOARD' && projectView !== 'PIPELINE' && projectView !== 'KB' && projectView !== domain) projectView = domain
    onBranchIdChanged: if (branchTab !== 'dashboard' && branchTab !== branchId) branchTab = branchId || 'dashboard'
    property var selectedKnowledge: null
    property var contextAudit: ({})
    property bool draftLoaded: false
    property int draftBaseRevision: 0
    property int documentEpoch: 0
    property var projects: []
    property var project: null
    property var branches: []
    property var documents: []
    property var currentDocument: null
    property var history: []
    property var gate: null
    property var analysisRows: []
    property var transport: Backend
    property var requestTokens: ({})
    property int requestSerial: 0
    property bool loading: false
    property string errorText: ''
    property string noticeText: ''
    property string newProjectTitle: ''
    property string newProjectBrainDump: ''
    // New Project type (section 9.2): 'product' keeps the dialog as it was; 'media' asks for the brief and its texts.
    property string newProjectKind: 'product'
    property string newMediaLyrics: ''
    property string newMediaConcepts: ''
    // A creative project may follow an earlier product project (the engine's link), or stand on its own.
    property string newMediaLink: ''
    property var linkedProject: null
    property var linkedCreativeProjects: []
    readonly property var productProjects: projects.filter(p => p.kind !== 'media')
    function linkProject(linkedId) {
        if (!projectId) return;
        var key = projectId;
        request('project-link', 'qroc-project-link', {project_id: key, linked_project_id: linkedId || null}, (r, ok) => {
            if (!ok || projectId !== key) return;
            project = r.project || project; linkedProject = r.linked_project || null;
            noticeText = linkedProject ? 'Now follows ' + linkedProject.title : 'Now stands on its own';
        });
    }
    readonly property bool isMedia: !!project && project.kind === 'media'
    readonly property var mediaTabs: ['BRIEF', 'LYRICS', 'CONCEPTS', 'VERSIONS']
    property alias media: mediaView
    property string newBranchName: ''
    property string newBranchModel: ''
    property string newBranchProvider: ''
    property bool newBranchAuthoringOnly: false
    property string gateDocumentId: ''
    readonly property bool exactSavedRevision: !!currentDocument && !!latestFor(documentType) && latestFor(documentType).id === currentDocument.id && draftText === currentDocument.content
    // The open document's standing in the store's own terms: never a review or an edit the store does not hold.
    readonly property string reviewStanding: {
        if (!currentDocument) return String(draftText || '').trim() ? 'Draft · not saved' : 'Not written yet';
        if (currentDocument.document_type !== documentType) return '';
        var latest = latestFor(documentType);
        if (latest && latest.id !== currentDocument.id) return 'Older revision · ' + currentDocument.revision;
        if (draftText !== currentDocument.content) return 'Edited · not saved';
        if (recordedGateReview) return 'Reviewed · ' + recordedGateReview.decision;
        return isGateType(documentType) && gateStatusOf(currentDocument) === 'FAILED' ? 'Saved · gate failed · not reviewed' : 'Saved · not reviewed';
    }
    readonly property var activeBranch: branches.find(b=>b.id===branchId) || null
    readonly property bool explicitModel: authoring.branchConfigured(activeBranch)
    property alias authoringState: authoring
    property string draftText: ''
    property string sourcePath: ''
    property string kbTitle: ''
    property string kbKind: 'note'
    property string kbContent: ''
    property var jobs: []
    property bool batchActive: false
    property var synthesisSources: []
    property string synthesisName: ''
    property bool synthesisStarting: false
    property var referenceDocuments: ({
        PRODUCT: ['STRATEGY_BRIEF','PRE_MORTEM','PRD','VALIDATION_PLAN','TECHNICAL_ARCHITECTURE','GATE_1','DESIGN_ALTERNATIVES','UI_UX_SPEC','ROADMAP','IMPLEMENTATION_PLAN','GATE_2','EPICS','SLICE_INDEX','GATE_3','AGENT_DIRECTIVES','TASKS'],
        BUSINESS: ['MARKET_ANALYSIS','COMPETITOR_ANALYSIS','GATE_4','BUSINESS_PLAN','BRAND_GUIDE','GO_TO_MARKET','GATE_5','FINANCIAL_OVERVIEW','SLIDE_DECK','FINANCIAL_SENSITIVITY','GATE_6'],
        PRESENCE: ['MESSAGING_FRAMEWORK','OUTBOUND_STRATEGY','INBOUND_STRATEGY','GATE_7','WEBSITE_SOCIAL','ADS_STRATEGY','VISUAL_ASSETS','GATE_8'],
        LEGAL: ['COMPLIANCE_PRIVACY','PR_POLICY','LICENSING','GATE_9','TERMS','CONTRACTS','REGISTRY_PREP','GATE_10']
    })
    readonly property var domainDocuments: authoring.documentsByDomain(referenceDocuments)
    AuthoringController {
        id:authoring
        transport:root.transport
        projectId:root.project?root.projectId:''
        branchId:root.project&&root.branches.some(b=>b.id===root.branchId)?root.branchId:''
        onScopeChanged:{root.persistDraft();root.draftLoaded=false;root.loadDocuments();}
        onCatalogChanged:{if(catalog)loadType(root.documentType);if(root.currentDocument&&root.documentType==='GATE_4'){root.gate=null;root.gateDocumentId='';root.evaluateGate(root.currentDocument,root.documentEpoch);}}
        onSemanticProfileChanged:if(semanticProfile&&root.currentDocument&&root.documentType==='GATE_4')root.evaluateGate(root.currentDocument,root.documentEpoch)
    }
    onDocumentTypeChanged:authoring.loadType(documentType)
    readonly property var semanticInspection: gate&&Object.prototype.hasOwnProperty.call(gate,'semantic')&&currentDocument ? Semantic.inspect(gate,currentDocument,documents,authoring.semanticProfile) : ({valid:!authoring.semanticDeclared(documentType),full:false,reason:authoring.semanticDeclared(documentType)?'Market review not available':'',criteria:[],consistency:[]})
    readonly property bool reviewSourcesCurrent: gate&&Object.prototype.hasOwnProperty.call(gate,'semantic')||authoring.semanticDeclared(documentType)?semanticInspection.valid:true
    property bool gateActionBusy: false
    readonly property var recordedGateReview: {
        if (!project || !currentDocument) return null;
        var reviews = project.gate_reviews || [];
        for (var i = reviews.length - 1; i >= 0; i--)
            if (reviews[i].document_id === currentDocument.id
                    && reviews[i].revision === currentDocument.revision
                    && reviews[i].sha256 === currentDocument.sha256)
                return (!Object.prototype.hasOwnProperty.call(reviews[i],'semantic_inputs')&&!authoring.semanticDeclared(currentDocument.document_type))||Semantic.reviewCurrent(reviews[i],currentDocument,documents,authoring.semanticProfile,gate)?reviews[i]:null;
        return null;
    }
    readonly property string nextReviewedDomain: {
        if (!project || !currentDocument) return '';
        var current = project.current_domain || 'PRODUCT';
        var next = {PRODUCT:['GATE_3','BUSINESS'],BUSINESS:['GATE_6','PRESENCE'],PRESENCE:['GATE_8','LEGAL']}[current];
        return next && currentDocument.document_type === next[0] ? next[1] : '';
    }
    function recordGateDecision(decision) {
        if (!exactSavedRevision || gateDocumentId !== currentDocument.id || !reviewSourcesCurrent || gateActionBusy) return;
        if(decision==='passed'&&(!gate||gate.status!=='passed'))return;
        var doc = currentDocument, projectKey = projectId;
        gateActionBusy = true;
        request('gate-review', 'qroc-gate-review', {project_id:projectKey,branch_id:doc.branch_id,document_type:doc.document_type,revision:doc.revision,sha256:doc.sha256,decision:decision}, (r,ok)=>{
            gateActionBusy = false;
            if (ok && projectId === projectKey) { project = r.project; noticeText = 'Recorded '+decision+' review for revision '+doc.revision; }
        });
    }
    function advanceReviewedDomain() {
        if (!exactSavedRevision || gateDocumentId !== currentDocument.id || !recordedGateReview || recordedGateReview.decision !== 'passed' || !nextReviewedDomain || gateActionBusy) return;
        var doc = currentDocument, projectKey = projectId;
        gateActionBusy = true;
        request('domain-advance','qroc-domain-advance',{project_id:projectKey,branch_id:doc.branch_id,document_type:doc.document_type,revision:doc.revision,expected_domain:project.current_domain || 'PRODUCT',target_domain:nextReviewedDomain},(r,ok)=>{
            gateActionBusy = false;
            if (ok && projectId === projectKey) { project = r; noticeText = 'Advanced to '+r.current_domain+' with pinned gate review evidence'; loadDomainStates(); }
        });
    }
    // ---- navigation API (shell, shot driver and the extracted views call these) ----
    function showLanding() { persistDraft(); draftLoaded = false; requestTokens = {}; pagedReads = ({}); documentEpoch++; gate = null; gateDocumentId = ''; history = []; currentDocument = null; documents = []; allDocuments = []; allDocumentsByBranch = ({}); gateEvaluations = ({}); gateQuestionRecords = ({}); domainStates = []; analysisRows = []; selectedKnowledge = null; branches = []; project = null; projectId = ''; projectView = 'DASHBOARD'; branchTab = 'dashboard'; knowledgeMode = false; }
    function showProject(id) {
        if (id && id !== projectId) loadProject(id, 'DASHBOARD');
        else if (!project && projects.length) loadProject(projects[0].id, 'DASHBOARD');
        else showDashboard();
    }
    function showDashboard() { if (isMedia) return; persistDraft(); projectView = 'DASHBOARD'; knowledgeMode = false; }
    // A domain shows its own documents only: the previous domain's open document is dropped at once, and the first
    // document of this domain is chosen from the branch's documents already in hand while they are read again.
    function showDomain(target) {
        persistDraft(); draftLoaded = false; documentEpoch++;
        domain = String(target || 'PRODUCT').toUpperCase(); projectView = domain; knowledgeMode = false;
        currentDocument = null; draftText = ''; draftBaseRevision = 0; gate = null; gateDocumentId = ''; history = [];
        var allowed = domainDocuments[domain] || [];
        documentType = '';
        if (documents.length && allowed.length) selectDocument(allowed[0]);
        loadDocuments();
    }
    function showKnowledge() { persistDraft(); projectView = 'KB'; knowledgeMode = true; }
    function showPipeline() { persistDraft(); projectView = 'PIPELINE'; knowledgeMode = false; loadRoutes(); }
    // ---- pipeline: the hardening loop and the routes table (bridge commands qroc-harden-*, qroc-routes-*) ----
    property var hardening: null
    property var routes: []
    signal hardeningFocusRequested()
    signal gateFocusRequested(string gate)
    signal settingsRequested()
    function loadHardening() {
        if (!projectId) { hardening = null; return; }
        var key = projectId;
        request('harden-get', 'qroc-harden-get', {project_id: key}, (r, ok) => { if (projectId === key) hardening = ok ? r : ({}); });
    }
    // A project is loading until its detail, every branch's documents, every latest gate's stored evaluation and the
    // hardening record have arrived; until then no count or state is shown as if it were the project's.
    readonly property bool projectLoading: {
        if (!projectId) return false;
        if (!project) return true;
        if (project.kind === 'media') return false;
        var docs = allDocuments, loaded = allDocumentsByBranch || {};
        if (!branches.every(b => loaded[b.id] !== undefined)) return true;
        if (latestDocs.some(d => isGateType(d.document_type) && !gateEvaluations[d.id + '#' + d.revision])) return true;
        if (latestDocs.some(d => gateAsksQuestions(d) && gateQuestionRecords[d.branch_id + '|' + d.document_type] === undefined)) return true;
        return hardening === null;
    }
    // The questions the last hardening round asked him and that he has not answered yet; nothing else can move until they are.
    readonly property var openQuestions: {
        var rounds = hardening && hardening.record ? hardening.record.rounds || [] : [], last = rounds.length ? rounds[rounds.length - 1] : null;
        return last && last.questions && last.questions.status === 'asked' && !last.answers ? last.questions.items || [] : [];
    }
    function openHardening() { showPipeline(); hardeningFocusRequested(); }
    function openGate(type) { showPipeline(); gateFocusRequested(type); }
    function startHardening() {
        showPipeline(); hardeningFocusRequested();
        if (!projectId || !project || !String(project.brain_dump || '').trim()) return;
        if (hardening && hardening.record && (hardening.record.rounds || []).length) return;
        request('harden-start', 'qroc-harden-start', {project_id: projectId}, (r, ok) => { if (ok) loadHardening(); });
    }
    function answerHardening(text) {
        if (!projectId || !String(text).trim()) return;
        request('harden-answer', 'qroc-harden-answer', {project_id: projectId, answers: text}, (r, ok) => { if (ok) loadHardening(); });
    }
    property Timer hardeningPoll: Timer { interval: 1500; repeat: true; running: !!root.hardening && root.hardening.running; onTriggered: root.loadHardening() }
    function loadRoutes() { request('routes', 'qroc-routes-list', {}, (r, ok) => { if (ok) routes = r.routes || []; }); }
    function selectRoute(name) { request('route-select', 'qroc-routes-select', {name: name}, (r, ok) => { routes = ok ? r.routes || [] : routes; if (!ok) loadRoutes(); }); }
    function selectBranchTab(id) {
        persistDraft();
        if (!id || id === 'dashboard') { branchTab = 'dashboard'; return; }
        draftLoaded = false; branchId = id; branchTab = id; loadDocuments();
    }
    function openNewProject() { newProjectDialog.open(); }
    function openImportProject() { importProjectDialog.open(); }
    function openNewBranch() { newBranchDialog.open(); }
    function openScope() { scopeDialog.open(); }
    function openGateReview() { gateReviewDialog.open(); }
    function openDocumentMetadata() { authoring.loadType(documentType); documentMetadata.open(); }
    function openProjectNotes() { projectNotesDialog.open(); }
    function openKnowledgeDialog() { knowledgeDialog.open(); }
    function openSkillsImport() { skillsDialog.open(); }
    function openSynthesis() { synthesisSources = [branchId]; synthesisName = 'Synthesis ' + (branches.length + 1); synthesisDialog.open(); }
    function openContext() { showContext(); }
    function openCompare() { runAnalysis(); comparisonDialog.open(); }
    function fileUrl(path) { return 'file://' + String(path).split('/').map(encodeURIComponent).join('/'); }
    function exportProject() { if (!projectId) return; request('export', 'qroc-export', {project_id: projectId}, (r, ok) => { if (ok) { noticeText = 'Exported'; Qt.openUrlExternally(fileUrl(r.path)); } }); }
    // The project's folder (one sub-folder per domain) opens in the file manager; its location is never shown.
    function openProjectFolder() {
        if (!projectId) return;
        request('open-folder', 'qroc-project-open-folder', {project_id: projectId}, (r, ok) => {
            if (ok && r && r.folder && !Qt.openUrlExternally(fileUrl(r.folder))) errorText = 'The folder could not be opened';
        });
    }

    // ---- dashboard numbers (DashboardView.tsx), derived from allDocuments / branches / project.gate_reviews ----
    readonly property var domainOrder: ['PRODUCT', 'BUSINESS', 'PRESENCE', 'LEGAL']
    readonly property int totalDocumentTargets: domainOrder.reduce((sum, d) => sum + (referenceDocuments[d] || []).length, 0)
    readonly property var latestDocs: {
        var best = {};
        for (var i = 0; i < allDocuments.length; i++) {
            var d = allDocuments[i], k = d.branch_id + '|' + d.document_type;
            if (!best[k] || d.revision > best[k].revision) best[k] = d;
        }
        return Object.keys(best).map(k => best[k]);
    }
    function domainOfType(type) {
        for (var i = 0; i < domainOrder.length; i++) if ((referenceDocuments[domainOrder[i]] || []).indexOf(type) >= 0) return domainOrder[i];
        return '';
    }
    function isGateType(type) { return String(type).indexOf('GATE_') === 0; }
    function gateStatusOf(doc) {
        var reviews = project ? project.gate_reviews || [] : [];
        for (var i = reviews.length - 1; i >= 0; i--)
            if (reviews[i].document_id === doc.id && reviews[i].revision === doc.revision && reviews[i].sha256 === doc.sha256)
                return reviews[i].decision === 'passed' ? 'PASSED' : 'FAILED';
        // A gate the engine scores as passed still needs a recorded passing review before the engine advances on it.
        var evaluated = gateEvaluations[doc.id + '#' + doc.revision];
        return evaluated && evaluated.status === 'failed' ? 'FAILED' : 'PENDING';
    }
    function gateLabelOf(doc, status) {
        var evaluated = gateEvaluations[doc.id + '#' + doc.revision];
        return status === 'PENDING' && evaluated && evaluated.status === 'passed' ? 'PENDING REVIEW' : status;
    }
    // The window's four groups read the engine's canonical domain standing (registry canonical_domain).
    readonly property var canonicalDomain: ({PRODUCT: 'PRODUCT', BUSINESS: 'DISTRIBUTION', PRESENCE: 'BRAND', LEGAL: 'ECOSYSTEM'})
    function domainStanding(domainName) {
        var id = canonicalDomain[domainName] || domainName, found = domainStates.find(d => d.id === id);
        return found ? String(found.status) : '';
    }
    function loadDomainStates() {
        if (!projectId) return;
        var key = projectId;
        request('domain-states', 'qroc-project-detail', {id: key}, (r, ok) => { if (ok && projectId === key) domainStates = r.domains || []; });
    }
    function branchDocCount(branch, domainName) {
        var seen = {};
        latestDocs.forEach(d => { if (d.branch_id === branch && (!domainName || domainOfType(d.document_type) === domainName)) seen[d.document_type] = true; });
        return Object.keys(seen).length;
    }
    function branchTopDomain(branch) {
        var best = project ? project.current_domain || 'PRODUCT' : 'PRODUCT', top = 0;
        domainOrder.forEach(d => { var n = branchDocCount(branch.id, d); if (n > top) { top = n; best = d; } });
        return best;
    }
    function branchModelText(branch) { return branch.provider && branch.provider !== 'authoring-only' && branch.model ? Theme.modelLabel(branch.model) : 'Authoring only'; }
    function branchGateState(branch, domainName) {
        var gates = latestDocs.filter(d => d.branch_id === branch && domainOfType(d.document_type) === domainName && isGateType(d.document_type)).map(gateStatusOf);
        var wanted = (referenceDocuments[domainName] || []).filter(isGateType).length;
        if (gates.indexOf('FAILED') >= 0) return 'FAILED';
        return gates.length > 0 && gates.length >= wanted && gates.every(g => g === 'PASSED') ? 'PASSED' : 'PENDING';
    }
    function projectDocCount() { var seen = {}; allDocuments.forEach(d => seen[d.document_type] = true); return Object.keys(seen).length; }
    function domainSummary(domainName) {
        var order = referenceDocuments[domainName] || [], best = 0;
        branches.forEach(b => best = Math.max(best, branchDocCount(b.id, domainName)));
        var gateDocs = latestDocs.filter(d => domainOfType(d.document_type) === domainName && isGateType(d.document_type)).map(gateStatusOf);
        var passed = gateDocs.filter(g => g === 'PASSED').length, failed = gateDocs.filter(g => g === 'FAILED').length;
        return {branches: branches.length, bestCompleted: best, total: order.length, percent: order.length ? best / order.length * 100 : 0, passed: passed, failed: failed, blocked: failed > 0};
    }
    function gateSummary() {
        var gates = latestDocs.filter(d => isGateType(d.document_type)).map(d => { var st = gateStatusOf(d); return {document: d, status: st, label: gateLabelOf(d, st)}; });
        var count = status => gates.filter(g => g.status === status).length;
        return {passed: count('PASSED'), pending: count('PENDING'), failed: count('FAILED'), recent: gates.slice(-4).reverse()};
    }
    // A failed gate whose latest review names questions only he can answer.
    function gateAsksQuestions(doc) {
        var evaluated = isGateType(doc.document_type) ? gateEvaluations[doc.id + '#' + doc.revision] : null;
        return !!evaluated && evaluated.status === 'failed' && (evaluated.questions || []).length > 0;
    }
    // The questions holding a gate: those its latest review asks, less any he has already answered or withdrawn,
    // each with the id the gate's record keeps it under (needed to answer it).
    // A stored question is the same as the review's only when its words, document and criterion all match.
    function gateOpenQuestions(result, stored) {
        var same = (a, b) => a.question === b.question && a.document === b.document && a.criterion === b.criterion;
        return (result && result.status === 'failed' ? result.questions || [] : []).map(q => {
            var kept = (stored || []).find(s => same(s, q));
            return kept && kept.standing !== 'open' ? null : Object.assign({}, q, {id: kept ? kept.id : ''});
        }).filter(q => !!q);
    }
    // His answer to one gate question, saved by the engine with the gate's record; the next revision reads it.
    // A gate never revised keeps no record yet: the question is then named by its words as the review asks it.
    function answerGateQuestion(branch, type, question, text, done) {
        var key = projectId;
        request('gate-answer-' + branch + '|' + type, 'qroc-gate-question-answer', {project_id: key, branch_id: branch, gate: type, question_id: question.id || '', question: question.question, document: question.document || '', criterion: question.criterion || '', answer: text}, (r, ok, error) => {
            if (projectId !== key) return;
            if (ok) loadGateQuestions(branch, type);
            if (done) done(ok, error);
        });
    }
    function loadGateQuestions(branch, type) {
        var key = projectId, slot = branch + '|' + type;
        request('gate-questions-' + slot, 'qroc-gate-revise-get', {project_id: key, branch_id: branch, gate: type}, (r, ok) => {
            if (projectId !== key) return;
            var next = Object.assign({}, gateQuestionRecords); next[slot] = ok ? r : {questions: []}; gateQuestionRecords = next;
        });
    }
    // Both rounds of the gate's last revise call are spent, and he has answered questions since it ended.
    function gateNewRoundReady(state) {
        var latest = state ? state.latest : null;
        if (!latest || latest.state === 'running' || latest.awaiting_press || (latest.rounds || []).length < (state.max_rounds || latest.max_rounds || 2)) return false;
        return (state.questions || []).some(q => q.standing === 'answered' && Number(q.settled_at) > Number(latest.finished_at || 0));
    }
    function gateNumberText(type) { var m = /^GATE_(\d+)$/.exec(String(type)); return m ? 'Gate ' + m[1] : titleFor(type); }
    function nextAction() {
        if (openQuestions.length) return {kind: 'questions', domain: 'PRODUCT', title: openQuestions.length === 1 ? 'One question waits for you' : openQuestions.length + ' questions wait for you',
            detail: 'Answer them in Hardening; the idea goes no further until you do.'};
        var failed = latestDocs.find(d => isGateType(d.document_type) && gateStatusOf(d) === 'FAILED');
        var reviseState = failed ? gateQuestionRecords[failed.branch_id + '|' + failed.document_type] || {} : {};
        var held = failed ? gateOpenQuestions(gateEvaluations[failed.id + '#' + failed.revision], reviseState.questions) : [];
        if (held.length) {
            var gd = domainOfType(failed.document_type), name = gateNumberText(failed.document_type);
            return {kind: 'gate', domain: gd, gate: failed.document_type, title: held.length === 1 ? 'One question holds ' + name : held.length + ' questions hold ' + name,
                detail: name + ' failed in ' + Theme.domainLabel(gd) + '. Only you can answer ' + (held.length === 1 ? 'it' : 'them') + ', and the gate goes no further until you do.'};
        }
        if (failed && gateNewRoundReady(reviseState)) {
            var nd = domainOfType(failed.document_type), gname = gateNumberText(failed.document_type);
            return {kind: 'newround', domain: nd, gate: failed.document_type, title: 'Start a new round of ' + gname,
                detail: gname + ' failed in ' + Theme.domainLabel(nd) + '. Your answers are saved; a new round writes its documents again with them.'};
        }
        if (failed) { var fd = domainOfType(failed.document_type); return {kind: 'open', domain: fd, title: 'Gate needs attention', detail: titleFor(failed.document_type) + ' failed in ' + Theme.domainLabel(fd) + '.'}; }
        if (!branches.length) return {kind: 'create', domain: 'PRODUCT', title: 'Product branch', detail: 'Create the next comparison branch for this domain.'};
        if (branchId && authoring.scopeAvailable && authoring.scopeReadKnown && !authoring.savedScope)
            return {kind: 'scope', domain: domain, title: 'Save a branch scope', detail: 'Choose the outputs this branch needs.'};
        var least = domainOrder.map(d => ({domain: d, summary: domainSummary(d)})).sort((a, b) => a.summary.percent - b.summary.percent)[0];
        return {kind: 'open', domain: least.domain, title: Theme.domainLabel(least.domain) + ' documents', detail: 'Continue where the fewest documents are done.'};
    }
    function loadAllDocuments() {
        if (!projectId) return;
        // A refresh keeps the lists in hand until the new ones arrive (a poll never empties the dashboard); a failed read
        // counts as read, so the window stops loading and the error shows.
        var key = projectId, byBranch = Object.assign({}, allDocumentsByBranch);
        branches.forEach(b => requestAll('all-docs-' + b.id, 'qroc-document-list', {project_id: key, branch_id: b.id}, 'documents', (result, ok) => {
            if (projectId !== key) return;
            byBranch[b.id] = ok ? result.documents || [] : byBranch[b.id] || [];
            allDocumentsByBranch = byBranch;
            allDocuments = [].concat.apply([], branches.map(x => byBranch[x.id] || []));
            evaluateStoredGates(key, b.id, byBranch[b.id]);
        }));
    }
    // Reads each latest stored gate revision through the engine's own evaluation; nothing is generated or changed.
    function evaluateStoredGates(key, branch, docs) {
        var latest = {};
        docs.forEach(d => { if (isGateType(d.document_type) && (!latest[d.document_type] || d.revision > latest[d.document_type].revision)) latest[d.document_type] = d; });
        Object.keys(latest).forEach(type => {
            var doc = latest[type], id = doc.id + '#' + doc.revision;
            if (gateEvaluations[id]) return;
            request('gate-eval-' + branch + '-' + type, 'qroc-gate-evaluate', {project_id: key, branch_id: branch, document_type: type, revision: doc.revision}, (r, ok) => {
                if (projectId !== key) return;
                var next = Object.assign({}, gateEvaluations); next[id] = ok ? r : {status: 'unavailable'}; gateEvaluations = next;
                if (gateAsksQuestions(doc)) loadGateQuestions(branch, type);
            });
        });
    }
    signal projectSelected(string id)
    signal documentChanged(var document)
    signal proposalRequested(var record, var segment)

    function request(key, command, args, done) {
        var next = ++requestSerial;
        var tokens = Object.assign({}, requestTokens);
        tokens[key] = next;
        requestTokens = tokens;
        if (key === 'load' || key === 'project' || key === 'documents') loading = true;
        transport.send(command, args || {}, function(result, ok, error) {
            if (!root || requestTokens[key] !== next) return;
            if (key === 'load' || key === 'project' || key === 'documents') loading = false;
            if (!ok) { errorText = error || 'QROC could not answer that request'; if (done) done(null, false, error); return; }
            errorText = '';
            if (done) done(result, true, '');
        });
    }
    // A list command answers a large project in pages (total, next_offset, more): read every page under one
    // key and hand back the whole list, so nothing stored is out of reach and every count is over all of it.
    // If the store changes between pages (the total moves), the read starts again from the first page, at most twice.
    // A poll that asks again while a read under the same key is still paging waits for it and then runs once,
    // so a read of many pages is never cut off by the next poll.
    property var pagedReads: ({})
    function requestAll(key, command, args, field, done, restarts) {
        if (!restarts && pagedReads[key]) { pagedReads[key].next = [command, args, field, done]; return; }
        pagedReads[key] = pagedReads[key] || {next: null};
        var items = [], first = null;
        function finish(result, ok, error) {
            var run = pagedReads[key];
            delete pagedReads[key];
            if (done) done(result, ok, error);
            if (run && run.next) requestAll(key, run.next[0], run.next[1], run.next[2], run.next[3]);
        }
        function page(offset) {
            request(key, command, offset ? Object.assign({}, args, {offset: offset}) : args, function(result, ok, error) {
                if (!ok) { finish(null, false, error); return; }
                if (!first) first = result;
                else if (result.total !== first.total && (restarts || 0) < 2) { requestAll(key, command, args, field, done, (restarts || 0) + 1); return; }
                items = items.concat(result[field] || []);
                if (result.more === true && result.next_offset > offset) { page(result.next_offset); return; }
                var whole = Object.assign({}, first);
                ['total', 'offset', 'next_offset', 'more'].forEach(k => delete whole[k]);
                whole[field] = items;
                finish(whole, true, '');
            });
        }
        page(0);
    }
    function pathFromUrl(url) { return decodeURIComponent(String(url).replace(/^file:\/\//, '')); }
    function latestFor(type) {
        var matches = documents.filter(function(d) { return d.document_type === type; });
        matches.sort(function(a, b) { return b.revision - a.revision; });
        return matches.length ? matches[0] : null;
    }
    function loadProjects() {
        var view = openViewOnLoad || (project ? 'keep' : 'DASHBOARD');
        openViewOnLoad = '';
        request('load', 'qroc-project-list', {}, function(result, ok) {
            if (!ok) return;
            projects = result.projects || [];
            if(!projectId&&Backend.prefs.qrocProjectId&&projects.some(p=>p.id===Backend.prefs.qrocProjectId))projectId=Backend.prefs.qrocProjectId;
            if (!projectId && projects.length) projectId = projects[0].id;
            if (projectId) loadProject(projectId, view);
        });
    }
    // view: undefined = land on the document workbench, 'keep' = reload in place, 'DASHBOARD' = open the project dashboard.
    function loadProject(id, view) {
        root.persistDraft();draftLoaded=false;
        requestTokens = {}; pagedReads = ({});
        documentEpoch++;
        gate = null; gateDocumentId = ''; history = []; currentDocument = null; documents = []; allDocuments = []; allDocumentsByBranch = ({}); gateEvaluations = ({}); gateQuestionRecords = ({}); domainStates = []; analysisRows = []; selectedKnowledge = null; project = null; hardening = null; linkedProject = null; linkedCreativeProjects = [];
        projectId = id || '';
        Backend.prefs=Object.assign({},Backend.prefs,{qrocProjectId:projectId});Backend.remember();
        projectSelected(projectId);
        request('project', 'qroc-project-detail', {id: projectId}, function(result, ok) {
            if (!ok) return;
            project = result.project || null;
            linkedProject = result.linked_project || null; linkedCreativeProjects = result.linked_creative_projects || [];
            branches = result.branches || [];
            domainStates = result.domains || [];
            if (project && project.kind === 'media') { branchId = ''; branchTab = 'dashboard'; knowledgeMode = false; projectView = 'BRIEF'; return; }
            if (!branches.length) { branchId = ''; documents = []; currentDocument = null; }
            else {
                if (!branches.some(function(b) { return b.id === branchId; })) branchId = branches[0].id;
                loadDocuments();
            }
            loadAllDocuments(); loadHardening(); loadRoutes();
            if (view === 'keep') { if (branchTab !== 'dashboard' && branchTab !== branchId) branchTab = branchId || 'dashboard'; }
            else if (view === 'DASHBOARD') { projectView = 'DASHBOARD'; branchTab = 'dashboard'; knowledgeMode = false; }
            else { knowledgeMode = false; projectView = domain; branchTab = branchId || 'dashboard'; }
        });
    }
    function loadDocuments() {
        if (!projectId || !branchId) return;
        requestAll('documents', 'qroc-document-list', {project_id: projectId, branch_id: branchId}, 'documents', function(result, ok) {
            if (!ok) return;
            documents = result.documents || [];
            var allowed = domainDocuments[domain] || [];
            if (!documentType || allowed.indexOf(documentType) < 0) documentType = allowed.length ? allowed[0] : '';
            selectDocument(documentType);
        });
    }
    function selectDocument(type) {
        root.persistDraft();
        draftLoaded=false;
        let epoch=++documentEpoch;
        documentType = type || '';
        currentDocument = latestFor(type);
        if(currentDocument && currentDocument.generation_finish_reason && currentDocument.generation_finish_reason!=='stop')noticeText='Partial draft (stopped: '+currentDocument.generation_finish_reason+')';
        draftBaseRevision = currentDocument ? currentDocument.revision : 0;
        draftText = currentDocument ? currentDocument.content : '';
        gate = null; gateDocumentId = '';
        history = [];
        if(projectId&&branchId&&documentType) Backend.send('qroc-draft-get',{project_id:projectId,branch_id:branchId,document_type:documentType},(r,ok,error)=>{if(!root||epoch!==root.documentEpoch)return;if(!ok){root.errorText=error||'Draft could not be restored';root.draftLoaded=false;return;}if(ok&&r.content!==undefined&&r.content!==null){root.draftText=r.content;root.draftBaseRevision=r.base_revision||0;if(root.draftBaseRevision!==(root.currentDocument?root.currentDocument.revision:0))root.noticeText='Restored draft is based on an older revision';}root.draftLoaded=true;},true);
        if (currentDocument)
            requestAll('history', 'qroc-document-history', {project_id: projectId, branch_id: branchId, document_type: type}, 'items', function(result, ok) { if (ok && epoch === documentEpoch) history = result.items || []; });
        if (currentDocument && type.indexOf('GATE_') === 0) evaluateGate(currentDocument,epoch);
    }
    function evaluateGate(doc,epoch) {
        gate=null;gateDocumentId='';
        if(doc.document_type==='GATE_4'&&authoring.semanticDeclared('GATE_4')&&!authoring.semanticSupported('GATE_4')){
            errorText='Market review not available';return;
        }
        var command=doc.document_type==='GATE_4'&&authoring.semanticSupported('GATE_4')?'qroc-gate-semantic-evaluate':'qroc-gate-evaluate';
        request('gate',command,{project_id:projectId,branch_id:branchId,document_type:doc.document_type,revision:doc.revision},function(r,ok){
            if(epoch!==documentEpoch||!currentDocument||currentDocument.id!==doc.id)return;
            if(ok){gate=r;gateDocumentId=doc.id;}
        });
    }
    function saveProject() {
        if (!project) return;
        var source = project.sources || [];
        request('save-project', 'qroc-project-save', {id: projectId, title: project.title, brain_dump: project.brain_dump, sources: source}, function(result, ok) { if (ok) { project = result; noticeText = 'Project saved'; loadProjects(); } });
    }
    function createProject() {
        if (newProjectKind === 'media') { createMediaProject(); return; }
        request('create', 'qroc-project-create', {title: newProjectTitle, brain_dump: newProjectBrainDump}, function(result, ok) {
            if (!ok) return;
            projectId=result.id;newProjectDialog.close();newProjectTitle = ''; newProjectBrainDump = ''; noticeText = 'Project created'; openViewOnLoad = 'DOMAIN'; loadProjects();
        });
    }
    function createMediaProject() {
        var args = {title: newProjectTitle, brief: newProjectBrainDump};
        if (newMediaLyrics.trim()) args.lyrics = newMediaLyrics;
        if (newMediaConcepts.trim()) args.concepts = newMediaConcepts;
        var link = newMediaLink;
        transport.send('qroc-media-create', args, function(result, ok, error) {
            if (!ok) { noticeText = ''; errorText = mediaView.refusal(error) || ('QROC could not do that.\n' + mediaView.plain(error)); return; }
            errorText = '';
            projectId = result.project.id; newProjectDialog.close();
            newProjectTitle = ''; newProjectBrainDump = ''; newMediaLyrics = ''; newMediaConcepts = ''; newProjectKind = 'product'; newMediaLink = '';
            noticeText = 'Project created'; openViewOnLoad = 'keep';
            if (!link) { loadProjects(); return; }
            // The media command makes the project; the link is saved by the engine's own link command right after.
            transport.send('qroc-project-link', {project_id: result.project.id, linked_project_id: link}, function(r, linked) {
                if (!linked) errorText = 'The project was made, but it could not be linked; choose the product project again from its page';
                loadProjects();
            });
        });
    }
    function addBranch() {
        if (!newBranchAuthoringOnly && (!newBranchModel.trim() || !newBranchProvider.trim())) { errorText = 'Choose an explicit model and provider for this branch'; return; }
        var args={project_id:projectId,name:newBranchName};
        if (!newBranchAuthoringOnly) { args.model=newBranchModel;args.provider=newBranchProvider; }
        else if (!authoring.registryAvailable) { args.model='authoring-only';args.provider='authoring-only'; } // Explicit metadata prevents historical foundation defaults; never a model choice.
        request('branch', 'qroc-branch-create', args, function(result, ok) {
            if (!ok) return;
            branchId=result.id;branchTab=result.id;newBranchDialog.close();newBranchName = ''; loadProject(projectId,'keep'); noticeText = 'Branch created';
        });
    }
    function saveDocument() {
        if (!projectId || !branchId || !documentType || !draftText.trim()) { errorText = 'A non-empty draft is required'; return; }
        request('save-document', 'qroc-document-save', {project_id: projectId, branch_id: branchId, document_type: documentType, content: draftText, source_revision: draftBaseRevision}, function(result, ok) {
            if (!ok) return;
            noticeText = 'Saved revision ' + result.revision; currentDocument = result; draftText = result.content;draftLoaded=false;Backend.send('qroc-draft-clear',{project_id:projectId,branch_id:branchId,document_type:documentType},()=>{},true);loadDocuments(); loadAllDocuments(); documentChanged(result);
        });
    }
    function generate() {
        if (!explicitModel) { errorText = 'Choose a branch that uses the configured model'; return; }
        if (!authoring.generationAllowed(documentType)) { errorText = 'Save a branch scope first'; return; }
        if (!projectId || !branchId || !documentType) return;
        request('generate', 'qroc-generate-start', {project_id: projectId, branch_id: branchId, document_type: documentType, context: project ? project.brain_dump : ''}, function(result, ok) { if (ok) { noticeText = 'Generation started'; jobId = result.id; jobTimer.start(); } });
    }
    function selectHistory(index) {
        if (!history[index]) return;
        persistDraft(); draftLoaded = false; documentEpoch++;
        currentDocument = history[index]; draftText = currentDocument.content; draftBaseRevision = currentDocument.revision;
        gate = null; gateDocumentId = ''; draftLoaded = true;
        noticeText = 'Viewing revision '+currentDocument.revision;
    }
    function runAnalysis() {
        analysisRows = [];
        request('analysis', 'qroc-analysis', {project_id: projectId, document_type: documentType}, function(result, ok) { if (ok) analysisRows = result.items || []; });
    }
    function chooseSource() { sourceDialog.open(); }
    function attachSource(path) {
        if (!project || !path) return;
        var parts = path.split('/');
        var next = (project.sources || []).slice();
        next.push({id: 'source-' + Date.now(), name: parts[parts.length - 1], path: path, sha256: '', size: 0});
        project.sources = next; sourcePath = path; saveProject();
    }
    function saveKnowledge() {
        if (!projectId || !kbTitle.trim() || !kbContent.trim()) { errorText = 'Knowledge item title and content are required'; return; }
        request('kb-save', 'qroc-kb-save', {project_id: projectId, title: kbTitle, kind: kbKind, content: kbContent, selected: true}, function(result, ok) {
            if (!ok) return;
            knowledgeDialog.close();kbTitle = ''; kbContent = ''; noticeText = 'Knowledge item saved'; loadProject(projectId,'keep');
        });
    }
    property string jobId: ''
    property var batchJobIds: []
    property Timer jobTimer: Timer {
        interval: 900
        repeat: true
        onTriggered: {
            if (root.batchActive)
                root.requestAll('job-list', 'qroc-job-list', {project_id: root.projectId}, 'jobs', function(result, ok) {
                    if (!ok) return;
                    root.jobs = result.jobs || [];
                    var batchJobs = root.jobs.filter(job => root.batchJobIds.indexOf(job.id) >= 0);
                    var active = batchJobs.some(function(job) { return ['running','queued','cancelling'].indexOf(job.state) >= 0; });
                    if (!active) {
                        root.batchActive = false;
                        var stopped = batchJobs.find(job => job.state !== 'succeeded');
                        root.noticeText = stopped ? ('Generation stopped: '+(stopped.error || stopped.state)+' · finished documents kept') : 'All documents generated';
                        root.loadDocuments();root.loadAllDocuments();
                    }
                });
            if (!root.jobId && !root.batchActive) { stop(); return; }
            if (!root.jobId) return;
            root.request('job', 'qroc-job-status', {id: root.jobId}, function(result, ok) {
                if (!ok || !result) return;
                if (['running','queued','cancelling'].indexOf(result.state) < 0) {
                    stop(); noticeText = result.state === 'succeeded' ? 'Draft generated' : ('Generation ' + result.state + (result.error ? ': ' + result.error : '')); root.jobId = ''; loadDocuments(); loadAllDocuments();
                }
            });
        }
    }
    FileDialog {
        id: sourceDialog
        title: 'Attach a project source'
        fileMode: FileDialog.OpenFile
        onAccepted: root.attachSource(root.pathFromUrl(selectedFile))
    }

    Component.onCompleted: loadProjects()

    function persistDraft() {
        draftTimer.stop();
        if(!draftLoaded||!projectId||!branchId||!documentType)return;
        if(draftText===(currentDocument?currentDocument.content:''))return;
        Backend.send('qroc-draft-save',{project_id:projectId,branch_id:branchId,document_type:documentType,content:draftText,base_revision:draftBaseRevision},(r,ok,error)=>{if(!ok)root.errorText=error||'Draft preservation failed; keep this text before switching.';},true);
    }
    onDraftTextChanged: if(draftLoaded)draftTimer.restart()
    Timer {id:draftTimer;interval:700;onTriggered:root.persistDraft()}
    Component.onDestruction:root.persistDraft()
    function toggleKnowledge(item) {
        request('kb-toggle','qroc-kb-save',{project_id:projectId,id:item.id,title:item.title,content:item.content,kind:item.kind,selected:!item.selected},(r,ok)=>{if(ok)loadProject(projectId,'keep');});
    }
    function showContext() {request('context',branchId?'qroc-context-preview':'qroc-kb-context',{project_id:projectId,branch_id:branchId,document_type:documentType},(r,ok)=>{if(ok){contextAudit=r;contextDialog.open();}});}
    function synthesize() {
        if (!explicitModel || !authoring.generationAllowed(documentType)) { errorText = 'Choose a scoped document and a configured branch'; return; }
        synthesisStarting=true;
        let sourceIds=synthesisSources.slice(), destinationName=synthesisName, projectKey=projectId, type=documentType;
        request('synthesis-branch','qroc-branch-create',{project_id:projectKey,name:destinationName,model:activeBranch.model,provider:activeBranch.provider},(branch,ok)=>{
            if(!ok){synthesisStarting=false;return;}
            request('synthesis','qroc-synthesis-start',{project_id:projectKey,target_branch_id:branch.id,source_branch_ids:sourceIds,document_type:type},(job,started)=>{
                synthesisStarting=false;
                if(!started)return;
                persistDraft();draftLoaded=false;branchId=branch.id;jobId=job.id;jobTimer.start();synthesisDialog.close();loadProject(projectKey,'keep');noticeText='Synthesizing into '+destinationName;
            });
        });
    }
    function titleFor(type) {var labels={PRD:'PRD',UI_UX_SPEC:'UI / UX specification',PRE_MORTEM:'Pre-mortem'}; if(labels[type])return labels[type]; return String(type||'').toLowerCase().split('_').map(w=>w.charAt(0).toUpperCase()+w.slice(1)).join(' ');}
    readonly property int selectedHistoryIndex: currentDocument ? history.findIndex(d=>d.id===currentDocument.id&&d.revision===currentDocument.revision&&d.sha256===currentDocument.sha256&&d.project_id===currentDocument.project_id&&d.branch_id===currentDocument.branch_id) : -1
    readonly property string historySelectionText: selectedHistoryIndex>=0 ? 'Revision '+history[selectedHistoryIndex].revision : !currentDocument ? 'No saved revision selected' : history.length ? 'No matching saved revision' : 'Loading saved revisions…'
    readonly property bool compactWorkspace: width < 600 || height < 680
    readonly property bool landing: !project && !loading
    readonly property bool domainView: !!project && !isMedia && projectView !== 'DASHBOARD' && projectView !== 'PIPELINE' && projectView !== 'KB'
    readonly property bool headerActions: !!project && !compactWorkspace && (domainView || projectView === 'KB')
    readonly property bool canSynthesize: explicitModel && authoring.generationAllowed(documentType) && !jobId && !batchActive
    readonly property bool providerReady: !!authoring.provider && authoring.provider.available === true
    readonly property var branchTabs: [{id: 'dashboard', name: 'Dashboard'}].concat(branches)
    property bool advancedBranchFields: false

    component StatusPills: Flow {
        spacing: Theme.s2
        Pill { text: root.explicitModel ? 'Model: ' + Theme.modelLabel(root.activeBranch.model) : 'Generation off'; neutral: !root.explicitModel; tone: Theme.success }
        Pill { text: authoring.savedScope ? 'Scope · revision ' + authoring.savedScope.revision : 'No scope'; neutral: !authoring.savedScope; tone: Theme.accentPrimary }
        Pill { visible: !!root.activeBranch && !root.explicitModel; text: 'Authoring only'; outline: true }
    }
    // The Brain Dump field's look at 120 px, with its drawn placeholder (the stock one is hidden there too).
    component MediaTextArea: STextArea {
        id: area
        property string hint: ''
        Layout.fillWidth: true; Layout.preferredHeight: 120
        placeholderTextColor: 'transparent'; wrapMode: TextEdit.Wrap
        Text {
            visible: !area.text && !area.preeditText
            x: area.leftPadding; y: area.topPadding; width: area.width - area.leftPadding - area.rightPadding
            text: area.hint; wrapMode: Text.Wrap; font: area.font; color: Theme.textMuted
        }
    }
    component FieldLabel: Text {
        font.family: Theme.font; font.pixelSize: Theme.fsSm; font.weight: Font.Medium; color: Theme.textPrimary
    }
    // Selectable row of the New branch modal (ModelListItem look): icon tile, title, caption, selection box.
    component ChoiceRow: Rectangle {
        id: choiceRow
        property string icon: 'cpu'
        property string title: ''
        property string caption: ''
        property bool selected: false
        property bool available: true
        default property alias trailing: trailingRow.data
        signal picked()
        Layout.fillWidth: true
        implicitHeight: rowContent.implicitHeight + Theme.s3 * 2
        radius: Theme.radius
        color: selected ? Qt.alpha(Theme.accentPrimary, 0.10) : rowHover.hovered && available ? Theme.bgTertiary : 'transparent'
        border.width: Theme.borderWidth
        border.color: selected ? Theme.accentPrimary : Theme.border
        opacity: available ? 1 : 0.6
        HoverHandler { id: rowHover }
        MouseArea { anchors.fill: parent; enabled: choiceRow.available; cursorShape: Qt.PointingHandCursor; onClicked: choiceRow.picked() }
        RowLayout {
            id: rowContent
            anchors.fill: parent
            anchors.leftMargin: Theme.s3; anchors.rightMargin: Theme.s3; anchors.topMargin: Theme.s3; anchors.bottomMargin: Theme.s3
            spacing: Theme.s3
            IconTile { icon: choiceRow.icon; size: 32; iconSize: Theme.icon; tileRadius: Theme.radiusMd; tone: choiceRow.selected ? Theme.accentPrimary : Theme.textSecondary }
            ColumnLayout {
                Layout.fillWidth: true
                spacing: 2
                Text { Layout.fillWidth: true; text: choiceRow.title; font.family: Theme.font; font.pixelSize: Theme.fsSm; font.weight: Font.Medium; color: Theme.textPrimary; elide: Text.ElideRight }
                Text { Layout.fillWidth: true; text: choiceRow.caption; visible: !!text; font.family: Theme.font; font.pixelSize: Theme.fsXs; color: Theme.textSecondary; elide: Text.ElideRight }
            }
            RowLayout { id: trailingRow; spacing: Theme.s2 }
        }
    }

    ColumnLayout {
        anchors.fill: parent
        spacing: 0
        AppHeader {
            Layout.fillWidth: true
            title: root.project ? root.project.title : root.landing ? 'Recent Projects' : ''
            large: !root.project
            RoutePicker {
                visible: !!root.project && !root.isMedia
                routes: root.routes
                onRefreshRequested: root.loadRoutes()
                onChosen: name => root.selectRoute(name)
                onEmptyClicked: root.settingsRequested()
            }
            SButton { visible: !!root.project && !root.compactWorkspace && root.projectView === 'PIPELINE'; text: 'Hardening'; iconName: 'shield'; quiet: true; implicitHeight: Theme.controlHeightSm; onClicked: root.startHardening() }
            SButton { visible: !!root.project && !root.compactWorkspace && root.projectView === 'PIPELINE'; text: 'Export'; iconName: 'download'; quiet: true; implicitHeight: Theme.controlHeightSm; onClicked: root.exportProject() }
            StatusPills { visible: root.headerActions && root.width >= Theme.breakpointXl - Theme.s6 * 6 }
            SButton { objectName: 'qroc-scope-open'; visible: root.headerActions; text: 'Scope'; iconName: 'filter'; quiet: true; implicitHeight: Theme.controlHeightSm; onClicked: root.openScope() }
            SButton { visible: root.headerActions; text: 'Export'; iconName: 'download'; quiet: true; implicitHeight: Theme.controlHeightSm; enabled: !!root.projectId; onClicked: root.exportProject() }
            SButton { objectName: 'qroc-compare'; visible: root.headerActions; text: 'Analyze'; iconName: 'chart'; quiet: true; implicitHeight: Theme.controlHeightSm; enabled: !!root.documentType; onClicked: root.openCompare() }
            SButton { visible: root.headerActions; text: 'Audit'; iconName: 'search'; quiet: true; implicitHeight: Theme.controlHeightSm; onClicked: root.openContext() }
            SButton { objectName: 'qroc-workspace-details-open'; visible: root.compactWorkspace && !!root.project && !root.isMedia; text: 'Project & branch'; quiet: true; implicitHeight: Theme.controlHeightSm; onClicked: workspaceDetails.open() }
            SButton { id: projectMenuButton; visible: !!root.project && !root.isMedia; text: ''; iconName: 'more-v'; quiet: true; implicitHeight: Theme.controlHeightSm; accessibleLabel: 'More actions'; onClicked: projectMenu.open() }
        }
        NoticePopup { scope: root; text: root.errorText || authoring.errorText || root.noticeText; error: !!root.errorText || !!authoring.errorText }
        DomainTabBar { Layout.fillWidth: true; visible: !!root.project; workspace: root }
        BranchTabBar { Layout.fillWidth: true; visible: root.domainView && !root.compactWorkspace; workspace: root }
        RowLayout {
            visible: root.domainView && root.compactWorkspace
            Layout.fillWidth: true; Layout.margins: Theme.s2
            spacing: Theme.s2
            Choice {
                Layout.fillWidth: true
                model: root.branchTabs; textRole: 'name'
                currentIndex: Math.max(0, root.branchTabs.findIndex(b => b.id === root.branchTab))
                onActivated: root.selectBranchTab(model[currentIndex].id)
                Accessible.name: 'Document branch'
            }
            SButton { text: ''; iconName: 'plus'; accessibleLabel: 'Add branch'; onClicked: root.openNewBranch() }
        }
        Item {
            Layout.fillWidth: true; Layout.fillHeight: true; Layout.minimumHeight: 0
            LandingView { anchors.fill: parent; visible: root.landing; workspace: root }
            ProjectDashboard { anchors.fill: parent; visible: !!root.project && !root.isMedia && root.projectView === 'DASHBOARD'; workspace: root }
            MediaView { id: mediaView; anchors.fill: parent; visible: root.isMedia; workspace: root }
            PipelineView { anchors.fill: parent; visible: !!root.project && root.projectView === 'PIPELINE'; workspace: root }
            KnowledgeView {
                anchors.fill: parent; anchors.margins: root.compactWorkspace ? Theme.s2 : Theme.s6
                visible: !!root.project && root.projectView === 'KB'
                workspace: root
            }
            EmptyState {
                anchors.fill: parent
                visible: root.domainView && root.branches.length === 0
                icon: 'layers'; tone: Theme.domainColor(root.domain)
                eyebrow: Theme.domainLabel(root.domain) + ' documents'
                title: 'No branches yet'
                detail: (root.referenceDocuments[root.domain] || []).length + ' documents, ' + (root.referenceDocuments[root.domain] || []).filter(root.isGateType).length + ' gates'
                facts: [{icon: 'git-branch', label: 'Branch comparison'}, {icon: 'layers', label: 'Document sequence'}]
                actionText: 'Add branch'
                onActionRequested: root.openNewBranch()
            }
            DomainOverview { anchors.fill: parent; visible: root.domainView && root.branches.length > 0 && root.branchTab === 'dashboard'; workspace: root }
            DocumentWorkbench {
                anchors.fill: parent; anchors.margins: root.compactWorkspace ? Theme.s2 : Theme.s4
                visible: root.domainView && root.branches.length > 0 && root.branchTab !== 'dashboard'
                workspace: root
            }
        }
    }

    // The project header's three-dot menu.
    SPopup {
        id: projectMenu
        objectName: 'qroc-project-menu'
        parent: Overlay.overlay
        trigger: projectMenuButton; alignRight: true
        padding: Theme.s1
        width: 160
        background: Rectangle { color: Theme.bgSecondary; border.width: Theme.borderWidth; border.color: Theme.border; radius: Theme.radius }
        contentItem: ColumnLayout {
            spacing: 0
            Rectangle {
                Layout.fillWidth: true; implicitHeight: Theme.controlHeightSm
                radius: Theme.radiusSm
                color: openFolderHover.hovered ? Theme.bgTertiary : 'transparent'
                HoverHandler { id: openFolderHover; cursorShape: Qt.PointingHandCursor }
                TapHandler { onTapped: { projectMenu.close(); root.openProjectFolder(); } }
                Accessible.role: Accessible.MenuItem
                Accessible.name: 'Open folder'
                RowLayout {
                    anchors.fill: parent; anchors.leftMargin: Theme.s3; anchors.rightMargin: Theme.s3
                    spacing: Theme.s2
                    SIcon { name: 'folder'; size: Theme.iconSm; color: Theme.textPrimary }
                    Text { Layout.fillWidth: true; text: 'Open folder'; font.family: Theme.font; font.pixelSize: Theme.fsSm; color: Theme.textPrimary }
                }
            }
        }
    }

    ModalFrame {
        id: workspaceDetails
        objectName: 'qroc-workspace-details'
        title: 'Project & branch'
        maxWidth: Theme.modalLg
        bodySpacing: Theme.s3
        Text { Layout.fillWidth: true; text: root.project ? root.project.title : ''; font.family: Theme.font; font.pixelSize: Theme.fsBase; font.weight: Font.DemiBold; color: Theme.textPrimary; wrapMode: Text.Wrap }
        Choice { Layout.fillWidth: true; model: root.projects; textRole: 'title'; currentIndex: Math.max(0, root.projects.findIndex(p => p.id === root.projectId)); onActivated: { workspaceDetails.close(); root.showProject(model[currentIndex].id); } Accessible.name: 'QROC project' }
        Flow {
            Layout.fillWidth: true; spacing: Theme.s2
            SButton { text: 'Import'; onClicked: { workspaceDetails.close(); root.openImportProject(); } }
            SButton { text: 'New Project'; onClicked: { workspaceDetails.close(); root.openNewProject(); } }
            SButton { text: 'Export'; enabled: !!root.projectId; onClicked: { workspaceDetails.close(); root.exportProject(); } }
        }
        StatusPills { Layout.fillWidth: true }
        Flow {
            Layout.fillWidth: true; spacing: Theme.s2
            SButton { text: 'Dashboard'; onClicked: { workspaceDetails.close(); root.showDashboard(); } }
            SButton { text: 'KB'; onClicked: { workspaceDetails.close(); root.showKnowledge(); } }
            SButton { text: 'Audit'; onClicked: { workspaceDetails.close(); root.openContext(); } }
            SButton { text: 'Scope'; onClicked: { workspaceDetails.close(); root.openScope(); } }
        }
        Choice {
            Layout.fillWidth: true
            model: root.branches; textRole: 'name'
            currentIndex: Math.max(0, root.branches.findIndex(b => b.id === root.branchId))
            onActivated: { workspaceDetails.close(); root.selectBranchTab(model[currentIndex].id); }
            Accessible.name: 'Document branch'
        }
        Flow {
            Layout.fillWidth: true; spacing: Theme.s2
            SButton { text: 'New branch'; onClicked: { workspaceDetails.close(); root.openNewBranch(); } }
            SButton { text: 'Compare'; enabled: !!root.documentType; onClicked: { workspaceDetails.close(); root.openCompare(); } }
            SButton { text: 'Synthesize'; enabled: root.canSynthesize; onClicked: { workspaceDetails.close(); root.openSynthesis(); } }
        }
    }

    ModalFrame {
        id: documentMetadata
        objectName: 'qroc-document-metadata'
        title: root.titleFor(root.documentType) || 'Details'
        maxWidth: Theme.modal2xl
        bodySpacing: Theme.s3
        Text { objectName: 'qroc-history-caption'; Layout.fillWidth: true; text: (root.currentDocument ? 'Revision ' + root.currentDocument.revision : 'First draft') + ' · ' + Theme.domainLabel(root.domainOfType(root.documentType) || root.domain); font.family: Theme.font; font.pixelSize: Theme.fsSm; color: Theme.textSecondary; wrapMode: Text.Wrap }
        Flow {
            Layout.fillWidth: true; spacing: Theme.s2
            Pill { text: root.reviewStanding; neutral: root.exactSavedRevision; tone: Theme.danger }
        }
        Text { objectName: 'qroc-policy-metadata'; Layout.fillWidth: true; text: authoring.typeProfile ? (authoring.typeProfile.input_policy.allowed_upstream.length ? 'Built from ' + authoring.typeProfile.input_policy.allowed_upstream.map(t => root.titleFor(t)).join(', ') : 'Built from the idea') : ''; font.family: Theme.font; font.pixelSize: Theme.fsXs; color: Theme.textSecondary; wrapMode: Text.Wrap }
        Repeater { model: root.project ? root.project.sources || [] : []; delegate: Text { required property var modelData; Layout.fillWidth: true; text: modelData.name; font.family: Theme.font; font.pixelSize: Theme.fsSm; color: Theme.textMuted; wrapMode: Text.Wrap } }
        SButton { objectName: 'qroc-history-latest'; text: 'Latest saved'; enabled: !!root.currentDocument && !root.exactSavedRevision; onClicked: root.selectDocument(root.documentType) }
        Choice { objectName: 'qroc-history-metadata'; Layout.fillWidth: true; enabled: root.history.length > 0; model: root.history.map(d => 'Revision ' + d.revision); currentIndex: root.selectedHistoryIndex; displayText: root.historySelectionText; onActivated: { root.persistDraft(); root.selectHistory(currentIndex); } Accessible.name: 'Saved revision' }
    }

    ModalFrame {
        id: scopeDialog
        objectName: 'qroc-scope-dialog'
        title: 'Branch scope'
        icon: 'filter'
        maxWidth: Theme.modal2xl
        bodySpacing: 0
        ScopeEditor { Layout.fillWidth: true; Layout.preferredHeight: Math.max(320, root.height - 220); controller: authoring }
    }

    ModalFrame {
        id: synthesisDialog
        title: 'Synthesize branch evidence'
        icon: 'git-branch'
        maxWidth: Theme.modalXl
        bodySpacing: Theme.s3
        Text { Layout.fillWidth: true; text: 'Generate ' + root.titleFor(root.documentType) + ' in a new branch from the selected branches.'; font.family: Theme.font; font.pixelSize: Theme.fsSm; color: Theme.textSecondary; wrapMode: Text.Wrap }
        Field { Layout.fillWidth: true; text: root.synthesisName; onTextEdited: root.synthesisName = text; placeholderText: 'Destination branch name'; Accessible.name: 'Synthesis branch name' }
        Repeater {
            model: root.branches
            delegate: SCheckBox {
                required property var modelData
                Layout.fillWidth: true
                text: modelData.name
                checked: root.synthesisSources.indexOf(modelData.id) >= 0
                onToggled: { let next = root.synthesisSources.filter(id => id !== modelData.id); if (checked) next.push(modelData.id); root.synthesisSources = next; }
            }
        }
        footer: [
            SButton { text: 'Cancel'; quiet: true; enabled: !root.synthesisStarting; onClicked: synthesisDialog.close() },
            SButton { busy: root.synthesisStarting; text: root.synthesisStarting ? 'Starting…' : 'Create synthesis'; primary: true; enabled: !root.synthesisStarting && root.synthesisSources.length > 0 && !!root.synthesisName.trim(); onClicked: root.synthesize() }
        ]
    }

    ModalFrame {
        id: newProjectDialog
        objectName: 'qroc-create-dialog'
        title: 'New Project'
        maxWidth: Theme.modal2xl
        bodySpacing: Theme.s6
        ColumnLayout {
            Layout.fillWidth: true; spacing: Theme.s2
            ChoiceRow {
                objectName: 'qroc-create-kind-product'
                icon: 'box'; title: 'Product'; caption: 'The idea, hardened and produced through the four domains.'
                selected: root.newProjectKind === 'product'
                onPicked: root.newProjectKind = 'product'
            }
            ChoiceRow {
                objectName: 'qroc-create-kind-media'
                icon: 'mic'; title: 'Media'; caption: 'A track or a piece: brief, lyrics, concepts and versions.'
                selected: root.newProjectKind === 'media'
                onPicked: root.newProjectKind = 'media'
            }
        }
        ColumnLayout {
            Layout.fillWidth: true; spacing: Theme.s2
            FieldLabel { text: 'Project Name' }
            Field { Layout.fillWidth: true; implicitHeight: Theme.controlHeightLg; objectName: 'qroc-create-title'; placeholderText: 'e.g., AI-Powered Task Manager'; text: root.newProjectTitle; onTextEdited: root.newProjectTitle = text; Accessible.name: 'New project name' }
        }
        ColumnLayout {
            Layout.fillWidth: true; spacing: Theme.s2
            FieldLabel { text: root.newProjectKind === 'media' ? 'Brief' : 'Brain Dump' }
            STextArea {
                id: ideaField
                Layout.fillWidth: true; Layout.preferredHeight: 264
                objectName: 'qroc-create-idea'
                placeholderText: root.newProjectKind === 'media' ? 'What this track or piece is, and what it is for.' : 'Describe your idea in detail. What problem does it solve? Who is it for? What are the key features?'
                placeholderTextColor: 'transparent'
                text: root.newProjectBrainDump; onTextChanged: root.newProjectBrainDump = text; wrapMode: TextEdit.Wrap
                Text {
                    visible: !ideaField.text && !ideaField.preeditText
                    x: ideaField.leftPadding; y: ideaField.topPadding; width: ideaField.width - ideaField.leftPadding - ideaField.rightPadding
                    text: ideaField.placeholderText; wrapMode: Text.Wrap
                    font: ideaField.font; color: Theme.textMuted
                }
            }
            Text { visible: root.newProjectKind !== 'media'; Layout.fillWidth: true; text: 'Tip: Be specific about the problem, target users, and desired outcomes. The more detail, the better the generated documentation.'; font.family: Theme.font; font.pixelSize: Theme.fsXs; color: Theme.textSecondary; wrapMode: Text.Wrap }
        }
        ColumnLayout {
            visible: root.newProjectKind === 'media'
            Layout.fillWidth: true; spacing: Theme.s2
            FieldLabel { text: 'Follows' }
            Choice {
                objectName: 'qroc-create-follows'
                Layout.fillWidth: true
                model: ['On its own'].concat(root.productProjects.map(p => p.title))
                currentIndex: root.newMediaLink ? root.productProjects.findIndex(p => p.id === root.newMediaLink) + 1 : 0
                onActivated: index => root.newMediaLink = index > 0 ? root.productProjects[index - 1].id : ''
                Accessible.name: 'Follows a product project'
            }
        }
        ColumnLayout {
            visible: root.newProjectKind === 'media'
            Layout.fillWidth: true; spacing: Theme.s2
            FieldLabel { text: 'Lyrics' }
            MediaTextArea { objectName: 'qroc-create-lyrics'; hint: 'Lyrics, if any yet.'; text: root.newMediaLyrics; onTextChanged: root.newMediaLyrics = text }
        }
        ColumnLayout {
            visible: root.newProjectKind === 'media'
            Layout.fillWidth: true; spacing: Theme.s2
            FieldLabel { text: 'Concepts' }
            MediaTextArea { objectName: 'qroc-create-concepts'; hint: 'Concepts, references and directions, if any yet.'; text: root.newMediaConcepts; onTextChanged: root.newMediaConcepts = text }
        }
        footer: [
            SButton { text: 'Cancel'; quiet: true; onClicked: newProjectDialog.close() },
            SButton { objectName: 'qroc-create-submit'; text: 'Create Project'; primary: true; enabled: !!root.newProjectTitle.trim() && !!root.newProjectBrainDump.trim(); onClicked: root.createProject() }
        ]
    }

    ModalFrame {
        id: newBranchDialog
        title: 'New branch'
        subtitle: 'Name the branch and choose how it generates'
        icon: 'cpu'
        maxWidth: Theme.modalLg
        bodySpacing: Theme.s3
        Field { Layout.fillWidth: true; implicitHeight: Theme.controlHeightLg; objectName: 'qroc-branch-name'; placeholderText: 'Branch name'; text: root.newBranchName; onTextEdited: root.newBranchName = text; Accessible.name: 'Branch name' }
        ChoiceRow {
            id: providerRow
            icon: 'cpu'
            title: 'Configured model · ' + (root.providerReady ? Theme.modelLabel(authoring.provider.model) : 'none')
            caption: root.providerReady ? 'Runs on this computer' : ''
            available: root.providerReady
            selected: root.providerReady && !root.newBranchAuthoringOnly && root.newBranchModel === authoring.provider.model && root.newBranchProvider === authoring.provider.provider
            onPicked: { root.newBranchAuthoringOnly = false; root.newBranchModel = authoring.provider.model; root.newBranchProvider = authoring.provider.provider; }
            Pill { visible: !root.providerReady; text: 'Unavailable'; neutral: true }
            SCheckBox { visible: root.providerReady; text: ''; checked: providerRow.selected; Accessible.name: 'Use configured provider'; onToggled: providerRow.picked() }
        }
        ChoiceRow {
            icon: 'edit'
            title: 'Authoring only'
            caption: 'Write and save documents without generating'
            selected: root.newBranchAuthoringOnly
            onPicked: root.newBranchAuthoringOnly = !root.newBranchAuthoringOnly
            SCheckBox { objectName: 'qroc-branch-authoring-only'; text: ''; checked: root.newBranchAuthoringOnly; onToggled: root.newBranchAuthoringOnly = checked; Accessible.name: 'Authoring only' }
        }
        ChoiceRow {
            icon: 'gear'
            title: 'Advanced'
            caption: 'Explicit model and provider'
            selected: false
            onPicked: root.advancedBranchFields = !root.advancedBranchFields
            SIcon { name: root.advancedBranchFields || !root.providerReady ? 'down' : 'right'; size: Theme.icon; color: Theme.textMuted }
        }
        ColumnLayout {
            visible: root.advancedBranchFields || !root.providerReady
            Layout.fillWidth: true; spacing: Theme.s2
            Field { enabled: !root.newBranchAuthoringOnly; Layout.fillWidth: true; objectName: 'qroc-branch-model'; placeholderText: 'Model'; text: root.newBranchModel; onTextEdited: root.newBranchModel = text; Accessible.name: 'Branch model' }
            Field { enabled: !root.newBranchAuthoringOnly; Layout.fillWidth: true; objectName: 'qroc-branch-provider'; placeholderText: 'Provider'; text: root.newBranchProvider; onTextEdited: root.newBranchProvider = text; Accessible.name: 'Branch provider' }
        }
        footer: [
            SButton { text: 'Cancel'; quiet: true; onClicked: newBranchDialog.close() },
            SButton { objectName: 'qroc-branch-submit'; text: 'Create branch'; primary: true; enabled: !!root.newBranchName.trim() && (root.newBranchAuthoringOnly || (!!root.newBranchModel.trim() && !!root.newBranchProvider.trim())); onClicked: root.addBranch() }
        ]
    }

    ModalFrame {
        id: knowledgeDialog
        title: 'Knowledge note'
        icon: 'book'
        maxWidth: Theme.modalXl
        bodySpacing: Theme.s3
        Field { Layout.fillWidth: true; placeholderText: 'Note title'; text: root.kbTitle; onTextEdited: root.kbTitle = text }
        STextArea { Layout.fillWidth: true; Layout.preferredHeight: 240; placeholderText: 'Evidence, requirements, decisions or source notes…'; text: root.kbContent; onTextChanged: root.kbContent = text; wrapMode: TextEdit.Wrap }
        footer: [
            SButton { text: 'Cancel'; quiet: true; onClicked: knowledgeDialog.close() },
            SButton { text: 'Add note'; primary: true; onClicked: root.saveKnowledge() }
        ]
    }

    ModalFrame {
        id: projectNotesDialog
        title: 'Project notes'
        icon: 'notes'
        maxWidth: Theme.modalXl
        STextArea { id: brainEditor; Layout.fillWidth: true; Layout.preferredHeight: 288; text: root.project ? root.project.brain_dump : ''; wrapMode: TextEdit.Wrap }
        footer: [
            SButton { text: 'Cancel'; quiet: true; onClicked: projectNotesDialog.close() },
            SButton { text: 'Save notes'; primary: true; onClicked: { root.project.brain_dump = brainEditor.text; root.saveProject(); projectNotesDialog.close(); } }
        ]
    }

    ModalFrame {
        id: contextDialog
        title: 'Context'
        icon: 'search'
        maxWidth: Theme.modal2xl
        bodySpacing: Theme.s3
        STextArea { Layout.fillWidth: true; Layout.preferredHeight: 320; text: root.contextAudit.context || 'No selected knowledge items'; readOnly: true; wrapMode: TextEdit.Wrap }
    }

    ModalFrame {
        id: comparisonDialog
        objectName: 'qroc-comparison'
        title: 'Compare branches'
        subtitle: root.titleFor(root.documentType)
        icon: 'chart'
        maxWidth: Theme.modalWide
        GridLayout {
            Layout.fillWidth: true
            columns: comparisonDialog.width >= 640 ? 2 : 1
            columnSpacing: Theme.s4; rowSpacing: Theme.s4
            Repeater {
                model: root.analysisRows
                delegate: Card {
                    required property var modelData
                    Layout.fillWidth: true
                    Layout.alignment: Qt.AlignTop
                    nested: true
                    spacing: Theme.s2
                    Text { Layout.fillWidth: true; text: modelData.branch.name + (modelData.document ? ' · Revision ' + modelData.document.revision : ' · No saved document'); font.family: Theme.font; font.pixelSize: Theme.fsSm; font.weight: Font.DemiBold; color: Theme.textPrimary; wrapMode: Text.Wrap }
                    Text { visible: !!modelData.document; Layout.fillWidth: true; text: modelData.document ? 'Saved revision ' + modelData.document.revision : ''; font.family: Theme.font; font.pixelSize: Theme.fsXs; color: Theme.textSecondary; wrapMode: Text.Wrap }
                    STextArea { Layout.fillWidth: true; Layout.preferredHeight: 320; text: modelData.document ? modelData.document.content : 'No saved alternative'; readOnly: true; wrapMode: TextEdit.Wrap }
                }
            }
        }
    }

    FolderDialog {
        id:skillsDialog
        title:'Import project skill documents'
        onAccepted:root.request('skills','qroc-skills-import',{project_id:root.projectId,path:root.pathFromUrl(selectedFolder)},(r,ok)=>{if(ok){root.noticeText='Imported '+(r.imported||[]).length+' skill documents';root.loadProject(root.projectId,'keep');}})
    }
    FolderDialog {
        id:importProjectDialog
        title:'Import an existing QROC export folder'
        onAccepted: {
            var path=decodeURIComponent(String(selectedFolder).replace(/^file:\/\//,''));
            root.persistDraft();
            root.request('import-project','qroc-project-import',{path:path},function(result,ok){
                if(!ok)return;
                root.noticeText='Imported '+(result.imported||0)+' revisions';
                root.loadProjects();
                root.loadProject(result.project.id,'DASHBOARD');
            });
        }
    }
    SDialog {
        id: gateReviewDialog; parent: Overlay.overlay; anchors.centerIn: parent; modal: true
        title: 'Gate review'; width: Math.min(720, parent.width - 32); height: Math.min(620, parent.height - 32)
        background: Rectangle { color: Theme.panel; border.color: Theme.edge }
        contentItem: ScrollView { id: gateScroll; clip: true; contentWidth: availableWidth
            ColumnLayout { width: gateScroll.availableWidth
                            QrocGateReview {
                                Layout.fillWidth: true
                                result: root.gate
                                review: root.recordedGateReview
                                nextDomain: root.nextReviewedDomain
                                actionBusy: root.gateActionBusy
                                standing: root.reviewStanding
                                revision: root.currentDocument ? root.currentDocument.revision : 0
                                semanticInspection:root.semanticInspection
                                canReview: root.exactSavedRevision && root.gateDocumentId === root.currentDocument.id && root.reviewSourcesCurrent
                                onReviewRequested: decision => root.recordGateDecision(decision)
                                onAdvanceRequested: root.advanceReviewedDomain()
                            }
            }
        }
        footer: RowLayout { Item { Layout.fillWidth: true } SButton { text: 'Close'; onClicked: gateReviewDialog.close() } }
    }

}

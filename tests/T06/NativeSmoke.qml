import QtQuick
import Quickshell
import "../../ui"
Item {
    id: smoke
    property var workspace
    property string projectId
    property string branchId
    property int phase: 0
    property bool started: false
    property bool failed: false
    function fail(error) { failed=true;console.error('T06_NATIVE_FAIL',error);Qt.quit() }
    function check(condition,error) { if(!condition) {fail(error);return false} return true }
    function request(command,args,next) { Backend.send(command,args,function(r,ok,error){if(!ok)fail(command+': '+error);else next(r)}) }
    function begin() {
        started=true;
        // Real development source, no fabricated Backend responses.
        request('qroc-project-import',{path:Quickshell.env('QROC_SMOKE_INPUT'),title:'T06 source review'},function(r){
            projectId=r.project.id;branchId=r.branch.id;
            if(!check(r.project.sources[0].sha256.length===64,'Retained source hash absent'))return;
            console.log('T06_IMPORTED',JSON.stringify(r));workspace.loadProject(projectId);phase=1;
        })
    }
    Timer {
        interval:100;repeat:true;running:!smoke.failed
        onTriggered: {
            var w=smoke.workspace;
            if(!w||!Backend.ready)return;
            if(!smoke.started){smoke.begin();return}
            if(smoke.phase===1&&w.currentDocument&&w.draftLoaded) {
                if(!smoke.check(w.currentDocument.content.indexOf('Inspect each pantry source')>=0,'Imported reader content absent'))return;
                if(!smoke.check(!w.explicitModel,'Imported historical model must not enable generation'))return;
                if(!smoke.check(w.authoringState.capabilities.scope===false&&w.authoringState.capabilities.registry===false,'Foundation flags invented extension availability'))return;
                w.generate();if(!smoke.check(w.errorText.indexOf('Provider discovery unavailable')>=0&&!w.jobId,'Generation guard absent'))return;
                w.errorText='';w.draftText+='\nRevised proposal: inspect evidence before substitutions.';w.saveDocument();smoke.phase=2;
            } else if(smoke.phase===2&&w.currentDocument&&w.currentDocument.revision===2&&w.draftLoaded) {
                console.log('T06_REVISION',JSON.stringify(w.currentDocument));
                w.draftText+='\nUnsaved uncertainty: retain allergen question.';w.persistDraft();w.selectDocument('PRE_MORTEM');smoke.phase=3;
            } else if(smoke.phase===3&&w.documentType==='PRE_MORTEM'&&w.draftLoaded) {
                w.selectDocument('STRATEGY_BRIEF');smoke.phase=4;
            } else if(smoke.phase===4&&w.documentType==='STRATEGY_BRIEF'&&w.draftLoaded&&w.history.length===2) {
                if(!smoke.check(w.draftText.indexOf('Unsaved uncertainty')>=0&&w.draftBaseRevision===2&&!w.exactSavedRevision,'Draft restoration or dirty standing failed'))return;
                console.log('T06_RESTORED',w.draftBaseRevision,w.reviewStanding);
                var old=w.history.findIndex(d=>d.revision===1);w.selectHistory(old);
                if(!smoke.check(w.draftBaseRevision===1&&w.gate===null&&!w.exactSavedRevision,'Historical revision retained stale base/review'))return;
                console.log('T06_HISTORICAL',w.draftBaseRevision,w.reviewStanding);
                w.selectDocument('GATE_1');smoke.phase=5;
            } else if(smoke.phase===5&&w.documentType==='GATE_1'&&w.draftLoaded) {
                w.draftText='Development review witness only.\n[SCORE]: 95\n- Evidence: 10/10 - deterministic supplied input';w.saveDocument();smoke.phase=6;
            } else if(smoke.phase===6&&w.currentDocument&&w.currentDocument.document_type==='GATE_1'&&w.gate&&w.draftLoaded) {
                if(!smoke.check(w.exactSavedRevision&&w.gateDocumentId===w.currentDocument.id,'Gate not bound to exact saved document'))return;
                w.recordGateDecision('passed');smoke.phase=7;
            } else if(smoke.phase===7&&w.recordedGateReview&&!w.gateActionBusy) {
                console.log('T06_EXACT_REVIEW',JSON.stringify(w.recordedGateReview),w.reviewStanding);
                w.draftText+='\nChanged evidence';
                if(!smoke.check(!w.exactSavedRevision&&w.reviewStanding.indexOf('not current')>=0,'Changed draft still current'))return;
                w.recordGateDecision('failed');
                if(!smoke.check(!w.gateActionBusy,'Changed draft dispatched review'))return;
                w.saveDocument();smoke.phase=8;
            } else if(smoke.phase===8&&w.currentDocument&&w.currentDocument.revision===2&&w.draftLoaded) {
                if(!smoke.check(w.recordedGateReview===null,'Old review applies to newer revision'))return;
                console.log('T06_NEW_REVISION_UNREVIEWED',w.reviewStanding);
                // Create another project with retained QML create flow, then restore the old project.
                w.newProjectTitle='Decision-local project';w.newProjectBrainDump='Draft without mandatory gate or generation';w.createProject();smoke.phase=9;
            } else if(smoke.phase===9&&w.project&&w.project.title==='Decision-local project') {
                console.log('T06_PROJECT_CREATED',w.project.id);w.loadProject(smoke.projectId);smoke.phase=10;
            } else if(smoke.phase===10&&w.project&&w.project.id===smoke.projectId&&w.currentDocument&&w.currentDocument.document_type==='GATE_1'&&w.draftLoaded) {
                if(!smoke.check(w.project.gate_reviews.length===1&&w.recordedGateReview===null,'Review history restoration failed'))return;
                // Only discovery, no PHOD compiler or model launch.
                smoke.phase=11;w.runAnalysis();
                Backend.send('qroc-scope-preview',{project_id:smoke.projectId,branch_id:smoke.branchId,nature:'internal',rationale:'Real unsupported route witness'},function(r,ok,error){
                    if(!smoke.check(!ok&&error.indexOf('capability-unavailable')>=0,'Real foundation scope must fail explicitly'))return;
                    console.log('T06_REAL_SCOPE_UNAVAILABLE',error);
                    Backend.send('qroc-registry-provider-capabilities',{},function(r,ok,error){
                        if(!smoke.check(!ok&&error.indexOf('capability-unavailable')>=0,'Real foundation provider discovery must fail explicitly'))return;
                        console.log('T06_REAL_PROVIDER_UNAVAILABLE',error);
                smoke.request('phod-list',{},function(p){console.log('T06_PHOD_DISCOVERY',JSON.stringify(p));
                    smoke.request('qroc-export',{project_id:smoke.projectId},function(e){
                        console.log('T06_EXPORT',JSON.stringify(e));console.log('T06_NATIVE_PASS',Backend.ownerRoot,Backend.executable);Qt.quit();
                    });
                });
                    });
                });
            }
        }
    }
    Timer {interval:20000;running:true;onTriggered:smoke.fail('Journey timeout phase '+smoke.phase)}
}

import QtQuick
import QtQuick.Controls
import QtTest
import Quickshell
import "../../ui"
import "../../ui/authoring"

ShellRoot {
    id:shell
    property bool started:false
    FixtureTransport {id:fixture}
    FloatingWindow {
        visible:true;implicitWidth:760;implicitHeight:900
        QrocWorkspace {id:w;width:Math.min(640,parent.width);height:parent.height}
        TestCase {
            id:checks;name:'T06SchemaFixtures';when:false
            function count(command){return fixture.requests.filter(x=>x.command===command).length}
            function typeText(text){for(var i=0;i<text.length;i++)keyClick(text[i]);}
            function configure(){
                var done=false;var error='';
                Backend.send('qroc-project-create',{title:'Schema-fixture UI only',brain_dump:'No integrated scope/provider acceptance'},function(p,ok,e){
                    if(!ok){error=e;done=true;return}
                    Backend.send('qroc-branch-create',{project_id:p.id,name:'Fixture selection',model:'authoring-fixture-only',provider:'authoring-fixture-only'},function(b,created,err){
                        error=created?'':err;w.loadProject(p.id);done=true;
                    });
                });
                tryVerify(()=>done);compare(error,'');tryVerify(()=>!!w.project&&w.branches.length===1&&w.draftLoaded);
                console.log('T06_SCHEMA_CONFIGURE_REAL_READY',w.projectId,w.branchId,w.draftLoaded);
                w.authoringState.transport=fixture;w.authoringState.discover();
                tryVerify(()=>!!w.authoringState.catalog&&!!w.authoringState.provider&&w.authoringState.scopeReadKnown);
            }
            function test_all(){
                console.log('T06_SCHEMA_FIXTURE_ONLY_BEGIN: admitted examples, no real extension implementation');
                configure();console.log('T06_SCHEMA_CONFIGURED');var a=w.authoringState;
                compare(a.projectId,w.projectId);compare(a.branchId,w.branchId);
                verify(!w.explicitModel);verify(a.branchConfigured({provider:a.provider.provider,model:a.provider.model}));verify(!a.generationAllowed('PRD'));verify(a.providerStatus.indexOf('reachability and inference unverified')>=0);
                verify(a.catalog.semantic_force_registry.available===false);
                a.loadType(' PRD ');tryVerify(()=>!!a.typeProfile);compare(a.typeProfile.id,'PRD');verify(a.typeProfile.semantic_force_profile_available===false);
                mouseClick(findChild(w,'qroc-scope-open'));var dialog=findChild(w,'qroc-scope-dialog');tryVerify(()=>dialog.visible);
                wait(100);var rationale=findChild(w,'qroc-scope-rationale');mouseClick(rationale);verify(rationale.activeFocus);typeText('Relevant internal authoring');
                var constraints=findChild(w,'qroc-scope-constraints');constraints.forceActiveFocus();typeText('Licence remains applicable');
                console.log('T06_SCHEMA_FIELDS',rationale.text,rationale.activeFocus,a.rationale,constraints.text,a.constraintsText);grabImage(w).save(Quickshell.env('QROC_INPUT_IMAGE'));
                var preview=findChild(w,'qroc-scope-preview');mouseClick(preview);tryVerify(()=>!!a.preview);
                verify(a.preview.revision===undefined);compare(a.preview.constraints[0],'Licence remains applicable');
                compare(fixture.stored,null);verify(a.canSave);verify(a.preview.gate_reviews.length===0&&a.preview.document_refs.length===0);
                mouseClick(findChild(w,'qroc-scope-save'));tryVerify(()=>!!a.savedScope&&a.savedScope.revision===1);
                var save=fixture.requests.filter(x=>x.command==='qroc-scope-save')[0];compare(save.args.project_id,w.projectId);compare(save.args.branch_id,w.branchId);compare(save.args.source_revision,0);
                verify(a.savedScope.execution_authority===false);verify(!a.canSave);
                // Explicit empty selection differs from omitted default selection.
                a.useDefaults=false;a.includedText='';a.previewScope();tryVerify(()=>!!a.preview);compare(a.preview.included.length,0);
                var empty=fixture.requests.filter(x=>x.command==='qroc-scope-preview').slice(-1)[0];verify(Array.isArray(empty.args.included));compare(empty.args.included.length,0);
                a.includedText='PRD, STRATEGY_BRIEF';a.excludedText='PRD: irrelevant document';a.previewScope();tryVerify(()=>!!a.preview);compare(a.preview.included.join(','),'STRATEGY_BRIEF');compare(a.preview.constraints[0],'Licence remains applicable');
                // Stale refusal keeps edits/constraints and sends exactly one save; reload does not auto-save.
                fixture.refuseSave=true;var before=count('qroc-scope-save');a.saveScope();tryVerify(()=>a.stale);compare(count('qroc-scope-save'),before+1);
                a.saveScope();compare(count('qroc-scope-save'),before+1);compare(a.constraintsText,'Licence remains applicable');verify(a.errorText.indexOf('Reload and compare')>=0);
                a.loadScope(true);tryVerify(()=>a.scopeReadKnown&&!a.stale);compare(count('qroc-scope-save'),before+1);compare(a.includedText,'PRD, STRATEGY_BRIEF');verify(!a.canSave);
                // Wrong wrappers and authority are errors, never successful previews.
                ['wrapped','revision','authority','constraints-loss'].forEach(function(mode){fixture.previewMode=mode;a.previewScope();tryVerify(()=>!a.previewing);compare(a.preview,null);verify(a.errorText.indexOf('Unexpected authoring response')>=0)});
                fixture.previewMode='';fixture.getMode='direct';a.loadScope(true);tryVerify(()=>!a.loadingScope);verify(!a.scopeReadKnown);verify(!a.canSave);
                fixture.getMode='wrong-owner';a.loadScope(true);tryVerify(()=>!a.loadingScope);verify(!a.scopeReadKnown);
                fixture.getMode='';a.loadScope(true);tryVerify(()=>a.scopeReadKnown);
                ['bad-endpoint','missing-model'].forEach(function(mode){fixture.providerMode=mode;a.errorText='';a.loadProvider();tryVerify(()=>a.errorText.indexOf('configured provider')>=0);compare(a.provider,null);verify(!w.explicitModel)});
                fixture.providerMode='unavailable';a.loadProvider();tryVerify(()=>!!a.provider);verify(a.provider.available===false);verify(a.providerStatus.indexOf('generation unavailable')>=0);
                // Delayed callback must not cross a selected owner.
                fixture.hold=true;fixture.previewMode='';a.previewScope();var oldProject=a.projectId;var oldBranch=a.branchId;
                a.projectId='another-project';a.branchId='another-branch';fixture.hold=false;fixture.flush();compare(a.preview,null);verify(!a.canSave);
                a.projectId=oldProject;a.branchId=oldBranch;fixture.stored=null;a.loadScope(false);tryVerify(()=>a.scopeReadKnown);compare(a.savedScope,null);
                a.rationale='Empty branch authoring';a.useDefaults=false;a.includedText='';a.previewScope();tryVerify(()=>!!a.preview);fixture.refuseSave=false;a.saveScope();tryVerify(()=>!!a.savedScope&&a.savedScope.included.length===0);
                tryVerify(()=>w.documentType==='');compare(w.domainDocuments.PRODUCT.length,0);verify(!w.explicitModel);
                wait(100);grabImage(w).save(Quickshell.env('QROC_INPUT_IMAGE'));
                console.log('T06_SCHEMA_FIXTURE_PASS',JSON.stringify(fixture.requests),w.width,'NOT INTEGRATED ACCEPTANCE');Qt.quit();
            }
        }
    }
    Timer {interval:200;repeat:true;running:!shell.started;onTriggered:if(Backend.ready){shell.started=true;stop();try{checks.test_all()}catch(e){console.error('T06_SCHEMA_FIXTURE_FAIL',e,JSON.stringify({project:w.projectId,branch:w.branchId,scopeKnown:w.authoringState.scopeReadKnown,error:w.authoringState.errorText,catalog:!!w.authoringState.catalog,provider:w.authoringState.provider,requests:fixture.requests}));Qt.quit()}}}
    Timer {interval:25000;running:true;onTriggered:{console.error('T06_SCHEMA_FIXTURE_FAIL timeout');Qt.quit()}}
}

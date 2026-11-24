import QtQuick
import QtQuick.Controls
import QtQuick.Window
import QtTest
import Quickshell
import "../../ui"
Item {
    id: qa
    property var shell
    property bool started:false
    TestCase {
        id:t
        when:false
        name:'T08Installed'
        function assertTrue(value,message) {if(!value)throw new Error(message||'assertion failed');}
        function assertEqual(actual,expected) {if(actual!==expected)throw new Error('expected '+expected+' observed '+actual);}
        function awaitCondition(predicate,ms,message) {var until=Date.now()+(ms||5000);while(!predicate()&&Date.now()<until)wait(20);assertTrue(predicate(),message||('Timeout: '+String(predicate)));}
        function request(c,a,expected) {
            var done=false,reply,good,error;
            Backend.send(c,a,function(r,ok,e){reply=r;good=ok;error=e;done=true});
            awaitCondition(()=>done,10000,c+' reply');
            if(expected===false){assertTrue(!good,c+' unexpectedly succeeded');return error;}
            if(!good)console.error('T08_REQUEST_ERROR',c,error);assertTrue(good,c+': '+error);console.log('T08_REPLY',c,JSON.stringify(reply));return reply;
        }
        function type(text) {for(var i=0;i<text.length;i++){if(text[i]==='\n')keyClick(Qt.Key_Return);else keyClick(text[i]);}}
        function buttonText(item,text){if(item&&item.text===text&&typeof item.clicked==='function')return item;var children=item&&item.children?item.children:[];for(var i=0;i<children.length;i++){var found=buttonText(children[i],text);if(found)return found;}return null;}
        function readerProof(reopen) {
            var w=qa.shell.workspace,win=w.Window.window,identity;
            console.log('T08_WINDOW_CONTEXT',JSON.stringify({window:[win.width,win.height],workspace:[w.width,w.height],screenName:w.Screen.name,screen:[w.Screen.width,w.Screen.height],scale:w.Screen.devicePixelRatio}));
            assertTrue(w.width>=700&&w.height>=608,'healthy normal available space required; no out-of-window normal proof');
            var normal=[w.width,w.height];
            if(reopen){identity=request('bridge-prefs-get',{}).prefs.reader_identity;assertTrue(identity);w.branchId=identity.branch_id;w.loadProject(identity.project_id);}
            else {var p=request('qroc-project-import',{path:Quickshell.env('QROC_QA_INPUT'),title:'Actual installed adaptive reader'});w.branchId=p.branch.id;w.loadProject(p.project.id);}
            awaitCondition(()=>w.project&&w.draftLoaded,5000,'actual selected reader owner');
            if(!reopen){var text='Retained source proposal with unresolved human questions.\n';for(var i=0;i<110;i++)text+='Line '+i+': retain real imported source identity, alternatives and human judgment.\n';w.draftText=text;w.saveDocument();awaitCondition(()=>w.currentDocument&&w.currentDocument.revision===1&&w.draftLoaded,5000);}
            else {awaitCondition(()=>w.history.length>=2,5000);assertEqual(w.currentDocument.id,identity.document_id);assertEqual(w.currentDocument.sha256,identity.sha256);assertEqual(w.currentDocument.revision,identity.revision);}
            function geometry(label){var reader=findChild(w,'qroc-document-reader'),scroll=reader.scroll,origin=scroll.mapToItem(w,0,0);console.log('T08_VISIBLE_GEOMETRY',label,JSON.stringify({window:[win.width,win.height],workspace:[w.width,w.height],reader:[reader.width,reader.height],viewport:[scroll.width,scroll.height],origin:[origin.x,origin.y],content:scroll.contentHeight,chars:w.draftText.length,positionKey:reader.positionKey,screenName:w.Screen.name,scale:w.Screen.devicePixelRatio}));assertTrue(scroll.height>=80&&scroll.width>0);assertTrue(origin.y>=0&&origin.x>=0&&origin.y+scroll.height<=w.height+1&&origin.x+scroll.width<=w.width+1,'reader contained');assertTrue(scroll.contentHeight>scroll.height);reader.editor.focus=false;scroll.forceActiveFocus();scroll.contentY=0;wait(100);grabImage(win.contentItem).save(Quickshell.env('QROC_QA_PROOF')+'/'+label+'-top.png');mouseWheel(scroll,scroll.width-12,Math.min(80,scroll.height/2),0,-1200);awaitCondition(()=>scroll.contentY>0,5000,'actual wheel');scroll.contentY=scroll.contentHeight-scroll.height;wait(100);assertTrue(Math.abs(scroll.contentY+scroll.height-scroll.contentHeight)<2);grabImage(win.contentItem).save(Quickshell.env('QROC_QA_PROOF')+'/'+label+'-bottom.png');mouseWheel(scroll,scroll.width-12,Math.min(80,scroll.height/2),0,1200);awaitCondition(()=>scroll.contentY<scroll.contentHeight-scroll.height-1);scroll.contentY=0;return reader;}
            geometry('normal');w.width=381;w.height=608;wait(150);var reader=geometry('narrow');
            mouseClick(findChild(w,'qroc-workspace-details-open'));awaitCondition(()=>findChild(w,'qroc-workspace-details').visible);grabImage(win.contentItem).save(Quickshell.env('QROC_QA_PROOF')+'/project-details.png');keyClick(Qt.Key_Escape);awaitCondition(()=>!findChild(w,'qroc-workspace-details').visible);
            mouseClick(findChild(w,'qroc-document-metadata-open'));awaitCondition(()=>findChild(w,'qroc-document-metadata').visible);awaitCondition(()=>w.authoringState.typeProfile&&w.authoringState.typeProfile.id===w.documentType&&findChild(w,'qroc-policy-metadata').text.indexOf('Allowed upstream:')>=0,5000,'actual metadata Details response');grabImage(win.contentItem).save(Quickshell.env('QROC_QA_PROOF')+'/document-metadata.png');keyClick(Qt.Key_Escape);awaitCondition(()=>!findChild(w,'qroc-document-metadata').visible);
            if(!reopen){mouseClick(reader.editor,25,15);keyClick(Qt.Key_End,Qt.ControlModifier);type(' Native narrow editing retained.');awaitCondition(()=>w.draftText.indexOf('Native narrow editing retained.')>=0);var save=findChild(w,'qroc-save-revision'),point=save.mapToItem(w,0,0);assertTrue(save.visible&&save.enabled&&point.y+save.height<=w.height);mouseClick(save);awaitCondition(()=>w.currentDocument.revision===2&&w.draftLoaded&&w.history.length===2,5000);identity={project_id:w.projectId,branch_id:w.branchId,document_id:w.currentDocument.id,revision:w.currentDocument.revision,sha256:w.currentDocument.sha256};request('bridge-prefs-save',{prefs:Object.assign({},Backend.prefs,{reader_identity:identity})});}
            mouseClick(findChild(w,'qroc-document-metadata-open'));awaitCondition(()=>findChild(w,'qroc-document-metadata').visible);var h=findChild(w,'qroc-history-metadata'),caption=findChild(w,'qroc-history-caption');awaitCondition(()=>h.displayText==='Revision 2'&&h.currentIndex===w.selectedHistoryIndex);assertTrue(caption.text.indexOf('Revision 2')===0);assertEqual(w.currentDocument.id,identity.document_id);grabImage(win.contentItem).save(Quickshell.env('QROC_QA_PROOF')+'/history-latest.png');
            var index=w.history.findIndex(d=>d.revision===1),old=w.history[index];mouseClick(h);keyClick(Qt.Key_Home);for(var n=0;n<index;n++)keyClick(Qt.Key_Down);keyClick(Qt.Key_Return);awaitCondition(()=>w.currentDocument.id===old.id&&h.displayText==='Revision 1');assertTrue(caption.text.indexOf('Revision 1')===0);assertEqual(w.draftText,old.content);grabImage(win.contentItem).save(Quickshell.env('QROC_QA_PROOF')+'/history-historical.png');
            w.selectDocument(w.documentType);assertEqual(h.currentIndex,-1);assertEqual(h.displayText,'Loading saved revisions…');awaitCondition(()=>w.currentDocument.id===identity.document_id&&w.draftLoaded&&h.displayText==='Revision 2',5000);assertEqual(w.currentDocument.sha256,identity.sha256);grabImage(win.contentItem).save(Quickshell.env('QROC_QA_PROOF')+'/history-restored.png');keyClick(Qt.Key_Escape);w.width=normal[0];w.height=normal[1];wait(100);geometry('normal-restored');assertEqual(request('qroc-job-list',{}).jobs.length,0);console.log('T08_ADAPTIVE_READER_PASS',JSON.stringify(identity),reopen?'REOPEN':'EDIT_SAVE');
        }
        function authoring() {
            var w=qa.shell.workspace;
            var source=Quickshell.env('QROC_QA_INPUT');
            var p=request('qroc-project-import',{path:source,title:'Installed source authoring'});
            assertTrue(p.project.sources[0].sha256.length===64);w.loadProject(p.project.id);
            awaitCondition(()=>w.projectId===p.project.id&&w.branchId===p.branch.id&&w.draftLoaded&&w.authoringState.catalog!==null&&w.authoringState.scopeReadKnown,5000);
            var a=w.authoringState;
            assertTrue(a.capabilities.scope&&a.capabilities.registry);assertTrue(a.provider&&!a.provider.available);assertTrue(!w.explicitModel);
            assertTrue(a.catalog.semantic_force_registry.available===true);assertEqual(a.catalog.semantic_force_registry.complete_catalogue_available,false);assertEqual(JSON.stringify(a.catalog.semantic_force_registry.supported_profiles),JSON.stringify(['retained-market-review/1']));
            var doc='Preserve exact source and human uncertainty.\n';for(var i=0;i<100;i++)doc+='Line '+i+': compare useful alternatives without execution authority.\n';
            w.draftText=doc;w.saveDocument();awaitCondition(()=>w.currentDocument&&w.currentDocument.revision===1&&w.draftLoaded,5000);
            var reader=findChild(w,'qroc-document-reader'),edit=findChild(w,'document-reader-text'),scroll=findChild(w,'document-reader-scroll');
            assertTrue(reader&&edit&&scroll);reader.forceActiveFocus();edit.forceActiveFocus();keyClick(Qt.Key_End,Qt.ControlModifier);type('Native keyboard retained.');
            awaitCondition(()=>w.draftText.indexOf('Native keyboard retained.')>=0);w.saveDocument();awaitCondition(()=>w.currentDocument.revision===2&&w.draftLoaded&&w.history.length===2,5000);
            edit.focus=false;reader.focus=false;scroll.contentY=0;
            console.log('T08_READER_BEFORE',w.width,w.height,reader.width,reader.height,scroll.width,scroll.height,scroll.contentHeight,w.draftText.length,scroll.mapToItem(null,0,0));
            awaitCondition(()=>scroll.height>0&&scroll.contentHeight>scroll.height,5000,'long reader has scrollable geometry');
            console.log('T08_READER_GEOMETRY',scroll.width,scroll.height,scroll.contentHeight,scroll.contentY,scroll.mapToItem(null,0,0));
            mouseWheel(scroll,scroll.width-12,Math.min(80,scroll.height/2),0,-1200);awaitCondition(()=>scroll.contentY>0,5000,'long reader wheel scroll');
            console.log('T08_KEYBOARD_SCROLL',w.currentDocument.sha256,scroll.contentY,scroll.contentHeight);
            var alt=request('qroc-branch-create',{project_id:w.projectId,name:'Different manual alternative'});
            request('qroc-document-save',{project_id:w.projectId,branch_id:alt.id,document_type:'STRATEGY_BRIEF',content:'Alternative: retain unresolved evidence and ask a focused question.',source_revision:0});
            w.runAnalysis();awaitCondition(()=>w.analysisRows.length===2,5000);assertTrue(w.analysisRows.every(r=>!!r.document));
            request('qroc-document-save',{project_id:w.projectId,branch_id:w.branchId,document_type:'GATE_1',content:'[SCORE]: 95\n- Evidence: 95/100 - supplied manual review witness\nNot provider generation.',source_revision:0});
            w.selectDocument('GATE_1');w.loadDocuments();awaitCondition(()=>w.currentDocument&&w.currentDocument.document_type==='GATE_1'&&w.gate&&w.draftLoaded,5000);
            w.recordGateDecision('passed');awaitCondition(()=>w.recordedGateReview&&!w.gateActionBusy,5000);
            mouseClick(findChild(w,'qroc-scope-open'));var dialog=findChild(w,'qroc-scope-dialog');awaitCondition(()=>dialog.visible);wait(150);
            var rationale=findChild(w,'qroc-scope-rationale');assertTrue(rationale.enabled);mouseClick(rationale);keyClick(Qt.Key_A,Qt.ControlModifier);type('Internal authoring with retained questions');
            var constraints=findChild(w,'qroc-scope-constraints');assertTrue(constraints!==null);
            // Qt wheel/key events are delivered only to this owned application surface.
            var sv=constraints.parent;while(sv&&sv.contentItem===undefined)sv=sv.parent;
            assertTrue(sv!==null);mouseWheel(sv,sv.width/2,sv.height/2,0,-2400);wait(200);
            constraints.forceActiveFocus();type('No proposal execution.\nPreserve source uncertainty.');
            awaitCondition(()=>a.constraintsText.indexOf('Preserve source uncertainty.')>=0);
            a.previewScope();awaitCondition(()=>a.preview!==null,5000);assertTrue(a.preview.revision===undefined);assertTrue(!Object.prototype.hasOwnProperty.call(a.preview,'scope'));assertTrue(a.preview.constraints.length===2);
            a.saveScope();awaitCondition(()=>a.savedScope&&a.savedScope.revision===1,5000);
            assertTrue(a.savedScope.document_refs.length>=3,'populated exact lineage');assertTrue(a.savedScope.gate_reviews.length===1,'owner-derived review');
            var saved=a.savedScope,selection=a.selection();selection.source_revision=1;
            request('qroc-scope-save',selection);a.previewScope();awaitCondition(()=>a.preview!==null);a.saveScope();awaitCondition(()=>a.stale&&!a.saving);assertTrue(a.errorText.indexOf('Scope changed')>=0);assertEqual(request('qroc-scope-get',{project_id:w.projectId,branch_id:w.branchId}).scope.revision,2);
            a.loadScope();awaitCondition(()=>!a.stale&&a.savedScope.revision===2,5000);
            var invalid=Object.assign({},selection,{project_id:'missing-owner',source_revision:0});assertTrue(request('qroc-scope-save',invalid,false).length>0);
            request('qroc-creative-source-import',{project_id:w.projectId,branch_id:alt.id+'-missing',path:Quickshell.env('QROC_QA_AUDIO')},false);
            keyClick(Qt.Key_Escape);awaitCondition(()=>!dialog.visible);
            w.draftText+='\nChanged review evidence';assertTrue(!w.exactSavedRevision);w.saveDocument();awaitCondition(()=>w.currentDocument.revision===2&&w.recordedGateReview===null&&w.draftLoaded,5000);
            var exp=request('qroc-export',{project_id:w.projectId});console.log('T08_AUTHORING_EXPORT',JSON.stringify(exp));
            request('bridge-prefs-save',{prefs:{qa_project:w.projectId,qa_branch:w.branchId}});
            console.log('T08_AUTHORING_PASS',Backend.ownerRoot,Backend.executable);
        }
        function creative() {
            var w=qa.shell.workspace,c=qa.shell.creativeView;
            var prefs=request('bridge-prefs-get',{}).prefs;w.loadProject(prefs.qa_project);
            awaitCondition(()=>w.branchId===prefs.qa_branch&&w.draftLoaded&&w.authoringState.savedScope!==null,5000);
            qa.shell.selectedView=2;awaitCondition(()=>c.projectId===w.projectId&&c.branchId===w.branchId&&c.capabilities.length>0&&!c.busy,5000);
            if(Quickshell.env('QROC_QA_REOPEN')==='1'){
                c.refresh();awaitCondition(()=>c.outputs.length===2&&!c.busy,5000);assertTrue(c.outputs.every(o=>o.status==='succeeded'));assertEqual(w.authoringState.savedScope.revision,2);console.log('T08_REOPEN_PASS',JSON.stringify(c.outputs));return;
            }
            c.sourcePath=Quickshell.env('QROC_QA_AUDIO');c.importSource(false);awaitCondition(()=>c.selectedAsset&&!c.busy,5000);
            c.durationSeconds=.5;c.volume=.5;c.convert(false);awaitCondition(()=>c.selectedOutput&&c.selectedOutput.status==='succeeded'&&!c.busy,10000);var first=c.selectedOutput.output_sha256;
            c.volume=1;c.convert(true);awaitCondition(()=>c.selectedOutput&&c.selectedOutput.revision===2&&c.selectedOutput.status==='succeeded'&&!c.busy,10000);assertTrue(first!==c.selectedOutput.output_sha256);
            c.exportOutput();awaitCondition(()=>c.lastExport!==null&&!c.busy,5000);assertEqual(c.outputs.length,2);assertTrue(c.capabilities.some(x=>x.domain==='game'&&!x.supported));console.log('T08_CREATIVE_EXPORT',JSON.stringify(c.lastExport));console.log('T08_CREATIVE_PASS',JSON.stringify(c.outputs));
        }
        function phod() {
            qa.shell.selectedView=1;var v=qa.shell.phodView;v.refreshJourney();awaitCondition(()=>v.moduleInfo!==null&&v.moduleInfo.capabilities.workbench.available,5000);
            v.intentName='installed-score-validator';v.intentPurpose='Reject invalid integer scores';v.lowerBound='0';v.upperBound='100';v.upperPolicy='inclusive';v.actor='Installed release journey';v.rationale='100 valid;101 rejected';v.trialUpper='101';v.repairEnabled=true;v.startJourney();
            awaitCondition(()=>v.selectedJob&&v.selectedJob.state==='succeeded',15000);var r=v.report();assertTrue(r&&r.passed&&r.attempts.length===3);assertEqual(r.attempts[0].decision.status,'rejected');assertEqual(r.attempts[2].decision.status,'accepted');assertEqual(r.repair.counterexamples[0].input,101);
            var replay=request('phod-replay',{id:v.selectedJob.id});assertTrue(!replay.effectful_rerun);assertEqual(replay.decisions[0].decision.status,'rejected');console.log('T08_PHOD_PASS',JSON.stringify(r));
        }
        function backendReopen() {
            var originalProject=Backend.prefs.qa_project,originalBranch=Backend.prefs.qa_branch;
            assertTrue(Backend.restored&&originalProject&&originalBranch,'retained owner prefs were not restored before ready');
            console.log('T08_RESTORED_PREFS_BEFORE_WORKSPACE',JSON.stringify(Backend.prefs));
            var w=qa.shell.workspace,c=qa.shell.creativeView;w.branchId=originalBranch;w.loadProject(originalProject);
            awaitCondition(()=>w.branchId===originalBranch&&w.projectId===originalProject&&w.draftLoaded&&w.authoringState.savedScope!==null,5000,'retained selected workspace/scope readback');
            assertEqual(w.authoringState.savedScope.revision,2);
            var selection={project_id:w.projectId,branch_id:w.branchId,document_type:w.documentType};
            console.log('T08_RETAINED_HISTORY_PENDING',JSON.stringify(selection),JSON.stringify(w.currentDocument),JSON.stringify(w.history));
            assertEqual(selection.project_id,originalProject);assertEqual(selection.branch_id,originalBranch);assertEqual(selection.document_type,'STRATEGY_BRIEF');
            var durable=request('qroc-document-history',selection).items;
            assertTrue(durable.length>=2,'durable retained document history');
            var durableIds=JSON.stringify(durable.map(d=>d.id));
            awaitCondition(()=>w.projectId===originalProject&&w.branchId===originalBranch&&w.documentType===selection.document_type&&JSON.stringify(w.history.map(d=>d.id))===durableIds,5000,'correlated retained UI history completion');
            assertTrue(w.history.length>=2,'retained document history');
            console.log('T08_RETAINED_HISTORY_COMPLETE',durableIds,JSON.stringify(w.history.map(d=>({id:d.id,revision:d.revision}))));
            qa.shell.selectedView=2;c.refresh();awaitCondition(()=>!c.busy&&c.outputs.length===2,5000,'retained creative history');assertTrue(c.outputs.every(o=>o.status==='succeeded'));
            wait(300);var read=t.request('bridge-prefs-get',{}).prefs;assertEqual(read.qa_project,originalProject);assertEqual(read.qa_branch,originalBranch);
            console.log('T08_BACKEND_RETAINED_REOPEN_PASS',JSON.stringify(read),JSON.stringify(c.outputs));
        }
        function semantic() {
            var w=qa.shell.workspace,p=request('qroc-project-import',{path:Quickshell.env('QROC_QA_INPUT'),title:'Manually authored bounded market assessment'});
            w.branchId=p.branch.id;w.domain='BUSINESS';w.loadProject(p.project.id);
            awaitCondition(()=>w.project&&w.draftLoaded&&w.authoringState.semanticProfile!==null,5000,'actual bounded profile loaded');
            var a=w.authoringState,model=a.semanticProfile.profile,gm=model.gates[0];
            assertEqual(a.semanticProfile.profile_sha256,'6ecc7fa0a012dcbf97aef7e878a5ef4fd85843e6d72fbc0bced44d4dd4db2c25');
            assertTrue(a.catalog.semantic_force_registry.available&&!a.catalog.semantic_force_registry.complete_catalogue_available);
            var supported=a.catalog.types.filter(x=>x.semantic_force_profile_available).map(x=>x.id).sort();assertEqual(JSON.stringify(supported),JSON.stringify(['COMPETITOR_ANALYSIS','GATE_4','MARKET_ANALYSIS']));
            assertTrue(a.catalog.types.filter(x=>!x.semantic_force_profile_available).every(x=>!Object.prototype.hasOwnProperty.call(x,'semantic_model')));
            var owners={},quote='Independent clinics are the segment.';
            ['MARKET_ANALYSIS','COMPETITOR_ANALYSIS'].forEach(k=>owners[k]=request('qroc-document-save',{project_id:w.projectId,branch_id:w.branchId,document_type:k,source_revision:0,content:quote+' Manually supplied exploratory assessment; external facts and human judgment remain unverified. Existing software and doing nothing are alternatives. SOM assumptions require research.'}));
            function evidence(f){var d=owners[f.documentType];return {force_ref:f,document_id:d.id,revision:d.revision,sha256:d.sha256,quote:quote};}
            var feedback='Manual exploratory assessment; independent human semantic judgment remains required. ';for(var i=0;i<8;i++)feedback+='Read exact evidence, preserve uncertainty, and do not execute this proposal. ';
            var body={schema:'qroc-semantic-gate/1',profile:model.id,gate_type:'GATE_4',score:96,reviewed_documents:Object.keys(owners).map(k=>({document_type:k,document_id:owners[k].id,revision:owners[k].revision,sha256:owners[k].sha256})),criteria:gm.criteria.map(c=>({id:c.id,score:24,max:c.weight,feedback:feedback,force_refs:c.derivedFromForces,evidence:c.derivedFromForces.map(evidence)})),consistency:gm.consistencyRules.map(r=>({id:r.id,status:'supported',feedback:feedback,force_refs:[r.left,r.right],evidence:[evidence(r.left),evidence(r.right)]})),sections:{}};
            gm.outputContract.requiredSections.forEach(s=>body.sections[s]=feedback);
            var revision=0;
            function evaluate(value,status){var d=request('qroc-document-save',{project_id:w.projectId,branch_id:w.branchId,document_type:'GATE_4',content:typeof value==='string'?value:JSON.stringify(value),source_revision:revision});revision=d.revision;w.documentType='GATE_4';w.loadDocuments();awaitCondition(()=>w.currentDocument&&w.currentDocument.id===d.id&&w.gate&&w.gateDocumentId===d.id&&w.draftLoaded,5000,'actual semantic gate callback');assertEqual(w.gate.status,status);console.log('T08_SEMANTIC_CASE',d.id,status,JSON.stringify(w.gate));return d;}
            function panel(label,longEvidence){var win=w.Window.window;function dismiss(item){if(item&&item.accessibleLabel==='Dismiss message'&&item.visible&&typeof item.clicked==='function')return item;var children=item&&item.children?item.children:[];for(var q=0;q<children.length;q++){var found=dismiss(children[q]);if(found)return found;}return null;}var closeNotice=dismiss(win.contentItem);if(closeNotice){mouseClick(closeNotice);awaitCondition(()=>!closeNotice.visible,5000,'owned notification dismissed');}var gateDialog=null;for(var z=0;z<w.data.length;z++){if(w.data[z].title==='Gate review')gateDialog=w.data[z];}assertTrue(gateDialog,'owned gate Dialog object');var win=w.Window.window,more=buttonText(w,'More');assertTrue(more);console.log('T08_GATE_MORE',label,more.visible,more.enabled,more.mapToItem(win.contentItem,0,0),more.width,more.height,w.width,w.height);mouseClick(more);var actions=findChild(w,'qroc-document-actions');awaitCondition(()=>actions.visible,5000,'actual More actions popup '+label);var open=buttonText(actions.contentItem,'Review gate');assertTrue(open&&open.enabled);mouseClick(open);var heading=findChild(win.contentItem,'qroc-gate-heading');awaitCondition(()=>heading&&heading.visible,5000,'actual gate review panel');var review=heading.parent.parent;assertEqual(review.result.status,w.gate.status);assertEqual(review.revision,w.currentDocument.revision);var parent=review.parent,sv=null;while(parent){if(parent.contentY!==undefined&&parent.contentHeight!==undefined){sv=parent;break;}parent=parent.parent;}assertTrue(sv,'actual evidence scroll view');var flick=sv;awaitCondition(()=>flick.height>0&&(!longEvidence||flick.contentHeight>flick.height),5000,'actual evidence extent ready '+label);console.log('T08_GATE_PANEL',label,heading.text,JSON.stringify({identity:String(sv),width:sv.width,height:sv.height,content:flick.contentHeight,status:review.result.status,document_id:w.currentDocument.id,revision:review.revision}));grabImage(win.contentItem).save(Quickshell.env('QROC_QA_PROOF')+'/'+label+'-top.png');if(longEvidence){assertTrue(flick.contentHeight>flick.height);flick.contentY=0;var changes=[];for(var step=0;step<20&&flick.contentY+flick.height<flick.contentHeight-2;step++){var beforeY=flick.contentY;mouseWheel(sv,sv.width-12,Math.min(80,sv.height/2),0,-1200);awaitCondition(()=>flick.contentY>beforeY,1000,'owned evidence wheel progress');awaitCondition(()=>!flick.moving&&!flick.flicking,2000,'owned evidence event settles');console.log('T08_WHEEL_STEP',label,JSON.stringify({direction:'down',before:beforeY,settled:flick.contentY,viewport:flick.height,content:flick.contentHeight,moving:flick.moving}));changes.push(flick.contentY);}console.log('T08_EVIDENCE_WHEEL_OBSERVED',label,JSON.stringify(changes),flick.contentY,flick.height,flick.contentHeight);assertTrue(changes.length>0);awaitCondition(()=>!flick.moving&&Math.abs(flick.contentY+flick.height-flick.contentHeight)<2,3000,'wheel settles at actual evidence bottom');console.log('T08_EVIDENCE_WHEEL_BOTTOM',label,JSON.stringify(changes),flick.contentY,flick.height,flick.contentHeight);grabImage(win.contentItem).save(Quickshell.env('QROC_QA_PROOF')+'/'+label+'-bottom.png');for(var back=0;back<20&&flick.contentY>0;back++){var priorY=flick.contentY;mouseWheel(sv,sv.width-12,Math.min(80,sv.height/2),0,1200);awaitCondition(()=>flick.contentY<priorY,1000,'owned evidence wheel return');awaitCondition(()=>!flick.moving&&!flick.flicking,2000,'owned return event settles');console.log('T08_WHEEL_STEP',label,JSON.stringify({direction:'up',before:priorY,settled:flick.contentY,viewport:flick.height,content:flick.contentHeight,moving:flick.moving}));}awaitCondition(()=>!flick.moving&&Math.abs(flick.contentY)<2,3000,'wheel settles at actual evidence top');console.log('T08_EVIDENCE_WHEEL_TOP',label,flick.contentY);}var closed=false;var onClosed=function(){closed=true;};gateDialog.closed.connect(onClosed);keyClick(Qt.Key_Escape);awaitCondition(()=>closed,5000,'actual gate Dialog closed signal');gateDialog.closed.disconnect(onClosed);}
            evaluate(body,'passed');assertTrue(w.semanticInspection.valid&&w.semanticInspection.full&&w.reviewSourcesCurrent);panel('semantic-full-pass',true);
            var actualReader=findChild(w,'qroc-document-reader');actualReader.editor.forceActiveFocus();keyClick(Qt.Key_Home,Qt.ControlModifier);keyClick(Qt.Key_Right,Qt.ShiftModifier);assertTrue(actualReader.editor.selectedText.length>0,'actual saved document keyboard selection');assertEqual(w.draftText,w.currentDocument.content);actualReader.editor.deselect();
            w.recordGateDecision('passed');awaitCondition(()=>w.recordedGateReview&&!w.gateActionBusy,5000,'exact semantic review');assertTrue(w.recordedGateReview.semantic_inputs&&w.recordedGateReview.semantic_inputs.profile===model.id);
            var failed=JSON.parse(JSON.stringify(body));failed.consistency[0].status='contradicted';evaluate(failed,'failed');assertTrue(w.semanticInspection.valid&&w.semanticInspection.full);panel('semantic-full-fail',true);
            evaluate('{broken','failed');assertTrue(w.semanticInspection.valid&&!w.semanticInspection.full&&w.gate.score===null);panel('semantic-short-fail',false);
            evaluate(body,'passed');
            request('qroc-document-save',{project_id:w.projectId,branch_id:w.branchId,document_type:'MARKET_ANALYSIS',content:owners.MARKET_ANALYSIS.content+' Changed source premise.',source_revision:1});
            w.loadDocuments();awaitCondition(()=>w.gate&&w.gate.status==='failed'&&w.draftLoaded,5000,'stale source assessment refusal');assertTrue(!w.recordedGateReview);
            panel('semantic-stale-source',false);
            var before=request('qroc-project-detail',{id:w.projectId}).project.gate_reviews.length;w.recordGateDecision('passed');assertEqual(request('qroc-project-detail',{id:w.projectId}).project.gate_reviews.length,before);
            request('qroc-gate-review',{project_id:w.projectId,branch_id:w.branchId,document_type:'GATE_4',revision:revision,sha256:w.currentDocument.sha256,decision:'passed'},false);
            var errorDone=false;w.request('actual-error','qroc-document-detail',{project_id:w.projectId,branch_id:w.branchId,document_type:'GATE_4',revision:999999},function(r,ok){assertTrue(!ok);errorDone=true;});awaitCondition(()=>errorDone&&w.errorText.length>0,5000,'actual missing revision error displayed');console.log('T08_VISIBLE_ERROR',w.errorText,w.currentDocument.id,w.currentDocument.revision);grabImage(w.Window.window.contentItem).save(Quickshell.env('QROC_QA_PROOF')+'/semantic-actual-error.png');var exportResult=request('qroc-export',{project_id:w.projectId});console.log('T08_SEMANTIC_EXPORT',JSON.stringify(exportResult));console.log('T08_SEMANTIC_PASS','manual exact bindings; no inference or human semantic acceptance');
        }
        function startup() {
            wait(500);var jobs=request('qroc-job-list',{});assertTrue(jobs.jobs.length===0);var ph=request('phod-list',{});assertTrue(ph.jobs.length===0);var cr=request('qroc-creative-list',{});assertTrue(cr.outputs.length===0);console.log('T08_MODEL_FREE_OPEN_PASS');
        }
        function run() {
            try {
                if(Quickshell.env('QROC_QA_MODE')==='reader')readerProof(false);else if(Quickshell.env('QROC_QA_MODE')==='reader-reopen')readerProof(true);else if(Quickshell.env('QROC_QA_MODE')==='semantic')semantic();else if(Quickshell.env('QROC_QA_MODE')==='backend-reopen')backendReopen();else if(Quickshell.env('QROC_QA_MODE')==='startup')startup();else if(Quickshell.env('QROC_QA_MODE')==='authoring')authoring();else if(Quickshell.env('QROC_QA_MODE')==='phod')phod();else creative();
                console.log('T08_INSTALLED_PASS',Quickshell.env('QROC_RESOURCE_ROOT'),Backend.executable,Backend.ownerRoot);Qt.quit();
            } catch(e) {console.error('T08_INSTALLED_FAIL',String(e));Qt.quit();}
        }
    }
    Timer {interval:150;repeat:true;running:true;onTriggered:if(qa.shell&&Backend.ready&&!qa.started){qa.started=true;t.run()}}
    Timer {interval:55000;running:true;onTriggered:{console.error('T08_INSTALLED_FAIL timeout');Qt.quit()}}
}

import QtQuick
import QtQuick.Controls
import QtTest
import Quickshell
import "../../ui"
ShellRoot {
    FloatingWindow {
        id: win
        visible:true
        implicitWidth:640
        implicitHeight:900
        QrocWorkspace {id:w;width:Math.min(640,parent.width);height:parent.height}
        TestCase {
            id:inputTest
            name:'T06Input'
            when:false
            function typeText(text) { for(var i=0;i<text.length;i++)keyClick(text[i]); }
            function test_create_with_keyboard_and_pointer() {
                var open=findChild(w,'qroc-new-project');verify(open!==null);mouseClick(open);
                var title=findChild(w,'qroc-create-title');var idea=findChild(w,'qroc-create-idea');
                tryVerify(()=>title.visible);
                mouseClick(title);typeText('Keyboard pantry project');
                mouseClick(idea);typeText('Keep uncertain source claims visible.');
                var submit=findChild(w,'qroc-create-submit');verify(submit.enabled);mouseClick(submit);
                tryVerify(()=>w.project!==null&&w.project.title==='Keyboard pantry project');
                compare(w.project.brain_dump,'Keep uncertain source claims visible.');
                mouseClick(findChild(w,'qroc-new-branch'));
                var branchName=findChild(w,'qroc-branch-name');tryVerify(()=>branchName.visible);
                mouseClick(branchName);typeText('Manual alternative');
                mouseClick(findChild(w,'qroc-branch-model'));typeText('no-inference-test');
                mouseClick(findChild(w,'qroc-branch-provider'));typeText('manual-test');
                mouseClick(findChild(w,'qroc-branch-submit'));
                tryVerify(()=>w.branches.length===1&&w.draftLoaded);
                // Actual foundation capability flags remain false; the scope controls must refuse writes.
                verify(w.authoringState.capabilities.scope===false&&w.authoringState.capabilities.registry===false);
                verify(w.authoringState.provider===null&&!w.explicitModel);
                mouseClick(findChild(w,'qroc-scope-open'));var scopeDialog=findChild(w,'qroc-scope-dialog');tryVerify(()=>scopeDialog.visible);wait(100);
                verify(!findChild(w,'qroc-scope-rationale').enabled);verify(!findChild(w,'qroc-scope-save').enabled);
                keyClick(Qt.Key_Escape);tryVerify(()=>!scopeDialog.visible);
                console.log('T06_REAL_UNAVAILABLE_SCOPE_UI',w.authoringState.scopeStatus,w.authoringState.providerStatus);
                // Write two actual alternatives through preserved store, never generate.
                w.draftText='Alternative A: keep the source uncertainty.';w.saveDocument();
                tryVerify(()=>w.currentDocument!==null&&w.currentDocument.revision===1&&w.draftLoaded);
                var done=false;var err='';
                Backend.send('qroc-branch-create',{project_id:w.projectId,name:'Alternative B',model:'no-inference-test',provider:'manual-test'},function(b,ok,e){
                    if(!ok){err=e;done=true;return}
                    Backend.send('qroc-document-save',{project_id:w.projectId,branch_id:b.id,document_type:'STRATEGY_BRIEF',content:'Alternative B: request missing allergen evidence.',source_revision:0},function(r,saved,error){err=saved?'':error;done=true});
                });
                tryVerify(()=>done);compare(err,'');
                mouseClick(findChild(w,'qroc-compare'));
                var comparison=findChild(w,'qroc-comparison');tryVerify(()=>comparison.visible&&w.analysisRows.length===2);
                verify(w.analysisRows.every(row=>!!row.document));
                console.log('T06_INPUT_COMPARE',JSON.stringify(w.analysisRows));
                wait(100);grabImage(w).save(Quickshell.env('QROC_INPUT_IMAGE'));
                keyClick(Qt.Key_Escape);tryVerify(()=>!comparison.visible);
                mouseClick(findChild(w,'qroc-new-branch'));wait(100);
                mouseClick(findChild(w,'qroc-branch-authoring-only'));
                var nameOnly=findChild(w,'qroc-branch-name');mouseClick(nameOnly);keyClick(Qt.Key_A,Qt.ControlModifier);typeText('Authoring without a model');
                verify(!findChild(w,'qroc-branch-model').enabled);verify(!findChild(w,'qroc-branch-provider').enabled);
                mouseClick(findChild(w,'qroc-branch-submit'));tryVerify(()=>w.branches.length===3&&w.activeBranch&&w.activeBranch.name==='Authoring without a model');
                verify(!w.explicitModel);compare(w.activeBranch.model,'authoring-only');compare(w.activeBranch.provider,'authoring-only');
                console.log('T06_REAL_AUTHORING_ONLY_BRANCH',JSON.stringify(w.activeBranch));
                console.log('T06_INPUT_PASS',w.width,w.height,w.project.id,Backend.ownerRoot);
                Qt.quit();
            }
        }
    }
    Timer {interval:200;repeat:true;running:true;onTriggered:if(Backend.ready){stop();console.log('T06_INPUT_BEGIN');inputTest.test_create_with_keyboard_and_pointer()}}
    Timer {interval:15000;running:true;onTriggered:{console.error('T06_INPUT_FAIL timeout');Qt.quit()}}
}

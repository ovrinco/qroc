import QtQuick
import Quickshell
import "../../ui"
Item {
    id: smoke
    Component.onCompleted:console.log("T01_SMOKE_LOADED")
    property var workspace
    property string projectId
    property string branchId
    property int phase: 0
    property bool started: false
    function fail(error) { console.error('T01_NATIVE_FAIL',error);Qt.quit() }
    function request(command,args,next) { Backend.send(command,args,function(r,ok,error){if(!ok)fail(command+': '+error);else next(r)}) }
    function begin() {
        started=true;
        request('qroc-project-import',{path:Quickshell.env('QROC_SMOKE_INPUT'),title:'T01 native source'},function(r){
            projectId=r.project.id;branchId=r.branch.id;
            console.log('T01_IMPORTED',JSON.stringify(r));workspace.loadProject(projectId);phase=1;
        })
    }
    Timer {
        interval:100;repeat:true;running:true
        onTriggered: {
            if(!smoke.workspace||!Backend.ready)return;
            if(!smoke.started){smoke.begin();return}
            if(smoke.phase===1&&smoke.workspace.currentDocument&&smoke.workspace.draftLoaded) {
                if(smoke.workspace.currentDocument.content.indexOf('Inspect each pantry source')<0){smoke.fail('Imported source is not in preserved reader');return}
                console.log('T01_READER',smoke.workspace.documentType,smoke.workspace.currentDocument.revision);
                smoke.workspace.draftText+='\nNative revised proposal: keep source evidence visible.';
                smoke.workspace.saveDocument();smoke.phase=2;
            } else if(smoke.phase===2&&smoke.workspace.currentDocument&&smoke.workspace.currentDocument.revision===2) {
                console.log('T01_REVISION',JSON.stringify(smoke.workspace.currentDocument));smoke.phase=3;
                smoke.request('qroc-analysis',{project_id:smoke.projectId,document_type:'STRATEGY_BRIEF'},function(r){
                    if(!r.items||!r.items.length){smoke.fail('No branch comparison');return}
                    console.log('T01_COMPARE',JSON.stringify(r));
                    smoke.request('phod-list',{},function(p){
                        if(!p.capabilities){smoke.fail('PHOD capabilities missing');return}
                        console.log('T01_PHOD_DISCOVERY',JSON.stringify(p));
                        smoke.request('qroc-export',{project_id:smoke.projectId},function(e){
                            console.log('T01_EXPORT',JSON.stringify(e));
                            console.log('T01_NATIVE_PASS',Backend.ownerRoot,Backend.executable);
                            Qt.quit();
                        });
                    });
                });
            }
        }
    }
    Timer {interval:20000;running:true;onTriggered:smoke.fail('Journey timeout in phase '+smoke.phase)}
}

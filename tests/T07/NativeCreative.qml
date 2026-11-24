import QtQuick
import QtQuick.Controls
import Quickshell
import "../../ui"
ShellRoot {
    id: harness
    property int phase:0
    property string projectId:""
    property string branchId:""
    property string firstHash:""
    property bool started:false
    property bool reopening:Quickshell.env("T07_REOPEN") === "1"
    function fail(e) { console.error("T07_NATIVE_FAIL",e); Qt.quit(); }
    function request(c,a,next) {Backend.send(c,a,function(r,ok,error){if(!ok)fail(c+": "+error);else next(r)})}
    FloatingWindow {
        visible:true; implicitWidth:1100;implicitHeight:900;title:"Q-ROC T07 private creative proof"
        CreativeWorkspace {id:view;anchors.fill:parent}
    }
    Timer {
        interval:100;repeat:true;running:true
        onTriggered: {
            if(!Backend.ready || view.busy || !view.capabilities.length) return;
            if(!harness.started) {
                harness.started=true;
                console.log("T07_NATIVE_IDENTITY", Backend.executable, Backend.ownerRoot, Quickshell.env("T07_QML_IDENTITY"));
                if (harness.reopening) {
                    harness.request("qroc-project-list",{},function(r){
                        harness.projectId=r.projects[0].id;
                        harness.request("qroc-branch-list",{project_id:harness.projectId},function(b){
                            harness.branchId=b.branches[0].id;view.projectId=harness.projectId;view.branchId=harness.branchId;harness.phase=10;view.refresh();
                        });
                    });
                } else {
                    harness.request("qroc-project-create",{title:"Neutral sound preview",brain_dump:"Make a short quiet local tone preview to learn versioned creative export."},function(p){
                        harness.projectId=p.id;
                        harness.request("qroc-branch-create",{project_id:p.id,name:"quiet preview"},function(b){
                            harness.branchId=b.id;view.projectId=p.id;view.branchId=b.id;view.sourcePath=Quickshell.env("T07_INPUT");harness.phase=1;
                        });
                    });
                }
                return;
            }
            if(harness.phase===1) {harness.phase=2;view.importSource(false);}
            else if(harness.phase===2 && view.selectedAsset) {
                console.log("T07_SOURCE",JSON.stringify(view.selectedAsset));
                view.durationSeconds=0.5;view.volume=0.5;harness.phase=3;view.convert(false);
            } else if(harness.phase===3 && view.selectedOutput && view.selectedOutput.status === "succeeded") {
                harness.firstHash=view.selectedOutput.output_sha256;console.log("T07_OUTPUT_R1",JSON.stringify(view.selectedOutput));
                view.volume=1;harness.phase=4;view.convert(true);
            } else if(harness.phase===4 && view.selectedOutput && view.selectedOutput.revision===2 && view.selectedOutput.status === "succeeded") {
                if(view.selectedOutput.output_sha256===harness.firstHash){harness.fail("Revised volume did not alter output");return;}
                console.log("T07_OUTPUT_R2",JSON.stringify(view.selectedOutput));harness.phase=5;view.exportOutput();
            } else if(harness.phase===5 && view.lastExport) {
                if(view.outputs.length!==2){harness.fail("Version history missing");return;}
                if(!view.capabilities.some(c=>c.domain==="game" && !c.supported)){harness.fail("Unsupported breadth missing");return;}
                console.log("T07_EXPORT",JSON.stringify(view.lastExport));harness.phase=6;
                view.grabToImage(function(image){
                    if (!image.saveToFile(Quickshell.env("T07_PREVIEW"))) {harness.fail("Preview save failed");return;}
                    console.log("T07_NATIVE_PASS",Backend.ownerRoot);Qt.quit();
                });
            } else if(harness.phase===10 && view.outputs.length===2 && view.selectedAsset) {
                if(view.outputs.some(o=>o.status!=="succeeded")){harness.fail("Restart lost completion status");return;}
                console.log("T07_RESTART_HISTORY",JSON.stringify(view.outputs));console.log("T07_NATIVE_RESTART_PASS",Backend.ownerRoot);Qt.quit();
            }
            if(view.errorText)harness.fail(view.errorText);
        }
    }
    Timer {interval:15000;running:true;onTriggered:harness.fail("Timed out in phase "+harness.phase)}
}

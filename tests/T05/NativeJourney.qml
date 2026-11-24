import QtQuick
import Quickshell
import "../../ui"
ShellRoot {
 id: test
 property int phase:0
 property string firstId:""
 property bool launched:false
 function fail(e){console.error("T05_NATIVE_FAIL",e);Qt.quit()}
 FloatingWindow {
  visible:true;implicitWidth:1280;implicitHeight:920;title:"Q-ROC PHOD T05 owned journey";color:Theme.base
  ModulePhod {id:view;anchors.fill:parent;anchors.margins:12}
 }
 Timer {
  interval:100;running:true;repeat:true
  onTriggered:{
   if(!Backend.ready)return
   if(!test.launched){
    test.launched=true;view.refreshJourney();return
   }
   if(view.journeyError.length){test.fail(view.journeyError);return}
   if(test.phase===0&&view.moduleInfo){
    if(!view.moduleInfo.capabilities.workbench.available){test.fail("Real workbench unavailable");return}
    view.intentName="native-score-validator";view.intentPurpose="Reject invalid local scores; inspect the source proposal";
    view.lowerBound="0";view.upperBound="100";view.upperPolicy="inclusive";view.actor="Native T05 development owner";view.rationale="100 valid, 101 rejected";view.trialUpper="101";view.repairEnabled=true;
    view.startJourney();test.phase=1;
   } else if(test.phase===1&&view.selectedJob&&view.selectedJob.state==="succeeded"){
    var report=view.report();
    if(!report||!report.passed||report.attempts.length!==3||report.attempts[0].decision.status!=="rejected"||report.attempts[2].decision.status!=="accepted"){test.fail("Retained view did not expose failed and repaired receipts");return}
    if(report.repair.counterexamples[0].input!==101){test.fail("Wrong counterexample");return}
    test.firstId=view.selectedJob.id;
    console.log("T05_NATIVE_COMPARE_REPAIR",JSON.stringify(report));
    var art=view.artifacts().filter(function(a){return a.path.indexOf("repaired/candidate.rs")>=0})[0];
    if(!art){test.fail("No real Rust artifact");return}
    view.readArtifact(art);test.phase=2;
   } else if(test.phase===2&&view.artifactText.length){
    if(view.artifactText.indexOf("if value > 100")<0){test.fail("Repaired artifact does not retain upper bound");return}
    console.log("T05_NATIVE_ARTIFACT",view.artifactPath,view.artifactText);
    test.phase=3;
    view.journeyRequest("phod-replay",{id:test.firstId},function(r){
     if(r.effectful_rerun||r.decisions.length!==3||r.decisions[0].decision.status!=="rejected"){test.fail("Replay lost failure or reran tools");return}
     console.log("T05_NATIVE_REPLAY",JSON.stringify(r.decisions));
     console.log("T05_NATIVE_PASS",Backend.ownerRoot,Backend.executable,test.firstId);
     Qt.quit();
    });
   }
  }
 }
 Timer {interval:25000;running:true;onTriggered:test.fail("Journey timeout in phase "+test.phase)}
}

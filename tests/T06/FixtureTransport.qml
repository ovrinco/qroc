import QtQuick
import "AdmittedFixtures.js" as Fixtures

QtObject {
    id:root
    property bool ready:true
    property var requests:[]
    property var queue:[]
    property var stored:null
    property bool refuseSave:false
    property string previewMode:''
    property string getMode:''
    property string providerMode:'configured'
    property bool scopeFlag:true
    property bool registryFlag:true
    property bool hold:false
    function clone(x){return JSON.parse(JSON.stringify(x))}
    function send(command,args,callback) {
        requests=requests.concat([{command:command,args:clone(args)}]);
        queue=queue.concat([{command:command,args:clone(args),callback:callback}]);timer.restart();
    }
    function record(key,args) {
        var r=clone(Fixtures.records[key]);r.project_id=args.project_id;r.branch_id=args.branch_id;
        // Identity rebind and explicitly requested selections are labelled fixture transformations.
        r.nature=args.nature;r.rationale=args.rationale;r.constraints=args.constraints||[];
        if(args.included!==undefined)r.included=args.included.filter(t=>!Object.prototype.hasOwnProperty.call(args.excluded||{},t));
        r.excluded=Object.assign({},r.excluded,args.excluded||{});return r;
    }
    function flush() {
        if(hold)return;
        var old=queue;queue=[];old.forEach(function(q){
            var r=null,ok=true,error='';
            if(q.command==='bridge-capabilities')r={version:1,scope:scopeFlag,registry:registryFlag};
            else if(q.command==='qroc-registry-catalog')r=clone(Fixtures.records[q.command]);
            else if(q.command==='qroc-registry-type'){var found=Fixtures.records['qroc-registry-catalog'].types.find(t=>t.id===q.args.document_type);if(found)r=clone(found);else{ok=false;error='Unknown Q-ROC document type';}}
            else if(q.command==='qroc-registry-provider-capabilities'){
                r=clone(Fixtures.records[q.command+(providerMode==='unavailable'?'-unavailable':'-configured')]);
                if(providerMode==='bad-endpoint')r.endpoint='10.0.0.1:9000';
                if(providerMode==='missing-model')delete r.model;
            } else if(q.command==='qroc-scope-get') {
                r={scope:stored?clone(stored):null};
                if(getMode==='direct')r=stored?clone(stored):{};
                if(getMode==='wrong-owner'&&r.scope)r.scope.branch_id='another-branch';
            } else if(q.command==='qroc-scope-preview') {
                r=record(q.command,q.args);
                if(previewMode==='wrapped')r={scope:r};
                if(previewMode==='revision')r.revision=0;
                if(previewMode==='authority')r.execution_authority=true;
                if(previewMode==='constraints-loss')r.constraints=[];
            } else if(q.command==='qroc-scope-save') {
                if(refuseSave){ok=false;error='Scope changed; exact source_revision required';}
                else {r=record(q.command,q.args);r.revision=q.args.source_revision+1;stored=clone(r);}
            } else {ok=false;error='SCHEMA FIXTURE: unsupported command';}
            q.callback(r,ok,error);
        });
    }
    property Timer timer:Timer {interval:1;onTriggered:root.flush()}
}

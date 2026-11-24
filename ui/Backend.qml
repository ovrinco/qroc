pragma Singleton
import QtQuick
import Quickshell
import Quickshell.Io

QtObject {
    id: backend
    property string ownerRoot: Quickshell.env('QROC_ROOT')
    property string executable: Quickshell.env('QROC_BRIDGE')
    property bool ready: false
    property bool failed: false
    property bool restored: false
    property bool restoringRoute: false
    property bool transportReady: false
    property string startupId: ''
    property double startupDeadline: Date.now() + 30000
    property var prefs: ({})
    property var pending: ({})
    property var queue: []
    property int serial: 0
    property string errorText: ''
    signal routeLeaving()
    signal replyObserved(string id, bool ok)
    function isRecord(value) { return value !== null && typeof value === 'object' && !Array.isArray(value) }
    function invoke(callback, result, ok, error) {
        if (!callback) return;
        try { callback(result, ok, error) } catch (e) { console.warn('Q-ROC callback failed:', e) }
    }
    function remember() {
        if (!ready || !restored || failed) return false;
        send('bridge-prefs-save', {prefs:prefs}, function(){});return true;
    }
    function setting(key, value) {
        if (!ready || !restored || failed) return false;
        var next=Object.assign({},prefs);next[key]=value;prefs=next;return remember();
    }
    function failAll(error) {
        if (failed) return;
        // The engine's own reason goes to the log; the window says what happened and what to do, in words.
        console.warn('QROC engine stopped:', error);
        errorText=/timed out|deadline/.test(String(error))?'QROC did not answer in time; close and reopen QROC, then check the project before trying again':'QROC stopped; close and reopen QROC';
        ready=false;restored=false;transportReady=false;failed=true;startupId='';
        var old=pending;pending={};queue=[];
        Object.keys(old).forEach(function(id){backend.invoke(old[id].callback,null,false,backend.errorText)});
    }
    function stop(error) { failAll(error);process.running=false }
    function send(command, args, callback, quiet) {
        if (!ownerRoot || !executable) { invoke(callback,null,false,'QROC_ROOT and QROC_BRIDGE must be explicit');return }
        if (failed) { invoke(callback,null,false,errorText || 'QROC stopped; close and reopen QROC');return }
        // Never replay a stale/default presentation snapshot after restoration.
        if (command === 'bridge-prefs-save' && !restored) { invoke(callback,null,false,'Preferences are not restored; startup writes refused');return }
        if (Object.keys(pending).length >= 64) { invoke(callback,null,false,'QROC is busy; try again in a moment');return }
        var id='qml-'+(++serial), frame;
        try { frame=JSON.stringify({version:1,id:id,command:command,args:args||{}})+'\n' }
        catch(e) { invoke(callback,null,false,'Request is not JSON serializable');return }
        if(frame.length>1048576) { invoke(callback,null,false,'QML request exceeds 1 MiB');return }
        var next=Object.assign({},pending);next[id]={callback:callback,deadline:Date.now()+30000};pending=next;
        if(ready && restored)process.write(frame);else queue.push(frame);
    }
    function receive(line) {
        if (failed) return;
        var message;
        try {message=JSON.parse(line)} catch(e) {stop('Invalid bridge reply');return}
        if(!isRecord(message) || message.version!==1) {stop('Bridge protocol version mismatch');return}
        if(message.event==='ready') {
            if(transportReady || restored || message.root!==ownerRoot) {stop('Bridge ready/root identity mismatch');return}
            transportReady=true;startupId='startup-prefs-'+(++serial);
            // This internal read is the first request written to the service.
            process.write(JSON.stringify({version:1,id:startupId,command:'bridge-prefs-get',args:{}})+'\n');return;
        }
        if(typeof message.id!=='string' || typeof message.ok!=='boolean'
                || (message.ok && message.error!==null)
                || (!message.ok && (typeof message.error!=='string' || message.result!==null))) {
            stop('Malformed bridge reply envelope');return;
        }
        if(message.id===startupId && !restored) {
            if(!message.ok) {stop('Initial prefs read failed: '+message.error);return}
            if(!isRecord(message.result) || Object.keys(message.result).length!==1
                    || !Object.prototype.hasOwnProperty.call(message.result,'prefs')
                    || !isRecord(message.result.prefs)) {stop('Initial prefs result must contain exactly one prefs object');return}
            var expired=Date.now()>=startupDeadline || Object.keys(pending).some(function(id){return backend.pending[id].deadline<=Date.now()});
            if(expired) {stop('Startup/pending request timed out; no queue replay');return}
            // Replace any startup defaults with the verified initial object before
            // restoredChanged, readyChanged, UI callbacks or writes can occur.
            prefs=JSON.parse(JSON.stringify(message.result.prefs));startupId='';restored=true;
            ready=true;
            var frames=queue;queue=[];
            frames.forEach(function(frame){if(!backend.failed)backend.process.write(frame)});return;
        }
        if(!pending[message.id])return;
        if(!ready || !restored) {stop('Domain reply arrived before initial prefs restoration');return}
        if(pending[message.id].deadline<=Date.now()) {stop('Bridge reply arrived after request deadline; completion may be uncertain. Inspect owned state before retry.');return}
        var cb=pending[message.id].callback,next=Object.assign({},pending);delete next[message.id];pending=next;
        replyObserved(message.id,message.ok);
        invoke(cb,message.result,message.ok,message.error||'');
    }
    property Process process: Process {
        command: [backend.executable,'--root',backend.ownerRoot]
        stdinEnabled: true
        running: !!backend.executable && !!backend.ownerRoot
        stdout: SplitParser { splitMarker: "\n"; onRead: data => backend.receive(data) }
        stderr: SplitParser { splitMarker: "\n"; onRead: data => console.warn('QROC engine:',data) }
        onExited: (exitCode, exitStatus) => backend.failAll('Bridge exited ('+exitCode+'); relaunch the shell')
    }
    property Timer timeout: Timer {
        interval:1000;repeat:true;running:!backend.failed
        onTriggered: {
            var expired=Object.keys(backend.pending).some(function(id){return backend.pending[id].deadline<=Date.now()});
            if((!backend.restored && Date.now()>=backend.startupDeadline) || expired) {
                backend.stop('Bridge request timed out; completion may be uncertain. Inspect owned state before retry.');
            }
        }
    }
    Component.onDestruction: { process.stdinEnabled=false;process.running=false }
}

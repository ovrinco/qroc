import QtQuick
import ".."
import "Semantics.js" as Semantic

Item {
    id: root
    // Test injection is owned development evidence, never the application's default.
    property var transport: Backend
    property string projectId: ''
    property string branchId: ''
    property var capabilities: ({scope:false,registry:false})
    property var catalog: null
    property var provider: null
    property var typeProfile: null
    property var semanticProfile: null
    property string semanticStatus: 'Semantic profiles unavailable'
    property var savedScope: null
    property var preview: null
    property bool scopeReadKnown: false
    property bool stale: false
    property bool saving: false
    property bool previewing: false
    property bool loadingScope: false
    property string errorText: ''
    property string scopeStatus: 'Scope selection unavailable'
    property string registryStatus: 'Document registry unavailable'
    property string providerStatus: 'Provider discovery unavailable; generation disabled'
    property string nature: 'internal'
    property string rationale: ''
    property bool useDefaults: true
    property string includedText: ''
    property string excludedText: ''
    property string constraintsText: ''
    property int ownerEpoch: 0
    property int serial: 0
    property var tickets: ({})
    property string previewFingerprint: ''
    readonly property bool ownerSelected: !!projectId && !!branchId
    readonly property bool scopeAvailable: capabilities.scope === true
    readonly property bool registryAvailable: capabilities.registry === true
    readonly property int sourceRevision: savedScope ? savedScope.revision : 0
    readonly property bool canSave: scopeAvailable && scopeReadKnown && !loadingScope && !saving && !stale && !!preview && previewFingerprint === fingerprint()
    signal scopeChanged()

    function clone(value) { return JSON.parse(JSON.stringify(value)); }
    function array(value) { return Array.isArray(value); }
    function strings(value) { return array(value) && value.every(x=>typeof x==='string'); }
    function integer(value) { return typeof value==='number' && Number.isInteger(value); }
    function identity(value) { return typeof value==='string' && /^[A-Za-z0-9_-]{1,200}$/.test(value); }
    function schemaError(message) {
        errorText='Unexpected authoring response: '+message;
        if(message.indexOf('provider')>=0){provider=null;providerStatus='Provider configuration could not be validated; generation unavailable';}
        if(message==='document catalogue'){catalog=null;registryStatus='Document catalogue could not be validated';}
    }
    function call(key, command, args, done) {
        var ticket=++serial, epoch=ownerEpoch;
        var next=Object.assign({},tickets);next[key]=ticket;tickets=next;
        transport.send(command,args,function(r,ok,error){
            if((['capabilities','catalog','provider','semantic-profile'].indexOf(key)<0 && epoch!==ownerEpoch) || tickets[key]!==ticket)return;
            done(r,ok,error||'Request failed');
        });
    }
    function ownerChanged() {
        ownerEpoch++;tickets={capabilities:tickets.capabilities,catalog:tickets.catalog,provider:tickets.provider,'semantic-profile':tickets['semantic-profile']};savedScope=null;preview=null;previewFingerprint='';scopeReadKnown=false;
        saving=false;previewing=false;loadingScope=false;stale=false;typeProfile=null;errorText='';
        nature='internal';rationale='';useDefaults=true;includedText='';excludedText='';constraintsText='';
        scopeStatus=scopeAvailable?'Choose a branch scope':'Scope selection unavailable';
        ownerTimer.restart();
    }
    onProjectIdChanged: ownerChanged()
    onBranchIdChanged: ownerChanged()
    Timer {id:ownerTimer;interval:0;onTriggered:if(root.ownerSelected&&root.scopeAvailable)root.loadScope(false)}
    Connections {target:root.transport;ignoreUnknownSignals:true;function onReadyChanged(){if(root.transport.ready)root.discover()}}
    Component.onCompleted: if(transport.ready)discover()

    function discover() {
        provider=null;catalog=null;typeProfile=null;semanticProfile=null;semanticStatus='Semantic profiles unavailable until exact source verification';capabilities={scope:false,registry:false};scopeReadKnown=false;preview=null;previewFingerprint='';
        providerStatus='Reading provider configuration · generation disabled';registryStatus='Reading document capabilities…';
        call('capabilities','bridge-capabilities',{},function(r,ok,error){
            if(!ok){root.errorText=error;return;}
            if(!r||r.version!==1||typeof r.scope!=='boolean'||typeof r.registry!=='boolean'){root.schemaError('capability flags');return;}
            root.capabilities={scope:r.scope,registry:r.registry};
            root.catalog=null;root.provider=null;root.typeProfile=null;
            root.registryStatus=r.registry?'Reading document profiles…':'Document registry unavailable; reference selection remains manual';
            root.providerStatus=r.registry?'Reading provider configuration…':'Provider discovery unavailable; generation disabled';
            root.scopeStatus=r.scope?'Choose a branch scope':'Scope selection unavailable; no scope was assumed';
            if(r.registry){root.loadCatalog();root.loadProvider();}
            if(r.scope&&root.ownerSelected)root.loadScope(false);
        });
    }
    // The engine names seven canonical domains (src/registry.rs DOMAIN_ORDER); earlier records use the four legacy names.
    readonly property var engineDomains: ['PRODUCT','DISTRIBUTION','ECOSYSTEM','BRAND','DATA','COMMUNITY','CAPITAL','BUSINESS','PRESENCE','LEGAL']
    function validType(t) {
        return t && typeof t.id==='string' && engineDomains.indexOf(t.domain)>=0
            && integer(t.order)&&t.order>=0 && t.source==='src/qroc.rs:qroc_prompt/document_template'
            && t.available===true && Semantic.typeFields(t)
            && t.input_policy && ['first','middle'].indexOf(t.input_policy.stage)>=0
            && strings(t.input_policy.allowed_upstream)&&t.input_policy.forbid_forward_references===true&&t.input_policy.synthesis_only===false
            && ['native-document-draft','native-score-criteria-review'].indexOf(t.output_contract)>=0
            && (t.id.indexOf('GATE_')===0 ? (t.gate_threshold===90 || t.gate_threshold===92) : t.gate_threshold===null);
    }
    function loadCatalog() {
        call('catalog','qroc-registry-catalog',{},function(r,ok,error){
            if(!ok){root.registryStatus=error;root.errorText=error;return;}
            if(!r||r.version!=='qroc-native-registry/1'||r.source!=='src/qroc.rs'||r.execution_authority!==false
                ||!array(r.types)||!r.types.every(t=>root.validType(t))||(new Set(r.types.map(t=>t.id))).size!==r.types.length||!r.semantic_force_registry
                ||!Semantic.registry(r.semantic_force_registry)){root.schemaError('document catalogue');return;}
            root.catalog=clone(r);
            if(r.semantic_force_registry.available===true){root.registryStatus='Bounded retained market review declared · complete catalogue unavailable';root.loadSemanticProfile();}
            else {root.registryStatus='Native draft profiles · semantic profiles unavailable';root.semanticProfile=null;root.semanticStatus='Semantic profiles unavailable';}
        });
    }
    function loadSemanticProfile() {
        semanticProfile=null;semanticStatus='Verifying exact retained market-review source bytes…';
        call('semantic-profile','qroc-registry-semantic-profile',{profile:'retained-market-review/1'},function(r,ok,error){
            if(!ok){root.semanticStatus='Bounded profile unavailable: '+error;root.errorText=error;return;}
            if(!Semantic.verifiedProfile(r)){root.semanticStatus='Exact semantic source bytes or identity mismatch; review unavailable';root.schemaError('semantic source profile');return;}
            root.semanticProfile=clone(r);root.semanticStatus='Retained market review only · complete catalogue unavailable · human semantic judgment separate';
        });
    }
    function semanticDeclared(type) {
        return !!catalog&&catalog.semantic_force_registry.available===true&&catalog.types.some(t=>t.id===type&&t.semantic_force_profile_available===true);
    }
    function semanticSupported(type) {return semanticDeclared(type)&&!!semanticProfile;}
    function loadType(type) {
        typeProfile=null;
        if(!registryAvailable||!type)return;
        var requested=type.trim().toUpperCase();
        call('type','qroc-registry-type',{document_type:requested},function(r,ok,error){
            if(!ok){root.errorText=error;return;}
            if(!root.validType(r)||r.id!==requested||(root.catalog&&!root.catalog.types.some(t=>t.id===r.id&&t.semantic_force_profile_available===r.semantic_force_profile_available&&Semantic.equal(t.semantic_model,r.semantic_model)))){root.schemaError('document profile');return;}
            root.typeProfile=clone(r);
        });
    }
    function loopbackEndpoint(endpoint) {
        if(typeof endpoint!=='string')return false;
        var match=/^(127(?:\.\d{1,3}){3}|localhost|\[::1\]):([1-9]\d{0,4})$/.exec(endpoint);
        return !!match && Number(match[2])<=65535 && (match[1].indexOf('127.')!==0 || match[1].split('.').every(n=>Number(n)<=255));
    }
    function loadProvider() {
        provider=null;providerStatus='Reading provider configuration · generation disabled';
        call('provider','qroc-registry-provider-capabilities',{},function(r,ok,error){
            if(!ok){root.providerStatus=error;root.errorText=error;return;}
            if(!r||typeof r.provider!=='string'||! /^[a-z0-9-]{1,64}$/.test(r.provider)||typeof r.available!=='boolean'){root.schemaError('provider configuration');return;}
            if(r.available){
                if(typeof r.model!=='string'||!r.model.trim()||typeof r.endpoint!=='string'
                    ||!root.loopbackEndpoint(r.endpoint)
                    ||r.standing!=='explicitly-configured; reachability/inference unverified'){root.schemaError('configured provider');return;}
                root.provider=clone(r);root.providerStatus='Configured: '+r.model+' · reachability and inference unverified';
            } else {
                if(typeof r.reason!=='string'||!r.reason.length){root.schemaError('unavailable provider');return;}
                root.provider=clone(r);root.providerStatus=r.reason+' · generation unavailable';
            }
        });
    }
    function branchConfigured(branch) {
        return !!provider && provider.available===true && !!branch && branch.provider===provider.provider && branch.model===provider.model;
    }
    function validScope(r,saved) {
        if(!r||r.schema!=='qroc-scope/1'||r.registry_version!=='qroc-native-registry/1'||r.project_id!==projectId||r.branch_id!==branchId
            ||['internal','research','creative','external-product'].indexOf(r.nature)<0||typeof r.rationale!=='string'||!r.rationale.trim()
            ||r.input_policy!=='selected-and-verified'||r.execution_authority!==false||!strings(r.included)
            ||(new Set(r.included)).size!==r.included.length||!r.excluded||typeof r.excluded!=='object'||array(r.excluded)
            ||!Object.keys(r.excluded).every(k=>typeof r.excluded[k]==='string'&&!!r.excluded[k].trim())
            ||!strings(r.constraints)||!array(r.document_refs)||!array(r.gate_reviews)
            ||!r.included.every(t=>!Object.prototype.hasOwnProperty.call(r.excluded,t)))return false;
        if(saved) {
            return integer(r.revision)&&r.revision>0&&r.document_refs.every(d=>d&&identity(d.id)&&typeof d.document_type==='string'&&integer(d.revision)&&d.revision>0&&typeof d.sha256==='string'&&/^[a-f0-9]{64}$/.test(d.sha256))
                && r.gate_reviews.every(g=>g&&identity(g.document_id)&&g.branch_id===branchId&&typeof g.document_type==='string'&&integer(g.revision)&&g.revision>0&&typeof g.sha256==='string'&&/^[a-f0-9]{64}$/.test(g.sha256)&&['passed','failed'].indexOf(g.decision)>=0);
        }
        return r.revision===undefined&&r.document_refs.length===0&&r.gate_reviews.length===0;
    }
    function selectionMatches(r,args) {
        if(r.nature!==args.nature||r.rationale!==args.rationale||r.input_policy!==args.input_policy
            ||JSON.stringify(r.constraints)!==JSON.stringify(args.constraints))return false;
        if(!Object.keys(args.excluded).every(k=>r.excluded[k]===args.excluded[k]))return false;
        if(args.included!==undefined) {
            var expected=Array.from(new Set(args.included.filter(t=>!Object.prototype.hasOwnProperty.call(args.excluded,t)))).sort();
            if(JSON.stringify(r.included.slice().sort())!==JSON.stringify(expected))return false;
        }
        return true;
    }
    function applySaved() {
        if(!savedScope)return;
        nature=savedScope.nature;rationale=savedScope.rationale;useDefaults=false;includedText=savedScope.included.join(', ');
        excludedText=Object.keys(savedScope.excluded).map(k=>k+': '+savedScope.excluded[k]).join('\n');constraintsText=savedScope.constraints.join('\n');invalidatePreview();
    }
    function loadScope(preserveEdits) {
        if(!scopeAvailable||!ownerSelected)return;
        loadingScope=true;preview=null;previewFingerprint='';scopeReadKnown=false;
        call('scope-get','qroc-scope-get',{project_id:projectId,branch_id:branchId},function(r,ok,error){
            root.loadingScope=false;
            if(!ok){root.scopeStatus=error;root.errorText=error;return;}
            if(!r||Object.keys(r).length!==1||!Object.prototype.hasOwnProperty.call(r,'scope')||(r.scope!==null&&!root.validScope(r.scope,true))){root.schemaError('scope readback wrapper or identity');return;}
            root.savedScope=r.scope===null?null:clone(r.scope);root.scopeReadKnown=true;root.stale=false;root.errorText='';
            root.scopeStatus=r.scope?'Saved scope revision '+r.scope.revision:'No saved scope · preview a selection';
            if(!preserveEdits&&r.scope)root.applySaved();
            root.scopeChanged();
        });
    }
    function invalidatePreview() { preview=null;previewFingerprint=''; }
    onNatureChanged:invalidatePreview()
    onRationaleChanged:invalidatePreview()
    onUseDefaultsChanged:invalidatePreview()
    onIncludedTextChanged:invalidatePreview()
    onExcludedTextChanged:invalidatePreview()
    onConstraintsTextChanged:invalidatePreview()
    function selection() {
        if(!ownerSelected)throw new Error('Select an actual project and branch');
        if(!rationale.trim())throw new Error('Explain why this scope is relevant');
        if(unescape(encodeURIComponent(rationale)).length>8192)throw new Error('Scope rationale exceeds 8192 bytes');
        var excluded={};excludedText.split('\n').filter(x=>x.trim()).forEach(function(line){
            var split=line.indexOf(':');if(split<1||!line.slice(split+1).trim())throw new Error('Each exclusion needs TYPE: reason');
            excluded[line.slice(0,split).trim().toUpperCase()]=line.slice(split+1).trim();
        });
        var args={project_id:projectId,branch_id:branchId,nature:nature,rationale:rationale.trim(),excluded:excluded,
            constraints:constraintsText.split('\n').map(x=>x.trim()).filter(x=>x),input_policy:'selected-and-verified'};
        if(!useDefaults)args.included=includedText.split(',').map(x=>x.trim().toUpperCase()).filter(x=>x);
        return args;
    }
    function fingerprint() { try{return JSON.stringify(selection());}catch(e){return '';} }
    function previewScope() {
        if(!scopeAvailable||previewing||saving)return;
        var args;try{args=selection();}catch(e){errorText=e.message;return;}
        preview=null;previewFingerprint='';previewing=true;var key=JSON.stringify(args);
        call('scope-preview','qroc-scope-preview',args,function(r,ok,error){
            root.previewing=false;
            if(key!==root.fingerprint())return;
            if(!ok){root.errorText=error;return;}
            if(!root.validScope(r,false)||!root.selectionMatches(r,args)){root.schemaError('direct scope preview');return;}
            root.preview=clone(r);root.previewFingerprint=key;root.errorText='';root.scopeStatus='Preview only · no scope was saved';
        });
    }
    function saveScope() {
        if(!canSave){errorText=stale?'Scope changed. Reload and compare before previewing and saving again.':'Read current scope and preview this exact selection before saving';return;}
        var args=selection(), key=previewFingerprint;args.source_revision=sourceRevision;saving=true;
        call('scope-save','qroc-scope-save',args,function(r,ok,error){
            root.saving=false;
            if(!ok){root.errorText=error;root.stale=true;root.preview=null;root.previewFingerprint='';root.scopeStatus='Save refused or uncertain · reload and compare; no retry was sent';return;}
            if(!root.validScope(r,true)||!root.selectionMatches(r,args)||r.revision!==args.source_revision+1){root.stale=true;root.preview=null;root.previewFingerprint='';root.scopeStatus='Saved response is uncertain · reload and compare; no retry was sent';root.schemaError('direct saved scope revision');return;}
            root.savedScope=clone(r);root.scopeReadKnown=true;root.stale=false;root.preview=null;root.previewFingerprint='';root.errorText='';
            root.scopeStatus='Saved scope revision '+r.revision+(key!==root.fingerprint()?' · current edits are not saved':'');root.scopeChanged();
        });
    }
    function documentsByDomain(reference) {
        var result={PRODUCT:[],BUSINESS:[],PRESENCE:[],LEGAL:[]};
        // The window keeps its four groups: each catalogued type joins the group that lists it, in catalogue order.
        if(catalog)catalog.types.slice().sort((a,b)=>a.order-b.order).forEach(t=>{var group=Object.keys(reference).find(k=>reference[k].indexOf(t.id)>=0)||(result[t.domain]?t.domain:'');if(group)result[group].push(t.id);});
        else Object.keys(reference).forEach(k=>result[k]=reference[k].slice());
        if(savedScope)Object.keys(result).forEach(k=>result[k]=result[k].filter(t=>savedScope.included.indexOf(t)>=0));
        return result;
    }
    function documentIncluded(type) { return !savedScope || savedScope.included.indexOf(type)>=0; }
    function generationAllowed(type) { return documentIncluded(type) && (!scopeAvailable || (scopeReadKnown && !!savedScope && !stale && !saving && !loadingScope)); }
}

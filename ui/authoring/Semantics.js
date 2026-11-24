.pragma library
.import "SemanticSource.js" as Source

function bytes(text) {return unescape(encodeURIComponent(text));}
function sha256(text) {
    var s=bytes(text),n=s.length,words=[],k=[0x428a2f98,0x71374491,0xb5c0fbcf,0xe9b5dba5,0x3956c25b,0x59f111f1,0x923f82a4,0xab1c5ed5,0xd807aa98,0x12835b01,0x243185be,0x550c7dc3,0x72be5d74,0x80deb1fe,0x9bdc06a7,0xc19bf174,0xe49b69c1,0xefbe4786,0x0fc19dc6,0x240ca1cc,0x2de92c6f,0x4a7484aa,0x5cb0a9dc,0x76f988da,0x983e5152,0xa831c66d,0xb00327c8,0xbf597fc7,0xc6e00bf3,0xd5a79147,0x06ca6351,0x14292967,0x27b70a85,0x2e1b2138,0x4d2c6dfc,0x53380d13,0x650a7354,0x766a0abb,0x81c2c92e,0x92722c85,0xa2bfe8a1,0xa81a664b,0xc24b8b70,0xc76c51a3,0xd192e819,0xd6990624,0xf40e3585,0x106aa070,0x19a4c116,0x1e376c08,0x2748774c,0x34b0bcb5,0x391c0cb3,0x4ed8aa4a,0x5b9cca4f,0x682e6ff3,0x748f82ee,0x78a5636f,0x84c87814,0x8cc70208,0x90befffa,0xa4506ceb,0xbef9a3f7,0xc67178f2];
    var h=[0x6a09e667,0xbb67ae85,0x3c6ef372,0xa54ff53a,0x510e527f,0x9b05688c,0x1f83d9ab,0x5be0cd19];
    function rotate(x,n){return (x>>>n)|(x<<(32-n));}
    for(var i=0;i<n;i++)words[i>>2]=(words[i>>2]||0)|(s.charCodeAt(i)<<(24-(i%4)*8));
    words[n>>2]=(words[n>>2]||0)|(0x80<<(24-(n%4)*8));var end=(((n+8)>>6)+1)*16;words[end-2]=Math.floor(n/0x20000000);words[end-1]=(n*8)|0;
    for(var offset=0;offset<end;offset+=16){
        var w=[],a=h[0],b=h[1],c=h[2],d=h[3],e=h[4],f=h[5],g=h[6],q=h[7];
        for(i=0;i<64;i++){
            if(i<16)w[i]=words[offset+i]||0;else {var x=w[i-15],y=w[i-2];w[i]=(w[i-16]+(rotate(x,7)^rotate(x,18)^(x>>>3))+w[i-7]+(rotate(y,17)^rotate(y,19)^(y>>>10)))|0;}
            var t1=(q+(rotate(e,6)^rotate(e,11)^rotate(e,25))+((e&f)^(~e&g))+k[i]+w[i])|0;
            var t2=((rotate(a,2)^rotate(a,13)^rotate(a,22))+((a&b)^(a&c)^(b&c)))|0;
            q=g;g=f;f=e;e=(d+t1)|0;d=c;c=b;b=a;a=(t1+t2)|0;
        }
        var v=[a,b,c,d,e,f,g,q];for(i=0;i<8;i++)h[i]=(h[i]+v[i])|0;
    }
    return h.map(x=>('00000000'+(x>>>0).toString(16)).slice(-8)).join('');
}
function equal(a,b) {
    if(a===b)return true;if(a===null||b===null||typeof a!=='object'||typeof b!=='object')return false;
    if(Array.isArray(a)!==Array.isArray(b))return false;var keys=Object.keys(a);
    return keys.length===Object.keys(b).length&&keys.every(k=>Object.prototype.hasOwnProperty.call(b,k)&&equal(a[k],b[k]));
}
function integer(n){return Number.isInteger(n)&&n>=0&&n<=4294967295;}
function digest(s){return typeof s==='string'&&/^[a-f0-9]{64}$/.test(s);}
function has(o,k){return !!o&&Object.prototype.hasOwnProperty.call(o,k);}
function model(type){return Source.profile.documents.find(x=>x.documentType===type)||Source.profile.gates.find(x=>x.gateType===type)||null;}
function typeFields(t) {
    if(t.semantic_force_profile_available===false)return !has(t,'semantic_profile')&&!has(t,'semantic_model');
    return t.semantic_force_profile_available===true&&!!model(t.id)&&t.semantic_profile===Source.profile.id&&equal(t.semantic_model,model(t.id));
}
function registry(r) {
    if(!r||typeof r.reason!=='string'||!r.reason.trim())return false;
    if(r.available===false)return !has(r,'supported_profiles')&&!has(r,'complete_catalogue_available');
    return r.available===true&&equal(r.supported_profiles,[Source.profile.id])&&r.complete_catalogue_available===false;
}
function verifiedProfile(r) {
    if(!r||r.version!=='qroc-semantic-registry/1'||r.validated!==true||r.execution_authority!==false
        ||typeof r.profile_canonical_json!=='string'||r.profile_sha256!==Source.digest||!equal(r.sources,Source.sources))return false;
    try{return sha256(r.profile_canonical_json)===r.profile_sha256&&equal(JSON.parse(r.profile_canonical_json),r.profile)&&equal(r.profile,Source.profile);}catch(e){return false;}
}
function strictJson(text) {
    var i=0;
    function whitespace(){while(/\s/.test(text.charAt(i))&&i<text.length)i++;}
    function readString(){var start=i++;while(i<text.length){var ch=text.charAt(i++);if(ch==='"')return JSON.parse(text.slice(start,i));if(ch==='\\')i++;}throw new Error('Unterminated string');}
    function value(depth){
        if(depth>64)throw new Error('JSON nesting limit');whitespace();var ch=text.charAt(i),out;
        if(ch==='"')return readString();
        if(ch==='{'){
            i++;out=Object.create(null);whitespace();if(text.charAt(i)==='}'){i++;return out;}
            while(true){whitespace();if(text.charAt(i)!=='"')throw new Error('Expected key');var key=readString();if(has(out,key))throw new Error('Duplicate JSON key');whitespace();if(text.charAt(i++)!==':')throw new Error('Expected colon');out[key]=value(depth+1);whitespace();ch=text.charAt(i++);if(ch==='}')return out;if(ch!==',')throw new Error('Expected object end');}
        }
        if(ch==='['){i++;out=[];whitespace();if(text.charAt(i)===']'){i++;return out;}while(true){out.push(value(depth+1));whitespace();ch=text.charAt(i++);if(ch===']')return out;if(ch!==',')throw new Error('Expected array end');}}
        var token=/^(?:true|false|null|-?(?:0|[1-9]\d*)(?:\.\d+)?(?:[eE][+-]?\d+)?)/.exec(text.slice(i));if(!token)throw new Error('Invalid JSON value');i+=token[0].length;return JSON.parse(token[0]);
    }
    var result=value(0);whitespace();if(i!==text.length)throw new Error('Extra JSON data');return result;
}
function latest(docs,type){var all=docs.filter(d=>d.document_type===type);all.sort((a,b)=>b.revision-a.revision);return all[0]||null;}
function refsCurrent(semantic,docs,gate) {
    if(!semantic||!Array.isArray(semantic.reviewed_documents)||semantic.reviewed_documents.length!==2)return false;
    return Source.profile.gates[0].reviewedDocuments.every(function(type){
        var refs=semantic.reviewed_documents.filter(r=>r.document_type===type),head=latest(docs,type);if(refs.length!==1||!head)return false;var ref=refs[0];
        return head.project_id===gate.project_id&&head.branch_id===gate.branch_id&&ref.document_id===head.id&&ref.revision===head.revision&&ref.sha256===head.sha256&&digest(ref.sha256)&&sha256(head.content)===head.sha256;
    });
}
function evidence(entries,forces,docs,gate) {
    if(!Array.isArray(entries)||!entries.length)return false;
    return forces.every(force=>entries.some(e=>equal(e.force_ref,force)))&&entries.every(function(e){
        if(!forces.some(f=>equal(f,e.force_ref)))return false;var head=latest(docs,e.force_ref.documentType);
        return head&&head.project_id===gate.project_id&&head.branch_id===gate.branch_id&&e.document_id===head.id&&e.revision===head.revision&&e.sha256===head.sha256&&typeof e.quote==='string'&&!!e.quote.trim()&&head.content.indexOf(e.quote)>=0;
    });
}
function inspect(result,gate,docs,profile) {
    function bad(reason){return {valid:false,full:false,reason:reason,criteria:[],consistency:[]};}
    if(!profile||!verifiedProfile(profile))return bad('Bounded source profile is unverified; semantic review unavailable');
    if(!result||result.document_type!=='GATE_4'||['passed','failed','incomplete'].indexOf(result.status)<0||result.threshold!==90||!integer(result.evaluated_at)||typeof result.reason!=='string'||!result.reason.trim()||!result.semantic)return bad('Malformed bounded gate result');
    var sem=result.semantic,source=Source.profile.gates[0];
    if(sem.profile!==Source.profile.id||typeof sem.standing!=='string'||!sem.standing.trim())return bad('Wrong semantic profile or missing standing');
    if(Object.keys(sem).length===2&&has(sem,'profile')&&has(sem,'standing')) {
        if(result.status==='passed'||result.score!==null||!equal(result.criteria,{}))return bad('Short semantic failure cannot be a pass');
        return {valid:true,full:false,reason:sem.standing,criteria:[],consistency:[]};
    }
    if(result.status==='incomplete'||sem.profile_sha256!==Source.digest||!equal(sem.source_identity,Source.sources)||sem.standing!=='deterministic binding of a source-backed assessment; semantic judgment remains human')return bad('Missing or altered exact semantic source identity');
    if(!refsCurrent(sem,docs,gate))return bad('Reviewed source is changed, missing, corrupt or from another owner; reevaluate');
    if(!Array.isArray(sem.consistency)||sem.consistency.length!==source.consistencyRules.length)return bad('Missing retained consistency rule');
    if(!result.criteria||Object.keys(result.criteria).length!==source.criteria.length||!integer(result.score))return bad('Missing retained criterion or score');
    var rows=[],sum=0;
    for(var i=0;i<source.criteria.length;i++) {
        var criterion=source.criteria[i],c=result.criteria[criterion.id];
        if(!c||!integer(c.score)||c.max!==criterion.weight||c.score>c.max||typeof c.feedback!=='string')return bad('Criterion identity/weight mismatch');
        rows.push({model:criterion,score:c.score,max:c.max,feedback:c.feedback});sum+=c.score;
    }
    if(sum!==result.score)return bad('Criterion total mismatch');
    for(i=0;i<source.consistencyRules.length;i++) {
        var rule=source.consistencyRules[i],matches=sem.consistency.filter(x=>x.id===rule.id);if(matches.length!==1)return bad('Duplicate or missing consistency identity');var item=matches[0];
        if(['supported','contradicted','insufficient'].indexOf(item.status)<0||typeof item.feedback!=='string'||!equal(item.force_refs,[rule.left,rule.right])||!evidence(item.evidence,[rule.left,rule.right],docs,gate))return bad('Consistency force or exact quote binding mismatch');
    }
    if(result.status==='passed'&&(result.score<90||rows.some(c=>c.score*100<c.max*70)||sem.consistency.some(c=>c.status!=='supported')))return bad('A failing assessment cannot appear passed');
    // Criterion quote details reside in the exact saved JSON body, not GateResult.criteria.
    var body;try{body=strictJson(gate.content);}catch(e){return bad('Saved GATE_4 is not a strict JSON assessment');}
    if(sha256(gate.content)!==gate.sha256||body.schema!=='qroc-semantic-gate/1'||body.profile!==Source.profile.id||body.gate_type!=='GATE_4'||body.score!==result.score||!equal(body.reviewed_documents,sem.reviewed_documents)||!Array.isArray(body.criteria)||body.criteria.length!==rows.length||!equal(body.consistency,sem.consistency)||Object.keys(body).length!==8||!body.sections||!source.outputContract.requiredSections.every(k=>typeof body.sections[k]==='string'&&!!body.sections[k].trim()))return bad('Saved assessment does not match exact gate revision');
    for(i=0;i<rows.length;i++) {
        var row=rows[i],found=body.criteria.filter(c=>c.id===row.model.id);if(found.length!==1)return bad('Duplicate or missing saved criterion');var detail=found[0];
        if(detail.score!==row.score||detail.max!==row.max||detail.feedback!==row.feedback||!equal(detail.force_refs,row.model.derivedFromForces)||!evidence(detail.evidence,detail.force_refs,docs,gate))return bad('Criterion evidence does not match the exact owner/source');row.evidence=detail.evidence;
    }
    return {valid:true,full:true,reason:sem.standing,criteria:rows,consistency:sem.consistency};
}
function reviewCurrent(review,gate,docs,profile,result) {
    if(!review||!has(review,'semantic_inputs')||!result||!equal(review.semantic_inputs,result.semantic))return false;
    var inspection=inspect(result,gate,docs,profile);
    return inspection.valid&&(review.decision==='failed'||(result.status==='passed'&&inspection.full));
}

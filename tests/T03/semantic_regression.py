#!/usr/bin/env python3
"""Owner-bound semantic validation and token admission; protocol fixtures are not inference."""
import argparse,atexit,copy,hashlib,json,os,pathlib,socket,subprocess,sys,threading,time,traceback
ap=argparse.ArgumentParser();ap.add_argument('--binary',required=True);ap.add_argument('--scratch',required=True);ap.add_argument('--evidence',required=True);ap.add_argument('--profile-source',required=True);a=ap.parse_args()
root=pathlib.Path(a.scratch);root.mkdir(parents=True,exist_ok=False);trace=[];checks=[];processes=[];output=pathlib.Path(a.evidence)
def check(ok,name):assert ok,name;checks.append(name)
def cleanup():
    for p in processes:
        if p.poll() is None:
            p.stdin.close()
            try:p.wait(timeout=5)
            except subprocess.TimeoutExpired:p.terminate();p.wait(timeout=5)
atexit.register(cleanup)
def failure(kind,value,tb):output.write_text(json.dumps({'status':'failed','error':str(value),'checks':checks,'trace':trace},indent=2)+'\n');traceback.print_exception(kind,value,tb)
sys.excepthook=failure
class Engine:
    def __init__(self,name,env=None):
        self.n=0;self.state=root/name
        clean={k:v for k,v in os.environ.items() if not k.startswith(('QROC_','SOLITON_QROC_'))};clean.update(env or {})
        self.p=subprocess.Popen([a.binary,'--root',str(self.state)],stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=subprocess.PIPE,text=True,env=clean);processes.append(self.p);trace.append(json.loads(self.p.stdout.readline()))
    def call(self,c,args=None,fail=None):
        self.n+=1;req={'version':1,'id':str(self.n),'command':c,'args':args or {}};self.p.stdin.write(json.dumps(req)+'\n');self.p.stdin.flush();v=json.loads(self.p.stdout.readline());trace.append({'request':req,'reply':v})
        if fail:check(not v['ok'] and fail in v['error'],c+' refuses '+fail);return v
        assert v['ok'],v;return v['result']
    def close(self):self.p.stdin.close();self.p.wait(timeout=5);check(self.p.returncode==0,'owned process closed cleanly')
def project(e):return e.call('qroc-project-create',{'title':'Independent clinic research proposal','brain_dump':'Bounded semantic fixture: compare clinic market and existing clinic alternatives; no execution authority.'})['id']
def branch(e,p,name='Research',provider=False):return e.call('qroc-branch-create',dict(project_id=p,name=name,**({'provider':'local-gateway','model':'t03-private-bounded-fixture'} if provider else {})))['id']
def save(e,p,b,kind,content,rev=0):return e.call('qroc-document-save',{'project_id':p,'branch_id':b,'document_type':kind,'content':content,'source_revision':rev})
e=Engine('semantic');loaded=e.call('qroc-registry-semantic-profile');model=loaded['profile'];raw=pathlib.Path(a.profile_source).read_bytes();expected=hashlib.sha256(raw).hexdigest()
check(expected=='6ecc7fa0a012dcbf97aef7e878a5ef4fd85843e6d72fbc0bced44d4dd4db2c25' and loaded['profile_canonical_json'].encode()==raw and loaded['profile_sha256']==expected,'profile identity equals exact original-source JSON bytes')
check(model==json.loads(raw),'source model fields/forces/criteria/rules retained without inventions')
check(e.call('qroc-registry-semantic-validate',{'profile':model})['validated'],'source-backed registry validates')
check(not (e.state/'registry').exists(),'candidate registry validation installs no state')
e.call('qroc-registry-semantic-profile',{'profile':'unknown'},fail='capability-unavailable')
cat=e.call('qroc-registry-catalog');check(cat['semantic_force_registry']['available'] and cat['semantic_force_registry']['supported_profiles']==['retained-market-review/1'] and cat['semantic_force_registry']['complete_catalogue_available']==False,'availability explicitly bounded; complete catalogue remains unavailable')
for kind in ['MARKET_ANALYSIS','COMPETITOR_ANALYSIS','GATE_4']:
    entry=e.call('qroc-registry-type',{'document_type':kind});check(entry['semantic_force_profile_available'] and entry['semantic_profile']==model['id'] and entry['semantic_model'] in model['documents']+model['gates'],'supported type returns exact authoritative semantic model')
entry=e.call('qroc-registry-type',{'document_type':'PRD'});check(not entry['semantic_force_profile_available'] and 'semantic_model' not in entry and 'semantic_profile' not in entry,'unbounded type has no semantic flags/models')
# Registry integrity negatives mutate actual source model, not a replacement invented registry.
def bad_model(name,mutate,reason):
    v=copy.deepcopy(model);mutate(v);e.call('qroc-registry-semantic-validate',{'profile':v},fail=reason);checks.append(name)
bad_model('unknown required document refuses',lambda p:p['documents'][0].update(documentType='INVENTED_TYPE'),'Unknown Q-ROC')
bad_model('duplicate documents refuse',lambda p:p['documents'].append(copy.deepcopy(p['documents'][0])),'Duplicate document')
bad_model('duplicate forces refuse',lambda p:p['documents'][0]['forces'].append(copy.deepcopy(p['documents'][0]['forces'][0])),'Duplicate/invalid force')
bad_model('unknown force ref refuses',lambda p:p['gates'][0]['criteria'][0]['derivedFromForces'][0].update(forceId='market_analysis.unknown'),'Unknown force reference')
bad_model('duplicate criteria refuse',lambda p:p['gates'][0]['criteria'].append(copy.deepcopy(p['gates'][0]['criteria'][0])),'Duplicate/invalid criterion')
bad_model('missing criteria refuses',lambda p:p['gates'][0].update(criteria=[]),'Missing required gate criteria')
bad_model('wrong weights refuse',lambda p:p['gates'][0]['criteria'][0].update(weight=24),'weights must total')
bad_model('missing required sections refuses',lambda p:p['gates'][0]['outputContract'].update(requiredSections=[]),'Missing gate sections')
bad_model('unknown upstream refuses',lambda p:p['documents'][1]['inputPolicy']['allowedUpstream'].append('INVENTED_UPSTREAM'),'Unknown required upstream')
bad_model('duplicate upstream refuses',lambda p:p['documents'][1]['inputPolicy']['allowedUpstream'].append('MARKET_ANALYSIS'),'Duplicate semantic identity')
def circular(p):p['documents'][0]['inputPolicy'].update(stage='middle');p['documents'][0]['inputPolicy']['allowedUpstream'].append('COMPETITOR_ANALYSIS')
bad_model('circular input dependencies refuse',circular,'Circular semantic')
bad_model('impossible ordered upstream refuses',lambda p:p.update(documentOrder=['COMPETITOR_ANALYSIS','MARKET_ANALYSIS','GATE_4']),'Forward/impossible')
bad_model('unknown consistency kind refuses',lambda p:p['gates'][0]['consistencyRules'][0].update(kind='invented'),'Unsupported required consistency')
bad_model('duplicate rules refuse',lambda p:p['gates'][0]['consistencyRules'].append(copy.deepcopy(p['gates'][0]['consistencyRules'][0])),'Duplicate/invalid consistency')
bad_model('unknown consistency force refuses',lambda p:p['gates'][0]['consistencyRules'][0]['right'].update(forceId='competitor_analysis.missing'),'Unknown force reference')
p=project(e);b=branch(e,p)
e.call('qroc-context-preview',{'project_id':p,'branch_id':b,'document_type':'GATE_4'},fail='Missing required semantic')
market=save(e,p,b,'MARKET_ANALYSIS','Independent clinics are the segment. SOM is 2% of the clinic market. Sizing uses dated source counts and explicit assumptions.')
competitor=save(e,p,b,'COMPETITOR_ANALYSIS','Independent clinics are the segment. Existing clinic software and doing nothing are direct and status-quo threats. The addressable market uses the same clinic category.')
owned={'MARKET_ANALYSIS':market,'COMPETITOR_ANALYSIS':competitor};gm=model['gates'][0]
def evidence(ref):
    d=owned[ref['documentType']];return {'force_ref':ref,'document_id':d['id'],'revision':d['revision'],'sha256':d['sha256'],'quote':'Independent clinics are the segment.'}
valid={'schema':'qroc-semantic-gate/1','profile':model['id'],'gate_type':'GATE_4','score':96,'reviewed_documents':[{'document_type':k,'document_id':d['id'],'revision':d['revision'],'sha256':d['sha256']} for k,d in owned.items()],'criteria':[{'id':c['id'],'score':24,'max':c['weight'],'feedback':'Bounded source assessment, human judgment remains required','force_refs':c['derivedFromForces'],'evidence':[evidence(f) for f in c['derivedFromForces']]} for c in gm['criteria']],'consistency':[{'id':r['id'],'status':'supported','feedback':'Both exact source revisions identify independent clinics; this is an assessment, not execution authority','force_refs':[r['left'],r['right']],'evidence':[evidence(f) for f in [r['left'],r['right']]]} for r in gm['consistencyRules']],'sections':{s:'Bounded source assessment; unresolved facts remain for human review.' for s in gm['outputContract']['requiredSections']}}
revision=0

def evaluate(body):
    global revision
    content=body if isinstance(body,str) else json.dumps(body)
    d=save(e,p,b,'GATE_4',content,revision);revision=d['revision'];args={'project_id':p,'branch_id':b,'document_type':'GATE_4','revision':revision};return d,e.call('qroc-gate-semantic-evaluate',args)
d,result=evaluate(valid)
check(result['status']=='passed' and result['score']==96 and result['threshold']==90 and len(result['criteria'])==4 and result['semantic']['profile_sha256']==expected,'exact source-backed criterion/force/rule/evidence positive passes deterministic binding')
check(e.call('qroc-gate-evaluate',{'project_id':p,'branch_id':b,'document_type':'GATE_4','revision':revision})==result,'legacy command routes supported gate through identical owner semantic evaluator')
ctx=e.call('qroc-context-preview',{'project_id':p,'branch_id':b,'document_type':'GATE_4'})['context'];check(market['id'] in ctx and competitor['id'] in ctx and 'gate_4.landscape_consistency' in ctx,'compiled context carries actual reviewed IDs and exact cross-document model')
# Only policy-permitted branch upstreams enter semantic document context.
save(e,p,b,'PRE_MORTEM','UNRELATED_BRANCH_MARKER')
ctx=e.call('qroc-context-preview',{'project_id':p,'branch_id':b,'document_type':'MARKET_ANALYSIS'})['context'];check('UNRELATED_BRANCH_MARKER' not in ctx,'source semantic input policy excludes unrelated earlier branch documents')
# Outcome assessment is bound to owner source bytes and source IDs, never arbitrary caller references.
def bad_gate(name,mutate,reason):
    v=copy.deepcopy(valid);mutate(v);_,result=evaluate(v);check(result['status']=='failed' and reason in result['reason'],name)
bad_gate('unknown stable criterion rejected',lambda v:v['criteria'][0].update(id='gate_4.invented'),'Unknown stable criterion')
bad_gate('duplicate stable criterion rejected',lambda v:v['criteria'].append(copy.deepcopy(v['criteria'][0])),'Duplicate stable criterion')
bad_gate('missing criterion rejected',lambda v:v['criteria'].pop(),'Missing required stable criterion')
bad_gate('wrong criterion weight rejected',lambda v:v['criteria'][0].update(max=26),'score/weight mismatch')
bad_gate('criterion over maximum rejected',lambda v:v['criteria'][0].update(score=26),'score/weight mismatch')
bad_gate('overall score inconsistent rejected',lambda v:v.update(score=95),'total score inconsistent')
bad_gate('unknown force rejected',lambda v:v['criteria'][0]['force_refs'][0].update(forceId='market_analysis.invented'),'force references')
bad_gate('missing force evidence rejected',lambda v:v['criteria'][0]['evidence'].pop(),'force references')
bad_gate('wrong exact evidence revision rejected',lambda v:v['criteria'][0]['evidence'][0].update(revision=2),'Wrong evidence')
bad_gate('fabricated source quote rejected',lambda v:v['criteria'][0]['evidence'][0].update(quote='Invented unseen source claim'),'quote not found')
bad_gate('wrong reviewed source digest rejected',lambda v:v['reviewed_documents'][0].update(sha256='0'*64),'Reviewed source revision/digest')
bad_gate('duplicate reviewed source rejected',lambda v:v['reviewed_documents'].append(copy.deepcopy(v['reviewed_documents'][0])),'Duplicate reviewed')
bad_gate('missing reviewed source rejected',lambda v:v['reviewed_documents'].pop(),'Missing/unknown reviewed')
bad_gate('wrong same-branch source ID rejected',lambda v:v['reviewed_documents'][0].update(document_id=competitor['id']),'Reviewed source revision/digest')
bad_gate('unknown rule rejected',lambda v:v['consistency'][0].update(id='gate_4.invented_rule'),'Unknown consistency rule')
bad_gate('duplicate rule rejected',lambda v:v['consistency'].append(copy.deepcopy(v['consistency'][0])),'Duplicate consistency rule')
bad_gate('missing cross-document rule rejected',lambda v:v['consistency'].pop(),'Missing required consistency')
bad_gate('unknown rule status rejected',lambda v:v['consistency'][0].update(status='approved'),'Unknown consistency assessment')
bad_gate('missing required output section rejected',lambda v:v['sections'].pop('VERDICT'),'required output sections')
bad_gate('unknown profile rejected',lambda v:v.update(profile='invented'),'Unknown semantic profile')
bad_gate('unknown required output field rejected',lambda v:v.update(execute=True),'Missing/unknown semantic gate fields')
for status in ['contradicted','insufficient']:
    v=copy.deepcopy(valid);v['consistency'][0]['status']=status;_,result=evaluate(v);check(result['status']=='failed' and result['score']==96 and result['semantic']['consistency'][0]['status']==status,'cross-document '+status+' blocks pass despite high score')
v=copy.deepcopy(valid);v['criteria'][0]['score']=16;v['score']=88;_,result=evaluate(v);check(result['status']=='failed','score threshold and criterion floor block pass')
for name,body in [('malformed','{broken'),('truncated-json',json.dumps(valid)[:-3]),('duplicate-JSON-key',json.dumps(valid)[:-1]+',"score":96}'),('legacy-score-body','[SCORE]:96\n- evidence:96/100 - legacy')]:
    _,result=evaluate(body);check(result['status']=='failed' and result['score'] is None,name+' cannot obtain semantic pass')
# Exact current source binding also governs saved human reviews and export standing.
d,result=evaluate(valid);review=e.call('qroc-gate-review',{'project_id':p,'branch_id':b,'document_type':'GATE_4','revision':revision,'sha256':d['sha256'],'decision':'passed'})['review'];check(review['semantic_inputs']==result['semantic'],'human review records complete exact semantic source identity')
new_market=save(e,p,b,'MARKET_ANALYSIS','Updated independent clinic source material.',1)
result=e.call('qroc-gate-evaluate',{'project_id':p,'branch_id':b,'document_type':'GATE_4','revision':revision});check(result['status']=='failed' and 'changed' in result['reason'],'upstream edit invalidates current semantic gate standing')
e.call('qroc-gate-review',{'project_id':p,'branch_id':b,'document_type':'GATE_4','revision':revision,'sha256':d['sha256'],'decision':'passed'},fail='does not meet')
export=e.call('qroc-export',{'project_id':p});exported=json.loads(pathlib.Path(export['path']).read_text());standing=next(x for x in exported['standing'] if x['id']==d['id']);check(standing['gate_evaluation']['status']=='failed' and standing['review']['document_id']==d['id'] and not exported['execution_authority'],'export exposes failed current source binding and retains historical review without authority')
native=save(e,p,b,'GATE_1','[SCORE]:95\n- evidence:95/100 - retained native profile');n=e.call('qroc-gate-evaluate',{'project_id':p,'branch_id':b,'document_type':'GATE_1','revision':1});check(n['status']=='passed' and 'semantic' not in n,'native-only gate has no inferred semantic standing')
e.call('qroc-gate-semantic-evaluate',{'project_id':p,'branch_id':b,'document_type':'GATE_5','revision':1},fail='capability-unavailable')
# Required semantic sources cannot be silently omitted by a selected scope.
scoped=branch(e,p,'Scope-refusal')
for kind,d0 in owned.items():save(e,p,scoped,kind,d0['content'])
e.call('qroc-scope-save',{'project_id':p,'branch_id':scoped,'nature':'research','included':['MARKET_ANALYSIS','GATE_4'],'rationale':'Adversarial missing required scope dependency','source_revision':0})
e.call('qroc-context-preview',{'project_id':p,'branch_id':scoped,'document_type':'GATE_4'},fail='Required semantic gate source excluded')
# A malformed apparent newer source must never resurrect the older valid head.
corrupt=branch(e,p,'Corrupt-source-refusal');corrupt_owned={k:save(e,p,corrupt,k,d0['content']) for k,d0 in owned.items()}
corrupt_body=copy.deepcopy(valid);corrupt_body['reviewed_documents']=[{'document_type':k,'document_id':d0['id'],'revision':d0['revision'],'sha256':d0['sha256']} for k,d0 in corrupt_owned.items()]
for part in corrupt_body['criteria']+corrupt_body['consistency']:
    for ref in part['evidence']:
        actual=corrupt_owned[ref['force_ref']['documentType']];ref.update(document_id=actual['id'],revision=actual['revision'],sha256=actual['sha256'])
corrupt_gate=save(e,p,corrupt,'GATE_4',json.dumps(corrupt_body));bad_path=e.state/'qroc/documents'/p/corrupt/'MARKET_ANALYSIS-r2.json';bad_path.write_text('{broken')
bad_result=e.call('qroc-gate-semantic-evaluate',{'project_id':p,'branch_id':corrupt,'document_type':'GATE_4','revision':1});check(bad_result['status']=='failed' and bad_result['score'] is None,'corrupt newer source cannot resurrect older semantic head')
e.call('qroc-context-preview',{'project_id':p,'branch_id':corrupt,'document_type':'GATE_4'},fail='line')
e.close()
# Token admission uses a dedicated unserved loopback listener, guaranteeing no host route.
listener=socket.socket();listener.bind(('127.0.0.1',0));listener.listen(16);listener.setblocking(False);endpoint='%s:%s'%listener.getsockname()
base={'QROC_GATEWAY':endpoint,'QROC_MODEL':'t03-private-bounded-fixture','QROC_TIMEOUT_SECONDS':'45'}
for i,value in enumerate([None,'','0','-1','1.5','not-a-number','2049','4294967296','99999999999999999999999999999999',' 1024','+1']):
    env=dict(base)
    if value is not None:env['QROC_MAX_TOKENS']=value
    token_engine=Engine('token-invalid-'+str(i),env);p=project(token_engine);b=branch(token_engine,p,provider=True)
    token_engine.call('qroc-generate-start',{'project_id':p,'branch_id':b,'document_type':'PRD'},fail='QROC_MAX_TOKENS')
    check(token_engine.call('qroc-job-list')['jobs']==[],'invalid/absent token admission creates zero jobs')
    token_engine.close()
try:listener.accept();raise AssertionError('invalid token admission emitted an inference request')
except BlockingIOError:checks.append('all invalid token configurations emit no request')
listener.setblocking(True);listener.settimeout(5);requests=[]
def serve():
    for _ in range(4):
        conn,_=listener.accept();conn.settimeout(3);buf=b''
        while b'\r\n\r\n' not in buf:buf+=conn.recv(8192)
        head,body=buf.split(b'\r\n\r\n',1);length=int(next(x for x in head.split(b'\r\n') if x.lower().startswith(b'content-length:')).split(b':')[1]);assert length<4*1024*1024
        while len(body)<length:body+=conn.recv(8192)
        req=json.loads(body);requests.append(req)
        # Fourth request deliberately returns semantically well-formed JSON with incomplete finish.
        content='Bounded private response; no actual inference.'
        reason='stop'
        if len(requests)==4:content=json.dumps(valid);reason='length'
        out=json.dumps({'choices':[{'message':{'content':content},'finish_reason':reason}]}).encode();conn.sendall(b'HTTP/1.1 200 OK\r\nContent-Length: '+str(len(out)).encode()+b'\r\nConnection: close\r\n\r\n'+out);conn.close()
    listener.close()
t=threading.Thread(target=serve,daemon=True);t.start()
def poll(e,j):
    for _ in range(200):
        status=e.call('qroc-job-status',{'id':j['id']})
        if status['state'] not in ['running','queued','cancelling']:return status
        time.sleep(.01)
    raise AssertionError('bounded response fixture timed out')
for bound in [1,1024,2048]:
    env=dict(base,QROC_MAX_TOKENS=str(bound));x=Engine('token-valid-'+str(bound),env);p=project(x);b=branch(x,p,provider=True);j=x.call('qroc-generate-start',{'project_id':p,'branch_id':b,'document_type':'PRD'});status=poll(x,j);doc=x.call('qroc-document-detail',{'project_id':p,'branch_id':b,'document_type':'PRD','revision':1});check(status['state']=='succeeded' and j['max_tokens']==bound and j['pinned_inputs']['max_tokens']==bound and doc['generation_inputs']['max_tokens']==bound and requests[-1]['max_tokens']==bound and j['timeout_seconds']==45,'actual serialized token bound and45s deadline match admitted job/provenance');x.close()
x=Engine('semantic-partial',dict(base,QROC_MAX_TOKENS='1024'));p=project(x);b=branch(x,p,provider=True)
owned={k:save(x,p,b,k,d['content']) for k,d in owned.items()};valid['reviewed_documents']=[{'document_type':k,'document_id':d['id'],'revision':d['revision'],'sha256':d['sha256']} for k,d in owned.items()]
for c in valid['criteria']:c['evidence']=[evidence(f) for f in c['force_refs']]
for c in valid['consistency']:c['evidence']=[evidence(f) for f in c['force_refs']]
j=x.call('qroc-generate-start',{'project_id':p,'branch_id':b,'document_type':'GATE_4'});status=poll(x,j);result=x.call('qroc-gate-semantic-evaluate',{'project_id':p,'branch_id':b,'document_type':'GATE_4','revision':1});check(status['state']=='partial' and result['status']=='incomplete' and result['score'] is None,'complete semantic JSON with truncated provider finish cannot pass')
check(j['pinned_inputs']['semantic_profile']=='retained-market-review/1' and j['pinned_inputs']['semantic_profile_sha256']==expected and j['pinned_inputs']['compiler_version']=='qroc-source-semantic-compiler/1','semantic model/source/compiler identities pinned in generation receipt')
check('numeric JSON score fulfills' in requests[-1]['messages'][1]['content'] and 'do NOT emit a literal [SCORE]' in requests[-1]['messages'][1]['content'],'compiled semantic prompt chooses one JSON lowering of retained score requirement')
x.close();t.join(5);check(not t.is_alive(),'bounded private token/semantic listener cleaned up')
output.write_text(json.dumps({'status':'passed','standing':'bounded source-backed structural/model assessment component proof; no actual inference or human/native acceptance','checks':checks,'count':len(checks),'binary_sha256':hashlib.sha256(pathlib.Path(a.binary).read_bytes()).hexdigest(),'profile_sha256':expected,'trace':trace,'private_fixture_requests':requests},indent=2)+'\n');print(json.dumps({'passed':len(checks),'actual_inference':False,'evidence':str(output)}))

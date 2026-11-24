#!/usr/bin/env python3
"""Real engine development checks. Dedicated loopback response fixture, never inference."""
import argparse,hashlib,json,os,pathlib,socket,subprocess,threading,time,atexit,sys,traceback
ap=argparse.ArgumentParser();ap.add_argument('--binary',required=True);ap.add_argument('--scratch',required=True);ap.add_argument('--evidence',required=True);ap.add_argument('--source',required=True);a=ap.parse_args()
scratch=pathlib.Path(a.scratch);scratch.mkdir(parents=True,exist_ok=False)
evidence=pathlib.Path(a.evidence);trace=[];assertions=[];engines=[]
def cleanup():
    for p in engines:
        if p.poll() is None:
            p.stdin.close()
            try:p.wait(timeout=5)
            except subprocess.TimeoutExpired:p.terminate();p.wait(timeout=5)
atexit.register(cleanup)
def failure_hook(kind,value,tb):
    evidence.write_text(json.dumps({'status':'failed','error':str(value),'assertions':assertions,'trace':trace},indent=2)+'\n')
    traceback.print_exception(kind,value,tb)
sys.excepthook=failure_hook
def check(condition,name):
    assert condition,name
    assertions.append(name)
class Engine:
    def __init__(self,root,env=None):
        self.root=root;self.n=0
        clean={k:v for k,v in os.environ.items() if not k.startswith(('QROC_','SOLITON_QROC_'))}
        clean.update(env or {})
        self.p=subprocess.Popen([a.binary,'--root',str(root)],stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=subprocess.PIPE,text=True,env=clean)
        engines.append(self.p)
        self.ready=json.loads(self.p.stdout.readline());trace.append(self.ready)
    def call(self,c,args=None,fail=None):
        self.n+=1;r={'version':1,'id':str(self.n),'command':c,'args':args or {}}
        self.p.stdin.write(json.dumps(r)+'\n');self.p.stdin.flush();v=json.loads(self.p.stdout.readline());trace.append({'request':r,'reply':v})
        if fail is not None:check(not v['ok'] and fail in v['error'],c+' refused '+fail);return v
        assert v['ok'],v
        return v['result']
    def close(self):
        self.p.stdin.close();self.p.wait(timeout=5);check(self.p.returncode==0,'bridge exits cleanly')
def setup(e,name='Private CLI'):
    p=e.call('qroc-project-create',{'title':name,'brain_dump':'Build a private note CLI; preserve licence and privacy constraints.'})['id']
    b=e.call('qroc-branch-create',{'project_id':p,'name':'Main'})['id'];return p,b
def branch(e,p,name):return e.call('qroc-branch-create',{'project_id':p,'name':name,'model':'t03-response-fixture','provider':'local-gateway'})['id']
def save(e,p,b,kind,text,rev=0):return e.call('qroc-document-save',{'project_id':p,'branch_id':b,'document_type':kind,'content':text,'source_revision':rev})
def scope(e,p,b,nature='internal',revision=0,**kw):return e.call('qroc-scope-save',dict(project_id=p,branch_id=b,nature=nature,rationale='Selected bounded user workflow',source_revision=revision,**kw))
e=Engine(scratch/'offline');p,b=setup(e)
check(e.call('qroc-registry-provider-capabilities')['available']==False,'absent provider unavailable without inference')
e.call('qroc-generate-start',{'project_id':p,'branch_id':b,'document_type':'PRD'},fail='QROC_GATEWAY is not declared')
check(e.call('qroc-job-list')['jobs']==[],'unavailable provider creates no job')
sets=[]
for nature in ['internal','research','creative','external-product']:
    v=e.call('qroc-scope-preview',{'project_id':p,'branch_id':b,'nature':nature,'rationale':'Bounded '+nature});sets.append(tuple(v['included']))
check(len(set(sets))==4,'four scopes choose different relevant outputs')
s=scope(e,p,b,constraints=['Retain licence obligations'],excluded={'PRD':'Architecture-only experiment'})
check('PRD' not in s['included'] and s['constraints']==['Retain licence obligations'],'excluded document retains cross-cutting constraint')
e.call('qroc-context-preview',{'project_id':p,'branch_id':b,'document_type':'PRD'},fail='excluded')
ctx=e.call('qroc-context-preview',{'project_id':p,'branch_id':b,'document_type':'TECHNICAL_ARCHITECTURE'})['context'];check('Retain licence obligations' in ctx and 'Architecture-only experiment' in ctx,'generation context pins constraints and exclusions')
e.call('qroc-scope-save',{'project_id':p,'branch_id':b,'nature':'internal','rationale':'stale','source_revision':0},fail='Scope changed')
check(e.call('qroc-scope-get',{'project_id':p,'branch_id':b})['scope']==s,'scope readback exact')
b2=branch(e,p,'Alternative');check(e.call('qroc-scope-get',{'project_id':p,'branch_id':b2})['scope'] is None,'branch scope independent')
d=save(e,p,b,'STRATEGY_BRIEF','Human draft')
e.call('qroc-document-save',{'project_id':p,'branch_id':b,'document_type':'STRATEGY_BRIEF','content':'unchecked overwrite'},fail='source_revision')
e.call('qroc-document-save',{'project_id':p,'branch_id':b,'document_type':'STRATEGY_BRIEF','content':'stale overwrite','source_revision':0},fail='changed')
d2=save(e,p,b,'STRATEGY_BRIEF','Human edit',1)
check(len(e.call('qroc-document-history',{'project_id':p,'branch_id':b,'document_type':'STRATEGY_BRIEF'})['items'])==2,'manual history retained')
check(e.call('qroc-document-list',{'project_id':p,'branch_id':b2})['documents']==[],'branch documents independent')
e.call('qroc-draft-save',{'project_id':p,'branch_id':b,'document_type':'PRD','content':'Unfinished thought','base_revision':0})
check(e.call('qroc-draft-get',{'project_id':p,'branch_id':b,'document_type':'PRD'})['content']=='Unfinished thought','partial manual draft retained')
# Import actual declared source bytes, arbitrary filename; originals remain byte-identical.
source=pathlib.Path(a.source);source_before=source.read_bytes()
intake=scratch/'intake';intake.mkdir();(intake/source.name).write_bytes(source_before)
imp=e.call('qroc-project-import',{'path':str(intake),'title':'Real Q-ROC domain input'})
project=e.call('qroc-project-detail',{'id':imp['project']['id']})['project']
check(pathlib.Path(project['sources'][0]['path']).read_bytes()==source_before and source.read_bytes()==source_before,'real intake original copied exactly and unchanged')
# Gate parser and exact review identity, with native minimal profile only.
g=save(e,p,b,'GATE_1','[SCORE] 95\n- evidence: 95/100 - source checked')
arg={'project_id':p,'branch_id':b,'document_type':'GATE_1','revision':g['revision']}
check(e.call('qroc-gate-evaluate',arg)['status']=='passed','valid native criterion total passes deterministic assessment')
e.call('qroc-gate-review',dict(arg,sha256=g['sha256'],decision='passed'))
for text in ['[SCORE] 95\n[SCORE] 100\n- evidence: 95/100 - source checked','[SCORE] 95\n- evidence: 95/100 - ok\n- evidence: 1/100 - contradiction','[SCORE] 95\n- evidence: 95/100 - ok\n- safety: broken/100 - dropped before','[SCORE] 95\n- evidence: 90/100 - mismatched total','[SCORE] 95 garbage\n- evidence: 95/100 - ok']:
    g=save(e,p,b,'GATE_1',text,g['revision']);arg['revision']=g['revision'];check(e.call('qroc-gate-evaluate',arg)['status']=='failed','malformed/duplicate/inconsistent gate refuses pass')
e.call('qroc-gate-review',dict(arg,revision=1,sha256=g['sha256'],decision='passed'),fail='gate changed')
# Export only current heads, exact selection and collision refusal; no target-owner write.
all_docs=e.call('qroc-document-list',{'project_id':p,'branch_id':b})['documents'];heads={}
for doc in all_docs:
    if doc['document_type'] not in heads or heads[doc['document_type']]['revision']<doc['revision']:heads[doc['document_type']]=doc
refs=[{k:d[k] for k in ['id','revision','sha256']} for d in heads.values()]
receiver=scratch/'receiving-owner.txt';receiver.write_text('Owner original')
ex=e.call('qroc-export',{'project_id':p,'export_id':'collision-test','expected_documents':refs,'proposal':{'receiving_owner':'separate-owner','target_path':str(receiver)}})
raw=pathlib.Path(ex['path']).read_bytes();payload=json.loads(raw)
check(len(payload['documents'])==len(heads) and not payload['executed'] and not payload['execution_authority'],'current-only proposal export has no execution authority')
check(receiver.read_text()=='Owner original','proposal cannot write receiving owner target')
e.call('qroc-export',{'project_id':p,'export_id':'collision-test'},fail='exists')
check(pathlib.Path(ex['path']).read_bytes()==raw,'export collision preserves original artifact')
e.call('qroc-export',{'project_id':p,'expected_documents':[]},fail='selection changed')
review_branch=branch(e,p,'Reviewed positive')
for kind in ['GATE_1','GATE_2','GATE_3']:
    reviewed=save(e,p,review_branch,kind,'[SCORE]: 95\n- evidence: 95/100 - source checked')
    e.call('qroc-gate-review',{'project_id':p,'branch_id':review_branch,'document_type':kind,'revision':1,'sha256':reviewed['sha256'],'decision':'passed'})
advanced=e.call('qroc-domain-advance',{'project_id':p,'branch_id':review_branch,'document_type':'GATE_3','revision':1,'expected_domain':'PRODUCT','target_domain':'BUSINESS'})
check(advanced['current_domain']=='BUSINESS' and len(advanced['domain_transitions'][0]['gates'])==3,'consistent current reviewed gates advance with exact three review identities')
e.close()
e=Engine(scratch/'offline');check(e.call('qroc-scope-get',{'project_id':p,'branch_id':b})['scope']==s,'scope survives process restart');check(e.call('qroc-draft-get',{'project_id':p,'branch_id':b,'document_type':'PRD'})['content']=='Unfinished thought','draft survives restart');e.close()
# Dedicated local response fixture. Binding to port 0 ensures no host gateway route.
listener=socket.socket();listener.bind(('127.0.0.1',0));listener.listen(8);port=listener.getsockname()[1];received=[];release=threading.Event();accepted=threading.Event();finish={'reason':'stop'}
def serve():
    for _ in range(9):
        conn,_=listener.accept();conn.settimeout(5);data=b''
        while b'\r\n\r\n' not in data:data+=conn.recv(8192)
        header,body=data.split(b'\r\n\r\n',1);length=int(next(x for x in header.split(b'\r\n') if x.lower().startswith(b'content-length:')).split(b':')[1])
        while len(body)<length:body+=conn.recv(8192)
        received.append(json.loads(body));accepted.set();assert release.wait(5)
        out=json.dumps({'choices':[{'message':{'content':'[SCORE] 95\n- evidence: 95/100 - private fixture'},'finish_reason':finish['reason']}]}).encode()
        status=finish.get('status',200);length=len(out)+finish.get('truncate',0)
        conn.sendall(f'HTTP/1.1 {status} Fixture\r\nContent-Length: {length}\r\nConnection: close\r\n\r\n'.encode()+out);conn.close()
    listener.close()
t=threading.Thread(target=serve,daemon=True);t.start()
e=Engine(scratch/'fixture',{'QROC_GATEWAY':f'127.0.0.1:{port}','QROC_MODEL':'t03-response-fixture','QROC_TIMEOUT_SECONDS':'15','QROC_MAX_TOKENS':'1024'})
p,_=setup(e);b=branch(e,p,'Generated')
def start(kind):
    accepted.clear();release.clear();job=e.call('qroc-generate-start',{'project_id':p,'branch_id':b,'document_type':kind});assert accepted.wait(5);return job

def complete(job):
    release.set()
    for _ in range(100):
        v=e.call('qroc-job-status',{'id':job['id']})
        if v['state'] not in ['running','queued','cancelling']:return v
        time.sleep(.01)
    raise AssertionError('bounded job did not finish')
job=start('PRD');manual=save(e,p,b,'PRD','Concurrent human edit');j=complete(job)
check(j['state']=='conflicted' and pathlib.Path(j['conflict_path']).exists(),'stale generation preserves conflict and cannot replace human revision')
check(e.call('qroc-document-detail',{'project_id':p,'branch_id':b,'document_type':'PRD','revision':1})['id']==manual['id'],'human revision remains current')
job=start('STRATEGY_BRIEF');scope(e,p,b,included=['STRATEGY_BRIEF','PRD','GATE_1']);j=complete(job);check(j['state']=='conflicted','scope change fences generation completion')
finish['reason']='length';job=start('GATE_1');j=complete(job);check(j['state']=='partial','truncated response retained as partial')
check(e.call('qroc-gate-evaluate',{'project_id':p,'branch_id':b,'document_type':'GATE_1','revision':1})['status']=='incomplete','partial generated gate cannot pass')
finish['reason']='stop';target=branch(e,p,'Synthesis');src=branch(e,p,'Other source');s1=save(e,p,src,'PRD','Selected original');save(e,p,src,'PRD','Later revision',1)
accepted.clear();release.clear();j=e.call('qroc-synthesis-start',{'project_id':p,'target_branch_id':target,'source_branch_ids':[b,src],'source_revisions':{b:1,src:1},'document_type':'PRD'});assert accepted.wait(5);j=complete(j)
syn=e.call('qroc-document-detail',{'project_id':p,'branch_id':target,'document_type':'PRD','revision':1})
check(j['state']=='succeeded' and [x['revision'] for x in syn['provenance']['source_documents']]==[1,1],'synthesis pins selected exact revisions rather than mutable heads')
check(syn['generation_inputs']['model']=='t03-response-fixture' and syn['generation_inputs']['context_sha256']==hashlib.sha256(syn['generation_inputs']['context'].encode()).hexdigest(),'generated revision carries pinned provider and context identity')
check('Selected original' in received[-1]['messages'][1]['content'] and 'Later revision' not in received[-1]['messages'][1]['content'],'synthesis prompt uses original selected source bytes')
job=start('GATE_1');j=complete(job);g2=e.call('qroc-document-detail',{'project_id':p,'branch_id':b,'document_type':'GATE_1','revision':2})
check(j['state']=='succeeded' and g2['generation_finish_reason']=='stop','complete identical output creates fresh revision after partial generation')
e.call('qroc-gate-review',{'project_id':p,'branch_id':b,'document_type':'GATE_1','revision':2,'sha256':g2['sha256'],'decision':'passed'})
job=start('GATE_1');j=complete(job);g3=e.call('qroc-document-detail',{'project_id':p,'branch_id':b,'document_type':'GATE_1','revision':3})
check(g3['id']!=g2['id'] and not any(r['document_id']==g3['id'] for r in e.call('qroc-project-detail',{'id':p})['project']['gate_reviews']),'identical regenerated gate has new revision and cannot inherit review')
finish['status']=500;job=start('PRD');j=complete(job);check(j['state']=='failed','HTTP error cannot publish valid-looking response content')
finish['status']=200;finish['truncate']=10;job=start('PRD');j=complete(job);check(j['state']=='failed','truncated HTTP body cannot publish a revision')
finish['truncate']=0;job=start('PRD');e.call('qroc-job-cancel',{'id':job['id']});j=complete(job);check(j['state']=='cancelled','cancelled output cannot replace current revision')
e.close();t.join(5);check(not t.is_alive(),'private fixture listener stopped')
evidence.write_text(json.dumps({'standing':'component fixture proof only; no actual model/native UI acceptance','binary':a.binary,'binary_sha256':hashlib.sha256(pathlib.Path(a.binary).read_bytes()).hexdigest(),'source':a.source,'source_sha256':hashlib.sha256(source_before).hexdigest(),'private_fixture_endpoint':f'127.0.0.1:{port}','assertions':assertions,'trace':trace,'fixture_requests':received},indent=2)+'\n')
print(json.dumps({'passed':len(assertions),'evidence':str(evidence),'real_provider_proven':False}))

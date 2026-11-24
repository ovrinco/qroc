#!/usr/bin/env python3
"""CPU fake-bridge mechanics only: no GPU, HTTP, native seat or actual inference."""
import importlib.util,pathlib,json,os,sys,copy,hashlib,time
path=pathlib.Path(__file__).with_name('provider-journey-v3.py');spec=importlib.util.spec_from_file_location('journey',path);m=importlib.util.module_from_spec(spec);spec.loader.exec_module(m)
root=pathlib.Path(sys.argv[1]);root.mkdir(exist_ok=False);checks=[]
os.environ.update(QROC_GATEWAY='127.0.0.1:1',GPU_LANE_ENDPOINT='http://127.0.0.1:1',QROC_MODEL='mechanics-only',QROC_MAX_TOKENS='1024',QROC_TIMEOUT_SECONDS='45')
def check(ok,name):assert ok,name;checks.append(name)
def run_failure(name,launcher,work=None,deadline=False):
 j=m.Journey(str(launcher),root/name)
 if deadline:j.deadline=time.monotonic()+.05
 code=j.execute(work);data=json.loads(j.evidence.read_text());check(code==1 and data['failure'] is not None and data['cleanup']['reaped'],name+' preserves failure and owned cleanup');return data
run_failure('missing-startup',root/'absent')
bad=root/'badstartup';bad.write_text('#!/usr/bin/env python3\nprint("not JSON",flush=True)\n');bad.chmod(0o755);run_failure('malformed-startup',bad)
reply=root/'badreply';reply.write_text('#!/usr/bin/env python3\nimport sys,json\nprint(json.dumps({"version":1,"ready":True}),flush=True)\nfor line in sys.stdin: print("broken reply",flush=True)\n');reply.chmod(0o755)
j=m.Journey(str(reply),root/'response-failure');code=j.execute(lambda:j.ask('fixture-command'));data=json.loads(j.evidence.read_text());check(code==1 and data['trace'][0]['request']['command']=='fixture-command' and data['trace'][0]['reply'] is None and data['failure']['type']=='JSONDecodeError','malformed reply retains original pending request trace')
sleep=root/'sleepreply';sleep.write_text('#!/usr/bin/env python3\nimport sys,json,time\nprint(json.dumps({"version":1}),flush=True)\nfor line in sys.stdin: time.sleep(10)\n');sleep.chmod(0o755)
j=m.Journey(str(sleep),root/'deadline');j.deadline=time.monotonic()+.05;start=time.monotonic();code=j.execute(lambda:j.ask('fixture-timeout'));data=json.loads(j.evidence.read_text());check(code==1 and time.monotonic()-start<5 and data['trace'][0]['reply'] is None and data['cleanup']['reaped'] and data['failure'],'deadline retains trace before cleanup timeout/reap')
j=m.Journey(str(reply),root/'cleanup-failure');j.cleanup=lambda:(_ for _ in ()).throw(RuntimeError('fixture-cleanup-failure'));code=j.execute(lambda:(_ for _ in ()).throw(ValueError('original-failure')));data=json.loads(j.evidence.read_text());check(code==1 and data['failure']['message']=='original-failure' and 'fixture-cleanup-failure' in data['cleanup']['error'],'cleanup exception cannot discard original evidence/failure')
# Reap this mechanics-owned process after deliberately mocked cleanup.
if j.p is not None:j.p.stdin.close();j.p.wait(timeout=3)
collision=root/'collision';collision.mkdir();(collision/'actual-provider.json').write_text('preserved historical bytes');j=m.Journey(str(reply),collision);code=j.execute();check(code==1 and (collision/'actual-provider.json').read_text()=='preserved historical bytes' and j.evidence!=collision/'actual-provider.json','root collision writes unique sidecar without clobbering history')
source=root/'mechanics-source';source.write_bytes(b'bounded private source');sourceinfo=[{'path':str(source),'size':len(source.read_bytes()),'sha256':m.sha(source.read_bytes())}];inputs={'job_id':'job-fixture','max_tokens':1024};doc={'id':'doc-fixture','project_id':'project-fixture','branch_id':'branch-fixture','revision':2,'content':'mechanics only','sha256':m.sha(b'mechanics only'),'generation_finish_reason':'stop','generation_inputs':inputs};job={'id':'job-fixture','state':'succeeded','document_id':doc['id'],'project_id':doc['project_id'],'branch_id':doc['branch_id'],'base_revision':1,'output_sha256':doc['sha256'],'max_tokens':1024,'pinned_inputs':inputs};export={'executed':False,'execution_authority':False,'proposal':{'receiving_owner':'Human review'},'project':{'id':'project-fixture','sources':sourceinfo},'documents':[doc]};check(m.validate_document(job,doc,export,sourceinfo)['job_pinned_document_export_max_tokens']==1024,'mechanics positive equality accepted')
for target in ['job','pinned','document','export','partial','digest','authority']:
 a,b,c=copy.deepcopy(job),copy.deepcopy(doc),copy.deepcopy(export)
 if target=='job':a['max_tokens']=999
 if target=='pinned':a['pinned_inputs']['max_tokens']=999
 if target=='document':b['generation_inputs']['max_tokens']=999
 if target=='export':c['documents'][0]['generation_inputs']['max_tokens']=999
 if target=='partial':a['state']='partial';b['generation_finish_reason']='length'
 if target=='digest':b['sha256']='0'*64
 if target=='authority':c['execution_authority']=True
 try:m.validate_document(a,b,c,sourceinfo);raise RuntimeError('wrong bound accepted')
 except AssertionError:checks.append(target+' refuses without acceptance')
baseline=[{'id':0,'is_processing':False,'id_task':1}];active=[{'id':0,'is_processing':True,'id_task':2}];running={'id':'owned','state':'running','started_at':1};check(m.owned_active(baseline,active,running)['id_task']==2,'sole new cooperative slot correlated')
for name,slots,jobvalue in [('two-active',active+active,running),('old-task',[dict(active[0],id_task=1)],running),('wrong-slot',[dict(active[0],id=2)],running),('not-running',active,dict(running,state='queued'))]:
 try:m.owned_active(baseline,slots,jobvalue);raise RuntimeError('ambiguous ownership accepted')
 except AssertionError:checks.append(name+' refuses ambiguous ownership')
# Provider names and batch refusal through private NDJSON fake bridges only.
provider_bridge=root/'provider-name-bridge'
provider_bridge.write_text('#!/usr/bin/env python3\nimport sys,json\nprint(json.dumps({"version":1,"ready":True}),flush=True)\nfor line in sys.stdin:\n r=json.loads(line); batch=r["command"]=="qroc-batch-start"; print(json.dumps({"version":1,"id":r["id"],"ok":not batch,"result":None if batch else {"provider":"route-shim","available":True,"model":"fixture-route"},"error":"capability-unavailable: QROC_BATCH_MAX must explicitly cover the requested cascade" if batch else None}),flush=True)\n')
provider_bridge.chmod(0o755)
j=m.Journey(str(provider_bridge),root/'provider-name')
code=j.execute(lambda:check(j.ask('qroc-registry-provider-capabilities')['provider']=='route-shim','fake bridge preserves declared provider name'))
check(code==0,'provider-name fake bridge journey succeeds')
def refused_batch():
 j.ask('qroc-batch-start',{'document_types':['STRATEGY_BRIEF','PRD']})
j=m.Journey(str(provider_bridge),root/'batch-refusal');code=j.execute(refused_batch);data=json.loads(j.evidence.read_text())
check(code==1 and data['cleanup']['reaped'] and data['trace'][0]['reply']['ok'] is False and 'capability-unavailable' in data['trace'][0]['reply']['error'],'batch refusal remains a traced failure without cascade')
out={'standing':'MECHANICS ONLY; fake private bridges/records; no native/model/provider/inference acceptance','status':'PASS','checks':checks,'count':len(checks),'source_sha256':hashlib.sha256(path.read_bytes()).hexdigest(),'network_requests':0,'native_seat':False,'actual_inference':False};pathlib.Path(sys.argv[2]).write_text(json.dumps(out,indent=2)+'\n');print(json.dumps({'status':'PASS','mechanics_checks':len(checks),'actual_inference':False}))

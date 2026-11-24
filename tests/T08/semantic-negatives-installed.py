#!/usr/bin/env python3
"""Exact installed API negative observations; never manufacture missing standing."""
import json,os,sys,pathlib,subprocess,hashlib
launcher,rootarg,outarg=sys.argv[1:];root=pathlib.Path(rootarg);root.mkdir(exist_ok=False);trace=[];seq=0
clean={k:v for k,v in os.environ.items() if not k.startswith(('QROC_','SOLITON_QROC_'))}
p=subprocess.Popen([launcher,'--root',str(root),'--bridge'],stdin=subprocess.PIPE,stdout=subprocess.PIPE,text=True,env=clean);trace.append(json.loads(p.stdout.readline()))
def ask(c,args={},good=True):
 global seq
 seq+=1;req=dict(version=1,id=str(seq),command=c,args=args);p.stdin.write(json.dumps(req)+'\n');p.stdin.flush();r=json.loads(p.stdout.readline());trace.append(dict(request=req,reply=r));assert r['id']==str(seq) and r['ok']==good,r
 return r['result'] if good else r['error']
def snapshot():return {str(f.relative_to(root)):hashlib.sha256(f.read_bytes()).hexdigest() for f in root.rglob('*') if f.is_file() and f.name!='.bridge.lock'}
try:
 profile=ask('qroc-registry-semantic-profile');assert profile['profile_sha256']=='6ecc7fa0a012dcbf97aef7e878a5ef4fd85843e6d72fbc0bced44d4dd4db2c25'
 assert not ask('qroc-registry-provider-capabilities')['available'];assert ask('qroc-job-list')['jobs']==[]
 project=ask('qroc-project-create',dict(title='Missing source assessment',brain_dump='Manual model-free negative check'));pid=project['id'];branch=ask('qroc-branch-create',dict(project_id=pid,name='Actual selected branch'));bid=branch['id']
 foreign=ask('qroc-project-create',dict(title='Protected separate owner',brain_dump='No foreign writes allowed'));foreignbranch=ask('qroc-branch-create',dict(project_id=foreign['id'],name='Protected branch'))
 doc=ask('qroc-document-save',dict(project_id=pid,branch_id=bid,document_type='GATE_4',source_revision=0,content='Manual uncompleted assessment. Required market/competitor source documents have not been authored.'))
 before=snapshot();args=dict(project_id=pid,branch_id=bid,document_type='GATE_4',revision=doc['revision']);result=ask('qroc-gate-semantic-evaluate',args);native=ask('qroc-gate-evaluate',args)
 assert result['score'] is None and result['criteria']=={};assert result.get('semantic',{}).get('profile')==profile['profile']['id'];assert not result.get('semantic',{}).get('reviewed_documents');assert result['status']==native['status'] and result['reason']==native['reason']
 errors=[]
 for owner in [dict(project_id='nonexistent-project',branch_id=bid),dict(project_id=pid,branch_id=foreignbranch['id'])]:
  q=dict(owner,document_type='GATE_4',revision=doc['revision'],sha256=doc['sha256'],decision='passed')
  for cmd in ['qroc-gate-review','qroc-gate-semantic-evaluate']:errors.append({'command':cmd,'owner':owner,'error':ask(cmd,q,False)})
  errors.append({'command':'qroc-document-save','owner':owner,'error':ask('qroc-document-save',dict(owner,document_type='GATE_4',source_revision=0,content='Must refuse wrong owner'),False)})
 errors.append({'command':'qroc-export','owner':'nonexistent-project','error':ask('qroc-export',dict(project_id='nonexistent-project'),False)})
 assert ask('qroc-job-list')['jobs']==[];assert ask('qroc-project-detail',{'id':foreign['id']})['project']['gate_reviews']==[];after=snapshot();assert before==after,'Negative endpoint check changed durable bytes'
 payload={'status':'PASS' if result['status']=='incomplete' else 'CONTRACT_EXPECTATION_FAILED','standing':'actual installed model-free API negatives; no native UI or human/inference acceptance','expected_missing_upstream_status':'incomplete','observed_missing_upstream':result,'selected_owner':{'project_id':pid,'branch_id':bid,'document_id':doc['id'],'revision':doc['revision'],'sha256':doc['sha256']},'profile_sha256':profile['profile_sha256'],'protected_owner':{'project_id':foreign['id'],'branch_id':foreignbranch['id']},'owner_refusal_checks':'PASS','errors':errors,'state_before':before,'state_after':after,'zero_jobs':True,'provider_available':False,'inference_performed':False,'native_retry':False,'trace':trace,'cause_if_conflict':'src/qroc.rs evaluate_owned_gate: missing semantic_current_document error lowered to short failed envelope by unwrap_or_else; incomplete path is currently truncated-generation only'}
 pathlib.Path(outarg).write_text(json.dumps(payload,indent=2)+'\n');print(json.dumps({'status':payload['status'],'observed_status':result['status'],'reason':result['reason'],'owner_refusal':'PASS','durable_state_unchanged':True}))
finally:
 p.stdin.close();assert p.wait(timeout=5)==0

import hashlib,http.server,json,os,pathlib,subprocess,sys,tempfile,threading,time,shutil
def synthetic_hardening(root, project_id):
    """A fixture acceptance for testing downstream advice, never a provider judgment."""
    root=root/'qroc'
    project=json.loads((root/'projects'/f'{project_id}.json').read_text())
    idea=project.get('brain_dump','')
    record=dict(schema='qroc-hardening/1',project=project_id,route=None,rounds=[dict(
        round=1,at=0,idea_text=idea,idea_sha256=hashlib.sha256(idea.encode()).hexdigest(),
        score=dict(total=100,idea=100,config=0,missing=[]),verdict='pass',sent_back=False,
        questions=None,answers=None,judgment=dict(status='synthetic downstream test fixture',answers=[],receipt=None))])
    folder=root/'hardening';folder.mkdir(exist_ok=True)
    (folder/f'{project_id}.json').write_text(json.dumps(record))

binary,roots=sys.argv[1:];roots=pathlib.Path(roots);roots.mkdir(parents=True,exist_ok=True);root=pathlib.Path(tempfile.mkdtemp(prefix='profiles-',dir=roots));commands=[]
def sha(p):return hashlib.sha256(p.read_bytes()).hexdigest()
class Shim(http.server.BaseHTTPRequestHandler):
 mode='good'
 def log_message(self,*a):pass
 def do_POST(self):
  r=json.loads(self.rfile.read(int(self.headers['Content-Length'])));c=json.loads(r['messages'][1]['content']);name=c['profile'].split('.')[1];assert json.loads(r['messages'][0]['content'])['contract']==c['profile'] and r['model']=='decide:'+name
  out=[]
  for subject in c['subjects']:
   groups=[]
   for source in subject['sources']:
    groups.append([dict(force_ref=ref,document_id=source['documents'][ref['documentType']]['id'],revision=source['documents'][ref['documentType']]['revision'],sha256=source['documents'][ref['documentType']]['sha256'],quote=subject['span'] if name=='source_class' else source['documents'][ref['documentType']]['content']) for ref in source['refs']])
   label=dict(id=subject['id'],label={'quote_support':'supported','scope_readiness':'research','source_class':'requirement','duplicates':'effectively-identical','phod_readiness':'integer-range/1'}[name],evidence=groups)
   if name=='scope_readiness':label.update(included_types=['MARKET_ANALYSIS'],excluded_types=['COMPETITOR_ANALYSIS'])
   if self.mode=='bad':label['evidence'][0][0]['quote']='FABRICATED'
   if self.mode=='ambiguous':label.update(label='ambiguous',ambiguity_category='invented')
   out.append(label)
  if self.mode=='authority':out[0]['authority']='accepted'
  if self.mode=='unknown':out[0]['id']='unknown'
  if self.mode=='extra':out[0]['evidence'].append(out[0]['evidence'][0])
  content='broken JSON' if self.mode=='malformed' else json.dumps(dict(labels=out))
  data=json.dumps(dict(receipt_id='fixture-'+name,model='fixture-actual',route='fixture:port0',choices=[dict(finish_reason='stop',message=dict(content=content))])).encode();self.send_response(200);self.send_header('Content-Length',str(len(data)));self.end_headers();self.wfile.write(data)
server=http.server.HTTPServer(('127.0.0.1',0),Shim);assert server.server_port not in (8080,8096,8097);t=threading.Thread(target=server.serve_forever);t.start();env=dict(os.environ,QROC_GATEWAY=f'http://127.0.0.1:{server.server_port}');bridge=None;count=0
def start():
 global bridge,count
 bridge=subprocess.Popen([binary,'--root',str(root)],stdin=subprocess.PIPE,stdout=subprocess.PIPE,text=True,env=env);count=0;assert json.loads(bridge.stdout.readline())['event']=='ready'
def close():
 bridge.stdin.close();assert bridge.wait(timeout=5)==0
def ask(command,args,ok=True):
 global count
 count+=1;commands.append(command);bridge.stdin.write(json.dumps(dict(version=1,id=str(count),command=command,args=args))+'\n');bridge.stdin.flush();response=json.loads(bridge.stdout.readline());assert response['ok']==ok,response
 return response['result'] if ok else response['error']
def raw(command,args,ok=True):return json.dumps(ask(command,args,ok),sort_keys=True,separators=(',',':')).encode()
try:
 start();p=ask('qroc-project-create',dict(title='T312 fixture',brain_dump='Evidence only'))['id'];b=ask('qroc-branch-create',dict(project_id=p,name='A',provider='unavailable',model='fixture'))['id'];b2=ask('qroc-branch-create',dict(project_id=p,name='B',provider='unavailable',model='fixture'))['id'];owner=dict(project_id=p,branch_id=b)
 synthetic_hardening(root,p)
 for branch in (b,b2):
  for kind in ('MARKET_ANALYSIS','COMPETITOR_ANALYSIS'):ask('qroc-document-save',dict(project_id=p,branch_id=branch,document_type=kind,content='The range must be integers from 1 to 4.',source_revision=0))
 criterion=ask('qroc-registry-semantic-profile',{})['profile']['gates'][0]['criteria'][0];source=dict(revisions={'MARKET_ANALYSIS':1},refs=[dict(documentType='MARKET_ANALYSIS',forceId='fixture.span')]);args=dict(owner,subjects=[dict(id='clause',sources=[source])]);requests={}
 for name in ('quote_support','scope_readiness','source_class','duplicates','phod_readiness'):
  a=json.loads(json.dumps(args))
  if name=='quote_support':a['subjects'][0].update(criterion_id=criterion['id'],claim='The range is bounded.');a['subjects'][0]['sources']=[dict(revisions={r['documentType']:1 for r in criterion['derivedFromForces']},refs=criterion['derivedFromForces'])]
  if name=='source_class':a['subjects'][0]['span']='The range must be integers from 1 to 4.'
  if name=='duplicates':a['subjects'][0]['sources'].append(dict(source,branch_id=b2))
  requests[name]=a
 scope=root/'scope'/'fixture.json';scope.parent.mkdir(parents=True,exist_ok=True);(root/'phod').mkdir(exist_ok=True);scope.write_text('{"nature":"internal"}\n');scope_before=sha(scope);stable={str(p.relative_to(root)):sha(p) for p in root.rglob('*') if p.is_file()};branches_before=len(ask('qroc-branch-list',dict(project_id=p))['branches']);(root/'phod-before').write_bytes(raw('phod-workbench',{},False));stamp=root/'stamp';stamp.touch();time.sleep(.01)
 for name,a in requests.items():
  before=list((root/'qroc/advice').glob('*.json')) if (root/'qroc/advice').exists() else [];record=ask('qroc-decide-'+name.replace('_','-'),a);assert record['advisory'] and record['quote_verified'] and not record['abstained'],record
  assert record['model']=='fixture-actual' and record['contract_sha256']==sha(pathlib.Path('runtime/decide/contracts')/(record['profile']+'.json'));assert len(list((root/'qroc/advice').glob('*.json')))==len(before)+1
  for key in ('receipt_id','profile','contract_sha256','consumer','revision_sha256s','model','route','labels','supporting_spans','quote_verified','abstained','advisory'):assert key in record
  print('CHECK1_RECORD',json.dumps(record,separators=(',',':')),flush=True)
 (root/'phod-after').write_bytes(raw('phod-workbench',{},False));subprocess.run(['cmp',root/'phod-before',root/'phod-after'],check=True);print('CHECK2_PHOD_WORKBENCH_REFUSAL_CMP_EXIT 0',flush=True);print('CHECK2_FIND_BEGIN',flush=True);subprocess.run(['find',root/'phod','-newer',stamp],check=True);print('CHECK2_FIND_END',flush=True)
 assert scope_before==sha(scope);print('CHECK3_SCOPE_SHA256',scope_before,sha(scope));assert branches_before==len(ask('qroc-branch-list',dict(project_id=p))['branches']);print('CHECK4_BRANCH_COUNTS',branches_before,branches_before);assert all(sha(root/p)==h for p,h in stable.items())
 close();pristine=roots/(root.name+'-snapshot');shutil.copytree(root,pristine);inputdir=roots/(root.name+'-input');inputdir.mkdir();(inputdir/'MARKET_ANALYSIS.md').write_text('Exact imported source.')
 while time.time()%1>.02:time.sleep(.002)
 start();import_before=raw('qroc-project-import',dict(path=str(inputdir),title='Import fixture'));close();root.rename(roots/(root.name+'-without-advice'));shutil.copytree(pristine,root);start();ask('qroc-decide-source-class',requests['source_class']);import_after=raw('qroc-project-import',dict(path=str(inputdir),title='Import fixture'));(root/'import-before').write_bytes(import_before);(root/'import-after').write_bytes(import_after);subprocess.run(['cmp',root/'import-before',root/'import-after'],check=True);print('CHECK4_IMPORT_CMP_EXIT 0')
 for name,a in requests.items():
  for mode in ('bad','malformed','unknown','extra','authority'):
   Shim.mode=mode;r=ask('qroc-decide-'+name.replace('_','-'),a);assert r['abstained'],(name,mode,r)
 Shim.mode='ambiguous';assert ask('qroc-decide-phod-readiness',requests['phod_readiness'])['abstained'];print('NEGATIVES_PASS five profiles: fabricated quotes, malformed JSON, unknown labels, extra groups, authority field; unnamed PHOD ambiguity')
 log=root/'bridge-commands.log';log.write_text('\n'.join(commands)+'\n');r=subprocess.run(['grep','-c','qroc-scope-save',log],capture_output=True,text=True);print('CHECK3_GREP_COUNT',r.stdout.strip());assert r.stdout.strip()=='0';print('FIXTURE_ROOT',root);print('T312_FIXTURE_PASS')
finally:
 if bridge and bridge.poll() is None:close()
 server.shutdown();t.join();server.server_close()

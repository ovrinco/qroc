#!/usr/bin/env python3
"""Offline provenance regression; optional installed bridge, fresh private roots only."""
import http.server,json,os,pathlib,select,socket,subprocess,sys,threading,time
BIN=sys.argv[1];root=pathlib.Path(sys.argv[2]).resolve();root.mkdir(parents=True,exist_ok=False)
label='fixture:provenance';requested='requested-provider-model';content='Offline source-backed fixture draft.'
cases=[('missing',{}),('null',{'model':None}),('number',{'model':42}),('boolean',{'model':True}),('object',{'model':{'name':'not-a-string'}}),('array',{'model':['not-a-string']}),('known',{'model':'actual-provider-model / exact'}),('empty-string',{'model':''})]
current={};requests=[];failures=[];trace=[];seq=0
class Handler(http.server.BaseHTTPRequestHandler):
 def log_message(self,*a):pass
 def do_POST(self):
  assert self.path=='/v1/chat/completions'
  body=json.loads(self.rfile.read(int(self.headers['Content-Length'])));requests.append(body)
  assert body['model']==requested and body['stream']==False and body['max_tokens']==64
  data=json.dumps(dict(choices=[dict(message=dict(content=content),finish_reason='stop')],usage={'completion_tokens':3},**current)).encode()
  self.send_response(200);self.send_header('Content-Length',str(len(data)));self.end_headers();self.wfile.write(data)
server=http.server.HTTPServer(('127.0.0.1',0),Handler);assert server.server_port not in [8080,8096,8097]
thread=threading.Thread(target=server.serve_forever);thread.start()
table=root/'routes.json';table.write_text(json.dumps({'routes':{label:{'kind':'apikey','base':f'http://127.0.0.1:{server.server_port}','model':requested,'key_env':'T301W2_OFFLINE_SENTINEL','metered':False}}}))
env=dict(os.environ,T301W2_OFFLINE_SENTINEL='offline-fixture-value')
state=root/'shim-state';log=(root/'shim.stderr').open('w');shim=None;bridge=None

def receipt(count):
 until=time.monotonic()+3
 while time.monotonic()<until:
  p=state/'receipts.jsonl';rows=p.read_text().splitlines() if p.exists() else []
  if len(rows)>=count:return json.loads(rows[count-1])
  time.sleep(.01)
 raise AssertionError('receipt deadline')
def ask(command,args=None):
 global seq
 seq+=1;request={'version':1,'id':str(seq),'command':command,'args':args or {}}
 bridge.stdin.write(json.dumps(request)+'\n');bridge.stdin.flush();assert select.select([bridge.stdout],[],[],5)[0],'bridge response deadline'
 reply=json.loads(bridge.stdout.readline());trace.append({'request':request,'reply':reply});assert reply['ok'],reply;return reply['result']
try:
 shim=subprocess.Popen([BIN,'serve','--listen','127.0.0.1:0','--routes',str(table),'--state',str(state)],env=env,stdout=subprocess.PIPE,stderr=log,text=True)
 assert select.select([shim.stdout],[],[],5)[0],'listen deadline';line=shim.stdout.readline();assert line.startswith('LISTEN '),line;port=int(line.rsplit(':',1)[1]);assert port not in [8080,8096,8097]
 for i,(name,model) in enumerate(cases,1):
  current=model;expected=model['model'] if isinstance(model.get('model'),str) else 'unknown'
  body=json.dumps({'model':label,'messages':[{'role':'user','content':'offline fixture'}],'stream':False,'max_tokens':64}).encode()
  with socket.create_connection(('127.0.0.1',port),timeout=5) as s:
   s.sendall(f'POST /v1/chat/completions HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nContent-Length: {len(body)}\r\nContent-Type: application/json\r\nConnection: close\r\n\r\n'.encode()+body);raw=b''
   while data:=s.recv(8192):raw+=data
  (root/(name+'.http')).write_bytes(raw);head,b=raw.split(b'\r\n\r\n');reply=json.loads(b);r=receipt(i)
  assert head.startswith(b'HTTP/1.1 200') and b'Transfer-Encoding' not in head
  assert reply['choices']==[{'message':{'content':content},'finish_reason':'stop'}] and reply['route']==label
  assert reply['usage']=={'completion_tokens':3} and r['finish_reason']=='stop' and r['route']==label and r['requested_model']==requested and r['max_tokens_honoured']==True
  if reply['model']!=expected or r['reported_model']!=expected:failures.append({'case':name,'expected':expected,'reply':reply['model'],'receipt':r['reported_model']})
 assert not failures,failures
 print('MODEL_PROVENANCE_PASS',flush=True)
 if len(sys.argv)>3:
  launcher=sys.argv[3];owner=root/'bridge-root';source=root/'source';source.mkdir();(source/'source-idea.md').write_text('Offline fixture source for a bounded strategy draft. Human review required.')
  benv=dict(os.environ,QROC_GATEWAY=f'127.0.0.1:{port}',QROC_MODEL=label,QROC_MAX_TOKENS='64',QROC_TIMEOUT_SECONDS='15')
  bridge=subprocess.Popen([launcher,'--root',str(owner),'--bridge'],env=benv,stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=(root/'bridge.stderr').open('w'),text=True)
  assert select.select([bridge.stdout],[],[],5)[0];trace.append({'startup':json.loads(bridge.stdout.readline())})
  imported=ask('qroc-project-import',{'path':str(source),'title':'Offline provenance fixture'});pid=imported['project']['id']
  branch=ask('qroc-branch-create',{'project_id':pid,'name':'Provenance fixture','provider':'local-gateway','model':label});bid=branch['id']
  ask('qroc-scope-save',{'project_id':pid,'branch_id':bid,'nature':'internal','rationale':'Offline provenance proof','included':['STRATEGY_BRIEF'],'source_revision':0})
  results=[]
  for j,(name,model) in enumerate(cases,1):
   current=model;expected=model['model'] if isinstance(model.get('model'),str) else 'unknown'
   job=ask('qroc-generate-start',{'project_id':pid,'branch_id':bid,'document_type':'STRATEGY_BRIEF','context':'Offline fixture draft only.'});deadline=time.monotonic()+10
   while time.monotonic()<deadline:
    job=ask('qroc-job-status',{'id':job['id']})
    if job['state'] not in ['queued','running','cancelling']:break
    time.sleep(.02)
   assert job['state']=='succeeded',job
   docs=ask('qroc-document-list',{'project_id':pid,'branch_id':bid})['documents'];doc=next(d for d in docs if d['id']==job['document_id']);r=receipt(len(cases)+j);inputs=doc['generation_inputs']
   assert inputs['response_model']==r['reported_model']==expected,(inputs,r)
   assert inputs['model']==label and inputs['branch']['model']==label and r['requested_model']==requested
   assert inputs['route']==r['route']==label and doc['generation_finish_reason']==r['finish_reason']=='stop' and doc['content']==content
   results.append({'case':name,'response_model':inputs['response_model'],'reported_model':r['reported_model'],'request_model':inputs['model'],'requested_provider_model':r['requested_model'],'route':inputs['route'],'finish_reason':doc['generation_finish_reason']})
  # Verify actual persisted document bytes, not just bridge replies.
  persisted=[json.loads(p.read_text()) for p in (owner/'qroc/documents').rglob('*.json')];assert len(persisted)==len(cases)
  assert [d['generation_inputs']['response_model'] for d in sorted(persisted,key=lambda d:d['revision'])]==[m['model'] if isinstance(m.get('model'),str) else 'unknown' for _,m in cases]
  (root/'bridge-results.json').write_text(json.dumps(results,indent=2));print('INSTALLED_BRIDGE_PROVENANCE_PASS',flush=True)
finally:
 (root/'bridge-trace.json').write_text(json.dumps(trace,indent=2));(root/'upstream-requests.json').write_text(json.dumps(requests,indent=2));(root/'failures.json').write_text(json.dumps(failures,indent=2))
 if bridge:
  bridge.stdin.close()
  try:bridge.wait(timeout=3)
  except subprocess.TimeoutExpired:bridge.terminate();bridge.wait(timeout=3)
 if shim:shim.terminate();shim.wait(timeout=3)
 log.close();server.shutdown();server.server_close();thread.join()

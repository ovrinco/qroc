#!/usr/bin/env python3
"""Private transport fixture: queued persistence and legacy restart; no model inference."""
import sys,os,json,socket,threading,subprocess,time,pathlib,hashlib
launcher,rootarg,outarg=sys.argv[1:];root=pathlib.Path(rootarg);root.mkdir(exist_ok=False);trace=[];bodies=[];release=threading.Event();accepted=threading.Event();errors=[]
s=socket.socket();s.bind(('127.0.0.1',0));s.listen();s.settimeout(5)
def serve():
 try:
  for i in range(2):
   c,_=s.accept();c.settimeout(5);buf=b''
   while b'\r\n\r\n' not in buf:buf+=c.recv(4096)
   h,body=buf.split(b'\r\n\r\n',1);length=int(next(v for v in h.split(b'\r\n') if v.lower().startswith(b'content-length:')).split(b':')[1])
   while len(body)<length:body+=c.recv(4096)
   bodies.append(json.loads(body));accepted.set()
   if i==0:assert release.wait(5)
   payload=json.dumps({'choices':[{'message':{'content':'Private bounded fixture '+str(i)},'finish_reason':'stop'}]}).encode();c.sendall(b'HTTP/1.1 200 OK\r\nContent-Length: '+str(len(payload)).encode()+b'\r\nConnection: close\r\n\r\n'+payload);c.close()
 except Exception as ex:errors.append(str(ex))
t=threading.Thread(target=serve);t.start();env={k:v for k,v in os.environ.items() if not k.startswith('QROC_')};env.update(QROC_GATEWAY='127.0.0.1:'+str(s.getsockname()[1]),QROC_MODEL='private-token-queue-fixture',QROC_MAX_TOKENS='1024',QROC_TIMEOUT_SECONDS='45')
def start():
 p=subprocess.Popen([launcher,'--root',str(root),'--bridge'],stdin=subprocess.PIPE,stdout=subprocess.PIPE,text=True,env=env);trace.append(json.loads(p.stdout.readline()));return p
p=start();n=0
def ask(cmd,args={}):
 global n
 n+=1;p.stdin.write(json.dumps(dict(version=1,id=str(n),command=cmd,args=args))+'\n');p.stdin.flush();r=json.loads(p.stdout.readline());trace.append(r);assert r['ok'],r;return r['result']
pid=ask('qroc-project-create',dict(title='Private queue persistence',brain_dump='Fixture only; no inference'))['id'];bid=ask('qroc-branch-create',dict(project_id=pid,name='Fixture',provider='local-gateway',model=env['QROC_MODEL']))['id']
jobs=ask('qroc-batch-start',dict(project_id=pid,branch_id=bid,document_types=['STRATEGY_BRIEF','PRE_MORTEM']))['jobs'];assert accepted.wait(5)
queued=jobs[1];assert queued['state']=='queued' and queued['max_tokens']==1024 and queued['pinned_inputs'] is None
jobpath=root/'qroc/jobs'/ (queued['id']+'.json');durable=json.loads(jobpath.read_text());assert durable['max_tokens']==1024 and durable['state']=='queued'
release.set()
for _ in range(400):
 status=ask('qroc-job-status',{'id':queued['id']})
 if status['state'] not in ['queued','running']:break
 time.sleep(.01)
assert status['state']=='succeeded',status
assert status['max_tokens']==1024 and status['pinned_inputs']['max_tokens']==1024
assert all(v['max_tokens']==1024 for v in bodies) and len(bodies)==2
exp=ask('qroc-export',dict(project_id=pid));payload=json.loads(pathlib.Path(exp['path']).read_text());assert len(payload['documents'])==2 and all(d['generation_inputs']['max_tokens']==1024 for d in payload['documents'])
p.stdin.close();assert p.wait(5)==0;t.join(5);assert not t.is_alive() and not errors,errors
# An intentionally unadmitted historical record remains sentinel0 and cannot resume on restart.
legacy=dict(durable);legacy.pop('max_tokens');legacy['pinned_inputs']=None;legacy['output_sha256']=None;jobpath.write_text(json.dumps(legacy))
p=start();read=ask('qroc-job-status',{'id':queued['id']});assert read['max_tokens']==0 and read['state']=='interrupted' and read['pinned_inputs'] is None
s.settimeout(.2)
try:s.accept();raise AssertionError('Legacy restart connected to fixture')
except socket.timeout:pass
p.stdin.close();assert p.wait(5)==0;s.close()
pathlib.Path(outarg).write_text(json.dumps(dict(status='PASS',standing='private installed transport fixture; no real inference',launcher=launcher,root=str(root),checks=['queued child persists bound1024 before launch','launch pins same bound after predecessor completion','two request/doc/export bounds equal1024','legacy absent bound serializes0; restart interrupts and emits no request'],durable_queued=durable,legacy_readback=read,requests=bodies,trace=trace),indent=2)+'\n');print('T08_INSTALLED_TOKEN_QUEUE_PASS')

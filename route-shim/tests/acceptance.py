#!/usr/bin/python3
import json,os,pathlib,socket,subprocess,sys,threading,time,http.server
BIN=sys.argv[1];ROOT=pathlib.Path(__file__).resolve().parents[3];Q=ROOT.parents[1];PROBE=Q/'tools/task_probe.py'
private=pathlib.Path(sys.argv[2]) if len(sys.argv)>2 else pathlib.Path(os.environ.get('ROUTE_SHIM_FIXTURES',Q/'work/T301W2/fixtures'))
root=private/('acceptance-'+str(time.time_ns()));root.mkdir(parents=True)
SCRIPT='''#!/usr/bin/python3
import os,sys,json,pathlib,time,signal
cwd=pathlib.Path.cwd();root=pathlib.Path(__file__).parent
(root/'argv.json').write_text(json.dumps({'argv':sys.argv,'stdin':sys.stdin.read(),'cwd':str(cwd),'empty':not list(cwd.iterdir())}))
if pathlib.Path(__file__).name=='fake-codex-hang':
 signal.signal(signal.SIGTERM,signal.SIG_IGN)
 while True: time.sleep(.1)
if pathlib.Path(__file__).name=='fake-codex-fail':sys.exit(7)
out=sys.argv[sys.argv.index('-o')+1];pathlib.Path(out).write_text('fake response')
print(json.dumps({'type':'turn.completed','usage':{'output_tokens':3}}))
'''
for name in ['fake-codex','fake-codex-hang','fake-codex-fail']:
 p=root/name;p.write_text(SCRIPT);p.chmod(0o700)
def probe(*args):
 # The campaign probe is optional: campaigns without tools/task_probe.py skip it (route-shim checks below still run).
 if not PROBE.exists():print('task probe absent, skipped:',args[0]);return ''
 p=subprocess.run(['python3',str(PROBE),*map(str,args)],capture_output=True,text=True);print(p.stdout.strip());assert p.returncode==0,p.stdout+p.stderr;return p.stdout
route={'kind':'codex','bin':str(root/'fake-codex'),'model':'fixture-model','effort':'medium','metered':False}
routes={'fixture':route,'zero':dict(route,metered=True,budget=0),'one':dict(route,metered=True,budget=1),'hang':dict(route,bin=str(root/'fake-codex-hang')),'fail':dict(route,bin=str(root/'fake-codex-fail'))}
table=root/'routes.json'
def table_write():table.write_text(json.dumps({'routes':routes}))
def req(port,label='fixture',extra=None,disconnect=False):
 b={'model':label,'messages':[{'role':'system','content':'sys fixture'},{'role':'user','content':'user fixture'}],'max_tokens':64,'stream':False,'temperature':.2,'response_format':{'type':'json_object'},'unknown':'ignored'};b.update(extra or {});data=json.dumps(b).encode();s=socket.create_connection(('127.0.0.1',port));s.sendall(f'POST /v1/chat/completions HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nContent-Type: application/json\r\nContent-Length: {len(data)}\r\nConnection: close\r\n\r\n'.encode()+data)
 if disconnect:s.close();return
 s.settimeout(5);out=b''
 while True:
  c=s.recv(8192)
  if not c:break
  out+=c
 s.close();head,body=out.split(b'\r\n\r\n',1);assert b'Transfer-Encoding' not in head;assert int(next(l.split(b':')[1] for l in head.split(b'\r\n') if l.startswith(b'Content-Length:')))==len(body)
 return int(head.split()[1]),json.loads(body)
def serve(state):
 table_write();log=open(root/(state.name+'.stderr'),'wb');p=subprocess.Popen([BIN,'serve','--listen','127.0.0.1:0','--routes',str(table),'--state',str(state),'--timeout-ms','1200'],stdout=subprocess.PIPE,stderr=log,text=True,cwd=ROOT)
 line=p.stdout.readline().strip();assert line.startswith('LISTEN '),line;print(line);return p,int(line.rsplit(':',1)[1]),log
# Capture idle-listener probes before any fixture child exists.
probe('procs',ROOT,'--allow','route-shim');table_write();p=subprocess.run([BIN,'plan','--routes',str(table)],capture_output=True,text=True);assert p.returncode==0;print(p.stdout.strip());probe('procs',ROOT,'--allow','route-shim')
state=root/'state';p,port,log=serve(state)
try:
 probe('procs',ROOT,'--allow','route-shim');probe('ports',ROOT)
 assert req(port,'zero')[0]==429;assert not (root/'argv.json').exists();print('budget 0: 429, no child')
 assert req(port,'one')[0]==200;before=(root/'argv.json').stat().st_mtime_ns;assert req(port,'one')[0]==429;assert (root/'argv.json').stat().st_mtime_ns==before;print('budget 1: 200 then 429, no second child')
 assert req(port)[0]==200;r=json.loads((root/'argv.json').read_text());a=r['argv'];assert a[0]==str(root/'fake-codex');assert a[1:9]==['exec','--ephemeral','--ignore-user-config','--skip-git-repo-check','-s','read-only','-m','fixture-model'];assert r['empty'];assert 'sys fixture' in r['stdin'] and 'user fixture' in r['stdin'];assert str(state/'cwd') in r['cwd'];print('fake argv:',json.dumps(a));print('stdin received; private cwd empty:',r['empty'])
 assert req(port,extra={'stream':True})[0]==400;print('stream true: 400; extra fields ignored')
 assert req(port,'fail')[0]==502;assert req(port,'hang')[0]==504;print('non-zero child: 502; SIGTERM-ignoring timeout: 504')
 req(port,'hang',disconnect=True);time.sleep(.2);out=probe('procs',ROOT,'--allow','route-shim');assert 'fake-codex' not in out;print('disconnect: fake child absent within 2 s')
 (state/'STOP').touch();assert req(port)[0]==503;print('state STOP: 503')
 p2=subprocess.run([BIN,'status','--state',str(state)],capture_output=True,text=True);assert p2.returncode==0 and 'remaining_budget=' in p2.stdout;print('status receipts and remaining budget: PASS')
 rows=[json.loads(l) for l in (state/'receipts.jsonl').read_text().splitlines()];assert len(rows)==9,len(rows);required=set('id ts route kind requested_model reported_model binary_sha256 max_tokens max_tokens_honoured finish_reason bytes_in bytes_out status ms child_exit budget_left'.split());assert all(required<=r.keys() for r in rows);assert any(r['binary_sha256'] and len(r['binary_sha256'])==64 for r in rows);assert all(not r['max_tokens_honoured'] for r in rows);print('receipt schema, binary hash, Codex max_tokens_honoured=false: PASS')
finally:p.terminate();p.wait(timeout=3);log.close()
for name,changes in [('port8080',{'base':'http://127.0.0.1:8080'}),('nonloopback',{'kind':'llama','base':'http://192.0.2.1:8084'}),('localbin',{'bin':'/example/operator/.local/bin/codex'}),('mise-shims',{'bin':'/example/operator/.local/share/mise/shims/codex'}),('no-budget',{'metered':True})]:
 bad=dict(route,**changes);f=root/(name+'.json');f.write_text(json.dumps({'routes':{'bad':bad}}));r=subprocess.run([BIN,'plan','--routes',str(f)],capture_output=True,text=True);assert r.returncode==2;print(name,'exit',r.returncode,r.stderr.strip())
# The API fixture checks the header in memory and deliberately never logs the key.
class Handler(http.server.BaseHTTPRequestHandler):
 def log_message(self,*a):pass
 def do_POST(self):
  assert self.headers['Authorization']=='Bearer SENTINEL-T301-KEY';b=json.loads(self.rfile.read(int(self.headers['Content-Length'])));assert b['model']=='api-fixture';assert b['max_tokens']==64;assert 'temperature' not in b and 'response_format' not in b
  data=json.dumps({'choices':[{'message':{'content':'api fixture'},'finish_reason':'length'}],'model':'api-actual','usage':{'completion_tokens':64}}).encode();self.send_response(200);self.send_header('Content-Length',str(len(data)));self.end_headers();self.wfile.write(data)
server=http.server.HTTPServer(('127.0.0.1',0),Handler);t=threading.Thread(target=server.serve_forever);t.start();routes['api']={'kind':'apikey','base':f'http://127.0.0.1:{server.server_port}','model':'api-fixture','key_env':'T301_FIXTURE_KEY','metered':False};os.environ['T301_FIXTURE_KEY']='SENTINEL-T301-KEY';p,port,log=serve(root/'api-state')
try:
 status,result=req(port,'api');assert status==200 and result['model']=='api-actual' and result['choices'][0]['finish_reason']=='length';print('API fake upstream: returned model, length, max_tokens, key stdin: PASS')
finally:p.terminate();p.wait(timeout=3);log.close();server.shutdown();server.server_close();t.join();os.environ.pop('T301_FIXTURE_KEY')
for state in [root/'state',root/'api-state']:
 for f in state.rglob('*'):
  if f.is_file():assert b'SENTINEL-T301-KEY' not in f.read_bytes()
for f in root.glob('*.stderr'):assert b'SENTINEL-T301-KEY' not in f.read_bytes()
print('sentinel absent from receipts, stdout/stderr and state: PASS');probe('procs',ROOT,'--allow','route-shim')

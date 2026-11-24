#!/usr/bin/python3
"""Offline: T404's 502 with child_exit 0. A fake codex replays a stream file and treats `-o` as codex does
(relative to its own cwd; a failed write is reported on stderr and the exit stays 0). Relay started with a
relative --state, as T404's live-r2.sh did. No real codex, no provider, loopback port 0 only."""
import json,os,pathlib,shutil,socket,subprocess,sys,time
BIN=sys.argv[1];root=pathlib.Path(sys.argv[2]).resolve();root.mkdir(parents=True,exist_ok=False);mode=sys.argv[3] if len(sys.argv)>3 else 'fixed'
STREAMS=pathlib.Path(__file__).resolve().parent/'streams';SENTINELS=['sk-fixtureSENTINEL','eyJfixtureSENTINEL','env-fixtureSENTINEL-value']
FAKE='''#!/usr/bin/python3
import os,pathlib,sys
sys.stdin.read();out=pathlib.Path(sys.argv[sys.argv.index('-o')+1]);stream=pathlib.Path(%r).read_text()
sys.stdout.write(stream);sys.stdout.flush()
print('codex fixture stderr; token from env: '+os.environ.get('FIXTURE_API_KEY',''),file=sys.stderr)
if %r:
 try:out.write_text('{"contracts":[]}')
 except OSError as e:print(f'Failed to write last message file {out}: {e}',file=sys.stderr)
else:print(f'Failed to write last message file {out}: fixture write refused',file=sys.stderr)
sys.exit(0)
'''
def fake(name,stream,writes):p=root/name;p.write_text(FAKE%(str(STREAMS/stream),writes));p.chmod(0o700);return str(p)
route=lambda b:{'kind':'codex','bin':b,'model':'fixture-model','effort':'medium','metered':False}
routes={'pass':route(fake('fake-codex-pass','codex-pass.jsonl',True)),'turn-failed':route(fake('fake-codex-fail','codex-turn-failed.jsonl',True)),'no-output':route(fake('fake-codex-nooutput','codex-pass.jsonl',False))}
(root/'relay').mkdir();(root/'relay/routes.json').write_text(json.dumps({'routes':routes}))
env=dict(os.environ,FIXTURE_API_KEY='env-fixtureSENTINEL-value')
p=subprocess.Popen([BIN,'serve','--listen','127.0.0.1:0','--routes','relay/routes.json','--state','relay/state','--timeout-ms','20000'],cwd=root,env=env,stdout=subprocess.PIPE,stderr=open(root/'relay/stderr.txt','wb'),text=True)
try:
 port=int(p.stdout.readline().split(':')[-1]);state=root/'relay/state'
 def req(label):
  b=json.dumps({'model':label,'messages':[{'role':'system','content':'contract designer'},{'role':'user','content':'hardened idea fixture'}],'stream':False}).encode()
  s=socket.create_connection(('127.0.0.1',port));s.sendall(f'POST /v1/chat/completions HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nContent-Type: application/json\r\nContent-Length: {len(b)}\r\nConnection: close\r\n\r\n'.encode()+b);s.settimeout(15);out=b''
  while (c:=s.recv(8192)):out+=c
  s.close();head,body=out.split(b'\r\n\r\n',1);rows=(state/'receipts.jsonl').read_text().splitlines();return int(head.split()[1]),json.loads(body),json.loads(rows[-1])
 if mode=='repro':
  st,_,r=req('pass');lasts=sorted(x.name for x in state.glob('*.last'));print(f'repro pass stream, relative state: status={st} child_exit={r["child_exit"]} finish_reason={r["finish_reason"]} last_files={lasts}')
  assert st==502 and r['child_exit']==0 and r['finish_reason'] is None and not lasts;print('REPRO: T404 signature reproduced (502, child_exit 0, finish_reason null, no .last)');sys.exit(0)
 st,v,r=req('pass');assert st==200,(st,v);assert v['choices'][0]['message']['content']=='{"contracts":[]}' and r['finish_reason']=='stop' and r['child_exit']==0
 assert not (state/'failed').exists(),'a passing request keeps no failure log';print(f'pass stream: status={st} child_exit=0 finish_reason=stop usage={json.dumps(v["usage"])}; no failure log')
 for label,reason in [('turn-failed','turn.failed or error event'),('no-output','no output file')]:
  st,v,r=req(label);assert st==502 and r['child_exit']==0 and r['finish_reason'] is None,(st,r);d=state/'failed';i=r['id']
  summary=json.loads((d/f'{i}.json').read_text());events=(d/f'{i}.events.jsonl').read_text();err=(d/f'{i}.stderr.txt').read_text()
  assert summary['reason']==reason and summary['status']==502 and summary['child_exit']==0,summary
  recorded=(STREAMS/('codex-turn-failed.jsonl' if label=='turn-failed' else 'codex-pass.jsonl')).read_text()
  assert len(events.splitlines())==len(recorded.splitlines()) and [json.loads(l)['type'] for l in events.splitlines()]==summary['event_types']
  assert 'codex fixture stderr' in err and '***' in err
  if label=='no-output':assert 'Failed to write last message file' in err and not summary['output_file_present'] and events==recorded
  else:assert '***' in events
  assert all(oct((d/f'{i}{x}').stat().st_mode&0o777)=='0o600' for x in ['.json','.events.jsonl','.stderr.txt']) and oct(d.stat().st_mode&0o777)=='0o700'
  print(f'{label} stream: status=502 child_exit=0 reason="{reason}" kept events={len(events.splitlines())} lines stderr={len(err)} bytes mode 0600')
 leaked=[f for f in state.rglob('*') if f.is_file() and any(t in f.read_text(errors='replace') for t in SENTINELS)];assert not leaked,leaked
 print(f'secrets masked: 0 of {sum(1 for f in state.rglob("*") if f.is_file())} state files carry a sentinel')
 print('relay_502 offline: PASS (3 of 3 streams)')
finally:
 p.terminate();p.wait(5)

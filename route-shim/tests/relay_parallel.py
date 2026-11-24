import concurrent.futures,json,pathlib,socket,subprocess,sys,time
root=pathlib.Path(sys.argv[2]).resolve();root.mkdir(parents=True,exist_ok=False)
fake=root/'fake-codex';fake.write_text('''#!/usr/bin/python3
import pathlib,sys,time
sys.stdin.read()
time.sleep(2)
pathlib.Path(sys.argv[sys.argv.index('-o')+1]).write_text('fixture response')
print('{"type":"turn.completed","usage":{"input_tokens":1,"output_tokens":1}}')
''');fake.chmod(0o700)
(root/'routes.json').write_text(json.dumps({'routes':{'fixture':{'kind':'codex','bin':str(fake),'model':'fixture','effort':'medium','metered':True,'budget':3}}}))
def run(width):
 state=root/f'state-{width}'
 p=subprocess.Popen([sys.argv[1],'serve','--listen','127.0.0.1:0','--routes',str(root/'routes.json'),'--state',str(state)]+([] if width==3 else ['--max-concurrent',str(width)]),stdout=subprocess.PIPE,text=True)
 try:
  port=int(p.stdout.readline().split(':')[-1])
  def req(_):
   b=json.dumps({'model':'fixture','messages':[{'role':'user','content':'offline fixture'}]}).encode()
   with socket.create_connection(('127.0.0.1',port),timeout=15) as s:
    s.sendall(f'POST /v1/chat/completions HTTP/1.1\r\nContent-Length: {len(b)}\r\n\r\n'.encode()+b)
    data=b''
    while c:=s.recv(8192):data+=c
    return int(data.split()[1])
  start=time.monotonic()
  with concurrent.futures.ThreadPoolExecutor(3) as pool:statuses=list(pool.map(req,range(3)))
  elapsed=time.monotonic()-start
  deadline=time.monotonic()+2
  while time.monotonic()<deadline:
   rows=[json.loads(l) for l in (state/'receipts.jsonl').read_text().splitlines()]
   if len(rows)==3:break
   time.sleep(.01)
  budget=json.loads((state/'budget.json').read_text())
  assert statuses==[200]*3 and len(rows)==3 and budget['fixture']==0,(statuses,rows,budget)
  assert len({r['id'] for r in rows})==3 and sorted(r['budget_left'] for r in rows)==[0,1,2]
  if width==3:assert elapsed<5,elapsed
  else:assert elapsed>=6,elapsed
  print(f'width={width}: 3/3 requests status=200; seconds={elapsed:.3f}; spent=3/3; receipts=3/3; remaining=0',flush=True)
  assert req(4)==429
 finally:p.terminate();p.wait(5)
run(3);run(1)

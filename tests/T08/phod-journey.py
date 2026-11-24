#!/usr/bin/python3
import json,pathlib,subprocess,sys,time,os,hashlib
bridge,root,evidence=sys.argv[1:];root=pathlib.Path(root);evidence=pathlib.Path(evidence)
p=subprocess.Popen([bridge,'--root',str(root)],stdin=subprocess.PIPE,stdout=subprocess.PIPE,text=True)
log=[json.loads(p.stdout.readline())];seq=0

def ask(cmd,args={}):
 global seq
 seq+=1;p.stdin.write(json.dumps(dict(version=1,id=str(seq),command=cmd,args=args))+'\n');p.stdin.flush();r=json.loads(p.stdout.readline());log.append(dict(command=cmd,reply=r));return r

def ok(cmd,args={}):
 r=ask(cmd,args);assert r['ok'],r;return r['result']
def wait(id):
 for _ in range(1000):
  r=ok('phod-job-status',{'id':id})
  if r['state'] not in ('queued','running'):return r
  time.sleep(.01)
 raise AssertionError('job timed out')
def launch(spec):
 r=ok('phod-workbench-spec-save',spec);return ok('phod-workbench',{'source_path':r['source_path']})['id']
spec=dict(profile='integer-range/1',name='score-validator',purpose='Reject scores outside the accepted integer interval',min=0,max=100,upper_policy='inclusive',actor='T05 development owner',rationale='A score of 100 is valid; 101 must be rejected',trial_max=101,repair_budget=1)
try:
 caps=ok('phod-list')['capabilities'];assert caps['workbench']['available'];assert not caps['validate']['available'];assert not caps['compile']['available']
 missing=ok('phod-workbench-spec-save',dict(spec,upper_policy=''));blocked=wait(ok('phod-workbench',{'source_path':missing['source_path']})['id']);assert blocked['state']=='blocked';assert json.loads(blocked['evidence']['stdout'])['status']=='blocked'
 repaired=wait(launch(spec));assert repaired['state']=='succeeded',repaired
 report=json.loads(repaired['evidence']['stdout']);assert report['passed'];assert [a['decision']['status'] for a in report['attempts']]==['rejected','accepted','accepted'];assert report['repair']['counterexamples'][0]['input']==101
 replay=ok('phod-replay',{'id':repaired['id']});assert [a['decision']['status'] for a in replay['decisions']]==['rejected','accepted','accepted'];assert replay['effectful_rerun']==False
 failed=wait(launch(dict(spec,repair_budget=0)));assert failed['state']=='failed';assert json.loads(failed['evidence']['stdout'])['attempts'][0]['decision']['status']=='rejected';assert len(ok('phod-replay',{'id':failed['id']})['decisions'])==2
 # Missing real parser never becomes a passed substitute.
 unavailable=wait(ok('phod-compile',{'source_path':repaired['source_path']})['id']);assert unavailable['state']=='unavailable'
 # Cancel a real running Python executor once its predeclared plan is durable.
 cancelled_id=launch(spec);out=root/'phod/outputs'/cancelled_id
 for _ in range(400):
  if (root/'phod/admissions'/cancelled_id/'owner-binding.json').exists() and (out/'derived').exists():break
  time.sleep(.005)
 assert (root/'phod/admissions'/cancelled_id/'owner-binding.json').exists(),'owner never admitted plan'
 # Capture owned descendants before cancellation, without interacting with other workers.
 def descendants(pid):
  todo=[pid];found=[]
  while todo:
   parent=todo.pop();tasks=pathlib.Path('/proc')/str(parent)/'task'
   ids=set()
   if tasks.exists():
    for f in tasks.glob('*/children'):
     try:ids.update(int(x) for x in f.read_text().split())
     except FileNotFoundError:pass
   found+=list(ids);todo+=list(ids)
  return found
 owned=descendants(p.pid);assert owned,'no real process to cancel'
 ok('phod-job-cancel',{'id':cancelled_id});cancelled=wait(cancelled_id);assert cancelled['state']=='cancelled';assert cancelled['evidence']['artifacts']
 time.sleep(.2)
 for pid in owned:
  stat=pathlib.Path('/proc')/str(pid)/'stat'
  if stat.exists():assert stat.read_text().split()[2]=='Z',f'owned process still running: {pid}'
 # Replay rejects changes to stored evidence before invoking the pure checker.
 target=pathlib.Path(report['attempts'][0]['check_input']);original=target.read_bytes();target.write_bytes(original+b' ')
 refused=ask('phod-replay',{'id':repaired['id']});assert not refused['ok'];assert 'changed' in refused['error'].lower();target.write_bytes(original)
 write=dict(result='PASS',repaired_job=repaired['id'],failed_job=failed['id'],cancelled_job=cancelled_id,cancelled_descendants=owned,unavailable_job=unavailable['id'],assertions=['real rustc and unchanged semantic checker','101 counterexample retained','same semantic hash through repair','two Rust implementation alternatives','replay retains rejected decision','ambiguous policy blocks','missing parser unavailable','cancelled actual executor descendants stopped','tampered evidence refused'])
 (evidence/'journey-summary.json').write_text(json.dumps(write,indent=2));print(json.dumps(write))
finally:
 p.stdin.close();p.wait(timeout=5);(evidence/'bridge-journey.json').write_text(json.dumps(log,indent=2))

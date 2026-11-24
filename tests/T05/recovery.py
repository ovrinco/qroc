#!/usr/bin/python3
import pathlib,json,subprocess,time,sys,os,signal,hashlib
binary,root,out=sys.argv[1:];root=pathlib.Path(root);out=pathlib.Path(out);seq=0
p=None
def open_bridge():
 global p
 p=subprocess.Popen([binary,'--root',str(root)],stdin=subprocess.PIPE,stdout=subprocess.PIPE,text=True);assert json.loads(p.stdout.readline())['event']=='ready'
def ask(cmd,args={}):
 global seq
 seq+=1;p.stdin.write(json.dumps(dict(version=1,id='r'+str(seq),command=cmd,args=args))+'\n');p.stdin.flush();r=json.loads(p.stdout.readline());assert r['ok'],r;return r['result']
def wait(id):
 for _ in range(1000):
  r=ask('phod-job-status',{'id':id})
  if r['state'] not in ('queued','running'):return r
  time.sleep(.01)
 raise AssertionError('timeout')
try:
 open_bridge();spec=dict(profile='integer-range/1',name='recovery-score',purpose='Retain interrupted attempts',min=-8,max=12,upper_policy='exclusive',actor='T05 recovery owner',rationale='12 excluded',repair_budget=0)
 saved=ask('phod-workbench-spec-save',spec);id=ask('phod-workbench',{'source_path':saved['source_path']})['id']
 for _ in range(300):
  if (root/'phod/outputs'/id/'derived').exists():break
  time.sleep(.005)
 assert (root/'phod/outputs'/id/'derived').exists()
 ids=set()
 for f in (pathlib.Path('/proc')/str(p.pid)/'task').glob('*/children'):ids.update(int(x) for x in f.read_text().split())
 assert ids
 p.kill();p.wait()
 for pid in ids:
  try:os.killpg(pid,signal.SIGKILL)
  except ProcessLookupError:pass
 open_bridge();interrupted=ask('phod-job-status',{'id':id});assert interrupted['state']=='interrupted';assert interrupted['evidence']['artifacts'];assert 'restarted' in interrupted['error']
 rerun_id=ask('phod-workbench',{'source_path':interrupted['source_path']})['id'];rerun=wait(rerun_id);assert rerun['state']=='succeeded';assert rerun_id!=id;assert rerun['source_hash']==interrupted['source_hash'];assert ask('phod-job-status',{'id':id})['state']=='interrupted'
 decision=json.loads(rerun['evidence']['stdout']);assert decision['passed'];assert all(a['decision']['status']=='accepted' for a in decision['attempts'])
 # Independent upper-exclusive witnesses include 12 rejected and 11 accepted.
 obs=decision['attempts'][0]['observations'];assert next(o for o in obs if o['input']==12)['observed']=='false\n';assert next(o for o in obs if o['input']==11)['observed']=='true\n'
 result=dict(result='PASS',binary=binary,binary_sha256=hashlib.sha256(pathlib.Path(binary).read_bytes()).hexdigest(),root=str(root),interrupted_job=id,rerun_job=rerun_id,source_hash=rerun['source_hash'],artifacts_preserved=len(interrupted['evidence']['artifacts']),killed_owned_process_groups=list(ids),cgroup=pathlib.Path('/proc/self/cgroup').read_text(),observed='restart records interrupted; explicit rerun creates a new ID and preserves source and old disposition')
 (out/'recovery.json').write_text(json.dumps(result,indent=2));print('T05_RECOVERY_PASS',json.dumps(result))
finally:
 if p and p.poll() is None:p.stdin.close();p.wait(timeout=5)

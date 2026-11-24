#!/usr/bin/python3
import json,pathlib,subprocess,sys,time,os
binary,root,out=sys.argv[1:];out=pathlib.Path(out);env=dict(os.environ);env.pop('QROC_PHOD_ADMITTED_BIN',None);env.pop('QROC_PHOD_ADMITTED_SHA256',None)
p=subprocess.Popen([binary,'--root',root],stdin=subprocess.PIPE,stdout=subprocess.PIPE,text=True,env=env);seq=0
assert json.loads(p.stdout.readline())['event']=='ready'
def ask(c,a={}):
 global seq
 seq+=1;p.stdin.write(json.dumps(dict(version=1,id=str(seq),command=c,args=a))+'\n');p.stdin.flush();r=json.loads(p.stdout.readline());assert r['ok'],r;return r['result']
try:
 spec=dict(profile='integer-range/1',name='no-admitted-tool',purpose='No checker cannot pass',min=0,max=100,upper_policy='inclusive',actor='T05 development owner',rationale='100 valid',repair_budget=0)
 s=ask('phod-workbench-spec-save',spec);j=ask('phod-workbench',{'source_path':s['source_path']})
 for _ in range(400):
  r=ask('phod-job-status',{'id':j['id']})
  if r['state'] not in ('queued','running'):break
  time.sleep(.01)
 receipt=json.loads(r['evidence']['stdout']);assert r['state']=='unavailable';assert receipt['passed']==False;assert all(a['structural_decision']['status']=='accepted' and a['decision']['status']=='blocked' for a in receipt['attempts'])
 (out/'missing-admitted-checker.json').write_text(json.dumps(r,indent=2));print('T05_MISSING_ADMITTED_CHECKER_CANNOT_PASS')
finally:p.stdin.close();p.wait(timeout=5)

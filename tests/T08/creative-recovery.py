#!/usr/bin/env python3
import pathlib,json,subprocess,sys,time,os,signal,hashlib
launcher,root,input,out=sys.argv[1:];root=pathlib.Path(root);out=pathlib.Path(out);trace=[];seq=0;p=None
def open_owner():
 global p
 p=subprocess.Popen([launcher,'--root',str(root),'--bridge'],stdin=subprocess.PIPE,stdout=subprocess.PIPE,text=True);trace.append(json.loads(p.stdout.readline()))
def ask(c,a={},good=True):
 global seq
 seq+=1;q={'version':1,'id':str(seq),'command':c,'args':a};p.stdin.write(json.dumps(q)+'\n');p.stdin.flush();r=json.loads(p.stdout.readline());trace.append({'request':q,'reply':r});assert r['ok']==good,r;return r['result'] if good else r['error']
def children(pid):
 found=set()
 for f in (pathlib.Path('/proc')/str(pid)/'task').glob('*/children'):
  try:found.update(int(x) for x in f.read_text().split())
  except FileNotFoundError:pass
 return found
def stopped(ids):
 for pid in ids:
  f=pathlib.Path('/proc')/str(pid)/'stat'
  if f.exists():assert f.read_text().split()[2]=='Z',pid
open_owner()
try:
 project=ask('qroc-project-create',{'title':'Installed creative recovery','brain_dump':'Retain cancelled/interrupted outputs honestly; no publication.'});branch=ask('qroc-branch-create',{'project_id':project['id'],'name':'Owned source'});owner={'project_id':project['id'],'branch_id':branch['id']}
 src=ask('qroc-creative-source-import',dict(owner,path=input));args=dict(owner,adapter_id='ffmpeg-audio-wav-v1',asset_id=src['asset_id'],source_revision=src['revision'],duration_seconds=30,volume=.5)
 running=ask('qroc-creative-start',args);ids=children(p.pid);assert ids,'no real ffmpeg process observed'
 cancelled=ask('qroc-creative-cancel',{'artifact_id':running['artifact_id'],'revision':running['revision']});assert cancelled['status']=='cancelled';ask('qroc-creative-export',{'artifact_id':running['artifact_id'],'revision':running['revision']},False);time.sleep(.1);stopped(ids)
 interrupted=ask('qroc-creative-start',args);ids2=children(p.pid);assert ids2,'no real ffmpeg child observed before crash';p.kill();p.wait();time.sleep(.2);stopped(ids2)
 open_owner();records=ask('qroc-creative-list',owner)['outputs'];record=next(v for v in records if v['artifact_id']==interrupted['artifact_id']);assert record['status']=='interrupted';ask('qroc-creative-export',{'artifact_id':record['artifact_id'],'revision':record['revision']},False)
 assert any(v['status']=='cancelled' for v in records);assert ask('qroc-creative-list',owner)['assets'][0]['sha256']==src['sha256']
 print('T08_INSTALLED_CREATIVE_RECOVERY_PASS')
finally:
 if p and p.poll() is None:p.stdin.close();p.wait(timeout=5)
 out.write_text(json.dumps({'launcher':launcher,'root':str(root),'source':input,'source_sha256':hashlib.sha256(pathlib.Path(input).read_bytes()).hexdigest(),'trace':trace,'standing':'actual ffmpeg child/cancel/crash-interrupt, refused exports, source+history retained'},indent=2)+'\n')

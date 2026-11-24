import json,subprocess,sys,time,pathlib
pid=int(sys.argv[1]);config=pathlib.Path(sys.argv[2])
def get(what):
 r=subprocess.run(['hyprctl','-j',what],capture_output=True,text=True)
 return json.loads(r.stdout) if r.returncode==0 else {'unavailable':r.stderr}
monitors=get('monitors');workspace=get('activeworkspace');clients=[]
for i in range(50):
 allclients=get('clients')
 if isinstance(allclients,list):clients=[{k:c.get(k) for k in ['pid','monitor','workspace','at','size','floating','mapped','fullscreen','class','title']} for c in allclients if c.get('pid')==pid]
 if clients:break
 time.sleep(.02)
summary={'qs_pid':pid,'read_only':True,'monitors':[{k:m.get(k) for k in ['name','width','height','scale','x','y','activeWorkspace','focused']} for m in monitors] if isinstance(monitors,list) else monitors,'active_workspace':{k:workspace.get(k) for k in ['id','name','monitor','monitorID','windows','hasfullscreen','tiledLayout']} if isinstance(workspace,dict) else workspace,'owned_clients_only':clients,'standing':'current placement only; historical failing placement unknown; no window/config/focus changes'}
(config/'monitor-context.json').write_text(json.dumps(summary,indent=2)+'\n')

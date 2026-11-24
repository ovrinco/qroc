#!/usr/bin/python3
import json,pathlib,subprocess,hashlib,sys,copy
root,binary,out=sys.argv[1:];root=pathlib.Path(root);out=pathlib.Path(out)
jobs=[json.loads(p.read_bytes()) for p in (root/'phod/jobs').glob('*.json')]
job=next(j for j in jobs if j['state']=='succeeded');report=json.loads(job['evidence']['stdout']);owner=pathlib.Path(report['owner_directory']);admission=owner/'owner-admission.json'
assert 'sha256:'+hashlib.sha256(admission.read_bytes()).hexdigest()==report['admission_hash']
x=json.loads(pathlib.Path(report['attempts'][-1]['check_input']).read_bytes());x['evidence'][0]['tool']['digest']='sha256:'+'0'*64
mutated=out/'unadmitted-verifier-input.json';mutated.write_text(json.dumps(x))
r=subprocess.run([binary,'check-admitted',str(mutated),str(admission)],capture_output=True,text=True,check=False);decision=json.loads(r.stdout)
assert r.returncode==1;assert any(d.startswith('untrusted_verifier:') for d in decision['diagnostics'])
(out/'unadmitted-verifier-decision.json').write_text(json.dumps(dict(argv=r.args,exit_code=r.returncode,decision=decision,owner_admission_sha256=hashlib.sha256(admission.read_bytes()).hexdigest()),indent=2));print('T05_UNADMITTED_VERIFIER_REJECTED')

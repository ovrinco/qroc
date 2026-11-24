#!/usr/bin/python3
import sys
sys.dont_write_bytecode=True
from workbench import *
def main():
 for line in (HERE/'manifest.sha256').read_text().splitlines():
  expected,relative=line.split()
  if hashlib.sha256((HERE/relative).read_bytes()).hexdigest()!=expected:raise ValueError('tool manifest mismatch: '+relative)
 raw=pathlib.Path(sys.argv[1]).read_bytes();spec=json.loads(raw,object_pairs_hook=unique)
 allowed={'profile','name','purpose','min','max','upper_policy','actor','rationale','trial_max','repair_budget'}
 if set(spec)-allowed:raise ValueError('unsupported required fields: '+str(sorted(set(spec)-allowed)))
 if spec.get('profile')!='integer-range/1':raise ValueError('unsupported profile; select integer-range/1')
 lo,hi=spec.get('min'),spec.get('max')
 if type(lo)!=int or type(hi)!=int or not -1000000<=lo<hi<=1000000:raise ValueError('bounded min/max integers required')
 if spec.get('upper_policy') not in ('inclusive','exclusive'):
  print(json.dumps(dict(passed=False,status='blocked',question={'id':'upper-policy','options':['inclusive','exclusive'],'reason':'The upper boundary needs a human decision.'})));return 1
 if not all(isinstance(spec.get(k),str) and 0<len(spec[k])<=512 for k in ('actor','rationale','name','purpose')):raise ValueError('explicit actor, rationale, name and purpose required')
 budget=spec.get('repair_budget',0)
 if type(budget)!=int or budget not in (0,1):raise ValueError('repair budget must be 0 or 1')
 maximum=hi if spec['upper_policy']=='inclusive' else hi-1
 trial=spec.get('trial_max',maximum)
 if type(trial)!=int or abs(trial)>1000000:raise ValueError('trial_max must be a bounded integer')
 rustc=pathlib.Path(os.environ['RUSTC']).resolve();semantic=HERE/'bin/phod-semantic';derive=HERE/'bin/phod-local-derive'
 env={'digest':digest(encode({'rustc':tool(rustc),'version':run([rustc,'--version'])['stdout'],'executor':tool(HERE/'workbench.py'),'owner':tool(pathlib.Path(__file__)),'system':os.uname().sysname,'machine':os.uname().machine}))}
 # The witness plan is fixed before either implementation. It is deliberately finite.
 cases=sorted(set([lo-1,lo,lo+1,(lo+hi)//2,hi-1,hi,hi+1]))
 expected={str(n):str(lo<=n<=maximum).lower()+'\n' for n in cases}
 plan=dict(profile='integer-range/1',spec_hash=digest(raw),cases=expected,bounds={'timeout_seconds_per_process':20,'cases':len(cases)},tools={'rustc':tool(rustc),'derive':tool(derive),'semantic':tool(semantic),'executor':tool(HERE/'workbench.py'),'owner':tool(pathlib.Path(__file__))},decision={k:spec[k] for k in ('upper_policy','actor','rationale')},limitations=['finite integer witnesses only','generated template only; no arbitrary code or models','no Zig/parser capability claimed'])
 write(pathlib.Path('plan.json'),plan)
 obligations=[dict(id='compile',semantic_key='compile',required=True,claim={'kind':'compile','language':'rust'},evidence={'kind':'compile','tool':'rustc'})]
 for n in cases:obligations.append(dict(id='case-'+str(n),semantic_key='range.case.'+str(n),required=True,claim={'kind':'runtime_exact','stdout':expected[str(n)],'exit_code':0},evidence={'kind':'runtime_exact','tool':'integer-range-executor'}))
 ir=dict(dialect='phod-ir/1',edition='phod-v1',source={'hash':digest(raw),'raw_ref':str(pathlib.Path(sys.argv[1]).resolve())},intent={'name':spec['name'],'purpose':spec['purpose']},obligations=obligations)
 write(pathlib.Path('ir-input.json'),ir)
 ir=json.loads(run([semantic,'normalize','ir','ir-input.json'])['stdout']);write(pathlib.Path('ir.json'),ir)

 compiler={'name':'rustc','version':run([rustc,'--version'])['stdout'].strip(),'digest':tool(rustc)['sha256']}
 executor={'name':'integer-range-executor','version':'1','digest':tool(HERE/'workbench.py')['sha256']}
 admission=dict(profile='qroc-phod-replay/1',source_bytes=list(raw),source_ref=str(pathlib.Path(sys.argv[1]).resolve()),semantic_hash=ir['semantic_hash'],environment_digest=env['digest'],verifiers={o['id']:compiler if o['id']=='compile' else executor for o in obligations})
 write(pathlib.Path('owner-admission.json'),admission)
 admitted=None
 selected=os.environ.get('QROC_PHOD_ADMITTED_BIN');expected_digest=os.environ.get('QROC_PHOD_ADMITTED_SHA256')
 if selected:
  selected=pathlib.Path(selected)
  if not selected.is_absolute() or not expected_digest or tool(selected)['sha256']!='sha256:'+expected_digest:raise ValueError('owner-selected admitted checker identity mismatch')
  admitted=tool(selected)
 binding=dict(spec_hash=digest(raw),semantic_hash=ir['semantic_hash'],plan_hash=digest(pathlib.Path('plan.json').read_bytes()),admission_hash=digest(pathlib.Path('owner-admission.json').read_bytes()),ir_hash=digest(pathlib.Path('ir.json').read_bytes()),admitted_checker=admitted,profile='integer-range/1',lo=lo,hi=hi,maximum=maximum,trial=trial,budget=budget)
 write(pathlib.Path('owner-binding.json'),binding)
 print(json.dumps(binding));return 0
if __name__=='__main__':
 try:sys.exit(main())
 except Exception as e:print(json.dumps({'passed':False,'status':'failed','error':str(e)}));sys.exit(1)

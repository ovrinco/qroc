#!/usr/bin/python3
"""Bounded integer-range profile; executor outside the unchanged pure ring."""
import hashlib,json,os,pathlib,subprocess,sys
HERE=pathlib.Path(__file__).resolve().parent

def digest(b):return 'sha256:'+hashlib.sha256(b).hexdigest()
def encode(v):return json.dumps(v,sort_keys=True,separators=(',',':'),ensure_ascii=False).encode()
def write(p,v):p.write_bytes(encode(v))
def unique(pairs):
 result={}
 for k,v in pairs:
  if k in result:raise ValueError('duplicate specification key: '+k)
  result[k]=v
 return result

def tool(p):return dict(path=str(p),sha256=digest(p.read_bytes()))
def run(argv,allowed=(0,)):
 r=subprocess.run([str(x) for x in argv],stdin=subprocess.DEVNULL,capture_output=True,timeout=20)
 if len(r.stdout)+len(r.stderr)>1024*1024:raise ValueError('executor output exceeds 1 MiB')
 record=dict(argv=[str(x) for x in argv],exit_code=r.returncode,stdout=r.stdout.decode(),stderr=r.stderr.decode())
 if r.returncode not in allowed:raise ValueError(json.dumps(record))
 return record

def main():
 owner=pathlib.Path(os.environ['PHOD_OWNER_DIRECTORY'])
 binding=json.loads((owner/'owner-binding.json').read_bytes())
 raw=pathlib.Path(sys.argv[1]).read_bytes();spec=json.loads(raw,object_pairs_hook=unique)
 if digest(raw)!=binding['spec_hash']:raise ValueError('source differs from owner admission')
 for name,key in [('plan.json','plan_hash'),('owner-admission.json','admission_hash'),('ir.json','ir_hash')]:
  if digest((owner/name).read_bytes())!=binding[key]:raise ValueError('owner binding changed')
 plan=json.loads((owner/'plan.json').read_bytes());ir=json.loads((owner/'ir.json').read_bytes());admission=json.loads((owner/'owner-admission.json').read_bytes())
 lo,hi,maximum,trial,budget=[binding[k] for k in ('lo','hi','maximum','trial','budget')]
 expected=plan['cases'];cases=sorted(int(n) for n in expected)
 env={'digest':admission['environment_digest']}
 rustc=pathlib.Path(plan['tools']['rustc']['path']);semantic=HERE/'bin/phod-semantic';derive=HERE/'bin/phod-local-derive'
 for key in ('rustc','derive','semantic','executor'):
  if tool(pathlib.Path(plan['tools'][key]['path']))['sha256']!=plan['tools'][key]['sha256']:raise ValueError('tool changed after owner admission')
 admitted=binding['admitted_checker']
 if admitted and tool(pathlib.Path(admitted['path']))['sha256']!=admitted['sha256']:raise ValueError('admitted checker changed')
 attempts=[]
 def verify(label,source,origin):
  folder=pathlib.Path(label);folder.mkdir()
  artifact=folder/'candidate.rs';artifact.write_text(source)
  manifest={'candidate.rs':digest(artifact.read_bytes())};aref={'bytes_hash':digest(artifact.read_bytes()),'manifest_hash':digest(encode(manifest))}
  compile_run=run([rustc,'--edition=2021',artifact,'-o',folder/'candidate'],allowed=(0,1));write(folder/'compiler.json',compile_run)
  evidence=[];observations=[]
  evidence.append(dict(id=label+'-compile',obligation_id='compile',semantic_hash=ir['semantic_hash'],artifact=aref,environment=env,tool={'name':'rustc','version':run([rustc,'--version'])['stdout'].strip(),'digest':tool(rustc)['sha256']},authority='external_executor',outcome='proven' if compile_run['exit_code']==0 else 'failed',payload={'kind':'compile'} if compile_run['exit_code']==0 else {'kind':'failed','reason':compile_run['stderr']}))
  if compile_run['exit_code']==0:
   for n in cases:
    obs=run([folder.resolve()/'candidate',str(n)]);observations.append(dict(input=n,expected=expected[str(n)],observed=obs['stdout'],exit_code=obs['exit_code']))
    evidence.append(dict(id=label+'-case-'+str(n),obligation_id='case-'+str(n),semantic_hash=ir['semantic_hash'],artifact=aref,environment=env,tool={'name':'integer-range-executor','version':'1','digest':tool(pathlib.Path(__file__))['sha256']},authority='external_executor',outcome='proven',payload={'kind':'runtime_exact','stdout':obs['stdout'],'exit_code':obs['exit_code']}))
  check_input=dict(candidate=ir,artifact={'bytes':list(artifact.read_bytes()),'manifest':manifest,'declared':aref},environment=env,evidence=evidence)
  write(folder/'check-input.json',check_input)
  decision_run=run([semantic,'check',folder/'check-input.json'],allowed=(0,1));structural=json.loads(decision_run['stdout']);write(folder/'structural-decision.json',structural)
  if admitted:
   decision=json.loads(run([admitted['path'],'check-admitted',folder/'check-input.json',owner/'owner-admission.json'],allowed=(0,1))['stdout'])
  else:decision={'status':'blocked','diagnostics':['admitted checker unavailable'],'structural':structural}
  write(folder/'decision.json',decision);write(folder/'witnesses.json',observations)
  result=dict(id=label,origin=origin,semantic_hash=ir['semantic_hash'],artifact=aref,decision=decision,structural_decision=structural,observations=observations,check_input=str((folder/'check-input.json').resolve()),differences={'source_bytes':artifact.stat().st_size,'implementation':origin})
  attempts.append(result);return result
 def derived(upper):
  canonical=dict(schema_version=1,source_path=ir['source']['raw_ref'],source_hash=digest(raw),intent={'name':'range_candidate','purpose':'Bounded integer range','fields':[{'name':'value','type':'Int'}]},constraints={'target.language':'rust','validation.min':lo,'validation.max':upper},derived_present=False)
  write(pathlib.Path('canonical.json'),canonical);run([derive,'canonical.json'])
  text=pathlib.Path('range_candidate.rs').read_text()
  # Keep the retained validation body. Only replace its sample main/test wrapper.
  body=text[:text.index('fn main()')]
  return body+'fn main() { let n: i64 = std::env::args().nth(1).unwrap().parse().unwrap(); println!("{}", validate(n)); }\n'
 first=verify('derived',derived(trial),'retained Rust codegen; trial upper '+str(trial))
 # An independently expressed equivalent Rust alternative shares exactly the same plan.
 second=verify('range-expression','fn validate(value:i64)->bool { ('+str(lo)+'..='+str(maximum)+').contains(&value) }\nfn main(){let n:i64=std::env::args().nth(1).unwrap().parse().unwrap();println!("{}",validate(n));}\n','Rust inclusive range expression; executor-owned bounded template')
 repair=None
 if first['structural_decision']['status']=='rejected' and budget:
  repair=dict(parent='derived',counterexamples=[o for o in first['observations'] if o['observed']!=o['expected']],frozen_semantic_hash=ir['semantic_hash'],allowed_edit='candidate upper constant only; specification, witness plan and rule unchanged',budget=1)
  repair['candidate']=verify('repaired',derived(maximum),'retained Rust codegen; approved upper restored')['id'];write(pathlib.Path('repair.json'),repair)
 passed=all(a['decision']['status']=='accepted' for a in attempts if a['id']!='derived' or not repair)
 report=dict(passed=passed,status='verified' if passed else ('unavailable' if not admitted else 'refuted'),profile='integer-range/1',spec_hash=digest(raw),semantic_hash=ir['semantic_hash'],plan_hash=binding['plan_hash'],owner_directory=str(owner),admission_hash=binding['admission_hash'],admitted_checker=admitted,attempts=attempts,repair=repair,unavailable={'zig':'No admitted Zig compiler/parser'},replay='Recheck preserved check-input.json with the unchanged pure semantic checker; never execute candidate binaries',checks=[dict(name=a['id'],passed=a['decision']['status']=='accepted',detail=a['origin']) for a in attempts])
 write(pathlib.Path('workbench.json'),report);print(json.dumps(report));return 0 if passed else 1
if __name__=='__main__':
 try:sys.exit(main())
 except Exception as e:print(json.dumps({'passed':False,'status':'failed','error':str(e)}));sys.exit(1)

#!/usr/bin/env python3
"""Independent bounded oracle and differential replay. All writes in owned scratch."""
import argparse, copy, hashlib, json, pathlib, subprocess
p=argparse.ArgumentParser();p.add_argument('--binary',required=True);p.add_argument('--scratch',required=True);a=p.parse_args()
scratch=pathlib.Path(a.scratch);scratch.mkdir(parents=True,exist_ok=True)
results=[]
def digest(b):return 'sha256:'+hashlib.sha256(b).hexdigest()
def put(name,v):
    path=scratch/name;path.write_text(json.dumps(v,ensure_ascii=False));return str(path)
def invoke(args,code=0):
    r=subprocess.run([a.binary,*args],capture_output=True,text=True,timeout=10)
    assert r.returncode==code,(args,r.returncode,r.stdout,r.stderr)
    return json.loads(r.stdout) if r.stdout else r.stderr
spec={'profile':'qroc-record/1','name':'person','text_field':'name','integer_field':'age','minimum':0,'maximum':150,'unknown_fields':'reject'}
sp=put('seed.json',spec)
c=invoke(['normalize','record',sp]);raw=pathlib.Path(sp).read_bytes()
# Independent Python oracle uses exact type (bool != integer) and finite contract.
def oracle(s,r):
    if not isinstance(r,dict):return False
    if type(r.get(s['text_field'])) is not str or len(r[s['text_field']])==0:return False
    n=r.get(s['integer_field'])
    if type(n) is not int or not s['minimum']<=n<=s['maximum']:return False
    return s['unknown_fields']!='reject' or not set(r)-{s['text_field'],s['integer_field']}
cases=[{'name':'Ada','age':0},{'name':'M','age':150},{'name':'','age':7},{'name':'a','age':151},{'name':'a','age':-1},{'name':'a','age':True},{'name':'a','age':2.0},{'name':'a','age':'3'},{'name':'a'},{'age':3},{'name':'a','age':8,'extra':1},{'name':'λ','age':32}]
# Held-out non-seed contract changes names, negative bounds, and policy.
held={**spec,'name':'stock','text_field':'sku','integer_field':'delta','minimum':-8,'maximum':12,'unknown_fields':'preserve'}
held_cases=[{'sku':'x','delta':-8},{'sku':'y','delta':12,'warehouse':'A'},{'sku':'z','delta':13},{'sku':'','delta':0},{'sku':'x','delta':False},{'sku':'x','delta':-9}]
for label,s,rs in [('seed',spec,cases),('held-out',held,held_cases),('ignore',{**held,'unknown_fields':'ignore'},held_cases)]:
    path=put(label+'.json',s)
    for i,r in enumerate(rs):
        expected=oracle(s,r);out=invoke(['record-inspect',path,put('record.json',r)],0 if expected else 1)
        assert out['valid']==expected
        if expected and s['unknown_fields']=='ignore':assert set(out['record'])=={s['text_field'],s['integer_field']}
        if expected and s['unknown_fields']=='preserve':assert out['record']==r
        results.append({'group':label,'case':i,'input':r,'expected':expected,'observed':out})
# Multiple valid implementations against same seed cases: direct predicates vs error-list.
def implementation_b(r):
    errors=[]
    if not isinstance(r,dict):return False
    if not isinstance(r.get('name'),str) or r.get('name')=='':errors.append('name')
    if type(r.get('age'))!=int:errors.append('type')
    elif r['age']<0 or r['age']>150:errors.append('range')
    if any(k not in ('name','age') for k in r):errors.append('unknown')
    return not errors
assert all(implementation_b(r)==oracle(spec,r) for r in cases)
# Retain known bad implementation counterexample; never count as accepted.
assert not oracle(spec,{'name':'x','age':151})
results.append({'group':'counterexample','bad_implementation':'maximum=151','input':{'name':'x','age':151},'oracle':False,'bad_observed':True})
# Check profile with source independently pinned and exact tool admission.
artifact=b'bounded-test-candidate';manifest={};tool={'name':'T04-independent-oracle','version':'1','digest':digest(pathlib.Path(__file__).read_bytes())}
ref={'bytes_hash':digest(artifact),'manifest_hash':digest(b'{}')};env={'digest':digest(b'T04-private-python-oracle')}
e={'id':'e1','obligation_id':'record.contract','semantic_hash':c['semantic_hash'],'artifact':ref,'environment':env,'tool':tool,'authority':'independent_checker','outcome':'proven','payload':{'kind':'property','seed':0,'cases':len(cases),'counterexample':None}}
x={'candidate':c,'artifact':{'bytes':list(artifact),'manifest':manifest,'declared':ref},'environment':env,'evidence':[e]}
ad={'profile':'qroc-phod-replay/1','source_bytes':list(raw),'source_ref':sp,'semantic_hash':c['semantic_hash'],'environment_digest':env['digest'],'verifiers':{'record.contract':tool}}
# This replay evidence is test-owned, not integrated compiler evidence.
def check(x,ad,code):return invoke(['check-admitted',put('input.json',x),put('admission.json',ad)],code)
base=check(x,ad,0)
mutations={
 'source':lambda y:y['candidate']['source'].update(hash=digest(b'other')),
 'raw-ref':lambda y:y['candidate']['source'].update(raw_ref='other'),
 'artifact':lambda y:y['artifact']['bytes'].append(0),
 'environment':lambda y:y['environment'].update(digest=digest(b'other')),
 'tool-version':lambda y:y['evidence'][0]['tool'].update(version='2'),
 'tool-digest':lambda y:y['evidence'][0]['tool'].update(digest=digest(b'other')),
 'self-attestation':lambda y:y['evidence'][0].update(authority='derivation_assertion'),
 'manual-attestation':lambda y:y['evidence'][0].update(authority='manual_assertion'),
 'tool-name':lambda y:y['evidence'][0]['tool'].update(name='imposter'),
 'manifest':lambda y:y['artifact']['manifest'].update(extra='changed'),
 'blocked':lambda y:y['evidence'][0].update(outcome='blocked'),
 'failed':lambda y:y['evidence'][0].update(outcome='failed'),
 'unsupported':lambda y:y['evidence'][0].update(outcome='unsupported'),
 'refuted':lambda y:y['evidence'][0].update(outcome='refuted',payload={'kind':'property','seed':0,'cases':12,'counterexample':{'input':'age=151','minimized':'151'}}),
 'missing':lambda y:y.update(evidence=[]),
 'duplicate':lambda y:y['evidence'].append(copy.deepcopy(y['evidence'][0])),
 'cross-obligation':lambda y:y['evidence'][0].update(obligation_id='elsewhere'),
 'stale-semantic':lambda y:y['evidence'][0].update(semantic_hash=digest(b'other')),
 'counterexample':lambda y:y['evidence'][0]['payload'].update(counterexample={'input':'151','minimized':'151'}),
 'zero-cases':lambda y:y['evidence'][0]['payload'].update(cases=0),
}
for name,mutate in mutations.items():
    y=copy.deepcopy(x);mutate(y);out=check(y,ad,1);results.append({'group':'identity-negative','case':name,'decision':out})
for key,val in [('profile','other'),('semantic_hash',digest(b'other')),('verifiers',{})]:
    policy=copy.deepcopy(ad);policy[key]=val;out=check(x,policy,1);results.append({'group':'admission-negative','case':key,'decision':out})
# Evidence-set identity insensitive to ordering; hash changes when payload changes.
y=copy.deepcopy(x);y['evidence'][0]['payload']['seed']=1
assert check(y,ad,0)['evidence_hash']!=base['evidence_hash']
assert check(x,ad,0)==base
# Deterministic evidence-set identity under reordering (duplicate set still rejected).
y=copy.deepcopy(x);extra=copy.deepcopy(y['evidence'][0]);extra['id']='e2';y['evidence'].append(extra)
d1=check(y,ad,1);y['evidence'].reverse();d2=check(y,ad,1);assert d1==d2
# Unknown current property cannot pass even with owner-pinned semantic hash.
u=copy.deepcopy(c);u['obligations'][0]['claim']={'kind':'property','predicate':'magic_is_safe'}
u=invoke(['normalize','ir',put('unknown-property.json',u)])
y=copy.deepcopy(x);y['candidate']=u;y['evidence'][0]['semantic_hash']=u['semantic_hash'];policy=copy.deepcopy(ad);policy['semantic_hash']=u['semantic_hash']
assert 'unsupported_profile_claim:record.contract' in check(y,policy,1)['diagnostics']
# Formatting source changes source hash but not semantic projection.
pretty=scratch/'pretty.json';pretty.write_text(json.dumps(spec,indent=2));c2=invoke(['normalize','record',str(pretty)])
assert c2['semantic_hash']==c['semantic_hash'] and c2['source']['hash']!=c['source']['hash']
# Semantic bound change changes semantic hash.
changed=invoke(['normalize','record',put('changed.json',{**spec,'maximum':151})]);assert changed['semantic_hash']!=c['semantic_hash']
# Fail closed parse/lowering diagnostics.
for name,v in [('policy-missing',{k:v for k,v in spec.items() if k!='unknown_fields'}),('unknown-key',{**spec,'network':True}),('wrong-profile',{**spec,'profile':'domain-charter'}),('range',{**spec,'minimum':151}),('alias',{**spec,'integer_field':'name'})]:
    r=subprocess.run([a.binary,'normalize','record',put('bad.json',v)],capture_output=True,text=True);assert r.returncode==2;results.append({'group':'parse-negative','case':name,'diagnostic':r.stderr})
dup=scratch/'duplicate.json';dup.write_text('{"name":"a","name":"b","age":2}')
r=subprocess.run([a.binary,'record-inspect',sp,str(dup)],capture_output=True,text=True);assert r.returncode==2 and 'duplicate_record_field' in r.stderr
# Legacy fixture raw bytes preserved; source hash is explicitly placeholder provenance.
legacy=pathlib.Path('phod-semantic/tests/fixtures/legacy-v1.json');v=json.loads(legacy.read_text());old=invoke(['normalize','legacy',str(legacy)])
for edition in [0,1]:
    v['schema_version']=edition
    normal=invoke(['normalize','legacy',put('legacy.json',v)])
    assert normal['edition']=='phod-v'+str(edition)
    roundtrip=invoke(['normalize','ir',put('roundtrip.json',normal)]);assert roundtrip==normal
    results.append({'group':'legacy-current-positive','edition':edition,'semantic_hash':normal['semantic_hash']})
    for constraints in [{'validation.magic':True},{'verification.required_checks':['stdout_exact_match']},{'verification.required_checks':['unknown']},{'safety.no_network':True}]:
        variant=copy.deepcopy(v);variant['constraints'].update(constraints)
        lowered=invoke(['normalize','legacy',put('legacy.json',variant)])
        assert any(o['claim']['kind']=='unsupported' for o in lowered['obligations'])
        results.append({'group':'legacy-negative','edition':edition,'constraints':constraints,'obligations':lowered['obligations']})
# Malformed or duplicate semantic legacy fields must not silently disappear.
for intent_patch in [{'fields':'not-an-array'},{'fields':[{'name':'a','type':'Int'},{'name':'a','type':'String'}]}]:
    bad=copy.deepcopy(v);bad['intent'].update(intent_patch)
    r=subprocess.run([a.binary,'normalize','legacy',put('bad-legacy.json',bad)],capture_output=True,text=True);assert r.returncode==2
# Current IR unknown tags cannot deserialize; direct check still structural legacy API.
unknown=copy.deepcopy(x);unknown['candidate']['obligations'][0]['claim']={'kind':'unknown'}
r=subprocess.run([a.binary,'check',put('bad-input.json',unknown)],capture_output=True,text=True);assert r.returncode==2
(scratch/'results.json').write_text(json.dumps({'status':'pass','cases':results,'replay':base,'legacy_fixture_hash':digest(legacy.read_bytes()),'binary_hash':digest(pathlib.Path(a.binary).read_bytes()),'oracle_hash':tool['digest'],'limits':'bounded test-owned evidence; no compiler/model/native proof'},indent=2))
print(json.dumps({'status':'pass','observations':len(results),'results':str(scratch/'results.json')}))

#!/usr/bin/env python3
"""Fixture-only NDJSON bridge exercise; every listener uses loopback port 0."""
import hashlib, http.server, json, os, pathlib, socket, subprocess, sys, tempfile, threading, time

def synthetic_hardening(root, project_id):
    """A fixture acceptance for testing downstream advice, never a provider judgment."""
    root=root/'qroc'
    project=json.loads((root/'projects'/f'{project_id}.json').read_text())
    idea=project.get('brain_dump','')
    record=dict(schema='qroc-hardening/1',project=project_id,route=None,rounds=[dict(
        round=1,at=0,idea_text=idea,idea_sha256=hashlib.sha256(idea.encode()).hexdigest(),
        score=dict(total=100,idea=100,config=0,missing=[]),verdict='pass',sent_back=False,
        questions=None,answers=None,judgment=dict(status='synthetic downstream test fixture',answers=[],receipt=None))])
    folder=root/'hardening';folder.mkdir(exist_ok=True)
    (folder/f'{project_id}.json').write_text(json.dumps(record))

def sha(path): return hashlib.sha256(path.read_bytes()).hexdigest()
class Shim(http.server.BaseHTTPRequestHandler):
    mode='good'
    def log_message(self,*args): pass
    def do_POST(self):
        assert self.path=='/v1/chat/completions'
        request=json.loads(self.rfile.read(int(self.headers['Content-Length'])))
        assert request['model']=='decide:gate4_consistency' and request['stream'] is False
        assert request['response_format']=={'type':'json_object'}
        context=json.loads(request['messages'][1]['content'])
        labels=[]
        for rule in context['rules']:
            evidence=[]
            for ref in [rule['left'],rule['right']]:
                d=context['sources'][ref['documentType']]
                evidence.append(dict(force_ref=ref,document_id=d['id'],revision=d['revision'],sha256=d['sha256'],quote=d['content']))
            labels.append(dict(id=rule['id'],label='supported',evidence=evidence))
        if self.mode=='bad': labels[0]['evidence'][0]['quote']='INVENTED QUOTATION'
        content=json.dumps(dict(labels=labels)) if self.mode!='malformed' else 'not JSON'
        envelope=dict(model='fixture-decide-actual',route='fixture:decide',receipt_id='fixture-receipt',choices=[dict(message=dict(content=content),finish_reason='stop')])
        body=json.dumps(envelope).encode()
        self.send_response(200); self.send_header('Content-Length',str(len(body)));self.send_header('Connection','close');self.end_headers();self.wfile.write(body)

def main():
    binary, roots=sys.argv[1:]; roots=pathlib.Path(roots);roots.mkdir(parents=True,exist_ok=True)
    root=pathlib.Path(tempfile.mkdtemp(prefix='ndjson-',dir=roots))
    server=http.server.HTTPServer(('127.0.0.1',0),Shim)
    assert server.server_port not in (8080,8096,8097)
    worker=threading.Thread(target=server.serve_forever);worker.start()
    env=dict(os.environ,QROC_GATEWAY=f'http://127.0.0.1:{server.server_port}')
    bridge=subprocess.Popen([binary,'--root',str(root)],stdin=subprocess.PIPE,stdout=subprocess.PIPE,text=True,env=env)
    count=0
    def ask(command,args,ok=True):
        nonlocal count
        count+=1;bridge.stdin.write(json.dumps(dict(version=1,id=str(count),command=command,args=args))+'\n');bridge.stdin.flush()
        response=json.loads(bridge.stdout.readline());assert response['ok']==ok,response
        return response['result'] if ok else response['error']
    def snapshot():
        return {str(p.relative_to(root)):sha(p) for p in sorted(root.rglob('*')) if p.is_file() and ('documents' in p.parts or 'scope' in p.parts or p.parent.name=='projects')}
    try:
        assert json.loads(bridge.stdout.readline())['event']=='ready'
        p=ask('qroc-project-create',dict(title='T305 fixture',brain_dump='Fixture only, no inference.'))['id']
        b=ask('qroc-branch-create',dict(project_id=p,name='Fixture branch',provider='unavailable',model='fixture'))['id']
        synthetic_hardening(root,p)
        owner=dict(project_id=p,branch_id=b)
        docs={}
        for kind in ['MARKET_ANALYSIS','COMPETITOR_ANALYSIS']:
            docs[kind]=ask('qroc-document-save',dict(owner,document_type=kind,content='Exact fixture source quotation.',source_revision=0))
        gate=ask('qroc-document-save',dict(owner,document_type='GATE_4',content='Manual fixture gate; not an acceptance.',source_revision=0))
        scope=root/'scope'/'T305-fixture.json';scope.parent.mkdir(parents=True,exist_ok=True);scope.write_text('{"fixture":"scope unchanged"}\n')
        args=dict(owner,revisions={k:d['revision'] for k,d in docs.items()})
        gateargs=dict(owner,document_type='GATE_4',revision=gate['revision'])
        # The gate evaluator stamps evaluated_at in seconds. Capture both calls within one second.
        while time.time()%1>.05: time.sleep(.002)
        before=snapshot()
        print('CHECK2_SHA256SUM_BEFORE',flush=True)
        subprocess.run(['sha256sum',*[str(root/p) for p in before]],check=True)
        raw_before=json.dumps(ask('qroc-gate-semantic-evaluate',gateargs),separators=(',',':'),sort_keys=True).encode()
        record=ask('qroc-decide-gate4-consistency',args)
        raw_after=json.dumps(ask('qroc-gate-semantic-evaluate',gateargs),separators=(',',':'),sort_keys=True).encode()
        (root/'gate-before.json').write_bytes(raw_before);(root/'gate-after.json').write_bytes(raw_after)
        subprocess.run(['cmp',str(root/'gate-before.json'),str(root/'gate-after.json')],check=True)
        assert before==snapshot()
        assert record['schema']=='qroc-advice/1' and record['advisory'] is True and not record['abstained'] and record['quote_verified'] and record['supporting_spans']
        assert record['model']=='fixture-decide-actual' and record['route']=='fixture:decide'
        assert record['contract_sha256']==sha(pathlib.Path('runtime/decide/contracts/qroc.gate4_consistency.v1.json'))
        first=root/'qroc/advice'/f"{record['id']}.json"; first_bytes=first.read_bytes();assert json.loads(first_bytes)==record
        print('CHECK1_RECORD',json.dumps(record,separators=(',',':')))
        print('CHECK1_LS');subprocess.run(['ls',str(root/'qroc/advice')],check=True)
        for relative in before:
            path=root/relative
            print('CHECK2_SHA256_BEFORE',before[relative],relative)
            print('CHECK2_SHA256_AFTER',sha(path),relative)
            subprocess.run(['sha256sum',str(path)],check=True)
        print('CHECK2_CMP_EXIT 0')
        Shim.mode='bad';bad=ask('qroc-decide-gate4-consistency',args)
        assert bad['quote_verified'] is False and bad['abstained'] is True
        assert bad['labels'][0]['quote_verified'] is False and bad['labels'][0]['abstained'] is True
        print('CHECK3_RECORD',json.dumps(bad,separators=(',',':')))
        assert first.read_bytes()==first_bytes
        listed=ask('qroc-decide-list',owner)['advice'];assert len(listed)==2
        ask('qroc-decide-gate4-consistency',dict(args,revisions={'MARKET_ANALYSIS':999,'COMPETITOR_ANALYSIS':1}),ok=False)
        ask('qroc-decide-gate4-consistency',dict(args,project_id='../escape'),ok=False)
        Shim.mode='malformed';malformed=ask('qroc-decide-gate4-consistency',args);assert malformed['abstained'] and malformed['error']=='invalid_json'
        print('EXTRA malformed response abstains; wrong revision/path refused; list and append preservation passed')
        server.shutdown();worker.join();server.server_close()
        advice_before={str(p):sha(p) for p in (root/'qroc/advice').glob('*.json')}
        unavailable=ask('qroc-decide-gate4-consistency',args);assert unavailable=={'error':'unavailable'}
        print(json.dumps(unavailable,separators=(',',':')))
        documents=ask('qroc-document-list',owner);assert len(documents['documents'])==3
        print('CHECK4_DOCUMENT_LIST',json.dumps({'count':len(documents['documents'])}))
        assert advice_before=={str(p):sha(p) for p in (root/'qroc/advice').glob('*.json')} and before==snapshot()
        print('CHECK4_UNCHANGED advice, documents, projects/gate reviews and scope')
        print('FIXTURE_ROOT',root)
    finally:
        bridge.stdin.close();bridge.wait(timeout=5)
        if worker.is_alive(): server.shutdown();worker.join();server.server_close()
    assert bridge.returncode==0
    print('T305_FIXTURE_PASS')
if __name__=='__main__': main()

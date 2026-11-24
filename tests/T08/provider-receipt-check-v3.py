#!/usr/bin/env python3
"""Only run AFTER actual helper drains/releases; absence is pending, never proof."""
import importlib.util,json,pathlib,sys,hashlib
module_path=pathlib.Path(__file__).with_name('provider-journey-v3.py');spec=importlib.util.spec_from_file_location('journey',module_path);m=importlib.util.module_from_spec(spec);spec.loader.exec_module(m)
clientpath,receiptpath,outpath=map(pathlib.Path,sys.argv[1:]);client=json.loads(clientpath.read_text());receipt=json.loads(receiptpath.read_text())
result=m.validate_helper_receipt(client,receipt);result['bindings']={str(p):hashlib.sha256(p.read_bytes()).hexdigest() for p in [clientpath,receiptpath,module_path]};pathlib.Path(outpath).write_text(json.dumps(result,indent=2)+'\n')

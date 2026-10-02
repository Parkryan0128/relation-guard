#!/usr/bin/env python3
"""Preserve per-error codes, pool state and host probe timing during a batch replay."""
import datetime,hashlib,json,os,pathlib,subprocess,time
from native_lab import Lab, ROOT
from load_dataset import seed

private=pathlib.Path(os.environ['RG_DIAGNOSTIC_DIR']);private.mkdir(exist_ok=True);private.chmod(0o700)
output=ROOT/'verification/batch-diagnosis';output.mkdir(exist_ok=True)
lab=Lab(private/'lab');runs=[]
report={'source_commit':subprocess.check_output(['git','rev-parse','HEAD'],cwd=ROOT,text=True).strip(),
        'source_dirty':bool(subprocess.check_output(['git','diff','--name-only','HEAD','--','crates'],cwd=ROOT)),
        'binary_sha256':hashlib.sha256((lab.bin/'authz-server').read_bytes()).hexdigest(),
        'purpose':'Diagnostic replay, not replacement of the failed original acceptance run',
        'runs':runs}
try:
 lab.initialize();report['dataset']=seed(lab,lab.tenant,10000,50000)
 config=private/'config.json';config.write_text(json.dumps({'url':f'http://127.0.0.1:{lab.port}','tenants':[{'id':lab.tenant,'token':lab.token}],'documents':10000}));config.chmod(0o600)
 for name,seconds in [('warmup',60)]+[(f'batch-{i}',300) for i in range(1,4)]:
  samples=[];log=output/(name+'.log');result=output/(name+'.json');previous=time.monotonic()
  with log.open('w') as f:
   process=subprocess.Popen([os.environ['K6_BIN'],'run','--quiet',str(ROOT/'benchmarks/load.js')],env={**os.environ,'CONFIG':str(config),'PROFILE':'batch','RATE':'10','DURATION':str(seconds)+'s','RESULT':str(result)},stdout=f,stderr=f)
   while process.poll() is None:
    now=time.monotonic();probe_started=time.monotonic()
    try:
     status,body=lab.probe('metrics');probe={'status':status,'metrics':body}
    except OSError as error:probe={'error_type':type(error).__name__}
    samples.append({'utc':datetime.datetime.now(datetime.timezone.utc).isoformat(),'interval_seconds':now-previous,'probe_seconds':time.monotonic()-probe_started,**probe});previous=now
    time.sleep(1)
  (output/(name+'-samples.json')).write_text(json.dumps(samples,indent=2))
  metrics=json.loads(result.read_text())['metrics']
  record={'name':name,'exit_code':process.returncode,'correct':metrics.get('correct_responses',{}).get('values',{}).get('count',0),'unexpected':metrics.get('unexpected_responses',{}).get('values',{}).get('count',0),'dropped':metrics.get('dropped_iterations',{}).get('values',{}).get('count',0),'latency':metrics.get('successful_latency',{}).get('values',{})}
  runs.append(record);(output/'report.json').write_text(json.dumps(report,indent=2));print(json.dumps(record),flush=True)
  assert metrics.get('correctness_mismatches',{}).get('values',{}).get('count',0)==0
finally:
 lab.close()
 report['resource_usage']=getattr(lab,'resource_usage',None)
 # Server diagnostics contain fixed error codes, request IDs and pool counts only.
 lines=(private/'lab/server.log').read_text().splitlines()
 errors=[]
 for line in lines:
  try:
   event=json.loads(line)
   if event.get('fields',{}).get('message')=='request failed':errors.append(event)
  except json.JSONDecodeError:pass
 (output/'server-errors.json').write_text(json.dumps(errors,indent=2))
 (output/'report.json').write_text(json.dumps(report,indent=2))

#!/usr/bin/env python3
"""Reproducible local S/M workloads. Does not claim Linux-container SLO equivalence."""
import hashlib,json,os,pathlib,platform,subprocess,time,uuid
from native_lab import Lab,ROOT
run=pathlib.Path(os.environ['RG_LOAD_DIR']);run.mkdir(exist_ok=True);run.chmod(0o700)
lab=Lab(run/'lab');k6=os.environ['K6_BIN'];out=pathlib.Path(os.environ.get('RG_LOAD_OUTPUT',str(ROOT/'verification/load')));out.mkdir(exist_ok=True)
manifest={'seed':424242,'environment':'native macOS loopback; no cgroup CPU/memory limits','os':platform.platform(),'architecture':platform.machine(),'source_dirty':bool(subprocess.check_output(['git','diff','--name-only','HEAD','--','Cargo.toml','Cargo.lock','crates'],cwd=ROOT)),'logical_cpus':os.cpu_count(),'postgres':subprocess.check_output([str(lab.pg/'postgres'),'--version'],text=True).strip(),'commit':subprocess.check_output(['git','rev-parse','HEAD'],cwd=ROOT,text=True).strip(),'binary_sha256':hashlib.sha256((lab.bin/'authz-server').read_bytes()).hexdigest(),'runs':[]}
from load_dataset import seed
def configure(tenants,documents):
 cfg=run/'load-config.json';cfg.write_text(json.dumps({'url':f'http://127.0.0.1:{lab.port}','tenants':tenants,'documents':documents}));cfg.chmod(0o600);return cfg
def measured(profile,duration,rate,label,cfg,gate=True):
 result=out/(label+'.json');log=run/(label+'.log');before=lab.probe('metrics')[1];(out/(label+'-metrics-before.txt')).write_text(before)
 with log.open('w') as f:
  p=subprocess.Popen([k6,'run','--quiet',str(ROOT/'benchmarks/load.js')],env={**os.environ,'CONFIG':str(cfg),'PROFILE':profile,'DURATION':str(duration)+'s','RATE':str(rate),'RESULT':str(result)},stdout=f,stderr=f)
  samples=[]
  while p.poll() is None:
   samples.append({'sample_index':len(samples),'application_metrics':lab.probe('metrics')[1]});time.sleep(5)
 (out/(label+'-resources.json')).write_text(json.dumps(samples));(out/(label+'-metrics-after.txt')).write_text(lab.probe('metrics')[1]);d=json.loads(result.read_text());m=d['metrics']
 good=m.get('correct_responses',{}).get('values',{}).get('count',0);bad=m.get('unexpected_responses',{}).get('values',{}).get('count',0);dropped=m.get('dropped_iterations',{}).get('values',{}).get('count',0);scheduled=m.get('iterations',{}).get('values',{}).get('count',0)+dropped
 latency=m.get('successful_latency',{}).get('values',{});record={'label':label,'scheduled':scheduled,'nominal_target':rate*duration,'correct':good,'unexpected':bad,'dropped':dropped,'p50_ms':latency.get('p(50)'),'p95_ms':latency.get('p(95)'),'p99_ms':latency.get('p(99)'),'exit_code':p.returncode,'http_requests':m.get('http_reqs',{}).get('values',{}).get('count',0),'revision_conflicts':m.get('revision_conflicts',{}).get('values',{}).get('count',0)}
 manifest['runs'].append(record);(out/'manifest.json').write_text(json.dumps(manifest,indent=2));print(json.dumps(record),flush=True)
 assert m.get('correctness_mismatches',{}).get('values',{}).get('count',0)==0,'Wrong authorization decision'
 if gate:
  record['gate_passed']=(bad+dropped)/scheduled<.001 and p.returncode==0 and latency.get('p(95)',float('inf'))<=(100 if profile=='batch' else 20) and (profile!='single' or latency.get('p(99)',float('inf'))<=50)
  manifest['native_baseline_passed']=all(r.get('gate_passed',True) for r in manifest['runs'])
  (out/'manifest.json').write_text(json.dumps(manifest,indent=2))

 return record
try:
 lab.initialize();(run/'context.json').write_text(json.dumps(lab.context(),indent=2));manifest['S']=seed(lab,lab.tenant,10000,50000);cfg=configure([{'id':lab.tenant,'token':lab.token}],10000)
 assert lab.sql('SELECT count(*) FROM relationships')=='50000'
 for profile,rate in [('single',100),('batch',10)]:
  for repeat in range(1,4):
   measured(profile,60,rate,f'S-{profile}-{repeat}-warmup',cfg,False)
   measured(profile,300,rate,f'S-{profile}-{repeat}',cfg)
 # Replace S with an independently seeded M dataset.
 lab.stop_server();manifest['S_server_resource_usage']=lab.resource_usage;lab.sql('TRUNCATE audit_events,mutation_receipts,relationships,entities,tenants CASCADE');lab.keys=[];tenants=[];shapes=[]
 for i in range(10):
  tenant=str(uuid.uuid5(uuid.NAMESPACE_DNS,f'relation-guard-424242-{i}'));shapes.append(seed(lab,tenant,20000,100000));lab.tenant=tenant;lab.admin('bootstrap-admin',tenant,'test-admin');token=lab.new_key('m'+str(i));tenants.append({'id':tenant,'token':token})
 manifest['M']=shapes;lab.start_server();cfg=configure(tenants,20000);assert lab.sql('SELECT count(*) FROM relationships')=='1000000'
 measured('single',60,100,'M-single',cfg,False)
 for profile in ['mixed-hot','mixed-distributed']:measured(profile,60,100,'M-'+profile,cfg,False)
 for rate in [100,300,600,1000]:measured('single',30,rate,f'M-stress-{rate}',cfg,False)
 measured('single',60,100,'M-recovery',cfg)
 (out/'manifest.json').write_text(json.dumps(manifest,indent=2));print('All native local workload profiles completed. Baseline pass: '+str(manifest.get('native_baseline_passed')),flush=True)
 if not manifest.get('native_baseline_passed'):raise AssertionError('One or more native baseline runs missed the fixed target; evidence retained.')
finally:
 lab.close()
 manifest['server_resource_usage']=getattr(lab,'resource_usage',None)
 (out/'manifest.json').write_text(json.dumps(manifest,indent=2))

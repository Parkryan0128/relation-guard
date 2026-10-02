#!/usr/bin/env python3
"""Isolated Compose release acceptance. Leaves the verified stack for load testing."""
import base64,hashlib,json,os,pathlib,secrets,socket,subprocess,sys,tempfile,time,urllib.request,urllib.error,uuid
ROOT=pathlib.Path(__file__).resolve().parents[1]
PG='postgres:18-alpine@sha256:77f585114c32fbca283dc835b0596f4e52b51b4c6662d7810b2f4084f60a1873'
PYTHON_IMAGE=os.environ.get('PYTHON_IMAGE','python:3.13-alpine')
run=pathlib.Path(os.environ.get('RG_RUN_DIR',tempfile.mkdtemp(prefix='rg-release-')));run.mkdir(exist_ok=True);run.chmod(0o700)
project='rg-'+uuid.uuid4().hex[:10]
def shell(args,input=None,check=True):
 r=subprocess.run(args,input=input,stdout=subprocess.PIPE,stderr=subprocess.PIPE,check=False)
 if check and r.returncode:raise RuntimeError(f'Command {args[:4]} failed: '+r.stderr.decode()[-3000:])
 return r
subprocess.run([sys.executable,str(ROOT/'scripts/bootstrap-local.py'),str(run)],check=True,stdout=subprocess.DEVNULL)
with socket.socket() as s:s.bind(('127.0.0.1',0));port=s.getsockname()[1]
p=run/'.env';p.write_text(p.read_text().replace('AUTHZ_PORT=8080',f'AUTHZ_PORT={port}'))
env=dict(line.split('=',1) for line in p.read_text().splitlines())
compose=['docker','compose','--project-directory',str(run),'--env-file',str(p),'-f',str(ROOT/'compose.yaml'),'-p',project]
def dc(*args,**kwargs):return shell(compose+list(args),**kwargs)
def cid(service):return dc('ps','-q',service).stdout.decode().strip()
def sql(statement,container=None):return shell(['docker','exec','-i',container or cid('db'),'psql','-X','-v','ON_ERROR_STOP=1','-U','relation_guard','-d','relation_guard','-At'],(statement+'\n').encode()).stdout.decode().strip()
cred=json.loads((run/'secrets/client.json').read_text());tenant=cred['tenant_id'];base=f'http://127.0.0.1:{port}/v1/tenants/{tenant}'
def http(route,body=None,token=None,target=None,key='deployment'):
 request=urllib.request.Request((target or base)+'/'+route,data=None if body is None else json.dumps(body).encode(),headers={'Authorization':'Bearer '+(token or cred['token']),'Content-Type':'application/json','Idempotency-Key':key})
 try:r=urllib.request.urlopen(request,timeout=4)
 except urllib.error.HTTPError as e:r=e
 return r.status,json.load(r)
def ready():
 for _ in range(120):
  try:
   if http('revision')[0]==200:return
  except OSError:pass
  time.sleep(.25)
 raise AssertionError('server not ready')
def internal(path):
 code='import urllib.request,urllib.error,json\ntry:r=urllib.request.urlopen("http://server:9090/'+path+'",timeout=2)\nexcept urllib.error.HTTPError as e:r=e\nprint(json.dumps([r.status,json.load(r)]))'
 return json.loads(shell(['docker','exec',project+'-client','python','-c',code]).stdout)
def snapshot(container=None):
 return sql("SELECT json_build_object('revision',(SELECT revision FROM tenants WHERE id='"+tenant+"'),'entities',(SELECT count(*) FROM entities),'relationships',(SELECT count(*) FROM relationships),'audit',(SELECT count(*) FROM audit_events),'receipts',(SELECT count(*) FROM mutation_receipts),'edges_hash',(SELECT md5(string_agg(row_to_json(r)::text,'' ORDER BY resource_kind,resource_id,relation,subject_kind,subject_id,subject_relation)) FROM relationships r))",container)
def rotate_entry(key_id):
 raw=secrets.token_bytes(32);token=key_id+'.'+base64.urlsafe_b64encode(raw).decode().rstrip('=')
 return token,{'key_id':key_id,'tenant_id':tenant,'scopes':['check','explain','relations:read','relations:write','audit:read'],'secret_sha256':hashlib.sha256(raw).hexdigest()}
def start_server():dc('up','-d','--force-recreate','server');ready()
try:
 dc('run','--rm','migrate');dc('run','--rm','--entrypoint','authz-admin','migrate','tenant',tenant);dc('up','-d','server');ready()
 for service in ('server','db'):
  inspect=json.loads(shell(['docker','inspect',cid(service)]).stdout)[0]
  assert all(not bindings for name,bindings in inspect['NetworkSettings']['Ports'].items() if service=='db' or name!='8080/tcp')
 inspect=json.loads(shell(['docker','inspect',cid('server')]).stdout)[0];assert inspect['Config']['User']=='10001:10001' and inspect['HostConfig']['ReadonlyRootfs']
 assert 'relation_guard_runtime' in next(v for v in inspect['Config']['Env'] if v.startswith('DATABASE_URL='))
 assert sql("SELECT has_table_privilege('relation_guard_runtime','tenants','INSERT')")=='f'
 assert sql("SELECT has_table_privilege('relation_guard_runtime','audit_events','DELETE')")=='f'
 shell(['docker','run','-d','--name',project+'-client','--network',project+'_default','-v',str(ROOT/'examples')+':/app:ro','-v',str(run/'secrets/client.json')+':/client.json:ro','-e','AUTHZ_URL=http://server:8080','-e','AUTHZ_CLIENT_FILE=/client.json',PYTHON_IMAGE,'sleep','infinity'])
 assert internal('healthz')==[200,{'status':'ok'}];assert internal('readyz')==[200,{'status':'ready'}]
 client=shell(['docker','exec',project+'-client','python','/app/client.py']);assert b'Revoked: deny' in client.stdout
 # Stable grant retained through restart/restore.
 user={'kind':'user','id':'recovery-user'};doc={'kind':'document','id':'recovery-doc'};edge={'resource':doc,'relation':'viewer','subject':user,'subject_relation':''}
 request={'expected_revision':http('revision')[1]['revision'],'operations':[{'op':'put_entity','entity':user},{'op':'put_entity','entity':doc},{'op':'put_relation','relationship':edge}]}
 assert http('write',request)[0]==200
 check={'subject':user,'resource':doc,'action':'view'};assert http('check',check)[1]['decision']=='allow'
 before=snapshot();dc('restart','db','server');ready();assert snapshot()==before;assert http('check',check)[1]['decision']=='allow';assert http('write',request)[1]['revision']==json.loads(before)['revision'].__str__()
 backup=shell(['docker','exec',cid('db'),'pg_dump','-U','relation_guard','-d','relation_guard','-Fc']).stdout;(run/'backup.dump').write_bytes(backup)
 restored=project+'-restore-db';shell(['docker','run','-d','--name',restored,'--network',project+'_default','-e','POSTGRES_HOST_AUTH_METHOD=trust','-e','POSTGRES_DB=relation_guard','-e','POSTGRES_USER=relation_guard',PG])
 for _ in range(100):
  if shell(['docker','exec',restored,'pg_isready','-U','relation_guard'],check=False).returncode==0:break
  time.sleep(.1)
 shell(['docker','exec','-i',restored,'pg_restore','--exit-on-error','--no-privileges','-U','relation_guard','-d','relation_guard'],backup)
 assert snapshot(restored)==before
 new_token,new_entry=rotate_entry('restored');restore_key=run/'restore-key.json';restore_key.write_text(json.dumps({'version':1,'keys':[new_entry]}));restore_key.chmod(0o444)
 with socket.socket() as s:s.bind(('127.0.0.1',0));restore_port=s.getsockname()[1]
 # Reapply runtime grants using the admin container against the restored DB.
 re=run/'restore.env';re.write_text('DATABASE_URL=postgres://relation_guard@'+restored+':5432/relation_guard\nAUTHZ_RUNTIME_PASSWORD='+env['AUTHZ_RUNTIME_PASSWORD']+'\n');re.chmod(0o600)
 shell(['docker','run','--rm','--network',project+'_default','--env-file',str(re),'--entrypoint','authz-admin','relation-guard:local','runtime-role'])
 re.write_text('DATABASE_URL=postgres://relation_guard_runtime@'+restored+':5432/relation_guard\nAUTHZ_KEYS_FILE=/key.json\n')
 restored_server=project+'-restore-server';shell(['docker','run','-d','--name',restored_server,'--network',project+'_default','--env-file',str(re),'-v',str(restore_key)+':/key.json:ro','-p',f'127.0.0.1:{restore_port}:8080','relation-guard:local'])
 target=f'http://127.0.0.1:{restore_port}/v1/tenants/{tenant}'
 for _ in range(100):
  try:
   if http('check',check,new_token,target)[0]==200:break
  except OSError:pass
  time.sleep(.1)
 assert http('check',check,new_token,target)[1]['decision']=='allow';assert http('check',check,cred['token'],target)[0]==401
 # Rotation: overlap both keys, then remove old credential and recreate the instance.
 token,entry=rotate_entry('rotated');keypath=run/'secrets/keys.json';configuration=json.loads(keypath.read_text());configuration['keys'].append(entry);keypath.chmod(0o644);keypath.write_text(json.dumps(configuration));keypath.chmod(0o444);start_server();assert http('revision',token=token)[0]==200
 configuration['keys']=[entry];keypath.chmod(0o644);keypath.write_text(json.dumps(configuration));keypath.chmod(0o444);dc('up','-d','--force-recreate','server')
 old_token=cred['token'];cred['token']=token;ready();assert http('revision',token=old_token)[0]==401
 (run/'secrets/client.json').write_text(json.dumps(cred))
 # Schema change and outage affect readiness, not liveness.
 sql('UPDATE _sqlx_migrations SET version=2 WHERE version=1');assert internal('readyz')[0]==503;assert internal('healthz')[0]==200
 sql('UPDATE _sqlx_migrations SET version=1 WHERE version=2');assert internal('readyz')[0]==200
 dc('stop','db');assert internal('healthz')[0]==200;assert internal('readyz')[0]==503;assert http('check',check)[0]==503
 dc('start','db');ready();assert internal('readyz')[0]==200;assert http('check',check)[1]['decision']=='allow'
 started=time.monotonic();dc('stop','server');assert time.monotonic()-started<12;dc('up','-d','server');ready()
 context={'project':project,'run_dir':str(run),'port':port,'tenant':tenant,'db':cid('db'),'server':cid('server'),'client':project+'-client','restore_db':restored,'restore_server':restored_server,'image':json.loads(shell(['docker','image','inspect','relation-guard:local']).stdout)[0]['Id']}
 (run/'context.json').write_text(json.dumps(context,indent=2));(ROOT/'verification/deployment-result.json').write_text(json.dumps({'passed':['M4-01','M4-02-happy-path','M4-03','M4-04','M4-05','M4-06'],'image':context['image'],'snapshot':json.loads(before),'runtime_uid':'10001:10001','database_and_operations_ports_published':False},indent=2))
 print('PASS: isolated non-root Compose, separate runtime DB role, Python container client, restart persistence, backup/restore, key rotation, DB/schema readiness and shutdown.')
 print('Context: '+str(run/'context.json'))
except BaseException:
 print('Failed deployment project retained for inspection: '+project+' in '+str(run),file=sys.stderr);raise

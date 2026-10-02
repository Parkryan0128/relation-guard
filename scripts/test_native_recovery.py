#!/usr/bin/env python3
"""Native replacement for deployment-dependent acceptance, per user instruction."""
import http.client,json,os,pathlib,socket,subprocess,sys,time
from native_lab import Lab,ROOT
lab=Lab(os.environ.get('RG_NATIVE_DIR'));restore=None
try:
 lab.initialize();assert os.getuid()!=0
 assert lab.sql("SELECT has_table_privilege('relation_guard_runtime','tenants','INSERT')")=='f'
 assert lab.sql("SELECT has_table_privilege('relation_guard_runtime','audit_events','DELETE')")=='f'
 client=lab.directory/'client.json';client.write_text(json.dumps({'tenant_id':lab.tenant,'token':lab.token}));client.chmod(0o600)
 subprocess.run([sys.executable,str(ROOT/'examples/client.py')],env={**os.environ,'AUTHZ_URL':f'http://127.0.0.1:{lab.port}','AUTHZ_CLIENT_FILE':str(client)},check=True)
 user={'kind':'user','id':'recover-user'};resource={'kind':'document','id':'recover-doc'};edge={'resource':resource,'relation':'viewer','subject':user,'subject_relation':''}
 write={'expected_revision':lab.request('revision')[1]['revision'],'operations':[{'op':'put_entity','entity':user},{'op':'put_entity','entity':resource},{'op':'put_relation','relationship':edge}]}
 before=lab.request('write',write,key='persist')[1];check={'subject':user,'resource':resource,'action':'view'};assert lab.request('check',check)[1]['decision']=='allow'
 def snapshot(l):return l.sql("SELECT json_build_object('revision',(SELECT revision FROM tenants WHERE id='"+lab.tenant+"'),'entities',(SELECT count(*) FROM entities),'relationships',(SELECT count(*) FROM relationships),'audit',(SELECT count(*) FROM audit_events),'receipts',(SELECT count(*) FROM mutation_receipts),'edges_hash',(SELECT md5(string_agg(row_to_json(r)::text,'' ORDER BY resource_kind,resource_id,relation,subject_kind,subject_id,subject_relation)) FROM relationships r))")
 state=snapshot(lab);lab.stop_server();lab.stop_db();lab.start_db();lab.start_server();assert snapshot(lab)==state;assert lab.request('write',write,key='persist')[1]==before
 backup=lab.pgcmd('pg_dump',lab.admin_url,'-Fc');(lab.directory/'backup.dump').write_bytes(backup)
 restore=Lab();restore.initialize();restore.stop_server();restore.sql('DROP SCHEMA public CASCADE; CREATE SCHEMA public;')
 restore.pgcmd('pg_restore','--exit-on-error','--no-privileges','-d',restore.admin_url,input=backup);restore.admin('runtime-role');restore.tenant=lab.tenant;restore.keys=[];restore.token=restore.new_key('restored');restore.start_server()
 assert snapshot(restore)==state;assert restore.request('check',check)[1]['decision']=='allow';assert restore.request('check',check,token=lab.token)[0]==401
 old=lab.token;new=lab.new_key('rotated');lab.stop_server();lab.start_server();assert lab.request('revision',token=old)[0]==200 and lab.request('revision',token=new)[0]==200
 lab.keys=lab.keys[-1:];lab.save_keys();lab.token=new;lab.stop_server();lab.start_server();assert lab.request('revision',token=old)[0]==401
 lab.sql('UPDATE _sqlx_migrations SET version=2 WHERE version=1');assert lab.probe('readyz')[0]==503 and lab.probe('healthz')[0]==200
 lab.sql('UPDATE _sqlx_migrations SET version=1 WHERE version=2');assert lab.probe('readyz')[0]==200
 lab.stop_db();assert lab.probe('healthz')[0]==200 and lab.probe('readyz')[0]==503;status,body,_=lab.request('check',check);assert status==503 and 'decision' not in body,(status,body)
 lab.start_db();lab.wait_ready();assert lab.request('check',check)[1]['decision']=='allow'
 # Hold an actual request body open; use observed inflight state as the barrier.
 held=socket.create_connection(('127.0.0.1',lab.port),timeout=3)
 held.sendall((f'POST /v1/tenants/{lab.tenant}/check HTTP/1.1\r\nHost: localhost\r\nAuthorization: Bearer {lab.token}\r\nContent-Type: application/json\r\nContent-Length: 1\r\n\r\n').encode())
 for _ in range(100):
  if 'relation_guard_inflight 1\n' in lab.probe('metrics')[1]:break
  time.sleep(.01)
 else:raise AssertionError('Request did not reach the inflight barrier')
 started=time.monotonic();lab.stop_server();elapsed=time.monotonic()-started;assert elapsed<10
 response=http.client.HTTPResponse(held);response.begin();assert response.status==504
 assert json.loads(response.read())['error']['code']=='DEADLINE_EXCEEDED';response.close();held.close()
 try:
  unexpected=socket.create_connection(('127.0.0.1',lab.port),timeout=.2)
 except OSError:pass
 else:unexpected.close();raise AssertionError('Data listener still accepts connections after shutdown')
 lab.start_server()
 result={'environment':sys.platform+' native application processes; isolated test PostgreSQL databases','passed':['runtime_role','python_client','server_restart_database_reconnect','backup_restore_new_credentials','rotation','database_schema_readiness','SIGTERM_inflight_drain'],'snapshot':json.loads(state),'shutdown_seconds':elapsed,'inflight_shutdown_response':504}
 (ROOT/'verification/native-recovery.json').write_text(json.dumps(result,indent=2));print('PASS: native restart, backup/restore, key rotation, readiness/outage recovery, privilege separation and SIGTERM.')
finally:
 if restore:restore.close()
 lab.close()

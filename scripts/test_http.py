#!/usr/bin/env python3
"""Real TCP smoke/contract tests. Uses only an explicitly isolated test database."""
import copy, json, os, pathlib, socket, subprocess, sys, tempfile, time, urllib.request, urllib.error, uuid
from jsonschema import Draft202012Validator
ROOT=pathlib.Path(__file__).resolve().parents[1]
URL=os.environ['TEST_DATABASE_URL']
BIN=ROOT/'target/debug'
ENV={**os.environ,'DATABASE_URL':URL}
def admin(*args):
    return subprocess.check_output([str(BIN/'authz-admin'),*args],env=ENV,text=True)
def port():
    with socket.socket() as s:
        s.bind(('127.0.0.1',0));return s.getsockname()[1]
spec=json.loads((ROOT/'spec/openapi.json').read_text())
def schema(body,name):
    Draft202012Validator({'$ref':f'#/components/schemas/{name}','components':spec['components']}).validate(body)
admin('migrate')
tenant=str(uuid.uuid4());other=str(uuid.uuid4())
admin('tenant',tenant);admin('tenant',other)
keys=[];tokens={}
for name,scopes in [('admin',['check','explain','relations:read','relations:write','audit:read']),('reader',['check']),('other',['check']),('writer',['relations:write'])]:
    pair=json.loads(admin('key',other if name=='other' else tenant,name))
    pair['config_entry']['scopes']=scopes;keys.append(pair['config_entry']);tokens[name]=pair['token']
all_scopes=['check','explain','relations:read','relations:write','audit:read']
for mask in range(1,32):
    name='scope'+str(mask);pair=json.loads(admin('key',tenant,name))
    pair['config_entry']['scopes']=[s for i,s in enumerate(all_scopes) if mask & (1<<i)]
    keys.append(pair['config_entry']);tokens[name]=pair['token']
fixture=json.loads((ROOT/'spec/examples/contracts.json').read_text())
count=0
with tempfile.TemporaryDirectory(prefix='relation-guard-http-') as td:
    path=pathlib.Path(td)/'keys.json';path.write_text(json.dumps({'version':1,'keys':keys}));path.chmod(0o600)
    data,ops=port(),port();base=f'http://127.0.0.1:{data}/v1/tenants/{tenant}'
    with open(pathlib.Path(td)/'server.log','w+') as log:
        process=subprocess.Popen([str(BIN/'authz-server')],env={**ENV,'AUTHZ_KEYS_FILE':str(path),'AUTHZ_BIND':f'127.0.0.1:{data}','AUTHZ_OPS_BIND':f'127.0.0.1:{ops}'},stdout=log,stderr=log)
        def call(route,body=None,*,key='admin',expected=200,code=None,raw=None,headers=None,method=None,name=None):
            global count
            h={'Content-Type':'application/json'}
            if key is not None:h['Authorization']='Bearer '+tokens[key]
            h.update(headers or {})
            encoded=raw if raw is not None else json.dumps(body).encode() if body is not None else None
            req=urllib.request.Request(base+'/'+route,data=encoded,headers=h,method=method or ('POST' if encoded is not None else 'GET'))
            try:r=urllib.request.urlopen(req,timeout=4)
            except urllib.error.HTTPError as e:r=e
            result=json.loads(r.read());assert r.status==expected,(route,r.status,result)
            assert r.headers.get('X-Request-Id')
            if code:assert result['error']['code']==code,(route,result)
            schema(result,name or ('Error' if expected!=200 else {'write':'WriteResponse','check':'CheckResponse','batch-check':'BatchResponse','revision':'RevisionResponse','relationships':'RelationshipPage','audit':'AuditPage'}[route.split('?')[0]]))
            count+=1;return result,r.headers
        try:
            for attempt in range(100):
                try:
                    with urllib.request.urlopen(f'http://127.0.0.1:{ops}/readyz',timeout=.2) as r:assert r.status==200
                    break
                except (OSError,urllib.error.URLError):
                    if process.poll() is not None:raise AssertionError('Server exited during startup')
                    time.sleep(.05)
            else:raise AssertionError('Readiness timeout')
            check=fixture['check']['request'];write=fixture['write']['request']
            for mask in range(1,32):
                name='scope'+str(mask);scopes={s for i,s in enumerate(all_scopes) if mask & (1<<i)}
                for route,body,required in [('check',{**check,'explain':False},{'check'}),('check',check,{'check','explain'}),('batch-check',{**fixture['batch_check']['request'],'explain':True},{'check','explain'}),('write',{'expected_revision':'0','operations':[{'op':'delete_entity','entity':{'kind':'user','id':'absent'}}]},{'relations:write'}),('revision',None,{'relations:read'}),('relationships?resource_kind=folder&resource_id=f10',None,{'relations:read'}),('audit',None,{'audit:read'})]:
                    allowed=required<=scopes
                    call(route,body,key=name,headers={'Idempotency-Key':'scope'},expected=200 if allowed else 403,code=None if allowed else 'FORBIDDEN')
            for n,token in enumerate(['unknown.'+'A'*43,'bad.key.'+'A'*43,'admin.'+'A'*43,'admin.short','admin.'+'='*43]):
                tokens['invalid'+str(n)]=token
                call('revision',key='invalid'+str(n),expected=401,code='UNAUTHENTICATED')
            valid=json.dumps(check).encode();call('check',raw=valid+b' '*(262144-len(valid)))
            call('check',raw=b'{"nested":'+b'['*16+b'0'+b']'*16+b'}',expected=400,code='INVALID_REQUEST')
            call('revision',key=None,expected=401,code='UNAUTHENTICATED')
            call('check',check,key='other',expected=403,code='FORBIDDEN')
            call('write',write,key='reader',expected=403,code='FORBIDDEN')
            call('check',check,key='reader',expected=403,code='FORBIDDEN') # explain requested
            for route in ['revision','relationships?resource_kind=folder&resource_id=f10','audit']:
                call(route,key='reader',expected=403,code='FORBIDDEN')
            call('check',check,key='writer',expected=403,code='FORBIDDEN')
            call('check',raw=b'{"subject":{},"subject":{}}',expected=400,code='INVALID_REQUEST')
            bad=copy.deepcopy(check);bad['unknown']=True
            call('check',bad,expected=400,code='INVALID_REQUEST')
            call('check',raw=b'\xff',expected=400,code='INVALID_REQUEST')
            call('check',raw=b' '*262145,key=None,expected=413,code='PAYLOAD_TOO_LARGE')
            call('check',check,headers={'Content-Type':'text/plain'},expected=415,code='UNSUPPORTED_MEDIA_TYPE')
            call('check',check,headers={'Content-Encoding':'gzip'},expected=415,code='UNSUPPORTED_MEDIA_TYPE')
            call('check',expected=405,code='METHOD_NOT_ALLOWED')
            call('missing',expected=404,code='NOT_FOUND')
            call('revision?unknown=1',expected=400,code='INVALID_REQUEST')
            call('audit?limit=1&limit=2',expected=400,code='INVALID_REQUEST')
            initial,_=call('write',write,headers={'Idempotency-Key':'grant'})
            result,_=call('check',check);assert result['decision']=='allow' and result['proof']
            batch=fixture['batch_check']['request'];call('batch-check',batch)
            call('relationships?resource_kind=folder&resource_id=f10&limit=1')
            call('relationships?resource_kind=folder&resource_id=f10&after=broken',expected=400,code='INVALID_REQUEST')
            call('write',write,headers={'Idempotency-Key':'stale'},expected=409,code='REVISION_CONFLICT')
            call('write',fixture['revoke']['request'],headers={'Idempotency-Key':'revoke'})
            result,_=call('check',check);assert result['decision']=='deny' and result['proof']==[]
            result,headers=call('write',write,headers={'Idempotency-Key':'grant'})
            assert result==initial and headers['Idempotency-Replayed']=='true' and headers['X-Request-Id']!=result['request_id']
            result,_=call('check',check);assert result['decision']=='deny'
            page,_=call('audit?limit=1');assert page['next_after_revision']=='1'
            page,_=call('audit?after_revision=1&limit=1');assert len(page['events'])==1 and page['next_after_revision'] is None
            noexplain={**check,'explain':False};result,_=call('check',noexplain,key='reader');assert 'proof' not in result
            with urllib.request.urlopen(f'http://127.0.0.1:{ops}/metrics') as r:assert b'relation_guard_requests_total' in r.read()
            call('healthz',expected=404,code='NOT_FOUND')
            grants=[{'op':'put_relation','relationship':{'resource':{'kind':'document','id':'d100'},'relation':'viewer','subject':subject,'subject_relation':relation}} for subject,relation in [({'kind':'user','id':'u42'},''),({'kind':'team','id':'dev'},'member')]]
            call('write',{'expected_revision':'2','operations':grants},headers={'Idempotency-Key':'page-seed'})
            page,_=call('relationships?resource_kind=document&resource_id=d100&limit=1');cursor=page['next_cursor'];assert cursor
            call('relationships?resource_kind=folder&resource_id=f10&after='+cursor,expected=400,code='INVALID_REQUEST')
            seen=page['relationships']
            while cursor:
                page,_=call('relationships?resource_kind=document&resource_id=d100&limit=1&after='+cursor);seen+=page['relationships'];cursor=page['next_cursor']
            assert len(seen)==3
            with urllib.request.urlopen(f'http://127.0.0.1:{ops}/healthz') as response:assert json.load(response)=={'status':'ok'}
            client_file=pathlib.Path(td)/'client.json'
            client_file.write_text(json.dumps({'tenant_id':tenant,'token':tokens['admin']}));client_file.chmod(0o600)
            subprocess.run([sys.executable,str(ROOT/'examples/client.py')],env={**ENV,'AUTHZ_URL':f'http://127.0.0.1:{data}','AUTHZ_CLIENT_FILE':str(client_file)},check=True)
            print(f'PASS: {count} real TCP HTTP assertions with OpenAPI response validation; grant/revoke/replay, scope isolation, strict parsing and pagination.')
        finally:
            process.terminate()
            try:process.wait(timeout=12)
            except subprocess.TimeoutExpired:process.kill();process.wait();raise
            if process.returncode!=0:
                log.seek(0);print(log.read());raise AssertionError(f'Server exit {process.returncode}')

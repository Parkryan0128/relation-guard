"""Disposable native PostgreSQL/server processes, bound only to loopback."""
import base64,hashlib,json,os,pathlib,secrets,socket,subprocess,tempfile,time,urllib.request,urllib.error,uuid
ROOT=pathlib.Path(__file__).resolve().parents[1]
def free_port():
 with socket.socket() as s:s.bind(('127.0.0.1',0));return s.getsockname()[1]
class Lab:
 def __init__(self,directory=None):
  self.directory=pathlib.Path(directory or tempfile.mkdtemp(prefix='rg-native-'));self.directory.mkdir(exist_ok=True);self.directory.chmod(0o700)
  self.pg=pathlib.Path(os.environ['PG_BIN']);self.bin=pathlib.Path(os.environ.get('AUTHZ_BIN',str(ROOT/'target/debug')))
  self.pgport=free_port();self.port=free_port();self.opsport=free_port();self.socket=pathlib.Path(tempfile.mkdtemp(prefix='rg-sock-'));self.data=self.directory/'pgdata';self.server=None
  self.admin_url=f'postgres://relation_guard@127.0.0.1:{self.pgport}/relation_guard';self.url=f'postgres://relation_guard_runtime@127.0.0.1:{self.pgport}/relation_guard'
  self.tenant=str(uuid.uuid4());self.keypath=self.directory/'keys.json';self.keys=[];self.runtime_password=secrets.token_hex(32)
 def pgcmd(self,*args,input=None):
  r=subprocess.run([str(self.pg/args[0]),*args[1:]],input=input,stdout=subprocess.PIPE,stderr=subprocess.PIPE)
  if r.returncode:raise RuntimeError(args[0]+': '+r.stderr.decode()[-2000:])
  return r.stdout
 def initialize(self):
  self.pgcmd('initdb','-D',str(self.data),'-U','relation_guard','-A','trust','--no-locale','-E','UTF8')
  with (self.data/'postgresql.conf').open('a') as f:f.write("\nshared_preload_libraries='pg_stat_statements'\nshared_buffers='256MB'\nmax_connections=100\n")
  self.start_db();self.pgcmd('createdb','-h','127.0.0.1','-p',str(self.pgport),'-U','relation_guard','relation_guard');self.admin('migrate');self.admin('tenant',self.tenant);self.admin('runtime-role')
  self.token=self.new_key('initial');self.start_server();return self
 def admin(self,*args):
  r=subprocess.run([str(self.bin/'authz-admin'),*args],env={**os.environ,'DATABASE_URL':self.admin_url,'AUTHZ_RUNTIME_PASSWORD':self.runtime_password},stdout=subprocess.PIPE,stderr=subprocess.PIPE)
  if r.returncode:raise RuntimeError('admin '+args[0]+': '+r.stderr.decode()[-2000:])
  return r.stdout
 def sql(self,statement):return self.pgcmd('psql',self.admin_url,'-X','-v','ON_ERROR_STOP=1','-At',input=statement.encode()).decode().strip()
 def new_key(self,name):
  raw=secrets.token_bytes(32);token=name+'.'+base64.urlsafe_b64encode(raw).decode().rstrip('=')
  self.keys.append({'key_id':name,'tenant_id':self.tenant,'scopes':['check','explain','relations:read','relations:write','audit:read'],'secret_sha256':hashlib.sha256(raw).hexdigest()});self.save_keys();return token
 def save_keys(self):self.keypath.write_text(json.dumps({'version':1,'keys':self.keys}));self.keypath.chmod(0o600)
 def start_db(self):self.pgcmd('pg_ctl','-D',str(self.data),'-l',str(self.directory/'postgres.log'),'-o',f'-h 127.0.0.1 -p {self.pgport} -k {self.socket}','-w','start')
 def stop_db(self):self.pgcmd('pg_ctl','-D',str(self.data),'-m','fast','-w','stop')
 def start_server(self):
  self.log=(self.directory/'server.log').open('a')
  self.server=subprocess.Popen([str(self.bin/'authz-server')],env={**os.environ,'DATABASE_URL':self.url,'AUTHZ_KEYS_FILE':str(self.keypath),'AUTHZ_BIND':f'127.0.0.1:{self.port}','AUTHZ_OPS_BIND':f'127.0.0.1:{self.opsport}','RUST_LOG':'info'},stdout=self.log,stderr=self.log)
  self.wait_ready()
 def stop_server(self):
  if self.server:
   self.server.terminate();deadline=time.monotonic()+12
   while True:
    pid,status,usage=os.wait4(self.server.pid,os.WNOHANG)
    if pid:
     self.server.returncode=os.waitstatus_to_exitcode(status);self.resource_usage={'user_cpu_seconds':usage.ru_utime,'system_cpu_seconds':usage.ru_stime,'peak_rss_bytes':usage.ru_maxrss};assert self.server.returncode==0;break
    if time.monotonic()>deadline:self.server.kill();self.server.wait();raise AssertionError('Server drain timeout')
    time.sleep(.01)
   self.log.close();self.server=None
 def wait_ready(self):
  for _ in range(200):
   try:
    if self.probe('readyz')[0]==200:return
   except OSError:pass
   time.sleep(.05)
  raise AssertionError('Readiness timeout')
 def probe(self,path):
  try:r=urllib.request.urlopen(f'http://127.0.0.1:{self.opsport}/{path}',timeout=2)
  except urllib.error.HTTPError as e:r=e
  return r.status,r.read().decode()
 def request(self,path,body=None,token=None,key=None):
  headers={'Authorization':'Bearer '+(token or self.token),'Content-Type':'application/json','Idempotency-Key':key or str(uuid.uuid4())}
  req=urllib.request.Request(f'http://127.0.0.1:{self.port}/v1/tenants/{self.tenant}/{path}',data=None if body is None else json.dumps(body).encode(),headers=headers)
  try:r=urllib.request.urlopen(req,timeout=4)
  except urllib.error.HTTPError as e:r=e
  return r.status,json.load(r),dict(r.headers)
 def close(self):
  self.stop_server()
  if (self.data/'postmaster.pid').exists():self.stop_db()
 def context(self):return {'directory':str(self.directory),'pg_bin':str(self.pg),'binary_dir':str(self.bin),'pgport':self.pgport,'port':self.port,'opsport':self.opsport,'tenant':self.tenant,'admin_url':self.admin_url,'runtime_url':self.url,'pid':self.server.pid if self.server else None}

class ExistingDatabaseLab(Lab):
 """Local application processes against an existing, explicitly isolated test PG."""
 def __init__(self,directory=None):
  super().__init__(directory)
  from urllib.parse import urlsplit,urlunsplit
  base=os.environ['EXISTING_TEST_DATABASE_URL'];parsed=urlsplit(base);self.control_url=base;self.database='rg_'+uuid.uuid4().hex;self.created=False
  self.pgport=parsed.port or 5432;self.admin_url=urlunsplit(parsed._replace(path='/'+self.database));self.url=urlunsplit(parsed._replace(netloc='relation_guard_runtime@'+parsed.netloc.rsplit('@',1)[-1],path='/'+self.database))
 def control(self,sql):return self.pgcmd('psql',self.control_url,'-X','-v','ON_ERROR_STOP=1','-At',input=sql.encode()).decode().strip()
 def initialize(self):
  self.control('CREATE DATABASE '+self.database);self.created=True;self.admin('migrate');self.admin('tenant',self.tenant);self.admin('runtime-role');self.token=self.new_key('initial');self.start_server();return self
 def start_db(self):self.control('ALTER DATABASE '+self.database+' ALLOW_CONNECTIONS true')
 def stop_db(self):
  self.control('ALTER DATABASE '+self.database+' ALLOW_CONNECTIONS false');self.control("SELECT pg_terminate_backend(pid) FROM pg_stat_activity WHERE datname='"+self.database+"'")
 def close(self):
  self.stop_server()
  if self.created:self.control('DROP DATABASE '+self.database+' WITH (FORCE)');self.created=False
if os.environ.get('EXISTING_TEST_DATABASE_URL'):Lab=ExistingDatabaseLab

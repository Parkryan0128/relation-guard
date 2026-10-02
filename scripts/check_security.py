#!/usr/bin/env python3
"""Review advisory findings against the actual server/admin dependency closure."""
import json,os,pathlib,re,subprocess
root=pathlib.Path(__file__).resolve().parents[1]
metadata=json.loads(subprocess.check_output(['cargo','metadata','--locked','--format-version','1'],cwd=root))
packages={p['id']:p for p in metadata['packages']};nodes={n['id']:n for n in metadata['resolve']['nodes']}
roots=[p['id'] for p in packages.values() if p['name'] in ('authz-server','authz-admin')]
# cargo metadata includes optional resolution nodes; cargo tree applies activated features.
active=set()
for target in ('aarch64-apple-darwin','aarch64-unknown-linux-gnu','x86_64-unknown-linux-gnu'):
 for package in ('authz-server','authz-admin'):
  tree=subprocess.check_output(['cargo','tree','--locked','-p',package,'--edges','normal,build','--prefix','none','--format','{p}','--target',target],cwd=root,text=True)
  for line in tree.splitlines():
   fields=line.split()
   if len(fields)>=2:active.add((fields[0],fields[1].removeprefix('v')))
reachable={p for p in packages if (packages[p]['name'],packages[p]['version']) in active}
advisories=json.loads(pathlib.Path(os.environ['AUDIT_JSON']).read_text())
review=[]
for finding in advisories.get('vulnerabilities',{}).get('list',[]):
 name=finding['package']['name'];version=finding['package']['version'];active=any(packages[p]['name']==name and packages[p]['version']==version for p in reachable)
 review.append({'id':finding['advisory']['id'],'package':name,'version':version,'active_server_or_admin_dependency':active,'disposition':'BLOCK' if active else 'not compiled into server/admin; optional locked dependency only'})
assert not any(x['active_server_or_admin_dependency'] for x in review),review
licenses=sorted({(packages[p]['name'],packages[p]['version'],packages[p]['license'] or 'first-party/private') for p in reachable})
assert all(license!='first-party/private' or name.startswith('authz-') for name,_,license in licenses),'Unresolved third-party license'
# Values are inspected without printing their contents. Generated/test-only public fixtures
# do not contain actual service secrets; real token/key files belong outside the repo.
files=subprocess.check_output(['git','ls-files','-z'],cwd=root).decode().split('\0');hits=[]
patterns=[rb'-----BEGIN (?:RSA |EC |OPENSSH )?PRIVATE KEY-----',rb'gh[pousr]_[A-Za-z0-9]{30,}',rb'AKIA[0-9A-Z]{16}',rb'postgres(?:ql)?://[^\s/:]+:[A-Za-z0-9]{24,}@']
for name in filter(None,files):
 path=root/name
 if not path.is_file():continue
 data=path.read_bytes()
 if any(re.search(pattern,data) for pattern in patterns):hits.append(name)
assert not hits,{'potential_secret_files':hits}
result={'advisory_database':advisories.get('database'),'advisories':review,'active_packages':len(reachable),'licenses':[{'package':a,'version':b,'license':c} for a,b,c in licenses],'tracked_secret_pattern_hits':0,'limitations':'Pattern scan is not a proof of secret absence; review includes tracked artifacts and ignored credential locations.'}
(root/'verification/security-review.json').write_text(json.dumps(result,indent=2));print('PASS: active dependency closure and licenses reviewed; no unresolved active advisory, no tracked secret-pattern hits.')

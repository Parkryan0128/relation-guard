#!/usr/bin/env python3
"""Create disposable local credentials. Refuses to overwrite an existing setup."""
import base64, hashlib, json, os, pathlib, secrets, uuid
root=pathlib.Path(__file__).resolve().parents[1]
if (root/'secrets').exists() or (root/'.env').exists():
    raise SystemExit('Existing setup found; no files changed. Reuse it or rotate keys explicitly.')
os.umask(0o077)
(root/'secrets').mkdir()
tenant=str(uuid.uuid4());password=secrets.token_hex(32);raw=secrets.token_bytes(32)
token='local.'+base64.urlsafe_b64encode(raw).decode().rstrip('=')
(root/'secrets/db_password').write_text(password)
(root/'secrets/keys.json').write_text(json.dumps({'version':1,'keys':[{'key_id':'local','tenant_id':tenant,'scopes':['check','explain','relations:read','relations:write','audit:read'],'secret_sha256':hashlib.sha256(raw).hexdigest()}]}))
(root/'secrets/client.json').write_text(json.dumps({'tenant_id':tenant,'token':token}))
(root/'.env').write_text(f'DATABASE_URL=postgres://relation_guard:{password}@db:5432/relation_guard\nAUTHZ_PORT=8080\n')
# Compose local secret bind mounts preserve the host mode. The directory is private,
# and this file contains only hashes; runtime UID 10001 needs read access.
(root/'secrets/keys.json').chmod(0o444)
print('Local credentials created. Tenant: '+tenant)
print('Run: docker compose run --rm migrate')
print('Then: docker compose run --rm --entrypoint authz-admin migrate tenant '+tenant)
print('Then: docker compose up -d server')

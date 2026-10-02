#!/usr/bin/env python3
"""Dependency-free design artifact checks; not an authorization/server test suite."""
import json
import re
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
errors = []
markdown = sorted(ROOT.rglob('*.md'))
for path in markdown:
    text = path.read_text(encoding='utf-8')
    if not text.strip():
        errors.append(f'Empty document: {path.relative_to(ROOT)}')
    for target in re.findall(r'\[[^\]]*\]\(([^)]+)\)', text):
        if target.startswith(('http://', 'https://', '#', 'mailto:')):
            continue
        local = target.split('#', 1)[0]
        if local and not (path.parent / local).resolve().exists():
            errors.append(f'Broken link: {path.relative_to(ROOT)} -> {target}')

examples = ROOT / 'spec/examples'
for path in examples.glob('*.json'):
    json.loads(path.read_text(encoding='utf-8'))
contracts = json.loads((examples / 'contracts.json').read_text())
cases = json.loads((examples / 'authorization-cases.json').read_text())
assert len(cases['cases']) == 12
assert len({c['name'] for c in cases['cases']}) == len(cases['cases'])
assert contracts['write']['request']['expected_revision'] == '0'
assert contracts['revoke']['request']['expected_revision'] == '1'
assert contracts['check_after_revoke']['response']['evaluated_revision'] == '2'
assert contracts['check_after_revoke']['response']['decision'] == 'deny'
assert len(contracts['batch_check']['request']['checks']) == len(contracts['batch_check']['response']['results'])
for label in ('database_failure', 'revision_conflict'):
    assert 'decision' not in contracts[label]['response']
    assert 'allowed' not in contracts[label]['response']

sql = (ROOT / 'spec/schema.sql').read_text()
for table in ('tenants', 'entities', 'relationships', 'mutation_receipts', 'audit_events'):
    assert f'CREATE TABLE {table}' in sql
assert 'relationships_one_parent' in sql
assert 'FOREIGN KEY (tenant_id, resource_kind, resource_id)' in sql
assert 'FOREIGN KEY (tenant_id, subject_kind, subject_id)' in sql
if errors:
    raise SystemExit('\n'.join(errors))
print(f'PASS: {len(markdown)} Markdown documents, local links, 2 JSON fixtures, basic contract and DDL artifact checks')
print('NOT CHECKED: Rust service behavior, PostgreSQL execution, security, performance, external URL availability')

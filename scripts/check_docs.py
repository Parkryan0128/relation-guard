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
assert len(cases['cases']) >= 12
assert len({c['name'] for c in cases['cases']}) == len(cases['cases'])
assert contracts['write']['request']['expected_revision'] == '0'
assert contracts['revoke']['request']['expected_revision'] == '1'
assert contracts['check_after_revoke']['response']['evaluated_revision'] == '2'
assert contracts['check_after_revoke']['response']['decision'] == 'deny'
assert len(contracts['batch_check']['request']['checks']) == len(contracts['batch_check']['response']['results'])
for label in ('database_failure', 'revision_conflict', 'unknown_commit_outcome'):
    assert 'decision' not in contracts[label]['response']
    assert 'allowed' not in contracts[label]['response']

# Scope of this checker: document consistency, not implementation correctness.
readme = (ROOT / 'README.md').read_text(encoding='utf-8')
assert not re.search(r'[\u1100-\u11ff\u3130-\u318f\uac00-\ud7af]', readme), 'README must be in English'
roadmap = (ROOT / 'docs/09-roadmap.md').read_text()
matrix = (ROOT / 'docs/12-acceptance-tests.md').read_text()
case_ids = re.findall(r'^\| (M[0-5]-\d{2}) \|', matrix, re.MULTILINE)
assert len(case_ids) == len(set(case_ids)), 'Duplicate acceptance case ID'
counts = [5, 6, 8, 7, 6, 5]
for stage, count in enumerate(counts):
    expected = {f'M{stage}-{n:02d}' for n in range(1, count + 1)}
    actual = {case for case in case_ids if case.startswith(f'M{stage}-')}
    assert actual == expected, f'Incomplete stage M{stage} acceptance cases'
    assert f'scripts/test-stage.sh M{stage}' in roadmap
    assert f'verification/M{stage}-report.md' in roadmap
replay = contracts['write_replay_after_revoke']
assert replay['response'] == contracts['write']['response']
assert replay['request'] == contracts['write']['request']
assert replay['response_headers']['Idempotency-Replayed'] == 'true'
assert replay['response_headers']['X-Request-Id'] != replay['response']['request_id']
assert contracts['deny_with_explain']['response']['proof'] == []
assert 'proof' not in contracts['check_after_revoke']['response']
assert contracts['empty_relationship_page']['response']['next_cursor'] is None
assert contracts['empty_audit_page']['response']['next_after_revision'] is None

sql = (ROOT / 'spec/schema.sql').read_text()
for table in ('tenants', 'entities', 'relationships', 'mutation_receipts', 'audit_events'):
    assert f'CREATE TABLE {table}' in sql
assert 'relationships_one_parent' in sql
assert "key_id ~ '^[A-Za-z0-9_-]{1,64}$'" in sql
assert 'FOREIGN KEY (tenant_id, resource_kind, resource_id)' in sql
assert 'FOREIGN KEY (tenant_id, subject_kind, subject_id)' in sql
if errors:
    raise SystemExit('\n'.join(errors))
print(f'PASS: {len(markdown)} Markdown documents, local links, English README, 37 acceptance IDs, 2 JSON fixtures, contract and DDL artifact checks')
print('NOT CHECKED: Rust service behavior, PostgreSQL execution, security, performance, external URL availability')

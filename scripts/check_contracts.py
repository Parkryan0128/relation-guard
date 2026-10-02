#!/usr/bin/env python3
"""M0 OpenAPI schema checks (pip install -r scripts/requirements-test.txt)."""
import copy
import json
from pathlib import Path
from jsonschema import Draft202012Validator
r=Path(__file__).resolve().parents[1]
spec=json.loads((r/'spec/openapi.json').read_text())
examples=json.loads((r/'spec/examples/contracts.json').read_text())
def validate(schema,value):
 root={'$ref':'#/components/schemas/'+schema,'components':spec['components']}
 Draft202012Validator(root).validate(value)
for name,req,res in [('write','WriteRequest','WriteResponse'),('revoke','WriteRequest','WriteResponse'),('check','CheckRequest','CheckResponse'),('batch_check','BatchRequest','BatchResponse'),('check_after_revoke','CheckRequest','CheckResponse'),('deny_with_explain','CheckRequest','CheckResponse'),('write_replay_after_revoke','WriteRequest','WriteResponse')]:
 validate(req,examples[name]['request']);validate(res,examples[name]['response'])
for name in ['database_failure','revision_conflict','unknown_commit_outcome']:validate('Error',examples[name]['response'])
for schema in spec['components']['schemas'].values():Draft202012Validator.check_schema(schema)
for field,value in [('extra',1),('subject',None),('action','admin')]:
 bad=copy.deepcopy(examples['check']['request']);bad[field]=value
 try:validate('CheckRequest',bad)
 except Exception:pass
 else:raise AssertionError('Invalid contract accepted')
print('PASS: OpenAPI 3.1 schemas and positive/negative contract examples')

flow=json.loads((r/"spec/examples/managed-flow.json").read_text())
for step in flow["steps"]:validate(step["schema"],step["request"])
print("PASS: managed grant/check/revoke workflow request schemas")

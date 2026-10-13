#!/usr/bin/env python3
"""OpenAPI schema checks (pip install -r scripts/requirements-test.txt)."""

import copy
import json
from pathlib import Path

from jsonschema import Draft202012Validator

root = Path(__file__).resolve().parents[1]
spec = json.loads((root / "spec/openapi.json").read_text())
fixtures = json.loads((root / "tests/fixtures/contracts.json").read_text())


def validate(schema, value):
    schema_root = {
        "$ref": "#/components/schemas/" + schema,
        "components": spec["components"],
    }
    Draft202012Validator(schema_root).validate(value)


for name, request_schema, response_schema in [
    ("write", "WriteRequest", "WriteResponse"),
    ("revoke", "WriteRequest", "WriteResponse"),
    ("check", "CheckRequest", "CheckResponse"),
    ("batch_check", "BatchRequest", "BatchResponse"),
    ("check_after_revoke", "CheckRequest", "CheckResponse"),
    ("deny_with_explain", "CheckRequest", "CheckResponse"),
    ("write_replay_after_revoke", "WriteRequest", "WriteResponse"),
]:
    validate(request_schema, fixtures[name]["request"])
    validate(response_schema, fixtures[name]["response"])
for name in ["database_failure", "revision_conflict", "unknown_commit_outcome"]:
    validate("Error", fixtures[name]["response"])
for schema in spec["components"]["schemas"].values():
    Draft202012Validator.check_schema(schema)
for field, value in [("extra", 1), ("subject", None), ("action", "admin")]:
    bad = copy.deepcopy(fixtures["check"]["request"])
    bad[field] = value
    try:
        validate("CheckRequest", bad)
    except Exception:
        pass
    else:
        raise AssertionError("Invalid contract accepted")
print("PASS: OpenAPI 3.1 schemas and positive/negative contract fixtures")

flow = json.loads((root / "tests/fixtures/managed-flow.json").read_text())
for step in flow["steps"]:
    validate(step["schema"], step["request"])
print("PASS: managed grant/check/revoke workflow request schemas")

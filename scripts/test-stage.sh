#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
stage="${1:?Usage: scripts/test-stage.sh M0|M1|M2|M3|M4|M5}"
case "$stage" in M[0-5]) ;; *) echo 'Unknown stage' >&2; exit 2;; esac
python_cmd="${PYTHON:-python3}"
: "${TEST_DATABASE_URL:?An isolated PostgreSQL 18 TEST_DATABASE_URL is required}"
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo build --workspace --locked
"$python_cmd" scripts/check_contracts.py
cargo test --workspace --locked m0_
if [[ "$stage" != M0 ]]; then
  if [[ ! -f crates/authz-core/src/evaluator.rs ]]; then echo 'M1 is not implemented' >&2; exit 1; fi
  PROPTEST_CASES="${PROPTEST_CASES:-10000}" cargo test -p authz-core --locked m1_
fi
if [[ "$stage" != M0 && "$stage" != M1 ]]; then
  echo 'M2 and later gates are not yet implemented; refusing to report PASS' >&2
  exit 1
fi

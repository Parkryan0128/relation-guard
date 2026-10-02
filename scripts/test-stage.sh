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
  cargo test -p authz-store --locked --test transactions
  if [[ "$stage" != M2 ]]; then
    cargo test -p authz-server --locked
    "$python_cmd" scripts/test_http.py
  fi
  echo 'Implemented regression cases passed; M2+ acceptance evidence is incomplete. See verification/status.md. Gate is NOT PASS.' >&2
  exit 1
fi
if [[ "$stage" == M1 ]]; then
  if [[ -z "${BENCHMARK_CPU:-}" ]]; then
    echo 'M1 functional checks passed; formal baseline requires accurate BENCHMARK_CPU metadata. Gate is NOT PASS.' >&2
    exit 1
  fi
  cargo bench --locked -p authz-core --bench evaluation
fi

#!/usr/bin/env bash
# Regression suite for implemented behavior; not an M0-M5 release certification.
set -euo pipefail
cd "$(dirname "$0")/.."
: "${TEST_DATABASE_URL:?An isolated PostgreSQL 18 TEST_DATABASE_URL is required}"
python_cmd="${PYTHON:-python3}"
stage="${1:-M3}"
case "$stage" in M[0-3]) ;; *) echo 'Usage: scripts/verify.sh [M0|M1|M2|M3]' >&2; exit 2;; esac
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo build --workspace --locked
"$python_cmd" scripts/check_docs.py
"$python_cmd" scripts/check_contracts.py
# Independent cases are serial; their explicit concurrent operations remain enabled.
export PROPTEST_CASES="${PROPTEST_CASES:-10000}"
export PROPTEST_RNG_SEED="${PROPTEST_RNG_SEED:-424242}"
case "$stage" in
  M0) cargo test --workspace --locked m0_ -- --test-threads=1 ;;
  M1)
    cargo test -p authz-core --locked -- --test-threads=1
    cargo test -p authz-store --locked --test migration m0_ -- --test-threads=1
    ;;
  M2) cargo test -p authz-core -p authz-store --locked -- --test-threads=1 ;;
  M3)
    cargo test --workspace --locked -- --test-threads=1
    "$python_cmd" scripts/test_http.py
    "$python_cmd" scripts/test_commit_faults.py
    "$python_cmd" scripts/test_client.py
    ;;
esac

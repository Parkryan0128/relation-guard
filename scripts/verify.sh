#!/usr/bin/env bash
# Regression suite for implemented behavior; not an M0-M5 release certification.
set -euo pipefail
cd "$(dirname "$0")/.."
: "${TEST_DATABASE_URL:?An isolated PostgreSQL 18 TEST_DATABASE_URL is required}"
python_cmd="${PYTHON:-python3}"
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo build --workspace --locked
"$python_cmd" scripts/check_docs.py
"$python_cmd" scripts/check_contracts.py
PROPTEST_CASES="${PROPTEST_CASES:-10000}" PROPTEST_RNG_SEED="${PROPTEST_RNG_SEED:-424242}" cargo test --workspace --locked
"$python_cmd" scripts/test_http.py

"$python_cmd" scripts/test_commit_faults.py

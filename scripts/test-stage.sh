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
"$python_cmd" scripts/check_docs.py
"$python_cmd" scripts/check_contracts.py
cargo test --workspace --locked m0_
[[ "$stage" == M0 ]] && exit 0
PROPTEST_CASES="${PROPTEST_CASES:-10000}" PROPTEST_RNG_SEED="${PROPTEST_RNG_SEED:-424242}" cargo test -p authz-core --locked m1_
cargo bench --locked -p authz-core --bench evaluation
[[ "$stage" == M1 ]] && exit 0
cargo test -p authz-store --locked --test transactions
cargo test -p authz-store --locked --test query_plans
[[ "$stage" == M2 ]] && exit 0
cargo test -p authz-server --locked
"$python_cmd" scripts/test_http.py
"$python_cmd" scripts/test_commit_faults.py
"$python_cmd" scripts/test_client.py
[[ "$stage" == M3 ]] && exit 0
: "${PG_BIN:?PostgreSQL client bin directory required for native recovery tests}"
EXISTING_TEST_DATABASE_URL="${EXISTING_TEST_DATABASE_URL:-$TEST_DATABASE_URL}" "$python_cmd" scripts/test_native_recovery.py
EXISTING_TEST_DATABASE_URL="${EXISTING_TEST_DATABASE_URL:-$TEST_DATABASE_URL}" "$python_cmd" scripts/test_pool_transport_stall.py
[[ "$stage" == M4 ]] && exit 0
: "${AUDIT_JSON:?Fresh cargo-audit JSON report required; see docs/14-native-verification.md}"
"$python_cmd" scripts/check_security.py
cargo build --release --locked -p authz-server -p authz-admin
EXISTING_TEST_DATABASE_URL="${EXISTING_TEST_DATABASE_URL:-$TEST_DATABASE_URL}" AUTHZ_BIN="$PWD/target/release" "$python_cmd" scripts/compare_batch.py
EXISTING_TEST_DATABASE_URL="${EXISTING_TEST_DATABASE_URL:-$TEST_DATABASE_URL}" AUTHZ_BIN="$PWD/target/release" "$python_cmd" scripts/run_native_load.py

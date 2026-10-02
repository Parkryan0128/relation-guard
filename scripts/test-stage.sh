#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
stage="${1:?Usage: scripts/test-stage.sh M0|M1|M2|M3|M4|M5}"
case "$stage" in M[0-5]) ;; *) echo 'Unknown stage' >&2; exit 2;; esac
python_cmd="${PYTHON:-python3}"
# One regression entry point owns the test inventory, including managed authorization.
regression_stage="$stage"
case "$stage" in M4|M5) regression_stage=M3 ;; esac
scripts/verify.sh "$regression_stage"
[[ "$stage" == M0 ]] && exit 0
cargo bench --locked -p authz-core --bench evaluation
case "$stage" in M1|M2|M3) exit 0 ;; esac
: "${PG_BIN:?PostgreSQL client bin directory required for native recovery tests}"
EXISTING_TEST_DATABASE_URL="${EXISTING_TEST_DATABASE_URL:-$TEST_DATABASE_URL}" "$python_cmd" scripts/test_native_recovery.py
EXISTING_TEST_DATABASE_URL="${EXISTING_TEST_DATABASE_URL:-$TEST_DATABASE_URL}" "$python_cmd" scripts/test_pool_transport_stall.py
EXISTING_TEST_DATABASE_URL="${EXISTING_TEST_DATABASE_URL:-$TEST_DATABASE_URL}" POOL_BURST_OUTPUT="${POOL_BURST_OUTPUT:-verification/pool-burst-current.json}" "$python_cmd" scripts/test_pool_burst.py
[[ "$stage" == M4 ]] && exit 0
: "${AUDIT_JSON:?Fresh cargo-audit JSON report required; see docs/14-native-verification.md}"
"$python_cmd" scripts/check_security.py
cargo build --release --locked -p authz-server -p authz-admin
EXISTING_TEST_DATABASE_URL="${EXISTING_TEST_DATABASE_URL:-$TEST_DATABASE_URL}" AUTHZ_BIN="$PWD/target/release" "$python_cmd" scripts/compare_batch.py
EXISTING_TEST_DATABASE_URL="${EXISTING_TEST_DATABASE_URL:-$TEST_DATABASE_URL}" AUTHZ_BIN="$PWD/target/release" "$python_cmd" scripts/run_native_load.py

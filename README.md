# Relation Guard

A Rust service for managing access to folders and documents, backed by PostgreSQL.

Give a team viewer access to a folder, and its members can view the documents inside it. Removing someone from the team removes that access path; they may still have access through another grant.

Application backends use the HTTP API to check access and change permissions. The service:

- Keeps each organization's users, teams and resources separate.
- Supports viewer, editor and manager access through direct grants, teams and parent folders.
- Checks who can change permissions and records completed writes for retries.

## How it works

```text
Access check
    ↓
Authenticate the service key and validate the actor
    ↓
Read direct grants, team membership and parent folders in one snapshot
    ↓
Return allow, deny or an error
```

Applications handle login and enforce the result. The backend sends its service key and the logged-in user's `X-Actor-Id`. Keep service keys on the backend, and block the protected action if the check fails.

## Project structure

```text
crates/authz-core/    Permission model and pure evaluator
crates/authz-store/   PostgreSQL reads, writes and management rules
crates/authz-server/  HTTP API, authentication, limits and metrics
crates/authz-admin/   Database setup and operator commands
migrations/          Database migrations
tests/               Shared fixtures and integration test helpers
scripts/             Contract, integration and recovery checks
benchmarks/          Load datasets and k6 profiles
```

## Build

Requirements:

- Rust 1.96.1, pinned in rust-toolchain.toml
- PostgreSQL 18
- Python 3 for the test scripts
- Docker Compose for container deployment

The server uses Axum, Tokio and SQLx. Dependency versions are pinned in Cargo.lock.

```bash
cargo build --locked --workspace
```

For Docker Compose:

```bash
python3 scripts/bootstrap-local.py
docker compose build
docker compose run --rm migrate
# Replace TENANT_UUID with the UUID printed by bootstrap-local.py.
docker compose run --rm --entrypoint authz-admin migrate tenant TENANT_UUID
docker compose run --rm --entrypoint authz-admin migrate bootstrap-admin TENANT_UUID local-admin
docker compose up -d server
```

The setup creates private local credentials and refuses to overwrite an existing setup. Keep `.env` and `secrets/` out of Git. The API binds to `127.0.0.1:8080`; change `AUTHZ_PORT` in `.env` if needed.

For native execution, `authz-admin` provides `migrate`, `tenant`, `bootstrap-admin`, `key` and `runtime-role` commands. Start `authz-server` with a runtime-role `DATABASE_URL` and `AUTHZ_KEYS_FILE`. Keep privileged database credentials for operator commands only. The key file has the shape `{"version":1,"keys":[CONFIG_ENTRY]}`, using `config_entry` from the `key` command. `bootstrap-admin` only works before the first administrator exists; `recover-admin` is the privileged recovery path.

## API

Requests use `Authorization: Bearer TOKEN` and `X-Actor-Id: USER_ID`. Routes are under `/v1/tenants/{tenant_id}`:

| Method | Route | Purpose |
|---|---|---|
| POST | `/check`, `/batch-check` | Check access for one resource or a batch |
| POST | `/write` | Change entities, roles or relationships |
| GET | `/revision`, `/relationships`, `/audit` | Read revision, relationships or audit records |

Batch checks accept up to 100 items and use the same database snapshot. Organization administrators manage users and teams; resource managers can share the resources they manage.

Writes require an `Idempotency-Key` and `expected_revision`. Reuse the same key and payload when retrying an uncertain write. Reads return their observed revision. The [OpenAPI specification](spec/openapi.json) defines request and response formats.

Health, readiness and metrics use a separate operations listener. In native mode, use `AUTHZ_BIND` and `AUTHZ_OPS_BIND` to set the two addresses. Keep the operations listener private.

## Design choices

Writes lock the tenant until the transaction ends. This simplifies conflict handling, but writes within one tenant must wait their turn.

Changes are authorized against the state before the request. New grants cannot authorize other changes in the same request. The resulting state must satisfy hierarchy rules and must not remove the last organization administrator.

Revision checks reject changes based on outdated state. Permission changes, the revision, audit records and a retry receipt commit together. If the commit succeeds but its response is lost, a matching retry returns the saved result while the receipt is retained.

Access checks read one PostgreSQL snapshot. A check already in progress may see permissions from before a revocation. Checking permission and updating another application's database are separate operations.

## Tests

The tests cover granting and revoking access, tenant isolation, rejected permission changes and concurrent writes. They also interrupt database connections around COMMIT to check rollback and retry behavior when the caller cannot tell whether a write succeeded.

Prepare a dedicated test PostgreSQL instance with `pg_stat_statements` enabled, then run:

```bash
python3 -m venv .venv
.venv/bin/pip install -r scripts/requirements-test.txt
export TEST_DATABASE_URL='postgres://postgres@127.0.0.1:5432/relation_guard_test'
PYTHON=.venv/bin/python scripts/verify.sh
```

The suite compares the evaluator with an independent reference across 10,000 generated graph cases by default. It also runs 309 HTTP assertions with OpenAPI response validation and five Python client tests for retries and invalid responses.

Recovery and connection-stall tests need PostgreSQL client tools and an isolated database:

```bash
export PG_BIN=/absolute/path/to/postgresql/bin
export EXISTING_TEST_DATABASE_URL="$TEST_DATABASE_URL"
python3 scripts/test_native_recovery.py
python3 scripts/test_pool_transport_stall.py
python3 scripts/test_pool_burst.py
```

Run `python3 scripts/test_deployment.py` for the isolated Compose deployment test. Generated results go to ignored `target/test-results/`; they are not committed.

## Benchmarks

Run the core evaluator benchmark:

```bash
cargo bench --locked -p authz-core --bench evaluation
```

For end-to-end load tests, use the same PostgreSQL client/database settings as the recovery tests:

```bash
cargo build --release --locked -p authz-server -p authz-admin
export AUTHZ_BIN="$PWD/target/release"
export K6_BIN=/absolute/path/to/k6
export RG_LOAD_DIR="$(mktemp -d)"
python3 scripts/run_native_load.py
```

The harness creates disposable datasets, measures single and batch checks, adds concurrent writes and stress, then checks recovery. It requires k6 2.3.0 and takes roughly 40 minutes. Its temporary working directory contains test credentials; keep it private.

## Scope

V1 uses a fixed user/team/folder/document model. Custom policy languages, nested teams, explicit deny rules, decision caching, gRPC and multi-region operation are outside its scope. Revoking one grant does not remove access through another grant.

Before upgrading an existing database, stop writers and back it up. Run migrations with operator credentials; the server does not apply them automatically. Rotate service keys by deploying both keys, updating callers, then removing the old key and restarting the server. `authz-admin cleanup-receipts` removes up to 1,000 receipts older than 24 hours without deleting audit records.

## Contact

- **Name:** Ryan Park
- **Email:** [parkryan0128@gmail.com](mailto:parkryan0128@gmail.com)
- **LinkedIn:** [linkedin.com/in/parkryan0128](https://www.linkedin.com/in/parkryan0128)
- **GitHub:** [github.com/Parkryan0128](https://github.com/Parkryan0128)

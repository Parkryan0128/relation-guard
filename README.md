# Relation Guard

A relationship-based authorization service written in Rust.

It stores permissions in PostgreSQL and lets application backends check access, share resources and revoke permissions over HTTP. It checks both who can access a resource and who can change its permissions.

## How it works

```text
Access check
    │
    ▼
Authenticate the backend and validate the actor
    │
    ▼
Read one PostgreSQL snapshot
    ├── Direct permissions
    ├── Team membership
    └── Inherited folder permissions
    │
    ▼
Evaluate → allow, deny or error
```

```text
Permission change
    │
    ▼
Authenticate the backend and lock the tenant
    ├── Return a matching receipt for a completed retry
    ├── Check the actor's authority before any changes
    └── Check the expected revision
    │
    ▼
Apply changes and validate the resulting state
    │
    ▼
Commit permissions, revision, audit and receipt together
```

The service:

1. Separates each organization's users, teams, folders and documents by tenant.
2. Supports viewer, editor and manager permissions through direct grants, teams and folder inheritance.
3. Allows organization administrators to manage users and teams, while resource managers can share their resources.
4. Prevents self-promotion through permissions added in the same request and protects the last organization administrator.
5. Evaluates up to 100 access checks in one batch using the same database snapshot.
6. Uses revision checks and idempotency receipts to handle concurrent changes and retries.

Applications still own login and enforce the returned decision. A trusted backend sends its service key and the logged-in user's `X-Actor-Id`. Service keys must stay on the backend; a failed check must block the protected action.

A check already in progress may see permissions from before a revocation. Checking permission and updating another application's database are separate operations.

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

Writes require an `Idempotency-Key` and `expected_revision`. Reuse the same key and payload when retrying an uncertain write. Reads return their observed revision. The [OpenAPI specification](spec/openapi.json) defines request and response formats.

Health, readiness and metrics use a separate operations listener. In native mode, use `AUTHZ_BIND` and `AUTHZ_OPS_BIND` to set the two addresses. Keep the operations listener private.

## Tests

Prepare a dedicated test PostgreSQL instance with `pg_stat_statements` enabled, then run:

```bash
python3 -m venv .venv
.venv/bin/pip install -r scripts/requirements-test.txt
export TEST_DATABASE_URL='postgres://postgres@127.0.0.1:5432/relation_guard_test'
PYTHON=.venv/bin/python scripts/verify.sh
```

The suite covers permission rules, tenant isolation, unauthorized changes, transaction rollback, concurrency, HTTP contracts and lost COMMIT acknowledgements. It includes 10,000 generated graph cases, 309 HTTP assertions and five Python client tests.

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

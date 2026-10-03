# Relation Guard

A relationship-based authorization service written in Rust.

It stores permissions in PostgreSQL and lets application backends check access, share resources and revoke permissions over HTTP. It checks both who can access a resource and who can change its permissions.

[Verification results](verification/status.md)

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

A check already in progress may see permissions from before a revocation. Checking permission and updating another application's database are separate operations. See the [consistency contract](docs/06-consistency.md).

## Project structure

```text
crates/authz-core/    Permission model and pure evaluator
crates/authz-store/   PostgreSQL reads, writes and management rules
crates/authz-server/  HTTP API, authentication, limits and metrics
crates/authz-admin/   Database setup and operator commands
migrations/          Database migrations
examples/            Python HTTP client
scripts/             Contract, integration and recovery checks
benchmarks/          Load datasets and k6 profiles
verification/        Test and performance evidence
```

## Build

Requirements:

- Rust 1.96.1, pinned in rust-toolchain.toml
- PostgreSQL 18
- Python 3 for the example client and test scripts
- Docker Compose for container deployment

The server uses Axum, Tokio and SQLx. Dependency versions are pinned in Cargo.lock.

```bash
cargo build --locked --workspace
```

Follow the [setup guide](docs/13-running.md) to create the database, service key and first administrator, then start the server natively or with Docker Compose.

After completing the Compose setup, run the sharing and revocation example:

```bash
python3 examples/client.py
```

## Tests

Prepare a dedicated test PostgreSQL instance with `pg_stat_statements` enabled, then run:

```bash
python3 -m venv .venv
.venv/bin/pip install -r scripts/requirements-test.txt
export TEST_DATABASE_URL='postgres://postgres@127.0.0.1:5432/relation_guard_test'
PYTHON=.venv/bin/python scripts/verify.sh
```

The suite covers permission rules, tenant isolation, unauthorized changes, transaction rollback, concurrency, HTTP contracts and lost COMMIT acknowledgements. It includes 10,000 generated graph cases, 309 HTTP assertions and five Python client tests.

See the [test stages](docs/09-roadmap.md) for recovery, deployment and load checks.

## Benchmarks

Run the core evaluator benchmark:

```bash
cargo bench --locked -p authz-core --bench evaluation
```

For end-to-end k6 measurements, follow the [load test setup](docs/14-native-verification.md).

The recorded macOS run completed 99,003 normal-load requests with zero errors or dropped iterations. The [full report](verification/new-mac-review/local/load-report.md) includes source, environment, datasets, latency percentiles and limitations. Deployment results and earlier failures remain in the [verification history](verification/status.md).

## Scope

V1 uses a fixed user/team/folder/document model. Custom policy languages, nested teams, explicit deny rules, decision caching, gRPC and multi-region operation are outside its scope. Revoking one grant does not remove access through another grant.

See the [HTTP API](docs/04-api.md), [management rules](docs/15-managed-authorization.md) and [operations guide](docs/07-operations.md) for the full contracts. Detailed design documents are in Korean.

## Contact

- **Name:** Ryan Park
- **Email:** [parkryan0128@gmail.com](mailto:parkryan0128@gmail.com)
- **LinkedIn:** [linkedin.com/in/parkryan0128](https://www.linkedin.com/in/parkryan0128)
- **GitHub:** [github.com/Parkryan0128](https://github.com/Parkryan0128)

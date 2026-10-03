# Relation Guard

A Rust authorization service for application backends. Store permissions in a dedicated PostgreSQL database and check or change them over HTTP.

## Features

- Users, teams, folders and documents, isolated by tenant.
- Viewer, editor and manager permissions, including team and folder inheritance.
- Organization administrators and checks on who can grant, revoke or change permissions.
- Atomic writes, revision conflicts, idempotent retries and audit records.
- Single and batch access checks, with optional decision evidence.

Applications own login and enforce the returned decision. A trusted backend sends the logged-in user's `X-Actor-Id` with its service key. Never expose service keys to end users. Errors must block the protected action.

## Run

Follow the [setup guide](docs/13-running.md) for Docker Compose or native execution. The [Python example](examples/client.py) demonstrates sharing, rejected privilege escalation and revocation.

Stack: Rust 1.96.1, Axum, Tokio, SQLx and PostgreSQL 18. Dependency versions are pinned in `Cargo.lock`.

## Test

With a dedicated test PostgreSQL database and the Python test dependencies installed:

```sh
PYTHON=.venv/bin/python scripts/verify.sh
```

See [test stages](docs/09-roadmap.md) and [recovery/load commands](docs/14-native-verification.md).

Latest local verification: 39 Rust tests, 10,000 generated graph cases, 309 HTTP assertions and five client tests passed. Normal-load measurements completed 99,003 requests with zero errors or dropped iterations. Docker image build passed; Compose on the new Mac remains blocked by Documents-folder access. These are measured results, not production throughput guarantees. [Evidence and earlier results](verification/status.md).

## Boundaries

Each check or batch reads one primary-database snapshot. Checks already in progress may observe permissions from before a revocation. A permission check and a business write in another database are not one atomic operation.

V1 uses the fixed `org-doc-v2` model. Login, custom policy languages, nested teams, explicit deny rules, decision caching, gRPC and multi-region operation are outside its scope. Removing one grant does not remove access through other grants.

## Documentation

Detailed documents are in Korean; API names and examples are in English.

- Design: [scope](docs/01-scope.md), [architecture](docs/02-architecture.md), [decisions](docs/10-decisions.md), [references](docs/11-references.md).
- Contracts: [access rules](docs/03-authorization-model.md), [management rules](docs/15-managed-authorization.md), [HTTP API](docs/04-api.md), [storage](docs/05-storage.md), [consistency](docs/06-consistency.md).
- Operations: [setup](docs/13-running.md), [security and recovery](docs/07-operations.md).
- Validation: [methodology](docs/08-validation.md), [stages](docs/09-roadmap.md), [test cases](docs/12-acceptance-tests.md), [results](verification/status.md).

Read [AGENTS.md](AGENTS.md) before changes. Run `python3 scripts/check_docs.py` to check documentation links and specification invariants.

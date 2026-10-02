# Relation Guard implementation instructions

## Status and boundaries

This repository currently contains a design proposal, not an implemented service. Read README and docs/01 through docs/09 before implementation. Do not silently turn proposed behavior or performance targets into verified claims. The user requested scoping and repository publication first; implement only after a subsequent implementation request.

## Normative order

Authorization semantics: docs/03. API: docs/04. Transactions/integration: docs/06. Storage: docs/05 and spec/schema.sql. If they conflict, resolve and update all affected documents before writing dependent code; do not guess a security-sensitive behavior. Numeric limits live in docs/04 and must be shared constants in implementation. ADRs explain decisions, not a second contradictory contract.

## Implementation principles

- Rust 2024 edition. Pin a verified stable toolchain and exact dependency lockfile in M0, not guessed versions from these documents.
- Separate pure authorization core from HTTP and PostgreSQL. Core must not depend on Axum, SQLx, Tokio or wall-clock time.
- First-party crates forbid unsafe code. No unwrap/expect for untrusted input or external failures in request paths.
- Never map DB failures, timeouts, corrupt graphs or evaluation exhaustion into successful allow/deny responses.
- Every storage operation is tenant-scoped. Use composite foreign keys and mandatory tenant context. No raw SQL interpolation of user inputs.
- Reads use one primary-DB REPEATABLE READ READ ONLY transaction per request. No replica reads or cross-request decision cache in v0.1.
- Writes lock the tenant row; check idempotency before expected revision; update graph, revision, audit, and receipt in one transaction.
- Any new authz behavior requires positive, negative, cross-tenant, and relevant concurrency tests. Use real PostgreSQL for SQL semantics.
- Use bounded work and bounded concurrency. No detached task per tuple or one network/DB request per batch item.
- Never commit real API tokens, tenant data, credentials, .env, raw production logs or captured environment dumps.
- Changing model semantics requires a new model version, migration/compatibility plan, and contract tests.

## Scope control

No custom policy DSL, dynamic arbitrary resource types, nested teams, explicit deny, Redis, gRPC, K8s or distributed caching without an explicit scope change. Do not add a full frontend to make the portfolio look bigger. Reuse the existing design fixtures as acceptance examples.

## Verification and reporting

M0 onward: fmt, Clippy with warnings denied, unit/property tests, real PostgreSQL integration tests, API/integration smoke checks. Exact commands are established when Cargo exists. Add benchmark results only with commit, hardware, limits, dataset, concurrency, errors and latency percentiles. Report unfinished checks candidly. Keep README's status and docs/09 checklist aligned with evidence.

## Language

Design explanations: Korean. Code identifiers, structured errors, and API properties: English. Keep third-party material linked and paraphrased, not copied into this repo.

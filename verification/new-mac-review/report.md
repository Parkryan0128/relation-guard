# Local re-review and expanded coverage

Status: **partial verification; database-dependent and load tests are blocked by local setup.** Reviewed working tree based on `afa34e2`. No new performance claim. This report supersedes neither earlier successful CI nor earlier failed local load results.

## Review scope

Reviewed the pure evaluator and strict input contracts, snapshot/fact loading, management authorization, mutation phases and canonical audit ordering, actor-bound receipts, last-admin invariant, SQL and migrations, HTTP routing/authentication/pagination/deadlines, operations/startup, operator CLI, Python integration client, and the corresponding test inventory.

The existing suite already exercises direct/team/inherited grants, role implications, missing resources, malformed graphs, tenant isolation, revision conflicts, receipt replay/expiry, snapshot races, competing writes and cycles (100 repetitions), management revocation races, last-admin protection, migrations, scope combinations, timeout/pool exhaustion, COMMIT ambiguity, disconnects, restart/restore and deployment.

## Added coverage

| Area | Added assertions |
|---|---|
| Core | Mixed batch result order, duplicate preservation, missing-item reasons and empty deny proofs; exact resource/grant limits and overflow, including no administrator bypass of resource limits |
| Store | All managed write rejection helpers now compare full tenant/entity/relation/audit/receipt contents rather than revision alone |
| Store | Direct + inherited + team manager paths survive partial revocation; removing the final membership blocks checks and writes |
| Store | New folders cannot become pre-existing authority within a manager's batch; the same hierarchy is legal for an administrator and usable after commit |
| Store | FK failure after entity creation and final cycle validation roll back all data and metadata |
| Store | Operation-order-independent replay, canonical effective audit diff, no-op receipt without extra audit/revision, and unchanged state after replay |
| Store | 100 valid input operations requiring 101 authority resources fail atomically; exactly 100 authority resources succeed |
| HTTP | Cross-tenant/version/shape cursor tampering, invalid limits/revisions, batch 100/101/empty boundaries and order, duplicate authorization/idempotency headers without mutation |
| Client | Malformed error envelopes, fixed retry budget, identical write payload/key/actor across retries, nonretryable errors |

Five new store tests and two new core tests are included in the regular workspace suite; HTTP and client additions use the existing verification entry point. These additions target identified gaps, not a claim of exhaustive correctness.

## Bug found and fixed

The Python client assumed an HTTP error body and its `error` field were dictionaries. Responses such as `[]` or `{"error":null}` escaped as `AttributeError`, instead of the documented `AuthorizationError`. A scripted HTTP peer reproduced six failing subcases before the fix. The client now validates the envelope and code type before dispatching retries. Malformed responses remain denied and are not retried blindly. Rust authorization semantics were not changed.

[Failing regression before fix](client-before.log); [passing client suite](client-after.log).

## Executed locally

- Rust 1.96.1, macOS arm64, existing locked dependencies.
- [Core tests](core-expanded.log): PASS, 11 tests; `PROPTEST_CASES=10000`, seed 424242.
- [Client tests](client-after.log): PASS, five real scripted-HTTP-peer tests.
- Formatting and [Clippy, warnings denied](clippy-final.log): PASS for all workspace targets.
- [All workspace tests compile](test-build.log): PASS (`--no-run` is not a database test pass).
- [Release application/admin build](release-build.log): PASS.
- Document links, OpenAPI fixtures and affected Python syntax: PASS.

## Not executed yet

The prior dedicated PostgreSQL at localhost:55438 is unavailable. Native PostgreSQL initialization is blocked by this execution environment's `shmget` permission. Docker CLI is present, but no daemon is reachable; launching `/Applications/Docker.app` reports `kLSNoExecutableErr` (missing executable). Docker runtime repair/start was requested from the user. GitHub CLI also reports invalid authentication, so this review has not been pushed or verified in CI.

Pending: real PostgreSQL/store tests, actual service HTTP suite, COMMIT/pool/recovery tests and the full unchanged S/M local load profiles. No pending test is recorded as passing. The new Mac's performance and the cause of the old Mac's stalls remain unmeasured.

Once the local Docker engine is available, start a dedicated loopback-only PostgreSQL 18 test service; run the application/test binaries locally with `scripts/verify.sh`, the recovery/fault harnesses, then `scripts/run_native_load.py` using a fresh output directory. Preserve all failed attempts and the previous load evidence.

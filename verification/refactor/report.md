# Refactor verification

Implementation: `b7c0dbcd5080d05ff35f29751f0dd1df8759b1e7` (2026-10-02).

## Changes

- Split the store into database setup, snapshot reads, transactional writes, and error mapping. Keep management rules together and reuse the read/evaluation path.
- Separate the HTTP data API from process startup/operations, share strict header extraction, and separate strict JSON parsing from typed contracts.
- Replace compressed mutation dispatch with a named operation function. Preserve phase order and canonical JSON order for writes/audit; serialization errors now return an internal error instead of silently using an empty sort key.
- Remove unused SQLx chrono support and store serde dependency; move test-only hex/Tokio dependencies to dev dependencies. The lockfile removes ten packages and adds none; no version upgrades.
- Consolidate stage test selection, including all managed-authorization tests. Share fault-proxy/metrics helpers, remove the obsolete duplicate prepared-pool experiment and superseded batch diagnostic script.
- Simplify the README while preserving historical failures and raw measurement evidence.

No authorization model, public HTTP API, migration, SQL statement, snapshot/transaction rule, timeout, or concurrency limit was changed. Existing tests and historical evidence remain available.

## Local validation

Native debug binaries on macOS arm64, Rust 1.96.1, existing dedicated PostgreSQL 18.6. No new Docker build or long local load run was needed.

| Check | Result |
|---|---|
| `scripts/verify.sh` | PASS: fmt, Clippy with warnings denied, build, contracts, all 32 Rust tests, 10,000 generated graph cases, 290 real HTTP assertions, COMMIT fault tests, three client tests |
| Prepared connection stall | PASS: 12/12 responses correct after a 750ms response stall at 10 requests/s |
| Short burst stall | PASS: 100/100 correct, zero overloads with a 400ms stall at 100 requests/s |
| Long burst stall | PASS of the fault expectation: a 900ms stall returned 65 correct responses and 35 expected overloads, then recovered; not a zero-error load result |
| Native recovery | PASS: privileges, restart, backup/restore, administrator protection, key rotation, readiness/outage and graceful shutdown |
| Dependency/license/secret review | PASS for the active server/admin closure. Raw cargo-audit still reports the previously documented RSA advisory in an inactive optional dependency; it is not compiled into the deployed binaries. |
| Shell syntax / affected Python compilation / document links | PASS |

Local artifacts were captured before the implementation commit. Their `source_commit` field records parent `028e3eb`; the tested working tree became `b7c0dbc`. The server binary SHA-256 was `4b9e6f1cfb3aa4e5dfeb30aa70dc9c21beff5f89c22644e34d4072812377f6b3`. No production source was edited after the regression build.

Evidence: [regression](regression.log), [query plans](query-plans.json), [pool stall](pool-stall.json), [burst stall](pool-burst.json), [recovery](recovery.json), [security review](../security-review.json).

## Linux CI

[Implementation verification](https://github.com/Parkryan0128/relation-guard/actions/runs/37078329256): PASS for both functional/recovery and clean release-container jobs. The latter built the release image and verified Compose restart/restore. [Recorded CI result](ci.json).

## Performance boundary

This is a structural cleanup, not a performance improvement claim. SQL and request limits are unchanged. A new full native load benchmark was not run for this refactor. Previous [Linux successes and unresolved local failures](../pool-burst/report.md) retain their original source/environment attribution; local memory pressure has not been established as the cause.

# Verification status

**Latest local re-review (2026-10-03):** source `0265298` passes 39 Rust tests, 10,000 generated graph cases, 309 HTTP assertions, five client tests, fault/recovery checks and all unchanged native S/M load gates. Normal S measurements: 99,003 correct responses, zero errors/drops; recovery 6,001/6,001. Docker build passes; local Compose secret mounting remains blocked by Documents access. GitHub authentication prevents pushing/new CI. [Full current evidence](new-mac-review/report.md). Earlier measurements below are historical and retain their source attribution.

**Latest refactor:** `b7c0dbc` separates responsibilities and removes redundant code/dependencies without changing authorization semantics. Local functional and fault/recovery suites pass; Linux functional/recovery and clean release-container CI also pass. [Refactor evidence](refactor/report.md). Performance figures below remain attributed to their original source commits.

**Latest pool-timeout result:** the bounded 500ms wait passes controlled short-stall, functional and container tests. Independent Linux native load gates PASS: 99,006 normal-load requests, zero errors/drops; recovery 6,001/6,001. The macOS rerun FAILS during longer stalls (40 overloads, four deadline errors); their external cause remains unresolved. Exploratory Linux stress dropped work and is not a throughput guarantee. [Full evidence and limitations](pool-burst/report.md). The earlier V1 results below retain their original source attribution.

**Current: managed-authorization V1 (`org-doc-v2`) implementation and approved verification complete.** [Full V1 report](v1/report.md).

| Current V1 verification | Result |
|---|---|
| Core, migration, real PostgreSQL transactions and concurrency, 290 HTTP assertions | PASS; includes 10,000 generated graph comparisons |
| Native restart, backup/restore, rotation and fault recovery | PASS |
| Clean Compose deployment and separate Python client | PASS locally and in Linux CI |
| Native S single/batch measurements, three runs each | PASS; 99,003 correct requests, zero unexpected responses or generator drops |
| Million-relationship mixed/stress and recovery | Completed; final recovery 6,000/6,000 correct, numeric gate PASS |
| Observed limits | Four warmup 503s and eight warmup drops; exploratory stress drops: one at 600/s, 266 at 1,000/s; no observed incorrect decision |

[Latest verified implementation CI](https://github.com/Parkryan0128/relation-guard/actions/runs/37064993225) passed both functional/recovery and container jobs. Native load source is `91b1c2106cd10f660b1abd1d2b874b309b7c27cb`; later client/docs/CI changes do not change its Rust binary. Native performance is not a resource-limited Linux certification. Credentials and private generator configuration are excluded from evidence.

## Historical v0.1 baseline

The following entries preserve the earlier baseline and its failed acceptance result. They do not describe current V1 verification.

All planned implementation stages and approved native test scenarios have been executed. Functional and recovery checks passed; **M5-native acceptance did not pass** because one batch repetition exceeded its error budget. This is not a production release or a container certification.

Original benchmark source: `afc0fe192681eba11e6867f6bf85413baa4598fb`. Subsequent diagnosis adds pool metrics, structured error logging and a service minimum of ten connections; historical performance results retain their original source and binary attribution. [Local regression](regression.log) and [clean Linux functional/recovery CI](https://github.com/Parkryan0128/relation-guard/actions/runs/37041474310) passed.

| Stage | Evidence | Status |
|---|---|---|
| M0 | [Contracts/build/constraints](M0-report.md) | PASS |
| M1 | [Reference/property tests and core baseline](M1-report.md) | Functional PASS; native baseline with hardware limitations |
| M2 | [Transactions, deterministic races and measured SQL](M2-report.md) | PASS |
| M3 | [261 HTTP assertions, scopes, deadlines, COMMIT fault proxy and client](M3-report.md) | PASS |
| M4-native | [Restart, restore, rotation and active-request drain](M4-report.md) | PASS locally and in clean Linux CI |
| M5-native | [All S/M/mixed/stress/recovery profiles and security review](M5-report.md) | Executed; batch run 3 failure-rate target missed |
| Original container gates | [Approved environment change](../docs/14-native-verification.md) | Deferred, not a container PASS |

S single-check runs all passed. S batch run 3 had six 503 responses out of 3,001 requests (0.20% versus the <0.1% target), with no incorrect decision or dropped iteration. The other two batch runs passed. [Finding and diagnostic limits](load-finding.json). No average or unchanged rerun is used to hide the failed repetition. The [earlier pre-optimization failure](load-before-setup-optimization/manifest.json) is retained as well.

The [connection-pool investigation](pool-timeout-investigation.md) documents a controlled transport-stall reproduction and a service minimum of ten prepared connections, with the original timeouts unchanged. Its diagnostic replays are separate evidence, not a rerun of the entire S/M release gate. The historical external trigger remains unproven.

All million-relationship profiles completed. The normal-load recovery after stress returned 6,001/6,001 correct responses. The [100-decision comparison](batch-comparison.json) and release-binary native recovery also passed. Disposable benchmark/recovery databases were removed by their harnesses; the existing dedicated PostgreSQL service was retained.

Docker image build succeeded earlier. Clean Compose execution remained blocked by host bind-mount access. At the user's instruction, native application/test processes used the existing dedicated test PostgreSQL instance. No Linux cgroup-limited performance or container restart/restore claim is made.

Run `scripts/verify.sh` for functional regression, or cumulative `scripts/test-stage.sh M4` for native recovery as well. M5 runs load and security checks and returns failure when the fixed targets are missed. See [commands and environment](../docs/14-native-verification.md). Credentials and private load-generator configuration are excluded from tracked evidence.

# Verification status

**Active pool-timeout investigation:** the bounded-wait change passes functional, controlled fault and container CI, but its extended local S-single-3 measurement failed. Independent Linux load verification is in progress. [Current report](pool-burst/report.md). The earlier V1 results below retain their original source attribution.

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

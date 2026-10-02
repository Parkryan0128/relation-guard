# Verification status

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

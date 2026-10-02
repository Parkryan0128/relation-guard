# Verification status

Production implementation: `afc0fe192681eba11e6867f6bf85413baa4598fb`, 2026-10-02 UTC. [Local regression](regression.log) and [clean Linux CI](https://github.com/Parkryan0128/relation-guard/actions/runs/37039157433) passed. Later CI/harness changes do not change the production source. This is a tested implementation, not a certified production release.

| Stage | Evidence | Status |
|---|---|---|
| M0 | [Contracts/build/constraints](M0-report.md) | PASS |
| M1 | [Reference/property tests and core baseline](M1-report.md) | Functional PASS; native baseline with hardware limitations |
| M2 | [Transactions, deterministic races and measured SQL](M2-report.md) | PASS |
| M3 | [261 HTTP assertions, scopes, deadlines, COMMIT fault proxy and client](M3-report.md) | PASS |
| M4-native | [Restart, restore, rotation and native recovery](M4-report.md) | PASS, including active-request drain in clean Linux CI |
| M5-native | [Load and release review](M5-report.md) | Full optimized workload running |
| Original container gates | [Approved environment change](../docs/14-native-verification.md) | Deferred, not a container PASS |

The first native load attempt missed its second single-check run's error and p99 targets. [Original raw evidence](load-before-setup-optimization/manifest.json) is retained. Transaction setup round trips were reduced, all functional/native recovery tests passed again, and the same fixed performance thresholds are being measured again. No failed run is removed from the record or averaged away.

Docker image build succeeded. Clean Compose execution remained blocked by host bind-mount access. At the user's instruction, native application/test processes now use the existing dedicated test PostgreSQL instance. Generated recovery/load databases are isolated and removed by their harnesses. No Linux cgroup-limited performance or container restart/restore claim is made.

Run `scripts/verify.sh` for functional regression, or cumulative `scripts/test-stage.sh M4` for native recovery as well. See [commands and environment](../docs/14-native-verification.md). Credentials and private load-generator configuration are excluded from tracked evidence.

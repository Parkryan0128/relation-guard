# Verification status

## Latest local verification — 2026-10-03

Performance source: `0265298`. [Report and artifacts](new-mac-review/report.md).

The final documentation cleanup removed unused test setup; production code is unchanged. The full `scripts/verify.sh` regression suite passed again after cleanup. Performance measurements below remain from the recorded source.

| Check | Result |
|---|---|
| Rust / generated graph cases | PASS: 39 tests / 10,000 cases |
| HTTP / Python client | PASS: 309 assertions / five tests |
| COMMIT faults, connection stalls, restart, restore and key rotation | PASS |
| Normal S load, three single and three batch runs | PASS: 99,003 correct responses; zero errors or dropped iterations |
| Million-relationship profiles and post-stress recovery | PASS; recovery 6,001/6,001 |
| Docker image build | PASS |
| Compose on the new Mac | BLOCKED: Docker cannot mount secrets from Documents |

Native measurements are not resource-limited Linux certification or production throughput guarantees. Earlier Linux container passes do not establish a new-Mac Compose pass. A faster new run does not prove the cause of old-Mac stalls.

## Earlier evidence

| Source | Result and limitations |
|---|---|
| [Refactor](refactor/report.md) | Functional, fault/recovery and Linux container checks passed for `b7c0dbc`. No new performance measurement. |
| [Pool reliability](pool-burst/report.md) | Linux normal-load gates passed: 99,006 correct requests, zero errors/drops. Old-Mac rerun failed with 40 overloads and four deadline errors; external cause unresolved. Exploratory Linux stress dropped work. |
| [Managed authorization V1](v1/report.md) | Functional, native recovery, Compose/Linux CI and native load gates passed for the reported sources. Warmup had four 503s/eight drops; exploratory stress also dropped work. |
| [v0.1 M0](M0-report.md), [M1](M1-report.md), [M2](M2-report.md), [M3](M3-report.md), [M4](M4-report.md) | Functional and native recovery passed. Initial container execution was blocked by host mounting. |
| [v0.1 M5](M5-report.md) | FAIL: batch run 3 returned six 503s in 3,001 requests (0.20%, target <0.1%). Other passes do not cancel this failure. |

The [pool investigation](pool-timeout-investigation.md), [original finding](load-finding.json) and [pre-optimization result](load-before-setup-optimization/manifest.json) remain available. Reports preserve the actual source, environment and observed failures.

For reproduction, use the [setup guide](../docs/13-running.md) and [native recovery/load commands](../docs/14-native-verification.md). Private credentials and generator configuration are excluded from tracked evidence.

# Connection-pool timeout investigation — 2026-10-02

**Conclusion:** a brief DB transport stall combined with on-demand connection creation can reproduce the observed kind of 503. Keeping the service pool's ten connections prepared prevents that failure in the controlled experiment. The original event's external trigger is still unproven because the original run did not capture per-error codes, connection state or scheduler/network traces. This is a reproduced failure mechanism and a targeted hardening change, not a claim of forensic certainty about the historical event.

## Original evidence

[Original finding](load-finding.json): S-batch-3 had 6 HTTP 503 responses out of 3,001 requests. Data SELECT count was exactly 2,995 × 3 and core evaluations exactly 2,995: all failed requests stopped before that path. The error-containing five-second sample accrued approximately 1.66 seconds of pool-acquisition time. PostgreSQL logs in the 17:47:50–17:48:25 UTC failure window contained no entries; absence of those logs does not rule out a DB/VM/network stall.

SQLx 0.8.6 source was inspected locally. The 250ms acquire limit includes both waiting for a free connection and creating/initializing a new connection. The previous service configuration set maximum 10, but retained default minimum 0, idle timeout 10 minutes and maximum lifetime 30 minutes. Maximum size is not a promise that ten established connections exist. SQLx also checks connections on release before making them available again.

The old endpoint did not log the individual failure code. SQLx's default slow-acquire threshold is 2 seconds, and its slow-acquire log is emitted after a successful acquire; it did not provide the missing timeout evidence. It would be incorrect to claim that changing that threshold alone fixes the failures.

## Unchanged-policy diagnostic replay

Instrumentation-only source: `cfde18b0ac4f1e32c94bf846d7ff72f25b5d104b`. Same S dataset and 100-item batch at 10 requests/s; one 60-second warmup followed by three consecutive 300-second measurements. All 9,002 measured requests succeeded with no dropped iteration. Observed pool size was generally 2–3. This did **not** reproduce the historical failure and is not a replacement PASS for it. It also does not recreate the original 35-minute single-plus-batch process age.

[Raw replay](batch-diagnosis/report.json) includes per-second pool metrics, HTTP probe timing and host sampler interval. Diagnostic runs deliberately add that sampling; they are separate from the original formal baseline.

## Controlled transport experiment

A test-only TCP proxy holds PostgreSQL response delivery for 750ms. The application receives 12 revision-read requests at 10 requests/s. Revision reads use the same pool-acquisition path as batch checks, isolating it from the authorization evaluator. No production fault endpoint or proxy is added to the server.

| Version / state | Established idle connections before fault | Acquire timeouts | HTTP outcome |
|---|---:|---:|---|
| Before, ordinary startup | 2 | 4 | 8 success, 4 × 503 OVERLOADED at roughly 254–256ms |
| Before, explicitly warmed by the experiment | 10 | 0 | 12 success |
| After, ordinary startup | 10 | 0 | 12 success |
| After, explicitly warmed by the experiment | 10 | 0 | 12 success |

BEGIN-error counters stayed zero, and every case recovered after the proxy released responses. Prepared connections avoid establishing a new connection within the 250ms acquire budget; they do not eliminate the injected DB delay. The first post-change response took about 769ms, within the existing request deadline, so this experiment establishes availability under this bounded stall, not a low-latency guarantee during a stall. [Before evidence](pool-transport-stall-before.json), [after evidence](pool-transport-stall.json). This proves susceptibility to the injected condition and effectiveness of preparation for that condition; it does not prove the original host experienced exactly a 750ms network stall.

## Change

Service source: `d665e2f9874869039e131eb84942052834534692`. `authz-store::service_pool` sets minimum and maximum connections to 10. Admin commands retain on-demand pools. This uses the existing maximum connection budget more consistently; allow ten DB connections per service replica. SQLx replenishes the minimum on a best-effort basis after connection loss. A real outage or exhausted pool can still correctly return an error.

The acquire deadline stays 250ms, SQL timeout 1s and request deadline 2s. Authorization semantics, tenant boundaries, snapshot consistency, limits and performance thresholds are unchanged. Added fixed-name counters distinguish acquire timeout, other acquire failure and BEGIN failure; error logs record request ID, bounded error code and pool size/idle count without tokens or request payloads. The load generator now retains each unexpected response's code and timestamp.

## Validation after hardening

Full Rust/property/PostgreSQL/HTTP/COMMIT-fault/client regression passed. The existing pool-exhaustion test now also verifies the diagnostic counter; true exhaustion still returns 503 and recovers. The controlled transport-stall case is included in the native stage and CI recovery workflow. [Regression log](pool-hardening-regression.log). [Clean Linux CI including the new transport experiment](https://github.com/Parkryan0128/relation-guard/actions/runs/37054316758) passed on the service source above.

Targeted batch replay is complete on source `d665e2f9874869039e131eb84942052834534692` using the recorded release-binary hash. Its protocol is the diagnostic replay described above, not a rerun of the entire original S/M acceptance suite.

| Run | Correct responses | Unexpected | Dropped | Successful p95 ms | Successful p99 ms |
|---|---:|---:|---:|---:|---:|
| batch-1 | 3001 | 0 | 0 | 29.30 | 39.48 |
| batch-2 | 3001 | 0 | 0 | 28.23 | 37.86 |
| batch-3 | 3001 | 0 | 0 | 28.61 | 44.21 |

All 9,003 measured batch requests succeeded, with zero dropped iterations and zero acquisition/BEGIN error counters. Every sampled pool size was ten; each run met the batch p95 <100ms and failure-rate <0.1% targets.

[Raw post-change replay](batch-diagnosis-warm/report.json). Original benchmark artifacts and the original failed acceptance result remain intact. A clean targeted replay does not prove the historical external cause or retroactively pass the complete release gate.

## Reproduction

Use the dedicated test PostgreSQL and native environment from [the native guide](../docs/14-native-verification.md). Build the release server/admin binaries first.

```sh
AUTHZ_BIN="$PWD/target/release" python3 scripts/test_pool_transport_stall.py
RG_DIAGNOSTIC_DIR=/absolute/private/scratch/directory \
RG_DIAGNOSTIC_OUTPUT="$PWD/verification/new-batch-diagnosis" \
AUTHZ_BIN="$PWD/target/release" python3 scripts/diagnose_batch.py
```

The transport test defaults to expecting a prepared service pool. `EXPECT_COLD_POOL_TIMEOUT=1` is only for explicitly testing the older binary, with a separate `POOL_STALL_OUTPUT` path. Each harness creates and removes its own database. Private key/config directories are excluded from repository evidence.

## CI test isolation follow-up

[Evidence-only commit CI](https://github.com/Parkryan0128/relation-guard/actions/runs/37056234115) subsequently failed in `m2_snapshot_and_concurrent_writers`: a losing writer returned `OVERLOADED` instead of `REVISION_CONFLICT`. This is distinct from the batch-load experiment. That test's custom pool uses SQLx's default 30-second acquisition timeout, while the entire failing test took approximately 1.3 seconds; the observed code is consistent with the existing 500ms tenant-row lock timeout. Eight store tests, including heavy graph fixtures, were executing concurrently against the same CI PostgreSQL service.

The regression runner now serializes independent test cases with `--test-threads=1`. Explicit races within each test retain their barriers and concurrent operations, including 100 competing-writer and 100 identical-idempotency-key pairs. No assertion, server deadline, lock timeout or performance target is relaxed. This isolates functional concurrency checks from unrelated test load; dedicated load profiles remain responsible for measuring overload behavior. The failed CI record is retained rather than replaced by an unchanged retry.

The [local regression with isolated test scheduling](pool-isolation-regression.log) passed all functional, property, PostgreSQL, HTTP, COMMIT-fault and client checks. Service code and the release binary used for the 9,003-request replay are unchanged by this test-runner adjustment.

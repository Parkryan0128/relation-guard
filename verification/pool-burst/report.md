# Short pool saturation correction

Status: controlled short-stall reproduction, functional/container regression and independent Linux native load gates PASS. Local macOS load gates FAIL (40 overloads, four deadline errors and generator drops); the source of its longer stalls remains unresolved. The correction is validated for short saturation, not a claim that all 503s are eliminated.

## Finding and change

The four V1 warmup errors were `OVERLOADED` during connection acquisition, with all ten connections occupied. The acquisition-timeout counter increased by four; BEGIN errors did not increase. Their historical external trigger remains unknown. [Original events](../v1/load/server-errors.json).

The previous 250ms acquisition budget rejected requests during a short, recoverable stall. The budget is now 500ms, using one FIFO pool acquisition rather than cancel/retry loops. This deliberately trades up to 250ms more waiting under sustained saturation for tolerance of short stalls. It does not make database queries faster. The overall HTTP deadline remains 2s, statement timeout 1s, lock timeout 500ms, readiness timeout 250ms, maximum connections ten and maximum in-flight requests 128. No transactions or writes are automatically replayed. [Decision](../../docs/10-decisions.md#adr-014--짧은-연결-포화의-대기-예산-2026-10-02).

## Before/after controlled experiment

The same harness forwards actual PostgreSQL traffic through a loopback proxy, pauses DB responses for a fixed duration and sends 100 real HTTP checks at 100 requests/s. Checks alternate known allow and deny decisions from a registered member. This reproduces the failure mechanism, not the historical external trigger.

| Injected response stall | Before correct / 503 | After correct / 503 |
|---|---:|---:|
| 400ms | 90 / 10 | 100 / 0 |
| 900ms | 40 / 60 | 65 / 35 |

Every successful response matched the expected decision. Errors had no decision; every 503 matched an acquisition timeout. Both configurations recovered after the stall. The long-stall case intentionally verifies bounded rejection rather than hiding overload. Request-level durations and exact binary hashes are in [before](before.json) and [after](after.json). This is a local native experiment, not a production SLO.

## Regression

[Full local log](regression.log): format, Clippy, build, 10,000 generated graph comparisons, real PostgreSQL store/concurrency/migration tests, 290 HTTP assertions, COMMIT ambiguity and Python client tests passed. New real TCP cases hold all ten connections for 350ms and verify both reads and writes wait and succeed exactly once. Sustained exhaustion returns OVERLOADED before the request deadline, leaves the attempted write absent and preserves the revision. Existing in-flight, lock, deadline and recovery tests remain enabled.

The [Linux CI run for the fix](https://github.com/Parkryan0128/relation-guard/actions/runs/37069873323) passed both functional/recovery and container-deployment jobs, including `test_pool_burst.py`. No claim is made that arbitrary host/DB pauses or sustained overload can never cause 503.

## Supplemental local observation

After the S-single-3 failure, a separate observer sampled a persistent PostgreSQL connection and the operations HTTP listener at five probes/s each, plus a 20ms host timer. It records counts and wait classes, not query text, credentials or entity identifiers. The subsequent local batch/M phases include this observer overhead; the failed single-check phase predates it. Reproduction helper: `scripts/observe_native_stalls.py PRIVATE_CONTEXT_JSON OUTPUT_JSONL`.

During the later batch-2 failures, an independent DB activity query took 1,943.893ms and an operations HTTP query (which does not use the DB) took 1,166.853ms. Sampled DB sessions were largely waiting for client messages, with no sampled row-lock wait explaining the episode. A host timer also recorded scheduling delays during the observation session. [Timestamped observations](stall-observation.json).

In batch-2 resource sample 22 (roughly a five-second sampling interval), aggregate DB-query elapsed time increased by 30.275395s and pool-wait time by 12.652737s, versus 0.025615s in core evaluation; 18 additional errors were counted. These are summed concurrent wall times, not CPU seconds or one request latency. PostgreSQL also logged three ancestor-query statement timeouts during the episode. The longest successful client-observed response in that batch profile was 2,924.723ms; the HTTP request budget does not guarantee client-observed wall time during scheduling or transport stalls. [Sampled metrics](load/S-batch-2-resources.json).

The old four warmup failures and the new 44 measured errors occurred in different executions with different observed stall severity. Their counts alone do not establish that the budget change improved or worsened the full workload. Only the controlled 400ms before/after experiment isolates that specific change.

This is evidence of broader execution/transport delays, not proof of a specific OS, Docker or hardware cause. It does not retrospectively identify the trigger of the original four warmup failures. Extending waits indefinitely would merely conceal the problem and consume the request budget; the service remains fail-closed. No further timeout increase was made.

## Native load

Native numeric gates: **FAIL**. Source `28af0d5ba7cd524b19607d6d2a8842f3f7e918ca`, clean production source `True`, release server SHA-256 `2d90e9f4e6b4591d47a29bdddf1407aa5070d9f8fb4d358f4422036ef1ceb8f2`. The same S/M datasets, fixed arrival rates, three 300-second measurements per S profile, 60-second warmups and unchanged acceptance thresholds were used. [Manifest](load/manifest.json).

| Profile | Correct | Unexpected | Generator dropped | Successful p95 ms | Successful p99 ms | Gate |
|---|---:|---:|---:|---:|---:|---|
| S-single-1-warmup | 6001 | 0 | 0 | 8.65 | 79.82 | observation |
| S-single-1 | 29992 | 0 | 9 | 6.01 | 21.60 | PASS |
| S-single-2-warmup | 6001 | 0 | 0 | 6.63 | 21.97 | observation |
| S-single-2 | 30001 | 0 | 0 | 6.13 | 17.30 | PASS |
| S-single-3-warmup | 6001 | 0 | 0 | 5.92 | 15.79 | observation |
| S-single-3 | 29919 | 22 | 60 | 8.46 | 89.76 | FAIL |
| S-batch-1-warmup | 601 | 0 | 0 | 39.46 | 70.93 | observation |
| S-batch-1 | 3000 | 0 | 0 | 37.65 | 130.87 | PASS |
| S-batch-2-warmup | 600 | 0 | 0 | 37.83 | 103.39 | observation |
| S-batch-2 | 2978 | 22 | 0 | 77.25 | 920.16 | FAIL |
| S-batch-3-warmup | 600 | 0 | 0 | 33.91 | 44.75 | observation |
| S-batch-3 | 3001 | 0 | 0 | 32.06 | 44.55 | PASS |
| M-single | 6001 | 0 | 0 | 6.55 | 19.17 | observation |
| M-mixed-hot | 6001 | 0 | 0 | 7.00 | 14.43 | observation |
| M-mixed-distributed | 6001 | 0 | 0 | 7.99 | 23.30 | observation |
| M-stress-100 | 3001 | 0 | 0 | 7.66 | 29.81 | observation |
| M-stress-300 | 9001 | 0 | 0 | 4.17 | 11.13 | observation |
| M-stress-600 | 18001 | 0 | 0 | 6.44 | 22.18 | observation |
| M-stress-1000 | 29978 | 0 | 23 | 7.35 | 19.38 | observation |
| M-recovery | 6000 | 0 | 0 | 5.62 | 18.47 | PASS |

S measurements: 98,891 correct, 44 unexpected, 69 dropped. Warmups: 0 unexpected, 0 dropped. No incorrect decision was observed. Mixed/stress results are exploratory, not guaranteed throughput; drops count separately from latency. [Server errors](load/server-errors.json).

This is macOS native execution against the dedicated local PostgreSQL service without CPU/memory caps. It is not resource-limited Linux performance certification. Passing one run does not guarantee immunity to longer DB/network/host stalls. The prior failed observations remain unchanged.

## Independent Linux measurement

[Linux load CI](https://github.com/Parkryan0128/relation-guard/actions/runs/37072149104) **PASS**, with the same fixed S/M workloads and unchanged numeric thresholds. This is an independent 2-logical-CPU Linux x86_64 runner, native release application and PostgreSQL 18.6 service container on the same host, without the supplemental local observer. It removes dependence on the development laptop but is not a controlled hardware-equivalent comparison or proof of the laptop's failure cause. The harness imposes no per-process CPU/memory caps; this is not the separately specified resource-limited container certification.

Source `d748eac01fa936ab3b649b01877a46deaba71c1a`, production source clean, release SHA-256 `33d25c3aaac0a1c034e25da5657d9d5b5276b155342add2ec31dc6d0562fe5db`. [Manifest](linux-load/manifest.json), [execution log](linux-load.log).

| Profile | Correct | Unexpected | Generator dropped | Successful p95 ms | Successful p99 ms | Gate |
|---|---:|---:|---:|---:|---:|---|
| S-single-1 | 30001 | 0 | 0 | 2.38 | 3.46 | PASS |
| S-single-2 | 30001 | 0 | 0 | 2.46 | 3.34 | PASS |
| S-single-3 | 30001 | 0 | 0 | 2.37 | 3.17 | PASS |
| S-batch-1 | 3001 | 0 | 0 | 13.11 | 16.08 | PASS |
| S-batch-2 | 3001 | 0 | 0 | 13.71 | 19.30 | PASS |
| S-batch-3 | 3001 | 0 | 0 | 13.40 | 16.22 | PASS |
| M-single | 6001 | 0 | 0 | 2.74 | 4.35 | observation |
| M-mixed-hot | 6001 | 0 | 0 | 3.71 | 4.57 | observation |
| M-mixed-distributed | 6001 | 0 | 0 | 3.64 | 4.33 | observation |
| M-stress-100 | 3000 | 0 | 0 | 2.78 | 3.69 | observation |
| M-stress-300 | 9001 | 0 | 0 | 2.58 | 5.39 | observation |
| M-stress-600 | 17788 | 0 | 213 | 207.08 | 221.01 | observation |
| M-stress-1000 | 18104 | 0 | 11897 | 237.13 | 256.90 | observation |
| M-recovery | 6001 | 0 | 0 | 2.74 | 3.91 | PASS |

The six S measurement intervals returned **99,006 correct responses with zero unexpected responses and zero dropped iterations**. All six warmups also had zero unexpected responses and drops. No wrong decision was observed in any profile, and no unexpected HTTP failure was observed across the run. Recovery returned 6,001/6,001 correct responses.

At the exploratory 600/s and 1,000/s offered rates, the generator dropped 213 and 11,897 iterations respectively. This runner therefore did not sustain those offered rates. Successful-response percentiles do not include dropped work; the run is not evidence of a 1,000 requests/s guarantee. Linux success does not erase the local failure.

## Conclusion and remaining limitation

The controlled experiment proves that the bounded 500ms wait absorbs the tested 400ms stall which the old 250ms wait rejected, while sustained saturation still fails closed and recovers. Functional and Linux normal-load behavior are verified. The longer local stalls are not fixed or causally attributed; the original four warmup events lack the observations needed to identify their external trigger. Further local root-cause work needs host/VM resource and scheduling evidence during a reproduced stall. Increasing timeouts again, rerunning until green or claiming a universal zero-error guarantee would not resolve that uncertainty.

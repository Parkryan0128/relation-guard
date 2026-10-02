# Short pool saturation correction

Status: targeted fault reproduction and functional regression passed, but the extended local load run has failed its third single-check gate (22 overloads, 60 generator drops, p99 89.76ms). Investigation and independent Linux measurement are in progress; the 503 issue is not claimed fully resolved.

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

## Native load

Pending; previous V1 measurements remain unchanged in their own directory.

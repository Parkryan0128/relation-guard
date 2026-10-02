# M5-native — numerical target missed

Production source: `afc0fe192681eba11e6867f6bf85413baa4598fb`. Release binary SHA-256: `eb8f6a85472ed307cc030ab4c341c2fd767fdec89fbefbf1e8db47070bf8ca39`. Production source was clean when the run started. Later commits change test tooling, CI and documentation only. [Full manifest](load/manifest.json), [native testing plan](../docs/14-native-verification.md).

M5-01 functional and native recovery tests passed in a [clean Linux checkout](https://github.com/Parkryan0128/relation-guard/actions/runs/37041474310). M5-02 native S targets were missed in one or more runs; see the manifest. M5-03 completed the million-relationship, mixed, stress and recovery profiles. M5-04 dependency/secret review is recorded below. This does not certify the original container deployment or Linux resource-limited release gates.

## Method

Native macOS arm64, Darwin 25.2, eight logical CPUs; Rust 1.96.1, PostgreSQL 18.6, k6 2.3.0. Application and load generator share the host; PostgreSQL is the existing dedicated local test endpoint. No cgroup CPU/memory limits. Exact CPU model, physical memory, DB CPU/IO and storage characteristics were not captured. Accordingly these results are a local engineering baseline, not a hardware-normalized comparison or production SLA.

Deterministic generator seed 424242. S: one tenant, 1,000 users, 100 teams, 1,000 folders, 10,000 documents and exactly 50,000 relationships, depth at most eight. M: ten tenants, each with 1,000 users, 100 teams, 1,000 folders, 20,000 documents and 100,000 relationships; one million relationships in total.

Each S repetition has 60 seconds of warmup followed by 300 seconds of measurement. Single: 100 requests/s, 50:50 allow/deny. Batch: 10 requests/s with 100 distinct resources, 50:50 allow/deny requests, equivalent to 1,000 decisions/s. k6 uses constant arrival rate, 32 preallocated VUs and at most 128. Warmup files remain available but are excluded from the table. Boundary scheduling sometimes produces one extra iteration; scheduled count is actual iterations plus dropped iterations, with the nominal target retained separately.

Percentiles below include successful responses only; unexpected and dropped counts are reported separately. Failure ratio is `(unexpected + dropped) / scheduled`. Every profile had zero wrong authorization decisions. Fixed S gates: single p95 <=20ms, p99 <=50ms; batch p95 <=100ms; each run's failure ratio <0.1%. No average hides a failed repetition.

## S baseline

| Profile | Scheduled | Correct | Unexpected | Dropped | p50 ms | p95 ms | p99 ms |
|---|---:|---:|---:|---:|---:|---:|---:|
| S-single-1 | 30,001 | 30,001 | 0 | 0 | 3.36 | 6.41 | 18.39 |
| S-single-2 | 30,001 | 30,001 | 0 | 0 | 3.27 | 6.08 | 17.99 |
| S-single-3 | 30,000 | 30,000 | 0 | 0 | 3.27 | 6.35 | 21.84 |
| S-batch-1 | 3,000 | 3,000 | 0 | 0 | 22.38 | 30.46 | 49.10 |
| S-batch-2 | 3,001 | 3,001 | 0 | 0 | 22.18 | 29.90 | 49.93 |
| S-batch-3 | 3,001 | 2,995 | 6 | 0 | 21.34 | 29.16 | 42.04 |

**Failed gate:** S-batch-3 returned six HTTP 503 responses, 6/3,001 = 0.20%, exceeding the <0.1% per-run limit. All six occurred before the data SELECT/evaluation path; the same five-second sample interval contains a pool-wait spike. That is consistent with acquire timeout, but the response error codes and host/DB scheduler traces were not captured, so the underlying cause is not established. [Diagnostic record](load-finding.json). The run remains failed; no unchanged retry replaces it.

## Larger dataset, mixed traffic and saturation

| Profile | Scheduled | Correct | Unexpected | Dropped | p50 ms | p95 ms | p99 ms |
|---|---:|---:|---:|---:|---:|---:|---:|
| M-single | 6,001 | 6,001 | 0 | 0 | 3.11 | 8.28 | 19.59 |
| M-mixed-hot | 6,001 | 6,001 | 0 | 0 | 3.11 | 6.62 | 18.61 |
| M-mixed-distributed | 6,001 | 6,001 | 0 | 0 | 3.16 | 6.94 | 16.81 |
| M-stress-100 | 3,000 | 3,000 | 0 | 0 | 3.23 | 5.36 | 15.01 |
| M-stress-300 | 9,001 | 8,989 | 0 | 12 | 1.99 | 6.09 | 24.81 |
| M-stress-600 | 18,001 | 17,873 | 0 | 128 | 1.74 | 13.16 | 58.42 |
| M-stress-1000 | 30,000 | 29,978 | 0 | 22 | 2.17 | 9.67 | 27.87 |
| M-recovery | 6,001 | 6,001 | 0 | 0 | 3.42 | 8.02 | 21.50 |

M single/mixed/recovery phases last 60 seconds each; each stress step lasts 30 seconds. Mixed traffic is 95% checks and 5% writes. Each write first reads the revision, so actual HTTP request count exceeds the business-iteration count; both are in the manifest. Hot writes use one tenant; distributed writes rotate across ten tenants. Revision conflicts are counted separately; the client does not blindly retry 409 responses. M profiles describe observed capacity and recovery, not a promised SLO. Since the generator shares the application host, dropped iterations do not establish a server-only capacity limit. The stress rates are not independent repeated capacity certifications. The runner additionally evaluated recovery against the single-check thresholds; that extra result remains visible in the manifest.

## Internal cost and resources

The following means are derived from before/after application counter deltas across the three measured S runs. They are instrumentation intervals, not separate end-to-end percentiles. SQL is the sum of the three data queries per request and excludes setup/commit; pool time is acquisition only, including failed attempts amortized over successful requests. Serialization, transaction setup, network and other overhead remain outside those columns.

| Profile | Data SELECTs / successful request | Mean pool wait ms | Mean data SQL ms / request | Mean core ms / request |
|---|---:|---:|---:|---:|
| single | 3.00 | 0.053 | 2.273 | 0.011 |
| batch | 3.00 | 0.200 | 16.460 | 0.480 |

- S server: user CPU 59.69s, system CPU 35.63s, peak RSS 10.25 MiB over the entire respective process lifetime.
- M server: user CPU 14.39s, system CPU 16.29s, peak RSS 10.23 MiB over the entire respective process lifetime.

CPU and RSS come from the parent's wait4 result for its own child, not host process scanning. Each run also saves five-second snapshots of application counters. These counters do not provide CPU percentiles or a full memory time series. Query count and EXPLAIN evidence are in [M2](M2-report.md).

## Separate batch comparison

[Comparison evidence](batch-comparison.json): same S dataset, fixed 100 distinct targets, alternating allow/deny groups, one warmup and ten measured groups per mode. Every result was correct. This is a small closed-loop Python experiment, including client scheduling, and is separate from the arrival-rate baseline above.

| Mode | HTTP requests per 100 decisions | Median group completion ms |
|---|---:|---:|
| serial | 100 | 281.44 |
| concurrent_10 | 100 | 76.35 |
| batch | 1 | 8.42 |

The measured release binary then passed native restart, restore, credential rotation, outage recovery and active-request shutdown again. [Recovery evidence](native-recovery.json).

## Preserved failure and change

The [pre-optimization attempt](load-before-setup-optimization/manifest.json) failed single run 2: 38 unexpected responses, 16 dropped iterations and successful-response p99 88.26ms, with zero wrong decisions. The failure remains part of the evidence. ADR-012 reduces constant transaction setup round trips and skips checkout ping; the following BEGIN still detects stale connections. All consistency, rollback, COMMIT acknowledgement-loss and outage recovery tests passed after the change. Thresholds and workload were not reduced.

## Security and release limits

[Dependency and license inventory](security-review.json) uses RustSec revision `117edb3bed98e9be112f277b7615eea3252e7c43`. Raw cargo-audit reports RUSTSEC-2023-0071 for optional locked rsa 0.9.10. Cargo's active normal/build dependency trees for server/admin on macOS arm64 and Linux arm64/x86_64 exclude that dependency; no unresolved active advisory was found. New features/targets require review again. Tracked secret patterns had zero hits; this is not proof of every possible secret's absence.

Docker image build succeeded earlier, but clean Compose deployment was not completed because of host bind-mount restrictions. The user explicitly selected native application tests against the existing test database. Container isolation/volume-restart and the original Linux resource-limit benchmark remain separate deployment gates. Native completion must not be relabeled as production or container certification.

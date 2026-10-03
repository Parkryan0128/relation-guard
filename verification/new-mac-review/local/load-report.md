# New Mac native load results

PASS, 2026-10-03 (America/Vancouver). Source `026529822eace0fe2297b1dae49f3d9a96196436`, production source dirty: `False`. Release server SHA-256 `7135f0e312727580656e522f4fef477e00e92c43c90234a7d8653a7fb993b4d8`.

Native macOS arm64 application and same-host Docker PostgreSQL 18.6; [environment](environment.json), [manifest](load/manifest.json), [execution log](load.log). Docker VM has 10 logical CPUs and 8,319,504,384 bytes RAM; these are VM resources, not measured host RAM. No per-process CPU/memory caps. Other host apps were not stopped.

Same seeded S/M datasets, arrival rates and numeric acceptance thresholds as the historical run: S has 50,000 relationships; three single-check measurements at 100 requests/s, three 100-check batch measurements at 10 requests/s. Each measurement lasts 300 seconds after a 60-second warmup. M has 1,000,000 relationships across ten tenants; mixed writes, 30-second stress steps and 60-second recovery follow.

The six normal-load S measurements returned **99,003 correct responses with zero errors or dropped iterations**. Every individual S gate passed. All warmups also had zero errors/drops. No wrong authorization decision was observed in any profile. Recovery returned 6,001/6,001 correct responses.

| Profile | Correct responses | Unexpected | Dropped | Successful p95 ms | Successful p99 ms |
|---|---:|---:|---:|---:|---:|
| S-single-1-warmup | 6001 | 0 | 0 | 3.102 | 4.682 |
| S-single-1 | 30001 | 0 | 0 | 3.112 | 4.982 |
| S-single-2-warmup | 6000 | 0 | 0 | 3.185 | 4.824 |
| S-single-2 | 30000 | 0 | 0 | 3.065 | 4.558 |
| S-single-3-warmup | 6000 | 0 | 0 | 2.466 | 3.668 |
| S-single-3 | 30001 | 0 | 0 | 2.485 | 3.707 |
| S-batch-1-warmup | 601 | 0 | 0 | 15.830 | 20.257 |
| S-batch-1 | 3000 | 0 | 0 | 15.581 | 17.350 |
| S-batch-2-warmup | 601 | 0 | 0 | 16.269 | 18.337 |
| S-batch-2 | 3001 | 0 | 0 | 15.573 | 17.823 |
| S-batch-3-warmup | 601 | 0 | 0 | 14.983 | 16.799 |
| S-batch-3 | 3000 | 0 | 0 | 15.173 | 17.486 |
| M-single | 6001 | 0 | 0 | 2.665 | 3.931 |
| M-mixed-hot | 6001 | 0 | 0 | 3.924 | 6.163 |
| M-mixed-distributed | 6001 | 0 | 0 | 3.552 | 5.343 |
| M-stress-100 | 3000 | 0 | 0 | 2.666 | 4.063 |
| M-stress-300 | 9001 | 0 | 0 | 2.115 | 3.067 |
| M-stress-600 | 18001 | 0 | 0 | 1.836 | 3.442 |
| M-stress-1000 | 30001 | 0 | 0 | 1.695 | 2.794 |
| M-recovery | 6001 | 0 | 0 | 2.402 | 2.857 |

The 1,000 requests/s stress step succeeded for 30 seconds; it is not a production throughput guarantee or a long-duration maximum-capacity measurement. This run supports the new local setup's stability under these conditions, not a causal diagnosis of the old laptop's stalls. Previous failures are preserved in [historical evidence](../../pool-burst/report.md).

The harness removed its disposable load database on completion. Private caller keys/context remain outside tracked evidence. Compose secret-file mounting is a separate, still-blocked deployment check and is not certified by these native application results.

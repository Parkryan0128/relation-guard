# Verification status

Implementation checkpoint, 2026-10-02 UTC. Full implemented regression suite passed on commit `3a2c49dab8e5736ca6f4ed20a3be4becd69f899d`; see [sanitized log](regression.log). The evidence-only follow-up commit changes no executable code. This is not a production release or a completed M0–M5 certification.

| Stage | Evidence | Gate status |
|---|---|---|
| M0 | [Report](M0-report.md): fmt, Clippy, build, contracts, real PostgreSQL constraints | Local checks passed |
| M1 | [Report](M1-report.md): 12 scenarios, 10,000 generated cases, proof/budget boundaries, Criterion run | Functional checks passed; benchmark hardware metadata incomplete |
| M2 | [Report](M2-report.md): six PostgreSQL integration tests, including two 100-round concurrency schedules | Partial; query-plan/count evidence and full boundary schedules remain |
| M3 | [Report](M3-report.md): 32 TCP/schema assertions, three TCP fault tests, Python client | Partial; controlled COMMIT acknowledgement loss and full credential/fault matrix remain |
| M4 | [Report](M4-report.md): Docker/Compose files and external client | Blocked for container verification; backup/restore/rotation unverified |
| M5 | [Report](M5-report.md): release acceptance and performance | Not run; earlier gates incomplete |

The attempted image build could not write Docker Buildx state under `~/.docker/buildx`. A filesystem permission request returned no grant. No image-build success, container restart/restore success, or HTTP latency target is claimed.

Run `scripts/verify.sh` for the implemented regression suite. Stage gates are intentionally stricter than that suite. Test databases are disposable and contain generated data only. Logs recorded here exclude credentials and production data.

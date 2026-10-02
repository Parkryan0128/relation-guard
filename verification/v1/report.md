# Managed authorization V1 verification

Status: functional, native recovery and Compose deployment checks passed; native S/M measurement is complete, with native acceptance passed. Warmup/stress observations and deployment limits are disclosed below.

## Tested implementation

Production Rust/model/migration source: `cdfcd7ef8598a65d5367a9fd3485309c7f14c85c`. Native server SHA-256: `abbe9e86802304b7f4c315314c1e212bbb259b56b05f9a02ccf651385def2cc5`. The subsequent `91b1c21` commit adds the tested Compose restart policy, restore-readiness harness correction and recovery evidence, without changing the Rust binary. The [clean Linux functional/recovery CI](https://github.com/Parkryan0128/relation-guard/actions/runs/37062375627) passed. The later [combined functional and container CI](https://github.com/Parkryan0128/relation-guard/actions/runs/37064993225) also passed, including the updated manager/editor Python scenario.

## Behavior and tests

| Behavior | Evidence |
|---|---|
| Direct/team/inherited manager grants; manager implies edit/view; editor cannot manage | Core `managed_permissions_matrix_and_inheritance`; independent reference comparison over 10,000 generated graphs |
| Organization admin applies only to existing resources and does not suppress graph corruption | Core `admin_does_not_allow_missing_resource_or_corrupt_graph` |
| Strict role payloads and duplicate operations | Core `role_operations_are_strict_and_canonical` |
| Members cannot promote themselves, modify teams or add grants; mixed unauthorized batches roll back completely | Store `managed_happy_paths_and_no_privilege_escalation`; HTTP actor cases |
| Child creation under an existing managed folder, two-sided move checks, protected deletion | Store `managed_resource_creation_moves_deletes_and_team_grants` |
| Last-admin protection, explicit bootstrap/recovery, actor-bound receipts and replay after demotion | Store `managed_admin_lifecycle_receipts_and_mixed_rollback` |
| Revocation before a change denies it; a change before revocation commits first; concurrent admin demotions leave one admin | Store `managed_revocation_serializes_with_changes_and_last_admin`; PostgreSQL blocker observations determine ordering |
| Admin transfer, deletion/recreation without restoring admin, same-key concurrent replay and changed-payload conflict | Store `managed_role_transfer_removal_and_recreation_do_not_restore_admin` |
| Migration preserves old users/receipts/audit without automatically promoting anyone | Store `managed_upgrade_preserves_legacy_data_without_implicit_admins` |
| Actor missing/duplicate/invalid/unregistered, scope isolation, protected relationship/audit reads, no actor-less HTTP writes | 290 real HTTP assertions with OpenAPI response validation |
| Existing hierarchy bounds, snapshots, transaction rollback, unknown COMMIT outcome and fail-closed client behavior | Complete existing regression suite remains enabled |

[Local regression log](regression.log). Independent test cases run serially; explicit concurrency inside each test is retained. The server's 250ms pool acquisition, 500ms lock, 1s SQL and 2s request deadlines are unchanged.

## Recovery and deployment

[Native recovery report](native-recovery.json) and [log](native-recovery.log): the restored database retains resource-manager grants and last-admin protection, in addition to restart persistence, receipt replay, key rotation, runtime DB privilege separation, outage/readiness behavior and active-request SIGTERM draining.

[Controlled pool-stall report](pool-transport-stall.json): the prepared pool still handles the bounded injected transport delay and recovers with actor checks enabled.

[Compose report](deployment-result.json) and [log](deployment.log): a fresh non-root/read-only application container, private DB/operations ports, separate Python client container, simultaneous DB/server restart, backup/restore to a separate DB, credential rotation, readiness/outage recovery and shutdown all passed. Initial failures and their configuration/harness fixes are retained in [deployment findings](deployment-findings.md). Disposable test containers and volumes were removed afterward. This is a development Compose verification on Docker Desktop, not HA or a resource-limited production certification.

## Trust and integration boundaries

The authenticated backend asserts the logged-in user's identity through `X-Actor-Id`; the engine checks membership and permissions. The backend must derive both tenant context and actor from its authenticated session, strip untrusted incoming actor headers and keep service keys private. This is not an end-user login server or protection against a compromised identity-asserting backend.

Permission changes are checked and committed inside one authorization transaction. A check does not make a later write in another application's database atomic. Entity deletion remains explicit: existing relationships must be removed before deleting a referenced entity; there is no unbounded cascade or bulk-offboarding endpoint. Only an organization administrator creates root resources. Custom roles, explicit deny, permission expiry, approval workflows, nested teams and arbitrary resource models are excluded.

## Native performance

Native acceptance result: **PASS** under the approved macOS/native protocol, not a resource-limited Linux performance certification. Source `91b1c2106cd10f660b1abd1d2b874b309b7c27cb`, source_dirty `False`, server hash `abbe9e86802304b7f4c315314c1e212bbb259b56b05f9a02ccf651385def2cc5`. Each S profile has a fixed 60-second warmup and three separate 300-second measurements; thresholds were not relaxed. [Full manifest](load/manifest.json).

| Profile | Correct | Unexpected | Generator dropped | Successful p95 ms | Successful p99 ms | Numeric gate |
|---|---:|---:|---:|---:|---:|---|
| S-single-1 | 30000 | 0 | 0 | 5.31 | 14.60 | True |
| S-single-2 | 30000 | 0 | 0 | 5.25 | 12.30 | True |
| S-single-3 | 30001 | 0 | 0 | 5.29 | 13.56 | True |
| S-batch-1 | 3001 | 0 | 0 | 29.57 | 38.84 | True |
| S-batch-2 | 3001 | 0 | 0 | 31.15 | 43.50 | True |
| S-batch-3 | 3000 | 0 | 0 | 29.38 | 40.15 | True |
| M-single | 6000 | 0 | 0 | 17.98 | 74.30 | exploratory |
| M-mixed-hot | 6001 | 0 | 0 | 7.57 | 15.78 | exploratory |
| M-mixed-distributed | 6000 | 0 | 0 | 7.92 | 16.84 | exploratory |
| M-stress-100 | 3000 | 0 | 0 | 6.10 | 12.93 | exploratory |
| M-stress-300 | 9000 | 0 | 0 | 4.90 | 16.14 | exploratory |
| M-stress-600 | 17999 | 0 | 1 | 3.88 | 10.04 | exploratory |
| M-stress-1000 | 29735 | 0 | 266 | 32.88 | 140.61 | exploratory |
| M-recovery | 6000 | 0 | 0 | 7.12 | 27.00 | True |

The six S measurement intervals returned 99,003 correct responses, 0 unexpected responses and 0 dropped iterations. No incorrect authorization decision was observed in any profile.

**Warmup observations are retained:** 4 unexpected responses and 8 dropped generator iterations occurred across the fixed warmup intervals. In S-single-3-warmup, four responses were 503 OVERLOADED at approximately 255–266ms; acquisition-timeout counters increased by four, BEGIN/error counters stayed zero, and all logged pool states were size=10/idle=0. The generator also dropped eight iterations. The failure path is confirmed as acquisition timeout with a fully occupied pool; the external source of that short stall is not proven. Prepared connections do not promise immunity to actual pool exhaustion. Warmups are outside the predefined numeric acceptance intervals, but these observations must not be summarized as an entirely error-free test session. [Server error events](load/server-errors.json), [generator error records](load/request-failures.json).

M mixed/stress profiles are exploratory; conflicts and dropped iterations must be read separately from successful-response percentiles. Mixed writes use an organization-admin actor and do not establish a separate throughput guarantee for worst-case manager-authorized 100-operation changes. M recovery has its own fixed normal-load gate. The dataset includes one management user per tenant beyond the generated benchmark users. The old v0.1 failure artifacts remain unchanged.

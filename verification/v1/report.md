# Managed authorization V1 verification

Status: functional, native recovery and Compose deployment checks passed; the full native S/M load suite is running. No final performance acceptance is claimed yet.

## Tested implementation

Production Rust/model/migration source: `cdfcd7ef8598a65d5367a9fd3485309c7f14c85c`. Native server SHA-256: `abbe9e86802304b7f4c315314c1e212bbb259b56b05f9a02ccf651385def2cc5`. The subsequent `91b1c21` commit adds the tested Compose restart policy, restore-readiness harness correction and recovery evidence, without changing the Rust binary. The [clean Linux functional/recovery CI](https://github.com/Parkryan0128/relation-guard/actions/runs/37062375627) passed.

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

Permission changes are checked and committed inside one authorization transaction. A check does not make a later write in another application's database atomic. Custom roles, explicit deny, permission expiry, approval workflows, nested teams and arbitrary resource models are excluded.

## Native performance

Pending. The new suite uses its own manifest in `verification/v1/load/manifest.json` (published after completion) and preserves the historical v0.1 failure artifacts separately. Successful functional tests do not substitute for the numeric performance gates.

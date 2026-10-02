# M2 partial verification

Full implemented regression rerun: `3a2c49dab8e5736ca6f4ed20a3be4becd69f899d`, 2026-10-02 UTC. See [log](regression.log).

Command: `cargo test -p authz-store --test transactions` against isolated PostgreSQL 18. Six tests passed, no ignored tests.

- Atomic writes, rollback, cross-tenant deny, revoke/replay without resurrecting a grant, hash mismatch, no-op receipt and stale revision.
- Snapshot acquired before revoke continues to see the coherent old graph; a subsequent check denies. Same-revision writers use a barrier for 100 rounds; exactly one commits. Same-key concurrent writes replay the same receipt.
- Depth 32 accepted, depth 33/cycle/multiple-parent/self-parent rejected; revision overflow rolls back.
- 100 duplicate batch items preserve results; 10,001 matching grant facts return EVALUATION_LIMIT.
- Receipt cleanup deletes at most 1,000 expired rows per call, preserves recent receipts and graph/audit revision; deleting/recreating a user does not restore old grants; a different credential key does not replay another key's receipt.
- Competing A→B and B→A writes run 100 rounds with a barrier. Stale revisions conflict, retrying the inverse at the current revision detects the cycle. A valid final-state move succeeds; concurrent no-ops both succeed.

M2-01–M2-08 have coverage, but the gate remains **incomplete**: measured query-count/EXPLAIN artifacts, exact 24-hour boundary, descendant-depth move schedules and all snapshot/batch interleavings are not yet recorded. The loader contains three data SELECTs (revision, ancestors, grants); that code inspection is not presented as a measured query-count result.

Real PostgreSQL testing found and fixed C-collation mismatch in a recursive CTE and PostgreSQL 18 RESTRICT violation SQLSTATE 23001 mapping.

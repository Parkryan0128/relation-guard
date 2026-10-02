# M2 verification — PASS

Tested production source: `afc0fe192681eba11e6867f6bf85413baa4598fb`. [Full regression](regression.log), [measured query plans](query-plans.json).

Commands: `cargo test -p authz-store --locked --test transactions` (eight tests) and `cargo test -p authz-store --locked --test query_plans` (one test), against PostgreSQL 18.6 with pg_stat_statements enabled. M2-01–M2-08 passed; no ignored tests.

- Atomic graph/revision/audit/receipt updates and rollback; tenant isolation, revocation, idempotent replay, mismatched payload rejection, no-op receipts and revision overflow.
- Barrier-controlled same-revision writers, same-key writers, competing inverse parent edges, and batch snapshot interleavings run 100 rounds per schedule. A batch keeps one coherent snapshot across concurrent revocation; a later snapshot observes the revocation.
- Depth 32/33, self/cycle/multiple-parent edges, and subtree moves whose descendants would exceed the limit are checked. Final-state moves are validated atomically.
- Receipt retention checks before/exactly/after the 24-hour boundary using one transaction timestamp. Cleanup removes at most 1,000 of 1,001 expired receipts, leaves recent receipts and audit intact, and does not change revision. Recreated entities and rotated credentials do not resurrect access or replay another key's receipt.
- 100 duplicate checks preserve ordering; 10,001 matching grants return EVALUATION_LIMIT without a partial decision.
- pg_stat_statements measures exactly three data SELECTs for both one target and 100 distinct targets sharing ancestors. EXPLAIN ANALYZE BUFFERS artifacts accompany the count. Transaction setup/commit statements are not data SELECTs.

Measured load prompted a setup optimization: BEGIN, isolation and local timeouts now share one constant protocol request, and checkout ping is disabled. Snapshot semantics, timeout limits and fail-closed connection errors remain unchanged; the full suite and native outage recovery were rerun after this change. See ADR-012.

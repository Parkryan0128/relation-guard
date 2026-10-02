# M0 verification — PASS

Tested production source: `afc0fe192681eba11e6867f6bf85413baa4598fb`, 2026-10-02 UTC. Tests ran immediately before the evidence commit, against the same source bytes. [Full regression log](regression.log).

Environment: Rust 1.96.1 (edition 2024), macOS arm64, PostgreSQL 18.6, Python 3.13 with jsonschema 4.26.0. Dependencies are pinned in Cargo.lock. The application and test processes ran natively; PostgreSQL was the existing dedicated test endpoint.

Command: `PYTHON=<python-with-jsonschema> TEST_DATABASE_URL=<isolated-test-db> scripts/verify.sh`.

M0-01–M0-05 passed: workspace format, Clippy with warnings denied, build, typed and OpenAPI contracts, canonical hash golden fixture, migration replay, and real PostgreSQL CHECK/FK/unique constraints. Cross-row cycle and depth constraints are enforced by the service transaction, not by SQL CHECK constraints alone. No tests were ignored.

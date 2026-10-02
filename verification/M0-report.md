# M0 local verification

Full implemented regression rerun: `3a2c49dab8e5736ca6f4ed20a3be4becd69f899d`, 2026-10-02 UTC. See [log](regression.log).

Tested core/contracts checkpoint: `af1c3b644e05c79c096a708e68cc4977f397f857`; later full-workspace regression is recorded in status and the regression log.

- Rust 1.96.1, edition 2024, macOS aarch64; local installed toolchain alias `stable` resolves to the pinned version.
- PostgreSQL 18 Alpine image: `sha256:77f585114c32fbca283dc835b0596f4e52b51b4c6662d7810b2f4084f60a1873`.
- Python 3.13, jsonschema 4.26.0.
- Command: `scripts/test-stage.sh M0` with isolated TEST_DATABASE_URL and the configured Python interpreter.

M0-01 through M0-05: local PASS. Workspace fmt/Clippy/build, strict typed contracts, JSON Schema fixture validation, canonical hash golden fixture, migration replay and PostgreSQL constraint rejection were executed. PostgreSQL tests use generated schemas. The migration permits cross-row cycles at SQL level; service transaction validation enforces graph invariants.

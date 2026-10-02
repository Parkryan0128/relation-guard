# M3 verification — PASS

Tested production source: `afc0fe192681eba11e6867f6bf85413baa4598fb`. [Full regression log](regression.log).

Commands: `cargo test -p authz-server --locked`, `scripts/test_http.py`, `scripts/test_commit_faults.py`, and `scripts/test_client.py`, with actual TCP sockets and PostgreSQL 18.6. Six Rust server tests, 261 HTTP/schema assertions and three Python client tests passed. M3-01–M3-07 passed; no ignored tests.

- All 31 nonempty scope combinations are exercised across seven routes, with tenant isolation and explain restrictions. Invalid startup credentials/scopes, unknown or duplicate JSON fields, invalid UTF-8, JSON depth, and exact body-size boundaries are covered.
- Pagination, cursor binding, request IDs, mutation/replay responses, error envelopes and OpenAPI schemas are validated.
- Ten occupied pool connections, a held tenant row lock, a real PostgreSQL statement timeout, 128 occupied request permits and request deadlines produce the specified errors and recover. Failed batch evaluation never returns partial allow results.
- A test-only barrier closes an HTTP request before COMMIT and verifies rollback and released permits. The hook is absent from production builds.
- A PostgreSQL wire proxy disconnects before forwarding COMMIT (actual rollback) and after the database commits but before its acknowledgement reaches the server (actual durable commit). Both uncertain responses are explicit errors. Same-key retries commit or replay exactly once, without duplicate audit/revision changes.
- The external Python client grants, checks, revokes and blocks the protected operation. Separate client tests cover bounded same-key retries after transport/unknown outcomes, no blind conflict retry, and fail-closed malformed responses.

Database connectivity, schema readiness, credential rotation and process recovery also passed in [M4-native](M4-report.md). No raw credentials or request payloads are stored in these logs.

# M3 partial verification

Commands: `cargo test -p authz-server` and `python scripts/test_http.py`, with real TCP listeners and PostgreSQL 18.

- 32 HTTP assertions validate response schemas, authorization and scope separation, strict/duplicate/unknown JSON, invalid UTF-8, oversized bodies, media/method/path errors, revision conflicts, revoke/replay request IDs, audit pagination and proof omission.
- TCP fault tests fill all ten pool slots, hold a tenant row lock, and verify OVERLOADED followed by recovery.
- 128 open request bodies hold the request permits; the 129th request returns 503 without joining a queue. The body deadline returns 504 and permits recover.
- A single check allows, but a 100-item batch exceeding the work budget returns one error with no partial result.
- The standard-library Python example independently completes grant→allow→revoke→deny.

M3-01–M3-07 are **partially covered**, not certified. Missing evidence includes controlled pre-COMMIT disconnect/acknowledgement-loss schedules, full startup/credential/scope combinations, exact transport/depth boundaries, SQL statement timeout and database-stop recovery. The implementation conservatively returns WRITE_OUTCOME_UNKNOWN after an uncertain COMMIT; that branch still needs deterministic fault evidence.

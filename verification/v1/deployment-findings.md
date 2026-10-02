# V1 container verification findings

The first attempt could not bind-mount the example client from macOS Documents. The test harness now copies the example into its private temporary run directory, alongside the already accessible test credentials. No host permission or secret access policy was changed.

The next attempt reached the client and mutation tests, but simultaneous `docker compose restart db server` left the server exited with `StoreError { code: "OVERLOADED" }` while PostgreSQL subsequently became healthy. The Compose server had no restart policy, so an initial database connection failure was terminal.

Compose now applies `restart: unless-stopped` to the database and server. This allows recovery from transient startup failures while respecting an explicit stop. Request deadlines and pool-acquisition limits are unchanged. The complete restart/restore/rotation sequence is rerun against a fresh test project after this configuration change.

After fixing restart recovery, the restore harness revealed a readiness race: `pg_isready` over the local socket can succeed against the image entrypoint’s temporary initialization server before the target database exists. The harness now waits for a successful `SELECT 1` against the target database over TCP, which is available only after initialization. No readiness check is replaced with an unconditional sleep.

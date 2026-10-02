# M4 blocked container verification

Dockerfile, Compose, local credential bootstrap, admin CLI and Python client are present. The Python client passed against native binaries and PostgreSQL.

The attempted `docker build -t relation-guard:local .` failed before building: Docker Buildx could not update its activity file under the host's `~/.docker/buildx` directory. A filesystem permission request did not grant access. No alternative path was used to bypass that restriction.

M4-01, M4-03–M4-06: not verified. M4-02: happy-path external client verified; the full retry/failure contract remains partial. Image build, non-root runtime, private ports, volume restart, backup/restore, key rotation and container SIGTERM acceptance must be executed before marking this stage complete. The Docker base image tag has not been resolved/pinned by a successful build.

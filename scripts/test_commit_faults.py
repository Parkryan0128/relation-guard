#!/usr/bin/env python3
"""Observe rollback or durable receipt after an actual lost PostgreSQL COMMIT ack."""

import json
import os
import pathlib
import socket
import subprocess
import tempfile
import time
import urllib.error
import urllib.parse
import urllib.request
import uuid

from pg_fault_proxy import Proxy

root = pathlib.Path(__file__).resolve().parents[1]
binary = root / "target/debug"
url = os.environ["TEST_DATABASE_URL"]
env = {**os.environ, "DATABASE_URL": url}


def admin(*args):
    return subprocess.check_output(
        [str(binary / "authz-admin"), *args], env=env, text=True
    )


def port():
    with socket.socket() as s:
        s.bind(("127.0.0.1", 0))
        return s.getsockname()[1]


admin("migrate")
tenant = str(uuid.uuid4())
admin("tenant", tenant)
admin("bootstrap-admin", tenant, "test-admin")
credential = json.loads(admin("key", tenant, "fault"))
parsed = urllib.parse.urlsplit(url)
proxy = Proxy(parsed.hostname, parsed.port or 5432)
netloc = parsed.netloc.rsplit("@", 1)[0] + "@127.0.0.1:" + str(proxy.port)
query = urllib.parse.parse_qs(parsed.query)
query["sslmode"] = ["disable"]
proxy_url = urllib.parse.urlunsplit(
    (parsed.scheme, netloc, parsed.path, urllib.parse.urlencode(query, doseq=True), "")
)
with tempfile.TemporaryDirectory() as td:
    keyfile = pathlib.Path(td) / "keys.json"
    keyfile.write_text(json.dumps({"version": 1, "keys": [credential["config_entry"]]}))
    keyfile.chmod(0o600)
    data, ops = port(), port()
    base = f"http://127.0.0.1:{data}/v1/tenants/{tenant}"
    with open(pathlib.Path(td) / "server.log", "w") as log:
        process = subprocess.Popen(
            [str(binary / "authz-server")],
            env={
                **env,
                "DATABASE_URL": proxy_url,
                "AUTHZ_KEYS_FILE": str(keyfile),
                "AUTHZ_BIND": f"127.0.0.1:{data}",
                "AUTHZ_OPS_BIND": f"127.0.0.1:{ops}",
            },
            stdout=log,
            stderr=log,
        )

        def call(route, body=None, key="same"):
            request = urllib.request.Request(
                base + "/" + route,
                data=None if body is None else json.dumps(body).encode(),
                headers={
                    "X-Actor-Id": "test-admin",
                    "Authorization": "Bearer " + credential["token"],
                    "Content-Type": "application/json",
                    "Idempotency-Key": key,
                },
            )
            try:
                r = urllib.request.urlopen(request, timeout=4)
            except urllib.error.HTTPError as e:
                r = e
            return r.status, json.load(r), r.headers

        try:
            for _ in range(100):
                try:
                    with urllib.request.urlopen(
                        f"http://127.0.0.1:{ops}/readyz", timeout=0.2
                    ):
                        break
                except OSError:
                    time.sleep(0.05)
            else:
                raise AssertionError("startup timeout")
            request = {
                "expected_revision": "1",
                "operations": [
                    {"op": "put_entity", "entity": {"kind": "user", "id": "first"}}
                ],
            }
            proxy.arm("before")
            status, response, _ = call("write", request, "before")
            assert proxy.hit.wait(2)
            assert (
                status == 503 and response["error"]["code"] == "WRITE_OUTCOME_UNKNOWN"
            )
            assert call("revision")[1]["revision"] == "1"
            status, response, headers = call("write", request, "before")
            assert (
                status == 200
                and response["revision"] == "2"
                and headers["Idempotency-Replayed"] == "false"
            )
            request = {
                "expected_revision": "2",
                "operations": [
                    {"op": "put_entity", "entity": {"kind": "user", "id": "second"}}
                ],
            }
            proxy.arm("after")
            status, response, _ = call("write", request, "after")
            assert proxy.hit.wait(2)
            assert (
                status == 503 and response["error"]["code"] == "WRITE_OUTCOME_UNKNOWN"
            )
            assert call("revision")[1]["revision"] == "3"
            status, response, headers = call("write", request, "after")
            assert (
                status == 200
                and response["revision"] == "3"
                and headers["Idempotency-Replayed"] == "true"
            )
            assert call("revision")[1]["revision"] == "3"
            print(
                "PASS: DB disconnect before COMMIT forwarding rolled back; lost durable COMMIT acknowledgement returned unknown and replayed exactly one receipt."
            )
        finally:
            process.terminate()
            process.wait(timeout=12)
            proxy.close()

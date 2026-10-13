#!/usr/bin/env python3
"""Controlled mechanism experiment; not proof of the historical failure's trigger."""

import concurrent.futures
import hashlib
import json
import os
import pathlib
import subprocess
import threading
import time
import urllib.parse

from native_lab import ROOT, Lab
from pg_fault_proxy import GateProxy
from test_output import output_path


def experiment():
    lab = Lab()
    proxy = None
    try:
        lab.initialize()
        lab.stop_server()
        parsed = urllib.parse.urlsplit(lab.url)
        proxy = GateProxy(parsed.hostname, parsed.port or 5432)
        lab.url = urllib.parse.urlunsplit(
            parsed._replace(
                netloc="relation_guard_runtime@127.0.0.1:" + str(proxy.port),
                query="sslmode=disable",
            )
        )
        lab.start_server()
        # Wait for release hygiene before measuring the available pool.
        deadline = time.monotonic() + 2
        while True:
            before = lab.metrics()
            if before["relation_guard_pool_idle"] == before["relation_guard_pool_size"]:
                break
            assert time.monotonic() < deadline
            time.sleep(0.005)
        proxy.pause()

        def release_after_first_response():
            assert proxy.blocked.wait(2)
            # Intentional transport latency, not sleep-based transaction ordering.
            time.sleep(0.75)
            proxy.resume()

        release = threading.Thread(target=release_after_first_response)
        release.start()

        def request():
            started = time.monotonic()
            status, body, _ = lab.request("revision")
            return {
                "status": status,
                "code": body.get("error", {}).get("code"),
                "milliseconds": (time.monotonic() - started) * 1000,
            }

        with concurrent.futures.ThreadPoolExecutor(max_workers=12) as workers:
            start = time.monotonic()
            futures = []
            for i in range(12):
                time.sleep(max(0, start + i * 0.1 - time.monotonic()))
                futures.append(workers.submit(request))
            responses = [future.result() for future in futures]
        release.join()
        after = lab.metrics()
        assert lab.request("revision")[0] == 200
        return {
            "initial_pool_size": int(before["relation_guard_pool_size"]),
            "initial_pool_idle": int(before["relation_guard_pool_idle"]),
            "response_delay_ms": 750,
            "offered_requests_per_second": 10,
            "responses": responses,
            "acquire_timeouts": int(
                after["relation_guard_pool_acquire_timeouts_total"]
                - before["relation_guard_pool_acquire_timeouts_total"]
            ),
            "begin_errors": int(
                after["relation_guard_transaction_begin_errors_total"]
                - before["relation_guard_transaction_begin_errors_total"]
            ),
            "recovered": True,
        }
    finally:
        if proxy:
            proxy.resume()
        lab.close()
        if proxy:
            proxy.close()


if __name__ == "__main__":
    reports = [experiment()]
    binary = (
        pathlib.Path(os.environ.get("AUTHZ_BIN", str(ROOT / "target/debug")))
        / "authz-server"
    )
    result = {
        "source_commit": subprocess.check_output(
            ["git", "rev-parse", "HEAD"], cwd=ROOT, text=True
        ).strip(),
        "binary_sha256": hashlib.sha256(binary.read_bytes()).hexdigest(),
        "purpose": "Controlled DB transport-delay mechanism experiment; does not establish what caused the original network/pool stall.",
        "runs": reports,
    }
    output_path("POOL_STALL_OUTPUT", "pool-stall.json").write_text(
        json.dumps(result, indent=2)
    )
    report = reports[0]
    assert report["initial_pool_size"] == 10, report
    assert report["acquire_timeouts"] == 0 and all(
        r["status"] == 200 for r in report["responses"]
    ), report
    print(
        "PASS: prepared service pool withstands the bounded transport stall and recovers."
    )

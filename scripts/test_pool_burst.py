#!/usr/bin/env python3
"""Bounded transport-stall reproduction at the normal 100 requests/s arrival rate."""
from test_output import output_path
import concurrent.futures
import hashlib
import json
import os
import pathlib
import subprocess
import threading
import time
import urllib.parse

from native_lab import Lab, ROOT
from pg_fault_proxy import GateProxy


def experiment(delay):
    lab = Lab()
    proxy = None
    try:
        lab.initialize()
        # Exercise the full evaluator with known allow AND deny results.
        entities = [{"kind": "user", "id": "member"}, {"kind": "document", "id": "doc"}]
        operations = [{"op": "put_entity", "entity": e} for e in entities]
        operations.append({"op": "put_relation", "relationship": {
            "resource": entities[1], "relation": "viewer", "subject": entities[0], "subject_relation": ""}})
        status, body, _ = lab.request("write", {"expected_revision": "1", "operations": operations})
        assert status == 200, (status, body)
        lab.stop_server()
        parsed = urllib.parse.urlsplit(lab.url)
        proxy = GateProxy(parsed.hostname, parsed.port or 5432)
        lab.url = urllib.parse.urlunsplit(parsed._replace(
            netloc=f"relation_guard_runtime@127.0.0.1:{proxy.port}", query="sslmode=disable"))
        lab.start_server()
        before = lab.metrics()
        assert before['relation_guard_pool_size'] == 10
        proxy.pause()
        released = threading.Event()

        def release():
            try:
                assert proxy.blocked.wait(2)
                # Intentional fault duration, not a transaction ordering shortcut.
                time.sleep(delay)
            finally:
                proxy.resume()
                released.set()

        thread = threading.Thread(target=release)
        thread.start()

        def request(i):
            expected = "allow" if i % 2 == 0 else "deny"
            started = time.monotonic()
            status, body, _ = lab.request("check", {
                "subject": entities[0], "resource": entities[1],
                "action": "view" if expected == "allow" else "edit"}, actor="member")
            if status == 200:
                assert body["decision"] == expected, body
            else:
                assert status == 503 and body["error"]["code"] == "OVERLOADED", (status, body)
                assert "decision" not in body
            return {"status": status, "milliseconds": round((time.monotonic()-started)*1000, 3)}

        with concurrent.futures.ThreadPoolExecutor(max_workers=100) as workers:
            start = time.monotonic()
            futures = []
            for i in range(100):
                time.sleep(max(0, start+i*.01-time.monotonic()))
                futures.append(workers.submit(request, i))
            responses = [f.result() for f in futures]
        thread.join(timeout=3)
        assert released.is_set()
        after = lab.metrics()
        assert lab.request("revision")[0] == 200
        return {"delay_ms": delay*1000, "offered_requests_per_second": 100,
                "correct": sum(r["status"] == 200 for r in responses),
                "overloaded": sum(r["status"] == 503 for r in responses),
                "acquire_timeouts": int(after['relation_guard_pool_acquire_timeouts_total']-before['relation_guard_pool_acquire_timeouts_total']),
                "begin_errors": int(after['relation_guard_transaction_begin_errors_total']-before['relation_guard_transaction_begin_errors_total']),
                "responses": responses, "recovered": True}
    finally:
        if proxy:
            proxy.resume()
        lab.close()
        if proxy:
            proxy.close()


if __name__ == '__main__':
    runs = [experiment(.4), experiment(.9)]
    binary = pathlib.Path(os.environ.get('AUTHZ_BIN', str(ROOT/'target/debug')))/'authz-server'
    report = {"source_commit": subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=ROOT, text=True).strip(),
              "source_dirty": bool(subprocess.check_output(['git', 'status', '--porcelain'], cwd=ROOT)),
              "binary_sha256": hashlib.sha256(binary.read_bytes()).hexdigest(),
              "purpose": "Reproduces pool exhaustion; does not identify the historical external stall trigger.",
              "runs": runs}
    output_path('POOL_BURST_OUTPUT', 'pool-burst.json').write_text(json.dumps(report, indent=2)+'\n')
    if os.environ.get('EXPECT_SHORT_STALL_FAILURE') == '1':
        assert runs[0]['overloaded'] > 0, runs[0]
    else:
        assert runs[0]['correct'] == 100 and runs[0]['acquire_timeouts'] == 0, runs[0]
    assert runs[1]['overloaded'] > 0, runs[1]
    assert all(r['begin_errors'] == 0 and r['acquire_timeouts'] == r['overloaded'] for r in runs)
    print(json.dumps({"result": "PASS", "runs": [{k: v for k, v in r.items() if k != 'responses'} for r in runs]}))

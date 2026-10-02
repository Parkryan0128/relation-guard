#!/usr/bin/env python3
"""Compare 100 decisions as serial requests, 10-way requests, and one batch.

Run separately from offered-load benchmarks to avoid contaminating their results.
No latency threshold is inferred from this small explanatory experiment.
"""
import concurrent.futures
import http.client
import hashlib
import subprocess
import json
import statistics
import threading
import time
from native_lab import Lab, ROOT
from load_dataset import seed

lab = Lab()
local = threading.local()
connections = []
connections_lock = threading.Lock()


def request(path, payload, expected):
    if not hasattr(local, 'connection'):
        local.connection = http.client.HTTPConnection('127.0.0.1', lab.port, timeout=3)
        with connections_lock:
            connections.append(local.connection)
    local.connection.request('POST', f'/v1/tenants/{lab.tenant}/{path}', json.dumps(payload),
                             {'Authorization': 'Bearer ' + lab.token,
                              'Content-Type': 'application/json'})
    response = local.connection.getresponse()
    body = json.loads(response.read())
    assert response.status == 200, response.status
    results = body['results'] if path == 'batch-check' else [body]
    assert len(results) == (100 if path == 'batch-check' else 1)
    assert all(item['decision'] == expected for item in results)


try:
    lab.initialize()
    shape = seed(lab, lab.tenant, 10000, 50000)
    checks = [{'resource': {'kind': 'document', 'id': f'd{j * 50}'},
               'action': 'view'} for j in range(100)]
    samples = {name: [] for name in ('serial', 'concurrent_10', 'batch')}
    with concurrent.futures.ThreadPoolExecutor(max_workers=10) as executor:
        # Rotate order, and exclude one warmup group per mode.
        for repetition in range(11):
            expected = 'allow' if repetition % 2 == 0 else 'deny'
            subject = {'kind': 'user', 'id': 'u0' if expected == 'allow' else 'u999'}
            modes = list(samples)
            modes = modes[repetition % 3:] + modes[:repetition % 3]
            for mode in modes:
                started = time.perf_counter()
                if mode == 'batch':
                    request('batch-check', {'subject': subject, 'checks': checks}, expected)
                elif mode == 'serial':
                    for check in checks:
                        request('check', {'subject': subject, **check}, expected)
                else:
                    futures = [executor.submit(request, 'check', {'subject': subject, **check}, expected)
                               for check in checks]
                    for future in futures:
                        future.result()
                elapsed = (time.perf_counter() - started) * 1000
                if repetition:
                    samples[mode].append(elapsed)
    report = {'source_commit': subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=ROOT, text=True).strip(),
              'binary_sha256': hashlib.sha256((lab.bin / 'authz-server').read_bytes()).hexdigest(),
              'environment': 'native application, existing dedicated PostgreSQL endpoint',
              'dataset': shape, 'decisions_per_group': 100, 'warmup_groups_per_mode': 1,
              'measured_groups_per_mode': 10, 'wrong_decisions': 0,
              'limitations': 'Small closed-loop experiment; includes Python client scheduling. Not an arrival-rate SLO.',
              'modes': {name: {'http_requests_per_group': 1 if name == 'batch' else 100,
                               'median_group_ms': statistics.median(values),
                               'min_group_ms': min(values), 'max_group_ms': max(values),
                               'samples_ms': values} for name, values in samples.items()}}
    (ROOT / 'verification/batch-comparison.json').write_text(json.dumps(report, indent=2))
    print('PASS: serial, 10-way concurrent and batch produced the same 100 expected decisions per group.')
finally:
    for connection in connections:
        connection.close()
    lab.close()

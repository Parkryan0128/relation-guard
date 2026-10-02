#!/usr/bin/env python3
"""Controlled mechanism experiment; not proof of the historical failure's trigger."""
import concurrent.futures,hashlib,json,os,pathlib,socket,subprocess,threading,time,urllib.parse
from native_lab import Lab,ROOT

class GateProxy:
    def __init__(self,host,port):
        self.target=(host,port)
        self.listener=socket.socket();self.listener.bind(('127.0.0.1',0));self.listener.listen()
        self.port=self.listener.getsockname()[1];self.gate=threading.Event();self.gate.set()
        self.blocked=threading.Event();self.sockets=[]
        threading.Thread(target=self.accept,daemon=True).start()
    def accept(self):
        try:
            while True:
                client,_=self.listener.accept()
                backend=socket.create_connection(self.target)
                self.sockets.extend([client,backend])
                threading.Thread(target=self.relay,args=(client,backend,False),daemon=True).start()
                threading.Thread(target=self.relay,args=(backend,client,True),daemon=True).start()
        except OSError:pass
    def relay(self,source,destination,responses):
        try:
            while data:=source.recv(65536):
                if responses and not self.gate.is_set():
                    self.blocked.set();self.gate.wait()
                destination.sendall(data)
        except OSError:pass
        finally:
            try:destination.shutdown(socket.SHUT_RDWR)
            except OSError:pass
    def pause(self):self.blocked.clear();self.gate.clear()
    def resume(self):self.gate.set()
    def close(self):
        self.resume();self.listener.close()
        for sock in self.sockets:
            try:sock.shutdown(socket.SHUT_RDWR)
            except OSError:pass
            sock.close()

def metrics(lab):
    return {key:float(value) for key,value in (line.split() for line in lab.probe('metrics')[1].splitlines())}

def experiment(prewarm):
    lab=Lab();proxy=None
    try:
        lab.initialize();lab.stop_server()
        parsed=urllib.parse.urlsplit(lab.url)
        proxy=GateProxy(parsed.hostname,parsed.port or 5432)
        lab.url=urllib.parse.urlunsplit(parsed._replace(netloc='relation_guard_runtime@127.0.0.1:'+str(proxy.port),query='sslmode=disable'))
        lab.start_server()
        if prewarm:
            # A barrier holds responses until all ten pool slots have been opened.
            proxy.pause()
            try:
                with concurrent.futures.ThreadPoolExecutor(max_workers=10) as workers:
                    futures=[workers.submit(lab.request,'revision') for _ in range(10)]
                    deadline=time.monotonic()+.2
                    while metrics(lab)['relation_guard_pool_size']<10:
                        if time.monotonic()>deadline:raise AssertionError('Prewarm barrier not reached within acquire budget')
                        time.sleep(.001)
                    proxy.resume()
                    assert all(future.result()[0]==200 for future in futures)
            finally:proxy.resume()
        # Wait for release hygiene before measuring the available pool.
        deadline=time.monotonic()+2
        while True:
            before=metrics(lab)
            if before['relation_guard_pool_idle']==before['relation_guard_pool_size']:break
            assert time.monotonic()<deadline
            time.sleep(.005)
        proxy.pause()
        def release_after_first_response():
            assert proxy.blocked.wait(2)
            # Intentional transport latency, not sleep-based transaction ordering.
            time.sleep(.75);proxy.resume()
        release=threading.Thread(target=release_after_first_response);release.start()
        def request():
            started=time.monotonic();status,body,_=lab.request('revision')
            return {'status':status,'code':body.get('error',{}).get('code'),'milliseconds':(time.monotonic()-started)*1000}
        with concurrent.futures.ThreadPoolExecutor(max_workers=12) as workers:
            start=time.monotonic();futures=[]
            for i in range(12):
                time.sleep(max(0,start+i*.1-time.monotonic()))
                futures.append(workers.submit(request))
            responses=[future.result() for future in futures]
        release.join();after=metrics(lab)
        assert lab.request('revision')[0]==200
        return {'prewarmed':prewarm,'initial_pool_size':int(before['relation_guard_pool_size']),
                'initial_pool_idle':int(before['relation_guard_pool_idle']),
                'response_delay_ms':750,'offered_requests_per_second':10,'responses':responses,
                'acquire_timeouts':int(after['relation_guard_pool_acquire_timeouts_total']-before['relation_guard_pool_acquire_timeouts_total']),
                'begin_errors':int(after['relation_guard_transaction_begin_errors_total']-before['relation_guard_transaction_begin_errors_total']),
                'recovered':True}
    finally:
        if proxy:proxy.resume()
        lab.close()
        if proxy:proxy.close()

if __name__=='__main__':
    reports=[experiment(False),experiment(True)]
    binary=pathlib.Path(os.environ.get('AUTHZ_BIN',str(ROOT/'target/debug')))/'authz-server'
    result={'source_commit':subprocess.check_output(['git','rev-parse','HEAD'],cwd=ROOT,text=True).strip(),'binary_sha256':hashlib.sha256(binary.read_bytes()).hexdigest(),'purpose':'Controlled DB transport-delay mechanism experiment; does not establish what caused the original network/pool stall.','runs':reports}
    pathlib.Path(os.environ.get('POOL_STALL_OUTPUT',str(ROOT/'verification/pool-transport-stall.json'))).write_text(json.dumps(result,indent=2))
    if os.environ.get('EXPECT_COLD_POOL_TIMEOUT')=='1':
        assert reports[0]['acquire_timeouts']>0, reports[0]
        assert all(r['status']==503 and r['code']=='OVERLOADED' for r in reports[0]['responses'] if r['status']!=200)
    else:
        assert reports[0]['initial_pool_size']==10, reports[0]
        assert reports[0]['acquire_timeouts']==0 and all(r['status']==200 for r in reports[0]['responses']),reports[0]
    assert reports[1]['acquire_timeouts']==0 and all(r['status']==200 for r in reports[1]['responses']),reports[1]
    print('PASS: controlled transport-stall expectations and subsequent recovery verified.')

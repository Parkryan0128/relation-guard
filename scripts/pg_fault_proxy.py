"""Test-only PostgreSQL wire proxy. Never imported or shipped by the Rust server."""
import socket, threading, struct

def exact(sock,n):
    chunks=[]
    while n:
        data=sock.recv(n)
        if not data:raise EOFError()
        chunks.append(data);n-=len(data)
    return b''.join(chunks)
class Proxy:
    def __init__(self,host,port):
        self.target=(host,port);self.listener=socket.socket();self.listener.bind(('127.0.0.1',0));self.listener.listen();self.port=self.listener.getsockname()[1]
        self.mode=None;self.hit=threading.Event();self.lock=threading.Lock();self.connections=[];self.thread=threading.Thread(target=self.accept,daemon=True);self.thread.start()
    def arm(self,mode):
        with self.lock:self.mode=mode;self.hit.clear()
    def accept(self):
        try:
            while True:
                client,_=self.listener.accept();threading.Thread(target=self.connection,args=(client,),daemon=True).start()
        except OSError:pass
    def connection(self,client):
        backend=socket.create_connection(self.target);self.connections.extend([client,backend]);state={'drop_ack':False}
        def close():
            for s in (client,backend):
                try:s.shutdown(socket.SHUT_RDWR)
                except OSError:pass
                s.close()
        def responses():
            try:
                while True:
                    tag=exact(backend,1);length=exact(backend,4);payload=exact(backend,struct.unpack('!I',length)[0]-4)
                    if state['drop_ack']:
                        if tag==b'Z':self.hit.set();close();return
                        continue
                    client.sendall(tag+length+payload)
            except (EOFError,OSError):close()
        try:
            length=exact(client,4);payload=exact(client,struct.unpack('!I',length)[0]-4);backend.sendall(length+payload)
            threading.Thread(target=responses,daemon=True).start()
            while True:
                tag=exact(client,1);length=exact(client,4);payload=exact(client,struct.unpack('!I',length)[0]-4)
                if tag==b'Q' and payload.rstrip(b'\x00').strip().upper()==b'COMMIT':
                    with self.lock:mode=self.mode;self.mode=None
                    if mode=='before':self.hit.set();close();return
                    if mode=='after':state['drop_ack']=True
                backend.sendall(tag+length+payload)
        except (EOFError,OSError):close()
    def close(self):
        self.listener.close()
        for s in self.connections:
            try:s.shutdown(socket.SHUT_RDWR);s.close()
            except OSError:pass


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

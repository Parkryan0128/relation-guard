#!/usr/bin/env python3
"""Client fail-closed and retry behavior against a real scripted HTTP peer."""
import importlib.util,json,pathlib,threading,unittest,uuid
from http.server import BaseHTTPRequestHandler,ThreadingHTTPServer
root=pathlib.Path(__file__).resolve().parents[1];spec=importlib.util.spec_from_file_location('client',root/'examples/client.py');module=importlib.util.module_from_spec(spec);spec.loader.exec_module(module)
class ClientTests(unittest.TestCase):
 def setUp(self):
  self.seen=[];self.replies=[];outer=self
  class Handler(BaseHTTPRequestHandler):
   def do_POST(self):
    body=self.rfile.read(int(self.headers.get('Content-Length','0')));outer.seen.append((self.path,self.headers.get('Idempotency-Key'),body))
    status,value=outer.replies.pop(0)
    if status==0:self.connection.shutdown(2);self.connection.close();return
    payload=json.dumps(value).encode();self.send_response(status);self.send_header('Content-Length',str(len(payload)));self.end_headers();self.wfile.write(payload)
   def log_message(self,*args):pass
  self.server=ThreadingHTTPServer(('127.0.0.1',0),Handler);self.thread=threading.Thread(target=self.server.serve_forever,daemon=True);self.thread.start();self.client=module.Client(f'http://127.0.0.1:{self.server.server_port}',str(uuid.uuid4()),'fixture')
 def tearDown(self):self.server.shutdown();self.server.server_close();self.thread.join()
 def test_unknown_and_transport_retry_same_write(self):
  for first in [(503,{'error':{'code':'WRITE_OUTCOME_UNKNOWN'}}),(0,{})]:
   self.seen=[];self.replies=[first,(200,{'changed':True,'revision':'1'})]
   result=self.client.request('write',{'expected_revision':'0'},key='same-key');self.assertEqual(result['revision'],'1');self.assertEqual(len(self.seen),2);self.assertEqual(self.seen[0],self.seen[1])
 def test_conflict_is_not_blindly_retried(self):
  self.replies=[(409,{'error':{'code':'REVISION_CONFLICT'}})]
  with self.assertRaises(module.RevisionConflict):self.client.request('write',{'expected_revision':'0'},key='same-key')
  self.assertEqual(len(self.seen),1)
 def test_malformed_allow_and_errors_block(self):
  base={'decision':'allow','reason':'MATCHING_RELATION','evaluated_revision':'1','model_version':'org-doc-v1','request_id':str(uuid.uuid4())}
  cases=[[],{},dict(base,reason='NOT_FOUND'),dict(base,evaluated_revision=None),dict(base,evaluated_revision='01'),dict(base,request_id='invalid')]
  for value in cases:
   self.replies=[(200,value)]
   with self.assertRaises(module.AuthorizationError):self.client.can_view({'kind':'user','id':'u'},{'kind':'document','id':'d'})
  self.replies=[(503,{'error':{'code':'DATABASE_UNAVAILABLE'}})]
  with self.assertRaises(module.AuthorizationError):self.client.can_view({'kind':'user','id':'u'},{'kind':'document','id':'d'})
  self.replies=[(200,dict(base,decision='deny',reason='NO_MATCHING_RELATION'))];self.assertFalse(self.client.can_view({'kind':'user','id':'u'},{'kind':'document','id':'d'}))
if __name__=='__main__':unittest.main()

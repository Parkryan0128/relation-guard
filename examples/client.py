#!/usr/bin/env python3
"""Python standard-library integration: fail closed, bounded same-key retries."""
import json, os, pathlib, time, urllib.error, urllib.request, uuid
class AuthorizationError(Exception):
    pass
class RevisionConflict(AuthorizationError):
    pass
class Client:
    def __init__(self,base,tenant,token):
        self.base=f'{base}/v1/tenants/{tenant}';self.token=token
    def request(self,path,body=None,key=None):
        headers={'Authorization':'Bearer '+self.token,'Content-Type':'application/json'}
        if key:headers['Idempotency-Key']=key
        encoded=None if body is None else json.dumps(body).encode()
        # A write retry keeps exactly the same body and key. A conflict needs a new decision.
        for attempt in range(3):
            try:
                with urllib.request.urlopen(urllib.request.Request(self.base+'/'+path,data=encoded,headers=headers),timeout=3) as response:
                    return json.load(response)
            except urllib.error.HTTPError as error:
                result=json.load(error);code=result.get('error',{}).get('code')
                if code=='REVISION_CONFLICT':raise RevisionConflict(result)
                if not key or code not in ('WRITE_OUTCOME_UNKNOWN','DATABASE_UNAVAILABLE','OVERLOADED') or attempt==2:raise AuthorizationError(result)
            except (OSError,ValueError) as error:
                if not key or attempt==2:raise AuthorizationError('No verified result') from error
            time.sleep(.1*(attempt+1))
        raise AuthorizationError('Retry budget exhausted')
    def can_view(self,user,document):
        result=self.request('check',{'subject':user,'resource':document,'action':'view'})
        if not isinstance(result,dict):
            raise AuthorizationError('Invalid decision response')
        revision=result.get('evaluated_revision')
        if (result.get('decision') not in ('allow','deny') or result.get('model_version')!='org-doc-v1'
                or not isinstance(revision,str) or not revision.isascii() or not revision.isdecimal()
                or len(revision)>19 or (len(revision)>1 and revision[0]=='0') or int(revision)>9223372036854775807):
            raise AuthorizationError('Invalid decision response')
        try: uuid.UUID(result['request_id'])
        except (KeyError,ValueError,TypeError,AttributeError) as error:
            raise AuthorizationError('Invalid request ID') from error
        if result['decision']=='allow':
            if result.get('reason')!='MATCHING_RELATION':raise AuthorizationError('Invalid allow reason')
            return True
        if result.get('reason') not in ('NOT_FOUND','NO_MATCHING_RELATION'):raise AuthorizationError('Invalid deny reason')
        return False
def main():
    credentials=json.loads(pathlib.Path(os.environ.get('AUTHZ_CLIENT_FILE','secrets/client.json')).read_text())
    client=Client(os.environ.get('AUTHZ_URL','http://127.0.0.1:8080'),credentials['tenant_id'],credentials['token'])
    suffix=uuid.uuid4().hex;user={'kind':'user','id':'demo-'+suffix};doc={'kind':'document','id':'demo-'+suffix}
    edge={'resource':doc,'relation':'viewer','subject':user,'subject_relation':''}
    revision=client.request('revision')['revision']
    changed=client.request('write',{'expected_revision':revision,'operations':[{'op':'put_entity','entity':user},{'op':'put_entity','entity':doc},{'op':'put_relation','relationship':edge}]},str(uuid.uuid4()))
    assert client.can_view(user,doc);print('Granted: allow; protected operation may proceed.')
    client.request('write',{'expected_revision':changed['revision'],'operations':[{'op':'delete_relation','relationship':edge}]},str(uuid.uuid4()))
    assert not client.can_view(user,doc);print('Revoked: deny; protected operation blocked.')
if __name__=='__main__':main()

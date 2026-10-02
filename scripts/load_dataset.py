"""Deterministic authorization benchmark datasets (seed 424242)."""
# Fixed shape: 125 independent chains of eight folders; docs attach to their index modulo 1000.
def seed(lab,tenant,documents,total):
 entities=[(kind,f'{prefix}{i}') for kind,prefix,n in [('user','u',1000),('team','t',100),('folder','f',1000),('document','d',documents)] for i in range(n)]
 rows=[]
 for i in range(1000):
  rows.append(('team',f't{i%100}','member','user',f'u{i}',''))
  if i%8:rows.append(('folder',f'f{i}','parent','folder',f'f{i-1}',''))
 for i in range(documents):
  rows.extend([('document',f'd{i}','parent','folder',f'f{i%1000}',''),('document',f'd{i}','viewer','user',f'u{i%500}',''),('document',f'd{i}','editor','user',f'u{(i+1)%500}',''),('document',f'd{i}','viewer','team',f't{i%50}','member')])
 for i in range(total-len(rows)):rows.append(('document',f'd{i}','viewer','user',f'u{(i+2)%500}',''))
 assert len(rows)==total and len(set(rows))==total
 sql="BEGIN;\nINSERT INTO tenants(id) VALUES('"+tenant+"') ON CONFLICT DO NOTHING;\nCOPY entities(tenant_id,kind,id) FROM STDIN;\n"+''.join('\t'.join((tenant,*e))+'\n' for e in entities)+"\\.\nCOPY relationships FROM STDIN;\n"+''.join('\t'.join((tenant,*r))+'\n' for r in rows)+"\\.\nCOMMIT;\nANALYZE;\n"
 lab.sql(sql);return {'tenant':tenant,'users':1000,'teams':100,'folders':1000,'documents':documents,'relationships':total,'max_depth':8}

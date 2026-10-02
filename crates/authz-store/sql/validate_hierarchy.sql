WITH RECURSIVE changed AS (
 SELECT * FROM jsonb_to_recordset($2::jsonb) AS x(kind text,id text)
), affected(kind,id) AS (
 SELECT kind COLLATE "C",id COLLATE "C" FROM changed
 UNION
 SELECT r.resource_kind,r.resource_id FROM relationships r JOIN affected a
 ON r.subject_kind=a.kind AND r.subject_id=a.id
 WHERE r.tenant_id=$1 AND r.relation='parent'
), limited AS (SELECT * FROM affected LIMIT 20001), paths(kind,id,depth,path,cycle) AS (
 SELECT kind,id,0,ARRAY[kind||':'||id],false FROM limited
 UNION ALL
 SELECT r.subject_kind,r.subject_id,p.depth+1,p.path||(r.subject_kind||':'||r.subject_id),
 (r.subject_kind||':'||r.subject_id)=ANY(p.path)
 FROM paths p JOIN relationships r ON r.tenant_id=$1 AND r.resource_kind=p.kind AND r.resource_id=p.id AND r.relation='parent'
 WHERE p.depth<=32 AND NOT p.cycle
), bounded AS (SELECT * FROM paths LIMIT 20001)
SELECT count(*) AS work,COALESCE(bool_or(cycle OR depth>32),false) AS invalid FROM bounded

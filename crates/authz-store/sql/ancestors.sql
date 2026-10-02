WITH RECURSIVE requested AS (
 SELECT * FROM jsonb_to_recordset($2::jsonb) AS x(kind text, id text)
), chain(kind,id,depth,path,cycle) AS (
 SELECT e.kind,e.id,0,ARRAY[e.kind||':'||e.id],false
 FROM entities e JOIN requested q ON q.kind=e.kind AND q.id=e.id
 WHERE e.tenant_id=$1
 UNION ALL
 SELECT r.subject_kind,r.subject_id,c.depth+1,c.path||(r.subject_kind||':'||r.subject_id),
        (r.subject_kind||':'||r.subject_id)=ANY(c.path)
 FROM chain c JOIN relationships r ON r.tenant_id=$1 AND r.resource_kind=c.kind
   AND r.resource_id=c.id AND r.relation='parent'
 WHERE c.depth<=32 AND NOT c.cycle
), nodes AS (SELECT DISTINCT kind,id FROM chain), limited AS (SELECT * FROM nodes LIMIT 3301)
SELECT
 (SELECT COALESCE(jsonb_agg(jsonb_build_object('kind',kind,'id',id)),'[]') FROM limited) AS entities,
 (SELECT COALESCE(jsonb_agg(jsonb_build_object('resource',jsonb_build_object('kind',r.resource_kind,'id',r.resource_id),
 'relation',r.relation,'subject',jsonb_build_object('kind',r.subject_kind,'id',r.subject_id),'subject_relation',r.subject_relation)),'[]')
 FROM relationships r JOIN limited n ON r.resource_kind=n.kind AND r.resource_id=n.id
 WHERE r.tenant_id=$1 AND r.relation='parent') AS parents,
 EXISTS(SELECT 1 FROM entities WHERE tenant_id=$1 AND kind='user' AND id=$3) AS subject_exists,
 COALESCE((SELECT bool_or(cycle OR depth>32) FROM chain),false) AS corrupt

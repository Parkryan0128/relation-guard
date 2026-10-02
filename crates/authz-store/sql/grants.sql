SELECT jsonb_build_object('resource',jsonb_build_object('kind',r.resource_kind,'id',r.resource_id),
 'relation',r.relation,'subject',jsonb_build_object('kind',r.subject_kind,'id',r.subject_id),'subject_relation',r.subject_relation) AS edge
FROM relationships r JOIN jsonb_to_recordset($2::jsonb) AS n(kind text,id text)
 ON n.kind=r.resource_kind AND n.id=r.resource_id
WHERE r.tenant_id=$1 AND r.relation IN ('viewer','editor') AND (
 (r.subject_kind='user' AND r.subject_id=$3 AND r.subject_relation='') OR
 (r.subject_kind='team' AND r.subject_relation='member' AND EXISTS(
 SELECT 1 FROM relationships m WHERE m.tenant_id=$1 AND m.resource_kind='team'
 AND m.resource_id=r.subject_id AND m.relation='member' AND m.subject_kind='user'
 AND m.subject_id=$3 AND m.subject_relation='')))
ORDER BY r.resource_kind,r.resource_id,r.relation,r.subject_kind,r.subject_id
LIMIT 10001

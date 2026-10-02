-- Existing users remain members. An operator must explicitly bootstrap an admin.
ALTER TABLE entities ADD COLUMN org_admin boolean NOT NULL DEFAULT false;
ALTER TABLE entities ADD CONSTRAINT admin_is_user CHECK (NOT org_admin OR kind='user');
CREATE INDEX entities_org_admins ON entities(tenant_id,id) WHERE org_admin;
ALTER TABLE relationships DROP CONSTRAINT valid_relation_shape;
ALTER TABLE relationships ADD CONSTRAINT valid_relation_shape CHECK (
 (resource_kind='team' AND relation='member' AND subject_kind='user' AND subject_relation='') OR
 (resource_kind IN ('folder','document') AND relation='parent' AND subject_kind='folder' AND subject_relation='') OR
 (resource_kind IN ('folder','document') AND relation IN ('viewer','editor','manager') AND
   ((subject_kind='user' AND subject_relation='') OR (subject_kind='team' AND subject_relation='member')))
);
ALTER TABLE mutation_receipts ADD COLUMN actor_id text COLLATE "C" NOT NULL DEFAULT ''
 CHECK (actor_id='' OR actor_id ~ '^[A-Za-z0-9][A-Za-z0-9._-]{0,127}$');
ALTER TABLE mutation_receipts DROP CONSTRAINT mutation_receipts_pkey;
ALTER TABLE mutation_receipts ADD PRIMARY KEY (tenant_id,key_id,actor_id,idempotency_key);
ALTER TABLE audit_events ADD COLUMN actor_id text COLLATE "C"
 CHECK (actor_id IS NULL OR actor_id ~ '^[A-Za-z0-9][A-Za-z0-9._-]{0,127}$');
ALTER TABLE audit_events ADD COLUMN authorization_basis jsonb NOT NULL DEFAULT '{"kind":"legacy_operator"}'::jsonb
 CHECK (jsonb_typeof(authorization_basis)='object');

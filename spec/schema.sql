-- Relation Guard v0.1 DESIGN DDL, not an application migration.
-- PostgreSQL 18. See docs/05-storage.md for transaction-level invariants.
BEGIN;

CREATE TABLE tenants (
    id uuid PRIMARY KEY,
    revision bigint NOT NULL DEFAULT 0 CHECK (revision >= 0),
    created_at timestamptz NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE entities (
    tenant_id uuid NOT NULL REFERENCES tenants(id) ON DELETE RESTRICT,
    kind text COLLATE "C" NOT NULL CHECK (kind IN ('user', 'team', 'folder', 'document')),
    id text COLLATE "C" NOT NULL CHECK (id ~ '^[A-Za-z0-9][A-Za-z0-9._-]{0,127}$'),
    PRIMARY KEY (tenant_id, kind, id)
);

CREATE TABLE relationships (
    tenant_id uuid NOT NULL REFERENCES tenants(id) ON DELETE RESTRICT,
    resource_kind text COLLATE "C" NOT NULL,
    resource_id text COLLATE "C" NOT NULL,
    relation text COLLATE "C" NOT NULL,
    subject_kind text COLLATE "C" NOT NULL,
    subject_id text COLLATE "C" NOT NULL,
    subject_relation text COLLATE "C" NOT NULL DEFAULT '',
    PRIMARY KEY (tenant_id, resource_kind, resource_id, relation,
                 subject_kind, subject_id, subject_relation),
    FOREIGN KEY (tenant_id, resource_kind, resource_id)
        REFERENCES entities(tenant_id, kind, id) ON DELETE RESTRICT,
    FOREIGN KEY (tenant_id, subject_kind, subject_id)
        REFERENCES entities(tenant_id, kind, id) ON DELETE RESTRICT,
    CONSTRAINT valid_relation_shape CHECK (
        (resource_kind = 'team' AND relation = 'member'
         AND subject_kind = 'user' AND subject_relation = '')
        OR
        (resource_kind IN ('folder', 'document') AND relation = 'parent'
         AND subject_kind = 'folder' AND subject_relation = '')
        OR
        (resource_kind IN ('folder', 'document') AND relation IN ('viewer', 'editor')
         AND ((subject_kind = 'user' AND subject_relation = '')
              OR (subject_kind = 'team' AND subject_relation = 'member')))
    ),
    CONSTRAINT no_self_parent CHECK (
        relation <> 'parent' OR resource_kind <> subject_kind OR resource_id <> subject_id
    )
);

CREATE UNIQUE INDEX relationships_one_parent
    ON relationships (tenant_id, resource_kind, resource_id)
    WHERE relation = 'parent';

CREATE INDEX relationships_by_subject
    ON relationships (tenant_id, subject_kind, subject_id, subject_relation,
                      relation, resource_kind, resource_id);

CREATE TABLE mutation_receipts (
    tenant_id uuid NOT NULL REFERENCES tenants(id) ON DELETE RESTRICT,
    key_id text COLLATE "C" NOT NULL CHECK (key_id ~ '^[A-Za-z0-9._-]{1,64}$'),
    idempotency_key text COLLATE "C" NOT NULL
        CHECK (idempotency_key ~ '^[A-Za-z0-9._-]{1,128}$'),
    request_hash bytea NOT NULL CHECK (octet_length(request_hash) = 32),
    response jsonb NOT NULL CHECK (jsonb_typeof(response) = 'object'),
    recorded_at timestamptz NOT NULL DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (tenant_id, key_id, idempotency_key)
);
CREATE INDEX mutation_receipts_retention ON mutation_receipts (recorded_at);

CREATE TABLE audit_events (
    tenant_id uuid NOT NULL REFERENCES tenants(id) ON DELETE RESTRICT,
    revision bigint NOT NULL CHECK (revision > 0),
    key_id text COLLATE "C" NOT NULL CHECK (key_id ~ '^[A-Za-z0-9._-]{1,64}$'),
    request_id uuid NOT NULL,
    recorded_at timestamptz NOT NULL DEFAULT CURRENT_TIMESTAMP,
    effective_changes jsonb NOT NULL
        CHECK (jsonb_typeof(effective_changes) = 'array'
               AND jsonb_array_length(effective_changes) BETWEEN 1 AND 100),
    PRIMARY KEY (tenant_id, revision)
);

-- Cross-row cycle/depth checks, receipt correctness, revision advancement,
-- tenant-scoped reads and API authorization are service responsibilities.
-- recorded_at is recording time, NOT an exact database commit timestamp.
COMMIT;

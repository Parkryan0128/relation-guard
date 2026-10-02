use crate::{StoreError, database::begin, management, metrics};
use authz_core::*;
use serde_json::{Value, json};
use sqlx::{PgPool, Postgres, Row, Transaction};
use std::collections::BTreeSet;
use uuid::Uuid;

pub struct PreparedWrite {
    pub tx: Transaction<'static, Postgres>,
    pub response: Value,
    pub replayed: bool,
}
pub async fn prepare_write(
    pool: &PgPool,
    tenant: Uuid,
    key_id: &str,
    key: &str,
    request: &WriteRequest,
    request_id: Uuid,
) -> Result<PreparedWrite, StoreError> {
    prepare_write_inner(pool, tenant, key_id, key, request, request_id, None).await
}
pub async fn prepare_actor_write(
    pool: &PgPool,
    tenant: Uuid,
    key_id: &str,
    key: &str,
    request: &WriteRequest,
    request_id: Uuid,
    actor: &Id,
) -> Result<PreparedWrite, StoreError> {
    prepare_write_inner(pool, tenant, key_id, key, request, request_id, Some(actor)).await
}
async fn prepare_write_inner(
    pool: &PgPool,
    tenant: Uuid,
    key_id: &str,
    key: &str,
    request: &WriteRequest,
    request_id: Uuid,
    actor: Option<&Id>,
) -> Result<PreparedWrite, StoreError> {
    request.validate()?;
    if key.is_empty()
        || key.len() > 128
        || !key
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
    {
        return Err(StoreError::new("INVALID_REQUEST"));
    }
    let hash = request.hash()?;
    let mut tx = begin(pool, false).await?;
    let lock_timer = metrics::LOCK_WAIT.timer();
    let revision =
        sqlx::query_scalar::<_, i64>("SELECT revision FROM tenants WHERE id=$1 FOR UPDATE")
            .bind(tenant)
            .fetch_optional(&mut *tx)
            .await?
            .ok_or(StoreError::new("FORBIDDEN"))?;
    drop(lock_timer);
    if let Some(row) = sqlx::query(
        "SELECT request_hash,response FROM mutation_receipts WHERE tenant_id=$1 AND key_id=$2 AND idempotency_key=$3 AND actor_id=$4")
        .bind(tenant).bind(key_id).bind(key).bind(actor.map_or("", Id::as_str))
        .fetch_optional(&mut *tx).await?
    {
        if row.try_get::<Vec<u8>, _>("request_hash")? != hash {
            return Err(StoreError::new("IDEMPOTENCY_CONFLICT"));
        }
        return Ok(PreparedWrite { tx, response: row.try_get("response")?, replayed: true });
    }
    let basis = if let Some(actor) = actor {
        management::authorize_write(&mut tx, tenant, actor, request).await?
    } else {
        json!({"kind":"operator"})
    };
    let had_admin: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM entities WHERE tenant_id=$1 AND org_admin)",
    )
    .bind(tenant)
    .fetch_one(&mut *tx)
    .await?;
    if request.expected_revision.0 != revision {
        return Err(StoreError {
            code: "REVISION_CONFLICT",
            current_revision: Some(revision),
        });
    }
    let mut ops = canonical_operations(&request.operations)?;
    ops.sort_by_key(|(_, op)| match op {
        Operation::DeleteRelation { .. } => 0,
        Operation::DeleteEntity { .. } => 1,
        Operation::PutEntity { .. } => 2,
        Operation::PutRelation { .. } => 3,
        Operation::SetOrgRole { .. } => 4,
    });
    let mut changed = Vec::new();
    let mut hierarchy = BTreeSet::new();
    for (_, op) in ops {
        if apply_operation(&mut tx, tenant, op).await? > 0 {
            if let Operation::PutRelation { relationship } = op
                && relationship.relation == Relation::Parent
            {
                hierarchy.insert(relationship.resource.clone());
            }
            changed.push(op.clone());
        }
    }
    if !hierarchy.is_empty() {
        let row = sqlx::query(include_str!("../sql/validate_hierarchy.sql"))
            .bind(tenant)
            .bind(json!(hierarchy))
            .fetch_one(&mut *tx)
            .await?;
        if row.try_get::<i64, _>("work")? > MAX_WORK as i64 {
            return Err(StoreError::new("EVALUATION_LIMIT"));
        }
        if row.try_get::<bool, _>("invalid")? {
            return Err(StoreError::new("INVALID_HIERARCHY"));
        }
    }
    if had_admin
        && !sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS(SELECT 1 FROM entities WHERE tenant_id=$1 AND org_admin)",
        )
        .bind(tenant)
        .fetch_one(&mut *tx)
        .await?
    {
        return Err(StoreError::new("LAST_ADMIN"));
    }
    let changed_any = !changed.is_empty();
    let new_revision = if changed_any {
        revision
            .checked_add(1)
            .ok_or(StoreError::new("REVISION_EXHAUSTED"))?
    } else {
        revision
    };
    if changed_any {
        sqlx::query("UPDATE tenants SET revision=$2 WHERE id=$1")
            .bind(tenant)
            .bind(new_revision)
            .execute(&mut *tx)
            .await?;
        let changed: Vec<_> = canonical_operations(&changed)?
            .into_iter()
            .map(|(_, op)| op)
            .collect();
        sqlx::query("INSERT INTO audit_events(tenant_id,revision,key_id,request_id,effective_changes,actor_id,authorization_basis) VALUES($1,$2,$3,$4,$5,$6,$7)").bind(tenant).bind(new_revision).bind(key_id).bind(request_id).bind(json!(changed)).bind(actor.map(Id::as_str)).bind(&basis).execute(&mut *tx).await?;
    }
    let response = json!({"changed":changed_any,"revision":new_revision.to_string(),"model_version":MODEL,"request_id":request_id});
    sqlx::query("INSERT INTO mutation_receipts(tenant_id,key_id,idempotency_key,request_hash,response,actor_id) VALUES($1,$2,$3,$4,$5,$6)").bind(tenant).bind(key_id).bind(key).bind(hash).bind(&response).bind(actor.map_or("", Id::as_str)).execute(&mut *tx).await?;
    Ok(PreparedWrite {
        tx,
        response,
        replayed: false,
    })
}
// Match canonical JSON ordering used by receipts and historical audit records.
fn canonical_operations(operations: &[Operation]) -> Result<Vec<(String, &Operation)>, StoreError> {
    let mut keyed = operations
        .iter()
        .map(|op| {
            serde_json::to_value(op)
                .map(|value| (value.to_string(), op))
                .map_err(|_| StoreError::new("INTERNAL_ERROR"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    keyed.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(keyed)
}

async fn apply_operation(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    op: &Operation,
) -> Result<u64, StoreError> {
    let result = match op {
        Operation::SetOrgRole { user_id, role } => {
            let exists: bool = sqlx::query_scalar(
                "SELECT EXISTS(SELECT 1 FROM entities WHERE tenant_id=$1 AND kind='user' AND id=$2)")
                .bind(tenant).bind(user_id.as_str()).fetch_one(&mut **tx).await?;
            if !exists {
                return Err(StoreError::new("UNKNOWN_ENTITY"));
            }
            sqlx::query("UPDATE entities SET org_admin=$3 WHERE tenant_id=$1 AND kind='user' AND id=$2 AND org_admin<>$3")
                .bind(tenant).bind(user_id.as_str()).bind(*role == OrgRole::Admin)
                .execute(&mut **tx).await?
        }
        Operation::PutEntity { entity } => {
            sqlx::query("INSERT INTO entities(tenant_id,kind,id) VALUES($1,$2,$3) ON CONFLICT(tenant_id,kind,id) DO NOTHING")
                .bind(tenant).bind(entity.kind.as_str()).bind(entity.id.as_str())
                .execute(&mut **tx).await?
        }
        Operation::DeleteEntity { entity } => {
            sqlx::query("DELETE FROM entities WHERE tenant_id=$1 AND kind=$2 AND id=$3")
                .bind(tenant).bind(entity.kind.as_str()).bind(entity.id.as_str())
                .execute(&mut **tx).await.map_err(|error| match error {
                    sqlx::Error::Database(ref detail)
                        if matches!(detail.code().as_deref(), Some("23503" | "23001")) => StoreError::new("ENTITY_IN_USE"),
                    other => other.into(),
                })?
        }
        Operation::PutRelation { relationship } | Operation::DeleteRelation { relationship } => {
            let sql = if matches!(op, Operation::PutRelation { .. }) {
                "INSERT INTO relationships(tenant_id,resource_kind,resource_id,relation,subject_kind,subject_id,subject_relation) VALUES($1,$2,$3,$4,$5,$6,$7) ON CONFLICT(tenant_id,resource_kind,resource_id,relation,subject_kind,subject_id,subject_relation) DO NOTHING"
            } else {
                "DELETE FROM relationships WHERE tenant_id=$1 AND resource_kind=$2 AND resource_id=$3 AND relation=$4 AND subject_kind=$5 AND subject_id=$6 AND subject_relation=$7"
            };
            sqlx::query(sql)
                .bind(tenant).bind(relationship.resource.kind.as_str())
                .bind(relationship.resource.id.as_str()).bind(relationship.relation.as_str())
                .bind(relationship.subject.kind.as_str()).bind(relationship.subject.id.as_str())
                .bind(&relationship.subject_relation).execute(&mut **tx).await?
        }
    };
    Ok(result.rows_affected())
}

pub async fn write(
    pool: &PgPool,
    tenant: Uuid,
    key_id: &str,
    key: &str,
    request: &WriteRequest,
    request_id: Uuid,
) -> Result<(Value, bool), StoreError> {
    let prepared = prepare_write(pool, tenant, key_id, key, request, request_id).await?;
    prepared
        .tx
        .commit()
        .await
        .map_err(|_| StoreError::new("WRITE_OUTCOME_UNKNOWN"))?;
    Ok((prepared.response, prepared.replayed))
}
pub async fn cleanup_receipts<'e, E: sqlx::Executor<'e, Database = Postgres>>(
    executor: E,
) -> Result<u64, StoreError> {
    Ok(sqlx::query("DELETE FROM mutation_receipts WHERE (tenant_id,key_id,actor_id,idempotency_key) IN (SELECT tenant_id,key_id,actor_id,idempotency_key FROM mutation_receipts WHERE recorded_at < CURRENT_TIMESTAMP - INTERVAL '24 hours' ORDER BY recorded_at LIMIT 1000)").execute(executor).await?.rows_affected())
}

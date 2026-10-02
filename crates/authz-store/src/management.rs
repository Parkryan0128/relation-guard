//! User-authorized changes. All decisions use the state before any operation runs.
use crate::{
    StoreError, begin_read,
    database::begin,
    read::{finish_check, load_tx},
};
use authz_core::*;
use serde_json::{Value, json};
use sqlx::{PgPool, Postgres, Transaction};
use std::collections::{BTreeMap, BTreeSet};
use uuid::Uuid;

pub async fn actor_is_admin(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    actor: &Id,
) -> Result<bool, StoreError> {
    sqlx::query_scalar(
        "SELECT org_admin FROM entities WHERE tenant_id=$1 AND kind='user' AND id=$2",
    )
    .bind(tenant)
    .bind(actor.as_str())
    .fetch_optional(&mut **tx)
    .await?
    .ok_or(StoreError::new("FORBIDDEN"))
}
pub async fn require_manage(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    actor: &Id,
    resources: BTreeSet<Entity>,
) -> Result<(), StoreError> {
    if resources.is_empty() {
        return Ok(());
    }
    if resources.len() > MAX_ITEMS {
        return Err(StoreError::new("EVALUATION_LIMIT"));
    }
    let request = BatchRequest {
        subject: Entity {
            kind: Kind::User,
            id: actor.clone(),
        },
        checks: resources
            .into_iter()
            .map(|resource| Check {
                resource,
                action: Action::Manage,
            })
            .collect(),
        explain: false,
    };
    let facts = load_tx(tx, tenant, &request).await?;
    if evaluate(&facts, &request)?
        .decisions
        .iter()
        .any(|d| d.decision != Verdict::Allow)
    {
        return Err(StoreError::new("FORBIDDEN"));
    }
    Ok(())
}
pub(crate) async fn authorize_write(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    actor: &Id,
    request: &WriteRequest,
) -> Result<Value, StoreError> {
    if actor_is_admin(tx, tenant, actor).await? {
        return Ok(json!({"kind":"org_admin"}));
    }
    let mut resources = BTreeSet::new();
    let mut creates = BTreeSet::new();
    let mut deletes = BTreeSet::new();
    let mut parents = BTreeMap::new();
    let mut changed_parent = BTreeSet::new();
    for op in &request.operations {
        match op {
            Operation::SetOrgRole { .. } => return Err(StoreError::new("FORBIDDEN")),
            Operation::PutEntity { entity } | Operation::DeleteEntity { entity } => {
                if !entity.resource() {
                    return Err(StoreError::new("FORBIDDEN"));
                }
                resources.insert(entity.clone());
                if matches!(op, Operation::PutEntity { .. }) {
                    creates.insert(entity.clone());
                } else {
                    deletes.insert(entity.clone());
                }
            }
            Operation::PutRelation { relationship: e }
            | Operation::DeleteRelation { relationship: e } => {
                if !e.resource.resource() {
                    return Err(StoreError::new("FORBIDDEN"));
                }
                resources.insert(e.resource.clone());
                if e.relation == Relation::Parent {
                    changed_parent.insert(e.resource.clone());
                    if matches!(op, Operation::PutRelation { .. })
                        && parents
                            .insert(e.resource.clone(), e.subject.clone())
                            .is_some()
                    {
                        return Err(StoreError::new("INVALID_HIERARCHY"));
                    }
                }
            }
        }
    }
    let existing:Value=sqlx::query_scalar("SELECT COALESCE(jsonb_agg(jsonb_build_object('kind',e.kind,'id',e.id)),'[]') FROM entities e JOIN jsonb_to_recordset($2::jsonb) AS r(kind text,id text) ON e.kind=r.kind AND e.id=r.id WHERE e.tenant_id=$1")
        .bind(tenant).bind(json!(resources)).fetch_one(&mut **tx).await?;
    let existing: BTreeSet<Entity> =
        serde_json::from_value(existing).map_err(|_| StoreError::new("INTERNAL_ERROR"))?;
    let new: BTreeSet<Entity> = creates.difference(&existing).cloned().collect();
    let mut required = BTreeSet::new();
    for resource in &resources {
        if new.contains(resource) {
            let parent = parents.get(resource).ok_or(StoreError::new("FORBIDDEN"))?;
            required.insert(parent.clone());
        } else {
            required.insert(resource.clone());
        }
    }
    for resource in changed_parent {
        if let Some(parent) = parents.get(&resource) {
            required.insert(parent.clone());
        } else if !deletes.contains(&resource) {
            return Err(StoreError::new("FORBIDDEN"));
        }
    }
    // Every authority anchor must already exist; grants made in this request do not count.
    require_manage(tx, tenant, actor, required.clone()).await?;
    Ok(json!({"kind":"resource_manager","resources":required}))
}

pub async fn check_as(
    pool: &PgPool,
    tenant: Uuid,
    actor: &Id,
    request: &BatchRequest,
) -> Result<(i64, Evaluation), StoreError> {
    request.validate()?;
    let mut read = begin_read(pool, tenant).await?;
    let admin = actor_is_admin(&mut read.tx, tenant, actor).await?;
    if request.subject.id != *actor && !admin {
        return Err(StoreError::new("FORBIDDEN"));
    }
    finish_check(read, request).await
}

/// Explicit privileged-DB operation; never routed from the HTTP data API.
pub async fn bootstrap_admin(
    pool: &PgPool,
    tenant: Uuid,
    user: &Id,
    recovery: bool,
) -> Result<(), StoreError> {
    let mut tx = begin(pool, false).await?;
    let revision: i64 = sqlx::query_scalar("SELECT revision FROM tenants WHERE id=$1 FOR UPDATE")
        .bind(tenant)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or(StoreError::new("FORBIDDEN"))?;
    if !recovery
        && sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS(SELECT 1 FROM entities WHERE tenant_id=$1 AND org_admin)",
        )
        .bind(tenant)
        .fetch_one(&mut *tx)
        .await?
    {
        return Err(StoreError::new("ADMIN_ALREADY_EXISTS"));
    }
    let existed: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM entities WHERE tenant_id=$1 AND kind='user' AND id=$2)",
    )
    .bind(tenant)
    .bind(user.as_str())
    .fetch_one(&mut *tx)
    .await?;
    let changed=sqlx::query("INSERT INTO entities(tenant_id,kind,id,org_admin) VALUES($1,'user',$2,true) ON CONFLICT(tenant_id,kind,id) DO UPDATE SET org_admin=true WHERE NOT entities.org_admin")
        .bind(tenant).bind(user.as_str()).execute(&mut *tx).await?.rows_affected();
    if changed > 0 {
        let next = revision
            .checked_add(1)
            .ok_or(StoreError::new("REVISION_EXHAUSTED"))?;
        sqlx::query("UPDATE tenants SET revision=$2 WHERE id=$1")
            .bind(tenant)
            .bind(next)
            .execute(&mut *tx)
            .await?;
        let mut changes = Vec::new();
        if !existed {
            changes.push(json!({"op":"put_entity","entity":{"kind":"user","id":user}}));
        }
        changes.push(json!({"op":"set_org_role","user_id":user,"role":"admin"}));
        sqlx::query("INSERT INTO audit_events(tenant_id,revision,key_id,request_id,effective_changes,authorization_basis) VALUES($1,$2,'operator',$3,$4,$5)")
            .bind(tenant).bind(next).bind(Uuid::new_v4()).bind(json!(changes))
            .bind(json!({"kind":if recovery {"operator_recovery"}else{"operator_bootstrap"}})).execute(&mut *tx).await?;
    }
    tx.commit()
        .await
        .map_err(|_| StoreError::new("WRITE_OUTCOME_UNKNOWN"))?;
    Ok(())
}

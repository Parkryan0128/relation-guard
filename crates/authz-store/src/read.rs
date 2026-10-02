use crate::{StoreError, database::begin, metrics};
use authz_core::*;
use serde_json::json;
use sqlx::{PgPool, Postgres, Row, Transaction};
use std::collections::BTreeSet;
use uuid::Uuid;

pub struct ReadSnapshot {
    pub tx: Transaction<'static, Postgres>,
    pub revision: i64,
    pub tenant: Uuid,
}
pub async fn begin_read(pool: &PgPool, tenant: Uuid) -> Result<ReadSnapshot, StoreError> {
    let mut tx = begin(pool, true).await?;
    let query_timer = metrics::DATA_SQL.timer();
    let revision = sqlx::query_scalar::<_, i64>("SELECT revision FROM tenants WHERE id=$1")
        .bind(tenant)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or(StoreError::new("FORBIDDEN"))?;
    drop(query_timer);
    Ok(ReadSnapshot {
        tx,
        revision,
        tenant,
    })
}
pub async fn load(
    snapshot: &mut ReadSnapshot,
    request: &BatchRequest,
) -> Result<Snapshot, StoreError> {
    load_tx(&mut snapshot.tx, snapshot.tenant, request).await
}
pub(crate) async fn load_tx(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    request: &BatchRequest,
) -> Result<Snapshot, StoreError> {
    request.validate()?;
    let resources: BTreeSet<_> = request.checks.iter().map(|c| c.resource.clone()).collect();
    let query_timer = metrics::DATA_SQL.timer();
    let row = sqlx::query(include_str!("../sql/ancestors.sql"))
        .bind(tenant)
        .bind(json!(resources))
        .bind(request.subject.id.as_str())
        .fetch_one(&mut **tx)
        .await?;
    drop(query_timer);
    if row.try_get::<bool, _>("corrupt")? {
        return Err(StoreError::new("INTERNAL_ERROR"));
    }
    let mut facts = Snapshot {
        administrators: BTreeSet::new(),
        entities: serde_json::from_value(row.try_get("entities")?)
            .map_err(|_| StoreError::new("INTERNAL_ERROR"))?,
        relationships: serde_json::from_value(row.try_get("parents")?)
            .map_err(|_| StoreError::new("INTERNAL_ERROR"))?,
    };
    if facts.entities.len() > MAX_RESOURCES {
        return Err(StoreError::new("EVALUATION_LIMIT"));
    }
    if row.try_get::<bool, _>("subject_exists")? {
        facts.entities.insert(request.subject.clone());
    }
    if row.try_get::<bool, _>("subject_admin")? {
        facts.administrators.insert(request.subject.clone());
    }
    let query_timer = metrics::DATA_SQL.timer();
    let rows = sqlx::query(include_str!("../sql/grants.sql"))
        .bind(tenant)
        .bind(json!(
            facts
                .entities
                .iter()
                .filter(|e| e.resource())
                .collect::<Vec<_>>()
        ))
        .bind(request.subject.id.as_str())
        .fetch_all(&mut **tx)
        .await?;
    drop(query_timer);
    if rows.len() > MAX_GRANTS {
        return Err(StoreError::new("EVALUATION_LIMIT"));
    }
    for row in rows {
        let e: Edge = serde_json::from_value(row.try_get("edge")?)
            .map_err(|_| StoreError::new("INTERNAL_ERROR"))?;
        facts.entities.insert(e.subject.clone());
        if e.subject.kind == Kind::Team {
            facts.relationships.insert(Edge {
                resource: e.subject.clone(),
                relation: Relation::Member,
                subject: request.subject.clone(),
                subject_relation: String::new(),
            });
        }
        facts.relationships.insert(e);
    }
    metrics::FACTS.observe(facts.relationships.len() as u64);
    Ok(facts)
}
pub async fn check(
    pool: &PgPool,
    tenant: Uuid,
    request: &BatchRequest,
) -> Result<(i64, Evaluation), StoreError> {
    finish_check(begin_read(pool, tenant).await?, request).await
}

pub(crate) async fn finish_check(
    mut read: ReadSnapshot,
    request: &BatchRequest,
) -> Result<(i64, Evaluation), StoreError> {
    let facts = load(&mut read, request).await?;
    let timer = metrics::CORE.timer();
    let result = evaluate(&facts, request)?;
    drop(timer);
    metrics::WORK.observe(result.work as u64);
    read.tx.commit().await?;
    Ok((read.revision, result))
}

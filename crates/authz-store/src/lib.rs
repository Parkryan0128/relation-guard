pub mod metrics;
use authz_core::*;
use serde_json::{Value, json};
use sqlx::{PgPool, Postgres, Row, Transaction, postgres::PgPoolOptions};
use std::{collections::BTreeSet, time::Duration};
use uuid::Uuid;
pub const SCHEMA_VERSION: i64 = 1;
pub static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("../../migrations");

#[derive(Debug, thiserror::Error)]
#[error("{code}")]
pub struct StoreError {
    pub code: &'static str,
    pub current_revision: Option<i64>,
}
impl StoreError {
    pub fn new(code: &'static str) -> Self {
        Self {
            code,
            current_revision: None,
        }
    }
}
impl From<Fault> for StoreError {
    fn from(e: Fault) -> Self {
        Self::new(match e {
            Fault::InvalidRequest => "INVALID_REQUEST",
            Fault::DuplicateOperation => "DUPLICATE_OPERATION",
            Fault::InvalidRelation => "INVALID_RELATION",
            Fault::EvaluationLimit => "EVALUATION_LIMIT",
            Fault::CorruptGraph => "INTERNAL_ERROR",
            Fault::PayloadTooLarge => "PAYLOAD_TOO_LARGE",
        })
    }
}
impl From<sqlx::Error> for StoreError {
    fn from(e: sqlx::Error) -> Self {
        let code = match &e {
            sqlx::Error::PoolTimedOut => "OVERLOADED",
            sqlx::Error::Io(_) | sqlx::Error::Tls(_) | sqlx::Error::PoolClosed => {
                "DATABASE_UNAVAILABLE"
            }
            sqlx::Error::Database(d) => match d.code().as_deref() {
                Some("55P03") => "OVERLOADED",
                Some("55000") if d.message().contains("not currently accepting connections") => {
                    "DATABASE_UNAVAILABLE"
                }
                Some("57014") => "DEADLINE_EXCEEDED",
                Some("23503") => "UNKNOWN_ENTITY",
                Some("23505") => "INVALID_HIERARCHY",
                Some("23514") if d.constraint() == Some("no_self_parent") => "INVALID_HIERARCHY",
                Some("23514") => "INVALID_RELATION",
                Some(c) if c.starts_with("08") || matches!(c, "57P01" | "57P02" | "57P03") => {
                    "DATABASE_UNAVAILABLE"
                }
                _ => "INTERNAL_ERROR",
            },
            _ => "INTERNAL_ERROR",
        };
        Self::new(code)
    }
}
pub async fn pool(url: &str) -> Result<PgPool, StoreError> {
    Ok(PgPoolOptions::new()
        .max_connections(10)
        .acquire_timeout(Duration::from_millis(250))
        .connect(url)
        .await?)
}
async fn begin(pool: &PgPool) -> Result<Transaction<'static, Postgres>, StoreError> {
    let timer = metrics::POOL_WAIT.timer();
    let connection = pool.acquire().await?;
    drop(timer);
    Ok(Transaction::begin(connection, None).await?)
}
async fn limits(tx: &mut Transaction<'_, Postgres>) -> Result<(), StoreError> {
    sqlx::query("SET LOCAL statement_timeout = '1s'")
        .execute(&mut **tx)
        .await?;
    sqlx::query("SET LOCAL lock_timeout = '500ms'")
        .execute(&mut **tx)
        .await?;
    Ok(())
}
pub async fn provision(pool: &PgPool, tenant: Uuid) -> Result<(), StoreError> {
    sqlx::query("INSERT INTO tenants(id) VALUES ($1) ON CONFLICT(id) DO NOTHING")
        .bind(tenant)
        .execute(pool)
        .await?;
    Ok(())
}
pub async fn ready(pool: &PgPool) -> bool {
    matches!(
        sqlx::query_scalar::<_, i64>("SELECT max(version) FROM _sqlx_migrations WHERE success")
            .fetch_one(pool)
            .await,
        Ok(SCHEMA_VERSION)
    )
}
pub struct ReadSnapshot {
    pub tx: Transaction<'static, Postgres>,
    pub revision: i64,
    pub tenant: Uuid,
}
pub async fn begin_read(pool: &PgPool, tenant: Uuid) -> Result<ReadSnapshot, StoreError> {
    let mut tx = begin(pool).await?;
    sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ READ ONLY")
        .execute(&mut *tx)
        .await?;
    limits(&mut tx).await?;
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
    request.validate()?;
    let resources: BTreeSet<_> = request.checks.iter().map(|c| c.resource.clone()).collect();
    let query_timer = metrics::DATA_SQL.timer();
    let row = sqlx::query(include_str!("../sql/ancestors.sql"))
        .bind(snapshot.tenant)
        .bind(json!(resources))
        .bind(request.subject.id.as_str())
        .fetch_one(&mut *snapshot.tx)
        .await?;
    drop(query_timer);
    if row.try_get::<bool, _>("corrupt")? {
        return Err(StoreError::new("INTERNAL_ERROR"));
    }
    let mut facts = Snapshot {
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
    let query_timer = metrics::DATA_SQL.timer();
    let rows = sqlx::query(include_str!("../sql/grants.sql"))
        .bind(snapshot.tenant)
        .bind(json!(
            facts
                .entities
                .iter()
                .filter(|e| e.resource())
                .collect::<Vec<_>>()
        ))
        .bind(request.subject.id.as_str())
        .fetch_all(&mut *snapshot.tx)
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
    let mut read = begin_read(pool, tenant).await?;
    let facts = load(&mut read, request).await?;
    let timer = metrics::CORE.timer();
    let result = evaluate(&facts, request)?;
    drop(timer);
    metrics::WORK.observe(result.work as u64);
    read.tx.commit().await?;
    Ok((read.revision, result))
}

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
    let mut tx = begin(pool).await?;
    limits(&mut tx).await?;
    let lock_timer = metrics::LOCK_WAIT.timer();
    let revision =
        sqlx::query_scalar::<_, i64>("SELECT revision FROM tenants WHERE id=$1 FOR UPDATE")
            .bind(tenant)
            .fetch_optional(&mut *tx)
            .await?
            .ok_or(StoreError::new("FORBIDDEN"))?;
    drop(lock_timer);
    if let Some(row)=sqlx::query("SELECT request_hash,response FROM mutation_receipts WHERE tenant_id=$1 AND key_id=$2 AND idempotency_key=$3").bind(tenant).bind(key_id).bind(key).fetch_optional(&mut *tx).await?{
  if row.try_get::<Vec<u8>,_>("request_hash")?!=hash{return Err(StoreError::new("IDEMPOTENCY_CONFLICT"))}
  return Ok(PreparedWrite{tx,response:row.try_get("response")?,replayed:true});
 }
    if request.expected_revision.0 != revision {
        return Err(StoreError {
            code: "REVISION_CONFLICT",
            current_revision: Some(revision),
        });
    }
    let mut ops: Vec<_> = request.operations.iter().collect();
    ops.sort_by_cached_key(|o| {
        let phase = match o {
            Operation::DeleteRelation { .. } => 0,
            Operation::DeleteEntity { .. } => 1,
            Operation::PutEntity { .. } => 2,
            Operation::PutRelation { .. } => 3,
        };
        (
            phase,
            serde_json::to_value(o)
                .map(|v| v.to_string())
                .unwrap_or_default(),
        )
    });
    let mut changed = Vec::new();
    let mut hierarchy = BTreeSet::new();
    for op in ops {
        let count=match op {
   Operation::PutEntity{entity}=>sqlx::query("INSERT INTO entities(tenant_id,kind,id) VALUES($1,$2,$3) ON CONFLICT(tenant_id,kind,id) DO NOTHING").bind(tenant).bind(entity.kind.as_str()).bind(entity.id.as_str()).execute(&mut *tx).await?.rows_affected(),
   Operation::DeleteEntity{entity}=>match sqlx::query("DELETE FROM entities WHERE tenant_id=$1 AND kind=$2 AND id=$3").bind(tenant).bind(entity.kind.as_str()).bind(entity.id.as_str()).execute(&mut *tx).await{
    Ok(r)=>r.rows_affected(),Err(sqlx::Error::Database(d)) if matches!(d.code().as_deref(),Some("23503" | "23001"))=>return Err(StoreError::new("ENTITY_IN_USE")),Err(e)=>return Err(e.into())},
   Operation::PutRelation{relationship:e}|Operation::DeleteRelation{relationship:e}=>{
    let putting=matches!(op,Operation::PutRelation{..});
    let sql=if putting{"INSERT INTO relationships(tenant_id,resource_kind,resource_id,relation,subject_kind,subject_id,subject_relation) VALUES($1,$2,$3,$4,$5,$6,$7) ON CONFLICT(tenant_id,resource_kind,resource_id,relation,subject_kind,subject_id,subject_relation) DO NOTHING"}else{"DELETE FROM relationships WHERE tenant_id=$1 AND resource_kind=$2 AND resource_id=$3 AND relation=$4 AND subject_kind=$5 AND subject_id=$6 AND subject_relation=$7"};
    let n=sqlx::query(sql).bind(tenant).bind(e.resource.kind.as_str()).bind(e.resource.id.as_str()).bind(e.relation.as_str()).bind(e.subject.kind.as_str()).bind(e.subject.id.as_str()).bind(&e.subject_relation).execute(&mut *tx).await?.rows_affected();
    if putting && e.relation==Relation::Parent && n>0{hierarchy.insert(e.resource.clone());}n
   }
  };
        if count > 0 {
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
        changed.sort_by_cached_key(|o| {
            serde_json::to_value(o)
                .map(|v| v.to_string())
                .unwrap_or_default()
        });
        sqlx::query("INSERT INTO audit_events(tenant_id,revision,key_id,request_id,effective_changes) VALUES($1,$2,$3,$4,$5)").bind(tenant).bind(new_revision).bind(key_id).bind(request_id).bind(json!(changed)).execute(&mut *tx).await?;
    }
    let response = json!({"changed":changed_any,"revision":new_revision.to_string(),"model_version":MODEL,"request_id":request_id});
    sqlx::query("INSERT INTO mutation_receipts(tenant_id,key_id,idempotency_key,request_hash,response) VALUES($1,$2,$3,$4,$5)").bind(tenant).bind(key_id).bind(key).bind(hash).bind(&response).execute(&mut *tx).await?;
    Ok(PreparedWrite {
        tx,
        response,
        replayed: false,
    })
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
    Ok(sqlx::query("DELETE FROM mutation_receipts WHERE (tenant_id,key_id,idempotency_key) IN (SELECT tenant_id,key_id,idempotency_key FROM mutation_receipts WHERE recorded_at < CURRENT_TIMESTAMP - INTERVAL '24 hours' ORDER BY recorded_at LIMIT 1000)").execute(executor).await?.rows_affected())
}

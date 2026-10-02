pub mod management;
pub mod metrics;
use authz_core::*;
use serde_json::{Value, json};
use sqlx::{PgPool, Postgres, Row, Transaction, postgres::PgPoolOptions};
use std::{collections::BTreeSet, time::Duration};
use uuid::Uuid;
pub const SCHEMA_VERSION: i64 = 2;
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
    pool_with_minimum(url, 0).await
}
/// Keep the bounded service pool ready before accepting data requests.
/// Admin commands retain the smaller, on-demand pool above.
pub async fn service_pool(url: &str) -> Result<PgPool, StoreError> {
    pool_with_minimum(url, 10).await
}
async fn pool_with_minimum(url: &str, minimum: u32) -> Result<PgPool, StoreError> {
    Ok(PgPoolOptions::new()
        .max_connections(10)
        .min_connections(minimum)
        // BEGIN detects a stale socket; never turn connection failure into a decision.
        .test_before_acquire(false)
        .acquire_timeout(Duration::from_millis(250))
        .connect(url)
        .await?)
}
async fn begin(
    pool: &PgPool,
    read_only: bool,
) -> Result<Transaction<'static, Postgres>, StoreError> {
    let timer = metrics::POOL_WAIT.timer();
    let connection = pool.acquire().await.inspect_err(|error| {
        let counter = if matches!(error, sqlx::Error::PoolTimedOut) {
            &metrics::POOL_ACQUIRE_TIMEOUTS
        } else {
            &metrics::POOL_ACQUIRE_ERRORS
        };
        counter.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    })?;
    drop(timer);
    // Constant setup statements share a single PostgreSQL protocol round trip.
    // The first data SELECT still establishes the repeatable-read snapshot.
    let setup = if read_only {
        "BEGIN ISOLATION LEVEL REPEATABLE READ READ ONLY; SET LOCAL statement_timeout='1s'; SET LOCAL lock_timeout='500ms'"
    } else {
        "BEGIN; SET LOCAL statement_timeout='1s'; SET LOCAL lock_timeout='500ms'"
    };
    Ok(Transaction::begin(connection, Some(setup.into()))
        .await
        .inspect_err(|_| {
            metrics::TRANSACTION_BEGIN_ERRORS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        })?)
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
    if let Some(row)=sqlx::query("SELECT request_hash,response FROM mutation_receipts WHERE tenant_id=$1 AND key_id=$2 AND idempotency_key=$3 AND actor_id=$4").bind(tenant).bind(key_id).bind(key).bind(actor.map_or("", Id::as_str)).fetch_optional(&mut *tx).await?{
  if row.try_get::<Vec<u8>,_>("request_hash")?!=hash{return Err(StoreError::new("IDEMPOTENCY_CONFLICT"))}
  return Ok(PreparedWrite{tx,response:row.try_get("response")?,replayed:true});
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
    let mut ops: Vec<_> = request.operations.iter().collect();
    ops.sort_by_cached_key(|o| {
        let phase = match o {
            Operation::DeleteRelation { .. } => 0,
            Operation::DeleteEntity { .. } => 1,
            Operation::PutEntity { .. } => 2,
            Operation::PutRelation { .. } => 3,
            Operation::SetOrgRole { .. } => 4,
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
   Operation::SetOrgRole{user_id,role}=>{
    let admin=*role==OrgRole::Admin;
    let exists:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM entities WHERE tenant_id=$1 AND kind='user' AND id=$2)").bind(tenant).bind(user_id.as_str()).fetch_one(&mut *tx).await?;
    if !exists{return Err(StoreError::new("UNKNOWN_ENTITY"));}
    sqlx::query("UPDATE entities SET org_admin=$3 WHERE tenant_id=$1 AND kind='user' AND id=$2 AND org_admin<>$3").bind(tenant).bind(user_id.as_str()).bind(admin).execute(&mut *tx).await?.rows_affected()
   },
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
        changed.sort_by_cached_key(|o| {
            serde_json::to_value(o)
                .map(|v| v.to_string())
                .unwrap_or_default()
        });
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

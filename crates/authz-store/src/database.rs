use crate::StoreError;
use crate::metrics;
use sqlx::{PgPool, Postgres, Transaction, postgres::PgPoolOptions};
use std::time::Duration;
use uuid::Uuid;

pub const SCHEMA_VERSION: i64 = 2;
// Allow brief pool saturation without retrying a transaction.
// HTTP's overall 2s deadline remains authoritative.
pub const POOL_ACQUIRE_TIMEOUT: Duration = Duration::from_millis(500);
pub static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("../../migrations");

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
        .acquire_timeout(POOL_ACQUIRE_TIMEOUT)
        .connect(url)
        .await?)
}
pub(crate) async fn begin(
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

mod api;
use api::endpoint;
mod auth;
mod queries;
use authz_store::service_pool as pool;
use axum::{
    Router,
    extract::{Request, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::any,
};
use serde_json::json;
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::Duration,
};
use tokio::sync::{Semaphore, watch};

static HTTP_LATENCY: authz_store::metrics::Metric = authz_store::metrics::Metric::new();
static ALLOW: AtomicU64 = AtomicU64::new(0);
static DENY: AtomicU64 = AtomicU64::new(0);
static CONFLICTS: AtomicU64 = AtomicU64::new(0);

struct App {
    pool: sqlx::PgPool,
    keys: auth::Config,
    slots: Arc<Semaphore>,
    draining: AtomicBool,
    requests: AtomicU64,
    errors: AtomicU64,
    #[cfg(test)]
    before_commit: Option<Arc<tests::CommitBarrier>>,
}
async fn ops(State(app): State<Arc<App>>, request: Request) -> Response {
    if request.method() != axum::http::Method::GET {
        return StatusCode::METHOD_NOT_ALLOWED.into_response();
    }
    match request.uri().path() {
        "/healthz" => axum::Json(json!({"status":"ok"})).into_response(),
        "/readyz" => {
            if !app.draining.load(Ordering::Acquire)
                && tokio::time::timeout(Duration::from_millis(250), authz_store::ready(&app.pool))
                    .await
                    .unwrap_or(false)
            {
                axum::Json(json!({"status":"ready"})).into_response()
            } else {
                (
                    StatusCode::SERVICE_UNAVAILABLE,
                    axum::Json(json!({"status":"not_ready"})),
                )
                    .into_response()
            }
        }
        "/metrics" => {
            let mut output = format!(
                "relation_guard_requests_total {}\nrelation_guard_errors_total {}\nrelation_guard_allow_total {}\nrelation_guard_deny_total {}\nrelation_guard_conflicts_total {}\nrelation_guard_inflight {}\n",
                app.requests.load(Ordering::Relaxed),
                app.errors.load(Ordering::Relaxed),
                ALLOW.load(Ordering::Relaxed),
                DENY.load(Ordering::Relaxed),
                CONFLICTS.load(Ordering::Relaxed),
                128 - app.slots.available_permits()
            );
            output.push_str(&format!(
                "relation_guard_pool_size {}\nrelation_guard_pool_idle {}\n",
                app.pool.size(),
                app.pool.num_idle()
            ));
            output.push_str(&HTTP_LATENCY.render("relation_guard_http_seconds", 1e9));
            output.push_str(&authz_store::metrics::render());
            ([("content-type", "text/plain; version=0.0.4")], output).into_response()
        }
        _ => StatusCode::NOT_FOUND.into_response(),
    }
}
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .json()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();
    let pool = pool(&std::env::var("DATABASE_URL")?).await?;
    if !authz_store::ready(&pool).await {
        return Err("Database migrations are required".into());
    }
    let keys = auth::Config::load(&std::env::var("AUTHZ_KEYS_FILE")?, &pool).await?;
    let app = Arc::new(App {
        pool,
        keys,
        slots: Arc::new(Semaphore::new(128)),
        draining: AtomicBool::new(false),
        requests: AtomicU64::new(0),
        errors: AtomicU64::new(0),
        #[cfg(test)]
        before_commit: None,
    });
    let data = tokio::net::TcpListener::bind(
        std::env::var("AUTHZ_BIND").unwrap_or("127.0.0.1:8080".into()),
    )
    .await?;
    let operations = tokio::net::TcpListener::bind(
        std::env::var("AUTHZ_OPS_BIND").unwrap_or("127.0.0.1:9090".into()),
    )
    .await?;
    let (tx, rx) = watch::channel(false);
    let mut rx2 = rx.clone();
    let mut rx1 = rx;
    let data_service = axum::serve(
        data,
        Router::new()
            .fallback(any(endpoint))
            .with_state(app.clone()),
    )
    .with_graceful_shutdown(async move {
        let _ = rx1.changed().await;
    });
    let ops_service = axum::serve(
        operations,
        Router::new().fallback(any(ops)).with_state(app.clone()),
    )
    .with_graceful_shutdown(async move {
        let _ = rx2.changed().await;
    });
    let a = tokio::spawn(async move { data_service.await });
    let b = tokio::spawn(async move { ops_service.await });
    #[cfg(unix)]
    {
        let mut terminate =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
        tokio::select! {_ = tokio::signal::ctrl_c()=>{},_=terminate.recv()=>{}}
    }
    #[cfg(not(unix))]
    tokio::signal::ctrl_c().await?;
    app.draining.store(true, Ordering::Release);
    let _ = tx.send(true);
    let _ = tokio::time::timeout(Duration::from_secs(10), async {
        let _ = tokio::join!(a, b);
        app.pool.close().await;
    })
    .await;
    Ok(())
}

#[cfg(test)]
mod tests;

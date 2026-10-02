mod auth;
mod queries;
use authz_core::*;
use authz_store::{StoreError, service_pool as pool};
use axum::{
    Router,
    body::to_bytes,
    extract::{Request, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::any,
};
use serde_json::{Value, json};
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::Duration,
};
use tokio::sync::{Semaphore, watch};
use uuid::Uuid;

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
fn error(e: StoreError, id: Uuid) -> Response {
    let status = match e.code {
        "INVALID_REQUEST" | "DUPLICATE_OPERATION" => 400,
        "UNAUTHENTICATED" => 401,
        "FORBIDDEN" => 403,
        "NOT_FOUND" => 404,
        "METHOD_NOT_ALLOWED" => 405,
        "REVISION_CONFLICT"
        | "IDEMPOTENCY_CONFLICT"
        | "ENTITY_IN_USE"
        | "INVALID_HIERARCHY"
        | "LAST_ADMIN" => 409,
        "PAYLOAD_TOO_LARGE" => 413,
        "UNSUPPORTED_MEDIA_TYPE" => 415,
        "INVALID_RELATION" | "UNKNOWN_ENTITY" | "EVALUATION_LIMIT" => 422,
        "OVERLOADED" | "DATABASE_UNAVAILABLE" | "WRITE_OUTCOME_UNKNOWN" | "REVISION_EXHAUSTED" => {
            503
        }
        "DEADLINE_EXCEEDED" => 504,
        _ => 500,
    };
    let mut body = json!({"error":{"code":e.code,"message":e.code,"request_id":id}});
    if let Some(revision) = e.current_revision {
        body["error"]["details"] = json!({"current_revision":revision.to_string()});
    }
    (
        StatusCode::from_u16(status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR),
        axum::Json(body),
    )
        .into_response()
}
async fn endpoint(State(app): State<Arc<App>>, request: Request) -> Response {
    let _timer = HTTP_LATENCY.timer();
    let id = Uuid::new_v4();
    app.requests.fetch_add(1, Ordering::Relaxed);
    let result = dispatch(&app, request, id).await;
    let mut response = match result {
        Ok(r) => r,
        Err(e) => {
            app.errors.fetch_add(1, Ordering::Relaxed);
            tracing::warn!(request_id=%id, error_code=e.code,
                pool_size=app.pool.size(), pool_idle=app.pool.num_idle(),
                "request failed");
            if e.code == "REVISION_CONFLICT" {
                CONFLICTS.fetch_add(1, Ordering::Relaxed);
            }
            error(e, id)
        }
    };
    if let Ok(value) = id.to_string().parse() {
        response.headers_mut().insert("X-Request-Id", value);
    }
    response
}
async fn dispatch(app: &Arc<App>, request: Request, id: Uuid) -> Result<Response, StoreError> {
    let started = tokio::time::Instant::now();
    let deadline = started + Duration::from_secs(2);
    let _permit = app
        .slots
        .clone()
        .try_acquire_owned()
        .map_err(|_| StoreError::new("OVERLOADED"))?;
    if app.draining.load(Ordering::Acquire) {
        return Err(StoreError::new("OVERLOADED"));
    }
    let (parts, body) = request.into_parts();
    let pieces: Vec<_> = parts.uri.path().split('/').collect();
    if pieces.len() != 5 || pieces[1] != "v1" || pieces[2] != "tenants" {
        return Err(StoreError::new("NOT_FOUND"));
    }
    let route = pieces[4];
    let (method, scope) = match route {
        "write" => ("POST", "relations:write"),
        "check" | "batch-check" => ("POST", "check"),
        "revision" | "relationships" => ("GET", "relations:read"),
        "audit" => ("GET", "audit:read"),
        _ => return Err(StoreError::new("NOT_FOUND")),
    };
    if parts.method.as_str() != method {
        return Err(StoreError::new("METHOD_NOT_ALLOWED"));
    }
    let bytes = tokio::time::timeout_at(deadline, to_bytes(body, MAX_BODY))
        .await
        .map_err(|_| StoreError::new("DEADLINE_EXCEEDED"))?
        .map_err(|_| StoreError::new("PAYLOAD_TOO_LARGE"))?;
    if parts.headers.get_all("authorization").iter().count() != 1 {
        return Err(StoreError::new("UNAUTHENTICATED"));
    }
    let key = app
        .keys
        .authenticate(
            parts
                .headers
                .get("authorization")
                .and_then(|s| s.to_str().ok()),
        )
        .ok_or(StoreError::new("UNAUTHENTICATED"))?;
    let tenant = Uuid::parse_str(pieces[3]).map_err(|_| StoreError::new("INVALID_REQUEST"))?;
    if key.tenant_id != tenant || !key.scopes.contains(scope) {
        return Err(StoreError::new("FORBIDDEN"));
    }
    if parts.headers.get_all("x-actor-id").iter().count() != 1 {
        return Err(StoreError::new("UNAUTHENTICATED"));
    }
    let actor = parts
        .headers
        .get("x-actor-id")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| Id::new(v).ok())
        .ok_or(StoreError::new("UNAUTHENTICATED"))?;
    if method == "POST" {
        let ct = parts
            .headers
            .get("content-type")
            .and_then(|s| s.to_str().ok())
            .unwrap_or("")
            .to_ascii_lowercase();
        if !["application/json", "application/json; charset=utf-8"].contains(&ct.as_str())
            || parts.headers.contains_key("content-encoding")
        {
            return Err(StoreError::new("UNSUPPORTED_MEDIA_TYPE"));
        }
        if parts.uri.query().is_some() {
            return Err(StoreError::new("INVALID_REQUEST"));
        }
    }
    if route == "write" {
        let data: WriteRequest = strict_json(&bytes)?;
        if parts.headers.get_all("idempotency-key").iter().count() != 1 {
            return Err(StoreError::new("INVALID_REQUEST"));
        }
        let receipt_key = parts
            .headers
            .get("idempotency-key")
            .and_then(|s| s.to_str().ok())
            .ok_or(StoreError::new("INVALID_REQUEST"))?;
        let prepared = tokio::time::timeout_at(
            deadline,
            authz_store::prepare_actor_write(
                &app.pool,
                tenant,
                &key.key_id,
                receipt_key,
                &data,
                id,
                &actor,
            ),
        )
        .await
        .map_err(|_| StoreError::new("DEADLINE_EXCEEDED"))??;
        #[cfg(test)]
        if let Some(hook) = &app.before_commit {
            hook.entered.notify_one();
            hook.release.notified().await;
        }
        if tokio::time::Instant::now() >= deadline {
            return Err(StoreError::new("DEADLINE_EXCEEDED"));
        }
        // Once COMMIT starts, cancellation must not be reported as a known rollback.
        let commit = tokio::spawn(async move {
            let outcome = prepared.tx.commit().await;
            tracing::info!(request_id=%id,outcome=if outcome.is_ok(){"committed"}else{"unknown"},"write commit completed");
            outcome
                .map(|_| (prepared.response, prepared.replayed))
                .map_err(|_| StoreError::new("WRITE_OUTCOME_UNKNOWN"))
        });
        let (body, replayed) = tokio::time::timeout_at(deadline, commit)
            .await
            .map_err(|_| StoreError::new("WRITE_OUTCOME_UNKNOWN"))?
            .map_err(|_| StoreError::new("WRITE_OUTCOME_UNKNOWN"))??;
        return Ok((
            [(
                "Idempotency-Replayed",
                if replayed { "true" } else { "false" },
            )],
            axum::Json(body),
        )
            .into_response());
    }
    let work = async {
        let body = if route == "check" || route == "batch-check" {
            let data: BatchRequest = if route == "check" {
                strict_json::<CheckRequest>(&bytes)?.batch()
            } else {
                strict_json(&bytes)?
            };
            if data.explain && !key.scopes.contains("explain") {
                return Err(StoreError::new("FORBIDDEN"));
            }
            let (revision, result) =
                authz_store::management::check_as(&app.pool, tenant, &actor, &data).await?;
            for decision in &result.decisions {
                match decision.decision {
                    Verdict::Allow => {
                        ALLOW.fetch_add(1, Ordering::Relaxed);
                    }
                    Verdict::Deny => {
                        DENY.fetch_add(1, Ordering::Relaxed);
                    }
                }
            }
            let mut body = if route == "check" {
                serde_json::to_value(
                    result
                        .decisions
                        .first()
                        .ok_or(StoreError::new("INTERNAL_ERROR"))?,
                )
                .map_err(|_| StoreError::new("INTERNAL_ERROR"))?
            } else {
                json!({"results":result.decisions})
            };
            body["evaluated_revision"] = json!(revision.to_string());
            body["model_version"] = json!(MODEL);
            body["request_id"] = json!(id);
            body
        } else {
            queries::get(&app.pool, tenant, &actor, route, parts.uri.query(), id).await?
        };
        Ok::<Value, StoreError>(body)
    };
    let body = tokio::time::timeout_at(deadline, work)
        .await
        .map_err(|_| StoreError::new("DEADLINE_EXCEEDED"))??;
    Ok(axum::Json(body).into_response())
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

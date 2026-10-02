use crate::{ALLOW, App, CONFLICTS, DENY, HTTP_LATENCY, queries};
use authz_core::*;
use authz_store::StoreError;
use axum::{
    body::to_bytes,
    extract::{Request, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use serde_json::{Value, json};
use std::{
    sync::{Arc, atomic::Ordering},
    time::Duration,
};
use uuid::Uuid;

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
pub(crate) async fn endpoint(State(app): State<Arc<App>>, request: Request) -> Response {
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
    let authorization = single_header(&parts.headers, "authorization", "UNAUTHENTICATED")?;
    let key = app
        .keys
        .authenticate(Some(authorization))
        .ok_or(StoreError::new("UNAUTHENTICATED"))?;
    let tenant = Uuid::parse_str(pieces[3]).map_err(|_| StoreError::new("INVALID_REQUEST"))?;
    if key.tenant_id != tenant || !key.scopes.contains(scope) {
        return Err(StoreError::new("FORBIDDEN"));
    }
    let actor = Id::new(single_header(
        &parts.headers,
        "x-actor-id",
        "UNAUTHENTICATED",
    )?)
    .map_err(|_| StoreError::new("UNAUTHENTICATED"))?;
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
        let receipt_key = single_header(&parts.headers, "idempotency-key", "INVALID_REQUEST")?;
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

fn single_header<'a>(
    headers: &'a HeaderMap,
    name: &'static str,
    error: &'static str,
) -> Result<&'a str, StoreError> {
    if headers.get_all(name).iter().count() != 1 {
        return Err(StoreError::new(error));
    }
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .ok_or(StoreError::new(error))
}

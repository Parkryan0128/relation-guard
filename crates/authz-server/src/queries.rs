use authz_core::*;
use authz_store::{StoreError, begin_read};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sqlx::Row;
use std::collections::BTreeMap;
use uuid::Uuid;
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Cursor {
    v: u8,
    tenant_id: Uuid,
    resource_kind: Kind,
    resource_id: Id,
    last: [String; 4],
}
fn invalid() -> StoreError {
    StoreError::new("INVALID_REQUEST")
}
pub async fn get(
    pool: &sqlx::PgPool,
    tenant: Uuid,
    route: &str,
    query: Option<&str>,
    id: Uuid,
) -> Result<Value, StoreError> {
    let pairs: Vec<(String, String)> =
        serde_urlencoded::from_str(query.unwrap_or("")).map_err(|_| invalid())?;
    let mut params = BTreeMap::new();
    for (k, v) in pairs {
        let valid = match route {
            "relationships" => {
                ["resource_kind", "resource_id", "limit", "after"].contains(&k.as_str())
            }
            "audit" => ["after_revision", "limit"].contains(&k.as_str()),
            _ => false,
        };
        if !valid || params.insert(k, v).is_some() {
            return Err(invalid());
        }
    }
    let limit = match params.get("limit") {
        Some(s) => s.parse::<i64>().map_err(|_| invalid())?,
        None => 50,
    };
    if !(1..=100).contains(&limit) {
        return Err(invalid());
    }
    let mut snapshot = begin_read(pool, tenant).await?;
    let response = match route {
        "revision" => {
            json!({"revision":snapshot.revision.to_string(),"model_version":MODEL,"request_id":id})
        }
        "relationships" => {
            let kind: Kind =
                serde_json::from_value(json!(params.get("resource_kind").ok_or_else(invalid)?))
                    .map_err(|_| invalid())?;
            let resource_id = Id::new(params.get("resource_id").ok_or_else(invalid)?.clone())?;
            let last = if let Some(encoded) = params.get("after") {
                if encoded.len() > 2048 {
                    return Err(invalid());
                }
                let cursor: Cursor =
                    strict_json(&URL_SAFE_NO_PAD.decode(encoded).map_err(|_| invalid())?)?;
                if cursor.v != 1
                    || cursor.tenant_id != tenant
                    || cursor.resource_kind != kind
                    || cursor.resource_id != resource_id
                {
                    return Err(invalid());
                }
                let relation: Relation =
                    serde_json::from_value(json!(cursor.last[0])).map_err(|_| invalid())?;
                let subject_kind: Kind =
                    serde_json::from_value(json!(cursor.last[1])).map_err(|_| invalid())?;
                Edge {
                    resource: Entity {
                        kind,
                        id: resource_id.clone(),
                    },
                    relation,
                    subject: Entity {
                        kind: subject_kind,
                        id: Id::new(cursor.last[2].clone())?,
                    },
                    subject_relation: cursor.last[3].clone(),
                }
                .validate()
                .map_err(|_| invalid())?;
                cursor.last
            } else {
                std::array::from_fn(|_| String::new())
            };
            let rows=sqlx::query("SELECT relation,subject_kind,subject_id,subject_relation FROM relationships WHERE tenant_id=$1 AND resource_kind=$2 AND resource_id=$3 AND (relation,subject_kind,subject_id,subject_relation)>($4,$5,$6,$7) ORDER BY relation,subject_kind,subject_id,subject_relation LIMIT $8")
                .bind(tenant).bind(kind.as_str()).bind(resource_id.as_str()).bind(&last[0]).bind(&last[1]).bind(&last[2]).bind(&last[3]).bind(limit+1).fetch_all(&mut *snapshot.tx).await?;
            let more = rows.len() > limit as usize;
            let mut edges = Vec::new();
            let mut tail = None;
            for row in rows.into_iter().take(limit as usize) {
                let last: [String; 4] = [
                    row.try_get("relation")?,
                    row.try_get("subject_kind")?,
                    row.try_get("subject_id")?,
                    row.try_get("subject_relation")?,
                ];
                edges.push(json!({"resource":{"kind":kind,"id":resource_id},"relation":last[0],"subject":{"kind":last[1],"id":last[2]},"subject_relation":last[3]}));
                tail = Some(last);
            }
            let cursor =
                if more {
                    let c = Cursor {
                        v: 1,
                        tenant_id: tenant,
                        resource_kind: kind,
                        resource_id,
                        last: tail.ok_or(StoreError::new("INTERNAL_ERROR"))?,
                    };
                    Some(URL_SAFE_NO_PAD.encode(
                        serde_json::to_vec(&c).map_err(|_| StoreError::new("INTERNAL_ERROR"))?,
                    ))
                } else {
                    None
                };
            json!({"relationships":edges,"evaluated_revision":snapshot.revision.to_string(),"next_cursor":cursor,"request_id":id})
        }
        "audit" => {
            let after: Revision = serde_json::from_value(json!(
                params
                    .get("after_revision")
                    .map(String::as_str)
                    .unwrap_or("0")
            ))
            .map_err(|_| invalid())?;
            let rows=sqlx::query("SELECT revision,key_id,request_id,to_char(recorded_at AT TIME ZONE 'UTC','YYYY-MM-DD\"T\"HH24:MI:SS.US\"Z\"') AS recorded_at,effective_changes FROM audit_events WHERE tenant_id=$1 AND revision>$2 ORDER BY revision LIMIT $3").bind(tenant).bind(after.0).bind(limit+1).fetch_all(&mut *snapshot.tx).await?;
            let more = rows.len() > limit as usize;
            let mut events = Vec::new();
            let mut tail = None;
            for row in rows.into_iter().take(limit as usize) {
                let revision = row.try_get::<i64, _>("revision")?.to_string();
                tail = Some(revision.clone());
                events.push(json!({"revision":revision,"key_id":row.try_get::<String,_>("key_id")?,"request_id":row.try_get::<Uuid,_>("request_id")?,"recorded_at":row.try_get::<String,_>("recorded_at")?,"effective_changes":row.try_get::<Value,_>("effective_changes")?}));
            }
            json!({"events":events,"evaluated_revision":snapshot.revision.to_string(),"next_after_revision":if more{tail}else{None},"request_id":id})
        }
        _ => return Err(StoreError::new("NOT_FOUND")),
    };
    snapshot.tx.commit().await?;
    Ok(response)
}

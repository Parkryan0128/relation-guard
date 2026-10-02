use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use subtle::ConstantTimeEq;
use uuid::Uuid;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub version: u8,
    pub keys: Vec<Key>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Key {
    pub key_id: String,
    pub tenant_id: Uuid,
    pub scopes: BTreeSet<String>,
    pub secret_sha256: String,
}
impl Config {
    pub async fn load(path: &str, pool: &sqlx::PgPool) -> Result<Self, Box<dyn std::error::Error>> {
        let config: Self = authz_core::strict_json(&std::fs::read(path)?)?;
        let mut ids = BTreeSet::new();
        if config.version != 1 || config.keys.is_empty() {
            return Err("Invalid key configuration".into());
        }
        for key in &config.keys {
            if key.key_id.is_empty()
                || key.key_id.len() > 64
                || !key
                    .key_id
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
                || !ids.insert(&key.key_id)
                || key.secret_sha256.len() != 64
                || !key
                    .secret_sha256
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
                || key.scopes.is_empty()
                || key.scopes.iter().any(|s| {
                    ![
                        "check",
                        "explain",
                        "relations:read",
                        "relations:write",
                        "audit:read",
                    ]
                    .contains(&s.as_str())
                })
            {
                return Err("Invalid key configuration".into());
            }
            let exists: bool =
                sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM tenants WHERE id=$1)")
                    .bind(key.tenant_id)
                    .fetch_one(pool)
                    .await?;
            if !exists {
                return Err("Key references unknown tenant".into());
            }
        }
        Ok(config)
    }
    pub fn authenticate(&self, header: Option<&str>) -> Option<&Key> {
        let token = header?.strip_prefix("Bearer ")?;
        let (id, secret) = token.split_once('.')?;
        if secret.len() != 43 {
            return None;
        }
        let raw = URL_SAFE_NO_PAD.decode(secret).ok()?;
        if raw.len() != 32 {
            return None;
        }
        let key = self.keys.iter().find(|k| k.key_id == id)?;
        let actual = hex::encode(Sha256::digest(raw));
        bool::from(actual.as_bytes().ct_eq(key.secret_sha256.as_bytes())).then_some(key)
    }
}

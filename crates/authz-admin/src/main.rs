use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use sha2::{Digest, Sha256};
use uuid::Uuid;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("key") if args.len() == 3 => {
            let tenant = Uuid::parse_str(&args[1])?;
            let id = &args[2];
            if id.is_empty()
                || id.len() > 64
                || !id
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
            {
                return Err("Invalid key ID".into());
            }
            let mut secret = [0u8; 32];
            getrandom::fill(&mut secret).map_err(|_| "OS random source failed")?;
            println!(
                "{}",
                serde_json::json!({"token":format!("{id}.{}",URL_SAFE_NO_PAD.encode(secret)),"config_entry":{"key_id":id,"tenant_id":tenant,"scopes":["check","explain","relations:read","relations:write","audit:read"],"secret_sha256":hex::encode(Sha256::digest(secret))}})
            );
        }
        Some(
            "migrate" | "tenant" | "cleanup-receipts" | "runtime-role" | "bootstrap-admin"
            | "recover-admin",
        ) => {
            let pool = authz_store::pool(&std::env::var("DATABASE_URL")?).await?;
            match args[0].as_str() {
                "migrate" if args.len() == 1 => authz_store::MIGRATOR.run(&pool).await?,
                "tenant" if args.len() == 2 => {
                    authz_store::provision(&pool, Uuid::parse_str(&args[1])?).await?
                }
                "bootstrap-admin" | "recover-admin" if args.len() == 3 => {
                    authz_store::management::bootstrap_admin(
                        &pool,
                        Uuid::parse_str(&args[1])?,
                        &authz_core::Id::new(args[2].clone())?,
                        args[0] == "recover-admin",
                    )
                    .await?;
                }
                "runtime-role" if args.len() == 1 => {
                    let password = std::env::var("AUTHZ_RUNTIME_PASSWORD")?;
                    if password.len() < 32 || !password.bytes().all(|b| b.is_ascii_hexdigit()) {
                        return Err("Runtime password must be at least 32 hex characters".into());
                    }
                    let mut tx = pool.begin().await?;
                    sqlx::query("DO $$ BEGIN IF NOT EXISTS(SELECT 1 FROM pg_roles WHERE rolname='relation_guard_runtime') THEN CREATE ROLE relation_guard_runtime LOGIN; END IF; END $$").execute(&mut *tx).await?;
                    // Password is constrained to hex above, so no SQL metacharacters are accepted.
                    sqlx::query(&format!(
                        "ALTER ROLE relation_guard_runtime PASSWORD '{password}'"
                    ))
                    .execute(&mut *tx)
                    .await?;
                    for sql in [
                        "GRANT USAGE ON SCHEMA public TO relation_guard_runtime",
                        "GRANT SELECT ON tenants,_sqlx_migrations TO relation_guard_runtime",
                        "GRANT UPDATE(revision) ON tenants TO relation_guard_runtime",
                        "GRANT SELECT,INSERT,UPDATE,DELETE ON entities,relationships TO relation_guard_runtime",
                        "GRANT SELECT,INSERT ON audit_events,mutation_receipts TO relation_guard_runtime",
                    ] {
                        sqlx::query(sql).execute(&mut *tx).await?;
                    }
                    tx.commit().await?;
                }
                "cleanup-receipts" if args.len() == 1 => {
                    println!("{}", authz_store::cleanup_receipts(&pool).await?)
                }
                _ => return Err("Invalid arguments".into()),
            }
            pool.close().await;
        }
        _ => {
            return Err(
                "Usage: authz-admin migrate | tenant UUID | bootstrap-admin UUID USER_ID | recover-admin UUID USER_ID | key UUID KEY_ID | runtime-role | cleanup-receipts"
                    .into(),
            );
        }
    }
    Ok(())
}

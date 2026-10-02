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
        Some("migrate" | "tenant" | "cleanup-receipts") => {
            let pool = authz_store::pool(&std::env::var("DATABASE_URL")?).await?;
            match args[0].as_str() {
                "migrate" if args.len() == 1 => authz_store::MIGRATOR.run(&pool).await?,
                "tenant" if args.len() == 2 => {
                    authz_store::provision(&pool, Uuid::parse_str(&args[1])?).await?
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
                "Usage: authz-admin migrate | tenant UUID | key UUID KEY_ID | cleanup-receipts"
                    .into(),
            );
        }
    }
    Ok(())
}

pub const SCHEMA_VERSION: i64 = 1;
pub static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("../../migrations");

use sqlx::{Connection, Executor, Row};
#[tokio::test]
async fn m0_real_postgres_constraints() {
    let url = std::env::var("TEST_DATABASE_URL")
        .expect("TEST_DATABASE_URL required; never silently skip PG tests");
    let mut c = sqlx::PgConnection::connect(&url).await.unwrap();
    let schema = format!("t_{}", uuid::Uuid::new_v4().simple());
    c.execute(format!("CREATE SCHEMA {schema}").as_str())
        .await
        .unwrap();
    c.execute(format!("SET search_path TO {schema}").as_str())
        .await
        .unwrap();
    authz_store::MIGRATOR.run(&mut c).await.unwrap();
    authz_store::MIGRATOR.run(&mut c).await.unwrap();
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM _sqlx_migrations")
        .fetch_one(&mut c)
        .await
        .unwrap();
    assert_eq!(count, 1);
    c.execute("INSERT INTO tenants(id) VALUES ('11111111-1111-4111-8111-111111111111'),('22222222-2222-4222-8222-222222222222')").await.unwrap();
    c.execute("INSERT INTO entities SELECT '11111111-1111-4111-8111-111111111111', k,i FROM (VALUES ('user','u'),('folder','f'),('folder','g'),('document','d')) v(k,i)").await.unwrap();
    for (sql, code) in [
        (
            "INSERT INTO entities VALUES ('11111111-1111-4111-8111-111111111111','user','u')",
            "23505",
        ),
        (
            "INSERT INTO entities VALUES ('11111111-1111-4111-8111-111111111111','unknown','x')",
            "23514",
        ),
        (
            "INSERT INTO relationships VALUES ('22222222-2222-4222-8222-222222222222','document','d','viewer','user','u','')",
            "23503",
        ),
        (
            "INSERT INTO relationships VALUES ('11111111-1111-4111-8111-111111111111','document','d','member','user','u','')",
            "23514",
        ),
    ] {
        let e = c.execute(sql).await.unwrap_err();
        assert_eq!(e.as_database_error().unwrap().code().as_deref(), Some(code));
    }
    c.execute("INSERT INTO relationships VALUES ('11111111-1111-4111-8111-111111111111','document','d','parent','folder','f','')").await.unwrap();
    let e=c.execute("INSERT INTO relationships VALUES ('11111111-1111-4111-8111-111111111111','document','d','parent','folder','g','')").await.unwrap_err();
    assert_eq!(
        e.as_database_error().unwrap().code().as_deref(),
        Some("23505")
    );
    let bad_key = c.execute("INSERT INTO mutation_receipts (tenant_id,key_id,idempotency_key,request_hash,response) VALUES ('11111111-1111-4111-8111-111111111111','bad.key','k',decode(repeat('00',32),'hex'),'{}')").await.unwrap_err();
    assert_eq!(
        bad_key.as_database_error().unwrap().code().as_deref(),
        Some("23514")
    );
    // SQL constraints cannot detect indirect cycles; the service must validate them.
    c.execute("BEGIN").await.unwrap();
    c.execute("INSERT INTO relationships VALUES ('11111111-1111-4111-8111-111111111111','folder','f','parent','folder','g',''), ('11111111-1111-4111-8111-111111111111','folder','g','parent','folder','f','')").await.unwrap();
    c.execute("ROLLBACK").await.unwrap();
    let n: i64 = sqlx::query("SELECT count(*) AS n FROM relationships")
        .fetch_one(&mut c)
        .await
        .unwrap()
        .get("n");
    assert_eq!(n, 1);
    c.execute(format!("DROP SCHEMA {schema} CASCADE").as_str())
        .await
        .unwrap();
}

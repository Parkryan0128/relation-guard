use sqlx::{Connection, Executor, Row};
#[tokio::test]
async fn real_postgres_constraints() {
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
    assert_eq!(count, 2);
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

#[tokio::test]
async fn managed_upgrade_preserves_legacy_data_without_implicit_admins() {
    let url = std::env::var("TEST_DATABASE_URL").unwrap();
    let mut c = sqlx::PgConnection::connect(&url).await.unwrap();
    let schema = format!("upgrade_{}", uuid::Uuid::new_v4().simple());
    c.execute(format!("CREATE SCHEMA {schema}").as_str())
        .await
        .unwrap();
    c.execute(format!("SET search_path TO {schema}").as_str())
        .await
        .unwrap();
    let initial = sqlx::migrate::Migrator {
        migrations: std::borrow::Cow::Owned(
            authz_store::MIGRATOR.iter().take(1).cloned().collect(),
        ),
        ignore_missing: false,
        locking: true,
        no_tx: false,
    };
    initial.run(&mut c).await.unwrap();
    c.execute("INSERT INTO tenants(id,revision) VALUES('11111111-1111-4111-8111-111111111111',1); INSERT INTO entities VALUES('11111111-1111-4111-8111-111111111111','user','legacy'); INSERT INTO audit_events(tenant_id,revision,key_id,request_id,effective_changes) VALUES('11111111-1111-4111-8111-111111111111',1,'old','22222222-2222-4222-8222-222222222222','[{\"op\":\"put_entity\",\"entity\":{\"kind\":\"user\",\"id\":\"legacy\"}}]'); INSERT INTO mutation_receipts(tenant_id,key_id,idempotency_key,request_hash,response) VALUES('11111111-1111-4111-8111-111111111111','old','same',decode(repeat('00',32),'hex'),'{\"model_version\":\"org-doc-v1\"}')").await.unwrap();
    authz_store::MIGRATOR.run(&mut c).await.unwrap();
    assert!(
        !sqlx::query_scalar::<_, bool>("SELECT org_admin FROM entities")
            .fetch_one(&mut c)
            .await
            .unwrap()
    );
    let row = sqlx::query("SELECT actor_id,authorization_basis FROM audit_events")
        .fetch_one(&mut c)
        .await
        .unwrap();
    assert_eq!(row.get::<Option<String>, _>("actor_id"), None);
    assert_eq!(
        row.get::<serde_json::Value, _>("authorization_basis")["kind"],
        "legacy_operator"
    );
    assert_eq!(
        sqlx::query_scalar::<_, String>("SELECT actor_id FROM mutation_receipts")
            .fetch_one(&mut c)
            .await
            .unwrap(),
        ""
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT revision FROM tenants")
            .fetch_one(&mut c)
            .await
            .unwrap(),
        1
    );
    let error=c.execute("INSERT INTO entities VALUES('11111111-1111-4111-8111-111111111111','folder','bad',true)").await.unwrap_err();
    assert_eq!(
        error.as_database_error().unwrap().code().as_deref(),
        Some("23514")
    );
    c.execute("INSERT INTO entities VALUES('11111111-1111-4111-8111-111111111111','folder','f',false); INSERT INTO relationships VALUES('11111111-1111-4111-8111-111111111111','folder','f','manager','user','legacy','')").await.unwrap();
}

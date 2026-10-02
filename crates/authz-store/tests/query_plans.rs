//! Run alone after enabling pg_stat_statements in an isolated PostgreSQL instance.
use authz_core::*;
use authz_store::*;
use serde_json::{Value, json};
use sqlx::Row;
use uuid::Uuid;
#[tokio::test]
async fn m2_measured_query_count_and_explain() {
    let p = pool(&std::env::var("TEST_DATABASE_URL").unwrap())
        .await
        .unwrap();
    MIGRATOR.run(&p).await.unwrap();
    sqlx::query("CREATE EXTENSION IF NOT EXISTS pg_stat_statements")
        .execute(&p)
        .await
        .unwrap();
    let t = Uuid::new_v4();
    provision(&p, t).await.unwrap();
    let fixture: Value =
        serde_json::from_str(include_str!("../../../spec/examples/contracts.json")).unwrap();
    let write_request: WriteRequest =
        serde_json::from_value(fixture["write"]["request"].clone()).unwrap();
    write(&p, t, "test", "seed", &write_request, Uuid::new_v4())
        .await
        .unwrap();
    sqlx::query("INSERT INTO entities SELECT $1,'document','q'||i FROM generate_series(1,100) i")
        .bind(t)
        .execute(&p)
        .await
        .unwrap();
    sqlx::query("INSERT INTO relationships SELECT $1,'document','q'||i,'parent','folder','f10','' FROM generate_series(1,100) i").bind(t).execute(&p).await.unwrap();
    let mut evidence = Vec::new();
    for size in [1, 100] {
        let request = BatchRequest {
            subject: Entity {
                kind: Kind::User,
                id: Id::new("u42").unwrap(),
            },
            checks: (1..=size)
                .map(|i| Check {
                    resource: Entity {
                        kind: Kind::Document,
                        id: Id::new(format!("q{i}")).unwrap(),
                    },
                    action: Action::View,
                })
                .collect(),
            explain: false,
        };
        sqlx::query("SELECT pg_stat_statements_reset()")
            .execute(&p)
            .await
            .unwrap();
        let (_, evaluation) = check(&p, t, &request).await.unwrap();
        assert!(
            evaluation
                .decisions
                .iter()
                .all(|d| d.decision == Verdict::Allow)
        );
        let rows=sqlx::query("SELECT query,calls FROM pg_stat_statements WHERE dbid=(SELECT oid FROM pg_database WHERE datname=current_database()) AND (query LIKE 'SELECT revision FROM tenants WHERE id=%' OR query LIKE 'WITH RECURSIVE requested AS%' OR query LIKE 'SELECT jsonb_build_object(% AS edge%') ORDER BY query").fetch_all(&p).await.unwrap();
        let calls: i64 = rows.iter().map(|r| r.get::<i64, _>("calls")).sum();
        assert_eq!(calls, 3, "must measure all three data SELECTs");
        let resources: Vec<_> = request.checks.iter().map(|c| &c.resource).collect();
        let ancestors = sqlx::query(&format!(
            "EXPLAIN (ANALYZE,BUFFERS,FORMAT JSON) {}",
            include_str!("../sql/ancestors.sql")
        ))
        .bind(t)
        .bind(json!(resources))
        .bind("u42")
        .fetch_one(&p)
        .await
        .unwrap()
        .get::<Value, _>(0);
        let mut nodes = resources.into_iter().cloned().collect::<Vec<_>>();
        nodes.push(Entity {
            kind: Kind::Folder,
            id: Id::new("f10").unwrap(),
        });
        let grants = sqlx::query(&format!(
            "EXPLAIN (ANALYZE,BUFFERS,FORMAT JSON) {}",
            include_str!("../sql/grants.sql")
        ))
        .bind(t)
        .bind(json!(nodes))
        .bind("u42")
        .fetch_one(&p)
        .await
        .unwrap()
        .get::<Value, _>(0);
        evidence.push(
            json!({"items":size,"data_select_calls":calls,"ancestors":ancestors,"grants":grants}),
        );
    }
    if let Ok(path) = std::env::var("QUERY_PLAN_OUTPUT") {
        std::fs::write(path, serde_json::to_vec_pretty(&evidence).unwrap()).unwrap();
    }
}

use super::*;
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use sha2::{Digest, Sha256};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
struct Harness {
    app: Arc<App>,
    address: std::net::SocketAddr,
    tenant: Uuid,
    token: String,
    task: tokio::task::JoinHandle<()>,
}
impl Drop for Harness {
    fn drop(&mut self) {
        self.task.abort();
    }
}
async fn harness() -> Harness {
    harness_with_hook(None).await
}
async fn harness_with_hook(hook: Option<Arc<CommitBarrier>>) -> Harness {
    let url = std::env::var("TEST_DATABASE_URL").expect("isolated database required");
    let pool = pool(&url).await.unwrap();
    authz_store::MIGRATOR.run(&pool).await.unwrap();
    let tenant = Uuid::new_v4();
    authz_store::provision(&pool, tenant).await.unwrap();
    sqlx::query(
        "INSERT INTO entities(tenant_id,kind,id,org_admin) VALUES($1,'user','test-admin',true)",
    )
    .bind(tenant)
    .execute(&pool)
    .await
    .unwrap();
    let secret = [42u8; 32];
    let keys=strict_json::<auth::Config>(&serde_json::to_vec(&json!({"version":1,"keys":[{"key_id":"test","tenant_id":tenant,"scopes":["check","explain","relations:write","relations:read"],"secret_sha256":hex::encode(Sha256::digest(secret))}]})).unwrap()).unwrap();
    let app = Arc::new(App {
        pool,
        keys,
        slots: Arc::new(Semaphore::new(128)),
        draining: AtomicBool::new(false),
        requests: AtomicU64::new(0),
        errors: AtomicU64::new(0),
        before_commit: hook,
    });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let state = app.clone();
    let task = tokio::spawn(async move {
        axum::serve(
            listener,
            Router::new().fallback(any(endpoint)).with_state(state),
        )
        .await
        .unwrap();
    });
    Harness {
        app,
        address,
        tenant,
        token: format!("test.{}", URL_SAFE_NO_PAD.encode(secret)),
        task,
    }
}
async fn request(h: &Harness, path: &str, body: Option<Value>) -> (u16, Value) {
    let mut connection = tokio::net::TcpStream::connect(h.address).await.unwrap();
    let method = if body.is_some() { "POST" } else { "GET" };
    let body = body.map(|b| b.to_string()).unwrap_or_default();
    let text = format!(
        "{method} /v1/tenants/{}/{path} HTTP/1.1\r\nHost: localhost\r\nAuthorization: Bearer {}\r\nX-Actor-Id: test-admin\r\nContent-Type: application/json\r\nIdempotency-Key: test\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        h.tenant,
        h.token,
        body.len()
    );
    connection.write_all(text.as_bytes()).await.unwrap();
    let mut response = Vec::new();
    connection.read_to_end(&mut response).await.unwrap();
    let response = String::from_utf8(response).unwrap();
    let (head, body) = response.split_once("\r\n\r\n").unwrap();
    (
        head.split_whitespace().nth(1).unwrap().parse().unwrap(),
        serde_json::from_str(body).unwrap(),
    )
}
#[tokio::test]
async fn m3_pool_lock_and_recovery() {
    let h = harness().await;
    let mut held = Vec::new();
    for _ in 0..10 {
        held.push(h.app.pool.acquire().await.unwrap());
    }
    let timeouts_before = authz_store::metrics::POOL_ACQUIRE_TIMEOUTS.load(Ordering::Relaxed);
    let (status, body) = request(&h, "revision", None).await;
    assert!(authz_store::metrics::POOL_ACQUIRE_TIMEOUTS.load(Ordering::Relaxed) > timeouts_before);
    assert_eq!(h.app.pool.size(), 10);
    assert_eq!(h.app.pool.num_idle(), 0);
    assert_eq!(status, 503);
    assert_eq!(body["error"]["code"], "OVERLOADED");
    drop(held);
    let mut lock = h.app.pool.begin().await.unwrap();
    sqlx::query("SELECT revision FROM tenants WHERE id=$1 FOR UPDATE")
        .bind(h.tenant)
        .fetch_one(&mut *lock)
        .await
        .unwrap();
    let write = json!({"expected_revision":"0","operations":[{"op":"put_entity","entity":{"kind":"user","id":"u"}}]});
    let (status, body) = request(&h, "write", Some(write.clone())).await;
    assert_eq!(status, 503);
    assert_eq!(body["error"]["code"], "OVERLOADED");
    lock.rollback().await.unwrap();
    assert_eq!(request(&h, "write", Some(write)).await.0, 200);
    assert_eq!(request(&h, "revision", None).await.1["revision"], "1");
}
#[tokio::test]
async fn m3_real_tcp_inflight_limit_and_deadline() {
    let h = harness().await;
    let mut connections = Vec::new();
    for _ in 0..128 {
        let mut connection = tokio::net::TcpStream::connect(h.address).await.unwrap();
        connection.write_all(format!("POST /v1/tenants/{}/check HTTP/1.1\r\nHost: localhost\r\nContent-Length: 100\r\nConnection: close\r\n\r\n",h.tenant).as_bytes()).await.unwrap();
        connections.push(connection);
    }
    tokio::time::timeout(Duration::from_secs(1), async {
        while h.app.slots.available_permits() != 0 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    let (status, body) = request(&h, "revision", None).await;
    assert_eq!(status, 503);
    assert_eq!(body["error"]["code"], "OVERLOADED");
    let mut first = connections.remove(0);
    let mut bytes = Vec::new();
    first.read_to_end(&mut bytes).await.unwrap();
    let text = String::from_utf8_lossy(&bytes);
    assert!(text.contains("504"));
    assert!(text.contains("DEADLINE_EXCEEDED"));
    drop(connections);
    drop(first);
    tokio::time::timeout(Duration::from_secs(1), async {
        while h.app.slots.available_permits() != 128 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert_eq!(request(&h, "revision", None).await.0, 200);
}
#[tokio::test]
async fn m3_batch_failure_is_not_partial_allow() {
    let h = harness().await;
    let mut ops = vec![
        json!({"op":"put_entity","entity":{"kind":"user","id":"u"}}),
        json!({"op":"put_entity","entity":{"kind":"document","id":"d"}}),
    ];
    for i in 0..98 {
        ops.push(json!({"op":"put_entity","entity":{"kind":"team","id":format!("t{i}")}}));
    }
    authz_store::write(
        &h.app.pool,
        h.tenant,
        "test",
        "seed",
        &strict_json(
            &serde_json::to_vec(&json!({"expected_revision":"0","operations":ops})).unwrap(),
        )
        .unwrap(),
        Uuid::new_v4(),
    )
    .await
    .unwrap();
    sqlx::query("INSERT INTO relationships SELECT $1,'team','t'||i,'member','user','u','' FROM generate_series(0,97) i").bind(h.tenant).execute(&h.app.pool).await.unwrap();
    sqlx::query("INSERT INTO relationships SELECT $1,'document','d',r,'team','t'||i,'member' FROM generate_series(0,97) i CROSS JOIN unnest(ARRAY['editor','viewer']) r").bind(h.tenant).execute(&h.app.pool).await.unwrap();
    let check = json!({"subject":{"kind":"user","id":"u"},"resource":{"kind":"document","id":"d"},"action":"view"});
    assert_eq!(
        request(&h, "check", Some(check)).await.1["decision"],
        "allow"
    );
    let batch = json!({"subject":{"kind":"user","id":"u"},"checks":vec![json!({"resource":{"kind":"document","id":"d"},"action":"view"});100]});
    let (status, body) = request(&h, "batch-check", Some(batch)).await;
    assert_eq!(status, 422);
    assert_eq!(body["error"]["code"], "EVALUATION_LIMIT");
    assert!(body.get("results").is_none());
}

#[tokio::test]
async fn m3_statement_timeout_and_recovery() {
    let h = harness().await;
    let name = format!("slow_{}", h.tenant.simple());
    sqlx::query(&format!("CREATE FUNCTION {name}() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN IF NEW.tenant_id='{}'::uuid THEN PERFORM pg_sleep(2); END IF; RETURN NEW; END $$",h.tenant)).execute(&h.app.pool).await.unwrap();
    sqlx::query(&format!(
        "CREATE TRIGGER {name} BEFORE INSERT ON entities FOR EACH ROW EXECUTE FUNCTION {name}() "
    ))
    .execute(&h.app.pool)
    .await
    .unwrap();
    let write = json!({"expected_revision":"0","operations":[{"op":"put_entity","entity":{"kind":"user","id":"slow"}}]});
    let (status, body) = request(&h, "write", Some(write.clone())).await;
    assert_eq!(status, 504);
    assert_eq!(body["error"]["code"], "DEADLINE_EXCEEDED");
    sqlx::query(&format!("DROP TRIGGER {name} ON entities"))
        .execute(&h.app.pool)
        .await
        .unwrap();
    sqlx::query(&format!("DROP FUNCTION {name}()"))
        .execute(&h.app.pool)
        .await
        .unwrap();
    assert_eq!(request(&h, "revision", None).await.1["revision"], "0");
    assert_eq!(request(&h, "write", Some(write)).await.0, 200);
}
#[tokio::test]
async fn m3_invalid_startup_configuration() {
    let h = harness().await;
    let base = json!({"version":1,"keys":[{"key_id":"valid","tenant_id":h.tenant,"scopes":["check"],"secret_sha256":hex::encode(Sha256::digest([42u8;32]))}]});
    let mut cases = Vec::new();
    for (key, value) in [
        ("key_id", json!("bad.dot")),
        ("scopes", json!([])),
        ("scopes", json!(["check", "check"])),
        ("scopes", json!(["unknown"])),
        ("secret_sha256", json!("A".repeat(64))),
        ("tenant_id", json!(Uuid::new_v4())),
    ] {
        let mut c = base.clone();
        c["keys"][0][key] = value;
        cases.push(c);
    }
    let mut duplicate = base.clone();
    duplicate["keys"] = json!([base["keys"][0], base["keys"][0]]);
    cases.push(duplicate);
    for value in cases {
        let path = std::env::temp_dir().join(format!("rg-config-{}.json", Uuid::new_v4()));
        std::fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
        assert!(
            auth::Config::load(path.to_str().unwrap(), &h.app.pool)
                .await
                .is_err()
        );
        std::fs::remove_file(path).unwrap();
    }
}

pub(super) struct CommitBarrier {
    pub entered: tokio::sync::Notify,
    pub release: tokio::sync::Notify,
}
#[tokio::test]
async fn m3_http_disconnect_before_commit() {
    let hook = Arc::new(CommitBarrier {
        entered: tokio::sync::Notify::new(),
        release: tokio::sync::Notify::new(),
    });
    let h = harness_with_hook(Some(hook.clone())).await;
    let mut connection = tokio::net::TcpStream::connect(h.address).await.unwrap();
    let body=json!({"expected_revision":"0","operations":[{"op":"put_entity","entity":{"kind":"user","id":"disconnect"}}]}).to_string();
    connection.write_all(format!("POST /v1/tenants/{}/write HTTP/1.1\r\nHost: localhost\r\nAuthorization: Bearer {}\r\nX-Actor-Id: test-admin\r\nContent-Type: application/json\r\nIdempotency-Key: disconnect\r\nContent-Length: {}\r\n\r\n{}",h.tenant,h.token,body.len(),body).as_bytes()).await.unwrap();
    tokio::time::timeout(Duration::from_secs(2), hook.entered.notified())
        .await
        .unwrap();
    drop(connection);
    tokio::time::timeout(Duration::from_secs(2), async {
        while h.app.slots.available_permits() != 128 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    hook.release.notify_one();
    // Taking the same tenant lock synchronizes with PostgreSQL rollback completion.
    let mut tx = h.app.pool.begin().await.unwrap();
    let revision: i64 = sqlx::query_scalar("SELECT revision FROM tenants WHERE id=$1 FOR UPDATE")
        .bind(h.tenant)
        .fetch_one(&mut *tx)
        .await
        .unwrap();
    assert_eq!(revision, 0);
    let count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM mutation_receipts WHERE tenant_id=$1")
            .bind(h.tenant)
            .fetch_one(&mut *tx)
            .await
            .unwrap();
    assert_eq!(count, 0);
    tx.rollback().await.unwrap();
}

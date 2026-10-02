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
    let url = std::env::var("TEST_DATABASE_URL").expect("isolated database required");
    let pool = pool(&url).await.unwrap();
    authz_store::MIGRATOR.run(&pool).await.unwrap();
    let tenant = Uuid::new_v4();
    authz_store::provision(&pool, tenant).await.unwrap();
    let secret = [42u8; 32];
    let keys=strict_json::<auth::Config>(&serde_json::to_vec(&json!({"version":1,"keys":[{"key_id":"test","tenant_id":tenant,"scopes":["check","explain","relations:write","relations:read"],"secret_sha256":hex::encode(Sha256::digest(secret))}]})).unwrap()).unwrap();
    let app = Arc::new(App {
        pool,
        keys,
        slots: Arc::new(Semaphore::new(128)),
        draining: AtomicBool::new(false),
        requests: AtomicU64::new(0),
        errors: AtomicU64::new(0),
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
        "{method} /v1/tenants/{}/{path} HTTP/1.1\r\nHost: localhost\r\nAuthorization: Bearer {}\r\nContent-Type: application/json\r\nIdempotency-Key: test\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
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
    let (status, body) = request(&h, "revision", None).await;
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
        connection.write_all(format!("POST /v1/tenants/{}/check HTTP/1.1\r\nHost: localhost\r\nContent-Length: 100\r\n\r\n",h.tenant).as_bytes()).await.unwrap();
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
    let mut bytes = vec![0; 4096];
    let count = first.read(&mut bytes).await.unwrap();
    let text = String::from_utf8_lossy(&bytes[..count]);
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

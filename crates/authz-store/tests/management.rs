use authz_core::*;
use authz_store::*;
use serde_json::Value;
use sqlx::{Executor, PgPool, postgres::PgPoolOptions};
use uuid::Uuid;
fn id(s: &str) -> Id {
    Id::new(s).unwrap()
}
fn ent(kind: Kind, s: &str) -> Entity {
    Entity { kind, id: id(s) }
}
fn put(kind: Kind, s: &str) -> Operation {
    Operation::PutEntity {
        entity: ent(kind, s),
    }
}
fn grant(resource: Entity, relation: Relation, subject: Entity) -> Operation {
    Operation::PutRelation {
        relationship: Edge {
            resource,
            relation,
            subject_relation: if subject.kind == Kind::Team {
                "member"
            } else {
                ""
            }
            .into(),
            subject,
        },
    }
}
fn remove(op: Operation) -> Operation {
    match op {
        Operation::PutRelation { relationship } => Operation::DeleteRelation { relationship },
        _ => panic!(),
    }
}
fn role(user: &str, role: OrgRole) -> Operation {
    Operation::SetOrgRole {
        user_id: id(user),
        role,
    }
}
async fn setup() -> (PgPool, Uuid) {
    let url = std::env::var("TEST_DATABASE_URL").unwrap();
    let schema = format!("managed_{}", Uuid::new_v4().simple());
    let bootstrap = PgPool::connect(&url).await.unwrap();
    bootstrap
        .execute(format!("CREATE SCHEMA {schema}").as_str())
        .await
        .unwrap();
    bootstrap.close().await;
    let p = PgPoolOptions::new()
        .max_connections(10)
        .min_connections(3)
        .after_connect(move |c, _| {
            let q = format!("SET search_path TO {schema}");
            Box::pin(async move {
                c.execute(q.as_str()).await?;
                Ok(())
            })
        })
        .connect(&url)
        .await
        .unwrap();
    MIGRATOR.run(&p).await.unwrap();
    let t = Uuid::new_v4();
    provision(&p, t).await.unwrap();
    management::bootstrap_admin(&p, t, &id("admin"), false)
        .await
        .unwrap();
    apply(
        &p,
        t,
        "admin",
        vec![
            put(Kind::User, "alice"),
            put(Kind::User, "bob"),
            put(Kind::Team, "team"),
            put(Kind::Folder, "root"),
            put(Kind::Folder, "other"),
            put(Kind::Document, "doc"),
            grant(
                ent(Kind::Document, "doc"),
                Relation::Parent,
                ent(Kind::Folder, "root"),
            ),
            grant(
                ent(Kind::Folder, "root"),
                Relation::Manager,
                ent(Kind::User, "alice"),
            ),
        ],
    )
    .await
    .unwrap();
    (p, t)
}
async fn revision(p: &PgPool, t: Uuid) -> i64 {
    sqlx::query_scalar("SELECT revision FROM tenants WHERE id=$1")
        .bind(t)
        .fetch_one(p)
        .await
        .unwrap()
}
async fn apply(p: &PgPool, t: Uuid, actor: &str, ops: Vec<Operation>) -> Result<Value, StoreError> {
    let r = WriteRequest {
        expected_revision: Revision(revision(p, t).await),
        operations: ops,
    };
    attempt(p, t, actor, &Uuid::new_v4().to_string(), &r).await
}
async fn attempt(
    p: &PgPool,
    t: Uuid,
    actor: &str,
    key: &str,
    r: &WriteRequest,
) -> Result<Value, StoreError> {
    let w = prepare_actor_write(p, t, "backend", key, r, Uuid::new_v4(), &id(actor)).await?;
    w.tx.commit().await?;
    Ok(w.response)
}
async fn denied(p: &PgPool, t: Uuid, actor: &str, ops: Vec<Operation>, code: &str) {
    let before = revision(p, t).await;
    assert_eq!(apply(p, t, actor, ops).await.unwrap_err().code, code);
    assert_eq!(revision(p, t).await, before);
}
#[tokio::test]
async fn managed_happy_paths_and_no_privilege_escalation() {
    let (p, t) = setup().await;
    let doc = ent(Kind::Document, "doc");
    let bob = ent(Kind::User, "bob");
    apply(
        &p,
        t,
        "alice",
        vec![grant(doc.clone(), Relation::Editor, bob.clone())],
    )
    .await
    .unwrap();
    let check = BatchRequest {
        subject: bob.clone(),
        checks: vec![Check {
            resource: doc.clone(),
            action: Action::Edit,
        }],
        explain: true,
    };
    assert_eq!(
        management::check_as(&p, t, &id("bob"), &check)
            .await
            .unwrap()
            .1
            .decisions[0]
            .decision,
        Verdict::Allow
    );
    denied(
        &p,
        t,
        "bob",
        vec![grant(doc.clone(), Relation::Manager, bob.clone())],
        "FORBIDDEN",
    )
    .await;
    denied(
        &p,
        t,
        "alice",
        vec![role("alice", OrgRole::Admin)],
        "FORBIDDEN",
    )
    .await;
    denied(
        &p,
        t,
        "alice",
        vec![grant(
            ent(Kind::Team, "team"),
            Relation::Member,
            ent(Kind::User, "alice"),
        )],
        "FORBIDDEN",
    )
    .await;
    denied(
        &p,
        t,
        "bob",
        vec![
            grant(doc.clone(), Relation::Manager, bob.clone()),
            grant(doc.clone(), Relation::Viewer, ent(Kind::User, "alice")),
        ],
        "FORBIDDEN",
    )
    .await;
    denied(
        &p,
        t,
        "alice",
        vec![
            grant(doc.clone(), Relation::Viewer, bob.clone()),
            grant(
                ent(Kind::Folder, "other"),
                Relation::Manager,
                ent(Kind::User, "alice"),
            ),
        ],
        "FORBIDDEN",
    )
    .await;
    assert_eq!(sqlx::query_scalar::<_,i64>("SELECT count(*) FROM relationships WHERE tenant_id=$1 AND resource_id='doc' AND relation='viewer'").bind(t).fetch_one(&p).await.unwrap(),0);
    apply(
        &p,
        t,
        "alice",
        vec![remove(grant(doc.clone(), Relation::Editor, bob.clone()))],
    )
    .await
    .unwrap();
    assert_eq!(
        management::check_as(&p, t, &id("bob"), &check)
            .await
            .unwrap()
            .1
            .decisions[0]
            .decision,
        Verdict::Deny
    );
    assert_eq!(
        management::check_as(&p, t, &id("alice"), &check)
            .await
            .unwrap_err()
            .code,
        "FORBIDDEN"
    );
    let other = Uuid::new_v4();
    provision(&p, other).await.unwrap();
    denied(&p, other, "admin", vec![put(Kind::User, "x")], "FORBIDDEN").await;
    let row:(Option<String>,Value)=sqlx::query_as("SELECT actor_id,authorization_basis FROM audit_events WHERE tenant_id=$1 ORDER BY revision DESC LIMIT 1").bind(t).fetch_one(&p).await.unwrap();
    assert_eq!(row.0.as_deref(), Some("alice"));
    assert_eq!(row.1["kind"], "resource_manager");
}
#[tokio::test]
async fn managed_resource_creation_moves_deletes_and_team_grants() {
    let (p, t) = setup().await;
    let root = ent(Kind::Folder, "root");
    let other = ent(Kind::Folder, "other");
    let child = ent(Kind::Document, "new");
    denied(
        &p,
        t,
        "alice",
        vec![put(Kind::Document, "new")],
        "FORBIDDEN",
    )
    .await;
    apply(
        &p,
        t,
        "alice",
        vec![
            put(Kind::Document, "new"),
            grant(child.clone(), Relation::Parent, root.clone()),
            grant(child.clone(), Relation::Viewer, ent(Kind::User, "bob")),
        ],
    )
    .await
    .unwrap();
    denied(
        &p,
        t,
        "alice",
        vec![
            remove(grant(child.clone(), Relation::Parent, root.clone())),
            grant(child.clone(), Relation::Parent, other.clone()),
        ],
        "FORBIDDEN",
    )
    .await;
    denied(
        &p,
        t,
        "alice",
        vec![remove(grant(child.clone(), Relation::Parent, root.clone()))],
        "FORBIDDEN",
    )
    .await;
    apply(
        &p,
        t,
        "admin",
        vec![
            grant(other.clone(), Relation::Manager, ent(Kind::Team, "team")),
            grant(
                ent(Kind::Team, "team"),
                Relation::Member,
                ent(Kind::User, "alice"),
            ),
        ],
    )
    .await
    .unwrap();
    apply(
        &p,
        t,
        "alice",
        vec![
            remove(grant(child.clone(), Relation::Parent, root)),
            grant(child.clone(), Relation::Parent, other.clone()),
        ],
    )
    .await
    .unwrap();
    denied(
        &p,
        t,
        "bob",
        vec![Operation::DeleteEntity {
            entity: child.clone(),
        }],
        "FORBIDDEN",
    )
    .await;
    apply(
        &p,
        t,
        "alice",
        vec![
            remove(grant(child.clone(), Relation::Parent, other)),
            remove(grant(
                child.clone(),
                Relation::Viewer,
                ent(Kind::User, "bob"),
            )),
            Operation::DeleteEntity { entity: child },
        ],
    )
    .await
    .unwrap();
    denied(
        &p,
        t,
        "alice",
        vec![put(Kind::Team, "another")],
        "FORBIDDEN",
    )
    .await;
}
#[tokio::test]
async fn managed_admin_lifecycle_receipts_and_mixed_rollback() {
    let (p, t) = setup().await;
    denied(
        &p,
        t,
        "admin",
        vec![role("admin", OrgRole::Member)],
        "LAST_ADMIN",
    )
    .await;
    denied(
        &p,
        t,
        "admin",
        vec![Operation::DeleteEntity {
            entity: ent(Kind::User, "admin"),
        }],
        "LAST_ADMIN",
    )
    .await;
    denied(
        &p,
        t,
        "admin",
        vec![put(Kind::User, "rollback"), role("admin", OrgRole::Member)],
        "LAST_ADMIN",
    )
    .await;
    assert!(
        !sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS(SELECT 1 FROM entities WHERE tenant_id=$1 AND id='rollback')"
        )
        .bind(t)
        .fetch_one(&p)
        .await
        .unwrap()
    );
    assert_eq!(
        management::bootstrap_admin(&p, t, &id("bob"), false)
            .await
            .unwrap_err()
            .code,
        "ADMIN_ALREADY_EXISTS"
    );
    apply(&p, t, "admin", vec![role("bob", OrgRole::Admin)])
        .await
        .unwrap();
    let r = WriteRequest {
        expected_revision: Revision(revision(&p, t).await),
        operations: vec![role("admin", OrgRole::Member)],
    };
    let original = attempt(&p, t, "admin", "same", &r).await.unwrap();
    assert_eq!(attempt(&p, t, "admin", "same", &r).await.unwrap(), original);
    assert_eq!(
        attempt(&p, t, "alice", "same", &r).await.unwrap_err().code,
        "FORBIDDEN"
    );
    denied(
        &p,
        t,
        "admin",
        vec![role("admin", OrgRole::Admin)],
        "FORBIDDEN",
    )
    .await;
    management::bootstrap_admin(&p, t, &id("admin"), true)
        .await
        .unwrap();
    let q = WriteRequest {
        expected_revision: Revision(revision(&p, t).await),
        operations: vec![put(Kind::User, "duplicate")],
    };
    attempt(&p, t, "admin", "shared-key", &q).await.unwrap();
    assert_eq!(
        attempt(&p, t, "bob", "shared-key", &q)
            .await
            .unwrap_err()
            .code,
        "REVISION_CONFLICT"
    );
    let mut q2 = q.clone();
    q2.expected_revision = Revision(revision(&p, t).await);
    attempt(&p, t, "bob", "shared-key", &q2).await.unwrap();
    assert_eq!(sqlx::query_scalar::<_,i64>("SELECT count(*) FROM mutation_receipts WHERE tenant_id=$1 AND idempotency_key='shared-key'").bind(t).fetch_one(&p).await.unwrap(),2);
}
async fn wait_blocked(p: &PgPool, pid: i32) {
    tokio::time::timeout(std::time::Duration::from_millis(400), async {
        loop {
            let blocked: bool = sqlx::query_scalar(
                "SELECT EXISTS(SELECT 1 FROM pg_stat_activity WHERE $1=ANY(pg_blocking_pids(pid)))",
            )
            .bind(pid)
            .fetch_one(p)
            .await
            .unwrap();
            if blocked {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
}
#[tokio::test]
async fn managed_revocation_serializes_with_changes_and_last_admin() {
    let (p, t) = setup().await;
    let alice_manager = grant(
        ent(Kind::Folder, "root"),
        Relation::Manager,
        ent(Kind::User, "alice"),
    );
    let r = WriteRequest {
        expected_revision: Revision(revision(&p, t).await),
        operations: vec![remove(alice_manager.clone())],
    };
    let mut revoke =
        prepare_actor_write(&p, t, "backend", "revoke", &r, Uuid::new_v4(), &id("admin"))
            .await
            .unwrap();
    let pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *revoke.tx)
        .await
        .unwrap();
    let p2 = p.clone();
    let stale = r.expected_revision;
    let pending = tokio::spawn(async move {
        let q = WriteRequest {
            expected_revision: stale,
            operations: vec![grant(
                ent(Kind::Document, "doc"),
                Relation::Viewer,
                ent(Kind::User, "bob"),
            )],
        };
        attempt(&p2, t, "alice", "pending", &q).await
    });
    wait_blocked(&p, pid).await;
    revoke.tx.commit().await.unwrap();
    assert_eq!(pending.await.unwrap().unwrap_err().code, "FORBIDDEN");
    apply(&p, t, "admin", vec![alice_manager]).await.unwrap();
    let q = WriteRequest {
        expected_revision: Revision(revision(&p, t).await),
        operations: vec![grant(
            ent(Kind::Document, "doc"),
            Relation::Viewer,
            ent(Kind::User, "bob"),
        )],
    };
    let mut first =
        prepare_actor_write(&p, t, "backend", "first", &q, Uuid::new_v4(), &id("alice"))
            .await
            .unwrap();
    let pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *first.tx)
        .await
        .unwrap();
    let p2 = p.clone();
    let next = q.expected_revision.0 + 1;
    let second = tokio::spawn(async move {
        let q = WriteRequest {
            expected_revision: Revision(next),
            operations: vec![remove(grant(
                ent(Kind::Folder, "root"),
                Relation::Manager,
                ent(Kind::User, "alice"),
            ))],
        };
        attempt(&p2, t, "admin", "second", &q).await
    });
    wait_blocked(&p, pid).await;
    first.tx.commit().await.unwrap();
    second.await.unwrap().unwrap();
    apply(&p, t, "admin", vec![role("bob", OrgRole::Admin)])
        .await
        .unwrap();
    let rev = revision(&p, t).await;
    let mut first = prepare_actor_write(
        &p,
        t,
        "backend",
        "demote1",
        &WriteRequest {
            expected_revision: Revision(rev),
            operations: vec![role("admin", OrgRole::Member)],
        },
        Uuid::new_v4(),
        &id("admin"),
    )
    .await
    .unwrap();
    let pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *first.tx)
        .await
        .unwrap();
    let p2 = p.clone();
    let second = tokio::spawn(async move {
        attempt(
            &p2,
            t,
            "bob",
            "demote2",
            &WriteRequest {
                expected_revision: Revision(rev + 1),
                operations: vec![role("bob", OrgRole::Member)],
            },
        )
        .await
    });
    wait_blocked(&p, pid).await;
    first.tx.commit().await.unwrap();
    assert_eq!(second.await.unwrap().unwrap_err().code, "LAST_ADMIN");
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT count(*) FROM entities WHERE tenant_id=$1 AND org_admin"
        )
        .bind(t)
        .fetch_one(&p)
        .await
        .unwrap(),
        1
    );
}

#[tokio::test]
async fn managed_role_transfer_removal_and_recreation_do_not_restore_admin() {
    let (p, t) = setup().await;
    let bootstrap: Value = sqlx::query_scalar(
        "SELECT effective_changes FROM audit_events WHERE tenant_id=$1 AND revision=1",
    )
    .bind(t)
    .fetch_one(&p)
    .await
    .unwrap();
    assert_eq!(bootstrap.as_array().unwrap().len(), 2);
    apply(
        &p,
        t,
        "admin",
        vec![role("bob", OrgRole::Admin), role("admin", OrgRole::Member)],
    )
    .await
    .unwrap();
    denied(
        &p,
        t,
        "admin",
        vec![put(Kind::User, "forbidden")],
        "FORBIDDEN",
    )
    .await;
    apply(
        &p,
        t,
        "bob",
        vec![Operation::DeleteEntity {
            entity: ent(Kind::User, "admin"),
        }],
    )
    .await
    .unwrap();
    apply(&p, t, "bob", vec![put(Kind::User, "admin")])
        .await
        .unwrap();
    denied(
        &p,
        t,
        "admin",
        vec![role("admin", OrgRole::Admin)],
        "FORBIDDEN",
    )
    .await;
    denied(
        &p,
        t,
        "bob",
        vec![role("absent", OrgRole::Admin)],
        "UNKNOWN_ENTITY",
    )
    .await;
    let request = WriteRequest {
        expected_revision: Revision(revision(&p, t).await),
        operations: vec![put(Kind::User, "same-user")],
    };
    let (a, b) = tokio::join!(
        attempt(&p, t, "bob", "concurrent-receipt", &request),
        attempt(&p, t, "bob", "concurrent-receipt", &request)
    );
    assert_eq!(a.unwrap(), b.unwrap());
    assert_eq!(revision(&p, t).await, request.expected_revision.0 + 1);
    let altered = WriteRequest {
        expected_revision: request.expected_revision,
        operations: vec![put(Kind::User, "different-user")],
    };
    assert_eq!(
        attempt(&p, t, "bob", "concurrent-receipt", &altered)
            .await
            .unwrap_err()
            .code,
        "IDEMPOTENCY_CONFLICT"
    );
}

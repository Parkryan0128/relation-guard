use authz_core::*;
use authz_store::*;
use serde_json::Value;
use sqlx::{Executor, PgPool, Row, postgres::PgPoolOptions};
use uuid::Uuid;
async fn setup() -> (PgPool, Uuid) {
    let url = std::env::var("TEST_DATABASE_URL").expect("isolated PostgreSQL required");
    let schema = format!("s_{}", Uuid::new_v4().simple());
    let bootstrap = PgPool::connect(&url).await.unwrap();
    bootstrap
        .execute(format!("CREATE SCHEMA {schema}").as_str())
        .await
        .unwrap();
    bootstrap.close().await;
    let p = PgPoolOptions::new()
        .max_connections(10)
        .after_connect(move |c, _| {
            let sql = format!("SET search_path TO {schema}");
            Box::pin(async move {
                c.execute(sql.as_str()).await?;
                Ok(())
            })
        })
        .connect(&url)
        .await
        .unwrap();
    MIGRATOR.run(&p).await.unwrap();
    let t = Uuid::new_v4();
    provision(&p, t).await.unwrap();
    (p, t)
}
fn fixture() -> WriteRequest {
    let v: Value =
        serde_json::from_str(include_str!("../../../tests/fixtures/contracts.json")).unwrap();
    serde_json::from_value(v["write"]["request"].clone()).unwrap()
}
fn request() -> BatchRequest {
    let v: Value =
        serde_json::from_str(include_str!("../../../tests/fixtures/contracts.json")).unwrap();
    let c: CheckRequest = serde_json::from_value(v["check"]["request"].clone()).unwrap();
    c.batch()
}
async fn put(
    p: &PgPool,
    t: Uuid,
    key: &str,
    w: &WriteRequest,
) -> Result<(Value, bool), StoreError> {
    write(p, t, "test", key, w, Uuid::new_v4()).await
}
async fn counts(p: &PgPool, t: Uuid) -> (i64, i64, i64, i64) {
    let r=sqlx::query("SELECT (SELECT revision FROM tenants WHERE id=$1) r,(SELECT count(*) FROM relationships WHERE tenant_id=$1) e,(SELECT count(*) FROM audit_events WHERE tenant_id=$1) a,(SELECT count(*) FROM mutation_receipts WHERE tenant_id=$1) i").bind(t).fetch_one(p).await.unwrap();
    (r.get("r"), r.get("e"), r.get("a"), r.get("i"))
}
fn entity(kind: Kind, id: &str) -> Entity {
    Entity {
        kind,
        id: Id::new(id).unwrap(),
    }
}
fn parent(child: Entity, target: Entity) -> Operation {
    Operation::PutRelation {
        relationship: Edge {
            resource: child,
            relation: Relation::Parent,
            subject: target,
            subject_relation: String::new(),
        },
    }
}
#[tokio::test]
async fn atomicity_replay_revocation_and_isolation() {
    let (p, t) = setup().await;
    let w = fixture();
    let first = put(&p, t, "grant", &w).await.unwrap();
    assert_eq!(counts(&p, t).await, (1, 3, 1, 1));
    let (_, evaluation) = check(&p, t, &request()).await.unwrap();
    assert_eq!(evaluation.decisions[0].decision, Verdict::Allow);
    let mut denied = request();
    denied.subject.id = Id::new("missing").unwrap();
    assert_eq!(
        check(&p, t, &denied).await.unwrap().1.decisions[0].reason,
        Reason::NotFound
    );
    let other = Uuid::new_v4();
    provision(&p, other).await.unwrap();
    let mut objects = w.clone();
    objects
        .operations
        .retain(|o| matches!(o, Operation::PutEntity { .. }));
    put(&p, other, "objects", &objects).await.unwrap();
    assert_eq!(
        check(&p, other, &request()).await.unwrap().1.decisions[0].decision,
        Verdict::Deny
    );
    let mut bad = w.clone();
    bad.expected_revision = Revision(1);
    bad.operations = vec![
        Operation::PutEntity {
            entity: entity(Kind::User, "new"),
        },
        parent(
            entity(Kind::Document, "d100"),
            entity(Kind::Folder, "missing"),
        ),
    ];
    assert!(put(&p, t, "bad", &bad).await.is_err());
    assert_eq!(counts(&p, t).await, (1, 3, 1, 1));
    let member = match &w.operations[4] {
        Operation::PutRelation { relationship } => relationship.clone(),
        _ => panic!(),
    };
    let revoke = WriteRequest {
        expected_revision: Revision(1),
        operations: vec![Operation::DeleteRelation {
            relationship: member,
        }],
    };
    put(&p, t, "revoke", &revoke).await.unwrap();
    assert_eq!(
        check(&p, t, &request()).await.unwrap().1.decisions[0].decision,
        Verdict::Deny
    );
    let replay = put(&p, t, "grant", &w).await.unwrap();
    assert!(replay.1);
    assert_eq!(replay.0, first.0);
    assert_eq!(counts(&p, t).await, (2, 2, 2, 2));
    let mut conflict = w.clone();
    conflict.expected_revision = Revision(2);
    assert_eq!(
        put(&p, t, "grant", &conflict).await.unwrap_err().code,
        "IDEMPOTENCY_CONFLICT"
    );
    sqlx::query("UPDATE mutation_receipts SET recorded_at=CURRENT_TIMESTAMP-INTERVAL '25 hours' WHERE tenant_id=$1 AND idempotency_key='grant'").bind(t).execute(&p).await.unwrap();
    assert_eq!(cleanup_receipts(&p).await.unwrap(), 1);
    assert_eq!(
        put(&p, t, "grant", &w).await.unwrap_err().code,
        "REVISION_CONFLICT"
    );
    let noop = WriteRequest {
        expected_revision: Revision(2),
        operations: vec![Operation::DeleteEntity {
            entity: entity(Kind::User, "absent"),
        }],
    };
    let no = put(&p, t, "no", &noop).await.unwrap();
    assert_eq!(no.0["changed"], false);
    assert_eq!(counts(&p, t).await, (2, 2, 2, 2));
    let used = WriteRequest {
        expected_revision: Revision(2),
        operations: vec![Operation::DeleteEntity {
            entity: entity(Kind::Folder, "f10"),
        }],
    };
    assert_eq!(
        put(&p, t, "used", &used).await.unwrap_err().code,
        "ENTITY_IN_USE"
    );
}
#[tokio::test]
async fn snapshot_and_concurrent_writers() {
    let (p, t) = setup().await;
    let w = fixture();
    put(&p, t, "init", &w).await.unwrap();
    let mut before = begin_read(&p, t).await.unwrap();
    let member = match &w.operations[4] {
        Operation::PutRelation { relationship } => relationship.clone(),
        _ => panic!(),
    };
    let revoke = WriteRequest {
        expected_revision: Revision(1),
        operations: vec![Operation::DeleteRelation {
            relationship: member,
        }],
    };
    put(&p, t, "revoke", &revoke).await.unwrap();
    let facts = load(&mut before, &request()).await.unwrap();
    assert_eq!(
        evaluate(&facts, &request()).unwrap().decisions[0].decision,
        Verdict::Allow
    );
    assert_eq!(before.revision, 1);
    before.tx.commit().await.unwrap();
    assert_eq!(
        check(&p, t, &request()).await.unwrap().1.decisions[0].decision,
        Verdict::Deny
    );
    for i in 0..100 {
        let rev = 2 + i;
        let a = WriteRequest {
            expected_revision: Revision(rev),
            operations: vec![Operation::PutEntity {
                entity: entity(Kind::User, &format!("a{i}")),
            }],
        };
        let mut b = a.clone();
        b.operations = vec![Operation::PutEntity {
            entity: entity(Kind::User, &format!("b{i}")),
        }];
        let barrier = tokio::sync::Barrier::new(2);
        let (ra, rb) = tokio::join!(
            async {
                barrier.wait().await;
                put(&p, t, &format!("a{i}"), &a).await
            },
            async {
                barrier.wait().await;
                put(&p, t, &format!("b{i}"), &b).await
            }
        );
        assert_ne!(ra.is_ok(), rb.is_ok());
        let err = if let Err(e) = ra { e } else { rb.unwrap_err() };
        assert_eq!(err.code, "REVISION_CONFLICT");
    }
    for i in 0..100 {
        let now = counts(&p, t).await.0;
        let w = WriteRequest {
            expected_revision: Revision(now),
            operations: vec![Operation::PutEntity {
                entity: entity(Kind::User, &format!("same-{i}")),
            }],
        };
        let key = format!("same-key-{i}");
        let barrier = tokio::sync::Barrier::new(2);
        let (a, b) = tokio::join!(
            async {
                barrier.wait().await;
                put(&p, t, &key, &w).await
            },
            async {
                barrier.wait().await;
                put(&p, t, &key, &w).await
            }
        );
        let (a, b) = (a.unwrap(), b.unwrap());
        assert_eq!(a.0, b.0);
        assert_ne!(a.1, b.1);
        assert_eq!(counts(&p, t).await.0, now + 1);
    }
}
#[tokio::test]
async fn hierarchy_and_revision_exhaustion() {
    let (p, t) = setup().await;
    let mut w = WriteRequest {
        expected_revision: Revision(0),
        operations: vec![],
    };
    for i in 0..=33 {
        w.operations.push(Operation::PutEntity {
            entity: entity(Kind::Folder, &format!("f{i}")),
        });
        if i > 0 && i <= 32 {
            w.operations.push(parent(
                entity(Kind::Folder, &format!("f{i}")),
                entity(Kind::Folder, &format!("f{}", i - 1)),
            ));
        }
    }
    put(&p, t, "tree", &w).await.unwrap();
    let start = counts(&p, t).await;
    for (name, op) in [
        (
            "depth",
            parent(entity(Kind::Folder, "f33"), entity(Kind::Folder, "f32")),
        ),
        (
            "cycle",
            parent(entity(Kind::Folder, "f0"), entity(Kind::Folder, "f32")),
        ),
        (
            "parents",
            parent(entity(Kind::Folder, "f2"), entity(Kind::Folder, "f0")),
        ),
    ] {
        let x = WriteRequest {
            expected_revision: Revision(1),
            operations: vec![op],
        };
        assert_eq!(
            put(&p, t, name, &x).await.unwrap_err().code,
            "INVALID_HIERARCHY"
        );
        assert_eq!(counts(&p, t).await, start);
    }
    sqlx::query("UPDATE tenants SET revision=$2 WHERE id=$1")
        .bind(t)
        .bind(i64::MAX)
        .execute(&p)
        .await
        .unwrap();
    let x = WriteRequest {
        expected_revision: Revision(i64::MAX),
        operations: vec![Operation::PutEntity {
            entity: entity(Kind::User, "overflow"),
        }],
    };
    assert_eq!(
        put(&p, t, "overflow", &x).await.unwrap_err().code,
        "REVISION_EXHAUSTED"
    );
    let n: i64 =
        sqlx::query_scalar("SELECT count(*) FROM entities WHERE tenant_id=$1 AND id='overflow'")
            .bind(t)
            .fetch_one(&p)
            .await
            .unwrap();
    assert_eq!(n, 0);
}

#[tokio::test]
async fn matching_grant_limit_and_duplicate_batch() {
    let (p, t) = setup().await;
    put(&p, t, "init", &fixture()).await.unwrap();
    let mut batch = request();
    batch.checks = vec![batch.checks[0].clone(); 100];
    let (revision, result) = check(&p, t, &batch).await.unwrap();
    assert_eq!(revision, 1);
    assert_eq!(result.decisions.len(), 100);
    assert!(
        result
            .decisions
            .iter()
            .all(|d| d.decision == Verdict::Allow)
    );
    sqlx::query("INSERT INTO entities(tenant_id,kind,id) SELECT $1,'team','bulk'||i FROM generate_series(1,10001) i").bind(t).execute(&p).await.unwrap();
    sqlx::query("INSERT INTO relationships SELECT $1,'team','bulk'||i,'member','user','u42','' FROM generate_series(1,10001) i").bind(t).execute(&p).await.unwrap();
    sqlx::query("INSERT INTO relationships SELECT $1,'folder','f10','viewer','team','bulk'||i,'member' FROM generate_series(1,10001) i").bind(t).execute(&p).await.unwrap();
    assert_eq!(
        check(&p, t, &request()).await.unwrap_err().code,
        "EVALUATION_LIMIT"
    );
}

#[tokio::test]
async fn cleanup_recreate_and_key_scope() {
    let (p, t) = setup().await;
    let w = fixture();
    put(&p, t, "init", &w).await.unwrap();
    let err = write(&p, t, "rotated", "init", &w, Uuid::new_v4())
        .await
        .unwrap_err();
    assert_eq!(err.code, "REVISION_CONFLICT");
    let mut operations: Vec<_> = w
        .operations
        .iter()
        .filter_map(|o| match o {
            Operation::PutRelation { relationship } => Some(Operation::DeleteRelation {
                relationship: relationship.clone(),
            }),
            _ => None,
        })
        .collect();
    operations.push(Operation::DeleteEntity {
        entity: entity(Kind::User, "u42"),
    });
    put(
        &p,
        t,
        "delete",
        &WriteRequest {
            expected_revision: Revision(1),
            operations,
        },
    )
    .await
    .unwrap();
    put(
        &p,
        t,
        "recreate",
        &WriteRequest {
            expected_revision: Revision(2),
            operations: vec![Operation::PutEntity {
                entity: entity(Kind::User, "u42"),
            }],
        },
    )
    .await
    .unwrap();
    assert_eq!(
        check(&p, t, &request()).await.unwrap().1.decisions[0].decision,
        Verdict::Deny
    );
    sqlx::query("INSERT INTO mutation_receipts(tenant_id,key_id,idempotency_key,request_hash,response,recorded_at) SELECT $1,'test','expired'||i,decode(repeat('00',32),'hex'),'{}'::jsonb,CURRENT_TIMESTAMP-INTERVAL '25 hours' FROM generate_series(1,1001) i").bind(t).execute(&p).await.unwrap();
    assert_eq!(cleanup_receipts(&p).await.unwrap(), 1000);
    assert_eq!(cleanup_receipts(&p).await.unwrap(), 1);
    assert_eq!(cleanup_receipts(&p).await.unwrap(), 0);
    assert_eq!(counts(&p, t).await, (3, 0, 3, 3));
}

#[tokio::test]
async fn concurrent_cycles_and_final_state_moves() {
    let (p, t) = setup().await;
    let folders: Vec<_> = ["a", "b", "c"]
        .iter()
        .map(|id| entity(Kind::Folder, id))
        .collect();
    put(
        &p,
        t,
        "init",
        &WriteRequest {
            expected_revision: Revision(0),
            operations: folders
                .iter()
                .map(|e| Operation::PutEntity { entity: e.clone() })
                .collect(),
        },
    )
    .await
    .unwrap();
    let self_parent = WriteRequest {
        expected_revision: Revision(1),
        operations: vec![parent(folders[0].clone(), folders[0].clone())],
    };
    assert_eq!(
        put(&p, t, "self", &self_parent).await.unwrap_err().code,
        "INVALID_HIERARCHY"
    );
    let mut rev = 1;
    for round in 0..100 {
        let first = WriteRequest {
            expected_revision: Revision(rev),
            operations: vec![parent(folders[0].clone(), folders[1].clone())],
        };
        let second = WriteRequest {
            expected_revision: Revision(rev),
            operations: vec![parent(folders[1].clone(), folders[0].clone())],
        };
        let barrier = std::sync::Arc::new(tokio::sync::Barrier::new(2));
        let (x, y) = tokio::join!(
            async {
                barrier.wait().await;
                put(&p, t, &format!("left-{round}"), &first).await
            },
            async {
                barrier.wait().await;
                put(&p, t, &format!("right-{round}"), &second).await
            }
        );
        assert_ne!(x.is_ok(), y.is_ok());
        let (winner, mut loser) = if x.is_ok() {
            (first, second)
        } else {
            (second, first)
        };
        rev += 1;
        loser.expected_revision = Revision(rev);
        assert_eq!(
            put(&p, t, &format!("cycle-{round}"), &loser)
                .await
                .unwrap_err()
                .code,
            "INVALID_HIERARCHY"
        );
        let Operation::PutRelation { relationship } = winner.operations[0].clone() else {
            unreachable!()
        };
        put(
            &p,
            t,
            &format!("reset-{round}"),
            &WriteRequest {
                expected_revision: Revision(rev),
                operations: vec![Operation::DeleteRelation { relationship }],
            },
        )
        .await
        .unwrap();
        rev += 1;
    }
    // A final-state two-edge move: deleting the old edge makes the inverse safe.
    put(
        &p,
        t,
        "old",
        &WriteRequest {
            expected_revision: Revision(rev),
            operations: vec![parent(folders[0].clone(), folders[1].clone())],
        },
    )
    .await
    .unwrap();
    rev += 1;
    let Operation::PutRelation { relationship } = parent(folders[0].clone(), folders[1].clone())
    else {
        unreachable!()
    };
    put(
        &p,
        t,
        "move",
        &WriteRequest {
            expected_revision: Revision(rev),
            operations: vec![
                parent(folders[1].clone(), folders[0].clone()),
                Operation::DeleteRelation { relationship },
            ],
        },
    )
    .await
    .unwrap();
    rev += 1;
    let noop = WriteRequest {
        expected_revision: Revision(rev),
        operations: vec![Operation::PutEntity {
            entity: folders[0].clone(),
        }],
    };
    let (a, b) = tokio::join!(put(&p, t, "noop-a", &noop), put(&p, t, "noop-b", &noop));
    assert!(!a.unwrap().0["changed"].as_bool().unwrap());
    assert!(!b.unwrap().0["changed"].as_bool().unwrap());
}

#[tokio::test]
async fn exact_retention_boundary_and_descendant_move() {
    let (p, t) = setup().await;
    let mut tx = p.begin().await.unwrap();
    sqlx::query("INSERT INTO mutation_receipts(tenant_id,key_id,idempotency_key,request_hash,response,recorded_at) SELECT $1,'test',label,decode(repeat('00',32),'hex'),'{}'::jsonb,CURRENT_TIMESTAMP-INTERVAL '24 hours'+offset_value FROM (VALUES ('before',INTERVAL '-1 microsecond'),('exact',INTERVAL '0 microsecond'),('after',INTERVAL '1 microsecond')) v(label,offset_value)").bind(t).execute(&mut *tx).await.unwrap();
    assert_eq!(cleanup_receipts(&mut *tx).await.unwrap(), 1);
    let keys: Vec<String> = sqlx::query_scalar(
        "SELECT idempotency_key FROM mutation_receipts WHERE tenant_id=$1 ORDER BY idempotency_key",
    )
    .bind(t)
    .fetch_all(&mut *tx)
    .await
    .unwrap();
    assert_eq!(keys, vec!["after", "exact"]);
    tx.rollback().await.unwrap();
    let mut ops = Vec::new();
    for i in 0..34 {
        ops.push(Operation::PutEntity {
            entity: entity(Kind::Folder, &format!("f{i}")),
        });
        if i > 0 && i < 33 {
            ops.push(parent(
                entity(Kind::Folder, &format!("f{i}")),
                entity(Kind::Folder, &format!("f{}", i - 1)),
            ));
        }
    }
    put(
        &p,
        t,
        "chain",
        &WriteRequest {
            expected_revision: Revision(0),
            operations: ops,
        },
    )
    .await
    .unwrap();
    let before = counts(&p, t).await;
    let movement = WriteRequest {
        expected_revision: Revision(1),
        operations: vec![parent(
            entity(Kind::Folder, "f0"),
            entity(Kind::Folder, "f33"),
        )],
    };
    assert_eq!(
        put(&p, t, "too-deep", &movement).await.unwrap_err().code,
        "INVALID_HIERARCHY"
    );
    assert_eq!(counts(&p, t).await, before);
}
#[tokio::test]
async fn batch_snapshot_between_loads() {
    let (p, t) = setup().await;
    put(&p, t, "initial", &fixture()).await.unwrap();
    for round in 0..100 {
        let mut snapshot = begin_read(&p, t).await.unwrap();
        let mut batch = request();
        batch.checks = vec![batch.checks[0].clone(); 100];
        let first = load(&mut snapshot, &batch).await.unwrap();
        let current = snapshot.revision;
        let Operation::PutRelation { relationship } = fixture().operations[4].clone() else {
            unreachable!()
        };
        put(
            &p,
            t,
            &format!("revoke-{round}"),
            &WriteRequest {
                expected_revision: Revision(current),
                operations: vec![Operation::DeleteRelation {
                    relationship: relationship.clone(),
                }],
            },
        )
        .await
        .unwrap();
        let second = load(&mut snapshot, &batch).await.unwrap();
        assert_eq!(first.relationships, second.relationships);
        assert!(
            evaluate(&second, &batch)
                .unwrap()
                .decisions
                .iter()
                .all(|x| x.decision == Verdict::Allow)
        );
        snapshot.tx.commit().await.unwrap();
        assert!(
            check(&p, t, &batch)
                .await
                .unwrap()
                .1
                .decisions
                .iter()
                .all(|x| x.decision == Verdict::Deny)
        );
        put(
            &p,
            t,
            &format!("restore-{round}"),
            &WriteRequest {
                expected_revision: Revision(current + 1),
                operations: vec![Operation::PutRelation { relationship }],
            },
        )
        .await
        .unwrap();
    }
}

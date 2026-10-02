use authz_core::*;
use criterion::{Criterion, black_box, criterion_group, criterion_main};
fn benchmarks(c: &mut Criterion) {
    for depth in [0, 8, 32] {
        for mode in ["direct", "team", "deny"] {
            let u = Entity {
                kind: Kind::User,
                id: Id::new("u").unwrap(),
            };
            let mut s = Snapshot::default();
            s.entities.insert(u.clone());
            for i in 0..=depth {
                let f = Entity {
                    kind: Kind::Folder,
                    id: Id::new(format!("f{i}")).unwrap(),
                };
                s.entities.insert(f.clone());
                if i > 0 {
                    s.relationships.insert(Edge {
                        resource: f,
                        relation: Relation::Parent,
                        subject: Entity {
                            kind: Kind::Folder,
                            id: Id::new(format!("f{}", i - 1)).unwrap(),
                        },
                        subject_relation: String::new(),
                    });
                }
            }
            let root = Entity {
                kind: Kind::Folder,
                id: Id::new("f0").unwrap(),
            };
            let target = Entity {
                kind: Kind::Folder,
                id: Id::new(format!("f{depth}")).unwrap(),
            };
            if mode != "deny" {
                let subject = if mode == "team" {
                    let t = Entity {
                        kind: Kind::Team,
                        id: Id::new("t").unwrap(),
                    };
                    s.entities.insert(t.clone());
                    s.relationships.insert(Edge {
                        resource: t.clone(),
                        relation: Relation::Member,
                        subject: u.clone(),
                        subject_relation: String::new(),
                    });
                    t
                } else {
                    u.clone()
                };
                s.relationships.insert(Edge {
                    resource: root,
                    relation: Relation::Editor,
                    subject,
                    subject_relation: if mode == "team" {
                        "member".into()
                    } else {
                        String::new()
                    },
                });
            }
            let req = BatchRequest {
                subject: u,
                checks: vec![Check {
                    resource: target,
                    action: Action::View,
                }],
                explain: false,
            };
            c.bench_function(&format!("{mode}/depth-{depth}"), |b| {
                b.iter(|| evaluate(black_box(&s), black_box(&req)).unwrap())
            });
        }
    }
}
criterion_group! {name=benches;config=Criterion::default().sample_size(10).warm_up_time(std::time::Duration::from_millis(100)).measurement_time(std::time::Duration::from_millis(200));targets=benchmarks}
criterion_main!(benches);

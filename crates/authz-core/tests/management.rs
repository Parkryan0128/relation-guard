use authz_core::*;
fn entity(kind: Kind, id: &str) -> Entity {
    Entity {
        kind,
        id: Id::new(id).unwrap(),
    }
}
fn edge(resource: Entity, relation: Relation, subject: Entity) -> Edge {
    let sr = if subject.kind == Kind::Team {
        "member"
    } else {
        ""
    };
    Edge {
        resource,
        relation,
        subject,
        subject_relation: sr.into(),
    }
}
#[test]
fn managed_permissions_matrix_and_inheritance() {
    let u = entity(Kind::User, "u");
    let team = entity(Kind::Team, "t");
    let f = entity(Kind::Folder, "f");
    let d = entity(Kind::Document, "d");
    for relation in [Relation::Viewer, Relation::Editor, Relation::Manager] {
        for team_grant in [false, true] {
            for inherited in [false, true] {
                let mut s = Snapshot::default();
                s.entities
                    .extend([u.clone(), team.clone(), f.clone(), d.clone()]);
                s.relationships
                    .insert(edge(d.clone(), Relation::Parent, f.clone()));
                s.relationships
                    .insert(edge(team.clone(), Relation::Member, u.clone()));
                s.relationships.insert(edge(
                    if inherited { f.clone() } else { d.clone() },
                    relation,
                    if team_grant { team.clone() } else { u.clone() },
                ));
                for action in [Action::View, Action::Edit, Action::Manage] {
                    let request = BatchRequest {
                        subject: u.clone(),
                        checks: vec![Check {
                            resource: d.clone(),
                            action,
                        }],
                        explain: true,
                    };
                    let expected = action == Action::View
                        || relation == Relation::Manager
                        || action == Action::Edit && relation == Relation::Editor;
                    let result = evaluate(&s, &request).unwrap();
                    assert_eq!(result.decisions[0].decision == Verdict::Allow, expected);
                    if expected {
                        assert!(
                            result.decisions[0]
                                .proof
                                .as_ref()
                                .unwrap()
                                .iter()
                                .any(|e| e.relation == relation)
                        );
                    }
                }
            }
        }
    }
}
#[test]
fn admin_does_not_allow_missing_resource_or_corrupt_graph() {
    let u = entity(Kind::User, "u");
    let d = entity(Kind::Document, "d");
    let mut s = Snapshot::default();
    s.entities.insert(u.clone());
    s.administrators.insert(u.clone());
    let r = BatchRequest {
        subject: u,
        checks: vec![Check {
            resource: d.clone(),
            action: Action::Manage,
        }],
        explain: true,
    };
    assert_eq!(
        evaluate(&s, &r).unwrap().decisions[0].reason,
        Reason::NotFound
    );
    s.entities.insert(d.clone());
    let result = evaluate(&s, &r).unwrap();
    assert_eq!(result.decisions[0].reason, Reason::OrgAdmin);
    assert_eq!(result.decisions[0].proof, Some(vec![]));
    s.relationships
        .insert(edge(d, Relation::Parent, entity(Kind::Folder, "missing")));
    assert!(matches!(evaluate(&s, &r), Err(Fault::CorruptGraph)));
}
#[test]
fn role_operations_are_strict_and_canonical() {
    let a=br#"{"expected_revision":"0","operations":[{"op":"set_org_role","user_id":"u","role":"admin"}]}"#;
    let r: WriteRequest = strict_json(a).unwrap();
    r.validate().unwrap();
    assert!(
        r.canonical()
            .unwrap()
            .windows(12)
            .any(|v| v == b"set_org_role")
    );
    for invalid in [
        r#"{"expected_revision":"0","operations":[{"op":"set_org_role","user_id":"u","role":"owner"}]}"#,
        r#"{"expected_revision":"0","operations":[{"op":"set_org_role","user_id":"u","role":"admin","skip_checks":true}]}"#,
    ] {
        assert!(strict_json::<WriteRequest>(invalid.as_bytes()).is_err());
    }
    let mut duplicate = r.clone();
    duplicate.operations.extend(r.operations.clone());
    assert_eq!(duplicate.validate(), Err(Fault::DuplicateOperation));
    duplicate.operations.pop();
    duplicate.operations.push(Operation::DeleteEntity {
        entity: entity(Kind::User, "u"),
    });
    assert_eq!(duplicate.validate(), Err(Fault::DuplicateOperation));
}

#[test]
fn batch_preserves_order_duplicates_and_missing_resources() {
    let user = entity(Kind::User, "u");
    let allowed = entity(Kind::Document, "allowed");
    let denied = entity(Kind::Document, "denied");
    let mut snapshot = Snapshot::default();
    snapshot
        .entities
        .extend([user.clone(), allowed.clone(), denied.clone()]);
    snapshot
        .relationships
        .insert(edge(allowed.clone(), Relation::Editor, user.clone()));
    let request = BatchRequest {
        subject: user,
        checks: vec![
            Check {
                resource: denied,
                action: Action::View,
            },
            Check {
                resource: allowed.clone(),
                action: Action::Edit,
            },
            Check {
                resource: entity(Kind::Document, "missing"),
                action: Action::View,
            },
            Check {
                resource: allowed.clone(),
                action: Action::Edit,
            },
            Check {
                resource: allowed,
                action: Action::Manage,
            },
        ],
        explain: true,
    };
    let result = evaluate(&snapshot, &request).unwrap();
    assert_eq!(
        result
            .decisions
            .iter()
            .map(|d| &d.reason)
            .collect::<Vec<_>>(),
        vec![
            &Reason::NoMatchingRelation,
            &Reason::MatchingRelation,
            &Reason::NotFound,
            &Reason::MatchingRelation,
            &Reason::NoMatchingRelation
        ]
    );
    assert_eq!(result.decisions[1], result.decisions[3]);
    for index in [0, 2, 4] {
        assert_eq!(result.decisions[index].proof, Some(vec![]));
    }
}

#[test]
fn resource_and_grant_limits_are_inclusive_and_errors_are_not_decisions() {
    let user = entity(Kind::User, "u");
    let target = entity(Kind::Document, "d0");
    let request = BatchRequest {
        subject: user.clone(),
        checks: vec![Check {
            resource: target.clone(),
            action: Action::View,
        }],
        explain: false,
    };
    let mut snapshot = Snapshot::default();
    snapshot.entities.insert(user.clone());
    for i in 0..MAX_RESOURCES {
        snapshot
            .entities
            .insert(entity(Kind::Document, &format!("d{i}")));
    }
    assert!(evaluate(&snapshot, &request).is_ok());
    snapshot.entities.insert(entity(Kind::Document, "overflow"));
    assert_eq!(
        evaluate(&snapshot, &request).unwrap_err(),
        Fault::EvaluationLimit
    );
    // Even an administrator cannot bypass fact limits with an early allow.
    snapshot.administrators.insert(user.clone());
    assert_eq!(
        evaluate(&snapshot, &request).unwrap_err(),
        Fault::EvaluationLimit
    );
    snapshot = Snapshot::default();
    snapshot.entities.extend([user, target.clone()]);
    for i in 0..=MAX_GRANTS {
        if i == MAX_GRANTS {
            assert!(evaluate(&snapshot, &request).is_ok());
        }
        let grantee = entity(Kind::User, &format!("grantee{i}"));
        snapshot.entities.insert(grantee.clone());
        snapshot
            .relationships
            .insert(edge(target.clone(), Relation::Viewer, grantee));
    }
    assert_eq!(
        evaluate(&snapshot, &request).unwrap_err(),
        Fault::EvaluationLimit
    );
}

use authz_core::*;
use proptest::prelude::*;
use serde_json::Value;
use std::collections::BTreeSet;
fn e(kind: Kind, id: &str) -> Entity {
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
fn request(user: Entity, resource: Entity, action: Action) -> BatchRequest {
    BatchRequest {
        subject: user,
        checks: vec![Check { resource, action }],
        explain: true,
    }
}
#[test]
fn authorization_fixtures() {
    let fixture: Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/authorization-cases.json"
    ))
    .unwrap();
    for case in fixture["cases"].as_array().unwrap() {
        let t = &fixture["tenants"][case["tenant_index"].as_u64().unwrap() as usize];
        let mut s = Snapshot {
            administrators: Default::default(),
            entities: serde_json::from_value(t["entities"].clone()).unwrap(),
            relationships: serde_json::from_value(t["relationships"].clone()).unwrap(),
        };
        for x in case["remove_relationships"]
            .as_array()
            .into_iter()
            .flatten()
        {
            s.relationships
                .remove(&serde_json::from_value::<Edge>(x.clone()).unwrap());
        }
        for x in case["add_relationships"].as_array().into_iter().flatten() {
            s.relationships
                .insert(serde_json::from_value(x.clone()).unwrap());
        }
        let req = request(
            serde_json::from_value(case["subject"].clone()).unwrap(),
            serde_json::from_value(case["resource"].clone()).unwrap(),
            serde_json::from_value(case["action"].clone()).unwrap(),
        );
        let result = evaluate(&s, &req).unwrap();
        assert_eq!(
            serde_json::to_value(&result.decisions[0].decision).unwrap(),
            case["decision"],
            "{}",
            case["name"]
        );
        if !case["reason"].is_null() {
            assert_eq!(
                serde_json::to_value(&result.decisions[0].reason).unwrap(),
                case["reason"]
            );
        }
    }
}
// Independent reference: upward reachability then raw tuple scans. No production index/evaluator reuse.
fn reference(s: &Snapshot, u: &Entity, r: &Entity, a: Action) -> bool {
    if !s.entities.contains(u) || !s.entities.contains(r) {
        return false;
    }
    let mut reachable = BTreeSet::from([r.clone()]);
    loop {
        let before = reachable.len();
        for x in &s.relationships {
            if x.relation == Relation::Parent && reachable.contains(&x.resource) {
                reachable.insert(x.subject.clone());
            }
        }
        if reachable.len() == before {
            break;
        }
    }
    s.relationships.iter().any(|x| {
        reachable.contains(&x.resource)
            && (x.relation == Relation::Manager
                || (a != Action::Manage && x.relation == Relation::Editor)
                || (a == Action::View && x.relation == Relation::Viewer))
            && (x.subject == *u
                || (x.subject.kind == Kind::Team
                    && s.relationships.iter().any(|m| {
                        m.relation == Relation::Member && m.resource == x.subject && m.subject == *u
                    })))
    })
}
proptest! {
 #[test]
 fn reference_and_properties(parents in prop::collection::vec(0usize..64,1..24), flags in prop::collection::vec(any::<u8>(),24)){
  let u=e(Kind::User,"u");let team=e(Kind::Team,"t");let mut s=Snapshot::default();s.entities.extend([u.clone(),team.clone()]);
  if flags[0]&1!=0{s.relationships.insert(edge(team.clone(),Relation::Member,u.clone()));}
  for(i,p)in parents.iter().enumerate(){let f=e(Kind::Folder,&format!("f{i}"));s.entities.insert(f.clone());if i>0{s.relationships.insert(edge(f.clone(),Relation::Parent,e(Kind::Folder,&format!("f{}",p%i))));}
   if flags[i]&2!=0{s.relationships.insert(edge(f.clone(),Relation::Viewer,u.clone()));}
   if flags[i]&4!=0{s.relationships.insert(edge(f.clone(),Relation::Editor,team.clone()));}
   if flags[i]&8!=0{s.relationships.insert(edge(f.clone(),Relation::Manager,u.clone()));}
   if flags[i]&16!=0{s.relationships.insert(edge(f.clone(),Relation::Manager,team.clone()));}
  }
  let target=e(Kind::Folder,&format!("f{}",parents.len()-1));
  let mut req=request(u.clone(),target.clone(),Action::Manage);
  let manage=evaluate(&s,&req).unwrap().decisions[0].decision==Verdict::Allow;
  prop_assert_eq!(manage,reference(&s,&u,&target,Action::Manage));
  req.checks[0].action=Action::Edit;
  let edit=evaluate(&s,&req).unwrap().decisions[0].decision==Verdict::Allow;
  prop_assert_eq!(edit,reference(&s,&u,&target,Action::Edit));prop_assert!(!manage||edit);
  req.checks[0].action=Action::View;let result=evaluate(&s,&req).unwrap();let view=result.decisions[0].decision==Verdict::Allow;
  prop_assert_eq!(view,reference(&s,&u,&target,Action::View));prop_assert!(!edit||view);
  if let Some(proof)=&result.decisions[0].proof{for x in proof{prop_assert!(s.relationships.contains(x));}}
  if view { let proof_snapshot=Snapshot { administrators: Default::default(), entities:s.entities.clone(), relationships:result.decisions[0].proof.clone().unwrap().into_iter().collect() }; prop_assert!(reference(&proof_snapshot,&u,&target,Action::View)); }
        let before=result.decisions[0].clone();req.checks.push(req.checks[0].clone());let batch=evaluate(&s,&req).unwrap();prop_assert_eq!(&batch.decisions[0],&before);prop_assert_eq!(&batch.decisions[1],&before);
  s.relationships.insert(edge(target.clone(),Relation::Editor,u.clone()));prop_assert_eq!(evaluate(&s,&req).unwrap().decisions[0].decision.clone(),Verdict::Allow);
 }
}
#[test]
fn proof_priority_and_errors() {
    let u = e(Kind::User, "u");
    let t = e(Kind::Team, "a");
    let f = e(Kind::Folder, "f");
    let d = e(Kind::Document, "d");
    let direct = edge(d.clone(), Relation::Viewer, u.clone());
    let mut s = Snapshot {
        administrators: Default::default(),
        entities: BTreeSet::from([u.clone(), t.clone(), f.clone(), d.clone()]),
        relationships: BTreeSet::from([
            direct.clone(),
            edge(t.clone(), Relation::Member, u.clone()),
            edge(d.clone(), Relation::Editor, t),
            edge(d.clone(), Relation::Parent, f),
        ]),
    };
    let mut q = request(u, d, Action::View);
    assert_eq!(
        evaluate(&s, &q).unwrap().decisions[0].proof,
        Some(vec![direct])
    );
    q.explain = false;
    assert!(evaluate(&s, &q).unwrap().decisions[0].proof.is_none());
    s.relationships.insert(edge(
        e(Kind::Folder, "f"),
        Relation::Parent,
        e(Kind::Folder, "f"),
    ));
    assert_eq!(evaluate(&s, &q).unwrap_err(), Fault::CorruptGraph);
}
#[test]
fn depth_and_budget_boundaries() {
    let u = e(Kind::User, "u");
    let mut s = Snapshot::default();
    s.entities.insert(u.clone());
    for i in 0..=33 {
        let f = e(Kind::Folder, &format!("f{i}"));
        s.entities.insert(f.clone());
        if i > 0 {
            s.relationships.insert(edge(
                f.clone(),
                Relation::Parent,
                e(Kind::Folder, &format!("f{}", i - 1)),
            ));
        }
        let q = request(u.clone(), f, Action::View);
        if i <= 32 {
            assert!(evaluate(&s, &q).is_ok());
        } else {
            assert_eq!(evaluate(&s, &q).unwrap_err(), Fault::CorruptGraph);
        }
    }
    let f = e(Kind::Folder, "f");
    let mut s = Snapshot::default();
    s.entities.extend([u.clone(), f.clone()]);
    // 1 state + 199 direct candidate grants = exactly 200 units per item.
    for i in 0..199 {
        let v = e(Kind::User, &format!("u{i}"));
        s.entities.insert(v.clone());
        s.relationships.insert(edge(f.clone(), Relation::Viewer, v));
    }
    let mut q = request(u.clone(), f.clone(), Action::View);
    q.checks = vec![q.checks[0].clone(); 100];
    assert_eq!(evaluate(&s, &q).unwrap().work, MAX_WORK);
    let v = e(Kind::User, "extra");
    s.entities.insert(v.clone());
    s.relationships.insert(edge(f, Relation::Viewer, v));
    assert_eq!(evaluate(&s, &q).unwrap_err(), Fault::EvaluationLimit);
}

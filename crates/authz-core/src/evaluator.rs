use crate::*;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Snapshot {
    #[serde(default)]
    pub administrators: BTreeSet<Entity>,
    pub entities: BTreeSet<Entity>,
    pub relationships: BTreeSet<Edge>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Verdict {
    Allow,
    Deny,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Reason {
    #[serde(rename = "MATCHING_RELATION")]
    MatchingRelation,
    #[serde(rename = "ORG_ADMIN")]
    OrgAdmin,
    #[serde(rename = "NOT_FOUND")]
    NotFound,
    #[serde(rename = "NO_MATCHING_RELATION")]
    NoMatchingRelation,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Decision {
    pub decision: Verdict,
    pub reason: Reason,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub proof: Option<Vec<Edge>>,
}
#[derive(Debug, Clone)]
pub struct Evaluation {
    pub decisions: Vec<Decision>,
    pub work: usize,
}
fn charge(work: &mut usize) -> Result<(), Fault> {
    *work += 1;
    if *work > MAX_WORK {
        Err(Fault::EvaluationLimit)
    } else {
        Ok(())
    }
}

pub fn evaluate(snapshot: &Snapshot, request: &BatchRequest) -> Result<Evaluation, Fault> {
    request.validate()?;
    if snapshot.entities.iter().filter(|e| e.resource()).count() > MAX_RESOURCES {
        return Err(Fault::EvaluationLimit);
    }
    if snapshot
        .administrators
        .iter()
        .any(|e| e.kind != Kind::User || !snapshot.entities.contains(e))
    {
        return Err(Fault::CorruptGraph);
    }
    let mut parents = BTreeMap::new();
    let mut grants: BTreeMap<&Entity, Vec<&Edge>> = BTreeMap::new();
    for edge in &snapshot.relationships {
        edge.validate().map_err(|_| Fault::CorruptGraph)?;
        if !snapshot.entities.contains(&edge.resource) || !snapshot.entities.contains(&edge.subject)
        {
            return Err(Fault::CorruptGraph);
        }
        match edge.relation {
            Relation::Parent => {
                if parents.insert(&edge.resource, edge).is_some() {
                    return Err(Fault::CorruptGraph);
                }
            }
            Relation::Viewer | Relation::Editor | Relation::Manager => {
                grants.entry(&edge.resource).or_default().push(edge);
            }
            Relation::Member => {}
        }
    }
    if grants.values().map(Vec::len).sum::<usize>() > MAX_GRANTS {
        return Err(Fault::EvaluationLimit);
    }
    // Validate the complete loaded hierarchy before producing any decision.
    for entity in snapshot.entities.iter().filter(|e| e.resource()) {
        let (mut cursor, mut visited) = (entity, BTreeSet::new());
        loop {
            if !visited.insert(cursor) || visited.len() > MAX_DEPTH + 1 {
                return Err(Fault::CorruptGraph);
            }
            match parents.get(cursor) {
                Some(parent) => cursor = &parent.subject,
                None => break,
            }
        }
    }
    for list in grants.values_mut() {
        list.sort_by(|a, b| {
            (a.subject.kind == Kind::Team, &a.subject.id, a.relation, *a).cmp(&(
                b.subject.kind == Kind::Team,
                &b.subject.id,
                b.relation,
                *b,
            ))
        });
    }
    let (mut decisions, mut work) = (Vec::with_capacity(request.checks.len()), 0);
    for check in &request.checks {
        if !snapshot.entities.contains(&request.subject)
            || !snapshot.entities.contains(&check.resource)
        {
            decisions.push(Decision {
                decision: Verdict::Deny,
                reason: Reason::NotFound,
                proof: request.explain.then(Vec::new),
            });
            continue;
        }
        if snapshot.administrators.contains(&request.subject) {
            decisions.push(Decision {
                decision: Verdict::Allow,
                reason: Reason::OrgAdmin,
                proof: request.explain.then(Vec::new),
            });
            continue;
        }
        let (mut cursor, mut path, mut proof) = (&check.resource, Vec::new(), None);
        loop {
            charge(&mut work)?;
            if let Some(list) = grants.get(cursor) {
                for &grant in list {
                    charge(&mut work)?;
                    let membership = if grant.subject.kind == Kind::Team {
                        charge(&mut work)?;
                        Some(Edge {
                            resource: grant.subject.clone(),
                            relation: Relation::Member,
                            subject: request.subject.clone(),
                            subject_relation: String::new(),
                        })
                    } else {
                        None
                    };
                    let matches = membership
                        .as_ref()
                        .map_or(grant.subject == request.subject, |m| {
                            snapshot.relationships.contains(m)
                        });
                    if matches
                        && (grant.relation == Relation::Manager
                            || check.action == Action::View
                            || (check.action == Action::Edit && grant.relation == Relation::Editor))
                        && proof.is_none()
                    {
                        let mut p = path.clone();
                        p.push(grant.clone());
                        if let Some(m) = membership {
                            p.push(m);
                        }
                        proof = Some(p);
                    }
                }
            }
            match parents.get(cursor) {
                Some(edge) => {
                    path.push((*edge).clone());
                    cursor = &edge.subject
                }
                None => break,
            }
        }
        let allowed = proof.is_some();
        decisions.push(Decision {
            decision: if allowed {
                Verdict::Allow
            } else {
                Verdict::Deny
            },
            reason: if allowed {
                Reason::MatchingRelation
            } else {
                Reason::NoMatchingRelation
            },
            proof: if request.explain {
                Some(proof.unwrap_or_default())
            } else {
                None
            },
        });
    }
    Ok(Evaluation { decisions, work })
}

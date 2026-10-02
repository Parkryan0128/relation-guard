use serde::{Deserialize, Deserializer, Serialize, de};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;

pub const MODEL: &str = "org-doc-v2";
pub const MAX_BODY: usize = 262_144;
pub const MAX_DEPTH: usize = 32;
pub const MAX_ITEMS: usize = 100;
pub const MAX_RESOURCES: usize = 3_300;
pub const MAX_GRANTS: usize = 10_000;
pub const MAX_WORK: usize = 20_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum Fault {
    #[error("INVALID_REQUEST")]
    InvalidRequest,
    #[error("DUPLICATE_OPERATION")]
    DuplicateOperation,
    #[error("INVALID_RELATION")]
    InvalidRelation,
    #[error("EVALUATION_LIMIT")]
    EvaluationLimit,
    #[error("INTERNAL_ERROR")]
    CorruptGraph,
    #[error("PAYLOAD_TOO_LARGE")]
    PayloadTooLarge,
}
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct Id(String);
impl Id {
    pub fn new(s: impl Into<String>) -> Result<Self, Fault> {
        let s = s.into();
        if s.is_empty()
            || s.len() > 128
            || !s.as_bytes()[0].is_ascii_alphanumeric()
            || !s
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
        {
            return Err(Fault::InvalidRequest);
        }
        Ok(Self(s))
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
impl<'de> Deserialize<'de> for Id {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        Self::new(String::deserialize(d)?).map_err(de::Error::custom)
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Document,
    Folder,
    Team,
    User,
}
impl Kind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Document => "document",
            Self::Folder => "folder",
            Self::Team => "team",
            Self::User => "user",
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Entity {
    pub kind: Kind,
    pub id: Id,
}
impl Entity {
    pub fn resource(&self) -> bool {
        matches!(self.kind, Kind::Folder | Kind::Document)
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Relation {
    Editor,
    Member,
    Manager,
    Parent,
    Viewer,
}
impl Relation {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Editor => "editor",
            Self::Member => "member",
            Self::Manager => "manager",
            Self::Parent => "parent",
            Self::Viewer => "viewer",
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Edge {
    pub resource: Entity,
    pub relation: Relation,
    pub subject: Entity,
    pub subject_relation: String,
}
impl Edge {
    pub fn validate(&self) -> Result<(), Fault> {
        let valid = match self.relation {
            Relation::Member => {
                self.resource.kind == Kind::Team
                    && self.subject.kind == Kind::User
                    && self.subject_relation.is_empty()
            }
            Relation::Parent => {
                self.resource.resource()
                    && self.subject.kind == Kind::Folder
                    && self.subject_relation.is_empty()
            }
            Relation::Viewer | Relation::Editor | Relation::Manager => {
                self.resource.resource()
                    && ((self.subject.kind == Kind::User && self.subject_relation.is_empty())
                        || (self.subject.kind == Kind::Team && self.subject_relation == "member"))
            }
        };
        if valid {
            Ok(())
        } else {
            Err(Fault::InvalidRelation)
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Action {
    View,
    Edit,
    Manage,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Check {
    pub resource: Entity,
    pub action: Action,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CheckRequest {
    pub subject: Entity,
    pub resource: Entity,
    pub action: Action,
    #[serde(default)]
    pub explain: bool,
}
impl CheckRequest {
    pub fn batch(self) -> BatchRequest {
        BatchRequest {
            subject: self.subject,
            checks: vec![Check {
                resource: self.resource,
                action: self.action,
            }],
            explain: self.explain,
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BatchRequest {
    pub subject: Entity,
    pub checks: Vec<Check>,
    #[serde(default)]
    pub explain: bool,
}
impl BatchRequest {
    pub fn validate(&self) -> Result<(), Fault> {
        if self.subject.kind != Kind::User
            || self.checks.is_empty()
            || self.checks.len() > MAX_ITEMS
            || self.checks.iter().any(|c| !c.resource.resource())
        {
            Err(Fault::InvalidRequest)
        } else {
            Ok(())
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(into = "String")]
pub struct Revision(pub i64);
impl From<Revision> for String {
    fn from(v: Revision) -> String {
        v.0.to_string()
    }
}
impl<'de> Deserialize<'de> for Revision {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        if s.is_empty()
            || (s.len() > 1 && s.starts_with('0'))
            || !s.bytes().all(|b| b.is_ascii_digit())
        {
            return Err(de::Error::custom("invalid revision"));
        }
        let n = s.parse::<i64>().map_err(de::Error::custom)?;
        Ok(Self(n))
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OrgRole {
    Admin,
    Member,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum Operation {
    SetOrgRole { user_id: Id, role: OrgRole },
    PutEntity { entity: Entity },
    DeleteEntity { entity: Entity },
    PutRelation { relationship: Edge },
    DeleteRelation { relationship: Edge },
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WriteRequest {
    pub expected_revision: Revision,
    pub operations: Vec<Operation>,
}
impl WriteRequest {
    pub fn validate(&self) -> Result<(), Fault> {
        if self.operations.is_empty() || self.operations.len() > MAX_ITEMS {
            return Err(Fault::InvalidRequest);
        }
        let mut entities = BTreeSet::new();
        let mut edges = BTreeSet::new();
        let mut roles = BTreeSet::new();
        for op in &self.operations {
            match op {
                Operation::SetOrgRole { user_id, .. } => {
                    if !roles.insert(user_id) {
                        return Err(Fault::DuplicateOperation);
                    }
                }
                Operation::PutEntity { entity } | Operation::DeleteEntity { entity } => {
                    if !entities.insert(entity) {
                        return Err(Fault::DuplicateOperation);
                    }
                }
                Operation::PutRelation { relationship }
                | Operation::DeleteRelation { relationship } => {
                    relationship.validate()?;
                    if !edges.insert(relationship) {
                        return Err(Fault::DuplicateOperation);
                    }
                }
            }
        }
        for op in &self.operations {
            if let Operation::DeleteEntity { entity } = op
                && entity.kind == Kind::User
                && roles.contains(&entity.id)
            {
                return Err(Fault::DuplicateOperation);
            }
        }
        Ok(())
    }
    pub fn canonical(&self) -> Result<Vec<u8>, Fault> {
        self.validate()?;
        let mut ops = self
            .operations
            .iter()
            .map(|o| serde_json::to_value(o).map_err(|_| Fault::InvalidRequest))
            .collect::<Result<Vec<_>, _>>()?;
        // serde_json's default Map is sorted; all write strings are restricted ASCII.
        ops.sort_by_cached_key(Value::to_string);
        serde_json::to_vec(
            &serde_json::json!({"expected_revision":self.expected_revision,"operations":ops}),
        )
        .map_err(|_| Fault::InvalidRequest)
    }
    pub fn hash(&self) -> Result<Vec<u8>, Fault> {
        Ok(Sha256::digest(self.canonical()?).to_vec())
    }
}

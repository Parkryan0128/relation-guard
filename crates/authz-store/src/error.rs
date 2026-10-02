use authz_core::Fault;

#[derive(Debug, thiserror::Error)]
#[error("{code}")]
pub struct StoreError {
    pub code: &'static str,
    pub current_revision: Option<i64>,
}
impl StoreError {
    pub fn new(code: &'static str) -> Self {
        Self {
            code,
            current_revision: None,
        }
    }
}
impl From<Fault> for StoreError {
    fn from(e: Fault) -> Self {
        Self::new(match e {
            Fault::InvalidRequest => "INVALID_REQUEST",
            Fault::DuplicateOperation => "DUPLICATE_OPERATION",
            Fault::InvalidRelation => "INVALID_RELATION",
            Fault::EvaluationLimit => "EVALUATION_LIMIT",
            Fault::CorruptGraph => "INTERNAL_ERROR",
            Fault::PayloadTooLarge => "PAYLOAD_TOO_LARGE",
        })
    }
}
impl From<sqlx::Error> for StoreError {
    fn from(e: sqlx::Error) -> Self {
        let code = match &e {
            sqlx::Error::PoolTimedOut => "OVERLOADED",
            sqlx::Error::Io(_) | sqlx::Error::Tls(_) | sqlx::Error::PoolClosed => {
                "DATABASE_UNAVAILABLE"
            }
            sqlx::Error::Database(d) => match d.code().as_deref() {
                Some("55P03") => "OVERLOADED",
                Some("55000") if d.message().contains("not currently accepting connections") => {
                    "DATABASE_UNAVAILABLE"
                }
                Some("57014") => "DEADLINE_EXCEEDED",
                Some("23503") => "UNKNOWN_ENTITY",
                Some("23505") => "INVALID_HIERARCHY",
                Some("23514") if d.constraint() == Some("no_self_parent") => "INVALID_HIERARCHY",
                Some("23514") => "INVALID_RELATION",
                Some(c) if c.starts_with("08") || matches!(c, "57P01" | "57P02" | "57P03") => {
                    "DATABASE_UNAVAILABLE"
                }
                _ => "INTERNAL_ERROR",
            },
            _ => "INTERNAL_ERROR",
        };
        Self::new(code)
    }
}

//! Typed contracts and pure authorization rules. No networking or database access.
pub mod contract;
pub use contract::*;
pub mod evaluator;
pub use evaluator::*;

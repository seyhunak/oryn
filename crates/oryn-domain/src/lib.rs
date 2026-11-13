//! ORYN domain model.
//!
//! This crate is deliberately free of I/O: it defines the vocabulary every other
//! crate speaks — roles and permissions, lifecycle statuses, the executable step
//! model produced by the BDD compiler and consumed by the execution engine, and
//! the job protocol exchanged between the API and workers.

#[macro_use]
pub mod enums;
pub mod job;
pub mod rbac;
pub mod steps;

pub use enums::*;
pub use job::*;
pub use rbac::*;
pub use steps::*;

/// Error returned when parsing a domain enum from its string form fails.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
#[error("invalid {kind}: {value}")]
pub struct ParseEnumError {
    pub kind: &'static str,
    pub value: String,
}

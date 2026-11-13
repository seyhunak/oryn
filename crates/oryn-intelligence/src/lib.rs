//! Deterministic intelligence first, AI second.
//!
//! Every module here produces a useful result with no model configured
//! (repository analysis, the heuristic planner, deterministic failure
//! classification, regression impact). AI, when configured, adds proposals and
//! interpretation on top. Those are always labelled and never become evidence.

pub mod analysis;
pub mod import;
pub mod investigator;
pub mod planner;
pub mod regression;

//! ORYN execution engine.
//!
//! Usable without the web application:
//!
//!
//! * [`lifecycle`] — the job state machine and legal transitions.
//! * [`retry`] — retry policies.
//! * [`browser`] — the versioned Playwright adapter protocol (`oryn.browser/v1`)
//!   and an HTTP client for it.
//! * [`runner`] — executes a [`oryn_domain::JobSpec`] (browser + API steps),
//!   evaluating every expectation in Rust, honouring timeouts and cancellation.
//! * [`queue`] — an in-memory job queue with leases, for embedding the engine
//!   (the control plane uses the PostgreSQL implementation in `oryn-db`).

pub mod browser;
pub mod lifecycle;
pub mod queue;
pub mod retry;
pub mod runner;

pub use browser::{BrowserAdapter, HttpBrowserAdapter};
pub use runner::{ArtifactPayload, JobOutput, ScenarioRunner};

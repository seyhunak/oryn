//! The job protocol exchanged between the control plane (API) and workers.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use utoipa::ToSchema;
use uuid::Uuid;

use crate::{ArtifactKind, Browser, ExecStep, JobStatus, StepStatus};

pub const JOB_PROTOCOL: &str = "oryn.job/v1";

/// Execution environment resolved for a job. Secret variables are decrypted by
/// the control plane only for the claiming worker and are redacted from evidence.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema, Default)]
pub struct JobEnvironment {
    pub name: String,
    pub base_url: Option<String>,
    pub api_base_url: Option<String>,
    #[serde(default)]
    pub variables: BTreeMap<String, String>,
    /// Names of variables whose values must be redacted from logs and evidence.
    #[serde(default)]
    pub secret_names: Vec<String>,
    /// Hosts the job may contact. Empty = derived from base URLs.
    #[serde(default)]
    pub allowed_hosts: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct JobOptions {
    pub browser: Browser,
    pub step_timeout_ms: u64,
    pub job_timeout_ms: u64,
    pub screenshots: ScreenshotMode,
    pub record_video: bool,
    pub record_trace: bool,
}

impl Default for JobOptions {
    fn default() -> Self {
        Self {
            browser: Browser::Chromium,
            step_timeout_ms: 10_000,
            job_timeout_ms: 300_000,
            screenshots: ScreenshotMode::OnFailure,
            record_video: false,
            record_trace: false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum ScreenshotMode {
    Off,
    OnFailure,
    EveryStep,
}

/// A step with its human-readable source text (the Gherkin line).
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct PlannedStep {
    pub index: usize,
    pub keyword: String,
    pub text: String,
    pub step: ExecStep,
}

/// What a worker receives when it claims a job.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct JobSpec {
    pub protocol: String,
    pub job_id: Uuid,
    pub execution_id: Uuid,
    pub project_id: Uuid,
    pub scenario_id: Option<Uuid>,
    pub scenario_name: String,
    pub attempt: i32,
    pub steps: Vec<PlannedStep>,
    pub environment: JobEnvironment,
    pub options: JobOptions,
}

/// Result of one step, produced by the worker's engine.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct StepResult {
    pub index: usize,
    pub keyword: String,
    pub text: String,
    pub status: StepStatus,
    pub started_at: DateTime<Utc>,
    pub duration_ms: u64,
    /// Raw observation (e.g. adapter observation or HTTP response summary).
    pub observation: Option<serde_json::Value>,
    /// Human-readable reason for failure, produced by deterministic evaluation.
    pub message: Option<String>,
    /// Expected vs actual, for assertions.
    pub expected: Option<String>,
    pub actual: Option<String>,
    /// Name of the artifact (screenshot) attached to this step, if any.
    pub artifact: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct LogLine {
    pub timestamp: DateTime<Utc>,
    pub level: String,
    pub source: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct NetworkEvent {
    pub url: String,
    pub method: String,
    pub status: Option<u16>,
    pub failure: Option<String>,
}

/// Artifact produced by a job, uploaded separately; the result references it by name.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ArtifactRef {
    pub name: String,
    pub kind: ArtifactKind,
    pub content_type: String,
    pub size_bytes: u64,
}

/// What a worker reports when a job finishes.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct JobResult {
    pub protocol: String,
    pub job_id: Uuid,
    pub status: JobStatus,
    pub started_at: DateTime<Utc>,
    pub finished_at: DateTime<Utc>,
    pub duration_ms: u64,
    pub steps: Vec<StepResult>,
    pub logs: Vec<LogLine>,
    pub network: Vec<NetworkEvent>,
    pub artifacts: Vec<ArtifactRef>,
    pub browser_version: Option<String>,
    /// Infrastructure error (adapter unreachable, timeout) as opposed to an assertion failure.
    pub error: Option<String>,
}

impl JobResult {
    /// Derive the job status from step results: any step error → error; any failure → failed.
    pub fn status_from_steps(steps: &[StepResult]) -> JobStatus {
        if steps.iter().any(|s| s.status == StepStatus::Error) {
            JobStatus::Error
        } else if steps.iter().any(|s| s.status == StepStatus::Failed) {
            JobStatus::Failed
        } else {
            JobStatus::Passed
        }
    }
}

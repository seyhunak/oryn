//! Scenario runner: executes a [`JobSpec`] and produces a [`JobResult`] with evidence.
//!
//! Steps run in order. Contiguous browser steps form a *segment* sent to the
//! browser adapter in one run (one isolated browser context); API steps run in
//! the Rust API engine. The first failing step stops the scenario and the rest
//! are reported as skipped. Job timeout and cancellation are checked around
//! every segment and step, so partial evidence is always kept.

use base64::Engine;
use chrono::Utc;
use oryn_api_engine::{ApiSession, NetworkPolicy};
use oryn_auth::redact::redact_with;
use oryn_domain::{
    ArtifactKind, ArtifactRef, ExecStep, JobResult, JobSpec, JobStatus, LogLine, NetworkEvent,
    PlannedStep, ScreenshotMode, StepResult, StepStatus, JOB_PROTOCOL,
};
use std::collections::BTreeMap;
use std::str::FromStr;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio_util::sync::CancellationToken;

use crate::browser::{
    evaluate, to_adapter_step, BrowserAdapter, RunRequest, Viewport, BROWSER_PROTOCOL,
};

/// Binary artifact produced by a run, to be persisted by the caller.
#[derive(Debug, Clone)]
pub struct ArtifactPayload {
    pub name: String,
    pub kind: ArtifactKind,
    pub content_type: String,
    pub bytes: Vec<u8>,
}

#[derive(Debug, Clone)]
pub struct JobOutput {
    pub result: JobResult,
    pub artifacts: Vec<ArtifactPayload>,
}

pub struct ScenarioRunner {
    browser: Option<Arc<dyn BrowserAdapter>>,
}

enum Interrupt {
    Timeout,
    Cancelled,
}

impl ScenarioRunner {
    pub fn new(browser: Option<Arc<dyn BrowserAdapter>>) -> Self {
        Self { browser }
    }

    pub async fn run(&self, spec: &JobSpec, cancel: CancellationToken) -> JobOutput {
        let started_at = Utc::now();
        let clock = Instant::now();
        let deadline = clock + Duration::from_millis(spec.options.job_timeout_ms.max(1_000));
        let secrets: Vec<String> = spec
            .environment
            .secret_names
            .iter()
            .filter_map(|n| spec.environment.variables.get(n).cloned())
            .collect();

        let mut logs = vec![log(
            "info",
            "engine",
            format!(
                "Job {} attempt {} started: {}",
                spec.job_id, spec.attempt, spec.scenario_name
            ),
        )];
        let mut network = Vec::new();
        let mut artifacts: Vec<ArtifactPayload> = Vec::new();
        let mut results: Vec<StepResult> = Vec::new();
        let mut browser_version = None;
        let mut infra_error: Option<String> = None;
        let mut interrupted: Option<Interrupt> = None;

        let mut api = ApiSession::new(
            spec.environment
                .api_base_url
                .clone()
                .or(spec.environment.base_url.clone()),
            spec.environment.variables.clone(),
            NetworkPolicy {
                allowed_hosts: allowed_hosts(spec),
                block_private: false,
            },
            Duration::from_millis(spec.options.step_timeout_ms.max(1_000)),
        );

        let mut stop = false;
        let mut i = 0;
        let steps = &spec.steps;
        while i < steps.len() {
            if stop {
                results.push(skipped(&steps[i], "Skipped after an earlier step failed"));
                i += 1;
                continue;
            }
            if cancel.is_cancelled() {
                interrupted = Some(Interrupt::Cancelled);
                break;
            }
            if Instant::now() >= deadline {
                interrupted = Some(Interrupt::Timeout);
                break;
            }
            if steps[i].step.is_browser() {
                let mut j = i;
                while j < steps.len() && steps[j].step.is_browser() {
                    j += 1;
                }
                let segment = &steps[i..j];
                let remaining = deadline.saturating_duration_since(Instant::now());
                let outcome = tokio::select! {
                    r = self.run_browser_segment(spec, segment, &api.vars) => Some(r),
                    _ = tokio::time::sleep(remaining) => { interrupted = Some(Interrupt::Timeout); None }
                    _ = cancel.cancelled() => { interrupted = Some(Interrupt::Cancelled); None }
                };
                let Some(seg) = outcome else { break };
                match seg {
                    Ok(mut seg) => {
                        browser_version = seg.browser_version.or(browser_version);
                        logs.append(&mut seg.logs);
                        network.append(&mut seg.network);
                        artifacts.append(&mut seg.artifacts);
                        stop = seg
                            .results
                            .iter()
                            .any(|r| matches!(r.status, StepStatus::Failed | StepStatus::Error));
                        results.append(&mut seg.results);
                    }
                    Err(e) => {
                        logs.push(log("error", "engine", e.clone()));
                        infra_error = Some(e.clone());
                        for (k, s) in segment.iter().enumerate() {
                            if k == 0 {
                                let mut r = skipped(s, &e);
                                r.status = StepStatus::Error;
                                results.push(r);
                            } else {
                                results.push(skipped(s, "Skipped: browser adapter unavailable"));
                            }
                        }
                        stop = true;
                    }
                }
                i = j;
            } else {
                let ps = &steps[i];
                let t0 = Utc::now();
                let c = Instant::now();
                let remaining = deadline.saturating_duration_since(Instant::now());
                let out = tokio::select! {
                    o = api.execute(&ps.step) => Some(o),
                    _ = tokio::time::sleep(remaining) => { interrupted = Some(Interrupt::Timeout); None }
                    _ = cancel.cancelled() => { interrupted = Some(Interrupt::Cancelled); None }
                };
                let Some(o) = out else { break };
                if let Some(obs) = &o.observation {
                    if let (Some(url), Some(method)) = (
                        obs.get("url").and_then(|v| v.as_str()),
                        obs.get("method").and_then(|v| v.as_str()),
                    ) {
                        network.push(NetworkEvent {
                            url: url.to_string(),
                            method: method.to_string(),
                            status: obs.get("status").and_then(|v| v.as_u64()).map(|v| v as u16),
                            failure: None,
                        });
                    }
                }
                let msg = o.message.as_ref().map(|m| redact_with(m, &secrets));
                if let Some(m) = &msg {
                    logs.push(log(
                        if o.status == StepStatus::Error {
                            "error"
                        } else {
                            "warn"
                        },
                        "api",
                        format!("{}: {m}", ps.text),
                    ));
                }
                let response_artifact = if matches!(ps.step, ExecStep::HttpRequest { .. }) {
                    o.observation.as_ref().map(|obs| {
                        let name = format!("step-{}-response.json", ps.index);
                        let body = redact_with(
                            &serde_json::to_string_pretty(obs).unwrap_or_default(),
                            &secrets,
                        );
                        artifacts.push(ArtifactPayload {
                            name: name.clone(),
                            kind: ArtifactKind::Response,
                            content_type: "application/json".into(),
                            bytes: body.into_bytes(),
                        });
                        name
                    })
                } else {
                    None
                };
                stop = matches!(o.status, StepStatus::Failed | StepStatus::Error);
                results.push(StepResult {
                    index: ps.index,
                    keyword: ps.keyword.clone(),
                    text: ps.text.clone(),
                    status: o.status,
                    started_at: t0,
                    duration_ms: c.elapsed().as_millis() as u64,
                    observation: o.observation.map(|v| redact_value(v, &secrets)),
                    message: msg,
                    expected: o.expected,
                    actual: o.actual.map(|a| redact_with(&a, &secrets)),
                    artifact: response_artifact,
                });
                i += 1;
            }
        }

        // Anything not reached is skipped.
        let reached = results.len();
        let reason = match interrupted {
            Some(Interrupt::Timeout) => "Not run: job timed out",
            Some(Interrupt::Cancelled) => "Not run: job cancelled",
            None => "Not run",
        };
        for s in steps.iter().skip(reached) {
            results.push(skipped(s, reason));
        }

        let status = match interrupted {
            Some(Interrupt::Timeout) => {
                logs.push(log(
                    "error",
                    "engine",
                    format!(
                        "Job exceeded its timeout of {} ms",
                        spec.options.job_timeout_ms
                    ),
                ));
                JobStatus::TimedOut
            }
            Some(Interrupt::Cancelled) => {
                logs.push(log("warn", "engine", "Job cancelled".into()));
                JobStatus::Cancelled
            }
            None if steps.is_empty() => {
                infra_error = Some("Scenario has no executable steps".into());
                JobStatus::Error
            }
            None => JobResult::status_from_steps(&results),
        };
        logs.push(log("info", "engine", format!("Job finished: {status}")));

        let refs = artifacts
            .iter()
            .map(|a| ArtifactRef {
                name: a.name.clone(),
                kind: a.kind,
                content_type: a.content_type.clone(),
                size_bytes: a.bytes.len() as u64,
            })
            .collect();
        let finished_at = Utc::now();
        JobOutput {
            result: JobResult {
                protocol: JOB_PROTOCOL.into(),
                job_id: spec.job_id,
                status,
                started_at,
                finished_at,
                duration_ms: clock.elapsed().as_millis() as u64,
                steps: results,
                logs: logs
                    .into_iter()
                    .map(|mut l| {
                        l.message = redact_with(&l.message, &secrets);
                        l
                    })
                    .collect(),
                network,
                artifacts: refs,
                browser_version,
                error: infra_error.map(|e| redact_with(&e, &secrets)),
            },
            artifacts,
        }
    }

    async fn run_browser_segment(
        &self,
        spec: &JobSpec,
        segment: &[PlannedStep],
        vars: &BTreeMap<String, String>,
    ) -> Result<Segment, String> {
        let adapter = self
            .browser
            .as_ref()
            .ok_or("No browser adapter configured on this worker")?;
        let base_url = spec
            .environment
            .base_url
            .clone()
            .ok_or("Environment has no base URL for browser steps")?;
        let interpolated: Vec<ExecStep> =
            segment.iter().map(|s| s.step.interpolate(vars)).collect();
        let adapter_steps = interpolated
            .iter()
            .zip(segment)
            .filter_map(|(st, ps)| {
                to_adapter_step(&format!("s{}", ps.index), st, spec.options.step_timeout_ms)
            })
            .collect();
        let req = RunRequest {
            protocol: BROWSER_PROTOCOL.into(),
            run_id: format!("{}-{}", spec.job_id, segment[0].index),
            browser: spec.options.browser.as_str().into(),
            base_url,
            viewport: Viewport {
                width: 1280,
                height: 800,
            },
            step_timeout_ms: spec.options.step_timeout_ms,
            screenshots: match spec.options.screenshots {
                ScreenshotMode::Off => "off",
                ScreenshotMode::OnFailure => "on_failure",
                ScreenshotMode::EveryStep => "every_step",
            }
            .into(),
            record_video: spec.options.record_video,
            record_trace: spec.options.record_trace,
            headers: BTreeMap::new(),
            storage_state: None,
            allowed_hosts: allowed_hosts(spec),
            steps: adapter_steps,
        };
        let resp = adapter.run(req).await.map_err(|e| e.to_string())?;
        let secrets: Vec<String> = spec
            .environment
            .secret_names
            .iter()
            .filter_map(|n| spec.environment.variables.get(n).cloned())
            .collect();

        let mut results = Vec::new();
        let mut failed_before = false;
        for (ps, st) in segment.iter().zip(&interpolated) {
            let id = format!("s{}", ps.index);
            let rep = resp.steps.iter().find(|r| r.id == id);
            let r = match rep {
                None => {
                    let mut r = skipped(ps, "Adapter did not report this step");
                    if !failed_before {
                        r.status = StepStatus::Error;
                    }
                    r
                }
                Some(rep) => {
                    let (mut status, message, expected, actual) = evaluate(st, rep);
                    if failed_before && status != StepStatus::Skipped {
                        // Observations after a failed expectation are not trusted as verdicts.
                        status = StepStatus::Skipped;
                    }
                    StepResult {
                        index: ps.index,
                        keyword: ps.keyword.clone(),
                        text: redact_with(&ps.text, &secrets),
                        status,
                        started_at: rep
                            .started_at
                            .as_deref()
                            .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
                            .map(|d| d.with_timezone(&Utc))
                            .unwrap_or_else(Utc::now),
                        duration_ms: rep.duration_ms,
                        observation: rep.observation.clone().map(|v| redact_value(v, &secrets)),
                        message: message.map(|m| redact_with(&m, &secrets)),
                        expected,
                        actual: actual.map(|a| redact_with(&a, &secrets)),
                        artifact: rep.screenshot.clone(),
                    }
                }
            };
            failed_before |= matches!(r.status, StepStatus::Failed | StepStatus::Error);
            results.push(r);
        }

        let mut logs: Vec<LogLine> = resp
            .console
            .iter()
            .map(|c| LogLine {
                timestamp: c
                    .timestamp
                    .as_deref()
                    .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
                    .map(|d| d.with_timezone(&Utc))
                    .unwrap_or_else(Utc::now),
                level: match c.kind.as_str() {
                    "error" => "error",
                    "warning" => "warn",
                    _ => "info",
                }
                .into(),
                source: "browser".into(),
                message: redact_with(&c.text, &secrets),
            })
            .collect();
        logs.insert(
            0,
            log(
                "info",
                "engine",
                format!(
                    "Browser segment {} ran on {} {} in {} ms",
                    resp.run_id,
                    resp.browser.clone().unwrap_or_default(),
                    resp.browser_version.clone().unwrap_or_default(),
                    resp.duration_ms
                ),
            ),
        );
        let network = resp
            .network
            .iter()
            .map(|n| NetworkEvent {
                url: n.url.clone(),
                method: n.method.clone(),
                status: n.status,
                failure: n.failure.clone(),
            })
            .collect();
        let artifacts = resp
            .artifacts
            .iter()
            .filter_map(|a| {
                let bytes = base64::engine::general_purpose::STANDARD
                    .decode(&a.data_base64)
                    .ok()?;
                Some(ArtifactPayload {
                    name: a.name.clone(),
                    kind: ArtifactKind::from_str(&a.kind).unwrap_or(ArtifactKind::Screenshot),
                    content_type: a.content_type.clone(),
                    bytes,
                })
            })
            .collect();
        Ok(Segment {
            results,
            logs,
            network,
            artifacts,
            browser_version: resp.browser_version,
        })
    }
}

struct Segment {
    results: Vec<StepResult>,
    logs: Vec<LogLine>,
    network: Vec<NetworkEvent>,
    artifacts: Vec<ArtifactPayload>,
    browser_version: Option<String>,
}

/// The environment's own base and API hosts are always reachable; declared
/// `allowed_hosts` extend that set (exact names or `*.domain`).
fn allowed_hosts(spec: &JobSpec) -> Vec<String> {
    let mut hosts: Vec<String> = [&spec.environment.base_url, &spec.environment.api_base_url]
        .iter()
        .filter_map(|u| u.as_deref())
        .filter_map(|u| url::Url::parse(u).ok())
        .filter_map(|u| u.host_str().map(str::to_string))
        .chain(spec.environment.allowed_hosts.iter().cloned())
        .collect();
    hosts.sort();
    hosts.dedup();
    hosts
}

fn redact_value(v: serde_json::Value, secrets: &[String]) -> serde_json::Value {
    if secrets.is_empty() {
        return v;
    }
    let s = redact_with(&v.to_string(), secrets);
    serde_json::from_str(&s).unwrap_or(serde_json::Value::String(s))
}

fn skipped(s: &PlannedStep, why: &str) -> StepResult {
    StepResult {
        index: s.index,
        keyword: s.keyword.clone(),
        text: s.text.clone(),
        status: StepStatus::Skipped,
        started_at: Utc::now(),
        duration_ms: 0,
        observation: None,
        message: Some(why.to_string()),
        expected: None,
        actual: None,
        artifact: None,
    }
}

fn log(level: &str, source: &str, message: String) -> LogLine {
    LogLine {
        timestamp: Utc::now(),
        level: level.into(),
        source: source.into(),
        message,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::browser::{AdapterCallError, AdapterHealth, AdapterStep, RunResponse};
    use async_trait::async_trait;
    use oryn_domain::{JobEnvironment, JobOptions};
    use uuid::Uuid;

    /// A fake adapter that "sees" a fixed page text and URL.
    struct FakePage {
        text: &'static str,
        url: &'static str,
        delay_ms: u64,
    }

    #[async_trait]
    impl BrowserAdapter for FakePage {
        async fn run(&self, req: RunRequest) -> Result<RunResponse, AdapterCallError> {
            tokio::time::sleep(Duration::from_millis(self.delay_ms)).await;
            let mut out = vec![];
            let mut broken = false;
            for s in &req.steps {
                let id = s["id"].as_str().unwrap().to_string();
                let action = s["action"].as_str().unwrap().to_string();
                if broken {
                    out.push(AdapterStep {
                        id,
                        action,
                        status: "skipped".into(),
                        started_at: None,
                        duration_ms: 0,
                        observation: None,
                        error: None,
                        screenshot: None,
                    });
                    continue;
                }
                let (status, obs, err) = match action.as_str() {
                    "observe_text" => {
                        let t = s["text"].as_str().unwrap();
                        (
                            "ok",
                            Some(serde_json::json!({"visible": self.text.contains(t), "count": 1})),
                            None,
                        )
                    }
                    "observe_url" => ("ok", Some(serde_json::json!({"url": self.url})), None),
                    "click" if s["target"] == "Missing" => {
                        broken = true;
                        (
                            "error",
                            None,
                            Some(crate::browser::AdapterError {
                                kind: "not_found".into(),
                                message: "no element".into(),
                            }),
                        )
                    }
                    _ => ("ok", None, None),
                };
                out.push(AdapterStep {
                    id,
                    action,
                    status: status.into(),
                    started_at: None,
                    duration_ms: 3,
                    observation: obs,
                    error: err,
                    screenshot: None,
                });
            }
            Ok(RunResponse {
                protocol: BROWSER_PROTOCOL.into(),
                run_id: req.run_id,
                status: "completed".into(),
                browser: Some("chromium".into()),
                browser_version: Some("1.0".into()),
                duration_ms: 5,
                steps: out,
                artifacts: vec![],
                console: vec![],
                network: vec![],
            })
        }
        async fn health(&self) -> Result<AdapterHealth, AdapterCallError> {
            Ok(AdapterHealth {
                status: "ok".into(),
                protocol: BROWSER_PROTOCOL.into(),
                browsers: vec![],
            })
        }
    }

    fn spec(src: &str, timeout_ms: u64) -> JobSpec {
        let f = oryn_bdd::compile(src).unwrap();
        JobSpec {
            protocol: JOB_PROTOCOL.into(),
            job_id: Uuid::new_v4(),
            execution_id: Uuid::new_v4(),
            project_id: Uuid::new_v4(),
            scenario_id: None,
            scenario_name: f.scenarios[0].name.clone(),
            attempt: 1,
            steps: f.scenarios[0].steps.clone(),
            environment: JobEnvironment {
                name: "test".into(),
                base_url: Some("http://localhost:4000".into()),
                ..Default::default()
            },
            options: JobOptions {
                job_timeout_ms: timeout_ms,
                ..Default::default()
            },
        }
    }

    fn runner(delay_ms: u64) -> ScenarioRunner {
        ScenarioRunner::new(Some(Arc::new(FakePage {
            text: "Welcome, Ada",
            url: "http://localhost:4000/dashboard",
            delay_ms,
        })))
    }

    #[tokio::test]
    async fn passes_when_observations_match() {
        let s = spec("Feature: f\n Scenario: s\n  Given I open \"/\"\n  Then I should see \"Welcome\"\n  And the URL should contain \"/dashboard\"\n", 10_000);
        let out = runner(0).run(&s, CancellationToken::new()).await;
        assert_eq!(
            out.result.status,
            JobStatus::Passed,
            "{:?}",
            out.result.steps
        );
        assert!(out
            .result
            .steps
            .iter()
            .all(|s| s.status == StepStatus::Passed));
    }

    #[tokio::test]
    async fn fails_and_skips_rest() {
        let s = spec("Feature: f\n Scenario: s\n  Given I open \"/\"\n  Then I should see \"Order confirmed\"\n  And the URL should contain \"/dashboard\"\n", 10_000);
        let out = runner(0).run(&s, CancellationToken::new()).await;
        assert_eq!(out.result.status, JobStatus::Failed);
        assert_eq!(out.result.steps[1].status, StepStatus::Failed);
        assert_eq!(out.result.steps[2].status, StepStatus::Skipped);
        let s = spec("Feature: f\n Scenario: s\n  Given I open \"/\"\n  When I click \"Missing\"\n  Then I should see \"Welcome\"\n", 10_000);
        let out = runner(0).run(&s, CancellationToken::new()).await;
        assert_eq!(out.result.status, JobStatus::Failed);
        assert_eq!(out.result.steps[2].status, StepStatus::Skipped);
    }

    #[tokio::test]
    async fn timeout_and_cancellation() {
        let s = spec("Feature: f\n Scenario: s\n  Given I open \"/\"\n", 1_000);
        let out = runner(3_000).run(&s, CancellationToken::new()).await;
        assert_eq!(out.result.status, JobStatus::TimedOut);
        assert_eq!(out.result.steps[0].status, StepStatus::Skipped);

        let token = CancellationToken::new();
        let t2 = token.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(100)).await;
            t2.cancel();
        });
        let s = spec("Feature: f\n Scenario: s\n  Given I open \"/\"\n", 10_000);
        let out = runner(2_000).run(&s, token).await;
        assert_eq!(out.result.status, JobStatus::Cancelled);
    }

    #[tokio::test]
    async fn missing_adapter_is_error() {
        let s = spec(
            "Feature: f\n Scenario: s\n  Given I open \"/\"\n  Then I should see \"x\"\n",
            10_000,
        );
        let out = ScenarioRunner::new(None)
            .run(&s, CancellationToken::new())
            .await;
        assert_eq!(out.result.status, JobStatus::Error);
        assert!(out.result.error.is_some());
    }

    #[tokio::test]
    async fn secrets_are_redacted() {
        let mut s = spec("Feature: f\n Scenario: s\n  Given I send a GET request to \"http://127.0.0.1:9/{{token}}\"\n", 10_000);
        s.environment
            .variables
            .insert("token".into(), "supersecretvalue".into());
        s.environment.secret_names.push("token".into());
        let out = ScenarioRunner::new(None)
            .run(&s, CancellationToken::new())
            .await;
        let dump = serde_json::to_string(&out.result).unwrap();
        assert!(!dump.contains("supersecretvalue"), "{dump}");
    }
}

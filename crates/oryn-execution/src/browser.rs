//! The Playwright adapter protocol (`oryn.browser/v1`) and its client.
//!
//! The adapter executes *primitive* actions and reports *observations*. It never
//! decides whether a test passed: [`evaluate`] turns observations into verdicts in
//! Rust. See `docs/protocol/browser-v1.md`.

use async_trait::async_trait;
use oryn_domain::{ExecStep, StepStatus};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::time::Duration;

pub const BROWSER_PROTOCOL: &str = "oryn.browser/v1";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Viewport {
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunRequest {
    pub protocol: String,
    pub run_id: String,
    pub browser: String,
    pub base_url: String,
    pub viewport: Viewport,
    pub step_timeout_ms: u64,
    pub screenshots: String,
    pub record_video: bool,
    pub record_trace: bool,
    pub headers: BTreeMap<String, String>,
    pub storage_state: Option<Value>,
    pub allowed_hosts: Vec<String>,
    pub steps: Vec<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdapterError {
    pub kind: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdapterStep {
    pub id: String,
    #[serde(default)]
    pub action: String,
    pub status: String,
    pub started_at: Option<String>,
    #[serde(default)]
    pub duration_ms: u64,
    pub observation: Option<Value>,
    pub error: Option<AdapterError>,
    pub screenshot: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdapterArtifact {
    pub name: String,
    pub kind: String,
    pub content_type: String,
    pub data_base64: String,
    #[serde(default)]
    pub size_bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConsoleEntry {
    #[serde(rename = "type")]
    pub kind: String,
    pub text: String,
    pub timestamp: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkEntry {
    pub url: String,
    pub method: String,
    pub status: Option<u16>,
    pub failure: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunResponse {
    pub protocol: String,
    pub run_id: String,
    pub status: String,
    pub browser: Option<String>,
    pub browser_version: Option<String>,
    #[serde(default)]
    pub duration_ms: u64,
    pub steps: Vec<AdapterStep>,
    #[serde(default)]
    pub artifacts: Vec<AdapterArtifact>,
    #[serde(default)]
    pub console: Vec<ConsoleEntry>,
    #[serde(default)]
    pub network: Vec<NetworkEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BrowserInfo {
    pub name: String,
    pub version: Option<String>,
    pub available: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdapterHealth {
    pub status: String,
    pub protocol: String,
    #[serde(default)]
    pub browsers: Vec<BrowserInfo>,
}

#[derive(Debug, thiserror::Error)]
pub enum AdapterCallError {
    #[error("browser adapter unreachable: {0}")]
    Unreachable(String),
    #[error("browser adapter rejected the run ({status}): {body}")]
    Rejected { status: u16, body: String },
    #[error("browser adapter protocol mismatch: {0}")]
    Protocol(String),
}

#[async_trait]
pub trait BrowserAdapter: Send + Sync {
    async fn run(&self, req: RunRequest) -> Result<RunResponse, AdapterCallError>;
    async fn health(&self) -> Result<AdapterHealth, AdapterCallError>;
}

/// HTTP client for a Playwright runner.
pub struct HttpBrowserAdapter {
    base: String,
    token: Option<String>,
    client: reqwest::Client,
}

impl HttpBrowserAdapter {
    pub fn new(base: impl Into<String>, token: Option<String>, timeout: Duration) -> Self {
        let client = reqwest::Client::builder()
            .timeout(timeout)
            .build()
            .expect("client");
        Self {
            base: base.into().trim_end_matches('/').to_string(),
            token,
            client,
        }
    }
}

#[async_trait]
impl BrowserAdapter for HttpBrowserAdapter {
    async fn run(&self, req: RunRequest) -> Result<RunResponse, AdapterCallError> {
        let mut r = self
            .client
            .post(format!("{}/v1/runs", self.base))
            .json(&req);
        if let Some(t) = &self.token {
            r = r.bearer_auth(t);
        }
        let resp = r
            .send()
            .await
            .map_err(|e| AdapterCallError::Unreachable(e.to_string()))?;
        let status = resp.status().as_u16();
        if !resp.status().is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(AdapterCallError::Rejected {
                status,
                body: body.chars().take(500).collect(),
            });
        }
        let out: RunResponse = resp
            .json()
            .await
            .map_err(|e| AdapterCallError::Protocol(e.to_string()))?;
        if out.protocol != BROWSER_PROTOCOL {
            return Err(AdapterCallError::Protocol(format!(
                "expected {BROWSER_PROTOCOL}, got {}",
                out.protocol
            )));
        }
        Ok(out)
    }

    async fn health(&self) -> Result<AdapterHealth, AdapterCallError> {
        let resp = self
            .client
            .get(format!("{}/v1/health", self.base))
            .timeout(Duration::from_secs(5))
            .send()
            .await
            .map_err(|e| AdapterCallError::Unreachable(e.to_string()))?;
        resp.json()
            .await
            .map_err(|e| AdapterCallError::Protocol(e.to_string()))
    }
}

/// Translate one executable browser step into an adapter step.
pub fn to_adapter_step(id: &str, step: &ExecStep, wait_ms: u64) -> Option<Value> {
    use ExecStep::*;
    Some(match step {
        Goto { url } => json!({"id": id, "action": "goto", "url": url}),
        Click { target } => json!({"id": id, "action": "click", "target": target}),
        Fill { target, value } => {
            json!({"id": id, "action": "fill", "target": target, "value": value})
        }
        // `target` is optional in oryn.browser/v1: omit it rather than send null.
        Press {
            key,
            target: Some(t),
        } => json!({"id": id, "action": "press", "key": key, "target": t}),
        Press { key, target: None } => json!({"id": id, "action": "press", "key": key}),
        Select { target, value } => {
            json!({"id": id, "action": "select", "target": target, "value": value})
        }
        Check { target } => json!({"id": id, "action": "check", "target": target}),
        Uncheck { target } => json!({"id": id, "action": "uncheck", "target": target}),
        Hover { target } => json!({"id": id, "action": "hover", "target": target}),
        Wait { ms } => json!({"id": id, "action": "wait", "ms": (*ms).min(30_000)}),
        Screenshot { name } => json!({"id": id, "action": "screenshot", "name": name}),
        ExpectText { text, visible } => {
            json!({"id": id, "action": "observe_text", "text": text, "visible": visible, "wait_ms": if *visible { wait_ms } else { wait_ms.min(2_000) }})
        }
        ExpectUrlContains { text } => {
            json!({"id": id, "action": "observe_url", "contains": text, "wait_ms": wait_ms})
        }
        ExpectTitle { text } => {
            json!({"id": id, "action": "observe_title", "equals": text, "wait_ms": wait_ms})
        }
        ExpectElementVisible { target } => {
            json!({"id": id, "action": "observe_element", "target": target, "wait_ms": wait_ms})
        }
        ExpectElementContains { target, text } => {
            json!({"id": id, "action": "observe_element", "target": target, "contains": text, "wait_ms": wait_ms})
        }
        _ => return None,
    })
}

/// Deterministic verdict for a browser step from the adapter's report.
/// Returns (status, message, expected, actual).
pub fn evaluate(
    step: &ExecStep,
    reported: &AdapterStep,
) -> (StepStatus, Option<String>, Option<String>, Option<String>) {
    match reported.status.as_str() {
        "skipped" => {
            return (
                StepStatus::Skipped,
                Some("Skipped after an earlier step failed".into()),
                None,
                None,
            )
        }
        "error" => {
            let e = reported.error.clone().unwrap_or(AdapterError {
                kind: "unknown".into(),
                message: "unknown adapter error".into(),
            });
            // A target that cannot be found is a failed expectation of the page, reported as failure.
            let status = if e.kind == "not_found" || e.kind == "timeout" {
                StepStatus::Failed
            } else {
                StepStatus::Error
            };
            return (
                status,
                Some(format!("{}: {}", e.kind, e.message)),
                None,
                None,
            );
        }
        _ => {}
    }
    let obs = reported.observation.clone().unwrap_or(Value::Null);
    let b = |k: &str| obs.get(k).and_then(Value::as_bool);
    let st = |k: &str| obs.get(k).and_then(Value::as_str).map(str::to_string);
    use ExecStep::*;
    match step {
        ExpectText { text, visible } => match b("visible") {
            Some(v) if v == *visible => (StepStatus::Passed, None, None, None),
            Some(v) => (
                StepStatus::Failed,
                Some(if *visible {
                    format!("Text \"{text}\" was not visible")
                } else {
                    format!("Text \"{text}\" was visible")
                }),
                Some(format!("visible = {visible}")),
                Some(format!("visible = {v}")),
            ),
            None => missing(),
        },
        ExpectUrlContains { text } => match st("url") {
            Some(u) if u.contains(text.as_str()) => (StepStatus::Passed, None, None, None),
            Some(u) => (
                StepStatus::Failed,
                Some(format!("URL does not contain \"{text}\"")),
                Some(format!("contains {text}")),
                Some(u),
            ),
            None => missing(),
        },
        ExpectTitle { text } => match st("title") {
            Some(t) if &t == text => (StepStatus::Passed, None, None, None),
            Some(t) => (
                StepStatus::Failed,
                Some("Page title differs".into()),
                Some(text.clone()),
                Some(t),
            ),
            None => missing(),
        },
        ExpectElementVisible { target } => match b("visible") {
            Some(true) => (StepStatus::Passed, None, None, None),
            Some(false) => (
                StepStatus::Failed,
                Some(format!("\"{target}\" was not visible")),
                Some("visible".into()),
                Some("not visible".into()),
            ),
            None => missing(),
        },
        ExpectElementContains { target, text } => {
            let content = format!(
                "{} {}",
                st("text").unwrap_or_default(),
                st("value").unwrap_or_default()
            );
            if b("visible") == Some(false) && content.trim().is_empty() {
                return (
                    StepStatus::Failed,
                    Some(format!("\"{target}\" was not found")),
                    Some(text.clone()),
                    Some("element not visible".into()),
                );
            }
            if content.contains(text.as_str()) {
                (StepStatus::Passed, None, None, None)
            } else {
                (
                    StepStatus::Failed,
                    Some(format!("\"{target}\" does not contain \"{text}\"")),
                    Some(text.clone()),
                    Some(content.trim().to_string()),
                )
            }
        }
        _ => (StepStatus::Passed, None, None, None),
    }
}

fn missing() -> (StepStatus, Option<String>, Option<String>, Option<String>) {
    (
        StepStatus::Error,
        Some("Adapter returned no observation for an expectation".into()),
        None,
        None,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn press_without_target_omits_the_field() {
        let v = to_adapter_step(
            "s1",
            &ExecStep::Press {
                key: "Enter".into(),
                target: None,
            },
            1000,
        )
        .unwrap();
        assert_eq!(v, json!({"id": "s1", "action": "press", "key": "Enter"}));
        let v = to_adapter_step(
            "s2",
            &ExecStep::Press {
                key: "Tab".into(),
                target: Some("Email".into()),
            },
            1000,
        )
        .unwrap();
        assert_eq!(v["target"], "Email");
    }

    fn rep(status: &str, obs: Value) -> AdapterStep {
        AdapterStep {
            id: "s".into(),
            action: "x".into(),
            status: status.into(),
            started_at: None,
            duration_ms: 1,
            observation: Some(obs),
            error: None,
            screenshot: None,
        }
    }

    #[test]
    fn verdicts() {
        let s = ExecStep::ExpectText {
            text: "Hi".into(),
            visible: true,
        };
        assert_eq!(
            evaluate(&s, &rep("ok", json!({"visible": true, "count": 1}))).0,
            StepStatus::Passed
        );
        assert_eq!(
            evaluate(&s, &rep("ok", json!({"visible": false, "count": 0}))).0,
            StepStatus::Failed
        );
        let n = ExecStep::ExpectText {
            text: "Hi".into(),
            visible: false,
        };
        assert_eq!(
            evaluate(&n, &rep("ok", json!({"visible": false}))).0,
            StepStatus::Passed
        );
        let u = ExecStep::ExpectUrlContains {
            text: "/dash".into(),
        };
        let (st, _, _, actual) = evaluate(&u, &rep("ok", json!({"url": "http://x/login"})));
        assert_eq!(st, StepStatus::Failed);
        assert_eq!(actual.as_deref(), Some("http://x/login"));
        assert_eq!(evaluate(&u, &rep("ok", json!({}))).0, StepStatus::Error);
        let mut e = rep("error", Value::Null);
        e.error = Some(AdapterError {
            kind: "not_found".into(),
            message: "no Sign in".into(),
        });
        assert_eq!(
            evaluate(
                &ExecStep::Click {
                    target: "Sign in".into()
                },
                &e
            )
            .0,
            StepStatus::Failed
        );
        e.error = Some(AdapterError {
            kind: "navigation".into(),
            message: "ERR_CONNECTION_REFUSED".into(),
        });
        assert_eq!(
            evaluate(&ExecStep::Goto { url: "/".into() }, &e).0,
            StepStatus::Error
        );
        let c = ExecStep::ExpectElementContains {
            target: "#total".into(),
            text: "21.60".into(),
        };
        assert_eq!(
            evaluate(
                &c,
                &rep("ok", json!({"visible": true, "text": "Total $21.60"}))
            )
            .0,
            StepStatus::Passed
        );
    }

    #[test]
    fn translation() {
        let v = to_adapter_step(
            "s1",
            &ExecStep::ExpectText {
                text: "x".into(),
                visible: false,
            },
            9000,
        )
        .unwrap();
        assert_eq!(v["action"], "observe_text");
        assert_eq!(v["wait_ms"], 2000);
        assert!(to_adapter_step("s1", &ExecStep::ExpectStatus { code: 1 }, 1).is_none());
    }
}

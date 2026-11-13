//! REST API testing engine.
//!
//! An [`ApiSession`] executes API steps in order, keeping headers, variables and
//! the last response. Every expectation is evaluated deterministically here and
//! returns an [`Outcome`] with expected/actual values as evidence.

pub mod policy;

use oryn_domain::{interpolate, ExecStep, HttpMethod, StepStatus};
use serde::Serialize;
use serde_json::Value;
use serde_json_path::JsonPath;
use std::collections::BTreeMap;
use std::time::{Duration, Instant};

pub use policy::NetworkPolicy;

/// Captured response, kept for subsequent assertions and as evidence.
#[derive(Debug, Clone, Serialize)]
pub struct ResponseSummary {
    pub method: String,
    pub url: String,
    pub status: u16,
    pub elapsed_ms: u64,
    pub headers: BTreeMap<String, String>,
    pub body: String,
    #[serde(skip)]
    pub json: Option<Value>,
}

/// Outcome of a single step.
#[derive(Debug, Clone, Serialize)]
pub struct Outcome {
    pub status: StepStatus,
    pub message: Option<String>,
    pub expected: Option<String>,
    pub actual: Option<String>,
    pub observation: Option<Value>,
}

impl Outcome {
    fn pass(observation: Option<Value>) -> Self {
        Self {
            status: StepStatus::Passed,
            message: None,
            expected: None,
            actual: None,
            observation,
        }
    }
    fn fail(
        message: impl Into<String>,
        expected: impl Into<String>,
        actual: impl Into<String>,
    ) -> Self {
        Self {
            status: StepStatus::Failed,
            message: Some(message.into()),
            expected: Some(expected.into()),
            actual: Some(actual.into()),
            observation: None,
        }
    }
    fn error(message: impl Into<String>) -> Self {
        Self {
            status: StepStatus::Error,
            message: Some(message.into()),
            expected: None,
            actual: None,
            observation: None,
        }
    }
}

const MAX_BODY_BYTES: usize = 2 * 1024 * 1024;

pub struct ApiSession {
    client: reqwest::Client,
    base_url: Option<String>,
    headers: BTreeMap<String, String>,
    pub vars: BTreeMap<String, String>,
    pub last: Option<ResponseSummary>,
    policy: NetworkPolicy,
}

impl ApiSession {
    pub fn new(
        base_url: Option<String>,
        vars: BTreeMap<String, String>,
        policy: NetworkPolicy,
        timeout: Duration,
    ) -> Self {
        // Every redirect hop is held to the same network policy as the first request.
        let hop_policy = policy.clone();
        let redirect = reqwest::redirect::Policy::custom(move |attempt| {
            if attempt.previous().len() >= 5 {
                return attempt.error("too many redirects");
            }
            let url = attempt.url();
            let ok = matches!(url.scheme(), "http" | "https")
                && url.host_str().is_some_and(|h| hop_policy.host_allowed(h));
            if ok {
                attempt.follow()
            } else {
                let msg = format!("Blocked by network policy: redirect to {url}");
                attempt.error(msg)
            }
        });
        let client = reqwest::Client::builder()
            .timeout(timeout)
            .redirect(redirect)
            .user_agent("oryn-api-engine/0.1")
            .build()
            .expect("reqwest client builds");
        Self {
            client,
            base_url,
            headers: BTreeMap::new(),
            vars,
            last: None,
            policy,
        }
    }

    fn resolve_url(&self, path: &str) -> Result<url::Url, String> {
        if path.starts_with("http://") || path.starts_with("https://") {
            return url::Url::parse(path).map_err(|e| format!("invalid URL {path}: {e}"));
        }
        let base = self
            .base_url
            .as_deref()
            .ok_or("no API base URL configured for this environment")?;
        let base = url::Url::parse(base).map_err(|e| format!("invalid base URL {base}: {e}"))?;
        base.join(path)
            .map_err(|e| format!("cannot join {path}: {e}"))
    }

    /// Execute one step. Variables are interpolated before execution.
    pub async fn execute(&mut self, step: &ExecStep) -> Outcome {
        let step = step.interpolate(&self.vars);
        match &step {
            ExecStep::SetHeader { name, value } => {
                self.headers.insert(name.clone(), value.clone());
                Outcome::pass(None)
            }
            ExecStep::HttpRequest { method, path, body } => {
                self.request(*method, path, body.as_deref()).await
            }
            ExecStep::ExpectStatus { code } => self.with_last(|r| {
                if r.status == *code {
                    Outcome::pass(None)
                } else {
                    Outcome::fail(
                        format!("Expected status {code} but received {}", r.status),
                        code.to_string(),
                        r.status.to_string(),
                    )
                }
            }),
            ExecStep::ExpectResponseTimeBelow { ms } => self.with_last(|r| {
                if r.elapsed_ms < *ms {
                    Outcome::pass(None)
                } else {
                    Outcome::fail(
                        format!("Response took {} ms (limit {ms} ms)", r.elapsed_ms),
                        format!("< {ms} ms"),
                        format!("{} ms", r.elapsed_ms),
                    )
                }
            }),
            ExecStep::ExpectJsonExists { path } => self.with_json(path, |found| {
                if found.is_empty() {
                    Outcome::fail(
                        format!("JSONPath {path} matched nothing"),
                        "at least one match",
                        "no match",
                    )
                } else {
                    Outcome::pass(Some(found[0].clone()))
                }
            }),
            ExecStep::ExpectJsonLength { path, length } => self.with_json(path, |found| {
                let actual = match found.first() {
                    Some(Value::Array(a)) => Some(a.len()),
                    Some(Value::String(s)) => Some(s.chars().count()),
                    Some(Value::Object(o)) => Some(o.len()),
                    _ => None,
                };
                match actual {
                    Some(n) if n == *length => Outcome::pass(None),
                    Some(n) => Outcome::fail(
                        format!("Expected {path} to have length {length}, got {n}"),
                        length.to_string(),
                        n.to_string(),
                    ),
                    None => Outcome::fail(
                        format!("{path} is not an array, string or object"),
                        length.to_string(),
                        found
                            .first()
                            .map(|v| v.to_string())
                            .unwrap_or("no match".into()),
                    ),
                }
            }),
            ExecStep::ExpectJsonEquals { path, value } => self.with_json(path, |found| {
                let Some(actual) = found.first() else {
                    return Outcome::fail(
                        format!("JSONPath {path} matched nothing"),
                        value.clone(),
                        "no match",
                    );
                };
                if json_equals(actual, value) {
                    Outcome::pass(Some(actual.clone()))
                } else {
                    Outcome::fail(
                        format!("Expected {path} to equal {value}"),
                        value.clone(),
                        display(actual),
                    )
                }
            }),
            ExecStep::ExpectJsonSchema { schema } => {
                let Some(r) = &self.last else {
                    return Outcome::error("No response to validate; send a request first");
                };
                let Some(json) = &r.json else {
                    return Outcome::fail(
                        "Response body is not JSON",
                        "JSON body",
                        truncate(&r.body, 200),
                    );
                };
                match jsonschema::validator_for(schema) {
                    Err(e) => Outcome::error(format!("Invalid JSON schema: {e}")),
                    Ok(v) => {
                        let errors: Vec<String> = v
                            .iter_errors(json)
                            .take(5)
                            .map(|e| format!("{} at {}", e, e.instance_path))
                            .collect();
                        if errors.is_empty() {
                            Outcome::pass(None)
                        } else {
                            Outcome::fail(
                                format!("Schema validation failed: {}", errors.join("; ")),
                                "valid against schema",
                                errors.join("; "),
                            )
                        }
                    }
                }
            }
            ExecStep::StoreJson { path, var } => {
                let res = self.with_json(path, |found| match found.first() {
                    Some(v) => Outcome::pass(Some(v.clone())),
                    None => Outcome::fail(
                        format!("JSONPath {path} matched nothing"),
                        "a value",
                        "no match",
                    ),
                });
                if let (StepStatus::Passed, Some(v)) = (res.status, &res.observation) {
                    let s = match v {
                        Value::String(s) => s.clone(),
                        other => other.to_string(),
                    };
                    self.vars.insert(var.clone(), s);
                }
                res
            }
            other => Outcome::error(format!("Not an API step: {other:?}")),
        }
    }

    fn with_last(&self, f: impl FnOnce(&ResponseSummary) -> Outcome) -> Outcome {
        match &self.last {
            Some(r) => f(r),
            None => Outcome::error("No response to assert on; send a request first"),
        }
    }

    fn with_json(&self, path: &str, f: impl FnOnce(Vec<Value>) -> Outcome) -> Outcome {
        let Some(r) = &self.last else {
            return Outcome::error("No response to assert on; send a request first");
        };
        let Some(json) = &r.json else {
            return Outcome::fail(
                "Response body is not JSON",
                "JSON body",
                truncate(&r.body, 200),
            );
        };
        let jp = match JsonPath::parse(path) {
            Ok(p) => p,
            Err(e) => return Outcome::error(format!("Invalid JSONPath {path}: {e}")),
        };
        let found: Vec<Value> = jp.query(json).all().into_iter().cloned().collect();
        f(found)
    }

    async fn request(&mut self, method: HttpMethod, path: &str, body: Option<&str>) -> Outcome {
        let url = match self.resolve_url(path) {
            Ok(u) => u,
            Err(e) => return Outcome::error(e),
        };
        if let Err(e) = self.policy.check(&url).await {
            return Outcome::error(e);
        }
        let m = reqwest::Method::from_bytes(method.as_str().as_bytes()).expect("valid method");
        let mut req = self.client.request(m, url.clone());
        let mut has_ct = false;
        for (k, v) in &self.headers {
            has_ct |= k.eq_ignore_ascii_case("content-type");
            req = req.header(k, v);
        }
        if let Some(b) = body {
            if !has_ct && serde_json::from_str::<Value>(b).is_ok() {
                req = req.header("content-type", "application/json");
            }
            req = req.body(b.to_string());
        }
        let start = Instant::now();
        let resp = match req.send().await {
            Ok(r) => r,
            Err(e) => {
                self.last = None;
                let kind = if e.is_timeout() {
                    "timeout"
                } else if e.is_connect() {
                    "connection refused or unreachable"
                } else {
                    "request error"
                };
                // reqwest hides the cause (e.g. a blocked redirect) in the source chain.
                let mut detail = e.to_string();
                let mut src = std::error::Error::source(&e);
                while let Some(cause) = src {
                    detail.push_str(": ");
                    detail.push_str(&cause.to_string());
                    src = cause.source();
                }
                return Outcome::error(format!("{} {}: {kind}: {detail}", method.as_str(), url));
            }
        };
        let status = resp.status().as_u16();
        let headers: BTreeMap<String, String> = resp
            .headers()
            .iter()
            .filter(|(k, _)| !matches!(k.as_str(), "set-cookie" | "authorization"))
            .map(|(k, v)| (k.to_string(), v.to_str().unwrap_or("").to_string()))
            .collect();
        let bytes = match resp.bytes().await {
            Ok(b) => b,
            Err(e) => return Outcome::error(format!("Failed reading response body: {e}")),
        };
        let elapsed_ms = start.elapsed().as_millis() as u64;
        let body = String::from_utf8_lossy(&bytes[..bytes.len().min(MAX_BODY_BYTES)]).to_string();
        let json = serde_json::from_str::<Value>(&body).ok();
        let summary = ResponseSummary {
            method: method.as_str().into(),
            url: url.to_string(),
            status,
            elapsed_ms,
            headers,
            body,
            json,
        };
        let obs = serde_json::json!({
            "method": summary.method, "url": summary.url, "status": status, "elapsed_ms": elapsed_ms,
            "headers": summary.headers, "body": truncate(&summary.body, 4000),
        });
        self.last = Some(summary);
        Outcome::pass(Some(obs))
    }
}

/// Compare a JSON value with an expected literal: JSON-parse the expectation when
/// possible (numbers compare numerically), otherwise compare as a string.
pub fn json_equals(actual: &Value, expected: &str) -> bool {
    if let Ok(exp) = serde_json::from_str::<Value>(expected) {
        if let (Some(a), Some(b)) = (actual.as_f64(), exp.as_f64()) {
            return (a - b).abs() < 1e-9;
        }
        if &exp == actual {
            return true;
        }
    }
    match actual {
        Value::String(s) => s == expected,
        other => other == expected,
    }
}

fn display(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

fn truncate(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.to_string()
    } else {
        format!("{}…", s.chars().take(n).collect::<String>())
    }
}

/// Interpolate convenience re-export.
pub fn interpolate_str(s: &str, vars: &BTreeMap<String, String>) -> String {
    interpolate(s, vars)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        routing::{get, post},
        Json, Router,
    };

    async fn serve() -> String {
        let app = Router::new()
            .route("/items", get(|| async { Json(serde_json::json!({"items":[{"id":1,"price":9.5},{"id":2,"price":3}],"total":12.5})) }))
            .route("/echo", post(|body: String| async move { (axum::http::StatusCode::CREATED, body) }))
            .route("/text", get(|| async { "plain" }));
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = l.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(l, app).await.unwrap() });
        format!("http://{addr}")
    }

    fn session(base: String) -> ApiSession {
        ApiSession::new(
            Some(base),
            BTreeMap::new(),
            NetworkPolicy::permissive_local(),
            Duration::from_secs(5),
        )
    }

    #[tokio::test]
    async fn full_flow() {
        let mut s = session(serve().await);
        let r = s
            .execute(&ExecStep::HttpRequest {
                method: HttpMethod::Get,
                path: "/items".into(),
                body: None,
            })
            .await;
        assert_eq!(r.status, StepStatus::Passed);
        assert_eq!(
            s.execute(&ExecStep::ExpectStatus { code: 200 })
                .await
                .status,
            StepStatus::Passed
        );
        let f = s.execute(&ExecStep::ExpectStatus { code: 201 }).await;
        assert_eq!(f.status, StepStatus::Failed);
        assert_eq!(f.actual.as_deref(), Some("200"));
        assert_eq!(
            s.execute(&ExecStep::ExpectJsonLength {
                path: "$.items".into(),
                length: 2
            })
            .await
            .status,
            StepStatus::Passed
        );
        assert_eq!(
            s.execute(&ExecStep::ExpectJsonEquals {
                path: "$.total".into(),
                value: "12.50".into()
            })
            .await
            .status,
            StepStatus::Passed
        );
        assert_eq!(
            s.execute(&ExecStep::ExpectJsonEquals {
                path: "$.items[0].id".into(),
                value: "2".into()
            })
            .await
            .status,
            StepStatus::Failed
        );
        assert_eq!(
            s.execute(&ExecStep::ExpectJsonExists {
                path: "$.nope".into()
            })
            .await
            .status,
            StepStatus::Failed
        );
        assert_eq!(
            s.execute(&ExecStep::StoreJson {
                path: "$.items[1].id".into(),
                var: "id".into()
            })
            .await
            .status,
            StepStatus::Passed
        );
        assert_eq!(s.vars.get("id").map(String::as_str), Some("2"));
        let schema = serde_json::json!({"type":"object","required":["items","total"],"properties":{"total":{"type":"number"}}});
        assert_eq!(
            s.execute(&ExecStep::ExpectJsonSchema { schema })
                .await
                .status,
            StepStatus::Passed
        );
        let bad = serde_json::json!({"type":"object","required":["missing"]});
        assert_eq!(
            s.execute(&ExecStep::ExpectJsonSchema { schema: bad })
                .await
                .status,
            StepStatus::Failed
        );
        assert_eq!(
            s.execute(&ExecStep::ExpectResponseTimeBelow { ms: 5000 })
                .await
                .status,
            StepStatus::Passed
        );

        let r = s
            .execute(&ExecStep::HttpRequest {
                method: HttpMethod::Post,
                path: "/echo".into(),
                body: Some(r#"{"id":"{{id}}"}"#.into()),
            })
            .await;
        assert_eq!(r.status, StepStatus::Passed);
        assert_eq!(
            s.execute(&ExecStep::ExpectStatus { code: 201 })
                .await
                .status,
            StepStatus::Passed
        );
        assert_eq!(
            s.execute(&ExecStep::ExpectJsonEquals {
                path: "$.id".into(),
                value: "2".into()
            })
            .await
            .status,
            StepStatus::Passed
        );

        s.execute(&ExecStep::HttpRequest {
            method: HttpMethod::Get,
            path: "/text".into(),
            body: None,
        })
        .await;
        assert_eq!(
            s.execute(&ExecStep::ExpectJsonExists { path: "$.x".into() })
                .await
                .status,
            StepStatus::Failed
        );
    }

    #[tokio::test]
    async fn connection_errors_are_errors() {
        let mut s = session("http://127.0.0.1:9".into());
        let r = s
            .execute(&ExecStep::HttpRequest {
                method: HttpMethod::Get,
                path: "/".into(),
                body: None,
            })
            .await;
        assert_eq!(r.status, StepStatus::Error);
        assert_eq!(
            s.execute(&ExecStep::ExpectStatus { code: 200 })
                .await
                .status,
            StepStatus::Error
        );
    }

    #[tokio::test]
    async fn redirects_to_disallowed_hosts_are_blocked() {
        let app = Router::new().route(
            "/go",
            get(|| async {
                axum::response::Redirect::temporary("http://169.254.169.254/latest/meta-data")
            }),
        );
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = l.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(l, app).await.unwrap() });
        let mut s = ApiSession::new(
            Some(format!("http://{addr}")),
            BTreeMap::new(),
            NetworkPolicy {
                allowed_hosts: vec!["127.0.0.1".into()],
                block_private: false,
            },
            Duration::from_secs(5),
        );
        let r = s
            .execute(&ExecStep::HttpRequest {
                method: HttpMethod::Get,
                path: "/go".into(),
                body: None,
            })
            .await;
        assert_eq!(r.status, StepStatus::Error, "{r:?}");
        assert!(format!("{r:?}").contains("network policy"), "{r:?}");
    }

    #[test]
    fn equality_rules() {
        assert!(json_equals(&serde_json::json!(21.6), "21.60"));
        assert!(json_equals(&serde_json::json!("abc"), "abc"));
        assert!(json_equals(&serde_json::json!(true), "true"));
        assert!(!json_equals(&serde_json::json!("1"), "2"));
    }
}

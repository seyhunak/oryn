//! Failure investigation.
//!
//! Deterministic code extracts **observations** (facts from the execution evidence)
//! and assigns a category by documented rules. An optional model call adds an
//! **interpretation** that is stored separately and labelled. The model never
//! changes a verdict. It sees only the redacted observations and the scenario's
//! Gherkin source, both fenced as untrusted data, never secrets or raw artifacts.

use oryn_model_gateway::{untrusted, CompletionRequest, Message, Role, SAFETY_PREAMBLE};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use utoipa::ToSchema;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct StepFact {
    pub position: i32,
    pub keyword: String,
    pub text: String,
    pub status: String,
    pub message: Option<String>,
    pub expected: Option<String>,
    pub actual: Option<String>,
    pub observation: Option<Value>,
    pub artifact: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct InvestigationInput {
    pub scenario_name: String,
    pub job_status: String,
    pub error: Option<String>,
    pub attempt: i32,
    pub steps: Vec<StepFact>,
    /// Array of `{level, source, message}`.
    pub logs: Value,
    /// Array of `{url, method, status, failure}`.
    pub network: Value,
    /// Recent final statuses of this scenario, newest first (excluding this job).
    pub history: Vec<String>,
    pub passed_on_retry: bool,
    pub base_hosts: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct Observation {
    /// e.g. `failing_step`, `network`, `console`, `history`, `job_error`.
    pub kind: String,
    pub source: String,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct Diagnosis {
    pub category: String,
    pub confidence: f32,
    pub summary: String,
    pub root_cause: String,
    /// Name of the rule that produced the category (see docs/METRICS.md § Failure classification).
    pub rule: String,
    pub observations: Vec<Observation>,
    pub investigate: Vec<String>,
    pub remediate: Vec<String>,
}

fn host_of(url: &str) -> Option<String> {
    url.split("://")
        .nth(1)?
        .split(['/', '?', '#'])
        .next()
        .map(|h| h.to_ascii_lowercase())
}

fn arr(v: &Value) -> &[Value] {
    v.as_array().map(Vec::as_slice).unwrap_or(&[])
}

pub fn observations(i: &InvestigationInput) -> Vec<Observation> {
    let mut out = vec![];
    if let Some(f) = i
        .steps
        .iter()
        .find(|s| s.status == "failed" || s.status == "error")
    {
        let mut d = format!(
            "Step {} \"{} {}\" {}",
            f.position + 1,
            f.keyword.trim(),
            f.text,
            f.status
        );
        if let Some(m) = &f.message {
            d.push_str(&format!(": {m}"));
        }
        if let Some(e) = &f.expected {
            d.push_str(&format!(" | expected: {e}"));
        }
        if let Some(a) = &f.actual {
            d.push_str(&format!(" | actual: {a}"));
        }
        out.push(Observation {
            kind: "failing_step".into(),
            source: format!("step {}", f.position + 1),
            detail: d,
        });
        if let Some(a) = &f.artifact {
            out.push(Observation {
                kind: "artifact".into(),
                source: format!("step {}", f.position + 1),
                detail: format!("Evidence captured: {a}"),
            });
        }
    }
    let skipped = i.steps.iter().filter(|s| s.status == "skipped").count();
    if skipped > 0 {
        out.push(Observation {
            kind: "skipped".into(),
            source: "job".into(),
            detail: format!("{skipped} later step(s) were skipped after the failure"),
        });
    }
    if let Some(e) = &i.error {
        out.push(Observation {
            kind: "job_error".into(),
            source: "job".into(),
            detail: e.chars().take(500).collect(),
        });
    }
    for n in arr(&i.network)
        .iter()
        .filter(|n| {
            n["status"].as_u64().map(|s| s >= 400).unwrap_or(false) || n["failure"].is_string()
        })
        .take(8)
    {
        let st = n["status"]
            .as_u64()
            .map(|s| s.to_string())
            .unwrap_or_else(|| n["failure"].as_str().unwrap_or("failed").to_string());
        out.push(Observation {
            kind: "network".into(),
            source: "network".into(),
            detail: format!(
                "{} {} → {st}",
                n["method"].as_str().unwrap_or("GET"),
                n["url"].as_str().unwrap_or("")
            ),
        });
    }
    for l in arr(&i.logs)
        .iter()
        .filter(|l| matches!(l["level"].as_str(), Some("error") | Some("pageerror")))
        .take(5)
    {
        out.push(Observation {
            kind: "console".into(),
            source: l["source"].as_str().unwrap_or("browser").into(),
            detail: l["message"]
                .as_str()
                .unwrap_or("")
                .chars()
                .take(300)
                .collect(),
        });
    }
    if !i.history.is_empty() {
        let recent: Vec<&str> = i.history.iter().take(10).map(String::as_str).collect();
        out.push(Observation {
            kind: "history".into(),
            source: "previous runs".into(),
            detail: format!(
                "Last {} final results, newest first: {}",
                recent.len(),
                recent.join(", ")
            ),
        });
    }
    if i.passed_on_retry {
        out.push(Observation {
            kind: "retry".into(),
            source: "job".into(),
            detail: "The same scenario passed on a retry with no code change".into(),
        });
    }
    out
}

fn contains_any(h: &str, needles: &[&str]) -> bool {
    let h = h.to_ascii_lowercase();
    needles.iter().any(|n| h.contains(n))
}

/// Deterministic classification. Rules are evaluated in order; the first match wins.
pub fn classify(i: &InvestigationInput) -> Diagnosis {
    let obs = observations(i);
    let failing = i
        .steps
        .iter()
        .find(|s| s.status == "failed" || s.status == "error");
    let fail_text = failing
        .map(|f| {
            format!(
                "{} {} {}",
                f.message.clone().unwrap_or_default(),
                f.actual.clone().unwrap_or_default(),
                f.text
            )
        })
        .unwrap_or_default();
    let all_text = format!("{fail_text} {}", i.error.clone().unwrap_or_default());
    let net = arr(&i.network);
    let flips = i.history.windows(2).filter(|w| w[0] != w[1]).count();
    let mixed_history = i.history.iter().any(|s| s == "passed")
        && i.history.iter().any(|s| s == "failed" || s == "error");
    let step_desc = failing
        .map(|f| format!("step {} (\"{}\")", f.position + 1, f.text))
        .unwrap_or_else(|| "the job".into());

    let (category, confidence, rule, summary, root, investigate, remediate): (
        &str,
        f32,
        &str,
        String,
        String,
        Vec<&str>,
        Vec<&str>,
    ) = if i.passed_on_retry || (mixed_history && flips >= 2) {
        (
            "flaky_test",
            0.7,
            "flaky.retry_or_flips",
            format!("{} fails intermittently", i.scenario_name),
            format!(
                "The scenario {} without a code change; {step_desc} is where it failed this time.",
                if i.passed_on_retry {
                    "passed on retry"
                } else {
                    "alternates between passing and failing"
                }
            ),
            vec![
                "Compare the failing and passing runs' network logs for the failing step",
                "Look for timing-dependent assertions or shared test data",
            ],
            vec![
                "Stabilise the dependency or wait on an explicit condition",
                "Quarantine the scenario until fixed if it blocks releases",
            ],
        )
    } else if contains_any(
        &all_text,
        &[
            "browser_unavailable",
            "econnrefused",
            "connection refused",
            "net::err_name_not_resolved",
            "net::err_connection",
            "dns error",
            "could not connect",
            "no browser adapter",
            "runner unavailable",
        ],
    ) || (i.job_status == "error"
        && i.steps
            .iter()
            .all(|s| s.status == "skipped" || s.status == "pending"))
    {
        ("environment_failure", 0.75, "environment.unreachable",
             format!("{} could not run: the environment was unreachable", i.scenario_name),
             format!("The target application or the browser runner could not be reached at {step_desc}; no product behaviour was evaluated."),
             vec!["Check the environment's base URL and that the application is running", "Check the worker's runner health on the Workers page"],
             vec!["Fix the environment configuration and re-run"])
    } else if failing
        .and_then(|f| f.actual.as_deref())
        .map(|a| a.trim() == "401" || a.trim() == "403")
        .unwrap_or(false)
        || contains_any(
            &fail_text,
            &[
                "unauthorized",
                "forbidden",
                "invalid email or password",
                "session expired",
            ],
        )
        || net.iter().any(|n| {
            matches!(n["status"].as_u64(), Some(401) | Some(403))
                && n["url"]
                    .as_str()
                    .map(|u| u.contains("auth") || u.contains("login"))
                    .unwrap_or(false)
        })
    {
        (
            "authentication_failure",
            0.65,
            "auth.rejected",
            format!("{} failed at authentication", i.scenario_name),
            format!("The application rejected the credentials or session at {step_desc}."),
            vec![
                "Confirm the environment's test credentials are still valid",
                "Check whether the session cookie or token was set before the failing step",
            ],
            vec![
                "Rotate or reseed the test account",
                "Fix the login flow if valid credentials are rejected",
            ],
        )
    } else if net.iter().any(|n| {
        let foreign = n["url"]
            .as_str()
            .and_then(host_of)
            .map(|h| !i.base_hosts.is_empty() && !i.base_hosts.contains(&h))
            .unwrap_or(false);
        (foreign
            && (n["status"].as_u64().map(|s| s >= 500).unwrap_or(false)
                || n["failure"].is_string()))
            || matches!(n["status"].as_u64(), Some(502) | Some(503) | Some(504))
    }) {
        ("dependency_failure", 0.6, "dependency.upstream_error",
             format!("{} failed because a dependency returned an error", i.scenario_name),
             "A request during the run returned 502/503/504 or failed against a third-party host; the application could not complete the flow.".into(),
             vec!["Open the network log and find the failing upstream request", "Check the dependency's status at the time of the run"],
             vec!["Add resilience (retry, fallback) for the dependency, or stub it in this environment"])
    } else if let Some(f) = failing.filter(|f| {
        f.message
            .as_deref()
            .map(|m| {
                contains_any(
                    m,
                    &[
                        "not_found",
                        "no element matched",
                        "strict mode violation",
                        "unsupported step",
                    ],
                )
            })
            .unwrap_or(false)
    }) {
        ("test_defect", 0.5, "test.target_missing",
             format!("{} could not find the element it targets", i.scenario_name),
             format!("Step {} targets an element that was not on the page. Either the UI changed and the step is stale, or the page did not reach the expected state.", f.position + 1),
             vec!["Open the failure screenshot and check what the page shows", "Check recent UI changes to the label, placeholder or test id"],
             vec!["Update the step to the current label or a stable data-testid", "If the element should exist, treat it as an application defect"])
    } else if net
        .iter()
        .any(|n| n["status"].as_u64().map(|s| s >= 500).unwrap_or(false))
    {
        ("application_defect", 0.7, "app.server_error",
             format!("{} hit a server error", i.scenario_name),
             format!("The application returned a 5xx response during the run; {step_desc} failed after it."),
             vec!["Find the 5xx request in the network log and its server logs at that time"],
             vec!["Fix the server error, then re-run the scenario"])
    } else if let Some(f) =
        failing.filter(|f| f.status == "failed" && (f.expected.is_some() || f.actual.is_some()))
    {
        (
            "application_defect",
            0.6,
            "app.assertion_mismatch",
            format!(
                "{}: {} did not match the expected result",
                i.scenario_name, f.text
            ),
            format!(
                "The application produced {} where the requirement expects {}.",
                f.actual.as_deref().unwrap_or("a different result"),
                f.expected.as_deref().unwrap_or("another result")
            ),
            vec![
                "Check the requirement linked to this scenario to confirm the expected value",
                "Reproduce the steps by hand and compare",
            ],
            vec![
                "Fix the application behaviour, or update the scenario if the requirement changed",
            ],
        )
    } else if failing.map(|f| f.status == "error").unwrap_or(false) || i.job_status == "timed_out" {
        (
            "environment_failure",
            0.4,
            "environment.step_error",
            format!("{} errored before reaching a verdict", i.scenario_name),
            format!("{step_desc} raised an error or timed out rather than failing an assertion."),
            vec![
                "Read the job log around the failing step",
                "Check whether the run exceeded its timeout",
            ],
            vec!["Increase the step timeout only if the page is legitimately slow"],
        )
    } else {
        (
            "unknown",
            0.2,
            "fallback",
            format!("{} did not pass", i.scenario_name),
            "The evidence does not match any classification rule.".into(),
            vec!["Read the observations and the failing step's evidence"],
            vec![],
        )
    };
    Diagnosis {
        category: category.into(),
        confidence,
        rule: rule.into(),
        summary,
        root_cause: root,
        observations: obs,
        investigate: investigate.into_iter().map(String::from).collect(),
        remediate: remediate.into_iter().map(String::from).collect(),
    }
}

pub fn interpretation_schema() -> Value {
    json!({
        "type": "object",
        "required": ["summary", "probable_root_cause", "category", "confidence", "suggested_investigation", "suggested_remediation"],
        "properties": {
            "summary": {"type": "string"},
            "probable_root_cause": {"type": "string"},
            "category": {"type": "string", "enum": ["application_defect","test_defect","environment_failure","dependency_failure","authentication_failure","flaky_test","unknown"]},
            "confidence": {"type": "number", "minimum": 0, "maximum": 1},
            "suggested_investigation": {"type": "array", "items": {"type": "string"}},
            "suggested_remediation": {"type": "array", "items": {"type": "string"}}
        }
    })
}

/// Build the interpretation request from observations only (never raw artifacts).
pub fn ai_request(
    i: &InvestigationInput,
    d: &Diagnosis,
    gherkin: Option<&str>,
    model: &str,
) -> CompletionRequest {
    let system = format!(
        "{SAFETY_PREAMBLE}\n\nYou interpret test failures for engineers. You receive observations extracted by deterministic code and \
a rule-based classification. Explain the most probable root cause and what to check next. Do not restate the observations; \
say what they mean. If the evidence is insufficient, say so and keep confidence low. You cannot change the test verdict."
    );
    let obs = serde_json::to_string_pretty(&d.observations).unwrap_or_default();
    let mut user = untrusted("scenario name", &i.scenario_name);
    user.push_str(&format!(
        "\nAttempt: {}\nRule-based category: {} (rule {}, confidence {:.2})\n",
        i.attempt, d.category, d.rule, d.confidence
    ));
    user.push_str(&untrusted("execution observations", &obs));
    if let Some(g) = gherkin {
        user.push('\n');
        user.push_str(&untrusted(
            "scenario source",
            &g.chars().take(4000).collect::<String>(),
        ));
    }
    CompletionRequest {
        model: model.into(),
        system,
        messages: vec![Message {
            role: Role::User,
            content: user,
        }],
        max_tokens: 4_000,
        json_schema: Some(interpretation_schema()),
        purpose: "investigation".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn step(
        pos: i32,
        status: &str,
        msg: Option<&str>,
        exp: Option<&str>,
        act: Option<&str>,
    ) -> StepFact {
        StepFact {
            position: pos,
            keyword: "Then".into(),
            text: "x".into(),
            status: status.into(),
            message: msg.map(String::from),
            expected: exp.map(String::from),
            actual: act.map(String::from),
            ..Default::default()
        }
    }

    #[test]
    fn rules() {
        let base = InvestigationInput {
            scenario_name: "Checkout".into(),
            job_status: "failed".into(),
            network: json!([]),
            logs: json!([]),
            base_hosts: vec!["localhost:4000".into()],
            ..Default::default()
        };

        let mut i = base.clone();
        i.steps = vec![
            step(0, "passed", None, None, None),
            step(
                1,
                "failed",
                Some("text mismatch"),
                Some("Total: $34.50"),
                Some("Total: $33.50"),
            ),
            step(2, "skipped", None, None, None),
        ];
        let d = classify(&i);
        assert_eq!(
            (d.category.as_str(), d.rule.as_str()),
            ("application_defect", "app.assertion_mismatch")
        );
        assert!(d.observations.iter().any(|o| o.detail.contains("$33.50")));

        let mut f = i.clone();
        f.history = vec!["passed".into(), "failed".into(), "passed".into()];
        assert_eq!(classify(&f).category, "flaky_test");

        let mut e = base.clone();
        e.job_status = "error".into();
        e.error = Some("runner unavailable: connection refused".into());
        e.steps = vec![step(0, "skipped", None, None, None)];
        assert_eq!(classify(&e).category, "environment_failure");

        let mut t = base.clone();
        t.steps = vec![step(
            3,
            "failed",
            Some("not_found: no element matched target \"Sign in\""),
            None,
            None,
        )];
        assert_eq!(classify(&t).category, "test_defect");

        let mut dep = base.clone();
        dep.steps = vec![step(
            1,
            "failed",
            Some("text mismatch"),
            Some("Drill"),
            Some(""),
        )];
        dep.network = json!([{"url": "http://localhost:4000/api/search?q=drill", "method": "GET", "status": 503}]);
        assert_eq!(classify(&dep).category, "dependency_failure");

        let mut a = base.clone();
        a.steps = vec![step(
            1,
            "failed",
            Some("status mismatch"),
            Some("200"),
            Some("401"),
        )];
        assert_eq!(classify(&a).category, "authentication_failure");

        let r = ai_request(&i, &classify(&i), Some("Feature: x"), "m");
        assert!(r.messages[0]
            .content
            .contains("<untrusted source=\"execution observations\">"));
    }
}

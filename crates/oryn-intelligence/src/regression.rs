//! Regression intelligence: impact-based scenario selection and deterministic recommendations.

use crate::analysis::Route;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeSet;
use utoipa::ToSchema;

#[derive(Debug, Clone, Default)]
pub struct ScenarioRef {
    pub id: String,
    pub name: String,
    pub suite: String,
    pub tags: Vec<String>,
    /// The scenario's step text, one step per line.
    pub text: String,
    pub last_status: Option<String>,
    pub flaky: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct Selection {
    pub scenario_id: String,
    pub name: String,
    pub score: f32,
    pub reasons: Vec<String>,
}

const STOP: &[&str] = &[
    "index", "main", "lib", "mod", "src", "app", "utils", "util", "types", "test", "tests", "spec",
    "page", "layout", "server", "client", "the", "and", "views", "store", "styles", "public",
    "config",
];

/// Keywords a changed file contributes: words of its stem and parent directory.
fn keywords(path: &str) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    let parts: Vec<&str> = path.split('/').collect();
    let file = parts.last().copied().unwrap_or("");
    let stem = file.split('.').next().unwrap_or(file);
    let parent = if parts.len() >= 2 {
        parts[parts.len() - 2]
    } else {
        ""
    };
    for w in stem
        .split(|c: char| !c.is_ascii_alphanumeric())
        .chain(parent.split(|c: char| !c.is_ascii_alphanumeric()))
    {
        // split camelCase
        let mut cur = String::new();
        for ch in w.chars() {
            if ch.is_ascii_uppercase() && !cur.is_empty() {
                out.insert(cur.to_ascii_lowercase());
                cur.clear();
            }
            cur.push(ch);
        }
        if !cur.is_empty() {
            out.insert(cur.to_ascii_lowercase());
        }
    }
    out.retain(|w| w.len() >= 4 && !STOP.contains(&w.as_str()));
    out
}

/// Score each scenario by its relationship to the changed files.
///
/// * +3 the scenario exercises a route defined in a changed file
/// * +2 a keyword from a changed path appears in the scenario name, suite, tags or steps
/// * +1 the scenario failed last time; +1 it is flaky
/// * smoke-tagged scenarios are always included as a baseline (score 0.5)
pub fn select(changed: &[String], routes: &[Route], scenarios: &[ScenarioRef]) -> Vec<Selection> {
    let changed_set: BTreeSet<&str> = changed.iter().map(String::as_str).collect();
    let touched_routes: Vec<&Route> = routes
        .iter()
        .filter(|r| changed_set.contains(r.file.as_str()))
        .collect();
    let words: BTreeSet<String> = changed.iter().flat_map(|p| keywords(p)).collect();

    let mut out = vec![];
    for s in scenarios {
        let hay =
            format!("{} {} {} {}", s.name, s.suite, s.tags.join(" "), s.text).to_ascii_lowercase();
        let mut score = 0.0;
        let mut reasons = vec![];
        let mut hit_routes = BTreeSet::new();
        for r in &touched_routes {
            let p = r.path.to_ascii_lowercase();
            if p.len() > 1
                && (hay.contains(&format!("\"{p}\""))
                    || hay.contains(&format!("{p}/"))
                    || hay.contains(&format!("{p}?")))
                && hit_routes.insert(p.clone())
            {
                reasons.push(format!(
                    "exercises {} {}, defined in changed file {}",
                    r.method, r.path, r.file
                ));
            }
        }
        if !hit_routes.is_empty() {
            score += 3.0;
        }
        let hits: Vec<&String> = words.iter().filter(|w| hay.contains(w.as_str())).collect();
        if !hits.is_empty() {
            score += 2.0;
            reasons.push(format!(
                "mentions {} from the changed paths",
                hits.iter()
                    .take(4)
                    .map(|w| format!("\"{w}\""))
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
        if score > 0.0 {
            if matches!(s.last_status.as_deref(), Some("failed") | Some("error")) {
                score += 1.0;
                reasons.push("failed on its last run".into());
            }
            if s.flaky {
                score += 1.0;
                reasons.push("marked flaky".into());
            }
        } else if s.tags.iter().any(|t| t.trim_start_matches('@') == "smoke") {
            score = 0.5;
            reasons.push("smoke baseline".into());
        }
        if score > 0.0 {
            out.push(Selection {
                scenario_id: s.id.clone(),
                name: s.name.clone(),
                score,
                reasons,
            });
        }
    }
    out.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.name.cmp(&b.name))
    });
    out
}

/// Facts the recommendation rules read. Filled by the API from analytics queries.
#[derive(Debug, Clone, Default)]
pub struct HealthFacts {
    /// (key, title) of requirements with no linked scenario.
    pub uncovered_requirements: Vec<(String, String)>,
    /// (key, title) of requirements whose linked scenarios have not all passed on their latest run.
    pub unverified_requirements: Vec<(String, String)>,
    /// (scenario id, name)
    pub flaky_scenarios: Vec<(String, String)>,
    /// (scenario id, name, consecutive failures)
    pub failing_scenarios: Vec<(String, String, i64)>,
    /// (scenario id, name, average duration ms over the window)
    pub slow_scenarios: Vec<(String, String, i64)>,
    pub never_run: Vec<(String, String)>,
    pub untested_routes: Vec<String>,
}

/// (kind, title, body, related) tuples, ready for `replace_heuristic_recommendations`.
pub fn recommendations(f: &HealthFacts) -> Vec<(String, String, String, Value)> {
    let mut out = vec![];
    for (key, title) in f.uncovered_requirements.iter().take(20) {
        out.push(("coverage".into(), format!("{key} has no test"), format!("Requirement {key} \"{title}\" is not linked to any scenario. Plan scenarios for it or link an existing one."), json!({"requirement_key": key})));
    }
    for (key, title) in f.unverified_requirements.iter().take(20) {
        out.push(("verification".into(), format!("{key} is covered but not verified"), format!("Requirement {key} \"{title}\" has linked scenarios, but not all of them passed on their latest run."), json!({"requirement_key": key})));
    }
    for (id, name) in f.flaky_scenarios.iter().take(20) {
        out.push(("flaky".into(), format!("Stabilise \"{name}\""), "This scenario passed on retry or alternated between pass and fail without an edit. Flaky tests hide real regressions.".into(), json!({"scenario_id": id})));
    }
    for (id, name, n) in f
        .failing_scenarios
        .iter()
        .filter(|(_, _, n)| *n >= 2)
        .take(20)
    {
        out.push(("failing".into(), format!("\"{name}\" has failed {n} runs in a row"), "A persistent failure is either a real defect that needs a fix or a stale test that needs updating. Open the latest investigation.".into(), json!({"scenario_id": id, "consecutive_failures": n})));
    }
    for (id, name, p95) in f.slow_scenarios.iter().take(10) {
        out.push(("performance".into(), format!("\"{name}\" is slow"), format!("Its average duration is {:.1} s. Consider splitting it or replacing setup steps with API calls.", *p95 as f64 / 1000.0), json!({"scenario_id": id, "avg_ms": p95})));
    }
    if !f.never_run.is_empty() {
        out.push((
            "coverage".into(),
            format!("{} approved scenarios have never run", f.never_run.len()),
            "Run them once to establish a baseline.".into(),
            json!({"scenario_ids": f.never_run.iter().map(|x| &x.0).collect::<Vec<_>>()}),
        ));
    }
    if !f.untested_routes.is_empty() {
        out.push((
            "coverage".into(),
            format!(
                "{} detected routes have no scenario",
                f.untested_routes.len()
            ),
            format!(
                "Routes such as {} appear in the code but in no scenario.",
                f.untested_routes
                    .iter()
                    .take(5)
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            json!({"routes": f.untested_routes}),
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selects_by_route_and_keyword() {
        let routes = vec![Route {
            method: "POST".into(),
            path: "/api/checkout".into(),
            file: "src/server.js".into(),
            kind: "api".into(),
        }];
        let sc = |id: &str, name: &str, text: &str, tags: &[&str]| ScenarioRef {
            id: id.into(),
            name: name.into(),
            text: text.into(),
            tags: tags.iter().map(|s| s.to_string()).collect(),
            ..Default::default()
        };
        let scenarios = vec![
            sc(
                "1",
                "Checkout totals",
                "When I send a POST request to \"/api/checkout\"",
                &[],
            ),
            sc(
                "2",
                "Pricing of two hammers",
                "Then I should see \"Subtotal\"",
                &[],
            ),
            sc("3", "Login", "Given I open \"/login\"", &[]),
            sc("4", "Home", "Given I open \"/\"", &["@smoke"]),
        ];
        let sel = select(
            &["src/server.js".into(), "src/pricing.js".into()],
            &routes,
            &scenarios,
        );
        let ids: Vec<&str> = sel.iter().map(|s| s.scenario_id.as_str()).collect();
        assert_eq!(ids, vec!["1", "2", "4"]);
        assert!(sel[0].reasons[0].contains("/api/checkout"));
        assert!(sel[1].reasons[0].contains("pricing"));
    }

    #[test]
    fn recs() {
        let f = HealthFacts {
            uncovered_requirements: vec![("REQ-1".into(), "Login".into())],
            failing_scenarios: vec![("s".into(), "x".into(), 1)],
            ..Default::default()
        };
        let r = recommendations(&f);
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].0, "coverage");
    }
}

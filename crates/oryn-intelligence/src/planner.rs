//! Test planning: a deterministic heuristic planner and an AI planner.
//!
//! Both produce a [`PlanDraft`] whose Gherkin is compiled against the closed step
//! library. A draft is a proposal: it is stored with status `proposed` and only
//! becomes runnable suites after a human approves it.

use crate::analysis::Route;
use once_cell::sync::Lazy;
use oryn_model_gateway::{untrusted, CompletionRequest, Message, Role, SAFETY_PREAMBLE};
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeSet;
use utoipa::ToSchema;

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct RequirementInput {
    pub key: String,
    pub title: String,
    pub body: String,
}

#[derive(Debug, Clone, Default)]
pub struct PlanInput {
    pub project_name: String,
    pub instructions: String,
    pub requirements: Vec<RequirementInput>,
    pub routes: Vec<Route>,
    /// (path, snippet) retrieved from the application context.
    pub context: Vec<(String, String)>,
    pub existing_scenarios: Vec<String>,
    pub app_summary: String,
    pub max_scenarios: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct RiskItem {
    pub area: String,
    pub level: String,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct DraftScenario {
    pub title: String,
    pub category: String,
    pub priority: String,
    pub risk: String,
    pub rationale: String,
    pub requirement_keys: Vec<String>,
    pub gherkin: String,
    /// True when every step compiled against the step library.
    pub valid: bool,
    pub diagnostics: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct PlanDraft {
    pub title: String,
    pub objective: String,
    pub rationale: String,
    pub risks: Vec<RiskItem>,
    pub coverage_gaps: Vec<String>,
    pub scenarios: Vec<DraftScenario>,
    /// `heuristic` or `ai`.
    pub source: String,
}

/// Escape a value for a double-quoted Gherkin step argument.
fn q(s: &str) -> String {
    s.replace('\\', "").replace('"', "'").trim().to_string()
}

fn tag(key: &str) -> String {
    key.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '-'
            }
        })
        .collect()
}

/// Compile one scenario's Gherkin and record validity.
pub fn validate(s: &mut DraftScenario) {
    s.diagnostics.clear();
    match oryn_bdd::compile(&s.gherkin) {
        Err(e) => {
            s.valid = false;
            s.diagnostics
                .push(format!("line {}: {}", e.line, e.message));
        }
        Ok(f) => {
            for d in f
                .errors
                .iter()
                .chain(f.scenarios.iter().flat_map(|sc| sc.unsupported.iter()))
            {
                s.diagnostics
                    .push(format!("line {}: {}", d.line, d.message));
            }
            if f.scenarios.is_empty() {
                s.diagnostics.push("no scenarios found".into());
            }
            s.valid = s.diagnostics.is_empty();
        }
    }
}

static API_CALL: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"`?\b(GET|POST|PUT|PATCH|DELETE)\s+(/api/[A-Za-z0-9_\-/{}:.]*)`?").unwrap()
});
static PAGE_PATH: Lazy<Regex> = Lazy::new(|| Regex::new(r"`(/[A-Za-z0-9_\-/]*)`").unwrap());
static QUOTED: Lazy<Regex> = Lazy::new(|| Regex::new(r#"["“]([^"”\n]{2,80})["”]"#).unwrap());
static BOLD: Lazy<Regex> = Lazy::new(|| Regex::new(r"\*\*([^*\n]{2,40})\*\*").unwrap());
static HTTP_CODE: Lazy<Regex> = Lazy::new(|| Regex::new(r"\b(?:HTTP|status)\s*(\d{3})\b").unwrap());
static CRITERION: Lazy<Regex> = Lazy::new(|| Regex::new(r"^\s*(?:[-*]|\d+[.)])\s+(.+)$").unwrap());

fn criteria(body: &str) -> Vec<String> {
    let lines: Vec<String> = body
        .lines()
        .filter_map(|l| CRITERION.captures(l).map(|c| c[1].to_string()))
        .collect();
    if lines.is_empty() {
        body.split(". ")
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect()
    } else {
        lines
    }
}

/// Deterministic planner: derives scenarios from acceptance criteria and detected routes.
/// Works with no model configured.
pub fn heuristic_plan(input: &PlanInput) -> PlanDraft {
    let max = if input.max_scenarios == 0 {
        25
    } else {
        input.max_scenarios
    };
    let mut scenarios = vec![];
    let mut covered_paths = BTreeSet::new();
    let mut gaps = vec![];

    for req in &input.requirements {
        let mut produced = 0;
        for (i, crit) in criteria(&req.body).iter().enumerate() {
            if scenarios.len() >= max {
                break;
            }
            let label = crit
                .chars()
                .take(70)
                .collect::<String>()
                .replace(['*', '`'], "");
            let tags = format!("@{}", tag(&req.key));
            if let Some(c) = API_CALL.captures(crit) {
                let (method, path) = (&c[1], &c[2]);
                if path.contains('{') || path.contains(':') || method != "GET" {
                    continue; // needs data the heuristic cannot invent
                }
                let status = HTTP_CODE
                    .captures(crit)
                    .map(|c| c[1].to_string())
                    .unwrap_or_else(|| "200".into());
                covered_paths.insert(path.to_string());
                scenarios.push(DraftScenario {
                    title: format!("{} – {}", req.key, label),
                    category: "integration".into(),
                    priority: "medium".into(),
                    risk: format!("{method} {path} contract regresses"),
                    rationale: format!("Derived from criterion {} of {}: it names the endpoint {method} {path}.", i + 1, req.key),
                    requirement_keys: vec![req.key.clone()],
                    gherkin: format!(
                        "Feature: {}\n\n  @api {tags}\n  Scenario: {} responds\n    When I send a {method} request to \"{path}\"\n    Then the response status should be {status}\n    And the response time should be below 2000 ms\n",
                        q(&req.title), q(&format!("{method} {path}"))
                    ),
                    valid: false,
                    diagnostics: vec![],
                });
                produced += 1;
                continue;
            }
            let Some(path) = PAGE_PATH
                .captures_iter(crit)
                .map(|c| c[1].to_string())
                .find(|p| !p.starts_with("/api") && !p.contains(':'))
            else {
                continue;
            };
            let texts: Vec<String> = QUOTED
                .captures_iter(crit)
                .map(|c| q(&c[1]))
                .filter(|t| !t.contains('<') && !t.is_empty())
                .take(2)
                .collect();
            let labels: Vec<String> = BOLD
                .captures_iter(crit)
                .map(|c| q(&c[1]))
                .filter(|t| !t.contains('"'))
                .take(4)
                .collect();
            if texts.is_empty() && labels.is_empty() {
                continue;
            }
            let mut steps = format!("    Given I open \"{path}\"\n");
            let mut first = true;
            for l in &labels {
                steps.push_str(&format!(
                    "    {} \"{l}\" should be visible\n",
                    if first { "Then" } else { "And" }
                ));
                first = false;
            }
            for t in &texts {
                steps.push_str(&format!(
                    "    {} I should see \"{t}\"\n",
                    if first { "Then" } else { "And" }
                ));
                first = false;
            }
            covered_paths.insert(path.clone());
            scenarios.push(DraftScenario {
                title: format!("{} – {}", req.key, label),
                category: "functional".into(),
                priority: if i == 0 { "high".into() } else { "medium".into() },
                risk: format!("{} does not meet its acceptance criterion", req.title),
                rationale: format!("Derived from criterion {} of {}: it names the page {path} and the visible text or labels asserted here.", i + 1, req.key),
                requirement_keys: vec![req.key.clone()],
                gherkin: format!("Feature: {}\n\n  @browser {tags}\n  Scenario: {}\n{steps}", q(&req.title), q(&label)),
                valid: false,
                diagnostics: vec![],
            });
            produced += 1;
        }
        if produced == 0 {
            gaps.push(format!("{} {}: no criterion names a page path or a GET endpoint the heuristic can test; write scenarios by hand or use the AI planner.", req.key, req.title));
        }
    }

    // Route smoke tests for detected routes that no requirement covered.
    for r in &input.routes {
        if scenarios.len() >= max {
            break;
        }
        if covered_paths.contains(&r.path)
            || r.method != "GET"
            || r.path.contains(':')
            || r.path.contains('{')
            || r.path.contains('[')
        {
            continue;
        }
        covered_paths.insert(r.path.clone());
        let (tagname, body) = if r.kind == "api" {
            ("@api", format!("    When I send a GET request to \"{}\"\n    Then the response time should be below 2000 ms\n", r.path))
        } else {
            ("@browser", format!("    Given I open \"{}\"\n    Then I should not see \"Internal Server Error\"\n", r.path))
        };
        scenarios.push(DraftScenario {
            title: format!("Smoke: {} {}", r.method, r.path),
            category: "smoke".into(),
            priority: "low".into(),
            risk: "Route fails to load".into(),
            rationale: format!(
                "Route detected in {} with no requirement covering it.",
                r.file
            ),
            requirement_keys: vec![],
            gherkin: format!(
                "Feature: Smoke\n\n  {tagname} @smoke\n  Scenario: {} loads\n{body}",
                q(&r.path)
            ),
            valid: false,
            diagnostics: vec![],
        });
    }

    for s in &mut scenarios {
        validate(s);
    }
    if input.requirements.is_empty() {
        gaps.push("No requirements imported; scenarios come from detected routes only.".into());
    }
    let n_req = input.requirements.len();
    PlanDraft {
        title: format!("{}: heuristic test plan", input.project_name),
        objective: if input.instructions.trim().is_empty() { "Cover each acceptance criterion that names a page or endpoint, then smoke-test uncovered routes.".into() } else { input.instructions.trim().to_string() },
        rationale: format!("Deterministic plan from {n_req} requirement(s) and {} detected route(s). No model was used.", input.routes.len()),
        risks: vec![],
        coverage_gaps: gaps,
        scenarios,
        source: "heuristic".into(),
    }
}

/// JSON Schema the AI planner must satisfy.
pub fn plan_schema() -> Value {
    json!({
        "type": "object",
        "required": ["title", "objective", "rationale", "risks", "coverage_gaps", "scenarios"],
        "properties": {
            "title": {"type": "string"},
            "objective": {"type": "string"},
            "rationale": {"type": "string"},
            "risks": {"type": "array", "items": {"type": "object", "required": ["area", "level", "reason"], "properties": {
                "area": {"type": "string"}, "level": {"type": "string", "enum": ["low", "medium", "high"]}, "reason": {"type": "string"}}}},
            "coverage_gaps": {"type": "array", "items": {"type": "string"}},
            "scenarios": {"type": "array", "items": {"type": "object",
                "required": ["title", "category", "priority", "risk", "rationale", "requirement_keys", "gherkin"],
                "properties": {
                    "title": {"type": "string"},
                    "category": {"type": "string", "enum": ["functional", "negative", "edge_case", "integration", "smoke", "regression", "security", "accessibility"]},
                    "priority": {"type": "string", "enum": ["low", "medium", "high"]},
                    "risk": {"type": "string"},
                    "rationale": {"type": "string"},
                    "requirement_keys": {"type": "array", "items": {"type": "string"}},
                    "gherkin": {"type": "string"}
                }}}
        }
    })
}

fn step_library_text() -> String {
    oryn_bdd::step_library()
        .iter()
        .map(|s| format!("- {}  (e.g. `{}`)", s.description, s.example))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Build the AI planner request. All project content is fenced as untrusted data.
pub fn ai_plan_request(input: &PlanInput, model: &str) -> CompletionRequest {
    let max = if input.max_scenarios == 0 {
        15
    } else {
        input.max_scenarios.min(40)
    };
    let system = format!(
        "{SAFETY_PREAMBLE}\n\nYou are a senior QA engineer designing a risk-based test plan. Write each scenario as a complete Gherkin feature \
(\"Feature:\" line, one or more tags, one \"Scenario:\"). Use ONLY these step phrasings; any other wording is rejected by the compiler:\n{}\n\n\
Rules: tag browser scenarios @browser and API scenarios @api; tag each scenario with the requirement keys it covers (e.g. @REQ-3); \
use relative paths (\"/login\"), never invent credentials beyond those shown in the requirements; cover positive, negative and edge cases \
in proportion to risk; at most {max} scenarios; do not duplicate the existing scenarios listed.",
        step_library_text()
    );
    let mut user = String::new();
    // Everything below except our own labels comes from users or repositories, so all of
    // it is fenced: project name, summary, requirements, routes, files and scenario names.
    user.push_str(&untrusted("project name", &input.project_name));
    user.push('\n');
    if !input.instructions.trim().is_empty() {
        user.push_str(&untrusted(
            "planning instructions from the user",
            &input.instructions,
        ));
        user.push('\n');
    }
    if !input.app_summary.is_empty() {
        user.push_str(&untrusted("application summary", &input.app_summary));
        user.push('\n');
    }
    for r in &input.requirements {
        user.push_str(&untrusted(
            &format!("requirement {}", r.key),
            &format!(
                "{} {}\n{}",
                r.key,
                r.title,
                r.body.chars().take(6000).collect::<String>()
            ),
        ));
        user.push('\n');
    }
    if !input.routes.is_empty() {
        let routes: Vec<String> = input
            .routes
            .iter()
            .take(120)
            .map(|r| format!("{} {} ({})", r.method, r.path, r.kind))
            .collect();
        user.push_str(&untrusted("detected routes", &routes.join("\n")));
        user.push('\n');
    }
    for (path, snippet) in input.context.iter().take(12) {
        user.push_str(&untrusted(
            path,
            &snippet.chars().take(2500).collect::<String>(),
        ));
        user.push('\n');
    }
    if !input.existing_scenarios.is_empty() {
        user.push_str(&untrusted(
            "existing scenario names (do not duplicate these)",
            &input
                .existing_scenarios
                .iter()
                .take(200)
                .cloned()
                .collect::<Vec<_>>()
                .join("\n"),
        ));
        user.push('\n');
    }
    user.push_str("\nReturn the test plan as JSON matching the schema.");
    CompletionRequest {
        model: model.to_string(),
        system,
        messages: vec![Message {
            role: Role::User,
            content: user,
        }],
        max_tokens: 16_000,
        json_schema: Some(plan_schema()),
        purpose: "test_plan".into(),
    }
}

/// Turn the validated JSON into a draft and compile every scenario.
pub fn parse_ai_plan(v: &Value) -> PlanDraft {
    let s = |x: &Value| x.as_str().unwrap_or("").to_string();
    let mut scenarios: Vec<DraftScenario> = v["scenarios"]
        .as_array()
        .map(|a| {
            a.iter()
                .map(|sc| DraftScenario {
                    title: s(&sc["title"]),
                    category: s(&sc["category"]),
                    priority: s(&sc["priority"]),
                    risk: s(&sc["risk"]),
                    rationale: s(&sc["rationale"]),
                    requirement_keys: sc["requirement_keys"]
                        .as_array()
                        .map(|k| {
                            k.iter()
                                .filter_map(|x| x.as_str().map(String::from))
                                .collect()
                        })
                        .unwrap_or_default(),
                    gherkin: s(&sc["gherkin"]),
                    valid: false,
                    diagnostics: vec![],
                })
                .collect()
        })
        .unwrap_or_default();
    for sc in &mut scenarios {
        validate(sc);
    }
    PlanDraft {
        title: s(&v["title"]),
        objective: s(&v["objective"]),
        rationale: s(&v["rationale"]),
        risks: v["risks"]
            .as_array()
            .map(|a| {
                a.iter()
                    .map(|r| RiskItem {
                        area: s(&r["area"]),
                        level: s(&r["level"]),
                        reason: s(&r["reason"]),
                    })
                    .collect()
            })
            .unwrap_or_default(),
        coverage_gaps: v["coverage_gaps"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|x| x.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default(),
        scenarios,
        source: "ai".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input() -> PlanInput {
        PlanInput {
            project_name: "Northwind".into(),
            requirements: vec![
                RequirementInput {
                    key: "US-2".into(),
                    title: "Login".into(),
                    body: "- **AC-2.1** The page `/login` has fields labelled **Email** and **Password** and a **Sign in** button.\n- **AC-2.4** `POST /api/auth/login` returns a token.\n- **AC-2.6** `GET /api/products` returns the catalogue (HTTP 200).".into(),
                },
                RequirementInput { key: "US-9".into(), title: "Vague".into(), body: "Users should be happy.".into() },
            ],
            routes: vec![
                Route { method: "GET".into(), path: "/api/health".into(), file: "src/server.js".into(), kind: "api".into() },
                Route { method: "GET".into(), path: "/login".into(), file: "src/server.js".into(), kind: "page".into() },
            ],
            ..Default::default()
        }
    }

    #[test]
    fn heuristic_plan_compiles() {
        let p = heuristic_plan(&input());
        assert!(
            p.scenarios.iter().all(|s| s.valid),
            "{:#?}",
            p.scenarios
                .iter()
                .map(|s| (&s.gherkin, &s.diagnostics))
                .collect::<Vec<_>>()
        );
        let titles: Vec<_> = p.scenarios.iter().map(|s| s.title.as_str()).collect();
        assert!(titles
            .iter()
            .any(|t| t.starts_with("US-2") && t.contains("AC-2.1")));
        assert!(p
            .scenarios
            .iter()
            .any(|s| s.gherkin.contains("GET request to \"/api/products\"")
                && s.gherkin.contains("status should be 200")));
        assert!(
            !p.scenarios.iter().any(|s| s.gherkin.contains("POST")),
            "no invented request bodies"
        );
        assert!(titles.contains(&"Smoke: GET /api/health"));
        assert!(
            !titles.contains(&"Smoke: GET /login"),
            "covered by a requirement"
        );
        assert!(p.coverage_gaps.iter().any(|g| g.contains("US-9")));
        assert!(p.scenarios[0].gherkin.contains("@US-2"));
    }

    #[test]
    fn ai_plan_is_validated() {
        let v = json!({"title":"t","objective":"o","rationale":"r","risks":[],"coverage_gaps":[],"scenarios":[
            {"title":"ok","category":"functional","priority":"high","risk":"r","rationale":"r","requirement_keys":["REQ-1"],
             "gherkin":"Feature: F\n  @browser\n  Scenario: S\n    Given I open \"/\"\n    Then I should see \"Hi\"\n"},
            {"title":"bad","category":"functional","priority":"high","risk":"r","rationale":"r","requirement_keys":[],
             "gherkin":"Feature: F\n  Scenario: S\n    Given I teleport to Mars\n"}]});
        let p = parse_ai_plan(&v);
        assert!(p.scenarios[0].valid);
        assert!(!p.scenarios[1].valid);
        assert!(!p.scenarios[1].diagnostics.is_empty());
        let req = ai_plan_request(&input(), "m");
        assert!(req.messages[0]
            .content
            .contains("<untrusted source=\"requirement US-2\">"));
        assert!(req.system.contains("I open"));
    }
}

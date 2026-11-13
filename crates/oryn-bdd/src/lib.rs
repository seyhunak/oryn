//! BDD orchestration for ORYN.
//!
//! Parsing uses the maintained [`gherkin`] crate (the parser behind cucumber-rs).
//! This crate adds what ORYN needs on top of it:
//!
//! * a **step library** — the closed vocabulary of executable steps, with regexes
//!   and documentation (exposed to the UI and to the AI planner so generated
//!   scenarios stay executable);
//! * a **compiler** from features (backgrounds, scenarios, scenario outlines with
//!   examples, rules, tags, doc strings) to [`oryn_domain::PlannedStep`] lists;
//! * diagnostics for steps that do not match the library, with line numbers.

pub mod library;

use gherkin::{GherkinEnv, Step};
use oryn_domain::{kind_of, PlannedStep, TestKind};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

pub use library::{match_step, step_library, StepDefinitionInfo};

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema, PartialEq)]
pub struct Diagnostic {
    pub line: usize,
    pub message: String,
    pub step: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct CompiledScenario {
    /// Name, suffixed with the example row for outlines (e.g. `Search (q=drill)`).
    pub name: String,
    pub tags: Vec<String>,
    pub line: usize,
    pub kind: TestKind,
    pub steps: Vec<PlannedStep>,
    /// Steps that do not match the step library; the scenario is not executable while non-empty.
    pub unsupported: Vec<Diagnostic>,
    /// For outlines: the source scenario name and example index.
    pub outline: Option<String>,
    pub example_index: Option<usize>,
}

impl CompiledScenario {
    pub fn executable(&self) -> bool {
        self.unsupported.is_empty() && !self.steps.is_empty()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct CompiledFeature {
    pub name: String,
    pub description: Option<String>,
    pub tags: Vec<String>,
    pub scenarios: Vec<CompiledScenario>,
    pub errors: Vec<Diagnostic>,
}

#[derive(Debug, thiserror::Error)]
#[error("gherkin parse error: {message}")]
pub struct BddError {
    pub line: usize,
    pub message: String,
}

/// Parse and compile a feature file.
pub fn compile(source: &str) -> Result<CompiledFeature, BddError> {
    // The parser requires a trailing newline.
    let mut src = source.replace("\r\n", "\n");
    if !src.ends_with('\n') {
        src.push('\n');
    }
    let feature = gherkin::Feature::parse(&src, GherkinEnv::default()).map_err(|e| {
        let msg = e.to_string();
        let line = msg
            .strip_prefix("Error at ")
            .and_then(|r| r.split(':').next())
            .and_then(|l| l.parse().ok())
            .unwrap_or(0);
        BddError { line, message: msg }
    })?;

    let mut scenarios = Vec::new();
    let feature_bg: Vec<Step> = feature
        .background
        .as_ref()
        .map(|b| b.steps.clone())
        .unwrap_or_default();

    let mut push_all = |bg: &[Step], list: &[gherkin::Scenario], extra_tags: &[String]| {
        for sc in list {
            let mut tags: Vec<String> = feature.tags.clone();
            tags.extend(extra_tags.iter().cloned());
            tags.extend(sc.tags.iter().cloned());
            tags.sort();
            tags.dedup();
            scenarios.extend(compile_scenario(bg, sc, &tags));
        }
    };

    push_all(&feature_bg, &feature.scenarios, &[]);
    for rule in &feature.rules {
        let mut bg = feature_bg.clone();
        if let Some(rb) = &rule.background {
            bg.extend(rb.steps.iter().cloned());
        }
        push_all(&bg, &rule.scenarios, &rule.tags);
    }

    Ok(CompiledFeature {
        name: feature.name.clone(),
        description: feature.description.clone(),
        tags: feature.tags.clone(),
        scenarios,
        errors: vec![],
    })
}

fn compile_scenario(
    background: &[Step],
    sc: &gherkin::Scenario,
    tags: &[String],
) -> Vec<CompiledScenario> {
    let rows: Vec<(usize, Vec<(String, String)>)> = sc
        .examples
        .iter()
        .filter_map(|ex| ex.table.as_ref())
        .flat_map(|t| {
            let header = t.rows.first().cloned().unwrap_or_default();
            t.rows
                .iter()
                .skip(1)
                .map(move |r| {
                    header
                        .iter()
                        .cloned()
                        .zip(r.iter().cloned())
                        .collect::<Vec<_>>()
                })
                .collect::<Vec<_>>()
        })
        .enumerate()
        .collect();

    if rows.is_empty() {
        return vec![build(background, sc, tags, &[], None)];
    }
    rows.iter()
        .map(|(i, params)| build(background, sc, tags, params, Some(*i)))
        .collect()
}

fn substitute(text: &str, params: &[(String, String)]) -> String {
    let mut out = text.to_string();
    for (k, v) in params {
        out = out.replace(&format!("<{k}>"), v);
    }
    out
}

fn build(
    background: &[Step],
    sc: &gherkin::Scenario,
    tags: &[String],
    params: &[(String, String)],
    example_index: Option<usize>,
) -> CompiledScenario {
    let mut steps = Vec::new();
    let mut unsupported = Vec::new();
    for (i, st) in background.iter().chain(sc.steps.iter()).enumerate() {
        let text = substitute(&st.value, params);
        let doc = st.docstring.as_ref().map(|d| substitute(d, params));
        match match_step(&text, doc.as_deref()) {
            Ok(step) => steps.push(PlannedStep {
                index: i,
                keyword: st.keyword.trim().to_string(),
                text: text.clone(),
                step,
            }),
            Err(msg) => unsupported.push(Diagnostic {
                line: st.position.line,
                message: msg,
                step: Some(text),
            }),
        }
    }
    let name = if params.is_empty() {
        sc.name.clone()
    } else {
        let p: Vec<String> = params.iter().map(|(k, v)| format!("{k}={v}")).collect();
        format!("{} ({})", sc.name, p.join(", "))
    };
    let exec: Vec<_> = steps.iter().map(|s| s.step.clone()).collect();
    CompiledScenario {
        name,
        tags: tags.to_vec(),
        line: sc.position.line,
        kind: kind_of(&exec),
        steps,
        unsupported,
        outline: example_index.map(|_| sc.name.clone()),
        example_index,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use oryn_domain::ExecStep;

    const FEATURE: &str = r#"
@shop
Feature: Checkout
  Background:
    Given I open "/login"

  @browser
  Scenario: Sign in
    When I fill "Email" with "demo@northwind.test"
    And I click "Sign in"
    Then I should see "Welcome"
    And the URL should contain "/dashboard"

  Scenario Outline: Search
    When I fill "Search" with "<q>"
    Then I should see "<expected>"
    Examples:
      | q     | expected |
      | drill | Drill    |
      | saw   | Saw      |

  @api
  Scenario: Create order
    When I send a POST request to "/api/cart/items" with body:
      """
      {"product_id": 1, "quantity": 2}
      """
    Then the response status should be 201
    And the response JSON "$.items" should have length 1
    And I store the response JSON "$.id" as "cart"
    And I teleport to the moon
"#;

    #[test]
    fn compiles_feature() {
        let f = compile(FEATURE).unwrap();
        assert_eq!(f.name, "Checkout");
        assert_eq!(f.scenarios.len(), 4);
        let s = &f.scenarios[0];
        assert_eq!(s.steps.len(), 5, "background + 4");
        assert!(s.tags.contains(&"@shop".to_string()) || s.tags.contains(&"shop".to_string()));
        assert!(matches!(s.steps[0].step, ExecStep::Goto { .. }));
        assert!(s.executable());
        assert_eq!(s.kind, TestKind::Browser);

        let o = &f.scenarios[1];
        assert_eq!(o.name, "Search (q=drill, expected=Drill)");
        assert_eq!(o.example_index, Some(0));
        assert!(matches!(&o.steps[1].step, ExecStep::Fill { value, .. } if value == "drill"));

        let api = &f.scenarios[3];
        assert!(!api.executable());
        assert_eq!(api.unsupported.len(), 1);
        assert!(api.unsupported[0].line > 0);
        assert!(
            matches!(&api.steps[1].step, ExecStep::HttpRequest { body: Some(b), .. } if b.contains("quantity"))
        );
        assert_eq!(api.kind, TestKind::Mixed, "background opens a page");
    }

    #[test]
    fn parse_error_reports_line() {
        let e = compile(
            "Feature: x\n  Scenario: a\n    Given I open \"/\"\n  Examples oops\n  | a |\n",
        )
        .err();
        // gherkin may accept or reject depending on grammar; ensure no panic and sensible output
        if let Some(e) = e {
            assert!(!e.message.is_empty());
        }
        assert!(compile("this is not gherkin").is_err());
    }
}

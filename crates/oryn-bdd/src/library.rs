//! The closed vocabulary of executable steps.

use once_cell::sync::Lazy;
use oryn_domain::{ExecStep, HttpMethod};
use regex::{Captures, Regex};
use serde::Serialize;
use utoipa::ToSchema;

type Builder = fn(&Captures, Option<&str>) -> Result<ExecStep, String>;

struct Def {
    re: Regex,
    pattern: &'static str,
    kind: &'static str,
    description: &'static str,
    example: &'static str,
    build: Builder,
}

/// Public description of a step definition (for the Test Studio and the AI planner).
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct StepDefinitionInfo {
    pub pattern: &'static str,
    pub kind: &'static str,
    pub description: &'static str,
    pub example: &'static str,
}

fn s(c: &Captures, i: usize) -> String {
    c.get(i).map(|m| m.as_str().to_string()).unwrap_or_default()
}

fn n<T: std::str::FromStr>(c: &Captures, i: usize) -> Result<T, String> {
    s(c, i)
        .parse::<T>()
        .map_err(|_| format!("invalid number: {}", s(c, i)))
}

macro_rules! def {
    ($re:expr, $pattern:expr, $kind:expr, $desc:expr, $ex:expr, $build:expr) => {
        Def {
            re: Regex::new(concat!("(?i)^", $re, "$")).unwrap(),
            pattern: $pattern,
            kind: $kind,
            description: $desc,
            example: $ex,
            build: $build,
        }
    };
}

const Q: &str = r#""((?:[^"\\]|\\.)*)""#;

static DEFS: Lazy<Vec<Def>> = Lazy::new(|| {
    let _ = Q;
    vec![
        // ---- browser actions ----
        def!(r#"I (?:open|navigate to|go to|am on) "((?:[^"\\]|\\.)*)"(?: page)?"#, r#"I open "<path>""#, "browser", "Navigate to a path (relative to the environment base URL) or absolute URL.", r#"Given I open "/login""#,
            |c, _| Ok(ExecStep::Goto { url: s(c, 1) })),
        def!(r#"I click(?: on)? "((?:[^"\\]|\\.)*)""#, r#"I click "<target>""#, "browser", "Click an element by its button/link text, label, placeholder, test id or CSS selector.", r#"When I click "Sign in""#,
            |c, _| Ok(ExecStep::Click { target: s(c, 1) })),
        def!(r#"I (?:fill|type into|enter into) "((?:[^"\\]|\\.)*)" with "((?:[^"\\]|\\.)*)""#, r#"I fill "<target>" with "<value>""#, "browser", "Fill an input located by label, placeholder or selector.", r#"When I fill "Email" with "demo@northwind.test""#,
            |c, _| Ok(ExecStep::Fill { target: s(c, 1), value: s(c, 2) })),
        def!(r#"I press "((?:[^"\\]|\\.)*)"(?: in "((?:[^"\\]|\\.)*)")?"#, r#"I press "<key>""#, "browser", "Press a keyboard key, optionally in a target element.", r#"And I press "Enter""#,
            |c, _| Ok(ExecStep::Press { key: s(c, 1), target: c.get(2).map(|m| m.as_str().to_string()) })),
        def!(r#"I select "((?:[^"\\]|\\.)*)" from "((?:[^"\\]|\\.)*)""#, r#"I select "<value>" from "<target>""#, "browser", "Choose an option in a select element.", r#"When I select "Express" from "Shipping""#,
            |c, _| Ok(ExecStep::Select { target: s(c, 2), value: s(c, 1) })),
        def!(r#"I check "((?:[^"\\]|\\.)*)""#, r#"I check "<target>""#, "browser", "Tick a checkbox or radio.", r#"And I check "Remember me""#,
            |c, _| Ok(ExecStep::Check { target: s(c, 1) })),
        def!(r#"I uncheck "((?:[^"\\]|\\.)*)""#, r#"I uncheck "<target>""#, "browser", "Untick a checkbox.", r#"And I uncheck "Newsletter""#,
            |c, _| Ok(ExecStep::Uncheck { target: s(c, 1) })),
        def!(r#"I hover(?: over)? "((?:[^"\\]|\\.)*)""#, r#"I hover "<target>""#, "browser", "Move the pointer over an element.", r#"When I hover "Account""#,
            |c, _| Ok(ExecStep::Hover { target: s(c, 1) })),
        def!(r#"I wait (\d+) ?ms"#, "I wait <n> ms", "browser", "Pause (avoid; prefer expectations, which wait automatically).", "And I wait 500 ms",
            |c, _| Ok(ExecStep::Wait { ms: n(c, 1)? })),
        def!(r#"I take a screenshot "((?:[^"\\]|\\.)*)""#, r#"I take a screenshot "<name>""#, "browser", "Capture a named screenshot as evidence.", r#"Then I take a screenshot "cart""#,
            |c, _| Ok(ExecStep::Screenshot { name: s(c, 1) })),
        // ---- browser expectations ----
        def!(r#"I should see "((?:[^"\\]|\\.)*)""#, r#"I should see "<text>""#, "browser", "Expect text to be visible (waits up to the step timeout).", r#"Then I should see "Order confirmed""#,
            |c, _| Ok(ExecStep::ExpectText { text: s(c, 1), visible: true })),
        def!(r#"I should not see "((?:[^"\\]|\\.)*)""#, r#"I should not see "<text>""#, "browser", "Expect text to be absent.", r#"Then I should not see "Invalid email""#,
            |c, _| Ok(ExecStep::ExpectText { text: s(c, 1), visible: false })),
        def!(r#"the URL should contain "((?:[^"\\]|\\.)*)""#, r#"the URL should contain "<text>""#, "browser", "Expect the current URL to contain text.", r#"And the URL should contain "/dashboard""#,
            |c, _| Ok(ExecStep::ExpectUrlContains { text: s(c, 1) })),
        def!(r#"the page title should be "((?:[^"\\]|\\.)*)""#, r#"the page title should be "<text>""#, "browser", "Expect the document title.", r#"Then the page title should be "Cart""#,
            |c, _| Ok(ExecStep::ExpectTitle { text: s(c, 1) })),
        def!(r#""((?:[^"\\]|\\.)*)" should be visible"#, r#""<target>" should be visible"#, "browser", "Expect an element to be visible.", r#"Then "Checkout" should be visible"#,
            |c, _| Ok(ExecStep::ExpectElementVisible { target: s(c, 1) })),
        def!(r#""((?:[^"\\]|\\.)*)" should contain "((?:[^"\\]|\\.)*)""#, r#""<target>" should contain "<text>""#, "browser", "Expect an element's text or value to contain text.", r##"Then "#total" should contain "$21.60""##,
            |c, _| Ok(ExecStep::ExpectElementContains { target: s(c, 1), text: s(c, 2) })),
        // ---- API ----
        def!(r#"I set (?:the )?header "((?:[^"\\]|\\.)*)" to "((?:[^"\\]|\\.)*)""#, r#"I set header "<name>" to "<value>""#, "api", "Set a request header for subsequent API requests.", r#"Given I set header "Authorization" to "Bearer {{token}}""#,
            |c, _| Ok(ExecStep::SetHeader { name: s(c, 1), value: s(c, 2) })),
        def!(r#"I send an? (GET|POST|PUT|PATCH|DELETE|HEAD|OPTIONS) request to "((?:[^"\\]|\\.)*)"(?: with body:?)?"#, r#"I send a <METHOD> request to "<path>" [with body:]"#, "api", "Send an HTTP request; a following doc string is the body.", r#"When I send a GET request to "/api/products""#,
            |c, doc| {
                let method = HttpMethod::parse(&s(c, 1)).ok_or("unknown method")?;
                Ok(ExecStep::HttpRequest { method, path: s(c, 2), body: doc.map(|d| d.to_string()) })
            }),
        def!(r#"the response status should be (\d{3})"#, "the response status should be <code>", "api", "Expect the HTTP status code.", "Then the response status should be 200",
            |c, _| Ok(ExecStep::ExpectStatus { code: n(c, 1)? })),
        def!(r#"the response JSON "((?:[^"\\]|\\.)*)" should equal "((?:[^"\\]|\\.)*)""#, r#"the response JSON "<jsonpath>" should equal "<value>""#, "api", "Expect a JSONPath value (compared as JSON when the value parses as JSON).", r#"And the response JSON "$.total" should equal "21.6""#,
            |c, _| Ok(ExecStep::ExpectJsonEquals { path: s(c, 1), value: s(c, 2) })),
        def!(r#"the response JSON "((?:[^"\\]|\\.)*)" should exist"#, r#"the response JSON "<jsonpath>" should exist"#, "api", "Expect a JSONPath to match at least one value.", r#"And the response JSON "$.order_id" should exist"#,
            |c, _| Ok(ExecStep::ExpectJsonExists { path: s(c, 1) })),
        def!(r#"the response JSON "((?:[^"\\]|\\.)*)" should have length (\d+)"#, r#"the response JSON "<jsonpath>" should have length <n>"#, "api", "Expect an array or string length at a JSONPath.", r#"And the response JSON "$.items" should have length 3"#,
            |c, _| Ok(ExecStep::ExpectJsonLength { path: s(c, 1), length: n(c, 2)? })),
        def!(r#"the response time should be (?:below|under|less than) (\d+) ?ms"#, "the response time should be below <n> ms", "api", "Expect the response time.", "And the response time should be below 500 ms",
            |c, _| Ok(ExecStep::ExpectResponseTimeBelow { ms: n(c, 1)? })),
        def!(r#"the response should match (?:the )?(?:JSON )?schema:?"#, "the response should match schema: (doc string)", "api", "Validate the response body against a JSON Schema given as a doc string.", "Then the response should match schema:",
            |_, doc| {
                let d = doc.ok_or("schema doc string required")?;
                let schema: serde_json::Value = serde_json::from_str(d).map_err(|e| format!("invalid JSON schema: {e}"))?;
                Ok(ExecStep::ExpectJsonSchema { schema })
            }),
        def!(r#"I store the response JSON "((?:[^"\\]|\\.)*)" as "((?:[^"\\]|\\.)*)""#, r#"I store the response JSON "<jsonpath>" as "<var>""#, "api", "Save a JSONPath value into a variable usable as {{var}}.", r#"And I store the response JSON "$.order_id" as "order""#,
            |c, _| Ok(ExecStep::StoreJson { path: s(c, 1), var: s(c, 2) })),
    ]
});

/// Unescape `\"` inside quoted parameters.
fn unescape(step: ExecStep) -> ExecStep {
    let mut v = serde_json::to_value(&step).expect("step serialises");
    if let Some(obj) = v.as_object_mut() {
        for (k, val) in obj.iter_mut() {
            if k == "body" || k == "schema" {
                continue;
            }
            if let Some(sv) = val.as_str() {
                *val = serde_json::Value::String(sv.replace("\\\"", "\""));
            }
        }
    }
    serde_json::from_value(v).unwrap_or(step)
}

/// Match a step's text (without keyword) against the library.
pub fn match_step(text: &str, docstring: Option<&str>) -> Result<ExecStep, String> {
    let t = text.trim();
    for d in DEFS.iter() {
        if let Some(c) = d.re.captures(t) {
            return (d.build)(&c, docstring).map(unescape);
        }
    }
    Err(format!(
        "No step definition matches \"{t}\". See the step library for supported steps."
    ))
}

pub fn step_library() -> Vec<StepDefinitionInfo> {
    DEFS.iter()
        .map(|d| StepDefinitionInfo {
            pattern: d.pattern,
            kind: d.kind,
            description: d.description,
            example: d.example,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_example_matches() {
        for d in step_library() {
            let text = d.example.split_once(' ').unwrap().1;
            let doc = if d.pattern.contains("schema") {
                Some(r#"{"type":"object"}"#)
            } else {
                None
            };
            assert!(
                match_step(text, doc).is_ok(),
                "example failed: {}",
                d.example
            );
        }
    }

    #[test]
    fn specifics() {
        assert_eq!(
            match_step(r#"I should not see "x""#, None).unwrap(),
            ExecStep::ExpectText {
                text: "x".into(),
                visible: false
            }
        );
        assert_eq!(
            match_step(r#"I fill "Name" with "say \"hi\"""#, None).unwrap(),
            ExecStep::Fill {
                target: "Name".into(),
                value: "say \"hi\"".into()
            }
        );
        assert!(matches!(
            match_step(r#"i send a delete request to "/x""#, None).unwrap(),
            ExecStep::HttpRequest {
                method: HttpMethod::Delete,
                ..
            }
        ));
        assert!(match_step("the response status should be abc", None).is_err());
        assert!(match_step("the response should match schema:", None).is_err());
    }
}

//! The executable step model.
//!
//! Gherkin text is compiled (by `oryn-bdd`) into a list of [`ExecStep`]s. Browser
//! steps are dispatched to the Playwright adapter as primitive actions or
//! *observations*; every expectation is evaluated by the Rust engine against the
//! observation the adapter reported, never by the adapter or by an LLM.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use utoipa::ToSchema;

use crate::TestKind;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ExecStep {
    // ---- browser actions ----
    Goto {
        url: String,
    },
    Click {
        target: String,
    },
    Fill {
        target: String,
        value: String,
    },
    Press {
        key: String,
        target: Option<String>,
    },
    Select {
        target: String,
        value: String,
    },
    Check {
        target: String,
    },
    Uncheck {
        target: String,
    },
    Hover {
        target: String,
    },
    Wait {
        ms: u64,
    },
    Screenshot {
        name: String,
    },
    // ---- browser expectations (evaluated in Rust) ----
    ExpectText {
        text: String,
        visible: bool,
    },
    ExpectUrlContains {
        text: String,
    },
    ExpectTitle {
        text: String,
    },
    ExpectElementVisible {
        target: String,
    },
    ExpectElementContains {
        target: String,
        text: String,
    },
    // ---- API actions ----
    SetHeader {
        name: String,
        value: String,
    },
    HttpRequest {
        method: HttpMethod,
        path: String,
        body: Option<String>,
    },
    // ---- API expectations ----
    ExpectStatus {
        code: u16,
    },
    ExpectJsonEquals {
        path: String,
        value: String,
    },
    ExpectJsonExists {
        path: String,
    },
    ExpectJsonLength {
        path: String,
        length: usize,
    },
    ExpectResponseTimeBelow {
        ms: u64,
    },
    ExpectJsonSchema {
        schema: serde_json::Value,
    },
    StoreJson {
        path: String,
        var: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "UPPERCASE")]
pub enum HttpMethod {
    Get,
    Post,
    Put,
    Patch,
    Delete,
    Head,
    Options,
}

impl HttpMethod {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Get => "GET",
            Self::Post => "POST",
            Self::Put => "PUT",
            Self::Patch => "PATCH",
            Self::Delete => "DELETE",
            Self::Head => "HEAD",
            Self::Options => "OPTIONS",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Some(match s.to_ascii_uppercase().as_str() {
            "GET" => Self::Get,
            "POST" => Self::Post,
            "PUT" => Self::Put,
            "PATCH" => Self::Patch,
            "DELETE" => Self::Delete,
            "HEAD" => Self::Head,
            "OPTIONS" => Self::Options,
            _ => return None,
        })
    }
}

impl ExecStep {
    pub fn is_browser(&self) -> bool {
        use ExecStep::*;
        matches!(
            self,
            Goto { .. }
                | Click { .. }
                | Fill { .. }
                | Press { .. }
                | Select { .. }
                | Check { .. }
                | Uncheck { .. }
                | Hover { .. }
                | Wait { .. }
                | Screenshot { .. }
                | ExpectText { .. }
                | ExpectUrlContains { .. }
                | ExpectTitle { .. }
                | ExpectElementVisible { .. }
                | ExpectElementContains { .. }
        )
    }

    pub fn is_api(&self) -> bool {
        !self.is_browser()
    }

    pub fn is_expectation(&self) -> bool {
        use ExecStep::*;
        matches!(
            self,
            ExpectText { .. }
                | ExpectUrlContains { .. }
                | ExpectTitle { .. }
                | ExpectElementVisible { .. }
                | ExpectElementContains { .. }
                | ExpectStatus { .. }
                | ExpectJsonEquals { .. }
                | ExpectJsonExists { .. }
                | ExpectJsonLength { .. }
                | ExpectResponseTimeBelow { .. }
                | ExpectJsonSchema { .. }
        )
    }

    /// Replace `{{var}}` placeholders in every string field.
    pub fn interpolate(&self, vars: &BTreeMap<String, String>) -> ExecStep {
        let f = |s: &String| interpolate(s, vars);
        use ExecStep::*;
        match self {
            Goto { url } => Goto { url: f(url) },
            Click { target } => Click { target: f(target) },
            Fill { target, value } => Fill {
                target: f(target),
                value: f(value),
            },
            Press { key, target } => Press {
                key: f(key),
                target: target.as_ref().map(f),
            },
            Select { target, value } => Select {
                target: f(target),
                value: f(value),
            },
            Check { target } => Check { target: f(target) },
            Uncheck { target } => Uncheck { target: f(target) },
            Hover { target } => Hover { target: f(target) },
            Wait { ms } => Wait { ms: *ms },
            Screenshot { name } => Screenshot { name: f(name) },
            ExpectText { text, visible } => ExpectText {
                text: f(text),
                visible: *visible,
            },
            ExpectUrlContains { text } => ExpectUrlContains { text: f(text) },
            ExpectTitle { text } => ExpectTitle { text: f(text) },
            ExpectElementVisible { target } => ExpectElementVisible { target: f(target) },
            ExpectElementContains { target, text } => ExpectElementContains {
                target: f(target),
                text: f(text),
            },
            SetHeader { name, value } => SetHeader {
                name: f(name),
                value: f(value),
            },
            HttpRequest { method, path, body } => HttpRequest {
                method: *method,
                path: f(path),
                body: body.as_ref().map(f),
            },
            ExpectStatus { code } => ExpectStatus { code: *code },
            ExpectJsonEquals { path, value } => ExpectJsonEquals {
                path: f(path),
                value: f(value),
            },
            ExpectJsonExists { path } => ExpectJsonExists { path: f(path) },
            ExpectJsonLength { path, length } => ExpectJsonLength {
                path: f(path),
                length: *length,
            },
            ExpectResponseTimeBelow { ms } => ExpectResponseTimeBelow { ms: *ms },
            ExpectJsonSchema { schema } => ExpectJsonSchema {
                schema: schema.clone(),
            },
            StoreJson { path, var } => StoreJson {
                path: f(path),
                var: var.clone(),
            },
        }
    }
}

/// Replace `{{name}}` placeholders. Unknown placeholders are left untouched so the
/// failure is visible in evidence rather than silently becoming an empty string.
pub fn interpolate(input: &str, vars: &BTreeMap<String, String>) -> String {
    if !input.contains("{{") {
        return input.to_string();
    }
    let mut out = String::with_capacity(input.len());
    let mut rest = input;
    while let Some(start) = rest.find("{{") {
        out.push_str(&rest[..start]);
        let after = &rest[start + 2..];
        match after.find("}}") {
            Some(end) => {
                let key = after[..end].trim();
                match vars.get(key) {
                    Some(v) => out.push_str(v),
                    None => {
                        out.push_str("{{");
                        out.push_str(&after[..end]);
                        out.push_str("}}");
                    }
                }
                rest = &after[end + 2..];
            }
            None => {
                out.push_str(&rest[start..]);
                rest = "";
            }
        }
    }
    out.push_str(rest);
    out
}

/// Determine the test kind from a list of steps.
pub fn kind_of(steps: &[ExecStep]) -> TestKind {
    let browser = steps.iter().any(|s| s.is_browser());
    let api = steps.iter().any(|s| s.is_api());
    match (browser, api) {
        (true, true) => TestKind::Mixed,
        (false, true) => TestKind::Api,
        _ => TestKind::Browser,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interpolation() {
        let mut v = BTreeMap::new();
        v.insert("id".to_string(), "42".to_string());
        assert_eq!(interpolate("/orders/{{id}}", &v), "/orders/42");
        assert_eq!(interpolate("/orders/{{ id }}/x", &v), "/orders/42/x");
        assert_eq!(interpolate("{{missing}}", &v), "{{missing}}");
        assert_eq!(interpolate("open {{", &v), "open {{");
    }

    #[test]
    fn kinds() {
        let b = ExecStep::Goto { url: "/".into() };
        let a = ExecStep::ExpectStatus { code: 200 };
        assert_eq!(kind_of(std::slice::from_ref(&b)), TestKind::Browser);
        assert_eq!(kind_of(std::slice::from_ref(&a)), TestKind::Api);
        assert_eq!(kind_of(&[b, a]), TestKind::Mixed);
    }

    #[test]
    fn serde_shape() {
        let s = ExecStep::HttpRequest {
            method: HttpMethod::Post,
            path: "/x".into(),
            body: None,
        };
        let j = serde_json::to_value(&s).unwrap();
        assert_eq!(j["type"], "http_request");
        assert_eq!(j["method"], "POST");
    }
}

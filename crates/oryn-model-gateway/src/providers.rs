//! Provider adapters. Each translates [`CompletionRequest`] to one vendor's wire format.

use async_trait::async_trait;
use oryn_domain::AiProviderKind;
use serde_json::{json, Value};
use std::time::Duration;

use crate::{http_error, CompletionRequest, CompletionResponse, GatewayError, ModelProvider, Role};

fn client(timeout: Duration) -> reqwest::Client {
    let builder = match crate::outbound_guard() {
        // Provider base URLs are user-supplied: resolve once, check, and pin.
        Some(allow_private) => oryn_auth::net::guarded_client(allow_private),
        None => reqwest::Client::builder(),
    };
    builder.timeout(timeout).build().expect("http client")
}

fn retry_after(resp: &reqwest::Response) -> Option<Duration> {
    resp.headers()
        .get("retry-after")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse::<u64>().ok())
        .map(Duration::from_secs)
}

async fn send_json(rb: reqwest::RequestBuilder) -> Result<Value, GatewayError> {
    let resp = rb
        .send()
        .await
        .map_err(|e| GatewayError::Network(e.to_string()))?;
    let status = resp.status().as_u16();
    let ra = retry_after(&resp);
    let text = resp
        .text()
        .await
        .map_err(|e| GatewayError::Network(e.to_string()))?;
    if !(200..300).contains(&status) {
        return Err(http_error(status, &text, ra));
    }
    serde_json::from_str(&text).map_err(|e| GatewayError::Rejected {
        status,
        message: format!("unparseable response: {e}"),
    })
}

fn role(r: &Role) -> &'static str {
    match r {
        Role::User => "user",
        Role::Assistant => "assistant",
    }
}

// ───────────── Anthropic (Messages API) ─────────────

pub struct AnthropicProvider {
    pub api_key: String,
    pub base_url: String,
    http: reqwest::Client,
}

impl AnthropicProvider {
    pub const API_VERSION: &'static str = "2026-10-21";
    pub const DEFAULT_MODEL: &'static str = "claude-opus-5-5";

    pub fn new(api_key: String, base_url: Option<String>) -> Self {
        Self {
            api_key,
            base_url: base_url.unwrap_or_else(|| "https://api.anthropic.com".into()),
            http: client(Duration::from_secs(600)),
        }
    }

    /// Models that support server-side refusal fallbacks; enabled by default for them.
    fn supports_fallbacks(model: &str) -> bool {
        [
            "claude-fable-5-2",
            "claude-fable-5-1",
            "claude-opus-5-5",
            "claude-opus-5",
            "claude-sonnet-5-5",
        ]
        .contains(&model)
    }
}

#[async_trait]
impl ModelProvider for AnthropicProvider {
    fn kind(&self) -> AiProviderKind {
        AiProviderKind::Anthropic
    }

    async fn complete(&self, req: &CompletionRequest) -> Result<CompletionResponse, GatewayError> {
        let mut body = json!({
            "model": req.model,
            "max_tokens": req.max_tokens,
            "system": req.system,
            "messages": req.messages.iter().map(|m| json!({"role": role(&m.role), "content": m.content})).collect::<Vec<_>>(),
        });
        if let Some(schema) = &req.json_schema {
            body["output_config"] =
                json!({"format": {"type": "json_schema", "schema": strictify(schema.clone())}});
        }
        let mut rb = self
            .http
            .post(format!(
                "{}/v1/messages",
                self.base_url.trim_end_matches('/')
            ))
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", Self::API_VERSION);
        if Self::supports_fallbacks(&req.model) {
            body["fallbacks"] = json!("default");
            rb = rb.header("anthropic-beta", "server-side-fallback-2026-07-01");
        }
        let v = send_json(rb.json(&body)).await?;
        if v["stop_reason"] == "refusal" {
            let cat = v["stop_details"]["category"]
                .as_str()
                .unwrap_or("unspecified");
            return Err(GatewayError::Refused(format!("category {cat}")));
        }
        let text = v["content"]
            .as_array()
            .map(|blocks| {
                blocks
                    .iter()
                    .filter(|b| b["type"] == "text")
                    .filter_map(|b| b["text"].as_str())
                    .collect::<Vec<_>>()
                    .join("")
            })
            .unwrap_or_default();
        Ok(CompletionResponse {
            text,
            input_tokens: v["usage"]["input_tokens"].as_u64().unwrap_or(0) as u32,
            output_tokens: v["usage"]["output_tokens"].as_u64().unwrap_or(0) as u32,
            model: v["model"].as_str().unwrap_or(&req.model).to_string(),
        })
    }
}

/// Structured-output schemas require `additionalProperties: false` on objects.
pub fn strictify(mut v: Value) -> Value {
    fn walk(v: &mut Value) {
        if let Some(o) = v.as_object_mut() {
            if o.get("type").and_then(Value::as_str) == Some("object")
                && !o.contains_key("additionalProperties")
            {
                o.insert("additionalProperties".into(), Value::Bool(false));
            }
            for (_, child) in o.iter_mut() {
                walk(child);
            }
        } else if let Some(a) = v.as_array_mut() {
            for c in a {
                walk(c);
            }
        }
    }
    walk(&mut v);
    v
}

// ───────────── OpenAI, Azure OpenAI and OpenAI-compatible (Chat Completions) ─────────────

pub enum OpenAiFlavor {
    OpenAi,
    Azure { api_version: String },
    Compatible,
}

pub struct OpenAiProvider {
    pub api_key: Option<String>,
    pub base_url: String,
    pub flavor: OpenAiFlavor,
    http: reqwest::Client,
}

impl OpenAiProvider {
    pub fn new(api_key: Option<String>, base_url: Option<String>, flavor: OpenAiFlavor) -> Self {
        let base = base_url.unwrap_or_else(|| "https://api.openai.com/v1".into());
        Self {
            api_key,
            base_url: base,
            flavor,
            http: client(Duration::from_secs(300)),
        }
    }
}

#[async_trait]
impl ModelProvider for OpenAiProvider {
    fn kind(&self) -> AiProviderKind {
        match self.flavor {
            OpenAiFlavor::OpenAi => AiProviderKind::Openai,
            OpenAiFlavor::Azure { .. } => AiProviderKind::AzureOpenai,
            OpenAiFlavor::Compatible => AiProviderKind::OpenaiCompatible,
        }
    }

    async fn complete(&self, req: &CompletionRequest) -> Result<CompletionResponse, GatewayError> {
        let mut messages = vec![json!({"role": "system", "content": req.system})];
        messages.extend(
            req.messages
                .iter()
                .map(|m| json!({"role": role(&m.role), "content": m.content})),
        );
        let mut body = json!({"messages": messages, "max_completion_tokens": req.max_tokens});
        if let Some(schema) = &req.json_schema {
            body["response_format"] = json!({"type": "json_schema", "json_schema": {"name": "oryn_output", "schema": schema}});
        }
        let base = self.base_url.trim_end_matches('/');
        let rb = match &self.flavor {
            OpenAiFlavor::Azure { api_version } => {
                let key = self
                    .api_key
                    .clone()
                    .ok_or_else(|| GatewayError::NotConfigured("Azure OpenAI API key".into()))?;
                self.http
                    .post(format!(
                        "{base}/openai/deployments/{}/chat/completions?api-version={api_version}",
                        req.model
                    ))
                    .header("api-key", key)
            }
            _ => {
                body["model"] = json!(req.model);
                let mut rb = self.http.post(format!("{base}/chat/completions"));
                if let Some(k) = &self.api_key {
                    rb = rb.bearer_auth(k);
                }
                rb
            }
        };
        let v = send_json(rb.json(&body)).await?;
        let choice = &v["choices"][0];
        if choice["message"]["refusal"].is_string() {
            return Err(GatewayError::Refused(
                choice["message"]["refusal"]
                    .as_str()
                    .unwrap_or("")
                    .to_string(),
            ));
        }
        Ok(CompletionResponse {
            text: choice["message"]["content"]
                .as_str()
                .unwrap_or("")
                .to_string(),
            input_tokens: v["usage"]["prompt_tokens"].as_u64().unwrap_or(0) as u32,
            output_tokens: v["usage"]["completion_tokens"].as_u64().unwrap_or(0) as u32,
            model: v["model"].as_str().unwrap_or(&req.model).to_string(),
        })
    }
}

// ───────────── Google Gemini (generateContent) ─────────────

pub struct GeminiProvider {
    pub api_key: String,
    pub base_url: String,
    http: reqwest::Client,
}

impl GeminiProvider {
    pub fn new(api_key: String, base_url: Option<String>) -> Self {
        Self {
            api_key,
            base_url: base_url
                .unwrap_or_else(|| "https://generativelanguage.googleapis.com".into()),
            http: client(Duration::from_secs(300)),
        }
    }
}

#[async_trait]
impl ModelProvider for GeminiProvider {
    fn kind(&self) -> AiProviderKind {
        AiProviderKind::Gemini
    }

    async fn complete(&self, req: &CompletionRequest) -> Result<CompletionResponse, GatewayError> {
        let contents: Vec<Value> = req
            .messages
            .iter()
            .map(|m| json!({"role": if m.role == Role::User { "user" } else { "model" }, "parts": [{"text": m.content}]}))
            .collect();
        let mut gen = json!({"maxOutputTokens": req.max_tokens});
        if req.json_schema.is_some() {
            gen["responseMimeType"] = json!("application/json");
        }
        let body = json!({"systemInstruction": {"parts": [{"text": req.system}]}, "contents": contents, "generationConfig": gen});
        let url = format!(
            "{}/v1beta/models/{}:generateContent",
            self.base_url.trim_end_matches('/'),
            req.model
        );
        let v = send_json(
            self.http
                .post(url)
                .header("x-goog-api-key", &self.api_key)
                .json(&body),
        )
        .await?;
        let text = v["candidates"][0]["content"]["parts"]
            .as_array()
            .map(|p| {
                p.iter()
                    .filter_map(|x| x["text"].as_str())
                    .collect::<Vec<_>>()
                    .join("")
            })
            .unwrap_or_default();
        if text.is_empty() {
            if let Some(r) = v["promptFeedback"]["blockReason"].as_str() {
                return Err(GatewayError::Refused(r.to_string()));
            }
        }
        Ok(CompletionResponse {
            text,
            input_tokens: v["usageMetadata"]["promptTokenCount"].as_u64().unwrap_or(0) as u32,
            output_tokens: v["usageMetadata"]["candidatesTokenCount"]
                .as_u64()
                .unwrap_or(0) as u32,
            model: req.model.clone(),
        })
    }
}

// ───────────── Ollama (local) ─────────────

pub struct OllamaProvider {
    pub base_url: String,
    http: reqwest::Client,
}

impl OllamaProvider {
    pub fn new(base_url: Option<String>) -> Self {
        Self {
            base_url: base_url.unwrap_or_else(|| "http://localhost:11434".into()),
            http: client(Duration::from_secs(900)),
        }
    }
}

#[async_trait]
impl ModelProvider for OllamaProvider {
    fn kind(&self) -> AiProviderKind {
        AiProviderKind::Ollama
    }

    async fn complete(&self, req: &CompletionRequest) -> Result<CompletionResponse, GatewayError> {
        let mut messages = vec![json!({"role": "system", "content": req.system})];
        messages.extend(
            req.messages
                .iter()
                .map(|m| json!({"role": role(&m.role), "content": m.content})),
        );
        let mut body = json!({"model": req.model, "messages": messages, "stream": false, "options": {"num_predict": req.max_tokens}});
        if let Some(schema) = &req.json_schema {
            body["format"] = schema.clone();
        }
        let v = send_json(
            self.http
                .post(format!("{}/api/chat", self.base_url.trim_end_matches('/')))
                .json(&body),
        )
        .await?;
        Ok(CompletionResponse {
            text: v["message"]["content"].as_str().unwrap_or("").to_string(),
            input_tokens: v["prompt_eval_count"].as_u64().unwrap_or(0) as u32,
            output_tokens: v["eval_count"].as_u64().unwrap_or(0) as u32,
            model: req.model.clone(),
        })
    }
}

// ───────────── Mock (deterministic, for CI and demos) ─────────────

/// Deterministic provider. Responds with a canned, schema-valid document chosen by
/// `purpose`, built from the request so tests can assert on it. Never used unless
/// explicitly configured (provider kind `mock`).
pub struct MockProvider;

#[async_trait]
impl ModelProvider for MockProvider {
    fn kind(&self) -> AiProviderKind {
        AiProviderKind::Mock
    }

    async fn complete(&self, req: &CompletionRequest) -> Result<CompletionResponse, GatewayError> {
        let input = req
            .messages
            .last()
            .map(|m| m.content.clone())
            .unwrap_or_default();
        let text = match req.purpose.as_str() {
            "test_plan" => mock_plan(&input).to_string(),
            "investigation" => json!({
                "summary": "Mock interpretation of the failure evidence.",
                "probable_root_cause": "The deterministic observations point at the first failing step; review its expected and actual values.",
                "category": "unknown",
                "confidence": 0.3,
                "suggested_investigation": ["Open the failing step's screenshot", "Re-run the scenario once to check for flakiness"],
                "suggested_remediation": ["Fix the behaviour or the assertion that disagrees with the requirement"]
            })
            .to_string(),
            "regression" => json!({"rationale": "Mock reasoning: selected scenarios touching the changed modules.", "extra_scenarios": []}).to_string(),
            _ => json!({"text": "mock"}).to_string(),
        };
        let tokens = (input.len() / 4) as u32;
        Ok(CompletionResponse {
            text,
            input_tokens: tokens,
            output_tokens: 200,
            model: req.model.clone(),
        })
    }
}

fn mock_plan(input: &str) -> Value {
    let wants_api = input.to_ascii_lowercase().contains("api");
    let mut scenarios = vec![
        json!({
            "title": "Happy path from the requirement",
            "category": "functional",
            "priority": "high",
            "risk": "Core journey broken for every user",
            "rationale": "Mock planner: covers the primary acceptance criterion.",
            "requirement_keys": [],
            "gherkin": "Feature: Generated\n  Scenario: Home page loads\n    Given I open \"/\"\n    Then I should see \"Northwind\"\n"
        }),
        json!({
            "title": "Rejects invalid credentials",
            "category": "negative",
            "priority": "medium",
            "risk": "Account takeover via weak validation",
            "rationale": "Mock planner: negative path.",
            "requirement_keys": [],
            "gherkin": "Feature: Generated\n  Scenario: Invalid login\n    Given I open \"/login\"\n    When I fill \"Email\" with \"nobody@northwind.test\"\n    And I fill \"Password\" with \"wrong\"\n    And I click \"Sign in\"\n    Then I should see \"Invalid email or password\"\n"
        }),
    ];
    if wants_api {
        scenarios.push(json!({
            "title": "Health endpoint responds",
            "category": "integration",
            "priority": "medium",
            "risk": "Monitoring blind spot",
            "rationale": "Mock planner: API availability.",
            "requirement_keys": [],
            "gherkin": "Feature: Generated\n  @api\n  Scenario: Health\n    When I send a GET request to \"/api/health\"\n    Then the response status should be 200\n"
        }));
    }
    json!({
        "title": "Mock test plan",
        "objective": "Exercise the described behaviour (mock provider; configure a real provider for real planning).",
        "rationale": "Generated by the deterministic mock provider.",
        "risks": [{"area": "Authentication", "level": "high", "reason": "Login gates every journey"}],
        "coverage_gaps": ["No performance scenarios"],
        "scenarios": scenarios
    })
}

/// Build a provider from stored configuration.
pub fn build(
    kind: AiProviderKind,
    api_key: Option<String>,
    base_url: Option<String>,
    api_version: Option<String>,
) -> Result<std::sync::Arc<dyn ModelProvider>, GatewayError> {
    use std::sync::Arc;
    // IP-literal hosts bypass the guarded resolver, so check them here.
    if let (Some(allow_private), Some(b)) = (crate::outbound_guard(), base_url.as_deref()) {
        if let Some(host) = url::Url::parse(b)
            .ok()
            .and_then(|u| u.host_str().map(str::to_string))
        {
            if let Ok(ip) = host.trim_matches(|c| c == '[' || c == ']').parse() {
                oryn_auth::net::check_ip(ip, allow_private)
                    .map_err(|e| GatewayError::NotConfigured(format!("base URL: {e}")))?;
            }
        }
    }
    let need = |k: Option<String>, what: &str| {
        k.filter(|s| !s.is_empty())
            .ok_or_else(|| GatewayError::NotConfigured(format!("{what} API key")))
    };
    Ok(match kind {
        AiProviderKind::Anthropic => Arc::new(AnthropicProvider::new(
            need(api_key, "Anthropic")?,
            base_url,
        )),
        AiProviderKind::Openai => Arc::new(OpenAiProvider::new(
            Some(need(api_key, "OpenAI")?),
            base_url,
            OpenAiFlavor::OpenAi,
        )),
        AiProviderKind::AzureOpenai => Arc::new(OpenAiProvider::new(
            Some(need(api_key, "Azure OpenAI")?),
            Some(
                base_url
                    .ok_or_else(|| GatewayError::NotConfigured("Azure OpenAI endpoint".into()))?,
            ),
            OpenAiFlavor::Azure {
                api_version: api_version.unwrap_or_else(|| "2024-10-21".into()),
            },
        )),
        AiProviderKind::OpenaiCompatible => Arc::new(OpenAiProvider::new(
            api_key,
            Some(base_url.ok_or_else(|| GatewayError::NotConfigured("base URL".into()))?),
            OpenAiFlavor::Compatible,
        )),
        AiProviderKind::Gemini => Arc::new(GeminiProvider::new(need(api_key, "Gemini")?, base_url)),
        AiProviderKind::Ollama => Arc::new(OllamaProvider::new(base_url)),
        AiProviderKind::Mock => Arc::new(MockProvider),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Gateway, Message, Pricing};
    use axum::{routing::post, Json, Router};
    use std::sync::Arc;

    async fn serve(router: Router) -> String {
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let a = l.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(l, router).await.unwrap() });
        format!("http://{a}")
    }

    fn req() -> CompletionRequest {
        CompletionRequest {
            model: "claude-opus-5-5".into(),
            system: "sys".into(),
            messages: vec![Message {
                role: Role::User,
                content: "hi".into(),
            }],
            max_tokens: 64,
            json_schema: Some(
                json!({"type":"object","required":["a"],"properties":{"a":{"type":"integer"}}}),
            ),
            purpose: "t".into(),
        }
    }

    #[tokio::test]
    async fn anthropic_wire_format() {
        let base = serve(Router::new().route(
            "/v1/messages",
            post(|h: axum::http::HeaderMap, Json(b): Json<Value>| async move {
                assert_eq!(h["anthropic-version"], "2026-10-21");
                assert_eq!(h["x-api-key"], "k");
                assert_eq!(b["output_config"]["format"]["type"], "json_schema");
                assert_eq!(b["output_config"]["format"]["schema"]["additionalProperties"], false);
                assert_eq!(b["fallbacks"], "default");
                assert!(b.get("temperature").is_none());
                Json(json!({"model":"claude-opus-5-5","stop_reason":"end_turn","content":[{"type":"thinking","thinking":""},{"type":"text","text":"{\"a\":1}"}],"usage":{"input_tokens":10,"output_tokens":3}}))
            }),
        ))
        .await;
        let g = Gateway::new(
            Arc::new(AnthropicProvider::new("k".into(), Some(base))),
            Pricing::default(),
        );
        let r = g.run(req()).await.unwrap();
        assert_eq!(r.json.unwrap()["a"], 1);
        assert_eq!((r.input_tokens, r.output_tokens), (10, 3));
    }

    #[tokio::test]
    async fn openai_compatible_and_errors() {
        let base = serve(Router::new().route(
            "/chat/completions",
            post(|Json(b): Json<Value>| async move {
                assert_eq!(b["response_format"]["type"], "json_schema");
                Json(json!({"model":"m","choices":[{"message":{"content":"{\"a\":2}"}}],"usage":{"prompt_tokens":5,"completion_tokens":2}}))
            }),
        ))
        .await;
        let p = OpenAiProvider::new(None, Some(base), OpenAiFlavor::Compatible);
        let r = Gateway::new(Arc::new(p), Pricing::default())
            .run(req())
            .await
            .unwrap();
        assert_eq!(r.json.unwrap()["a"], 2);

        let bad = serve(Router::new().route(
            "/chat/completions",
            post(|| async { (axum::http::StatusCode::UNAUTHORIZED, "nope") }),
        ))
        .await;
        let e = Gateway::new(
            Arc::new(OpenAiProvider::new(
                None,
                Some(bad),
                OpenAiFlavor::Compatible,
            )),
            Pricing::default(),
        )
        .run(req())
        .await
        .unwrap_err();
        assert!(matches!(e.0, GatewayError::Rejected { status: 401, .. }));
        assert_eq!(e.1, 1, "4xx not retried");
    }

    #[tokio::test]
    async fn mock_plan_is_valid_json() {
        let mut r = req();
        r.purpose = "test_plan".into();
        r.json_schema = None;
        let out = MockProvider.complete(&r).await.unwrap();
        let v: Value = serde_json::from_str(&out.text).unwrap();
        assert!(v["scenarios"].as_array().unwrap().len() >= 2);
    }
}

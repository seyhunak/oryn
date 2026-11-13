//! Model gateway configuration and usage. ORYN works without any provider; AI adds
//! planning proposals and failure interpretation on top of deterministic features.

use crate::auth::{ws_ctx, Authed};
use crate::error::{ApiError, ApiResult};
use crate::routes::{ok, Ok};
use crate::state::AppState;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::Json;
use oryn_db::ai::{AiModel, AiProvider, UsageRow};
use oryn_db::governance::audit;
use oryn_domain::{AiProviderKind, Permission};
use oryn_model_gateway::{CompletionRequest, Message, Role};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::time::Duration;
use utoipa::{IntoParams, ToSchema};
use utoipa_axum::{router::OpenApiRouter, routes};
use uuid::Uuid;

pub fn routes() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(list_providers, create_provider))
        .routes(routes!(update_provider, delete_provider))
        .routes(routes!(add_model))
        .routes(routes!(delete_model))
        .routes(routes!(test_provider))
        .routes(routes!(usage))
}

#[derive(Serialize, ToSchema)]
pub struct ProviderView {
    #[serde(flatten)]
    pub provider: AiProvider,
    pub has_api_key: bool,
    pub models: Vec<AiModel>,
}

async fn view(s: &AppState, p: AiProvider) -> ApiResult<ProviderView> {
    let models = oryn_db::ai::list_models(&s.pool, p.id).await?;
    Ok(ProviderView {
        has_api_key: p.api_key_encrypted.is_some(),
        models,
        provider: p,
    })
}

#[utoipa::path(get, path = "/api/v1/workspaces/{ws}/ai/providers", tag = "ai", responses((status = 200, body = Vec<ProviderView>)))]
async fn list_providers(
    State(s): State<AppState>,
    auth: Authed,
    Path(ws): Path<Uuid>,
) -> ApiResult<Json<Vec<ProviderView>>> {
    ws_ctx(&s, &auth, ws)
        .await?
        .require(Permission::WorkspaceRead)?;
    let mut out = vec![];
    for p in oryn_db::ai::list_providers(&s.pool, ws).await? {
        out.push(view(&s, p).await?);
    }
    Ok(Json(out))
}

#[derive(Deserialize, ToSchema, Clone)]
pub struct ModelReq {
    pub name: String,
    pub context_window: Option<i32>,
    /// USD per million input tokens.
    #[serde(default)]
    pub input_cost_per_mtok: f64,
    /// USD per million output tokens.
    #[serde(default)]
    pub output_cost_per_mtok: f64,
    #[serde(default)]
    pub is_default: bool,
}

#[derive(Deserialize, ToSchema)]
pub struct CreateProvider {
    pub kind: AiProviderKind,
    pub name: String,
    /// Required for azure_openai (resource endpoint) and openai_compatible; optional otherwise.
    pub base_url: Option<String>,
    /// Encrypted at rest; never returned.
    pub api_key: Option<String>,
    /// Azure OpenAI API version.
    pub api_version: Option<String>,
    #[serde(default)]
    pub is_default: bool,
    /// Models with prices. Anthropic and Mock get sensible defaults when empty.
    #[serde(default)]
    pub models: Vec<ModelReq>,
}

fn default_models(kind: AiProviderKind) -> Vec<ModelReq> {
    let m = |name: &str, i: f64, o: f64, ctx: i32, d: bool| ModelReq {
        name: name.into(),
        context_window: Some(ctx),
        input_cost_per_mtok: i,
        output_cost_per_mtok: o,
        is_default: d,
    };
    match kind {
        AiProviderKind::Anthropic => vec![
            m("claude-opus-5-5", 4.0, 20.0, 1_000_000, true),
            m("claude-sonnet-5-5", 2.0, 10.0, 1_000_000, false),
            m("claude-haiku-5-5", 0.10, 0.50, 200_000, false),
        ],
        AiProviderKind::Mock => vec![m("mock-1", 0.0, 0.0, 100_000, true)],
        _ => vec![],
    }
}

async fn check_base_url(
    kind: AiProviderKind,
    base: &Option<String>,
    allow_private: bool,
) -> ApiResult<()> {
    let needs = matches!(
        kind,
        AiProviderKind::AzureOpenai | AiProviderKind::OpenaiCompatible
    );
    match base.as_deref().filter(|b| !b.is_empty()) {
        None if needs => Err(ApiError::bad("base_url is required for this provider")),
        None => Ok(()),
        Some(b) => {
            let u = url::Url::parse(b).map_err(|_| ApiError::bad("base_url is not a valid URL"))?;
            if u.scheme() != "https" && u.scheme() != "http" {
                return Err(ApiError::bad("base_url must be http or https"));
            }
            // Resolve and check now for a clear error; requests are checked again and
            // pinned by the gateway's guarded client.
            oryn_auth::net::check_url_host(&u, allow_private)
                .await
                .map_err(|e| ApiError::bad(format!("base_url: {e}")))?;
            Ok(())
        }
    }
}

#[utoipa::path(post, path = "/api/v1/workspaces/{ws}/ai/providers", tag = "ai", responses((status = 201, body = ProviderView)))]
async fn create_provider(
    State(s): State<AppState>,
    auth: Authed,
    Path(ws): Path<Uuid>,
    Json(req): Json<CreateProvider>,
) -> ApiResult<(StatusCode, Json<ProviderView>)> {
    let c = ws_ctx(&s, &auth, ws).await?;
    c.require(Permission::AiConfigure)?;
    check_base_url(req.kind, &req.base_url, s.cfg.allow_private_network).await?;
    let needs_key = matches!(
        req.kind,
        AiProviderKind::Anthropic
            | AiProviderKind::Openai
            | AiProviderKind::AzureOpenai
            | AiProviderKind::Gemini
    );
    if needs_key && req.api_key.as_deref().unwrap_or("").is_empty() {
        return Err(ApiError::bad("an API key is required for this provider"));
    }
    let models = if req.models.is_empty() {
        default_models(req.kind)
    } else {
        req.models.clone()
    };
    if models.is_empty() {
        return Err(ApiError::bad(
            "add at least one model (name and prices) for this provider",
        ));
    }
    let key = req.api_key.filter(|k| !k.is_empty()).map(|k| s.seal(&k));
    let p = oryn_db::ai::create_provider(
        &s.pool,
        ws,
        req.kind.as_str(),
        req.name.trim(),
        req.base_url.as_deref().filter(|b| !b.is_empty()),
        key.as_deref(),
        req.api_version.as_deref(),
        req.is_default,
        c.user_id(),
    )
    .await?;
    let any_default = models.iter().any(|m| m.is_default);
    for (i, m) in models.iter().enumerate() {
        oryn_db::ai::upsert_model(
            &s.pool,
            p.id,
            m.name.trim(),
            m.context_window,
            m.input_cost_per_mtok.max(0.0),
            m.output_cost_per_mtok.max(0.0),
            m.is_default || (!any_default && i == 0),
        )
        .await?;
    }
    audit(
        &s.pool,
        Some(ws),
        &c.actor(),
        "ai.provider_created",
        Some("ai_provider"),
        Some(p.id),
        json!({"kind": p.kind, "name": p.name}),
    )
    .await;
    Ok((StatusCode::CREATED, Json(view(&s, p).await?)))
}

#[derive(Deserialize, ToSchema)]
pub struct UpdateProvider {
    pub enabled: Option<bool>,
    pub is_default: Option<bool>,
    pub base_url: Option<String>,
    /// A new key replaces the stored one.
    pub api_key: Option<String>,
}

#[utoipa::path(patch, path = "/api/v1/workspaces/{ws}/ai/providers/{pid}", tag = "ai", responses((status = 200, body = ProviderView)))]
async fn update_provider(
    State(s): State<AppState>,
    auth: Authed,
    Path((ws, pid)): Path<(Uuid, Uuid)>,
    Json(req): Json<UpdateProvider>,
) -> ApiResult<Json<ProviderView>> {
    let c = ws_ctx(&s, &auth, ws).await?;
    c.require(Permission::AiConfigure)?;
    let existing = oryn_db::ai::get_provider(&s.pool, ws, pid).await?;
    let kind: AiProviderKind = existing
        .kind
        .parse()
        .map_err(|_| ApiError::bad("unknown provider kind"))?;
    if req.base_url.is_some() {
        check_base_url(kind, &req.base_url, s.cfg.allow_private_network).await?;
    }
    let key = req.api_key.filter(|k| !k.is_empty()).map(|k| s.seal(&k));
    let p = oryn_db::ai::update_provider(
        &s.pool,
        ws,
        pid,
        req.enabled,
        req.is_default,
        req.base_url.as_deref(),
        key.as_deref(),
    )
    .await?;
    audit(
        &s.pool,
        Some(ws),
        &c.actor(),
        "ai.provider_updated",
        Some("ai_provider"),
        Some(pid),
        json!({"enabled": req.enabled, "is_default": req.is_default, "key_rotated": key.is_some()}),
    )
    .await;
    Ok(Json(view(&s, p).await?))
}

#[utoipa::path(delete, path = "/api/v1/workspaces/{ws}/ai/providers/{pid}", tag = "ai", responses((status = 200, body = Ok)))]
async fn delete_provider(
    State(s): State<AppState>,
    auth: Authed,
    Path((ws, pid)): Path<(Uuid, Uuid)>,
) -> ApiResult<Json<Ok>> {
    let c = ws_ctx(&s, &auth, ws).await?;
    c.require(Permission::AiConfigure)?;
    oryn_db::ai::delete_provider(&s.pool, ws, pid).await?;
    audit(
        &s.pool,
        Some(ws),
        &c.actor(),
        "ai.provider_deleted",
        Some("ai_provider"),
        Some(pid),
        json!({}),
    )
    .await;
    Ok(ok())
}

#[utoipa::path(post, path = "/api/v1/workspaces/{ws}/ai/providers/{pid}/models", tag = "ai", responses((status = 201, body = AiModel)))]
async fn add_model(
    State(s): State<AppState>,
    auth: Authed,
    Path((ws, pid)): Path<(Uuid, Uuid)>,
    Json(m): Json<ModelReq>,
) -> ApiResult<(StatusCode, Json<AiModel>)> {
    ws_ctx(&s, &auth, ws)
        .await?
        .require(Permission::AiConfigure)?;
    oryn_db::ai::get_provider(&s.pool, ws, pid).await?;
    if m.name.trim().is_empty() {
        return Err(ApiError::bad("model name is required"));
    }
    Ok((
        StatusCode::CREATED,
        Json(
            oryn_db::ai::upsert_model(
                &s.pool,
                pid,
                m.name.trim(),
                m.context_window,
                m.input_cost_per_mtok.max(0.0),
                m.output_cost_per_mtok.max(0.0),
                m.is_default,
            )
            .await?,
        ),
    ))
}

#[utoipa::path(delete, path = "/api/v1/workspaces/{ws}/ai/providers/{pid}/models/{mid}", tag = "ai", responses((status = 200, body = Ok)))]
async fn delete_model(
    State(s): State<AppState>,
    auth: Authed,
    Path((ws, pid, mid)): Path<(Uuid, Uuid, Uuid)>,
) -> ApiResult<Json<Ok>> {
    ws_ctx(&s, &auth, ws)
        .await?
        .require(Permission::AiConfigure)?;
    oryn_db::ai::get_provider(&s.pool, ws, pid).await?;
    oryn_db::ai::delete_model(&s.pool, pid, mid).await?;
    Ok(ok())
}

#[derive(Serialize, ToSchema)]
pub struct TestResult {
    pub ok: bool,
    pub model: String,
    pub latency_ms: u64,
    pub input_tokens: u32,
    pub output_tokens: u32,
    pub cost_usd: f64,
    pub message: String,
}

/// Send a tiny structured request to verify the key, model and JSON output support.
#[utoipa::path(post, path = "/api/v1/workspaces/{ws}/ai/providers/{pid}/test", tag = "ai", responses((status = 200, body = TestResult)))]
async fn test_provider(
    State(s): State<AppState>,
    auth: Authed,
    Path((ws, pid)): Path<(Uuid, Uuid)>,
) -> ApiResult<Json<TestResult>> {
    let c = ws_ctx(&s, &auth, ws).await?;
    c.require(Permission::AiConfigure)?;
    s.limiter
        .check(&format!("ai-test:{ws}"), 20, Duration::from_secs(3600))
        .await?;
    let p = oryn_db::ai::get_provider(&s.pool, ws, pid).await?;
    let h = s.ai_provider(&p).await?;
    let req = CompletionRequest {
        model: h.model.clone(),
        system: "You are a connectivity check. Reply only with the requested JSON.".into(),
        messages: vec![Message {
            role: Role::User,
            content: "Return {\"ok\": true}.".into(),
        }],
        max_tokens: 1024,
        json_schema: Some(
            json!({"type": "object", "required": ["ok"], "properties": {"ok": {"type": "boolean"}}}),
        ),
        purpose: "connection_test".into(),
    };
    match s.ai_call(ws, None, Some(c.user_id()), &h, req).await {
        Result::Ok((r, _)) => Ok(Json(TestResult {
            ok: true,
            model: r.model,
            latency_ms: r.latency_ms,
            input_tokens: r.input_tokens,
            output_tokens: r.output_tokens,
            cost_usd: r.cost_usd,
            message: "The provider answered with valid structured output.".into(),
        })),
        Err(e) => Ok(Json(TestResult {
            ok: false,
            model: h.model,
            latency_ms: 0,
            input_tokens: 0,
            output_tokens: 0,
            cost_usd: 0.0,
            message: e.to_string(),
        })),
    }
}

#[derive(Deserialize, IntoParams)]
pub struct UsageQuery {
    pub days: Option<i32>,
}

#[derive(Serialize, ToSchema)]
pub struct Usage {
    pub days: i32,
    pub rows: Vec<UsageRow>,
    pub total_requests: i64,
    pub total_cost_usd: f64,
    pub total_input_tokens: i64,
    pub total_output_tokens: i64,
}

#[utoipa::path(get, path = "/api/v1/workspaces/{ws}/ai/usage", tag = "ai", params(UsageQuery), responses((status = 200, body = Usage)))]
async fn usage(
    State(s): State<AppState>,
    auth: Authed,
    Path(ws): Path<Uuid>,
    Query(q): Query<UsageQuery>,
) -> ApiResult<Json<Usage>> {
    ws_ctx(&s, &auth, ws)
        .await?
        .require(Permission::WorkspaceRead)?;
    let days = q.days.unwrap_or(30).clamp(1, 365);
    let rows = oryn_db::ai::usage_summary(&s.pool, ws, days).await?;
    Ok(Json(Usage {
        days,
        total_requests: rows.iter().map(|r| r.requests).sum(),
        total_cost_usd: rows.iter().map(|r| r.cost_usd).sum(),
        total_input_tokens: rows.iter().map(|r| r.input_tokens).sum(),
        total_output_tokens: rows.iter().map(|r| r.output_tokens).sum(),
        rows,
    }))
}

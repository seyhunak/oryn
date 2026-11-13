//! Outbound integrations: signed webhooks and Slack incoming webhooks.

use crate::auth::{ws_ctx, Authed};
use crate::error::{ApiError, ApiResult};
use crate::routes::{ok, Ok};
use crate::services::notify::{check_outbound, deliver};
use crate::state::AppState;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use oryn_db::governance::{audit, Delivery, Integration};
use oryn_domain::Permission;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::time::Instant;
use utoipa::ToSchema;
use utoipa_axum::{router::OpenApiRouter, routes};
use uuid::Uuid;

pub fn routes() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(list, create))
        .routes(routes!(update, remove))
        .routes(routes!(test))
        .routes(routes!(deliveries))
}

const EVENTS: &[&str] = &["execution.finished", "execution.failed"];

#[utoipa::path(get, path = "/api/v1/workspaces/{ws}/integrations", tag = "integrations", responses((status = 200, body = Vec<Integration>)))]
async fn list(
    State(s): State<AppState>,
    auth: Authed,
    Path(ws): Path<Uuid>,
) -> ApiResult<Json<Vec<Integration>>> {
    ws_ctx(&s, &auth, ws)
        .await?
        .require(Permission::WorkspaceRead)?;
    Ok(Json(
        oryn_db::governance::list_integrations(&s.pool, ws).await?,
    ))
}

#[derive(Deserialize, ToSchema)]
pub struct CreateIntegration {
    /// `webhook` or `slack`.
    pub kind: String,
    pub name: String,
    /// Limit to one project; all projects when omitted.
    pub project_id: Option<Uuid>,
    /// Webhook endpoint (https).
    pub url: Option<String>,
    /// Webhook: HMAC-SHA256 signing secret (sent as `x-oryn-signature: sha256=…`).
    /// Slack: the incoming-webhook URL. Encrypted at rest.
    pub secret: Option<String>,
    /// `execution.finished`, `execution.failed`.
    pub events: Vec<String>,
}

fn check_events(ev: &[String]) -> ApiResult<()> {
    if ev.is_empty() || ev.iter().any(|e| !EVENTS.contains(&e.as_str())) {
        return Err(ApiError::bad(format!(
            "events must be one or more of {}",
            EVENTS.join(", ")
        )));
    }
    Ok(())
}

#[utoipa::path(post, path = "/api/v1/workspaces/{ws}/integrations", tag = "integrations", responses((status = 201, body = Integration)))]
async fn create(
    State(s): State<AppState>,
    auth: Authed,
    Path(ws): Path<Uuid>,
    Json(req): Json<CreateIntegration>,
) -> ApiResult<(StatusCode, Json<Integration>)> {
    let c = ws_ctx(&s, &auth, ws).await?;
    c.require(Permission::IntegrationManage)?;
    check_events(&req.events)?;
    let config = match req.kind.as_str() {
        "webhook" => {
            let url = req
                .url
                .clone()
                .ok_or_else(|| ApiError::bad("url is required for a webhook"))?;
            check_outbound(&url, s.cfg.allow_private_network)
                .await
                .map_err(ApiError::bad)?;
            json!({"url": url})
        }
        "slack" => {
            let hook = req.secret.clone().ok_or_else(|| {
                ApiError::bad("the Slack incoming-webhook URL is required (as secret)")
            })?;
            let u = check_outbound(&hook, s.cfg.allow_private_network)
                .await
                .map_err(ApiError::bad)?;
            if u.host_str() != Some("hooks.slack.com") && !s.cfg.allow_private_network {
                return Err(ApiError::bad(
                    "Slack webhooks must point at hooks.slack.com",
                ));
            }
            json!({})
        }
        other => {
            return Err(ApiError::unprocessable(format!(
                "{other} integrations are not implemented yet; use a webhook"
            )))
        }
    };
    if let Some(pid) = req.project_id {
        let p = oryn_db::projects::get_project(&s.pool, pid).await?;
        if p.workspace_id != ws {
            return Err(ApiError::not_found("project"));
        }
    }
    let secret = req.secret.filter(|x| !x.is_empty()).map(|x| s.seal(&x));
    let id = oryn_db::governance::create_integration(
        &s.pool,
        ws,
        req.project_id,
        &req.kind,
        req.name.trim(),
        &config,
        secret.as_deref(),
        &req.events,
        c.user_id(),
    )
    .await?;
    audit(
        &s.pool,
        Some(ws),
        &c.actor(),
        "integration.created",
        Some("integration"),
        Some(id),
        json!({"kind": req.kind, "events": req.events}),
    )
    .await;
    Ok((
        StatusCode::CREATED,
        Json(oryn_db::governance::get_integration(&s.pool, ws, id).await?),
    ))
}

#[derive(Deserialize, ToSchema)]
pub struct UpdateIntegration {
    pub enabled: Option<bool>,
    pub events: Option<Vec<String>>,
    pub url: Option<String>,
}

#[utoipa::path(patch, path = "/api/v1/workspaces/{ws}/integrations/{id}", tag = "integrations", responses((status = 200, body = Integration)))]
async fn update(
    State(s): State<AppState>,
    auth: Authed,
    Path((ws, id)): Path<(Uuid, Uuid)>,
    Json(req): Json<UpdateIntegration>,
) -> ApiResult<Json<Integration>> {
    let c = ws_ctx(&s, &auth, ws).await?;
    c.require(Permission::IntegrationManage)?;
    let i = oryn_db::governance::get_integration(&s.pool, ws, id).await?;
    if let Some(ev) = &req.events {
        check_events(ev)?;
    }
    let config = match (&req.url, i.kind.as_str()) {
        (Some(u), "webhook") => {
            check_outbound(u, s.cfg.allow_private_network)
                .await
                .map_err(ApiError::bad)?;
            Some(json!({"url": u}))
        }
        (Some(_), _) => return Err(ApiError::bad("only webhooks have a url")),
        _ => None,
    };
    oryn_db::governance::update_integration(
        &s.pool,
        ws,
        id,
        req.enabled,
        config.as_ref(),
        req.events.as_deref(),
    )
    .await?;
    audit(
        &s.pool,
        Some(ws),
        &c.actor(),
        "integration.updated",
        Some("integration"),
        Some(id),
        json!({"enabled": req.enabled}),
    )
    .await;
    Ok(Json(
        oryn_db::governance::get_integration(&s.pool, ws, id).await?,
    ))
}

#[utoipa::path(delete, path = "/api/v1/workspaces/{ws}/integrations/{id}", tag = "integrations", responses((status = 200, body = Ok)))]
async fn remove(
    State(s): State<AppState>,
    auth: Authed,
    Path((ws, id)): Path<(Uuid, Uuid)>,
) -> ApiResult<Json<Ok>> {
    let c = ws_ctx(&s, &auth, ws).await?;
    c.require(Permission::IntegrationManage)?;
    oryn_db::governance::delete_integration(&s.pool, ws, id).await?;
    audit(
        &s.pool,
        Some(ws),
        &c.actor(),
        "integration.deleted",
        Some("integration"),
        Some(id),
        json!({}),
    )
    .await;
    Ok(ok())
}

#[derive(Serialize, ToSchema)]
pub struct TestDelivery {
    pub ok: bool,
    pub status: Option<i32>,
    pub error: Option<String>,
}

#[utoipa::path(post, path = "/api/v1/workspaces/{ws}/integrations/{id}/test", tag = "integrations", responses((status = 200, body = TestDelivery)))]
async fn test(
    State(s): State<AppState>,
    auth: Authed,
    Path((ws, id)): Path<(Uuid, Uuid)>,
) -> ApiResult<Json<TestDelivery>> {
    ws_ctx(&s, &auth, ws)
        .await?
        .require(Permission::IntegrationManage)?;
    let i = oryn_db::governance::get_integration(&s.pool, ws, id).await?;
    let payload = json!({"event": "integration.test", "execution": {"title": "Test delivery from ORYN", "status": "passed", "environment_name": "-", "passed": 0, "failed": 0, "errored": 0, "total": 0}, "url": s.cfg.public_url});
    let t = Instant::now();
    let (status, error) = deliver(&s, &i, "integration.test", &payload).await;
    oryn_db::governance::record_delivery(
        &s.pool,
        id,
        "integration.test",
        status,
        error.as_deref(),
        t.elapsed().as_millis() as i64,
    )
    .await?;
    Ok(Json(TestDelivery {
        ok: error.is_none(),
        status,
        error,
    }))
}

#[utoipa::path(get, path = "/api/v1/workspaces/{ws}/integrations/{id}/deliveries", tag = "integrations", responses((status = 200, body = Vec<Delivery>)))]
async fn deliveries(
    State(s): State<AppState>,
    auth: Authed,
    Path((ws, id)): Path<(Uuid, Uuid)>,
) -> ApiResult<Json<Vec<Delivery>>> {
    ws_ctx(&s, &auth, ws)
        .await?
        .require(Permission::WorkspaceRead)?;
    oryn_db::governance::get_integration(&s.pool, ws, id).await?;
    Ok(Json(
        oryn_db::governance::list_deliveries(&s.pool, id).await?,
    ))
}

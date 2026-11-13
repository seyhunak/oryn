//! Audit events, approvals and integrations.

use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::Value;
use sqlx::{FromRow, PgPool};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::{new_id, opt, DbResult, Page};

#[derive(Debug, Clone)]
pub struct Actor {
    pub kind: &'static str,
    pub id: Option<Uuid>,
    pub label: String,
    pub ip: Option<String>,
}

#[derive(Debug, Clone, FromRow, Serialize, ToSchema)]
pub struct AuditEvent {
    pub id: i64,
    pub workspace_id: Option<Uuid>,
    pub actor_kind: String,
    pub actor_id: Option<Uuid>,
    pub actor_label: Option<String>,
    pub action: String,
    pub subject_type: Option<String>,
    pub subject_id: Option<Uuid>,
    #[schema(value_type = Object)]
    pub data: Value,
    pub ip: Option<String>,
    pub created_at: DateTime<Utc>,
}

pub async fn audit(
    pool: &PgPool,
    ws: Option<Uuid>,
    actor: &Actor,
    action: &str,
    subject_type: Option<&str>,
    subject_id: Option<Uuid>,
    data: Value,
) {
    let r = sqlx::query(
        "INSERT INTO audit_events (workspace_id, actor_kind, actor_id, actor_label, action, subject_type, subject_id, data, ip) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9)",
    )
    .bind(ws)
    .bind(actor.kind)
    .bind(actor.id)
    .bind(&actor.label)
    .bind(action)
    .bind(subject_type)
    .bind(subject_id)
    .bind(data)
    .bind(&actor.ip)
    .execute(pool)
    .await;
    if let Err(e) = r {
        tracing::error!(error = %e, action, "failed to write audit event");
    }
}

pub async fn list_audit(
    pool: &PgPool,
    ws: Uuid,
    action_prefix: Option<&str>,
    page: Page,
) -> DbResult<(Vec<AuditEvent>, i64)> {
    let rows = sqlx::query_as::<_, AuditEvent>(
        "SELECT id, workspace_id, actor_kind, actor_id, actor_label, action, subject_type, subject_id, data, ip, created_at FROM audit_events
         WHERE workspace_id = $1 AND ($2::text IS NULL OR action LIKE $2 || '%') ORDER BY id DESC LIMIT $3 OFFSET $4",
    )
    .bind(ws)
    .bind(action_prefix)
    .bind(page.limit)
    .bind(page.offset)
    .fetch_all(pool)
    .await?;
    let total: i64 = sqlx::query_scalar("SELECT count(*) FROM audit_events WHERE workspace_id = $1 AND ($2::text IS NULL OR action LIKE $2 || '%')")
        .bind(ws)
        .bind(action_prefix)
        .fetch_one(pool)
        .await?;
    Ok((rows, total))
}

// ── approvals ──
#[derive(Debug, Clone, FromRow, Serialize, ToSchema)]
pub struct Approval {
    pub id: Uuid,
    pub workspace_id: Uuid,
    pub project_id: Option<Uuid>,
    pub subject_type: String,
    pub subject_id: Uuid,
    pub status: String,
    pub requested_by: Option<Uuid>,
    pub decided_by: Option<Uuid>,
    pub decided_at: Option<DateTime<Utc>>,
    pub comment: Option<String>,
    pub created_at: DateTime<Utc>,
}

#[allow(clippy::too_many_arguments)]
pub async fn record_approval(
    pool: &PgPool,
    ws: Uuid,
    project: Option<Uuid>,
    subject_type: &str,
    subject_id: Uuid,
    status: &str,
    user: Uuid,
    comment: Option<&str>,
) -> DbResult<()> {
    sqlx::query(
        "INSERT INTO approvals (id, workspace_id, project_id, subject_type, subject_id, status, requested_by, decided_by, decided_at, comment)
         VALUES ($1,$2,$3,$4,$5,$6,$7,$7, now(), $8)",
    )
    .bind(new_id())
    .bind(ws)
    .bind(project)
    .bind(subject_type)
    .bind(subject_id)
    .bind(status)
    .bind(user)
    .bind(comment)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn approvals_for(pool: &PgPool, subject_id: Uuid) -> DbResult<Vec<Approval>> {
    Ok(sqlx::query_as::<_, Approval>(
        "SELECT id, workspace_id, project_id, subject_type, subject_id, status, requested_by, decided_by, decided_at, comment, created_at
         FROM approvals WHERE subject_id = $1 ORDER BY created_at DESC",
    )
    .bind(subject_id)
    .fetch_all(pool)
    .await?)
}

// ── integrations ──
#[derive(Debug, Clone, FromRow, Serialize, ToSchema)]
pub struct Integration {
    pub id: Uuid,
    pub workspace_id: Uuid,
    pub project_id: Option<Uuid>,
    pub kind: String,
    pub name: String,
    #[schema(value_type = Object)]
    pub config: Value,
    #[serde(skip)]
    pub secret_encrypted: Option<String>,
    pub has_secret: bool,
    pub events: Vec<String>,
    pub enabled: bool,
    pub created_at: DateTime<Utc>,
    pub last_delivery_status: Option<i32>,
    pub last_delivery_at: Option<DateTime<Utc>>,
}

const INT_SELECT: &str = "SELECT i.id, i.workspace_id, i.project_id, i.kind, i.name, i.config, i.secret_encrypted, (i.secret_encrypted IS NOT NULL) AS has_secret,
  i.events, i.enabled, i.created_at, d.status_code AS last_delivery_status, d.created_at AS last_delivery_at
  FROM integrations i LEFT JOIN LATERAL (SELECT status_code, created_at FROM integration_deliveries x WHERE x.integration_id = i.id ORDER BY created_at DESC LIMIT 1) d ON true";

pub async fn list_integrations(pool: &PgPool, ws: Uuid) -> DbResult<Vec<Integration>> {
    Ok(sqlx::query_as::<_, Integration>(&format!(
        "{INT_SELECT} WHERE i.workspace_id = $1 ORDER BY i.kind, i.name"
    ))
    .bind(ws)
    .fetch_all(pool)
    .await?)
}

pub async fn get_integration(pool: &PgPool, ws: Uuid, id: Uuid) -> DbResult<Integration> {
    opt(sqlx::query_as::<_, Integration>(&format!(
        "{INT_SELECT} WHERE i.workspace_id = $1 AND i.id = $2"
    ))
    .bind(ws)
    .bind(id)
    .fetch_one(pool)
    .await)
}

/// Enabled integrations subscribed to an event for a project (or workspace-wide).
pub async fn integrations_for_event(
    pool: &PgPool,
    ws: Uuid,
    project: Uuid,
    event: &str,
) -> DbResult<Vec<Integration>> {
    Ok(sqlx::query_as::<_, Integration>(&format!(
        "{INT_SELECT} WHERE i.workspace_id = $1 AND i.enabled AND (i.project_id IS NULL OR i.project_id = $2) AND $3 = ANY(i.events)"
    ))
    .bind(ws)
    .bind(project)
    .bind(event)
    .fetch_all(pool)
    .await?)
}

#[allow(clippy::too_many_arguments)]
pub async fn create_integration(
    pool: &PgPool,
    ws: Uuid,
    project: Option<Uuid>,
    kind: &str,
    name: &str,
    config: &Value,
    secret_enc: Option<&str>,
    events: &[String],
    user: Uuid,
) -> DbResult<Uuid> {
    let id = new_id();
    sqlx::query(
        "INSERT INTO integrations (id, workspace_id, project_id, kind, name, config, secret_encrypted, events, created_by) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9)",
    )
    .bind(id)
    .bind(ws)
    .bind(project)
    .bind(kind)
    .bind(name)
    .bind(config)
    .bind(secret_enc)
    .bind(events)
    .bind(user)
    .execute(pool)
    .await?;
    Ok(id)
}

pub async fn update_integration(
    pool: &PgPool,
    ws: Uuid,
    id: Uuid,
    enabled: Option<bool>,
    config: Option<&Value>,
    events: Option<&[String]>,
) -> DbResult<()> {
    sqlx::query(
        "UPDATE integrations SET enabled = COALESCE($3, enabled), config = COALESCE($4, config), events = COALESCE($5, events) WHERE workspace_id = $1 AND id = $2",
    )
    .bind(ws)
    .bind(id)
    .bind(enabled)
    .bind(config)
    .bind(events)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn delete_integration(pool: &PgPool, ws: Uuid, id: Uuid) -> DbResult<()> {
    sqlx::query("DELETE FROM integrations WHERE workspace_id = $1 AND id = $2")
        .bind(ws)
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn record_delivery(
    pool: &PgPool,
    integration: Uuid,
    event: &str,
    status: Option<i32>,
    error: Option<&str>,
    duration_ms: i64,
) -> DbResult<()> {
    sqlx::query("INSERT INTO integration_deliveries (id, integration_id, event, status_code, error, duration_ms) VALUES ($1,$2,$3,$4,$5,$6)")
        .bind(new_id())
        .bind(integration)
        .bind(event)
        .bind(status)
        .bind(error)
        .bind(duration_ms)
        .execute(pool)
        .await?;
    Ok(())
}

#[derive(Debug, Clone, FromRow, Serialize, ToSchema)]
pub struct Delivery {
    pub id: Uuid,
    pub event: String,
    pub status_code: Option<i32>,
    pub error: Option<String>,
    pub duration_ms: Option<i64>,
    pub created_at: DateTime<Utc>,
}

pub async fn list_deliveries(pool: &PgPool, integration: Uuid) -> DbResult<Vec<Delivery>> {
    Ok(sqlx::query_as::<_, Delivery>(
        "SELECT id, event, status_code, error, duration_ms, created_at FROM integration_deliveries WHERE integration_id = $1 ORDER BY created_at DESC LIMIT 50",
    )
    .bind(integration)
    .fetch_all(pool)
    .await?)
}

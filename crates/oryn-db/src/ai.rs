//! AI providers, models, request/usage accounting, investigations and recommendations.

use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::Value;
use sqlx::{FromRow, PgPool};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::{new_id, opt, DbError, DbResult};

#[derive(Debug, Clone, FromRow, Serialize, ToSchema)]
pub struct AiProvider {
    pub id: Uuid,
    pub workspace_id: Uuid,
    pub kind: String,
    pub name: String,
    pub base_url: Option<String>,
    #[serde(skip)]
    pub api_key_encrypted: Option<String>,
    pub api_version: Option<String>,
    pub enabled: bool,
    pub is_default: bool,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, FromRow, Serialize, ToSchema)]
pub struct AiModel {
    pub id: Uuid,
    pub provider_id: Uuid,
    pub name: String,
    pub context_window: Option<i32>,
    pub input_cost_per_mtok: f64,
    pub output_cost_per_mtok: f64,
    pub supports_json: bool,
    pub is_default: bool,
}

const PROVIDER_COLS: &str = "id, workspace_id, kind, name, base_url, api_key_encrypted, api_version, enabled, is_default, created_at";
const MODEL_COLS: &str = "id, provider_id, name, context_window, input_cost_per_mtok::float8 AS input_cost_per_mtok, output_cost_per_mtok::float8 AS output_cost_per_mtok, supports_json, is_default";

pub async fn list_providers(pool: &PgPool, ws: Uuid) -> DbResult<Vec<AiProvider>> {
    Ok(sqlx::query_as::<_, AiProvider>(&format!("SELECT {PROVIDER_COLS} FROM ai_providers WHERE workspace_id = $1 ORDER BY is_default DESC, name"))
        .bind(ws)
        .fetch_all(pool)
        .await?)
}

pub async fn get_provider(pool: &PgPool, ws: Uuid, id: Uuid) -> DbResult<AiProvider> {
    opt(sqlx::query_as::<_, AiProvider>(&format!(
        "SELECT {PROVIDER_COLS} FROM ai_providers WHERE workspace_id = $1 AND id = $2"
    ))
    .bind(ws)
    .bind(id)
    .fetch_one(pool)
    .await)
}

pub async fn default_provider(pool: &PgPool, ws: Uuid) -> DbResult<Option<AiProvider>> {
    Ok(sqlx::query_as::<_, AiProvider>(&format!(
        "SELECT {PROVIDER_COLS} FROM ai_providers WHERE workspace_id = $1 AND enabled ORDER BY is_default DESC, created_at LIMIT 1"
    ))
    .bind(ws)
    .fetch_optional(pool)
    .await?)
}

#[allow(clippy::too_many_arguments)]
pub async fn create_provider(
    pool: &PgPool,
    ws: Uuid,
    kind: &str,
    name: &str,
    base_url: Option<&str>,
    key_enc: Option<&str>,
    api_version: Option<&str>,
    is_default: bool,
    user: Uuid,
) -> DbResult<AiProvider> {
    let mut tx = pool.begin().await?;
    if is_default {
        sqlx::query("UPDATE ai_providers SET is_default = false WHERE workspace_id = $1")
            .bind(ws)
            .execute(&mut *tx)
            .await?;
    }
    let p = sqlx::query_as::<_, AiProvider>(&format!(
        "INSERT INTO ai_providers (id, workspace_id, kind, name, base_url, api_key_encrypted, api_version, is_default, created_by)
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9) RETURNING {PROVIDER_COLS}"
    ))
    .bind(new_id())
    .bind(ws)
    .bind(kind)
    .bind(name)
    .bind(base_url)
    .bind(key_enc)
    .bind(api_version)
    .bind(is_default)
    .bind(user)
    .fetch_one(&mut *tx)
    .await
    .map_err(|e| DbError::from_sqlx(e, "provider"))?;
    tx.commit().await?;
    Ok(p)
}

pub async fn update_provider(
    pool: &PgPool,
    ws: Uuid,
    id: Uuid,
    enabled: Option<bool>,
    is_default: Option<bool>,
    base_url: Option<&str>,
    key_enc: Option<&str>,
) -> DbResult<AiProvider> {
    let mut tx = pool.begin().await?;
    if is_default == Some(true) {
        sqlx::query("UPDATE ai_providers SET is_default = false WHERE workspace_id = $1")
            .bind(ws)
            .execute(&mut *tx)
            .await?;
    }
    let p = opt(sqlx::query_as::<_, AiProvider>(&format!(
        "UPDATE ai_providers SET enabled = COALESCE($3, enabled), is_default = COALESCE($4, is_default), base_url = COALESCE($5, base_url),
         api_key_encrypted = COALESCE($6, api_key_encrypted) WHERE workspace_id = $1 AND id = $2 RETURNING {PROVIDER_COLS}"
    ))
    .bind(ws)
    .bind(id)
    .bind(enabled)
    .bind(is_default)
    .bind(base_url)
    .bind(key_enc)
    .fetch_one(&mut *tx)
    .await)?;
    tx.commit().await?;
    Ok(p)
}

pub async fn delete_provider(pool: &PgPool, ws: Uuid, id: Uuid) -> DbResult<()> {
    sqlx::query("DELETE FROM ai_providers WHERE workspace_id = $1 AND id = $2")
        .bind(ws)
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn list_models(pool: &PgPool, provider_id: Uuid) -> DbResult<Vec<AiModel>> {
    Ok(sqlx::query_as::<_, AiModel>(&format!(
        "SELECT {MODEL_COLS} FROM ai_models WHERE provider_id = $1 ORDER BY is_default DESC, name"
    ))
    .bind(provider_id)
    .fetch_all(pool)
    .await?)
}

pub async fn upsert_model(
    pool: &PgPool,
    provider_id: Uuid,
    name: &str,
    ctx: Option<i32>,
    in_cost: f64,
    out_cost: f64,
    is_default: bool,
) -> DbResult<AiModel> {
    let mut tx = pool.begin().await?;
    if is_default {
        sqlx::query("UPDATE ai_models SET is_default = false WHERE provider_id = $1")
            .bind(provider_id)
            .execute(&mut *tx)
            .await?;
    }
    let m = sqlx::query_as::<_, AiModel>(&format!(
        "INSERT INTO ai_models (id, provider_id, name, context_window, input_cost_per_mtok, output_cost_per_mtok, is_default)
         VALUES ($1,$2,$3,$4,$5::float8::numeric,$6::float8::numeric,$7)
         ON CONFLICT (provider_id, name) DO UPDATE SET context_window = EXCLUDED.context_window, input_cost_per_mtok = EXCLUDED.input_cost_per_mtok,
           output_cost_per_mtok = EXCLUDED.output_cost_per_mtok, is_default = EXCLUDED.is_default
         RETURNING {MODEL_COLS}"
    ))
    .bind(new_id())
    .bind(provider_id)
    .bind(name)
    .bind(ctx)
    .bind(in_cost)
    .bind(out_cost)
    .bind(is_default)
    .fetch_one(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(m)
}

pub async fn delete_model(pool: &PgPool, provider_id: Uuid, id: Uuid) -> DbResult<()> {
    sqlx::query("DELETE FROM ai_models WHERE provider_id = $1 AND id = $2")
        .bind(provider_id)
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub async fn record_request(
    pool: &PgPool,
    ws: Uuid,
    project: Option<Uuid>,
    provider_id: Option<Uuid>,
    provider_kind: &str,
    model: &str,
    purpose: &str,
    ok: bool,
    attempts: i32,
    latency_ms: i64,
    error: Option<&str>,
    input_tokens: i32,
    output_tokens: i32,
    cost_usd: f64,
    user: Option<Uuid>,
) -> DbResult<Uuid> {
    let mut tx = pool.begin().await?;
    let id = new_id();
    sqlx::query(
        "INSERT INTO ai_requests (id, workspace_id, project_id, provider_id, provider_kind, model, purpose, status, attempts, latency_ms, error, created_by)
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12)",
    )
    .bind(id)
    .bind(ws)
    .bind(project)
    .bind(provider_id)
    .bind(provider_kind)
    .bind(model)
    .bind(purpose)
    .bind(if ok { "succeeded" } else { "failed" })
    .bind(attempts)
    .bind(latency_ms)
    .bind(error)
    .bind(user)
    .execute(&mut *tx)
    .await?;
    sqlx::query("INSERT INTO ai_usage (id, request_id, workspace_id, input_tokens, output_tokens, cost_usd) VALUES ($1,$2,$3,$4,$5,$6::float8::numeric)")
        .bind(new_id())
        .bind(id)
        .bind(ws)
        .bind(input_tokens)
        .bind(output_tokens)
        .bind(cost_usd)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(id)
}

#[derive(Debug, Clone, FromRow, Serialize, ToSchema)]
pub struct UsageRow {
    pub provider_kind: String,
    pub model: String,
    pub purpose: String,
    pub requests: i64,
    pub failures: i64,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cost_usd: f64,
}

pub async fn usage_summary(pool: &PgPool, ws: Uuid, days: i32) -> DbResult<Vec<UsageRow>> {
    Ok(sqlx::query_as::<_, UsageRow>(
        "SELECT r.provider_kind, r.model, r.purpose, count(*) AS requests, count(*) FILTER (WHERE r.status = 'failed') AS failures,
           COALESCE(sum(u.input_tokens),0)::bigint AS input_tokens, COALESCE(sum(u.output_tokens),0)::bigint AS output_tokens,
           COALESCE(sum(u.cost_usd),0)::float8 AS cost_usd
         FROM ai_requests r LEFT JOIN ai_usage u ON u.request_id = r.id
         WHERE r.workspace_id = $1 AND r.created_at > now() - make_interval(days => $2)
         GROUP BY 1,2,3 ORDER BY requests DESC",
    )
    .bind(ws)
    .bind(days)
    .fetch_all(pool)
    .await?)
}

// ── investigations ──
#[derive(Debug, Clone, FromRow, Serialize, ToSchema)]
pub struct Investigation {
    pub id: Uuid,
    pub job_id: Uuid,
    pub execution_id: Uuid,
    pub project_id: Uuid,
    pub category: String,
    pub deterministic_category: String,
    pub confidence: f32,
    pub summary: String,
    pub probable_root_cause: String,
    #[schema(value_type = Object)]
    pub observations: Value,
    #[schema(value_type = Object)]
    pub interpretation: Option<Value>,
    #[schema(value_type = Object)]
    pub suggested_investigation: Value,
    #[schema(value_type = Object)]
    pub suggested_remediation: Value,
    pub ai_request_id: Option<Uuid>,
    pub created_at: DateTime<Utc>,
}

const INV_COLS: &str = "id, job_id, execution_id, project_id, category, deterministic_category, confidence, summary, probable_root_cause, observations,
  interpretation, suggested_investigation, suggested_remediation, ai_request_id, created_at";

#[allow(clippy::too_many_arguments)]
pub async fn insert_investigation(
    pool: &PgPool,
    job_id: Uuid,
    execution_id: Uuid,
    project_id: Uuid,
    category: &str,
    deterministic_category: &str,
    confidence: f32,
    summary: &str,
    root_cause: &str,
    observations: &Value,
    interpretation: Option<&Value>,
    investigate: &Value,
    remediate: &Value,
    ai_request_id: Option<Uuid>,
    user: Option<Uuid>,
) -> DbResult<Investigation> {
    Ok(sqlx::query_as::<_, Investigation>(&format!(
        "INSERT INTO failure_investigations (id, job_id, execution_id, project_id, category, deterministic_category, confidence, summary, probable_root_cause,
           observations, interpretation, suggested_investigation, suggested_remediation, ai_request_id, created_by)
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15) RETURNING {INV_COLS}"
    ))
    .bind(new_id())
    .bind(job_id)
    .bind(execution_id)
    .bind(project_id)
    .bind(category)
    .bind(deterministic_category)
    .bind(confidence)
    .bind(summary)
    .bind(root_cause)
    .bind(observations)
    .bind(interpretation)
    .bind(investigate)
    .bind(remediate)
    .bind(ai_request_id)
    .bind(user)
    .fetch_one(pool)
    .await?)
}

pub async fn job_investigations(pool: &PgPool, job_id: Uuid) -> DbResult<Vec<Investigation>> {
    Ok(sqlx::query_as::<_, Investigation>(&format!(
        "SELECT {INV_COLS} FROM failure_investigations WHERE job_id = $1 ORDER BY created_at DESC"
    ))
    .bind(job_id)
    .fetch_all(pool)
    .await?)
}

pub async fn execution_investigations(
    pool: &PgPool,
    execution_id: Uuid,
) -> DbResult<Vec<Investigation>> {
    Ok(sqlx::query_as::<_, Investigation>(&format!("SELECT {INV_COLS} FROM failure_investigations WHERE execution_id = $1 ORDER BY created_at DESC"))
        .bind(execution_id)
        .fetch_all(pool)
        .await?)
}

pub async fn get_investigation(pool: &PgPool, id: Uuid) -> DbResult<Investigation> {
    opt(sqlx::query_as::<_, Investigation>(&format!(
        "SELECT {INV_COLS} FROM failure_investigations WHERE id = $1"
    ))
    .bind(id)
    .fetch_one(pool)
    .await)
}

// ── recommendations ──
#[derive(Debug, Clone, FromRow, Serialize, ToSchema)]
pub struct Recommendation {
    pub id: Uuid,
    pub project_id: Uuid,
    pub kind: String,
    pub title: String,
    pub body: String,
    pub source: String,
    pub status: String,
    #[schema(value_type = Object)]
    pub related: Value,
    pub created_at: DateTime<Utc>,
}

pub async fn replace_heuristic_recommendations(
    pool: &PgPool,
    project_id: Uuid,
    recs: &[(String, String, String, Value)],
) -> DbResult<()> {
    let mut tx = pool.begin().await?;
    sqlx::query("DELETE FROM ai_recommendations WHERE project_id = $1 AND source = 'heuristic' AND status = 'open'").bind(project_id).execute(&mut *tx).await?;
    for (kind, title, body, related) in recs {
        let dismissed: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM ai_recommendations WHERE project_id = $1 AND title = $2 AND status = 'dismissed')")
            .bind(project_id)
            .bind(title)
            .fetch_one(&mut *tx)
            .await?;
        if dismissed {
            continue;
        }
        sqlx::query("INSERT INTO ai_recommendations (id, project_id, kind, title, body, source, related) VALUES ($1,$2,$3,$4,$5,'heuristic',$6)")
            .bind(new_id())
            .bind(project_id)
            .bind(kind)
            .bind(title)
            .bind(body)
            .bind(related)
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;
    Ok(())
}

pub async fn insert_recommendation(
    pool: &PgPool,
    project_id: Uuid,
    kind: &str,
    title: &str,
    body: &str,
    source: &str,
    related: &Value,
) -> DbResult<()> {
    sqlx::query("INSERT INTO ai_recommendations (id, project_id, kind, title, body, source, related) VALUES ($1,$2,$3,$4,$5,$6,$7)")
        .bind(new_id())
        .bind(project_id)
        .bind(kind)
        .bind(title)
        .bind(body)
        .bind(source)
        .bind(related)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn list_recommendations(
    pool: &PgPool,
    project_id: Uuid,
) -> DbResult<Vec<Recommendation>> {
    Ok(sqlx::query_as::<_, Recommendation>(
        "SELECT id, project_id, kind, title, body, source, status, related, created_at FROM ai_recommendations WHERE project_id = $1 AND status = 'open' ORDER BY created_at DESC",
    )
    .bind(project_id)
    .fetch_all(pool)
    .await?)
}

pub async fn set_recommendation_status(
    pool: &PgPool,
    project_id: Uuid,
    id: Uuid,
    status: &str,
) -> DbResult<()> {
    sqlx::query("UPDATE ai_recommendations SET status = $3 WHERE project_id = $1 AND id = $2")
        .bind(project_id)
        .bind(id)
        .bind(status)
        .execute(pool)
        .await?;
    Ok(())
}

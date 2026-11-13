use crate::auth::{project_ctx, Authed};
use crate::error::{ApiError, ApiResult};
use crate::routes::{ok, Ok};
use crate::state::AppState;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use once_cell::sync::Lazy;
use oryn_db::analytics::RequirementCoverage;
use oryn_db::governance::audit;
use oryn_db::projects::{Requirement, TraceLink};
use oryn_domain::{Permission, RequirementKind};
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::json;
use utoipa::ToSchema;
use utoipa_axum::{router::OpenApiRouter, routes};
use uuid::Uuid;

pub fn routes() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(list_requirements, create_requirement))
        .routes(routes!(import_requirements))
        .routes(routes!(update_requirement, delete_requirement))
        .routes(routes!(link))
        .routes(routes!(unlink))
        .routes(routes!(traceability))
}

#[utoipa::path(get, path = "/api/v1/projects/{id}/requirements", tag = "requirements", responses((status = 200, body = Vec<Requirement>)))]
async fn list_requirements(
    State(s): State<AppState>,
    auth: Authed,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Vec<Requirement>>> {
    project_ctx(&s, &auth, id)
        .await?
        .require(Permission::ProjectRead)?;
    Ok(Json(
        oryn_db::projects::list_requirements(&s.pool, id).await?,
    ))
}

#[derive(Deserialize, ToSchema)]
pub struct RequirementReq {
    /// Optional stable key (e.g. US-8). Defaults to the next REQ-n.
    pub key: Option<String>,
    pub title: String,
    #[serde(default)]
    pub body: String,
    pub kind: Option<RequirementKind>,
    pub priority: Option<String>,
    pub parent_id: Option<Uuid>,
}

fn check_priority(p: &str) -> ApiResult<()> {
    if ["low", "medium", "high", "critical"].contains(&p) {
        Ok(())
    } else {
        Err(ApiError::bad(
            "priority must be low, medium, high or critical",
        ))
    }
}

static KEY: Lazy<Regex> = Lazy::new(|| Regex::new(r"^[A-Za-z][A-Za-z0-9]{0,15}-\d{1,6}$").unwrap());

#[utoipa::path(post, path = "/api/v1/projects/{id}/requirements", tag = "requirements", responses((status = 201, body = Requirement)))]
async fn create_requirement(
    State(s): State<AppState>,
    auth: Authed,
    Path(id): Path<Uuid>,
    Json(req): Json<RequirementReq>,
) -> ApiResult<(StatusCode, Json<Requirement>)> {
    let p = project_ctx(&s, &auth, id).await?;
    p.require(Permission::ProjectWrite)?;
    if req.title.trim().is_empty() {
        return Err(ApiError::bad("title is required"));
    }
    if let Some(k) = &req.key {
        if !KEY.is_match(k) {
            return Err(ApiError::bad("keys look like PREFIX-123"));
        }
    }
    let priority = req.priority.unwrap_or_else(|| "medium".into());
    check_priority(&priority)?;
    let kind = req.kind.unwrap_or(RequirementKind::Requirement);
    let r = oryn_db::projects::create_requirement(
        &s.pool,
        id,
        req.key.as_deref(),
        req.title.trim(),
        &req.body,
        kind.as_str(),
        "manual",
        &priority,
        req.parent_id,
        Some(p.user_id()),
    )
    .await?;
    audit(
        &s.pool,
        Some(p.ws_id()),
        &p.actor(),
        "requirement.created",
        Some("requirement"),
        Some(r.id),
        json!({"key": r.key}),
    )
    .await;
    Ok((StatusCode::CREATED, Json(r)))
}

#[derive(Deserialize, ToSchema)]
pub struct ImportReq {
    /// Markdown or plain text. Markdown: each `##` heading (or `#` when there are no `##`)
    /// becomes a requirement; a leading `KEY-n` in the heading becomes its key.
    pub content: String,
    pub kind: Option<RequirementKind>,
    /// Name of the source document, recorded on each requirement.
    pub source: Option<String>,
}

#[derive(Serialize, ToSchema)]
pub struct ImportResult {
    pub created: Vec<Requirement>,
    pub skipped: Vec<String>,
}

/// Split a document into (key, title, body) sections.
pub fn split_document(content: &str) -> Vec<(Option<String>, String, String)> {
    static HEAD_KEY: Lazy<Regex> =
        Lazy::new(|| Regex::new(r"^([A-Za-z][A-Za-z0-9]{0,15}-\d{1,6})[\s:.\-–—]+(.+)$").unwrap());
    let level = if content.lines().any(|l| l.starts_with("## ")) {
        "## "
    } else {
        "# "
    };
    let mut out = vec![];
    let mut cur: Option<(String, Vec<&str>)> = None;
    for line in content.lines() {
        if let Some(h) = line.strip_prefix(level) {
            if let Some((t, b)) = cur.take() {
                out.push((t, b.join("\n")));
            }
            cur = Some((h.trim().to_string(), vec![]));
        } else if let Some((_, b)) = cur.as_mut() {
            b.push(line);
        }
    }
    if let Some((t, b)) = cur {
        out.push((t, b.join("\n")));
    }
    if out.is_empty() {
        // Plain text: one requirement per paragraph; first line is the title.
        for para in content
            .split("\n\n")
            .map(str::trim)
            .filter(|p| !p.is_empty())
        {
            let mut lines = para.lines();
            let title = lines.next().unwrap_or("").trim().to_string();
            out.push((title, lines.collect::<Vec<_>>().join("\n")));
        }
    }
    out.into_iter()
        .filter(|(t, _)| !t.is_empty())
        .map(|(t, b)| match HEAD_KEY.captures(&t) {
            Some(c) => (
                Some(c[1].to_string()),
                c[2].trim().to_string(),
                b.trim().to_string(),
            ),
            None => (None, t.chars().take(200).collect(), b.trim().to_string()),
        })
        .collect()
}

#[utoipa::path(post, path = "/api/v1/projects/{id}/requirements/import", tag = "requirements", responses((status = 201, body = ImportResult)))]
async fn import_requirements(
    State(s): State<AppState>,
    auth: Authed,
    Path(id): Path<Uuid>,
    Json(req): Json<ImportReq>,
) -> ApiResult<(StatusCode, Json<ImportResult>)> {
    let p = project_ctx(&s, &auth, id).await?;
    p.require(Permission::ProjectWrite)?;
    if req.content.len() > 2_000_000 {
        return Err(ApiError::bad("documents are limited to 2 MB"));
    }
    let kind = req.kind.unwrap_or(RequirementKind::UserStory);
    let source = req.source.unwrap_or_else(|| "import".into());
    let existing: Vec<String> = oryn_db::projects::list_requirements(&s.pool, id)
        .await?
        .into_iter()
        .map(|r| r.key.to_ascii_uppercase())
        .collect();
    let mut created = vec![];
    let mut skipped = vec![];
    for (key, title, body) in split_document(&req.content) {
        if let Some(k) = &key {
            if existing.contains(&k.to_ascii_uppercase()) {
                skipped.push(format!("{k} already exists"));
                continue;
            }
        }
        match oryn_db::projects::create_requirement(
            &s.pool,
            id,
            key.as_deref(),
            &title,
            &body,
            kind.as_str(),
            &source,
            "medium",
            None,
            Some(p.user_id()),
        )
        .await
        {
            Result::Ok(r) => created.push(r),
            Err(e) => skipped.push(format!("{title}: {e}")),
        }
    }
    audit(
        &s.pool,
        Some(p.ws_id()),
        &p.actor(),
        "requirement.imported",
        Some("project"),
        Some(id),
        json!({"created": created.len(), "skipped": skipped.len(), "source": source}),
    )
    .await;
    Ok((StatusCode::CREATED, Json(ImportResult { created, skipped })))
}

#[derive(Deserialize, ToSchema)]
pub struct UpdateRequirement {
    pub title: Option<String>,
    pub body: Option<String>,
    pub kind: Option<RequirementKind>,
    pub priority: Option<String>,
}

#[utoipa::path(patch, path = "/api/v1/projects/{id}/requirements/{rid}", tag = "requirements", responses((status = 200, body = Requirement)))]
async fn update_requirement(
    State(s): State<AppState>,
    auth: Authed,
    Path((id, rid)): Path<(Uuid, Uuid)>,
    Json(req): Json<UpdateRequirement>,
) -> ApiResult<Json<Requirement>> {
    let p = project_ctx(&s, &auth, id).await?;
    p.require(Permission::ProjectWrite)?;
    if let Some(pr) = &req.priority {
        check_priority(pr)?;
    }
    let r = oryn_db::projects::update_requirement(
        &s.pool,
        id,
        rid,
        req.title.as_deref(),
        req.body.as_deref(),
        req.kind.map(|k| k.as_str()),
        req.priority.as_deref(),
    )
    .await?;
    audit(
        &s.pool,
        Some(p.ws_id()),
        &p.actor(),
        "requirement.updated",
        Some("requirement"),
        Some(rid),
        json!({}),
    )
    .await;
    Ok(Json(r))
}

#[utoipa::path(delete, path = "/api/v1/projects/{id}/requirements/{rid}", tag = "requirements", responses((status = 200, body = Ok)))]
async fn delete_requirement(
    State(s): State<AppState>,
    auth: Authed,
    Path((id, rid)): Path<(Uuid, Uuid)>,
) -> ApiResult<Json<Ok>> {
    let p = project_ctx(&s, &auth, id).await?;
    p.require(Permission::ProjectWrite)?;
    oryn_db::projects::delete_requirement(&s.pool, id, rid).await?;
    audit(
        &s.pool,
        Some(p.ws_id()),
        &p.actor(),
        "requirement.deleted",
        Some("requirement"),
        Some(rid),
        json!({}),
    )
    .await;
    Ok(ok())
}

#[derive(Deserialize, ToSchema)]
pub struct LinkReq {
    pub scenario_id: Uuid,
}

#[utoipa::path(post, path = "/api/v1/projects/{id}/requirements/{rid}/links", tag = "requirements", responses((status = 200, body = Ok)))]
async fn link(
    State(s): State<AppState>,
    auth: Authed,
    Path((id, rid)): Path<(Uuid, Uuid)>,
    Json(req): Json<LinkReq>,
) -> ApiResult<Json<Ok>> {
    let p = project_ctx(&s, &auth, id).await?;
    p.require(Permission::TestWrite)?;
    oryn_db::testing::get_scenario(&s.pool, id, req.scenario_id).await?;
    oryn_db::projects::link_requirement(&s.pool, id, rid, req.scenario_id, "manual").await?;
    audit(
        &s.pool,
        Some(p.ws_id()),
        &p.actor(),
        "requirement.linked",
        Some("requirement"),
        Some(rid),
        json!({"scenario_id": req.scenario_id}),
    )
    .await;
    Ok(ok())
}

#[utoipa::path(delete, path = "/api/v1/projects/{id}/requirements/{rid}/links/{scenario_id}", tag = "requirements", responses((status = 200, body = Ok)))]
async fn unlink(
    State(s): State<AppState>,
    auth: Authed,
    Path((id, rid, scenario_id)): Path<(Uuid, Uuid, Uuid)>,
) -> ApiResult<Json<Ok>> {
    let p = project_ctx(&s, &auth, id).await?;
    p.require(Permission::TestWrite)?;
    oryn_db::projects::get_requirement(&s.pool, id, rid).await?;
    oryn_db::projects::unlink_requirement(&s.pool, rid, scenario_id).await?;
    Ok(ok())
}

#[derive(Serialize, ToSchema)]
pub struct Traceability {
    pub links: Vec<TraceLink>,
    pub coverage: RequirementCoverage,
}

/// Requirement → scenario → latest result matrix, with coverage figures (see docs/METRICS.md).
#[utoipa::path(get, path = "/api/v1/projects/{id}/traceability", tag = "requirements", responses((status = 200, body = Traceability)))]
async fn traceability(
    State(s): State<AppState>,
    auth: Authed,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Traceability>> {
    project_ctx(&s, &auth, id)
        .await?
        .require(Permission::ProjectRead)?;
    Ok(Json(Traceability {
        links: oryn_db::projects::trace_links(&s.pool, id).await?,
        coverage: oryn_db::analytics::requirement_coverage(&s.pool, id).await?,
    }))
}

#[cfg(test)]
mod tests {
    #[test]
    fn splits_markdown() {
        let doc = "# Spec\nintro\n\n## US-1 Registration\n- AC-1.1 x\n\n## Login\nbody";
        let r = super::split_document(doc);
        assert_eq!(r.len(), 2);
        assert_eq!(r[0].0.as_deref(), Some("US-1"));
        assert_eq!(r[0].1, "Registration");
        assert_eq!(r[1].0, None);
    }
}

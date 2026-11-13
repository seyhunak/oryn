//! Quality Intelligence: metrics (formulas in docs/METRICS.md), investigations,
//! recommendations and regression selection.

use crate::auth::{project_ctx, visible_projects, ws_ctx, Authed};
use crate::error::{ApiError, ApiResult};
use crate::routes::{ok, Ok};
use crate::services::executions::{runnable, start, RunRequest};
use crate::state::AppState;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::Json;
use oryn_db::ai::{Investigation, Recommendation};
use oryn_db::analytics::{
    FlakyScenario, Overview, RecentFailure, RequirementCoverage, ScenarioStat, TrendPoint,
};
use oryn_db::execution::Execution;
use oryn_db::testing::ScenarioFilter;
use oryn_domain::{Browser, Permission};
use oryn_intelligence::analysis::{modules_for, Analysis};
use oryn_intelligence::regression::{self, HealthFacts, ScenarioRef, Selection};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::time::Duration;
use utoipa::{IntoParams, ToSchema};
use utoipa_axum::{router::OpenApiRouter, routes};
use uuid::Uuid;

pub fn routes() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(overview))
        .routes(routes!(trends))
        .routes(routes!(flaky))
        .routes(routes!(failures))
        .routes(routes!(quality))
        .routes(routes!(refresh_recommendations))
        .routes(routes!(dismiss_recommendation))
        .routes(routes!(investigate_job))
        .routes(routes!(get_investigation))
        .routes(routes!(regression_select))
        .routes(routes!(regression_run))
}

#[derive(Deserialize, IntoParams)]
pub struct WindowQuery {
    pub days: Option<i32>,
    pub project_id: Option<Uuid>,
}

async fn scope(
    s: &AppState,
    auth: &Authed,
    ws: Uuid,
    project: Option<Uuid>,
) -> ApiResult<Vec<Uuid>> {
    let c = ws_ctx(s, auth, ws).await?;
    let visible = visible_projects(s, &c).await?;
    Ok(match project {
        Some(p) if visible.contains(&p) => vec![p],
        Some(_) => return Err(ApiError::not_found("project")),
        None => visible,
    })
}

fn days(d: Option<i32>) -> i32 {
    d.unwrap_or(30).clamp(1, 365)
}

#[utoipa::path(get, path = "/api/v1/workspaces/{ws}/overview", tag = "intelligence", params(WindowQuery), responses((status = 200, body = Overview)))]
async fn overview(
    State(s): State<AppState>,
    auth: Authed,
    Path(ws): Path<Uuid>,
    Query(q): Query<WindowQuery>,
) -> ApiResult<Json<Overview>> {
    let projects = scope(&s, &auth, ws, q.project_id).await?;
    Ok(Json(
        oryn_db::analytics::overview(&s.pool, &projects, days(q.days)).await?,
    ))
}

#[utoipa::path(get, path = "/api/v1/workspaces/{ws}/trends", tag = "intelligence", params(WindowQuery), responses((status = 200, body = Vec<TrendPoint>)))]
async fn trends(
    State(s): State<AppState>,
    auth: Authed,
    Path(ws): Path<Uuid>,
    Query(q): Query<WindowQuery>,
) -> ApiResult<Json<Vec<TrendPoint>>> {
    let projects = scope(&s, &auth, ws, q.project_id).await?;
    Ok(Json(
        oryn_db::analytics::trends(&s.pool, &projects, days(q.days)).await?,
    ))
}

#[utoipa::path(get, path = "/api/v1/workspaces/{ws}/flaky", tag = "intelligence", params(WindowQuery), responses((status = 200, body = Vec<FlakyScenario>)))]
async fn flaky(
    State(s): State<AppState>,
    auth: Authed,
    Path(ws): Path<Uuid>,
    Query(q): Query<WindowQuery>,
) -> ApiResult<Json<Vec<FlakyScenario>>> {
    let projects = scope(&s, &auth, ws, q.project_id).await?;
    Ok(Json(
        oryn_db::analytics::flaky_scenarios(&s.pool, &projects, days(q.days)).await?,
    ))
}

#[derive(Deserialize, IntoParams)]
pub struct FailuresQuery {
    pub project_id: Option<Uuid>,
    pub limit: Option<i64>,
}

#[utoipa::path(get, path = "/api/v1/workspaces/{ws}/failures", tag = "intelligence", params(FailuresQuery), responses((status = 200, body = Vec<RecentFailure>)))]
async fn failures(
    State(s): State<AppState>,
    auth: Authed,
    Path(ws): Path<Uuid>,
    Query(q): Query<FailuresQuery>,
) -> ApiResult<Json<Vec<RecentFailure>>> {
    let projects = scope(&s, &auth, ws, q.project_id).await?;
    Ok(Json(
        oryn_db::analytics::recent_failures(
            &s.pool,
            &projects,
            q.limit.unwrap_or(20).clamp(1, 100),
        )
        .await?,
    ))
}

#[derive(Serialize, ToSchema)]
pub struct ProjectQuality {
    pub overview: Overview,
    pub scenarios: Vec<ScenarioStat>,
    pub coverage: RequirementCoverage,
    pub flaky: Vec<FlakyScenario>,
    pub recommendations: Vec<Recommendation>,
}

#[utoipa::path(get, path = "/api/v1/projects/{id}/quality", tag = "intelligence", params(WindowQuery), responses((status = 200, body = ProjectQuality)))]
async fn quality(
    State(s): State<AppState>,
    auth: Authed,
    Path(id): Path<Uuid>,
    Query(q): Query<WindowQuery>,
) -> ApiResult<Json<ProjectQuality>> {
    project_ctx(&s, &auth, id)
        .await?
        .require(Permission::ProjectRead)?;
    let d = days(q.days);
    Ok(Json(ProjectQuality {
        overview: oryn_db::analytics::overview(&s.pool, &[id], d).await?,
        scenarios: oryn_db::analytics::scenario_stats(&s.pool, id, d).await?,
        coverage: oryn_db::analytics::requirement_coverage(&s.pool, id).await?,
        flaky: oryn_db::analytics::flaky_scenarios(&s.pool, &[id], d).await?,
        recommendations: oryn_db::ai::list_recommendations(&s.pool, id).await?,
    }))
}

async fn scenario_text(s: &AppState, id: Uuid) -> ApiResult<String> {
    Ok(oryn_db::testing::scenario_steps(&s.pool, id)
        .await?
        .into_iter()
        .map(|r| format!("{} {}", r.keyword.trim(), r.text))
        .collect::<Vec<_>>()
        .join("\n"))
}

/// Recompute deterministic recommendations from coverage, flakiness, failures and routes.
#[utoipa::path(post, path = "/api/v1/projects/{id}/recommendations/refresh", tag = "intelligence", responses((status = 200, body = Vec<Recommendation>)))]
async fn refresh_recommendations(
    State(s): State<AppState>,
    auth: Authed,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Vec<Recommendation>>> {
    project_ctx(&s, &auth, id)
        .await?
        .require(Permission::ProjectRead)?;
    let cov = oryn_db::analytics::requirement_coverage(&s.pool, id).await?;
    let stats = oryn_db::analytics::scenario_stats(&s.pool, id, 30).await?;
    let scenarios =
        oryn_db::testing::list_scenarios(&s.pool, id, &ScenarioFilter::default()).await?;
    let mut f = HealthFacts {
        uncovered_requirements: cov
            .rows
            .iter()
            .filter(|r| r.scenarios == 0)
            .map(|r| (r.key.clone(), r.title.clone()))
            .collect(),
        unverified_requirements: cov
            .rows
            .iter()
            .filter(|r| r.scenarios > 0 && (r.failing > 0 || r.never_run > 0))
            .map(|r| (r.key.clone(), r.title.clone()))
            .collect(),
        flaky_scenarios: oryn_db::analytics::flaky_scenarios(&s.pool, &[id], 30)
            .await?
            .into_iter()
            .map(|x| (x.scenario_id.to_string(), x.scenario_name))
            .collect(),
        slow_scenarios: stats
            .iter()
            .filter(|x| x.avg_duration_ms.unwrap_or(0.0) > 30_000.0)
            .map(|x| {
                (
                    x.scenario_id.to_string(),
                    x.scenario_name.clone(),
                    x.avg_duration_ms.unwrap_or(0.0) as i64,
                )
            })
            .collect(),
        never_run: scenarios
            .iter()
            .filter(|x| x.status == "approved" && x.last_status.is_none())
            .map(|x| (x.id.to_string(), x.name.clone()))
            .collect(),
        ..Default::default()
    };
    for sc in scenarios
        .iter()
        .filter(|x| matches!(x.last_status.as_deref(), Some("failed") | Some("error")))
    {
        let hist = oryn_db::execution::scenario_history(&s.pool, sc.id, 10).await?;
        let streak = hist
            .iter()
            .filter(|r| !r.superseded && r.finished_at.is_some())
            .take_while(|r| matches!(r.status.as_str(), "failed" | "error" | "timed_out"))
            .count() as i64;
        f.failing_scenarios
            .push((sc.id.to_string(), sc.name.clone(), streak));
    }
    if let Some(ctx) = oryn_db::projects::get_context(&s.pool, id).await? {
        let analysis: Analysis = serde_json::from_value(ctx.analysis).unwrap_or_default();
        let mut corpus = String::new();
        for sc in scenarios.iter().take(500) {
            corpus.push_str(&scenario_text(&s, sc.id).await?);
            corpus.push('\n');
        }
        f.untested_routes = analysis
            .routes
            .iter()
            .filter(|r| {
                r.method == "GET"
                    && !r.path.contains(':')
                    && !r.path.contains('{')
                    && !corpus.contains(&format!("\"{}\"", r.path))
            })
            .map(|r| format!("{} {}", r.method, r.path))
            .collect();
    }
    oryn_db::ai::replace_heuristic_recommendations(&s.pool, id, &regression::recommendations(&f))
        .await?;
    Ok(Json(oryn_db::ai::list_recommendations(&s.pool, id).await?))
}

#[utoipa::path(post, path = "/api/v1/projects/{id}/recommendations/{rid}/dismiss", tag = "intelligence", responses((status = 200, body = Ok)))]
async fn dismiss_recommendation(
    State(s): State<AppState>,
    auth: Authed,
    Path((id, rid)): Path<(Uuid, Uuid)>,
) -> ApiResult<Json<Ok>> {
    project_ctx(&s, &auth, id)
        .await?
        .require(Permission::TestWrite)?;
    oryn_db::ai::set_recommendation_status(&s.pool, id, rid, "dismissed").await?;
    Ok(ok())
}

#[derive(Deserialize, ToSchema)]
pub struct InvestigateReq {
    /// Ask the configured model for an interpretation (labelled, stored separately).
    #[serde(default)]
    pub use_ai: bool,
}

#[utoipa::path(post, path = "/api/v1/jobs/{job_id}/investigate", tag = "intelligence", responses((status = 201, body = Investigation)))]
async fn investigate_job(
    State(s): State<AppState>,
    auth: Authed,
    Path(job_id): Path<Uuid>,
    Json(req): Json<InvestigateReq>,
) -> ApiResult<(StatusCode, Json<Investigation>)> {
    let job = oryn_db::execution::get_job(&s.pool, job_id)
        .await
        .map_err(|_| ApiError::not_found("job"))?;
    let p = project_ctx(&s, &auth, job.project_id)
        .await
        .map_err(|_| ApiError::not_found("job"))?;
    p.require(Permission::ProjectRead)?;
    if !matches!(job.status.as_str(), "failed" | "error" | "timed_out") {
        return Err(ApiError::unprocessable(format!(
            "only failed jobs can be investigated; this one is {}",
            job.status
        )));
    }
    let ai_for = if req.use_ai {
        p.require(Permission::AiUse)?;
        s.limiter
            .check(
                &format!("ai-investigate:{}", p.user_id()),
                60,
                Duration::from_secs(3600),
            )
            .await?;
        Some(Some(p.user_id()))
    } else {
        None
    };
    let inv = crate::services::investigate::investigate(&s, job_id, false, ai_for).await?;
    Ok((StatusCode::CREATED, Json(inv)))
}

#[utoipa::path(get, path = "/api/v1/investigations/{inv_id}", tag = "intelligence", responses((status = 200, body = Investigation)))]
async fn get_investigation(
    State(s): State<AppState>,
    auth: Authed,
    Path(inv_id): Path<Uuid>,
) -> ApiResult<Json<Investigation>> {
    let inv = oryn_db::ai::get_investigation(&s.pool, inv_id)
        .await
        .map_err(|_| ApiError::not_found("investigation"))?;
    project_ctx(&s, &auth, inv.project_id)
        .await
        .map_err(|_| ApiError::not_found("investigation"))?
        .require(Permission::ProjectRead)?;
    Ok(Json(inv))
}

#[derive(Deserialize, ToSchema)]
pub struct RegressionReq {
    /// Changed file paths (e.g. from `git diff --name-only`). Empty = the change set
    /// from the last repository analysis.
    #[serde(default)]
    pub changed_files: Vec<String>,
    /// Minimum score to include (smoke baseline scores 0.5).
    pub min_score: Option<f32>,
}

#[derive(Serialize, ToSchema)]
pub struct RegressionSelection {
    pub changed_files: Vec<String>,
    pub changed_modules: Vec<String>,
    pub selections: Vec<Selection>,
    pub total_runnable: usize,
}

async fn select(
    s: &AppState,
    project: Uuid,
    req: &RegressionReq,
) -> ApiResult<RegressionSelection> {
    let ctx = oryn_db::projects::get_context(&s.pool, project).await?;
    let analysis: Analysis = ctx
        .and_then(|c| serde_json::from_value(c.analysis).ok())
        .unwrap_or_default();
    let changed: Vec<String> = if req.changed_files.is_empty() {
        analysis.changes.touched()
    } else {
        req.changed_files
            .iter()
            .map(|p| p.trim().trim_start_matches("./").to_string())
            .filter(|p| !p.is_empty())
            .take(5000)
            .collect()
    };
    let ws = oryn_db::projects::get_project(&s.pool, project)
        .await?
        .workspace_id;
    let require = oryn_db::identity::get_workspace(&s.pool, ws)
        .await?
        .require_approval;
    let all =
        oryn_db::testing::list_scenarios(&s.pool, project, &ScenarioFilter::default()).await?;
    let flaky: Vec<Uuid> = oryn_db::analytics::flaky_scenarios(&s.pool, &[project], 30)
        .await?
        .into_iter()
        .map(|f| f.scenario_id)
        .collect();
    let mut refs = vec![];
    for sc in all.iter().filter(|x| runnable(x, require)) {
        refs.push(ScenarioRef {
            id: sc.id.to_string(),
            name: sc.name.clone(),
            suite: sc.suite_name.clone(),
            tags: sc.tags.clone(),
            text: scenario_text(s, sc.id).await?,
            last_status: sc.last_status.clone(),
            flaky: flaky.contains(&sc.id),
        });
    }
    let min = req.min_score.unwrap_or(0.5);
    let selections: Vec<Selection> = regression::select(&changed, &analysis.routes, &refs)
        .into_iter()
        .filter(|x| x.score >= min)
        .collect();
    Ok(RegressionSelection {
        changed_modules: modules_for(&changed, &analysis.modules),
        changed_files: changed,
        selections,
        total_runnable: refs.len(),
    })
}

/// Rank runnable scenarios by their relationship to the changed files (rules in docs/METRICS.md).
#[utoipa::path(post, path = "/api/v1/projects/{id}/regression/select", tag = "intelligence", responses((status = 200, body = RegressionSelection)))]
async fn regression_select(
    State(s): State<AppState>,
    auth: Authed,
    Path(id): Path<Uuid>,
    Json(req): Json<RegressionReq>,
) -> ApiResult<Json<RegressionSelection>> {
    project_ctx(&s, &auth, id)
        .await?
        .require(Permission::ProjectRead)?;
    Ok(Json(select(&s, id, &req).await?))
}

#[derive(Deserialize, ToSchema)]
pub struct RegressionRunReq {
    #[serde(flatten)]
    pub selection: RegressionReq,
    pub environment_id: Option<Uuid>,
    pub browser: Option<Browser>,
    pub commit_sha: Option<String>,
    pub branch: Option<String>,
}

#[utoipa::path(post, path = "/api/v1/projects/{id}/regression/run", tag = "intelligence", responses((status = 201, body = Execution)))]
async fn regression_run(
    State(s): State<AppState>,
    auth: Authed,
    Path(id): Path<Uuid>,
    Json(req): Json<RegressionRunReq>,
) -> ApiResult<(StatusCode, Json<Execution>)> {
    let p = project_ctx(&s, &auth, id).await?;
    p.require(Permission::ExecutionRun)?;
    let sel = select(&s, id, &req.selection).await?;
    if sel.selections.is_empty() {
        return Err(ApiError::unprocessable(
            "no scenarios relate to the changed files; run the full suite instead",
        ));
    }
    let run = RunRequest {
        scenario_ids: sel
            .selections
            .iter()
            .filter_map(|x| x.scenario_id.parse().ok())
            .collect(),
        environment_id: req.environment_id,
        browser: req.browser,
        title: Some(format!(
            "Regression: {} scenarios for {} changed files",
            sel.selections.len(),
            sel.changed_files.len()
        )),
        trigger: Some("regression".into()),
        commit_sha: req.commit_sha,
        branch: req.branch,
        ..Default::default()
    };
    let e = start(&s, &p, run).await?;
    oryn_db::governance::audit(
        &s.pool,
        Some(p.ws_id()),
        &p.actor(),
        "regression.run",
        Some("execution"),
        Some(e.id),
        json!({"changed_files": sel.changed_files.len(), "selected": sel.selections.len()}),
    )
    .await;
    Ok((StatusCode::CREATED, Json(e)))
}

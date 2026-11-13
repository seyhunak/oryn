use crate::auth::{project_ctx, visible_projects, ws_ctx, Authed};
use crate::error::{ApiError, ApiResult};
use crate::routes::{ok, Ok, Paged};
use crate::services::executions::{start, RunRequest};
use crate::state::AppState;
use axum::extract::{Path, Query, State};
use axum::http::{header, HeaderValue, StatusCode};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::Json;
use futures::stream::Stream;
use oryn_db::ai::Investigation;
use oryn_db::analytics::ComparisonRow;
use oryn_db::execution::{Artifact, Execution, ExecutionFilter, ExecutionStep, Job, JobEvent};
use oryn_db::governance::audit;
use oryn_db::Page;
use oryn_domain::Permission;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::convert::Infallible;
use std::time::Duration;
use utoipa::{IntoParams, ToSchema};
use utoipa_axum::{router::OpenApiRouter, routes};
use uuid::Uuid;

pub fn routes() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(list_executions))
        .routes(routes!(create_execution))
        .routes(routes!(get_execution))
        .routes(routes!(cancel))
        .routes(routes!(rerun))
        .routes(routes!(events))
        .routes(routes!(get_job))
        .routes(routes!(artifact_content))
        .routes(routes!(compare))
}

#[derive(Deserialize, IntoParams)]
pub struct ListQuery {
    pub project_id: Option<Uuid>,
    pub status: Option<String>,
    pub trigger: Option<String>,
    pub suite_id: Option<Uuid>,
    pub page: Option<i64>,
    pub per_page: Option<i64>,
    /// `asc` for oldest first.
    pub order: Option<String>,
}

#[utoipa::path(get, path = "/api/v1/workspaces/{ws}/executions", tag = "executions", params(ListQuery), responses((status = 200, body = Paged<Execution>)))]
async fn list_executions(
    State(s): State<AppState>,
    auth: Authed,
    Path(ws): Path<Uuid>,
    Query(q): Query<ListQuery>,
) -> ApiResult<Json<Paged<Execution>>> {
    let c = ws_ctx(&s, &auth, ws).await?;
    let visible = visible_projects(&s, &c).await?;
    let page = Page::new(q.page, q.per_page);
    let f = ExecutionFilter {
        project_id: q.project_id,
        status: q.status,
        trigger: q.trigger,
        suite_id: q.suite_id,
    };
    let (items, total) = oryn_db::execution::list_executions(
        &s.pool,
        ws,
        &f,
        &visible,
        page,
        q.order.as_deref() == Some("asc"),
    )
    .await?;
    Ok(Json(Paged {
        items,
        total,
        page: page.offset / page.limit.max(1) + 1,
        per_page: page.limit,
    }))
}

/// Queue an execution. Only approved, executable scenarios run (draft ones too when
/// the workspace does not require approval). Returns immediately; follow progress
/// with the events stream.
#[utoipa::path(post, path = "/api/v1/projects/{id}/executions", tag = "executions", request_body = RunRequest, responses((status = 201, body = Execution), (status = 422)))]
async fn create_execution(
    State(s): State<AppState>,
    auth: Authed,
    Path(id): Path<Uuid>,
    Json(mut req): Json<RunRequest>,
) -> ApiResult<(StatusCode, Json<Execution>)> {
    let p = project_ctx(&s, &auth, id).await?;
    p.require(Permission::ExecutionRun)?;
    if matches!(p.ws.auth.via, crate::auth::Via::Token { .. }) && req.trigger.is_none() {
        req.trigger = Some("api".into());
    }
    if req.trigger.as_deref() == Some("rerun") || req.trigger.as_deref() == Some("regression") {
        req.trigger = Some("manual".into());
    }
    Ok((StatusCode::CREATED, Json(start(&s, &p, req).await?)))
}

#[derive(Serialize, ToSchema)]
pub struct ExecutionDetail {
    pub execution: Execution,
    pub jobs: Vec<Job>,
    pub artifacts: Vec<Artifact>,
    pub investigations: Vec<Investigation>,
}

async fn load_execution(
    s: &AppState,
    auth: &Authed,
    eid: Uuid,
    perm: Permission,
) -> ApiResult<(Execution, crate::auth::ProjectCtx)> {
    let e = oryn_db::execution::get_execution(&s.pool, eid)
        .await
        .map_err(|_| ApiError::not_found("execution"))?;
    let p = project_ctx(s, auth, e.project_id)
        .await
        .map_err(|_| ApiError::not_found("execution"))?;
    p.require(perm)?;
    Ok((e, p))
}

#[utoipa::path(get, path = "/api/v1/executions/{eid}", tag = "executions", responses((status = 200, body = ExecutionDetail)))]
async fn get_execution(
    State(s): State<AppState>,
    auth: Authed,
    Path(eid): Path<Uuid>,
) -> ApiResult<Json<ExecutionDetail>> {
    let (execution, _) = load_execution(&s, &auth, eid, Permission::ProjectRead).await?;
    Ok(Json(ExecutionDetail {
        jobs: oryn_db::execution::execution_jobs(&s.pool, eid).await?,
        artifacts: oryn_db::execution::execution_artifacts(&s.pool, eid).await?,
        investigations: oryn_db::ai::execution_investigations(&s.pool, eid).await?,
        execution,
    }))
}

#[utoipa::path(post, path = "/api/v1/executions/{eid}/cancel", tag = "executions", responses((status = 200, body = Execution)))]
async fn cancel(
    State(s): State<AppState>,
    auth: Authed,
    Path(eid): Path<Uuid>,
) -> ApiResult<Json<Execution>> {
    let (e, p) = load_execution(&s, &auth, eid, Permission::ExecutionCancel).await?;
    if e.status
        .parse::<oryn_domain::ExecutionStatus>()
        .map(|x| x.is_terminal())
        .unwrap_or(false)
    {
        return Err(ApiError::Conflict(format!(
            "execution already {}",
            e.status
        )));
    }
    let (status, finished) = oryn_db::execution::cancel_execution(&s.pool, eid).await?;
    audit(
        &s.pool,
        Some(p.ws_id()),
        &p.actor(),
        "execution.cancelled",
        Some("execution"),
        Some(eid),
        json!({"status": status.to_string()}),
    )
    .await;
    if finished {
        crate::services::notify::execution_finished(s.clone(), eid);
    }
    Ok(Json(oryn_db::execution::get_execution(&s.pool, eid).await?))
}

#[derive(Deserialize, ToSchema)]
pub struct RerunReq {
    /// Only scenarios whose final result was failed or error.
    #[serde(default)]
    pub failed_only: bool,
}

#[utoipa::path(post, path = "/api/v1/executions/{eid}/rerun", tag = "executions", responses((status = 201, body = Execution)))]
async fn rerun(
    State(s): State<AppState>,
    auth: Authed,
    Path(eid): Path<Uuid>,
    Json(req): Json<RerunReq>,
) -> ApiResult<(StatusCode, Json<Execution>)> {
    let (e, p) = load_execution(&s, &auth, eid, Permission::ExecutionRun).await?;
    let jobs = oryn_db::execution::execution_jobs(&s.pool, eid).await?;
    let ids: Vec<Uuid> = jobs
        .iter()
        .filter(|j| !j.superseded)
        .filter(|j| {
            !req.failed_only || matches!(j.status.as_str(), "failed" | "error" | "timed_out")
        })
        .filter_map(|j| j.scenario_id)
        .collect();
    if ids.is_empty() {
        return Err(ApiError::unprocessable("nothing to re-run"));
    }
    let browser = e.browser.parse().ok();
    let policy = oryn_db::execution::execution_retry_policy(&s.pool, eid).await?;
    let run = RunRequest {
        suite_id: e.suite_id,
        scenario_ids: ids,
        max_attempts: policy.map(|p| p.0),
        retry_on_failure: policy.is_some_and(|p| p.1),
        environment_id: e.environment_id,
        browser,
        title: Some(format!("Re-run of {}", e.title)),
        trigger: Some("rerun".into()),
        rerun_of: Some(eid),
        commit_sha: e.commit_sha.clone(),
        branch: e.branch.clone(),
        ..Default::default()
    };
    Ok((StatusCode::CREATED, Json(start(&s, &p, run).await?)))
}

#[derive(Deserialize, IntoParams)]
pub struct EventsQuery {
    /// Resume after this event id.
    pub after: Option<i64>,
}

/// Server-sent events: `job` events as they are recorded (claimed, started,
/// finished, retried, cancelled) and a final `end` event with the execution status.
/// Step results are not streamed; read them from the job once it has finished.
#[utoipa::path(get, path = "/api/v1/executions/{eid}/events", tag = "executions", params(EventsQuery), responses((status = 200, content_type = "text/event-stream", body = JobEvent)))]
async fn events(
    State(s): State<AppState>,
    auth: Authed,
    Path(eid): Path<Uuid>,
    Query(q): Query<EventsQuery>,
) -> ApiResult<Sse<impl Stream<Item = Result<Event, Infallible>>>> {
    load_execution(&s, &auth, eid, Permission::ProjectRead).await?;
    let pool = s.pool.clone();
    let stream = futures::stream::unfold(
        (q.after.unwrap_or(0), false, 0u32),
        move |(after, done, idle)| {
            let pool = pool.clone();
            async move {
                if done {
                    return None;
                }
                if after > 0 || idle > 0 {
                    tokio::time::sleep(Duration::from_millis(800)).await;
                }
                let evs = oryn_db::execution::events_since(&pool, eid, after)
                    .await
                    .unwrap_or_default();
                let exec = oryn_db::execution::get_execution(&pool, eid).await.ok();
                let terminal = exec
                    .as_ref()
                    .map(|e| {
                        matches!(
                            e.status.as_str(),
                            "passed" | "failed" | "error" | "cancelled"
                        )
                    })
                    .unwrap_or(true);
                let mut out: Vec<Result<Event, Infallible>> = Vec::new();
                let mut last = after;
                for e in &evs {
                    last = e.id;
                    out.push(Ok(Event::default()
                        .event("job")
                        .id(e.id.to_string())
                        .json_data(e)
                        .unwrap_or_default()));
                }
                if let Some(x) = &exec {
                    out.push(Ok(Event::default().event("execution").json_data(json!({"status": x.status, "total": x.total, "passed": x.passed, "failed": x.failed, "errored": x.errored, "cancelled": x.cancelled})).unwrap_or_default()));
                }
                // Close after the terminal state is observed with no new events, or after ~2 h.
                let end = (terminal && evs.is_empty()) || idle > 9000;
                if end {
                    out.push(Ok(Event::default().event("end").data(
                        exec.map(|e| e.status).unwrap_or_else(|| "unknown".into()),
                    )));
                }
                Some((futures::stream::iter(out), (last, end, idle + 1)))
            }
        },
    );
    use futures::StreamExt;
    Ok(Sse::new(stream.flatten()).keep_alive(KeepAlive::new().interval(Duration::from_secs(15))))
}

#[derive(Serialize, ToSchema)]
pub struct JobDetail {
    pub job: Job,
    pub steps: Vec<ExecutionStep>,
    #[schema(value_type = Object)]
    pub logs: Value,
    #[schema(value_type = Object)]
    pub network: Value,
    pub artifacts: Vec<Artifact>,
    pub investigations: Vec<Investigation>,
    /// Earlier attempts of this scenario in the same execution.
    pub attempts: Vec<Job>,
}

#[utoipa::path(get, path = "/api/v1/jobs/{job_id}", tag = "executions", responses((status = 200, body = JobDetail)))]
async fn get_job(
    State(s): State<AppState>,
    auth: Authed,
    Path(job_id): Path<Uuid>,
) -> ApiResult<Json<JobDetail>> {
    let job = oryn_db::execution::get_job(&s.pool, job_id)
        .await
        .map_err(|_| ApiError::not_found("job"))?;
    load_execution(&s, &auth, job.execution_id, Permission::ProjectRead).await?;
    let ev = oryn_db::execution::job_evidence(&s.pool, job_id).await?;
    let artifacts = oryn_db::execution::execution_artifacts(&s.pool, job.execution_id)
        .await?
        .into_iter()
        .filter(|a| a.job_id == job_id)
        .collect();
    let attempts = oryn_db::execution::execution_jobs(&s.pool, job.execution_id)
        .await?
        .into_iter()
        .filter(|j| j.scenario_id == job.scenario_id && j.id != job_id)
        .collect();
    Ok(Json(JobDetail {
        steps: oryn_db::execution::job_steps(&s.pool, job_id).await?,
        logs: ev.logs,
        network: ev.network,
        artifacts,
        investigations: oryn_db::ai::job_investigations(&s.pool, job_id).await?,
        attempts,
        job,
    }))
}

/// Download artifact bytes. Images, video, JSON and text are served inline;
/// anything else (e.g. trace zips) as an attachment. Always sandboxed.
#[utoipa::path(get, path = "/api/v1/artifacts/{aid}/content", tag = "executions", responses((status = 200, content_type = "application/octet-stream")))]
async fn artifact_content(
    State(s): State<AppState>,
    auth: Authed,
    Path(aid): Path<Uuid>,
) -> ApiResult<Response> {
    let a = oryn_db::execution::get_artifact(&s.pool, aid)
        .await
        .map_err(|_| ApiError::not_found("artifact"))?;
    let p = project_ctx(&s, &auth, a.project_id)
        .await
        .map_err(|_| ApiError::not_found("artifact"))?;
    p.require(Permission::ProjectRead)?;
    let bytes = s
        .artifacts
        .get(&a.storage_key)
        .await
        .map_err(|_| ApiError::not_found("artifact content"))?;
    let inline = [
        "image/png",
        "image/jpeg",
        "image/webp",
        "video/webm",
        "application/json",
        "text/plain",
    ]
    .contains(&a.content_type.as_str());
    let ct = if inline {
        a.content_type.clone()
    } else {
        "application/octet-stream".into()
    };
    let disp = format!(
        "{}; filename=\"{}\"",
        if inline { "inline" } else { "attachment" },
        oryn_artifacts::sanitize_name(&a.name)
    );
    let mut r = bytes.into_response();
    let h = r.headers_mut();
    h.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_str(&ct).unwrap_or(HeaderValue::from_static("application/octet-stream")),
    );
    h.insert(
        header::CONTENT_DISPOSITION,
        HeaderValue::from_str(&disp).unwrap_or(HeaderValue::from_static("attachment")),
    );
    h.insert(
        header::CONTENT_SECURITY_POLICY,
        HeaderValue::from_static("sandbox; default-src 'none'; img-src 'self'; media-src 'self'"),
    );
    h.insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("private, max-age=3600"),
    );
    Ok(r)
}

#[derive(Deserialize, IntoParams)]
pub struct CompareQuery {
    pub base: Uuid,
    pub head: Uuid,
}

/// Scenario-by-scenario comparison of two executions of the same project.
#[utoipa::path(get, path = "/api/v1/executions/compare", tag = "executions", params(CompareQuery), responses((status = 200, body = Vec<ComparisonRow>)))]
async fn compare(
    State(s): State<AppState>,
    auth: Authed,
    Query(q): Query<CompareQuery>,
) -> ApiResult<Json<Vec<ComparisonRow>>> {
    let (a, _) = load_execution(&s, &auth, q.base, Permission::ProjectRead).await?;
    let (b, _) = load_execution(&s, &auth, q.head, Permission::ProjectRead).await?;
    if a.project_id != b.project_id {
        return Err(ApiError::bad(
            "both executions must belong to the same project",
        ));
    }
    Ok(Json(
        oryn_db::analytics::compare_executions(&s.pool, q.base, q.head).await?,
    ))
}

#[allow(dead_code)]
fn _ok() -> Json<Ok> {
    ok()
}

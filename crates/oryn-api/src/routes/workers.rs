//! Execution Fabric: worker administration and the worker protocol (`oryn.job/v1`).
//!
//! Workers authenticate with a per-worker token (`oryn_wrk_…`, stored hashed). A worker
//! with a workspace serves only that workspace; a shared worker (registered with the
//! instance bootstrap token) serves all. Jobs are leased; a lease that is not renewed
//! by heartbeats expires and the job is retried or failed by the reaper.

use crate::auth::{ws_ctx, Authed};
use crate::error::{ApiError, ApiResult};
use crate::routes::{ok, Ok};
use crate::services;
use crate::state::AppState;
use axum::body::Bytes;
use axum::extract::{FromRequestParts, Path, Query, State};
use axum::http::{header, request::Parts, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use oryn_db::execution::{Job, Worker, WorkerCapability};
use oryn_db::governance::{audit, Actor};
use oryn_domain::job::JobResult;
use oryn_domain::{JobStatus, Permission};
use oryn_execution::retry::RetryPolicy;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use utoipa::{IntoParams, ToSchema};
use utoipa_axum::{router::OpenApiRouter, routes};
use uuid::Uuid;

pub fn routes() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(list_workers, create_worker))
        .routes(routes!(get_worker, revoke_worker))
        .routes(routes!(queue))
        .routes(routes!(register))
        .routes(routes!(heartbeat))
        .routes(routes!(claim))
        .routes(routes!(start))
        .routes(routes!(job_heartbeat))
        .routes(routes!(upload_artifact))
        .routes(routes!(complete))
}

// ── administration ──

#[utoipa::path(get, path = "/api/v1/workspaces/{ws}/workers", tag = "workers", responses((status = 200, body = Vec<Worker>)))]
async fn list_workers(
    State(s): State<AppState>,
    auth: Authed,
    Path(ws): Path<Uuid>,
) -> ApiResult<Json<Vec<Worker>>> {
    ws_ctx(&s, &auth, ws)
        .await?
        .require(Permission::WorkspaceRead)?;
    Ok(Json(oryn_db::execution::list_workers(&s.pool, ws).await?))
}

#[derive(Deserialize, ToSchema)]
pub struct CreateWorker {
    pub name: String,
}

#[derive(Serialize, ToSchema)]
pub struct WorkerCredentials {
    pub worker_id: Uuid,
    /// Shown once. Start the worker with `ORYN_WORKER_TOKEN=<token>`.
    pub token: String,
}

/// Register a worker for this workspace and return its token.
#[utoipa::path(post, path = "/api/v1/workspaces/{ws}/workers", tag = "workers", responses((status = 201, body = WorkerCredentials)))]
async fn create_worker(
    State(s): State<AppState>,
    auth: Authed,
    Path(ws): Path<Uuid>,
    Json(req): Json<CreateWorker>,
) -> ApiResult<(StatusCode, Json<WorkerCredentials>)> {
    let c = ws_ctx(&s, &auth, ws).await?;
    c.require(Permission::WorkerManage)?;
    let name = req.name.trim();
    if name.is_empty() || name.len() > 80 {
        return Err(ApiError::bad("name must be 1 to 80 characters"));
    }
    let token = oryn_auth::generate_token("oryn_wrk_");
    let id = oryn_db::execution::register_worker(
        &s.pool,
        Some(ws),
        name,
        &oryn_auth::hash_token(&token),
        "",
        "",
        "",
        1,
        &[],
        &[],
    )
    .await?;
    audit(
        &s.pool,
        Some(ws),
        &c.actor(),
        "worker.created",
        Some("worker"),
        Some(id),
        json!({"name": name}),
    )
    .await;
    Ok((
        StatusCode::CREATED,
        Json(WorkerCredentials {
            worker_id: id,
            token,
        }),
    ))
}

#[derive(Serialize, ToSchema)]
pub struct WorkerDetail {
    pub worker: Worker,
    pub capabilities: Vec<WorkerCapability>,
    pub recent_jobs: Vec<Job>,
}

async fn worker_in_ws(s: &AppState, ws: Uuid, id: Uuid) -> ApiResult<Worker> {
    let w = oryn_db::execution::get_worker(&s.pool, id)
        .await
        .map_err(|_| ApiError::not_found("worker"))?;
    if w.workspace_id.is_some() && w.workspace_id != Some(ws) {
        return Err(ApiError::not_found("worker"));
    }
    Ok(w)
}

#[utoipa::path(get, path = "/api/v1/workspaces/{ws}/workers/{id}", tag = "workers", responses((status = 200, body = WorkerDetail)))]
async fn get_worker(
    State(s): State<AppState>,
    auth: Authed,
    Path((ws, id)): Path<(Uuid, Uuid)>,
) -> ApiResult<Json<WorkerDetail>> {
    ws_ctx(&s, &auth, ws)
        .await?
        .require(Permission::WorkspaceRead)?;
    let worker = worker_in_ws(&s, ws, id).await?;
    let mut recent_jobs = oryn_db::execution::worker_jobs(&s.pool, id, 50).await?;
    if worker.workspace_id.is_none() {
        // A shared worker's history may include other workspaces' jobs: show only this workspace's.
        let mut visible = vec![];
        for j in recent_jobs {
            if let Result::Ok(e) = oryn_db::execution::get_execution(&s.pool, j.execution_id).await
            {
                if e.workspace_id == ws {
                    visible.push(j);
                }
            }
        }
        recent_jobs = visible;
    }
    Ok(Json(WorkerDetail {
        capabilities: oryn_db::execution::worker_capabilities(&s.pool, id).await?,
        recent_jobs,
        worker,
    }))
}

/// Revoke a worker: its token stops working immediately and its leased jobs are requeued.
#[utoipa::path(delete, path = "/api/v1/workspaces/{ws}/workers/{id}", tag = "workers", responses((status = 200, body = Ok)))]
async fn revoke_worker(
    State(s): State<AppState>,
    auth: Authed,
    Path((ws, id)): Path<(Uuid, Uuid)>,
) -> ApiResult<Json<Ok>> {
    let c = ws_ctx(&s, &auth, ws).await?;
    c.require(Permission::WorkerManage)?;
    let w = worker_in_ws(&s, ws, id).await?;
    if w.workspace_id.is_none() && !c.auth.user.is_instance_admin {
        return Err(ApiError::forbidden(
            "shared workers can only be revoked by an instance admin",
        ));
    }
    oryn_db::execution::revoke_worker(&s.pool, id).await?;
    audit(
        &s.pool,
        Some(ws),
        &c.actor(),
        "worker.revoked",
        Some("worker"),
        Some(id),
        json!({"name": w.name}),
    )
    .await;
    Ok(ok())
}

#[derive(Serialize, ToSchema)]
pub struct QueueDepth {
    pub queued: i64,
    pub running: i64,
}

#[utoipa::path(get, path = "/api/v1/workspaces/{ws}/queue", tag = "workers", responses((status = 200, body = QueueDepth)))]
async fn queue(
    State(s): State<AppState>,
    auth: Authed,
    Path(ws): Path<Uuid>,
) -> ApiResult<Json<QueueDepth>> {
    ws_ctx(&s, &auth, ws)
        .await?
        .require(Permission::WorkspaceRead)?;
    let (queued, running) = oryn_db::execution::queue_depth(&s.pool, ws).await?;
    Ok(Json(QueueDepth { queued, running }))
}

// ── worker protocol ──

pub struct WorkerAuth {
    pub id: Uuid,
    pub workspace_id: Option<Uuid>,
    pub status: String,
}

impl FromRequestParts<AppState> for WorkerAuth {
    type Rejection = ApiError;
    async fn from_request_parts(parts: &mut Parts, s: &AppState) -> Result<Self, Self::Rejection> {
        let t = parts
            .headers
            .get(header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer "))
            .ok_or(ApiError::Unauthorized)?;
        if !t.starts_with("oryn_wrk_") {
            return Err(ApiError::Unauthorized);
        }
        let (id, workspace_id, status) =
            oryn_db::execution::worker_by_token(&s.pool, &oryn_auth::hash_token(t.trim()))
                .await?
                .ok_or(ApiError::Unauthorized)?;
        Ok(WorkerAuth {
            id,
            workspace_id,
            status,
        })
    }
}

#[derive(Deserialize, Serialize, ToSchema, Clone)]
pub struct Capability {
    /// e.g. `api`, `browser:chromium`, `browser:firefox`.
    pub capability: String,
    pub version: Option<String>,
    #[serde(default = "yes")]
    pub available: bool,
}

fn yes() -> bool {
    true
}

fn caps(c: &[Capability]) -> Vec<WorkerCapability> {
    c.iter()
        .take(50)
        .map(|c| WorkerCapability {
            capability: c.capability.chars().take(64).collect(),
            version: c.version.clone(),
            available: c.available,
        })
        .collect()
}

#[derive(Deserialize, ToSchema)]
pub struct RegisterReq {
    pub bootstrap_token: String,
    pub name: String,
    #[serde(default)]
    pub version: String,
    #[serde(default)]
    pub hostname: String,
    #[serde(default)]
    pub os: String,
    #[serde(default = "one")]
    pub concurrency: i32,
    #[serde(default)]
    pub labels: Vec<String>,
    #[serde(default)]
    pub capabilities: Vec<Capability>,
}

fn one() -> i32 {
    1
}

/// Self-registration of a shared worker with the instance bootstrap token.
#[utoipa::path(post, path = "/api/v1/worker/register", tag = "worker-protocol", responses((status = 201, body = WorkerCredentials)))]
async fn register(
    State(s): State<AppState>,
    Json(req): Json<RegisterReq>,
) -> ApiResult<(StatusCode, Json<WorkerCredentials>)> {
    s.limiter
        .check("worker-register", 60, std::time::Duration::from_secs(60))
        .await?;
    let expected = s.cfg.worker_bootstrap_token.clone().ok_or_else(|| {
        ApiError::forbidden("worker self-registration is disabled (no ORYN_WORKER_BOOTSTRAP_TOKEN)")
    })?;
    if !oryn_auth::constant_time_eq(expected.as_bytes(), req.bootstrap_token.as_bytes()) {
        return Err(ApiError::Unauthorized);
    }
    let token = oryn_auth::generate_token("oryn_wrk_");
    let name: String = req.name.trim().chars().take(80).collect();
    let id = oryn_db::execution::register_worker(
        &s.pool,
        None,
        &name,
        &oryn_auth::hash_token(&token),
        &req.version,
        &req.hostname,
        &req.os,
        req.concurrency.clamp(1, 64),
        &req.labels,
        &caps(&req.capabilities),
    )
    .await?;
    audit(
        &s.pool,
        None,
        &Actor {
            kind: "worker",
            id: Some(id),
            label: name.clone(),
            ip: None,
        },
        "worker.registered",
        Some("worker"),
        Some(id),
        json!({"hostname": req.hostname, "version": req.version}),
    )
    .await;
    Ok((
        StatusCode::CREATED,
        Json(WorkerCredentials {
            worker_id: id,
            token,
        }),
    ))
}

#[derive(Deserialize, ToSchema)]
pub struct HeartbeatReq {
    pub active_jobs: i32,
    #[serde(default)]
    pub draining: bool,
    pub capabilities: Option<Vec<Capability>>,
    pub version: Option<String>,
    pub hostname: Option<String>,
    pub os: Option<String>,
    pub concurrency: Option<i32>,
}

#[utoipa::path(post, path = "/api/v1/worker/heartbeat", tag = "worker-protocol", responses((status = 200, body = Ok)))]
async fn heartbeat(
    State(s): State<AppState>,
    w: WorkerAuth,
    Json(req): Json<HeartbeatReq>,
) -> ApiResult<Json<Ok>> {
    let c = req.capabilities.as_deref().map(caps);
    oryn_db::execution::worker_heartbeat(
        &s.pool,
        w.id,
        req.active_jobs.max(0),
        c.as_deref(),
        req.draining,
    )
    .await?;
    if let (Some(v), Some(h)) = (&req.version, &req.hostname) {
        oryn_db::execution::update_worker_info(
            &s.pool,
            w.id,
            v,
            h,
            req.os.as_deref().unwrap_or(""),
            req.concurrency.map(|c| c.clamp(1, 64)),
        )
        .await?;
    }
    Ok(ok())
}

#[derive(Deserialize, ToSchema)]
pub struct ClaimReq {
    /// Job kinds this worker can run now: `api`, `browser`, `mixed`.
    pub kinds: Vec<String>,
}

#[derive(Serialize, ToSchema)]
pub struct ClaimedJob {
    pub job_id: Uuid,
    pub execution_id: Uuid,
    pub attempt: i32,
    pub lease_secs: i64,
    /// The `oryn.job/v1` JobSpec.
    #[schema(value_type = Object)]
    pub spec: Value,
    /// Decrypted environment secrets for this job only. Never logged; redacted from evidence.
    #[schema(value_type = Object)]
    pub secrets: Map<String, Value>,
}

/// Lease the next queued job (204 when none).
#[utoipa::path(post, path = "/api/v1/worker/claim", tag = "worker-protocol", responses((status = 200, body = ClaimedJob), (status = 204)))]
async fn claim(
    State(s): State<AppState>,
    w: WorkerAuth,
    Json(req): Json<ClaimReq>,
) -> ApiResult<Response> {
    if w.status == "draining" {
        return Ok(StatusCode::NO_CONTENT.into_response());
    }
    let kinds: Vec<String> = req
        .kinds
        .into_iter()
        .filter(|k| ["api", "browser", "mixed"].contains(&k.as_str()))
        .collect();
    if kinds.is_empty() {
        return Err(ApiError::bad("kinds must include api, browser or mixed"));
    }
    let Some(job) =
        oryn_db::execution::claim_job(&s.pool, w.id, w.workspace_id, &kinds, s.cfg.lease_secs)
            .await?
    else {
        return Ok(StatusCode::NO_CONTENT.into_response());
    };
    let mut spec = job.spec.clone();
    // A retry row carries its parent's spec: the worker must report against this row.
    spec["job_id"] = json!(job.id);
    spec["attempt"] = json!(job.attempt);
    let mut secrets = Map::new();
    let exec = oryn_db::execution::get_execution(&s.pool, job.execution_id).await?;
    if let Some(env_id) = exec.environment_id {
        if let Result::Ok(env) =
            oryn_db::projects::get_environment(&s.pool, exec.project_id, env_id).await
        {
            if let Some(o) = env.secrets.as_object() {
                for (k, v) in o {
                    if let Some(sealed) = v.as_str() {
                        secrets.insert(k.clone(), json!(s.open(sealed)?));
                    }
                }
            }
        }
    }
    Ok(Json(ClaimedJob {
        job_id: job.id,
        execution_id: job.execution_id,
        attempt: job.attempt,
        lease_secs: s.cfg.lease_secs,
        spec,
        secrets,
    })
    .into_response())
}

async fn owned_job(s: &AppState, w: &WorkerAuth, job_id: Uuid) -> ApiResult<Job> {
    let j = oryn_db::execution::get_job(&s.pool, job_id)
        .await
        .map_err(|_| ApiError::not_found("job"))?;
    if j.worker_id != Some(w.id) {
        return Err(ApiError::Conflict(
            "this job is not leased to you (lease expired or job reassigned)".into(),
        ));
    }
    Ok(j)
}

#[utoipa::path(post, path = "/api/v1/worker/jobs/{job_id}/start", tag = "worker-protocol", responses((status = 200, body = Ok)))]
async fn start(
    State(s): State<AppState>,
    w: WorkerAuth,
    Path(job_id): Path<Uuid>,
) -> ApiResult<Json<Ok>> {
    oryn_db::execution::start_job(&s.pool, job_id, w.id).await?;
    Ok(ok())
}

#[derive(Serialize, ToSchema)]
pub struct JobHeartbeat {
    pub cancel_requested: bool,
}

/// Renew the job lease. `cancel_requested: true` tells the worker to stop the job.
#[utoipa::path(post, path = "/api/v1/worker/jobs/{job_id}/heartbeat", tag = "worker-protocol", responses((status = 200, body = JobHeartbeat)))]
async fn job_heartbeat(
    State(s): State<AppState>,
    w: WorkerAuth,
    Path(job_id): Path<Uuid>,
) -> ApiResult<Json<JobHeartbeat>> {
    let cancel = oryn_db::execution::heartbeat_job(&s.pool, job_id, w.id, s.cfg.lease_secs).await?;
    Ok(Json(JobHeartbeat {
        cancel_requested: cancel,
    }))
}

#[derive(Deserialize, IntoParams)]
pub struct ArtifactQuery {
    pub name: String,
    /// screenshot, video, trace, log, console, network, response
    pub kind: String,
    pub content_type: String,
}

#[derive(Serialize, ToSchema)]
pub struct UploadedArtifact {
    pub id: Uuid,
    pub sha256: String,
    pub size_bytes: i64,
}

#[utoipa::path(post, path = "/api/v1/worker/jobs/{job_id}/artifacts", tag = "worker-protocol", params(ArtifactQuery),
    request_body(content = Vec<u8>, content_type = "application/octet-stream"), responses((status = 201, body = UploadedArtifact)))]
async fn upload_artifact(
    State(s): State<AppState>,
    w: WorkerAuth,
    Path(job_id): Path<Uuid>,
    Query(q): Query<ArtifactQuery>,
    body: Bytes,
) -> ApiResult<(StatusCode, Json<UploadedArtifact>)> {
    let j = owned_job(&s, &w, job_id).await?;
    let kind: oryn_domain::ArtifactKind = q
        .kind
        .parse()
        .map_err(|_| ApiError::bad("unknown artifact kind"))?;
    let allowed = [
        "image/png",
        "image/jpeg",
        "image/webp",
        "video/webm",
        "application/zip",
        "application/json",
        "text/plain",
    ];
    if !allowed.contains(&q.content_type.as_str()) {
        return Err(ApiError::bad("unsupported artifact content type"));
    }
    if body.len() > s.cfg.max_artifact_bytes {
        return Err(ApiError::bad("artifact exceeds the size limit"));
    }
    let name = oryn_artifacts::sanitize_name(&q.name);
    let key = format!("{}/{}/{}/{}", j.project_id, j.execution_id, job_id, name);
    let stored = s
        .artifacts
        .put(&key, &body)
        .await
        .map_err(|e| ApiError::bad(e.to_string()))?;
    let a = oryn_db::execution::insert_artifact(
        &s.pool,
        j.execution_id,
        job_id,
        j.project_id,
        &name,
        kind.as_str(),
        &q.content_type,
        stored.size_bytes as i64,
        &stored.sha256,
        s.artifacts.backend(),
        &stored.key,
    )
    .await?;
    Ok((
        StatusCode::CREATED,
        Json(UploadedArtifact {
            id: a.id,
            sha256: a.sha256,
            size_bytes: a.size_bytes,
        }),
    ))
}

#[derive(Serialize, ToSchema)]
pub struct CompleteResult {
    pub execution_status: String,
    pub execution_finished: bool,
    /// Set when the job will be retried as a new attempt.
    pub retried_job: Option<Uuid>,
}

/// Report a finished job. The verdict comes from the worker's deterministic engine;
/// retries are decided here from the execution's retry policy.
#[utoipa::path(post, path = "/api/v1/worker/jobs/{job_id}/complete", tag = "worker-protocol", request_body = JobResult, responses((status = 200, body = CompleteResult)))]
async fn complete(
    State(s): State<AppState>,
    w: WorkerAuth,
    Path(job_id): Path<Uuid>,
    Json(result): Json<JobResult>,
) -> ApiResult<Json<CompleteResult>> {
    if result.job_id != job_id {
        return Err(ApiError::bad("job_id in the body does not match the path"));
    }
    owned_job(&s, &w, job_id).await?;
    let decide =
        |status: JobStatus, attempt: i32, max: i32, retry_on_failure: bool| -> Option<u64> {
            let p = RetryPolicy {
                max_attempts: max,
                retry_on_failure,
                backoff_ms: 2_000,
            };
            p.should_retry(status, attempt)
                .then(|| p.delay(attempt).as_millis() as u64)
        };
    let c = oryn_db::execution::complete_job(&s.pool, job_id, w.id, &result, &decide).await?;
    services::investigate::after_completion(s.clone(), job_id);
    if c.execution_finished {
        services::notify::execution_finished(s.clone(), c.execution_id);
    }
    Ok(Json(CompleteResult {
        execution_status: c.execution_status.to_string(),
        execution_finished: c.execution_finished,
        retried_job: c.retried_job,
    }))
}

//! Executions, the PostgreSQL job queue, evidence and workers.
//!
//! The queue uses `SELECT … FOR UPDATE SKIP LOCKED` so any number of API replicas
//! and workers can claim concurrently without double-claiming. Retries create a new
//! job row (attempt + 1) and mark the previous one `superseded`, so every attempt's
//! evidence is kept — flaky-test detection reads that history.

use chrono::{DateTime, Utc};
use oryn_domain::{ExecutionStatus, JobResult, JobStatus};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::{FromRow, PgPool, Postgres, Transaction};
use std::str::FromStr;
use utoipa::ToSchema;
use uuid::Uuid;

use crate::{new_id, opt, DbError, DbResult, Page};

#[derive(Debug, Clone, FromRow, Serialize, ToSchema)]
pub struct Execution {
    pub id: Uuid,
    pub workspace_id: Uuid,
    pub project_id: Uuid,
    pub project_name: String,
    pub environment_id: Option<Uuid>,
    pub environment_name: String,
    pub suite_id: Option<Uuid>,
    pub title: String,
    pub status: String,
    pub trigger: String,
    pub browser: String,
    pub rerun_of: Option<Uuid>,
    pub commit_sha: Option<String>,
    pub branch: Option<String>,
    pub triggered_by: Option<Uuid>,
    pub triggered_by_name: Option<String>,
    pub total: i32,
    pub passed: i32,
    pub failed: i32,
    pub errored: i32,
    pub cancelled: i32,
    pub created_at: DateTime<Utc>,
    pub started_at: Option<DateTime<Utc>>,
    pub finished_at: Option<DateTime<Utc>>,
    pub duration_ms: Option<i64>,
}

const EXEC_SELECT: &str = "SELECT e.id, e.workspace_id, e.project_id, p.name AS project_name, e.environment_id, e.environment_name, e.suite_id, e.title, e.status,
  e.trigger, e.browser, e.rerun_of, e.commit_sha, e.branch, e.triggered_by, u.name AS triggered_by_name, e.total, e.passed, e.failed, e.errored, e.cancelled,
  e.created_at, e.started_at, e.finished_at, e.duration_ms
  FROM test_executions e JOIN projects p ON p.id = e.project_id LEFT JOIN users u ON u.id = e.triggered_by";

#[derive(Debug, Clone, FromRow, Serialize, ToSchema)]
pub struct Job {
    pub id: Uuid,
    pub execution_id: Uuid,
    pub project_id: Uuid,
    pub scenario_id: Option<Uuid>,
    pub scenario_name: String,
    pub kind: String,
    pub status: String,
    pub attempt: i32,
    pub max_attempts: i32,
    pub superseded: bool,
    pub worker_id: Option<Uuid>,
    pub worker_name: Option<String>,
    pub cancel_requested: bool,
    pub claimed_at: Option<DateTime<Utc>>,
    pub started_at: Option<DateTime<Utc>>,
    pub finished_at: Option<DateTime<Utc>>,
    pub duration_ms: Option<i64>,
    pub browser_version: Option<String>,
    pub error: Option<String>,
    pub created_at: DateTime<Utc>,
}

const JOB_SELECT: &str = "SELECT j.id, j.execution_id, j.project_id, j.scenario_id, j.scenario_name, j.kind, j.status, j.attempt, j.max_attempts, j.superseded,
  j.worker_id, w.name AS worker_name, j.cancel_requested, j.claimed_at, j.started_at, j.finished_at, j.duration_ms, j.browser_version, j.error, j.created_at
  FROM execution_jobs j LEFT JOIN execution_workers w ON w.id = j.worker_id";

#[derive(Debug, Clone, FromRow, Serialize, ToSchema)]
pub struct JobEvidence {
    #[schema(value_type = Object)]
    pub logs: Value,
    #[schema(value_type = Object)]
    pub network: Value,
}

#[derive(Debug, Clone, FromRow, Serialize, ToSchema)]
pub struct ExecutionStep {
    pub id: Uuid,
    pub job_id: Uuid,
    pub position: i32,
    pub keyword: String,
    pub text: String,
    pub status: String,
    pub started_at: DateTime<Utc>,
    pub duration_ms: i64,
    pub message: Option<String>,
    pub expected: Option<String>,
    pub actual: Option<String>,
    #[schema(value_type = Object)]
    pub observation: Option<Value>,
    pub artifact_name: Option<String>,
}

#[derive(Debug, Clone, FromRow, Serialize, ToSchema)]
pub struct Artifact {
    pub id: Uuid,
    pub execution_id: Uuid,
    pub job_id: Uuid,
    pub project_id: Uuid,
    pub name: String,
    pub kind: String,
    pub content_type: String,
    pub size_bytes: i64,
    pub sha256: String,
    #[serde(skip)]
    pub storage_key: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, FromRow, Serialize, ToSchema)]
pub struct JobEvent {
    pub id: i64,
    pub job_id: Uuid,
    pub execution_id: Uuid,
    pub kind: String,
    #[schema(value_type = Object)]
    pub data: Value,
    pub created_at: DateTime<Utc>,
}

/// A job to create, with its spec already serialised (secrets sealed).
#[derive(Debug, Clone)]
pub struct NewJob {
    pub id: Uuid,
    pub scenario_id: Option<Uuid>,
    pub scenario_name: String,
    pub kind: String,
    pub spec: Value,
    pub max_attempts: i32,
    pub retry_on_failure: bool,
}

#[derive(Debug, Clone)]
pub struct NewExecution {
    pub id: Uuid,
    pub workspace_id: Uuid,
    pub project_id: Uuid,
    pub environment_id: Option<Uuid>,
    pub environment_name: String,
    pub suite_id: Option<Uuid>,
    pub title: String,
    pub trigger: String,
    pub browser: String,
    pub rerun_of: Option<Uuid>,
    pub commit_sha: Option<String>,
    pub branch: Option<String>,
    pub triggered_by: Option<Uuid>,
}

pub async fn create_execution(pool: &PgPool, e: &NewExecution, jobs: &[NewJob]) -> DbResult<()> {
    let mut tx = pool.begin().await?;
    sqlx::query(
        "INSERT INTO test_executions (id, workspace_id, project_id, environment_id, environment_name, suite_id, title, trigger, browser, rerun_of,
           commit_sha, branch, triggered_by, total) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14)",
    )
    .bind(e.id)
    .bind(e.workspace_id)
    .bind(e.project_id)
    .bind(e.environment_id)
    .bind(&e.environment_name)
    .bind(e.suite_id)
    .bind(&e.title)
    .bind(&e.trigger)
    .bind(&e.browser)
    .bind(e.rerun_of)
    .bind(&e.commit_sha)
    .bind(&e.branch)
    .bind(e.triggered_by)
    .bind(jobs.len() as i32)
    .execute(&mut *tx)
    .await?;
    for j in jobs {
        sqlx::query(
            "INSERT INTO execution_jobs (id, execution_id, workspace_id, project_id, scenario_id, scenario_name, kind, spec, max_attempts, retry_on_failure)
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10)",
        )
        .bind(j.id)
        .bind(e.id)
        .bind(e.workspace_id)
        .bind(e.project_id)
        .bind(j.scenario_id)
        .bind(&j.scenario_name)
        .bind(&j.kind)
        .bind(&j.spec)
        .bind(j.max_attempts)
        .bind(j.retry_on_failure)
        .execute(&mut *tx)
        .await?;
        event(
            &mut tx,
            j.id,
            e.id,
            "job.queued",
            serde_json::json!({"attempt": 1}),
        )
        .await?;
    }
    tx.commit().await?;
    Ok(())
}

async fn event(
    tx: &mut Transaction<'_, Postgres>,
    job_id: Uuid,
    execution_id: Uuid,
    kind: &str,
    data: Value,
) -> DbResult<()> {
    sqlx::query("INSERT INTO job_events (job_id, execution_id, kind, data) VALUES ($1,$2,$3,$4)")
        .bind(job_id)
        .bind(execution_id)
        .bind(kind)
        .bind(data)
        .execute(&mut **tx)
        .await?;
    Ok(())
}

#[derive(Debug, Default, Clone, Deserialize)]
pub struct ExecutionFilter {
    pub project_id: Option<Uuid>,
    pub status: Option<String>,
    pub trigger: Option<String>,
    pub suite_id: Option<Uuid>,
}

/// `visible_projects`: when Some, restrict to these project ids (restricted projects).
pub async fn list_executions(
    pool: &PgPool,
    workspace_id: Uuid,
    f: &ExecutionFilter,
    visible_projects: &[Uuid],
    page: Page,
    sort_asc: bool,
) -> DbResult<(Vec<Execution>, i64)> {
    let where_ = "WHERE e.workspace_id = $1 AND e.project_id = ANY($2) AND ($3::uuid IS NULL OR e.project_id = $3) AND ($4::text IS NULL OR e.status = $4)
                  AND ($5::text IS NULL OR e.trigger = $5) AND ($6::uuid IS NULL OR e.suite_id = $6)";
    let order = if sort_asc { "ASC" } else { "DESC" };
    let rows = sqlx::query_as::<_, Execution>(&format!(
        "{EXEC_SELECT} {where_} ORDER BY e.created_at {order} LIMIT $7 OFFSET $8"
    ))
    .bind(workspace_id)
    .bind(visible_projects)
    .bind(f.project_id)
    .bind(&f.status)
    .bind(&f.trigger)
    .bind(f.suite_id)
    .bind(page.limit)
    .bind(page.offset)
    .fetch_all(pool)
    .await?;
    let total: i64 =
        sqlx::query_scalar(&format!("SELECT count(*) FROM test_executions e {where_}"))
            .bind(workspace_id)
            .bind(visible_projects)
            .bind(f.project_id)
            .bind(&f.status)
            .bind(&f.trigger)
            .bind(f.suite_id)
            .fetch_one(pool)
            .await?;
    Ok((rows, total))
}

pub async fn get_execution(pool: &PgPool, id: Uuid) -> DbResult<Execution> {
    opt(
        sqlx::query_as::<_, Execution>(&format!("{EXEC_SELECT} WHERE e.id = $1"))
            .bind(id)
            .fetch_one(pool)
            .await,
    )
}

pub async fn execution_jobs(pool: &PgPool, execution_id: Uuid) -> DbResult<Vec<Job>> {
    Ok(sqlx::query_as::<_, Job>(&format!(
        "{JOB_SELECT} WHERE j.execution_id = $1 ORDER BY j.scenario_name, j.attempt"
    ))
    .bind(execution_id)
    .fetch_all(pool)
    .await?)
}

pub async fn get_job(pool: &PgPool, job_id: Uuid) -> DbResult<Job> {
    opt(
        sqlx::query_as::<_, Job>(&format!("{JOB_SELECT} WHERE j.id = $1"))
            .bind(job_id)
            .fetch_one(pool)
            .await,
    )
}

pub async fn job_spec(pool: &PgPool, job_id: Uuid) -> DbResult<Value> {
    opt(
        sqlx::query_scalar("SELECT spec FROM execution_jobs WHERE id = $1")
            .bind(job_id)
            .fetch_one(pool)
            .await,
    )
}

pub async fn job_evidence(pool: &PgPool, job_id: Uuid) -> DbResult<JobEvidence> {
    opt(
        sqlx::query_as::<_, JobEvidence>("SELECT logs, network FROM execution_jobs WHERE id = $1")
            .bind(job_id)
            .fetch_one(pool)
            .await,
    )
}

pub async fn job_steps(pool: &PgPool, job_id: Uuid) -> DbResult<Vec<ExecutionStep>> {
    Ok(sqlx::query_as::<_, ExecutionStep>(
        "SELECT id, job_id, position, keyword, text, status, started_at, duration_ms, message, expected, actual, observation, artifact_name
         FROM test_execution_steps WHERE job_id = $1 ORDER BY position",
    )
    .bind(job_id)
    .fetch_all(pool)
    .await?)
}

pub async fn execution_artifacts(pool: &PgPool, execution_id: Uuid) -> DbResult<Vec<Artifact>> {
    Ok(sqlx::query_as::<_, Artifact>(
        "SELECT id, execution_id, job_id, project_id, name, kind, content_type, size_bytes, sha256, storage_key, created_at FROM test_artifacts
         WHERE execution_id = $1 ORDER BY created_at",
    )
    .bind(execution_id)
    .fetch_all(pool)
    .await?)
}

pub async fn get_artifact(pool: &PgPool, id: Uuid) -> DbResult<Artifact> {
    opt(sqlx::query_as::<_, Artifact>(
        "SELECT id, execution_id, job_id, project_id, name, kind, content_type, size_bytes, sha256, storage_key, created_at FROM test_artifacts WHERE id = $1",
    )
    .bind(id)
    .fetch_one(pool)
    .await)
}

#[allow(clippy::too_many_arguments)]
pub async fn insert_artifact(
    pool: &PgPool,
    execution_id: Uuid,
    job_id: Uuid,
    project_id: Uuid,
    name: &str,
    kind: &str,
    content_type: &str,
    size: i64,
    sha: &str,
    backend: &str,
    key: &str,
) -> DbResult<Artifact> {
    Ok(sqlx::query_as::<_, Artifact>(
        "INSERT INTO test_artifacts (id, execution_id, job_id, project_id, name, kind, content_type, size_bytes, sha256, storage_backend, storage_key)
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11)
         ON CONFLICT (job_id, name) DO UPDATE SET size_bytes = EXCLUDED.size_bytes, sha256 = EXCLUDED.sha256, storage_key = EXCLUDED.storage_key
         RETURNING id, execution_id, job_id, project_id, name, kind, content_type, size_bytes, sha256, storage_key, created_at",
    )
    .bind(new_id())
    .bind(execution_id)
    .bind(job_id)
    .bind(project_id)
    .bind(name)
    .bind(kind)
    .bind(content_type)
    .bind(size)
    .bind(sha)
    .bind(backend)
    .bind(key)
    .fetch_one(pool)
    .await?)
}

pub async fn events_since(
    pool: &PgPool,
    execution_id: Uuid,
    after: i64,
) -> DbResult<Vec<JobEvent>> {
    Ok(sqlx::query_as::<_, JobEvent>(
        "SELECT id, job_id, execution_id, kind, data, created_at FROM job_events WHERE execution_id = $1 AND id > $2 ORDER BY id LIMIT 500",
    )
    .bind(execution_id)
    .bind(after)
    .fetch_all(pool)
    .await?)
}

// ── queue ──

#[derive(Debug, Clone, FromRow)]
pub struct ClaimedJob {
    pub id: Uuid,
    pub execution_id: Uuid,
    pub workspace_id: Uuid,
    pub project_id: Uuid,
    pub attempt: i32,
    pub spec: Value,
}

/// Atomically claim the oldest eligible job.
/// `workspace_id` None = shared instance worker (may serve any workspace).
pub async fn claim_job(
    pool: &PgPool,
    worker_id: Uuid,
    workspace_id: Option<Uuid>,
    kinds: &[String],
    lease_secs: i64,
) -> DbResult<Option<ClaimedJob>> {
    let mut tx = pool.begin().await?;
    let job = sqlx::query_as::<_, ClaimedJob>(
        "WITH next AS (
            SELECT id FROM execution_jobs
            WHERE status = 'queued' AND not_before <= now() AND kind = ANY($3) AND ($2::uuid IS NULL OR workspace_id = $2)
            ORDER BY created_at FOR UPDATE SKIP LOCKED LIMIT 1)
         UPDATE execution_jobs j SET status = 'claimed', worker_id = $1, claimed_at = now(), lease_until = now() + make_interval(secs => $4)
         FROM next WHERE j.id = next.id
         RETURNING j.id, j.execution_id, j.workspace_id, j.project_id, j.attempt, j.spec",
    )
    .bind(worker_id)
    .bind(workspace_id)
    .bind(kinds)
    .bind(lease_secs as f64)
    .fetch_optional(&mut *tx)
    .await?;
    if let Some(j) = &job {
        event(
            &mut tx,
            j.id,
            j.execution_id,
            "job.claimed",
            serde_json::json!({"worker_id": worker_id, "attempt": j.attempt}),
        )
        .await?;
        sqlx::query("UPDATE test_executions SET status = 'running', started_at = COALESCE(started_at, now()) WHERE id = $1 AND status = 'queued'")
            .bind(j.execution_id)
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;
    Ok(job)
}

/// Mark a claimed job as running. Fails if the job is not owned by the worker.
pub async fn start_job(pool: &PgPool, job_id: Uuid, worker_id: Uuid) -> DbResult<()> {
    let mut tx = pool.begin().await?;
    let row: Option<(Uuid,)> = sqlx::query_as(
        "UPDATE execution_jobs SET status = 'running', started_at = now() WHERE id = $1 AND worker_id = $2 AND status = 'claimed' RETURNING execution_id",
    )
    .bind(job_id)
    .bind(worker_id)
    .fetch_optional(&mut *tx)
    .await?;
    let Some((exec,)) = row else {
        return Err(DbError::InvalidState(
            "job is not claimed by this worker".into(),
        ));
    };
    event(
        &mut tx,
        job_id,
        exec,
        "job.started",
        serde_json::json!({"worker_id": worker_id}),
    )
    .await?;
    tx.commit().await?;
    Ok(())
}

/// Extend the lease. Returns whether cancellation was requested.
pub async fn heartbeat_job(
    pool: &PgPool,
    job_id: Uuid,
    worker_id: Uuid,
    lease_secs: i64,
) -> DbResult<bool> {
    let row: Option<(bool,)> = sqlx::query_as(
        "UPDATE execution_jobs SET lease_until = now() + make_interval(secs => $3) WHERE id = $1 AND worker_id = $2 AND status IN ('claimed','running')
         RETURNING cancel_requested",
    )
    .bind(job_id)
    .bind(worker_id)
    .bind(lease_secs as f64)
    .fetch_optional(pool)
    .await?;
    match row {
        Some((c,)) => Ok(c),
        None => Err(DbError::InvalidState(
            "job is not active on this worker".into(),
        )),
    }
}

/// Retry decision callback: (status, attempt, max_attempts, retry_on_failure) -> Some(delay_ms).
pub type RetryDecider<'a> = &'a (dyn Fn(JobStatus, i32, i32, bool) -> Option<u64> + Sync);

/// Outcome of completing a job.
#[derive(Debug, Clone)]
pub struct Completion {
    pub execution_id: Uuid,
    pub project_id: Uuid,
    pub retried_job: Option<Uuid>,
    pub execution_status: ExecutionStatus,
    pub execution_finished: bool,
}

/// Record a job's result and evidence, schedule a retry if the policy says so and
/// recompute the execution — all in one transaction.
pub async fn complete_job(
    pool: &PgPool,
    job_id: Uuid,
    worker_id: Uuid,
    result: &JobResult,
    decide: RetryDecider<'_>,
) -> DbResult<Completion> {
    let mut tx = pool.begin().await?;
    let row: Option<(Uuid, Uuid, String, i32, i32, bool, Value, Uuid, Option<Uuid>, String, String, bool)> = sqlx::query_as(
        "SELECT execution_id, project_id, status, attempt, max_attempts, retry_on_failure, spec, workspace_id, scenario_id, scenario_name, kind, cancel_requested
         FROM execution_jobs WHERE id = $1 AND worker_id = $2 FOR UPDATE",
    )
    .bind(job_id)
    .bind(worker_id)
    .fetch_optional(&mut *tx)
    .await?;
    let Some((
        exec,
        project,
        current,
        attempt,
        max_attempts,
        retry_on_failure,
        spec,
        ws,
        scenario_id,
        scenario_name,
        kind,
        cancel_requested,
    )) = row
    else {
        return Err(DbError::InvalidState(
            "job is not owned by this worker".into(),
        ));
    };
    let current =
        JobStatus::from_str(&current).map_err(|e| DbError::InvalidState(e.to_string()))?;
    if current.is_terminal() {
        return Err(DbError::InvalidState(format!("job already {current}")));
    }
    let status = if cancel_requested
        && result.status != JobStatus::Passed
        && result.status != JobStatus::Failed
    {
        JobStatus::Cancelled
    } else {
        result.status
    };

    sqlx::query(
        "UPDATE execution_jobs SET status = $2, finished_at = $3, started_at = COALESCE(started_at, $4), duration_ms = $5, browser_version = $6,
         error = $7, logs = $8, network = $9, lease_until = NULL WHERE id = $1",
    )
    .bind(job_id)
    .bind(status.as_str())
    .bind(result.finished_at)
    .bind(result.started_at)
    .bind(result.duration_ms as i64)
    .bind(&result.browser_version)
    .bind(&result.error)
    .bind(serde_json::to_value(&result.logs).unwrap_or_default())
    .bind(serde_json::to_value(&result.network).unwrap_or_default())
    .execute(&mut *tx)
    .await?;

    sqlx::query("DELETE FROM test_execution_steps WHERE job_id = $1")
        .bind(job_id)
        .execute(&mut *tx)
        .await?;
    for s in &result.steps {
        sqlx::query(
            "INSERT INTO test_execution_steps (id, job_id, execution_id, position, keyword, text, status, started_at, duration_ms, message, expected, actual, observation, artifact_name)
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14)",
        )
        .bind(new_id())
        .bind(job_id)
        .bind(exec)
        .bind(s.index as i32)
        .bind(&s.keyword)
        .bind(&s.text)
        .bind(s.status.as_str())
        .bind(s.started_at)
        .bind(s.duration_ms as i64)
        .bind(&s.message)
        .bind(&s.expected)
        .bind(&s.actual)
        .bind(&s.observation)
        .bind(&s.artifact)
        .execute(&mut *tx)
        .await?;
    }
    event(&mut tx, job_id, exec, "job.finished", serde_json::json!({"status": status, "duration_ms": result.duration_ms, "attempt": attempt})).await?;

    let mut retried_job = None;
    if !cancel_requested {
        if let Some(delay) = decide(status, attempt, max_attempts, retry_on_failure) {
            retried_job = Some(
                insert_retry(
                    &mut tx,
                    job_id,
                    exec,
                    ws,
                    project,
                    scenario_id,
                    &scenario_name,
                    &kind,
                    &spec,
                    attempt + 1,
                    max_attempts,
                    retry_on_failure,
                    delay,
                )
                .await?,
            );
        }
    }
    let (st, finished) = recompute_execution(&mut tx, exec).await?;
    tx.commit().await?;
    Ok(Completion {
        execution_id: exec,
        project_id: project,
        retried_job,
        execution_status: st,
        execution_finished: finished,
    })
}

#[allow(clippy::too_many_arguments)]
async fn insert_retry(
    tx: &mut Transaction<'_, Postgres>,
    old: Uuid,
    exec: Uuid,
    ws: Uuid,
    project: Uuid,
    scenario_id: Option<Uuid>,
    scenario_name: &str,
    kind: &str,
    spec: &Value,
    attempt: i32,
    max_attempts: i32,
    retry_on_failure: bool,
    delay_ms: u64,
) -> DbResult<Uuid> {
    let id = new_id();
    let mut spec = spec.clone();
    spec["job_id"] = serde_json::json!(id);
    spec["attempt"] = serde_json::json!(attempt);
    sqlx::query("UPDATE execution_jobs SET superseded = true WHERE id = $1")
        .bind(old)
        .execute(&mut **tx)
        .await?;
    sqlx::query(
        "INSERT INTO execution_jobs (id, execution_id, workspace_id, project_id, scenario_id, scenario_name, kind, spec, attempt, max_attempts, retry_on_failure, not_before)
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11, now() + make_interval(secs => $12))",
    )
    .bind(id)
    .bind(exec)
    .bind(ws)
    .bind(project)
    .bind(scenario_id)
    .bind(scenario_name)
    .bind(kind)
    .bind(spec)
    .bind(attempt)
    .bind(max_attempts)
    .bind(retry_on_failure)
    .bind(delay_ms as f64 / 1000.0)
    .execute(&mut **tx)
    .await?;
    event(
        tx,
        id,
        exec,
        "job.retry_scheduled",
        serde_json::json!({"attempt": attempt, "previous_job": old, "delay_ms": delay_ms}),
    )
    .await?;
    Ok(id)
}

/// Recompute counters and status from the final (non-superseded) attempts.
///
/// Concurrent completions of jobs in the same execution serialise on the execution row
/// lock taken first. Under READ COMMITTED the counting statement that follows takes a
/// fresh snapshot, so it sees every job result committed before the lock was granted;
/// without the lock two simultaneous completions each miss the other and the execution
/// never reaches a terminal state.
async fn recompute_execution(
    tx: &mut Transaction<'_, Postgres>,
    exec: Uuid,
) -> DbResult<(ExecutionStatus, bool)> {
    sqlx::query("SELECT 1 FROM test_executions WHERE id = $1 FOR UPDATE")
        .bind(exec)
        .execute(&mut **tx)
        .await?;
    let (active, passed, failed, errored, cancelled, total): (i64, i64, i64, i64, i64, i64) =
        sqlx::query_as(
            "SELECT count(*) FILTER (WHERE status IN ('queued','claimed','running')),
                count(*) FILTER (WHERE status = 'passed'),
                count(*) FILTER (WHERE status = 'failed'),
                count(*) FILTER (WHERE status IN ('error','timed_out')),
                count(*) FILTER (WHERE status = 'cancelled'),
                count(*)
         FROM execution_jobs WHERE execution_id = $1 AND NOT superseded",
        )
        .bind(exec)
        .fetch_one(&mut **tx)
        .await?;
    let status = if active > 0 {
        ExecutionStatus::Running
    } else if failed > 0 {
        ExecutionStatus::Failed
    } else if errored > 0 {
        ExecutionStatus::Error
    } else if cancelled > 0 {
        ExecutionStatus::Cancelled
    } else if total == 0 {
        ExecutionStatus::Error
    } else {
        ExecutionStatus::Passed
    };
    let finished = active == 0;
    let prev: Option<(String,)> = sqlx::query_as(
        "UPDATE test_executions SET passed = $2, failed = $3, errored = $4, cancelled = $5, total = $6,
           status = CASE WHEN $8 THEN $7 WHEN status = 'queued' AND $7 = 'running' THEN status ELSE $7 END,
           finished_at = CASE WHEN $8 AND finished_at IS NULL THEN now() ELSE finished_at END,
           duration_ms = CASE WHEN $8 THEN (EXTRACT(EPOCH FROM (now() - COALESCE(started_at, created_at))) * 1000)::bigint ELSE duration_ms END
         WHERE id = $1 RETURNING (SELECT status FROM test_executions WHERE id = $1)",
    )
    .bind(exec)
    .bind(passed as i32)
    .bind(failed as i32)
    .bind(errored as i32)
    .bind(cancelled as i32)
    .bind(total as i32)
    .bind(status.as_str())
    .bind(finished)
    .fetch_optional(&mut **tx)
    .await?;
    let was_terminal = prev
        .and_then(|(s,)| ExecutionStatus::from_str(&s).ok())
        .is_some_and(|s| s.is_terminal());
    Ok((status, finished && !was_terminal))
}

/// Cancel an execution: queued jobs are cancelled immediately, active jobs are
/// flagged and stopped by their worker at the next heartbeat.
pub async fn cancel_execution(pool: &PgPool, exec: Uuid) -> DbResult<(ExecutionStatus, bool)> {
    let mut tx = pool.begin().await?;
    let ids: Vec<(Uuid,)> = sqlx::query_as("UPDATE execution_jobs SET status = 'cancelled', finished_at = now() WHERE execution_id = $1 AND status = 'queued' RETURNING id")
        .bind(exec)
        .fetch_all(&mut *tx)
        .await?;
    for (id,) in &ids {
        event(&mut tx, *id, exec, "job.cancelled", serde_json::json!({})).await?;
    }
    let active: Vec<(Uuid,)> = sqlx::query_as("UPDATE execution_jobs SET cancel_requested = true WHERE execution_id = $1 AND status IN ('claimed','running') RETURNING id")
        .bind(exec)
        .fetch_all(&mut *tx)
        .await?;
    for (id,) in &active {
        event(
            &mut tx,
            *id,
            exec,
            "job.cancel_requested",
            serde_json::json!({}),
        )
        .await?;
    }
    let r = recompute_execution(&mut tx, exec).await?;
    tx.commit().await?;
    Ok(r)
}

/// Expire leases of jobs whose worker stopped heart-beating. Jobs with attempts
/// left are retried; others become `timed_out`. Returns executions that finished.
pub async fn reap_expired_leases(pool: &PgPool) -> DbResult<Vec<(Uuid, ExecutionStatus)>> {
    let mut tx = pool.begin().await?;
    let expired: Vec<(Uuid, Uuid, Uuid, Uuid, Option<Uuid>, String, String, Value, i32, i32, bool, bool)> = sqlx::query_as(
        "SELECT id, execution_id, workspace_id, project_id, scenario_id, scenario_name, kind, spec, attempt, max_attempts, retry_on_failure, cancel_requested
         FROM execution_jobs WHERE status IN ('claimed','running') AND lease_until < now() FOR UPDATE SKIP LOCKED",
    )
    .fetch_all(&mut *tx)
    .await?;
    let mut execs = std::collections::BTreeSet::new();
    for (id, exec, ws, project, scenario_id, name, kind, spec, attempt, max, rof, cancel) in expired
    {
        let status = if cancel { "cancelled" } else { "timed_out" };
        sqlx::query("UPDATE execution_jobs SET status = $2, finished_at = now(), error = 'Worker lease expired (no heartbeat)', lease_until = NULL WHERE id = $1")
            .bind(id)
            .bind(status)
            .execute(&mut *tx)
            .await?;
        event(
            &mut tx,
            id,
            exec,
            "job.lease_expired",
            serde_json::json!({"attempt": attempt}),
        )
        .await?;
        if !cancel && attempt < max {
            insert_retry(
                &mut tx,
                id,
                exec,
                ws,
                project,
                scenario_id,
                &name,
                &kind,
                &spec,
                attempt + 1,
                max,
                rof,
                0,
            )
            .await?;
        }
        execs.insert(exec);
    }
    let mut finished = vec![];
    for e in execs {
        let (st, done) = recompute_execution(&mut tx, e).await?;
        if done {
            finished.push((e, st));
        }
    }
    tx.commit().await?;
    Ok(finished)
}

/// Scenario history for flaky detection and comparison: final status per run, newest first.
#[derive(Debug, Clone, FromRow, Serialize, ToSchema)]
pub struct ScenarioRun {
    pub job_id: Uuid,
    pub execution_id: Uuid,
    pub status: String,
    pub attempt: i32,
    pub superseded: bool,
    pub duration_ms: Option<i64>,
    pub finished_at: Option<DateTime<Utc>>,
    pub commit_sha: Option<String>,
    pub environment_name: String,
}

pub async fn scenario_history(
    pool: &PgPool,
    scenario_id: Uuid,
    limit: i64,
) -> DbResult<Vec<ScenarioRun>> {
    Ok(sqlx::query_as::<_, ScenarioRun>(
        "SELECT j.id AS job_id, j.execution_id, j.status, j.attempt, j.superseded, j.duration_ms, j.finished_at, e.commit_sha, e.environment_name
         FROM execution_jobs j JOIN test_executions e ON e.id = j.execution_id
         WHERE j.scenario_id = $1 AND j.finished_at IS NOT NULL ORDER BY j.finished_at DESC LIMIT $2",
    )
    .bind(scenario_id)
    .bind(limit)
    .fetch_all(pool)
    .await?)
}

// ── workers ──
#[derive(Debug, Clone, FromRow, Serialize, ToSchema)]
pub struct Worker {
    pub id: Uuid,
    pub workspace_id: Option<Uuid>,
    pub name: String,
    pub status: String,
    pub version: Option<String>,
    pub hostname: Option<String>,
    pub os: Option<String>,
    pub concurrency: i32,
    pub labels: Vec<String>,
    pub active_jobs: i32,
    pub last_heartbeat_at: Option<DateTime<Utc>>,
    pub registered_at: DateTime<Utc>,
    pub revoked_at: Option<DateTime<Utc>>,
    pub jobs_completed: i64,
}

#[derive(Debug, Clone, FromRow, Serialize, Deserialize, ToSchema)]
pub struct WorkerCapability {
    pub capability: String,
    pub version: Option<String>,
    pub available: bool,
}

const WORKER_SELECT: &str = "SELECT w.id, w.workspace_id, w.name, w.status, w.version, w.hostname, w.os, w.concurrency, w.labels, w.active_jobs, w.last_heartbeat_at,
  w.registered_at, w.revoked_at, (SELECT count(*) FROM execution_jobs j WHERE j.worker_id = w.id AND j.finished_at IS NOT NULL) AS jobs_completed
  FROM execution_workers w";

#[allow(clippy::too_many_arguments)]
pub async fn register_worker(
    pool: &PgPool,
    workspace_id: Option<Uuid>,
    name: &str,
    token_hash: &str,
    version: &str,
    hostname: &str,
    os: &str,
    concurrency: i32,
    labels: &[String],
    caps: &[WorkerCapability],
) -> DbResult<Uuid> {
    let mut tx = pool.begin().await?;
    let id = new_id();
    sqlx::query(
        "INSERT INTO execution_workers (id, workspace_id, name, token_hash, version, hostname, os, concurrency, labels, last_heartbeat_at)
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9, now())",
    )
    .bind(id)
    .bind(workspace_id)
    .bind(name)
    .bind(token_hash)
    .bind(version)
    .bind(hostname)
    .bind(os)
    .bind(concurrency)
    .bind(labels)
    .execute(&mut *tx)
    .await?;
    upsert_caps(&mut tx, id, caps).await?;
    tx.commit().await?;
    Ok(id)
}

async fn upsert_caps(
    tx: &mut Transaction<'_, Postgres>,
    id: Uuid,
    caps: &[WorkerCapability],
) -> DbResult<()> {
    sqlx::query("DELETE FROM worker_capabilities WHERE worker_id = $1")
        .bind(id)
        .execute(&mut **tx)
        .await?;
    for c in caps {
        sqlx::query("INSERT INTO worker_capabilities (worker_id, capability, version, available) VALUES ($1,$2,$3,$4)")
            .bind(id)
            .bind(&c.capability)
            .bind(&c.version)
            .bind(c.available)
            .execute(&mut **tx)
            .await?;
    }
    Ok(())
}

/// Resolve a worker token. Revoked workers never authenticate.
pub async fn worker_by_token(
    pool: &PgPool,
    token_hash: &str,
) -> DbResult<Option<(Uuid, Option<Uuid>, String)>> {
    Ok(sqlx::query_as("SELECT id, workspace_id, name FROM execution_workers WHERE token_hash = $1 AND status <> 'revoked'")
        .bind(token_hash)
        .fetch_optional(pool)
        .await?)
}

pub async fn worker_heartbeat(
    pool: &PgPool,
    id: Uuid,
    active_jobs: i32,
    caps: Option<&[WorkerCapability]>,
    draining: bool,
) -> DbResult<()> {
    let mut tx = pool.begin().await?;
    sqlx::query(
        "UPDATE execution_workers SET last_heartbeat_at = now(), active_jobs = $2, status = CASE WHEN $3 THEN 'draining' ELSE 'online' END
         WHERE id = $1 AND status <> 'revoked'",
    )
    .bind(id)
    .bind(active_jobs)
    .bind(draining)
    .execute(&mut *tx)
    .await?;
    if let Some(c) = caps {
        upsert_caps(&mut tx, id, c).await?;
    }
    tx.commit().await?;
    Ok(())
}

pub async fn mark_stale_workers_offline(pool: &PgPool, stale_secs: i64) -> DbResult<u64> {
    Ok(sqlx::query(
        "UPDATE execution_workers SET status = 'offline', active_jobs = 0 WHERE status IN ('online','draining') AND last_heartbeat_at < now() - make_interval(secs => $1)",
    )
    .bind(stale_secs as f64)
    .execute(pool)
    .await?
    .rows_affected())
}

/// Workers visible to a workspace: its own plus shared instance workers.
pub async fn list_workers(pool: &PgPool, workspace_id: Uuid) -> DbResult<Vec<Worker>> {
    Ok(sqlx::query_as::<_, Worker>(&format!("{WORKER_SELECT} WHERE w.workspace_id = $1 OR w.workspace_id IS NULL ORDER BY w.status, w.name"))
        .bind(workspace_id)
        .fetch_all(pool)
        .await?)
}

pub async fn get_worker(pool: &PgPool, id: Uuid) -> DbResult<Worker> {
    opt(
        sqlx::query_as::<_, Worker>(&format!("{WORKER_SELECT} WHERE w.id = $1"))
            .bind(id)
            .fetch_one(pool)
            .await,
    )
}

pub async fn worker_capabilities(pool: &PgPool, id: Uuid) -> DbResult<Vec<WorkerCapability>> {
    Ok(sqlx::query_as::<_, WorkerCapability>("SELECT capability, version, available FROM worker_capabilities WHERE worker_id = $1 ORDER BY capability")
        .bind(id)
        .fetch_all(pool)
        .await?)
}

pub async fn worker_jobs(pool: &PgPool, worker_id: Uuid, limit: i64) -> DbResult<Vec<Job>> {
    Ok(sqlx::query_as::<_, Job>(&format!(
        "{JOB_SELECT} WHERE j.worker_id = $1 ORDER BY j.created_at DESC LIMIT $2"
    ))
    .bind(worker_id)
    .bind(limit)
    .fetch_all(pool)
    .await?)
}

pub async fn revoke_worker(pool: &PgPool, id: Uuid) -> DbResult<()> {
    let mut tx = pool.begin().await?;
    sqlx::query(
        "UPDATE execution_workers SET status = 'revoked', revoked_at = now() WHERE id = $1",
    )
    .bind(id)
    .execute(&mut *tx)
    .await?;
    // Its in-flight jobs lose their lease immediately and will be retried elsewhere.
    sqlx::query("UPDATE execution_jobs SET lease_until = now() - interval '1 second' WHERE worker_id = $1 AND status IN ('claimed','running')")
        .bind(id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(())
}

/// Count of queued jobs (for the Workers screen).
pub async fn queue_depth(pool: &PgPool, workspace_id: Uuid) -> DbResult<(i64, i64)> {
    Ok(sqlx::query_as(
        "SELECT count(*) FILTER (WHERE status = 'queued'), count(*) FILTER (WHERE status IN ('claimed','running')) FROM execution_jobs WHERE workspace_id = $1",
    )
    .bind(workspace_id)
    .fetch_one(pool)
    .await?)
}

/// Refresh self-reported worker metadata (pre-registered workers report it on first heartbeat).
pub async fn update_worker_info(
    pool: &PgPool,
    id: Uuid,
    version: &str,
    hostname: &str,
    os: &str,
    concurrency: Option<i32>,
) -> DbResult<()> {
    sqlx::query("UPDATE execution_workers SET version = $2, hostname = $3, os = $4, concurrency = COALESCE($5, concurrency) WHERE id = $1")
        .bind(id)
        .bind(version)
        .bind(hostname)
        .bind(os)
        .bind(concurrency)
        .execute(pool)
        .await?;
    Ok(())
}

/// The retry policy an execution's jobs were created with: (max_attempts, retry_on_failure).
pub async fn execution_retry_policy(pool: &PgPool, exec: Uuid) -> DbResult<Option<(i32, bool)>> {
    Ok(sqlx::query_as(
        "SELECT max_attempts, retry_on_failure FROM execution_jobs WHERE execution_id = $1 ORDER BY created_at LIMIT 1",
    )
    .bind(exec)
    .fetch_optional(pool)
    .await?)
}

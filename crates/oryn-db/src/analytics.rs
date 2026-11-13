//! Quality metrics. Every formula here is documented in `docs/METRICS.md`;
//! change both together. Nothing is estimated: metrics are computed from stored
//! jobs, scenarios and requirements only.

use chrono::{DateTime, NaiveDate, Utc};
use serde::Serialize;
use sqlx::{FromRow, PgPool};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::DbResult;

#[derive(Debug, Clone, Serialize, ToSchema, Default)]
pub struct Overview {
    pub window_days: i32,
    pub projects: i64,
    pub suites: i64,
    pub scenarios: i64,
    pub scenarios_approved: i64,
    pub scenarios_browser: i64,
    pub scenarios_api: i64,
    pub executions: i64,
    pub executions_active: i64,
    /// Final-attempt job outcomes in the window.
    pub jobs_passed: i64,
    pub jobs_failed: i64,
    pub jobs_errored: i64,
    /// passed / (passed + failed + errored); null when the denominator is 0.
    pub pass_rate: Option<f64>,
    pub failure_rate: Option<f64>,
    /// Mean duration of finished executions in the window (ms).
    pub avg_execution_ms: Option<f64>,
    /// Mean duration of final-attempt jobs (ms).
    pub avg_scenario_ms: Option<f64>,
    pub flaky_scenarios: i64,
}

/// `projects`: the caller-visible project ids (tenant + project-level access already applied).
pub async fn overview(pool: &PgPool, projects: &[Uuid], days: i32) -> DbResult<Overview> {
    let (suites, scenarios, approved, browser, api): (i64, i64, i64, i64, i64) = sqlx::query_as(
        "SELECT (SELECT count(*) FROM test_suites WHERE project_id = ANY($1) AND deleted_at IS NULL),
                count(*), count(*) FILTER (WHERE status = 'approved'), count(*) FILTER (WHERE kind IN ('browser','mixed')), count(*) FILTER (WHERE kind = 'api')
         FROM test_scenarios WHERE project_id = ANY($1) AND deleted_at IS NULL",
    )
    .bind(projects)
    .fetch_one(pool)
    .await?;
    let (executions, active, avg_exec): (i64, i64, Option<f64>) = sqlx::query_as(
        "SELECT count(*), count(*) FILTER (WHERE status IN ('queued','running')), (avg(duration_ms) FILTER (WHERE finished_at IS NOT NULL))::float8
         FROM test_executions WHERE project_id = ANY($1) AND created_at > now() - make_interval(days => $2)",
    )
    .bind(projects)
    .bind(days)
    .fetch_one(pool)
    .await?;
    let (p, f, e, avg_job): (i64, i64, i64, Option<f64>) = sqlx::query_as(
        "SELECT count(*) FILTER (WHERE status = 'passed'), count(*) FILTER (WHERE status = 'failed'), count(*) FILTER (WHERE status IN ('error','timed_out')),
                avg(duration_ms)::float8
         FROM execution_jobs WHERE project_id = ANY($1) AND NOT superseded AND finished_at > now() - make_interval(days => $2) AND status <> 'cancelled'",
    )
    .bind(projects)
    .bind(days)
    .fetch_one(pool)
    .await?;
    let denom = p + f + e;
    let flaky = flaky_scenarios(pool, projects, days).await?.len() as i64;
    Ok(Overview {
        window_days: days,
        projects: projects.len() as i64,
        suites,
        scenarios,
        scenarios_approved: approved,
        scenarios_browser: browser,
        scenarios_api: api,
        executions,
        executions_active: active,
        jobs_passed: p,
        jobs_failed: f,
        jobs_errored: e,
        pass_rate: (denom > 0).then(|| p as f64 / denom as f64),
        failure_rate: (denom > 0).then(|| (f + e) as f64 / denom as f64),
        avg_execution_ms: avg_exec,
        avg_scenario_ms: avg_job,
        flaky_scenarios: flaky,
    })
}

#[derive(Debug, Clone, FromRow, Serialize, ToSchema)]
pub struct TrendPoint {
    pub day: NaiveDate,
    pub executions: i64,
    pub passed: i64,
    pub failed: i64,
    pub errored: i64,
    pub avg_duration_ms: Option<f64>,
}

pub async fn trends(pool: &PgPool, projects: &[Uuid], days: i32) -> DbResult<Vec<TrendPoint>> {
    Ok(sqlx::query_as::<_, TrendPoint>(
        "WITH d AS (SELECT generate_series((now() - make_interval(days => $2 - 1))::date, now()::date, interval '1 day')::date AS day),
         j AS (SELECT finished_at::date AS day, status, duration_ms FROM execution_jobs
               WHERE project_id = ANY($1) AND NOT superseded AND finished_at > now() - make_interval(days => $2)),
         e AS (SELECT created_at::date AS day, count(*) AS n FROM test_executions WHERE project_id = ANY($1) AND created_at > now() - make_interval(days => $2) GROUP BY 1)
         SELECT d.day, COALESCE(e.n, 0) AS executions,
            (SELECT count(*) FROM j WHERE j.day = d.day AND j.status = 'passed') AS passed,
            (SELECT count(*) FROM j WHERE j.day = d.day AND j.status = 'failed') AS failed,
            (SELECT count(*) FROM j WHERE j.day = d.day AND j.status IN ('error','timed_out')) AS errored,
            (SELECT avg(duration_ms)::float8 FROM j WHERE j.day = d.day) AS avg_duration_ms
         FROM d LEFT JOIN e ON e.day = d.day ORDER BY d.day",
    )
    .bind(projects)
    .bind(days)
    .fetch_all(pool)
    .await?)
}

#[derive(Debug, Clone, FromRow, Serialize, ToSchema)]
pub struct FlakyScenario {
    pub scenario_id: Uuid,
    pub scenario_name: String,
    pub project_id: Uuid,
    pub runs: i64,
    pub passes: i64,
    pub failures: i64,
    /// Number of pass↔fail transitions across consecutive final runs.
    pub flips: i64,
    /// Executions where an attempt failed and a retry then passed.
    pub passed_on_retry: i64,
    pub flakiness: f64,
    pub last_run_at: Option<DateTime<Utc>>,
}

/// Flaky = within the window, either (a) passed on retry after a failed/errored attempt
/// in the same execution at least once, or (b) at least 2 pass↔fail flips across
/// consecutive final runs since the scenario was last edited.
/// flakiness = flips / (runs - 1), or 1.0 when only (a) applies with ≤ 2 runs.
pub async fn flaky_scenarios(
    pool: &PgPool,
    projects: &[Uuid],
    days: i32,
) -> DbResult<Vec<FlakyScenario>> {
    Ok(sqlx::query_as::<_, FlakyScenario>(
        "WITH runs AS (
            SELECT j.scenario_id, j.project_id, j.status, j.finished_at, j.execution_id, j.superseded,
                   lag(CASE WHEN j.status = 'passed' THEN 1 ELSE 0 END) OVER (PARTITION BY j.scenario_id ORDER BY j.finished_at) AS prev_pass
            FROM execution_jobs j JOIN test_scenarios s ON s.id = j.scenario_id
            WHERE j.project_id = ANY($1) AND j.finished_at > now() - make_interval(days => $2) AND NOT j.superseded
              AND j.status IN ('passed','failed','error','timed_out') AND j.finished_at > s.updated_at AND s.deleted_at IS NULL),
         agg AS (
            SELECT scenario_id, project_id, count(*) AS runs, count(*) FILTER (WHERE status = 'passed') AS passes,
                   count(*) FILTER (WHERE status <> 'passed') AS failures,
                   count(*) FILTER (WHERE prev_pass IS NOT NULL AND prev_pass <> CASE WHEN status = 'passed' THEN 1 ELSE 0 END) AS flips,
                   max(finished_at) AS last_run_at
            FROM runs GROUP BY scenario_id, project_id),
         retry AS (
            SELECT f.scenario_id, count(DISTINCT f.execution_id) AS passed_on_retry FROM execution_jobs f
            JOIN execution_jobs p ON p.execution_id = f.execution_id AND p.scenario_id = f.scenario_id AND p.attempt > f.attempt AND p.status = 'passed' AND NOT p.superseded
            WHERE f.project_id = ANY($1) AND f.superseded AND f.status IN ('failed','error','timed_out') AND f.finished_at > now() - make_interval(days => $2)
            GROUP BY f.scenario_id)
         SELECT s.id AS scenario_id, s.name AS scenario_name, s.project_id, COALESCE(a.runs,0) AS runs, COALESCE(a.passes,0) AS passes,
                COALESCE(a.failures,0) AS failures, COALESCE(a.flips,0) AS flips, COALESCE(r.passed_on_retry,0) AS passed_on_retry,
                CASE WHEN COALESCE(a.runs,0) > 2 THEN a.flips::float8 / (a.runs - 1) ELSE 1.0 END AS flakiness, a.last_run_at
         FROM test_scenarios s LEFT JOIN agg a ON a.scenario_id = s.id LEFT JOIN retry r ON r.scenario_id = s.id
         WHERE s.project_id = ANY($1) AND s.deleted_at IS NULL AND (COALESCE(a.flips,0) >= 2 OR COALESCE(r.passed_on_retry,0) > 0)
         ORDER BY flakiness DESC, a.runs DESC",
    )
    .bind(projects)
    .bind(days)
    .fetch_all(pool)
    .await?)
}

#[derive(Debug, Clone, FromRow, Serialize, ToSchema)]
pub struct RecentFailure {
    pub job_id: Uuid,
    pub execution_id: Uuid,
    pub project_id: Uuid,
    pub project_name: String,
    pub scenario_id: Option<Uuid>,
    pub scenario_name: String,
    pub status: String,
    pub message: Option<String>,
    pub finished_at: Option<DateTime<Utc>>,
    pub investigated: bool,
}

pub async fn recent_failures(
    pool: &PgPool,
    projects: &[Uuid],
    limit: i64,
) -> DbResult<Vec<RecentFailure>> {
    Ok(sqlx::query_as::<_, RecentFailure>(
        "SELECT j.id AS job_id, j.execution_id, j.project_id, p.name AS project_name, j.scenario_id, j.scenario_name, j.status,
            COALESCE((SELECT st.message FROM test_execution_steps st WHERE st.job_id = j.id AND st.status IN ('failed','error') ORDER BY st.position LIMIT 1), j.error) AS message,
            j.finished_at, EXISTS(SELECT 1 FROM failure_investigations fi WHERE fi.job_id = j.id) AS investigated
         FROM execution_jobs j JOIN projects p ON p.id = j.project_id
         WHERE j.project_id = ANY($1) AND NOT j.superseded AND j.status IN ('failed','error','timed_out')
         ORDER BY j.finished_at DESC NULLS LAST LIMIT $2",
    )
    .bind(projects)
    .bind(limit)
    .fetch_all(pool)
    .await?)
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct RequirementCoverage {
    pub requirements: i64,
    /// Requirements linked to ≥1 approved, executable scenario.
    pub covered: i64,
    /// Covered requirements whose linked scenarios' latest final runs all passed.
    pub verified: i64,
    /// Covered requirements with at least one linked scenario whose latest run failed.
    pub failing: i64,
    pub coverage_rate: Option<f64>,
    pub verified_rate: Option<f64>,
    pub rows: Vec<RequirementCoverageRow>,
}

#[derive(Debug, Clone, FromRow, Serialize, ToSchema)]
pub struct RequirementCoverageRow {
    pub requirement_id: Uuid,
    pub key: String,
    pub title: String,
    pub priority: String,
    pub scenarios: i64,
    pub executable_scenarios: i64,
    pub passing: i64,
    pub failing: i64,
    pub never_run: i64,
}

pub async fn requirement_coverage(
    pool: &PgPool,
    project_id: Uuid,
) -> DbResult<RequirementCoverage> {
    let rows = sqlx::query_as::<_, RequirementCoverageRow>(
        "WITH latest AS (
            SELECT DISTINCT ON (j.scenario_id) j.scenario_id, j.status FROM execution_jobs j
            WHERE j.project_id = $1 AND NOT j.superseded AND j.finished_at IS NOT NULL AND j.status <> 'cancelled' ORDER BY j.scenario_id, j.finished_at DESC)
         SELECT r.id AS requirement_id, r.key, r.title, r.priority,
            count(s.id) AS scenarios,
            count(s.id) FILTER (WHERE s.status = 'approved' AND s.executable) AS executable_scenarios,
            count(s.id) FILTER (WHERE s.status = 'approved' AND s.executable AND l2.status = 'passed') AS passing,
            count(s.id) FILTER (WHERE s.status = 'approved' AND s.executable AND l2.status IN ('failed','error','timed_out')) AS failing,
            count(s.id) FILTER (WHERE s.status = 'approved' AND s.executable AND l2.status IS NULL) AS never_run
         FROM requirements r
         LEFT JOIN requirement_links l ON l.requirement_id = r.id
         LEFT JOIN test_scenarios s ON s.id = l.scenario_id AND s.deleted_at IS NULL
         LEFT JOIN latest l2 ON l2.scenario_id = s.id
         WHERE r.project_id = $1 AND r.deleted_at IS NULL
         GROUP BY r.id ORDER BY r.key",
    )
    .bind(project_id)
    .fetch_all(pool)
    .await?;
    let n = rows.len() as i64;
    let covered = rows.iter().filter(|r| r.executable_scenarios > 0).count() as i64;
    let verified = rows
        .iter()
        .filter(|r| r.executable_scenarios > 0 && r.passing == r.executable_scenarios)
        .count() as i64;
    let failing = rows.iter().filter(|r| r.failing > 0).count() as i64;
    Ok(RequirementCoverage {
        requirements: n,
        covered,
        verified,
        failing,
        coverage_rate: (n > 0).then(|| covered as f64 / n as f64),
        verified_rate: (n > 0).then(|| verified as f64 / n as f64),
        rows,
    })
}

#[derive(Debug, Clone, FromRow, Serialize, ToSchema)]
pub struct ScenarioStat {
    pub scenario_id: Uuid,
    pub scenario_name: String,
    pub suite_name: String,
    pub runs: i64,
    pub passes: i64,
    pub failures: i64,
    pub avg_duration_ms: Option<f64>,
    pub last_status: Option<String>,
}

pub async fn scenario_stats(
    pool: &PgPool,
    project_id: Uuid,
    days: i32,
) -> DbResult<Vec<ScenarioStat>> {
    Ok(sqlx::query_as::<_, ScenarioStat>(
        "SELECT s.id AS scenario_id, s.name AS scenario_name, su.name AS suite_name,
            count(j.id) AS runs, count(j.id) FILTER (WHERE j.status = 'passed') AS passes,
            count(j.id) FILTER (WHERE j.status IN ('failed','error','timed_out')) AS failures, avg(j.duration_ms)::float8 AS avg_duration_ms,
            (SELECT x.status FROM execution_jobs x WHERE x.scenario_id = s.id AND NOT x.superseded AND x.finished_at IS NOT NULL ORDER BY x.finished_at DESC LIMIT 1) AS last_status
         FROM test_scenarios s JOIN test_suites su ON su.id = s.suite_id
         LEFT JOIN execution_jobs j ON j.scenario_id = s.id AND NOT j.superseded AND j.finished_at > now() - make_interval(days => $2) AND j.status <> 'cancelled'
         WHERE s.project_id = $1 AND s.deleted_at IS NULL GROUP BY s.id, su.name ORDER BY failures DESC, runs DESC",
    )
    .bind(project_id)
    .bind(days)
    .fetch_all(pool)
    .await?)
}

/// Per-scenario comparison of two executions.
#[derive(Debug, Clone, FromRow, Serialize, ToSchema)]
pub struct ComparisonRow {
    pub scenario_name: String,
    pub scenario_id: Option<Uuid>,
    pub base_status: Option<String>,
    pub head_status: Option<String>,
    pub base_duration_ms: Option<i64>,
    pub head_duration_ms: Option<i64>,
    pub base_job_id: Option<Uuid>,
    pub head_job_id: Option<Uuid>,
}

pub async fn compare_executions(
    pool: &PgPool,
    base: Uuid,
    head: Uuid,
) -> DbResult<Vec<ComparisonRow>> {
    Ok(sqlx::query_as::<_, ComparisonRow>(
        "WITH b AS (SELECT scenario_name, scenario_id, status, duration_ms, id FROM execution_jobs WHERE execution_id = $1 AND NOT superseded),
              h AS (SELECT scenario_name, scenario_id, status, duration_ms, id FROM execution_jobs WHERE execution_id = $2 AND NOT superseded)
         SELECT COALESCE(h.scenario_name, b.scenario_name) AS scenario_name, COALESCE(h.scenario_id, b.scenario_id) AS scenario_id,
                b.status AS base_status, h.status AS head_status, b.duration_ms AS base_duration_ms, h.duration_ms AS head_duration_ms,
                b.id AS base_job_id, h.id AS head_job_id
         FROM b FULL OUTER JOIN h ON h.scenario_name = b.scenario_name ORDER BY 1",
    )
    .bind(base)
    .bind(head)
    .fetch_all(pool)
    .await?)
}

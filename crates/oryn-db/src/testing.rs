//! Test suites (feature files), scenarios, steps, plans, step definitions and recordings.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::{FromRow, PgPool};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::{new_id, opt, DbError, DbResult};

#[derive(Debug, Clone, FromRow, Serialize, ToSchema)]
pub struct Suite {
    pub id: Uuid,
    pub project_id: Uuid,
    pub name: String,
    pub description: String,
    pub path: Option<String>,
    pub feature_source: String,
    pub tags: Vec<String>,
    pub parse_error: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, FromRow, Serialize, ToSchema)]
pub struct SuiteSummary {
    pub id: Uuid,
    pub project_id: Uuid,
    pub name: String,
    pub description: String,
    pub path: Option<String>,
    pub tags: Vec<String>,
    pub parse_error: Option<String>,
    pub updated_at: DateTime<Utc>,
    pub scenario_count: i64,
    pub approved_count: i64,
    pub last_status: Option<String>,
}

#[derive(Debug, Clone, FromRow, Serialize, ToSchema)]
pub struct Scenario {
    pub id: Uuid,
    pub suite_id: Uuid,
    pub project_id: Uuid,
    pub name: String,
    pub tags: Vec<String>,
    pub kind: String,
    pub status: String,
    pub origin: String,
    pub line: i32,
    pub outline: Option<String>,
    pub example_index: Option<i32>,
    pub executable: bool,
    #[schema(value_type = Object)]
    pub diagnostics: Value,
    pub position: i32,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    /// Status of the most recent final job for this scenario.
    pub last_status: Option<String>,
    pub last_run_at: Option<DateTime<Utc>>,
    pub suite_name: String,
}

#[derive(Debug, Clone, FromRow, Serialize, ToSchema)]
pub struct StepRow {
    pub position: i32,
    pub keyword: String,
    pub text: String,
    #[schema(value_type = Object)]
    pub step: Value,
}

const SUITE_COLS: &str = "id, project_id, name, description, path, feature_source, tags, parse_error, created_at, updated_at";

pub async fn list_suites(pool: &PgPool, project_id: Uuid) -> DbResult<Vec<SuiteSummary>> {
    Ok(sqlx::query_as::<_, SuiteSummary>(
        "SELECT s.id, s.project_id, s.name, s.description, s.path, s.tags, s.parse_error, s.updated_at,
           (SELECT count(*) FROM test_scenarios c WHERE c.suite_id = s.id AND c.deleted_at IS NULL) AS scenario_count,
           (SELECT count(*) FROM test_scenarios c WHERE c.suite_id = s.id AND c.deleted_at IS NULL AND c.status = 'approved') AS approved_count,
           (SELECT e.status FROM test_executions e WHERE e.suite_id = s.id ORDER BY e.created_at DESC LIMIT 1) AS last_status
         FROM test_suites s WHERE s.project_id = $1 AND s.deleted_at IS NULL ORDER BY s.name",
    )
    .bind(project_id)
    .fetch_all(pool)
    .await?)
}

pub async fn get_suite(pool: &PgPool, project_id: Uuid, id: Uuid) -> DbResult<Suite> {
    opt(sqlx::query_as::<_, Suite>(&format!("SELECT {SUITE_COLS} FROM test_suites WHERE id = $1 AND project_id = $2 AND deleted_at IS NULL"))
        .bind(id)
        .bind(project_id)
        .fetch_one(pool)
        .await)
}

/// A compiled scenario to persist (built by the API from `oryn_bdd::CompiledScenario`).
#[derive(Debug, Clone)]
pub struct ScenarioInput {
    pub name: String,
    pub tags: Vec<String>,
    pub kind: String,
    pub line: i32,
    pub outline: Option<String>,
    pub example_index: Option<i32>,
    pub executable: bool,
    pub diagnostics: Value,
    pub steps: Vec<(String, String, Value)>,
}

/// Create or update a suite and synchronise its scenarios from the compiled source.
/// Scenarios are matched by name so ids, approval status and history survive edits;
/// scenarios removed from the source are soft-deleted. `@REQ-n` tags link requirements.
#[allow(clippy::too_many_arguments)]
pub async fn save_suite(
    pool: &PgPool,
    project_id: Uuid,
    suite_id: Option<Uuid>,
    name: &str,
    description: &str,
    path: Option<&str>,
    source: &str,
    tags: &[String],
    parse_error: Option<&str>,
    scenarios: &[ScenarioInput],
    new_status: &str,
    origin: &str,
    user: Option<Uuid>,
) -> DbResult<Uuid> {
    let mut tx = pool.begin().await?;
    let id = match suite_id {
        None => {
            let id = new_id();
            sqlx::query(
                "INSERT INTO test_suites (id, project_id, name, description, path, feature_source, tags, parse_error, created_by) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9)",
            )
            .bind(id)
            .bind(project_id)
            .bind(name)
            .bind(description)
            .bind(path)
            .bind(source)
            .bind(tags)
            .bind(parse_error)
            .bind(user)
            .execute(&mut *tx)
            .await?;
            id
        }
        Some(id) => {
            let r = sqlx::query(
                "UPDATE test_suites SET name=$3, description=$4, path=$5, feature_source=$6, tags=$7, parse_error=$8, updated_at=now()
                 WHERE id=$1 AND project_id=$2 AND deleted_at IS NULL",
            )
            .bind(id)
            .bind(project_id)
            .bind(name)
            .bind(description)
            .bind(path)
            .bind(source)
            .bind(tags)
            .bind(parse_error)
            .execute(&mut *tx)
            .await?;
            if r.rows_affected() == 0 {
                return Err(DbError::NotFound);
            }
            id
        }
    };

    // A parse error keeps the previous scenarios intact.
    if parse_error.is_none() {
        let existing: Vec<(Uuid, String)> = sqlx::query_as(
            "SELECT id, name FROM test_scenarios WHERE suite_id = $1 AND deleted_at IS NULL",
        )
        .bind(id)
        .fetch_all(&mut *tx)
        .await?;
        let mut keep = Vec::new();
        for (pos, sc) in scenarios.iter().enumerate() {
            let sid = match existing.iter().find(|(_, n)| n == &sc.name) {
                Some((sid, _)) => {
                    sqlx::query(
                        "UPDATE test_scenarios SET tags=$2, kind=$3, line=$4, outline=$5, example_index=$6, executable=$7, diagnostics=$8, position=$9, updated_at=now() WHERE id=$1",
                    )
                    .bind(sid)
                    .bind(&sc.tags)
                    .bind(&sc.kind)
                    .bind(sc.line)
                    .bind(&sc.outline)
                    .bind(sc.example_index)
                    .bind(sc.executable)
                    .bind(&sc.diagnostics)
                    .bind(pos as i32)
                    .execute(&mut *tx)
                    .await?;
                    *sid
                }
                None => {
                    let sid = new_id();
                    sqlx::query(
                        "INSERT INTO test_scenarios (id, suite_id, project_id, name, tags, kind, status, origin, line, outline, example_index, executable, diagnostics, position)
                         VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14)",
                    )
                    .bind(sid)
                    .bind(id)
                    .bind(project_id)
                    .bind(&sc.name)
                    .bind(&sc.tags)
                    .bind(&sc.kind)
                    .bind(new_status)
                    .bind(origin)
                    .bind(sc.line)
                    .bind(&sc.outline)
                    .bind(sc.example_index)
                    .bind(sc.executable)
                    .bind(&sc.diagnostics)
                    .bind(pos as i32)
                    .execute(&mut *tx)
                    .await?;
                    sid
                }
            };
            sqlx::query("DELETE FROM test_steps WHERE scenario_id = $1")
                .bind(sid)
                .execute(&mut *tx)
                .await?;
            for (i, (kw, text, step)) in sc.steps.iter().enumerate() {
                sqlx::query("INSERT INTO test_steps (id, scenario_id, position, keyword, text, step) VALUES ($1,$2,$3,$4,$5,$6)")
                    .bind(new_id())
                    .bind(sid)
                    .bind(i as i32)
                    .bind(kw)
                    .bind(text)
                    .bind(step)
                    .execute(&mut *tx)
                    .await?;
            }
            // Any tag equal to a requirement key (e.g. @REQ-3, @US-8) links the scenario.
            let keys: Vec<String> = sc
                .tags
                .iter()
                .map(|t| t.trim_start_matches('@').to_string())
                .filter(|t| t.contains('-'))
                .collect();
            if !keys.is_empty() {
                sqlx::query(
                    "INSERT INTO requirement_links (requirement_id, scenario_id, source)
                     SELECT r.id, $2, 'tag' FROM requirements r WHERE r.project_id = $3 AND upper(r.key) = ANY(SELECT upper(k) FROM unnest($1::text[]) k) AND r.deleted_at IS NULL
                     ON CONFLICT DO NOTHING",
                )
                .bind(&keys)
                .bind(sid)
                .bind(project_id)
                .execute(&mut *tx)
                .await?;
            }
            keep.push(sid);
        }
        sqlx::query("UPDATE test_scenarios SET deleted_at = now() WHERE suite_id = $1 AND deleted_at IS NULL AND NOT (id = ANY($2))")
            .bind(id)
            .bind(&keep)
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;
    Ok(id)
}

pub async fn delete_suite(pool: &PgPool, project_id: Uuid, id: Uuid) -> DbResult<()> {
    let mut tx = pool.begin().await?;
    sqlx::query("UPDATE test_suites SET deleted_at = now() WHERE id = $1 AND project_id = $2")
        .bind(id)
        .bind(project_id)
        .execute(&mut *tx)
        .await?;
    sqlx::query(
        "UPDATE test_scenarios SET deleted_at = now() WHERE suite_id = $1 AND deleted_at IS NULL",
    )
    .bind(id)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(())
}

const SCENARIO_SELECT: &str = "SELECT s.id, s.suite_id, s.project_id, s.name, s.tags, s.kind, s.status, s.origin, s.line, s.outline, s.example_index,
   s.executable, s.diagnostics, s.position, s.created_at, s.updated_at, lj.status AS last_status, lj.finished_at AS last_run_at, su.name AS suite_name
   FROM test_scenarios s JOIN test_suites su ON su.id = s.suite_id
   LEFT JOIN LATERAL (SELECT j.status, j.finished_at FROM execution_jobs j WHERE j.scenario_id = s.id AND NOT j.superseded AND j.finished_at IS NOT NULL
                      ORDER BY j.finished_at DESC LIMIT 1) lj ON true";

#[derive(Debug, Default, Clone, Deserialize)]
pub struct ScenarioFilter {
    pub suite_id: Option<Uuid>,
    pub status: Option<String>,
    pub kind: Option<String>,
    pub tag: Option<String>,
    pub q: Option<String>,
}

pub async fn list_scenarios(
    pool: &PgPool,
    project_id: Uuid,
    f: &ScenarioFilter,
) -> DbResult<Vec<Scenario>> {
    Ok(sqlx::query_as::<_, Scenario>(&format!(
        "{SCENARIO_SELECT} WHERE s.project_id = $1 AND s.deleted_at IS NULL
           AND ($2::uuid IS NULL OR s.suite_id = $2) AND ($3::text IS NULL OR s.status = $3) AND ($4::text IS NULL OR s.kind = $4)
           AND ($5::text IS NULL OR $5 = ANY(s.tags)) AND ($6::text IS NULL OR s.name ILIKE '%' || $6 || '%')
         ORDER BY su.name, s.position"
    ))
    .bind(project_id)
    .bind(f.suite_id)
    .bind(&f.status)
    .bind(&f.kind)
    .bind(&f.tag)
    .bind(&f.q)
    .fetch_all(pool)
    .await?)
}

pub async fn get_scenario(pool: &PgPool, project_id: Uuid, id: Uuid) -> DbResult<Scenario> {
    opt(sqlx::query_as::<_, Scenario>(&format!(
        "{SCENARIO_SELECT} WHERE s.id = $1 AND s.project_id = $2 AND s.deleted_at IS NULL"
    ))
    .bind(id)
    .bind(project_id)
    .fetch_one(pool)
    .await)
}

pub async fn scenario_steps(pool: &PgPool, scenario_id: Uuid) -> DbResult<Vec<StepRow>> {
    Ok(sqlx::query_as::<_, StepRow>("SELECT position, keyword, text, step FROM test_steps WHERE scenario_id = $1 ORDER BY position")
        .bind(scenario_id)
        .fetch_all(pool)
        .await?)
}

pub async fn set_scenario_status(
    pool: &PgPool,
    project_id: Uuid,
    ids: &[Uuid],
    status: &str,
) -> DbResult<u64> {
    Ok(sqlx::query("UPDATE test_scenarios SET status = $3, updated_at = now() WHERE project_id = $1 AND id = ANY($2) AND deleted_at IS NULL")
        .bind(project_id)
        .bind(ids)
        .bind(status)
        .execute(pool)
        .await?
        .rows_affected())
}

pub async fn scenario_requirements(
    pool: &PgPool,
    scenario_id: Uuid,
) -> DbResult<Vec<(Uuid, String, String)>> {
    Ok(sqlx::query_as(
        "SELECT r.id, r.key, r.title FROM requirement_links l JOIN requirements r ON r.id = l.requirement_id WHERE l.scenario_id = $1 AND r.deleted_at IS NULL ORDER BY r.key",
    )
    .bind(scenario_id)
    .fetch_all(pool)
    .await?)
}

// ── plans ──
#[derive(Debug, Clone, FromRow, Serialize, ToSchema)]
pub struct TestPlan {
    pub id: Uuid,
    pub project_id: Uuid,
    pub title: String,
    pub objective: String,
    pub input: String,
    pub status: String,
    pub source: String,
    pub rationale: String,
    #[schema(value_type = Object)]
    pub risk_analysis: Value,
    #[schema(value_type = Object)]
    pub coverage_gaps: Value,
    pub ai_request_id: Option<Uuid>,
    pub approved_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub item_count: i64,
}

#[derive(Debug, Clone, FromRow, Serialize, ToSchema)]
pub struct TestPlanItem {
    pub id: Uuid,
    pub plan_id: Uuid,
    pub position: i32,
    pub title: String,
    pub category: String,
    pub priority: String,
    pub risk: String,
    pub rationale: String,
    pub requirement_keys: Vec<String>,
    pub gherkin: String,
    pub executable: bool,
    #[schema(value_type = Object)]
    pub diagnostics: Value,
    pub status: String,
    pub scenario_id: Option<Uuid>,
}

#[derive(Debug, Clone)]
pub struct PlanItemInput {
    pub title: String,
    pub category: String,
    pub priority: String,
    pub risk: String,
    pub rationale: String,
    pub requirement_keys: Vec<String>,
    pub gherkin: String,
    pub executable: bool,
    pub diagnostics: Value,
}

const PLAN_SELECT: &str = "SELECT p.id, p.project_id, p.title, p.objective, p.input, p.status, p.source, p.rationale, p.risk_analysis, p.coverage_gaps,
  p.ai_request_id, p.approved_at, p.created_at, (SELECT count(*) FROM test_plan_items i WHERE i.plan_id = p.id) AS item_count FROM test_plans p";

#[allow(clippy::too_many_arguments)]
pub async fn create_plan(
    pool: &PgPool,
    project_id: Uuid,
    title: &str,
    objective: &str,
    input: &str,
    source: &str,
    rationale: &str,
    risks: &Value,
    gaps: &Value,
    ai_request_id: Option<Uuid>,
    items: &[PlanItemInput],
    user: Uuid,
) -> DbResult<Uuid> {
    let mut tx = pool.begin().await?;
    let id = new_id();
    sqlx::query(
        "INSERT INTO test_plans (id, project_id, title, objective, input, source, rationale, risk_analysis, coverage_gaps, ai_request_id, created_by)
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11)",
    )
    .bind(id)
    .bind(project_id)
    .bind(title)
    .bind(objective)
    .bind(input)
    .bind(source)
    .bind(rationale)
    .bind(risks)
    .bind(gaps)
    .bind(ai_request_id)
    .bind(user)
    .execute(&mut *tx)
    .await?;
    for (i, it) in items.iter().enumerate() {
        sqlx::query(
            "INSERT INTO test_plan_items (id, plan_id, position, title, category, priority, risk, rationale, requirement_keys, gherkin, executable, diagnostics)
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12)",
        )
        .bind(new_id())
        .bind(id)
        .bind(i as i32)
        .bind(&it.title)
        .bind(&it.category)
        .bind(&it.priority)
        .bind(&it.risk)
        .bind(&it.rationale)
        .bind(&it.requirement_keys)
        .bind(&it.gherkin)
        .bind(it.executable)
        .bind(&it.diagnostics)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    Ok(id)
}

pub async fn list_plans(pool: &PgPool, project_id: Uuid) -> DbResult<Vec<TestPlan>> {
    Ok(sqlx::query_as::<_, TestPlan>(&format!(
        "{PLAN_SELECT} WHERE p.project_id = $1 ORDER BY p.created_at DESC"
    ))
    .bind(project_id)
    .fetch_all(pool)
    .await?)
}

pub async fn get_plan(pool: &PgPool, project_id: Uuid, id: Uuid) -> DbResult<TestPlan> {
    opt(sqlx::query_as::<_, TestPlan>(&format!(
        "{PLAN_SELECT} WHERE p.id = $1 AND p.project_id = $2"
    ))
    .bind(id)
    .bind(project_id)
    .fetch_one(pool)
    .await)
}

pub async fn plan_items(pool: &PgPool, plan_id: Uuid) -> DbResult<Vec<TestPlanItem>> {
    Ok(sqlx::query_as::<_, TestPlanItem>(
        "SELECT id, plan_id, position, title, category, priority, risk, rationale, requirement_keys, gherkin, executable, diagnostics, status, scenario_id
         FROM test_plan_items WHERE plan_id = $1 ORDER BY position",
    )
    .bind(plan_id)
    .fetch_all(pool)
    .await?)
}

pub async fn update_plan_item(
    pool: &PgPool,
    plan_id: Uuid,
    item_id: Uuid,
    gherkin: Option<&str>,
    executable: Option<bool>,
    diagnostics: Option<&Value>,
    status: Option<&str>,
    scenario_id: Option<Uuid>,
) -> DbResult<()> {
    let r = sqlx::query(
        "UPDATE test_plan_items SET gherkin = COALESCE($3, gherkin), executable = COALESCE($4, executable), diagnostics = COALESCE($5, diagnostics),
         status = COALESCE($6, status), scenario_id = COALESCE($7, scenario_id) WHERE id = $2 AND plan_id = $1",
    )
    .bind(plan_id)
    .bind(item_id)
    .bind(gherkin)
    .bind(executable)
    .bind(diagnostics)
    .bind(status)
    .bind(scenario_id)
    .execute(pool)
    .await?;
    if r.rows_affected() == 0 {
        return Err(DbError::NotFound);
    }
    Ok(())
}

pub async fn set_plan_status(
    pool: &PgPool,
    project_id: Uuid,
    id: Uuid,
    status: &str,
    user: Uuid,
) -> DbResult<()> {
    sqlx::query(
        "UPDATE test_plans SET status = $3, approved_by = CASE WHEN $3 = 'approved' THEN $4 ELSE approved_by END,
         approved_at = CASE WHEN $3 = 'approved' THEN now() ELSE approved_at END WHERE id = $1 AND project_id = $2",
    )
    .bind(id)
    .bind(project_id)
    .bind(status)
    .bind(user)
    .execute(pool)
    .await?;
    Ok(())
}

/// Atomically move a plan from `from` to `to`. Returns false when the plan was not
/// in `from` (for example, another reviewer approved it first).
pub async fn transition_plan(
    pool: &PgPool,
    project_id: Uuid,
    id: Uuid,
    from: &str,
    to: &str,
    user: Uuid,
) -> DbResult<bool> {
    let n = sqlx::query(
        "UPDATE test_plans SET status = $4, approved_by = CASE WHEN $4 = 'approved' THEN $5 ELSE NULL END,
         approved_at = CASE WHEN $4 = 'approved' THEN now() ELSE NULL END
         WHERE id = $1 AND project_id = $2 AND status = $3",
    )
    .bind(id)
    .bind(project_id)
    .bind(from)
    .bind(to)
    .bind(user)
    .execute(pool)
    .await?
    .rows_affected();
    Ok(n == 1)
}

// ── custom step definitions ──
#[derive(Debug, Clone, FromRow, Serialize, ToSchema)]
pub struct StepDefinition {
    pub id: Uuid,
    pub project_id: Uuid,
    pub phrase: String,
    pub description: String,
    pub body: String,
    pub created_at: DateTime<Utc>,
}

pub async fn list_step_definitions(
    pool: &PgPool,
    project_id: Uuid,
) -> DbResult<Vec<StepDefinition>> {
    Ok(sqlx::query_as::<_, StepDefinition>("SELECT id, project_id, phrase, description, body, created_at FROM step_definitions WHERE project_id = $1 ORDER BY phrase")
        .bind(project_id)
        .fetch_all(pool)
        .await?)
}

pub async fn create_step_definition(
    pool: &PgPool,
    project_id: Uuid,
    phrase: &str,
    description: &str,
    body: &str,
    user: Uuid,
) -> DbResult<StepDefinition> {
    sqlx::query_as::<_, StepDefinition>(
        "INSERT INTO step_definitions (id, project_id, phrase, description, body, created_by) VALUES ($1,$2,$3,$4,$5,$6)
         RETURNING id, project_id, phrase, description, body, created_at",
    )
    .bind(new_id())
    .bind(project_id)
    .bind(phrase)
    .bind(description)
    .bind(body)
    .bind(user)
    .fetch_one(pool)
    .await
    .map_err(|e| DbError::from_sqlx(e, "step definition"))
}

pub async fn delete_step_definition(pool: &PgPool, project_id: Uuid, id: Uuid) -> DbResult<()> {
    sqlx::query("DELETE FROM step_definitions WHERE id = $1 AND project_id = $2")
        .bind(id)
        .bind(project_id)
        .execute(pool)
        .await?;
    Ok(())
}

// ── recordings ──
#[derive(Debug, Clone, FromRow, Serialize, ToSchema)]
pub struct Recording {
    pub id: Uuid,
    pub project_id: Uuid,
    pub runner_recording_id: Option<String>,
    pub url: String,
    pub status: String,
    #[schema(value_type = Object)]
    pub events: Value,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

const REC_COLS: &str =
    "id, project_id, runner_recording_id, url, status, events, created_at, updated_at";

pub async fn create_recording(
    pool: &PgPool,
    project_id: Uuid,
    url: &str,
    runner_id: Option<&str>,
    user: Uuid,
) -> DbResult<Recording> {
    Ok(sqlx::query_as::<_, Recording>(&format!(
        "INSERT INTO recordings (id, project_id, url, runner_recording_id, created_by) VALUES ($1,$2,$3,$4,$5) RETURNING {REC_COLS}"
    ))
    .bind(new_id())
    .bind(project_id)
    .bind(url)
    .bind(runner_id)
    .bind(user)
    .fetch_one(pool)
    .await?)
}

pub async fn get_recording(pool: &PgPool, project_id: Uuid, id: Uuid) -> DbResult<Recording> {
    opt(sqlx::query_as::<_, Recording>(&format!(
        "SELECT {REC_COLS} FROM recordings WHERE id = $1 AND project_id = $2"
    ))
    .bind(id)
    .bind(project_id)
    .fetch_one(pool)
    .await)
}

pub async fn list_recordings(pool: &PgPool, project_id: Uuid) -> DbResult<Vec<Recording>> {
    Ok(sqlx::query_as::<_, Recording>(&format!(
        "SELECT {REC_COLS} FROM recordings WHERE project_id = $1 ORDER BY created_at DESC LIMIT 50"
    ))
    .bind(project_id)
    .fetch_all(pool)
    .await?)
}

pub async fn update_recording(
    pool: &PgPool,
    id: Uuid,
    status: &str,
    events: &Value,
) -> DbResult<()> {
    sqlx::query("UPDATE recordings SET status = $2, events = $3, updated_at = now() WHERE id = $1")
        .bind(id)
        .bind(status)
        .bind(events)
        .execute(pool)
        .await?;
    Ok(())
}

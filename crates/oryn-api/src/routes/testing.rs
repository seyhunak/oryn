use crate::auth::{project_ctx, Authed};
use crate::error::{ApiError, ApiResult};
use crate::routes::{ok, Ok};
use crate::state::AppState;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::Json;
use oryn_db::execution::ScenarioRun;
use oryn_db::governance::audit;
use oryn_db::testing::{
    Recording, Scenario, ScenarioFilter, ScenarioInput, StepDefinition, StepRow, Suite,
    SuiteSummary,
};
use oryn_domain::{Permission, ScenarioStatus};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use utoipa::{IntoParams, ToSchema};
use utoipa_axum::{router::OpenApiRouter, routes};
use uuid::Uuid;

pub fn routes() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(validate))
        .routes(routes!(list_suites, create_suite))
        .routes(routes!(get_suite, update_suite, delete_suite))
        .routes(routes!(list_scenarios))
        .routes(routes!(get_scenario))
        .routes(routes!(set_status))
        .routes(routes!(list_step_defs, create_step_def))
        .routes(routes!(delete_step_def))
        .routes(routes!(list_recordings, start_recording))
        .routes(routes!(get_recording))
        .routes(routes!(stop_recording))
}

#[derive(Deserialize, ToSchema)]
pub struct ValidateReq {
    pub source: String,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct Diagnostic {
    pub line: usize,
    pub message: String,
    pub step: Option<String>,
}

#[derive(Serialize, ToSchema)]
pub struct CompiledScenarioView {
    pub name: String,
    pub tags: Vec<String>,
    pub kind: String,
    pub line: usize,
    pub executable: bool,
    pub steps: usize,
    pub unsupported: Vec<Diagnostic>,
}

#[derive(Serialize, ToSchema)]
pub struct ValidateResult {
    pub ok: bool,
    pub feature: Option<String>,
    pub scenarios: Vec<CompiledScenarioView>,
    pub errors: Vec<Diagnostic>,
}

pub fn compile_view(source: &str) -> (ValidateResult, Option<oryn_bdd::CompiledFeature>) {
    match oryn_bdd::compile(source) {
        Err(e) => (
            ValidateResult {
                ok: false,
                feature: None,
                scenarios: vec![],
                errors: vec![Diagnostic {
                    line: e.line,
                    message: e.message,
                    step: None,
                }],
            },
            None,
        ),
        Ok(f) => {
            let scenarios: Vec<CompiledScenarioView> = f
                .scenarios
                .iter()
                .map(|s| CompiledScenarioView {
                    name: s.name.clone(),
                    tags: s.tags.clone(),
                    kind: s.kind.to_string(),
                    line: s.line,
                    executable: s.executable(),
                    steps: s.steps.len(),
                    unsupported: s
                        .unsupported
                        .iter()
                        .map(|d| Diagnostic {
                            line: d.line,
                            message: d.message.clone(),
                            step: d.step.clone(),
                        })
                        .collect(),
                })
                .collect();
            let errors: Vec<Diagnostic> = f
                .errors
                .iter()
                .map(|d| Diagnostic {
                    line: d.line,
                    message: d.message.clone(),
                    step: d.step.clone(),
                })
                .collect();
            let ok = errors.is_empty()
                && !scenarios.is_empty()
                && scenarios.iter().all(|s| s.executable);
            (
                ValidateResult {
                    ok,
                    feature: Some(f.name.clone()),
                    scenarios,
                    errors,
                },
                Some(f),
            )
        }
    }
}

/// Compile Gherkin against the step library without saving.
#[utoipa::path(post, path = "/api/v1/projects/{id}/gherkin/validate", tag = "testing", responses((status = 200, body = ValidateResult)))]
async fn validate(
    State(s): State<AppState>,
    auth: Authed,
    Path(id): Path<Uuid>,
    Json(req): Json<ValidateReq>,
) -> ApiResult<Json<ValidateResult>> {
    project_ctx(&s, &auth, id)
        .await?
        .require(Permission::ProjectRead)?;
    Ok(Json(compile_view(&req.source).0))
}

#[utoipa::path(get, path = "/api/v1/projects/{id}/suites", tag = "testing", responses((status = 200, body = Vec<SuiteSummary>)))]
async fn list_suites(
    State(s): State<AppState>,
    auth: Authed,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Vec<SuiteSummary>>> {
    project_ctx(&s, &auth, id)
        .await?
        .require(Permission::ProjectRead)?;
    Ok(Json(oryn_db::testing::list_suites(&s.pool, id).await?))
}

#[derive(Deserialize, ToSchema)]
pub struct SuiteReq {
    /// Defaults to the Feature name.
    pub name: Option<String>,
    #[serde(default)]
    pub description: String,
    /// Repository path the feature came from, if any.
    pub path: Option<String>,
    pub source: String,
}

/// Convert a compiled feature into DB scenario inputs.
pub fn scenario_inputs(f: &oryn_bdd::CompiledFeature) -> Vec<ScenarioInput> {
    f.scenarios
        .iter()
        .map(|sc| ScenarioInput {
            name: sc.name.clone(),
            tags: sc.tags.clone(),
            kind: sc.kind.to_string(),
            line: sc.line as i32,
            outline: sc.outline.clone(),
            example_index: sc.example_index.map(|i| i as i32),
            executable: sc.executable(),
            diagnostics: serde_json::to_value(
                sc.unsupported
                    .iter()
                    .map(|d| json!({"line": d.line, "message": d.message, "step": d.step}))
                    .collect::<Vec<_>>(),
            )
            .unwrap_or_default(),
            steps: sc
                .steps
                .iter()
                .map(|st| {
                    (
                        st.keyword.clone(),
                        st.text.clone(),
                        serde_json::to_value(&st.step).unwrap_or(Value::Null),
                    )
                })
                .collect(),
        })
        .collect()
}

#[allow(clippy::too_many_arguments)]
pub async fn save(
    s: &AppState,
    project_id: Uuid,
    suite_id: Option<Uuid>,
    name: Option<String>,
    description: &str,
    path: Option<&str>,
    source: &str,
    new_status: ScenarioStatus,
    origin: &str,
    user: Option<Uuid>,
) -> ApiResult<(Uuid, ValidateResult)> {
    if source.len() > 1_000_000 {
        return Err(ApiError::bad("feature files are limited to 1 MB"));
    }
    let (view, feature) = compile_view(source);
    let parse_error = if feature.is_none() {
        view.errors
            .first()
            .map(|e| format!("line {}: {}", e.line, e.message))
    } else {
        None
    };
    let (name, tags, inputs) = match &feature {
        Some(f) => (
            name.filter(|n| !n.trim().is_empty())
                .unwrap_or_else(|| f.name.clone()),
            f.tags.clone(),
            scenario_inputs(f),
        ),
        None => (
            name.unwrap_or_else(|| "Untitled feature".into()),
            vec![],
            vec![],
        ),
    };
    let id = oryn_db::testing::save_suite(
        &s.pool,
        project_id,
        suite_id,
        name.trim(),
        description,
        path,
        source,
        &tags,
        parse_error.as_deref(),
        &inputs,
        new_status.as_str(),
        origin,
        user,
    )
    .await?;
    Ok((id, view))
}

#[derive(Serialize, ToSchema)]
pub struct SaveResult {
    pub suite_id: Uuid,
    pub validation: ValidateResult,
}

#[utoipa::path(post, path = "/api/v1/projects/{id}/suites", tag = "testing", responses((status = 201, body = SaveResult)))]
async fn create_suite(
    State(s): State<AppState>,
    auth: Authed,
    Path(id): Path<Uuid>,
    Json(req): Json<SuiteReq>,
) -> ApiResult<(StatusCode, Json<SaveResult>)> {
    let p = project_ctx(&s, &auth, id).await?;
    p.require(Permission::TestWrite)?;
    let (suite_id, validation) = save(
        &s,
        id,
        None,
        req.name,
        &req.description,
        req.path.as_deref(),
        &req.source,
        ScenarioStatus::Draft,
        "manual",
        Some(p.user_id()),
    )
    .await?;
    audit(
        &s.pool,
        Some(p.ws_id()),
        &p.actor(),
        "suite.created",
        Some("suite"),
        Some(suite_id),
        json!({"scenarios": validation.scenarios.len()}),
    )
    .await;
    Ok((
        StatusCode::CREATED,
        Json(SaveResult {
            suite_id,
            validation,
        }),
    ))
}

#[derive(Serialize, ToSchema)]
pub struct SuiteDetail {
    pub suite: Suite,
    pub scenarios: Vec<Scenario>,
}

#[utoipa::path(get, path = "/api/v1/projects/{id}/suites/{suite_id}", tag = "testing", responses((status = 200, body = SuiteDetail)))]
async fn get_suite(
    State(s): State<AppState>,
    auth: Authed,
    Path((id, suite_id)): Path<(Uuid, Uuid)>,
) -> ApiResult<Json<SuiteDetail>> {
    project_ctx(&s, &auth, id)
        .await?
        .require(Permission::ProjectRead)?;
    let suite = oryn_db::testing::get_suite(&s.pool, id, suite_id).await?;
    let scenarios = oryn_db::testing::list_scenarios(
        &s.pool,
        id,
        &ScenarioFilter {
            suite_id: Some(suite_id),
            ..Default::default()
        },
    )
    .await?;
    Ok(Json(SuiteDetail { suite, scenarios }))
}

/// Save edited Gherkin. Scenarios are matched by name: unchanged names keep their
/// history and approval; edited steps return an approved scenario to draft.
#[utoipa::path(put, path = "/api/v1/projects/{id}/suites/{suite_id}", tag = "testing", responses((status = 200, body = SaveResult)))]
async fn update_suite(
    State(s): State<AppState>,
    auth: Authed,
    Path((id, suite_id)): Path<(Uuid, Uuid)>,
    Json(req): Json<SuiteReq>,
) -> ApiResult<Json<SaveResult>> {
    let p = project_ctx(&s, &auth, id).await?;
    p.require(Permission::TestWrite)?;
    let before = oryn_db::testing::get_suite(&s.pool, id, suite_id).await?;
    let old_scenarios = oryn_db::testing::list_scenarios(
        &s.pool,
        id,
        &ScenarioFilter {
            suite_id: Some(suite_id),
            ..Default::default()
        },
    )
    .await?;
    let mut old_steps = std::collections::HashMap::new();
    for sc in &old_scenarios {
        old_steps.insert(
            sc.name.clone(),
            oryn_db::testing::scenario_steps(&s.pool, sc.id)
                .await?
                .into_iter()
                .map(|r| r.step)
                .collect::<Vec<_>>(),
        );
    }
    let (sid, validation) = save(
        &s,
        id,
        Some(suite_id),
        req.name,
        &req.description,
        req.path.as_deref(),
        &req.source,
        ScenarioStatus::Draft,
        "manual",
        Some(p.user_id()),
    )
    .await?;
    // Approval is for specific steps: if an approved scenario's steps changed, it needs re-approval.
    let now = oryn_db::testing::list_scenarios(
        &s.pool,
        id,
        &ScenarioFilter {
            suite_id: Some(suite_id),
            ..Default::default()
        },
    )
    .await?;
    let mut demote = vec![];
    for sc in now.iter().filter(|x| x.status == "approved") {
        let steps: Vec<Value> = oryn_db::testing::scenario_steps(&s.pool, sc.id)
            .await?
            .into_iter()
            .map(|r| r.step)
            .collect();
        if old_steps.get(&sc.name).map(|o| o != &steps).unwrap_or(true) {
            demote.push(sc.id);
        }
    }
    if !demote.is_empty() {
        oryn_db::testing::set_scenario_status(&s.pool, id, &demote, "draft").await?;
    }
    audit(
        &s.pool,
        Some(p.ws_id()),
        &p.actor(),
        "suite.updated",
        Some("suite"),
        Some(sid),
        json!({"name": before.name, "returned_to_draft": demote.len()}),
    )
    .await;
    Ok(Json(SaveResult {
        suite_id: sid,
        validation,
    }))
}

#[utoipa::path(delete, path = "/api/v1/projects/{id}/suites/{suite_id}", tag = "testing", responses((status = 200, body = Ok)))]
async fn delete_suite(
    State(s): State<AppState>,
    auth: Authed,
    Path((id, suite_id)): Path<(Uuid, Uuid)>,
) -> ApiResult<Json<Ok>> {
    let p = project_ctx(&s, &auth, id).await?;
    p.require(Permission::TestWrite)?;
    oryn_db::testing::delete_suite(&s.pool, id, suite_id).await?;
    audit(
        &s.pool,
        Some(p.ws_id()),
        &p.actor(),
        "suite.deleted",
        Some("suite"),
        Some(suite_id),
        json!({}),
    )
    .await;
    Ok(ok())
}

#[derive(Deserialize, IntoParams)]
pub struct ScenarioQuery {
    pub suite_id: Option<Uuid>,
    pub status: Option<String>,
    pub kind: Option<String>,
    pub tag: Option<String>,
    pub q: Option<String>,
}

#[utoipa::path(get, path = "/api/v1/projects/{id}/scenarios", tag = "testing", params(ScenarioQuery), responses((status = 200, body = Vec<Scenario>)))]
async fn list_scenarios(
    State(s): State<AppState>,
    auth: Authed,
    Path(id): Path<Uuid>,
    Query(q): Query<ScenarioQuery>,
) -> ApiResult<Json<Vec<Scenario>>> {
    project_ctx(&s, &auth, id)
        .await?
        .require(Permission::ProjectRead)?;
    let f = ScenarioFilter {
        suite_id: q.suite_id,
        status: q.status,
        kind: q.kind,
        tag: q.tag,
        q: q.q,
    };
    Ok(Json(
        oryn_db::testing::list_scenarios(&s.pool, id, &f).await?,
    ))
}

#[derive(Serialize, ToSchema)]
pub struct RequirementRef {
    pub id: Uuid,
    pub key: String,
    pub title: String,
}

#[derive(Serialize, ToSchema)]
pub struct ScenarioDetail {
    pub scenario: Scenario,
    pub steps: Vec<StepRow>,
    pub requirements: Vec<RequirementRef>,
    pub history: Vec<ScenarioRun>,
}

#[utoipa::path(get, path = "/api/v1/projects/{id}/scenarios/{scenario_id}", tag = "testing", responses((status = 200, body = ScenarioDetail)))]
async fn get_scenario(
    State(s): State<AppState>,
    auth: Authed,
    Path((id, scenario_id)): Path<(Uuid, Uuid)>,
) -> ApiResult<Json<ScenarioDetail>> {
    project_ctx(&s, &auth, id)
        .await?
        .require(Permission::ProjectRead)?;
    let scenario = oryn_db::testing::get_scenario(&s.pool, id, scenario_id).await?;
    Ok(Json(ScenarioDetail {
        steps: oryn_db::testing::scenario_steps(&s.pool, scenario_id).await?,
        requirements: oryn_db::testing::scenario_requirements(&s.pool, scenario_id)
            .await?
            .into_iter()
            .map(|(id, key, title)| RequirementRef { id, key, title })
            .collect(),
        history: oryn_db::execution::scenario_history(&s.pool, scenario_id, 30).await?,
        scenario,
    }))
}

#[derive(Deserialize, ToSchema)]
pub struct StatusReq {
    pub scenario_ids: Vec<Uuid>,
    pub status: ScenarioStatus,
    pub comment: Option<String>,
}

#[derive(Serialize, ToSchema)]
pub struct StatusResult {
    pub updated: u64,
}

/// Approve, return to draft, or archive scenarios. Approval requires `test:approve`
/// and only executable scenarios (every step compiled) can be approved.
#[utoipa::path(post, path = "/api/v1/projects/{id}/scenarios/status", tag = "testing", responses((status = 200, body = StatusResult)))]
async fn set_status(
    State(s): State<AppState>,
    auth: Authed,
    Path(id): Path<Uuid>,
    Json(req): Json<StatusReq>,
) -> ApiResult<Json<StatusResult>> {
    let p = project_ctx(&s, &auth, id).await?;
    p.require(if req.status == ScenarioStatus::Approved {
        Permission::TestApprove
    } else {
        Permission::TestWrite
    })?;
    if req.scenario_ids.is_empty() || req.scenario_ids.len() > 1000 {
        return Err(ApiError::bad("give 1 to 1000 scenario ids"));
    }
    let mut ids = req.scenario_ids.clone();
    if req.status == ScenarioStatus::Approved {
        let all = oryn_db::testing::list_scenarios(&s.pool, id, &ScenarioFilter::default()).await?;
        let blocked: Vec<&str> = all
            .iter()
            .filter(|x| ids.contains(&x.id) && !x.executable)
            .map(|x| x.name.as_str())
            .collect();
        if !blocked.is_empty() {
            return Err(ApiError::unprocessable(format!(
                "these scenarios have unsupported steps and cannot be approved: {}",
                blocked.join(", ")
            )));
        }
        ids.retain(|i| all.iter().any(|x| &x.id == i));
    }
    let n = oryn_db::testing::set_scenario_status(&s.pool, id, &ids, req.status.as_str()).await?;
    for sid in &ids {
        if req.status == ScenarioStatus::Approved {
            oryn_db::governance::record_approval(
                &s.pool,
                p.ws_id(),
                Some(id),
                "scenario",
                *sid,
                "approved",
                p.user_id(),
                req.comment.as_deref(),
            )
            .await?;
        }
    }
    audit(
        &s.pool,
        Some(p.ws_id()),
        &p.actor(),
        &format!("scenario.{}", req.status),
        Some("project"),
        Some(id),
        json!({"count": n, "ids": ids}),
    )
    .await;
    Ok(Json(StatusResult { updated: n }))
}

// ── custom step definitions (composite steps built from library phrasings) ──

#[utoipa::path(get, path = "/api/v1/projects/{id}/step-definitions", tag = "testing", responses((status = 200, body = Vec<StepDefinition>)))]
async fn list_step_defs(
    State(s): State<AppState>,
    auth: Authed,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Vec<StepDefinition>>> {
    project_ctx(&s, &auth, id)
        .await?
        .require(Permission::ProjectRead)?;
    Ok(Json(
        oryn_db::testing::list_step_definitions(&s.pool, id).await?,
    ))
}

#[derive(Deserialize, ToSchema)]
pub struct StepDefReq {
    /// The phrase authors write, e.g. `I sign in as the demo customer`.
    pub phrase: String,
    #[serde(default)]
    pub description: String,
    /// Library steps it expands to, one per line (validated).
    pub body: String,
}

#[utoipa::path(post, path = "/api/v1/projects/{id}/step-definitions", tag = "testing", responses((status = 201, body = StepDefinition)))]
async fn create_step_def(
    State(s): State<AppState>,
    auth: Authed,
    Path(id): Path<Uuid>,
    Json(req): Json<StepDefReq>,
) -> ApiResult<(StatusCode, Json<StepDefinition>)> {
    let p = project_ctx(&s, &auth, id).await?;
    p.require(Permission::TestWrite)?;
    let phrase = req.phrase.trim();
    if phrase.is_empty() || phrase.len() > 200 {
        return Err(ApiError::bad("phrase must be 1 to 200 characters"));
    }
    let wrapped = format!(
        "Feature: check\n  Scenario: check\n{}",
        req.body
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .map(|l| format!("    * {}\n", strip_keyword(l)))
            .collect::<String>()
    );
    let (view, _) = compile_view(&wrapped);
    if !view.ok {
        let msg = view
            .errors
            .iter()
            .chain(view.scenarios.iter().flat_map(|s| s.unsupported.iter()))
            .map(|d| d.message.clone())
            .collect::<Vec<_>>()
            .join("; ");
        return Err(ApiError::unprocessable(format!(
            "the body must use library steps only: {msg}"
        )));
    }
    let d = oryn_db::testing::create_step_definition(
        &s.pool,
        id,
        phrase,
        &req.description,
        &req.body,
        p.user_id(),
    )
    .await?;
    Ok((StatusCode::CREATED, Json(d)))
}

#[utoipa::path(delete, path = "/api/v1/projects/{id}/step-definitions/{def_id}", tag = "testing", responses((status = 200, body = Ok)))]
async fn delete_step_def(
    State(s): State<AppState>,
    auth: Authed,
    Path((id, def_id)): Path<(Uuid, Uuid)>,
) -> ApiResult<Json<Ok>> {
    project_ctx(&s, &auth, id)
        .await?
        .require(Permission::TestWrite)?;
    oryn_db::testing::delete_step_definition(&s.pool, id, def_id).await?;
    Ok(ok())
}

// ── browser recordings (proxied to a Playwright runner with a display) ──

#[utoipa::path(get, path = "/api/v1/projects/{id}/recordings", tag = "testing", responses((status = 200, body = Vec<Recording>)))]
async fn list_recordings(
    State(s): State<AppState>,
    auth: Authed,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Vec<Recording>>> {
    project_ctx(&s, &auth, id)
        .await?
        .require(Permission::ProjectRead)?;
    Ok(Json(oryn_db::testing::list_recordings(&s.pool, id).await?))
}

#[derive(Deserialize, ToSchema)]
pub struct StartRecording {
    pub url: String,
}

fn runner() -> ApiResult<(String, Option<String>)> {
    let url = std::env::var("ORYN_RECORDER_URL").ok().filter(|v| !v.is_empty()).ok_or_else(|| {
        ApiError::Unavailable("recording needs a Playwright runner with a display; set ORYN_RECORDER_URL (see docs/DEVELOPMENT.md)".into())
    })?;
    Ok((
        url.trim_end_matches('/').to_string(),
        std::env::var("ORYN_RECORDER_TOKEN").ok(),
    ))
}

#[utoipa::path(post, path = "/api/v1/projects/{id}/recordings", tag = "testing", responses((status = 201, body = Recording), (status = 503)))]
async fn start_recording(
    State(s): State<AppState>,
    auth: Authed,
    Path(id): Path<Uuid>,
    Json(req): Json<StartRecording>,
) -> ApiResult<(StatusCode, Json<Recording>)> {
    let p = project_ctx(&s, &auth, id).await?;
    p.require(Permission::TestWrite)?;
    let target = url::Url::parse(&req.url).map_err(|_| ApiError::bad("url must be absolute"))?;
    if target.scheme() != "http" && target.scheme() != "https" {
        return Err(ApiError::bad("url must be http or https"));
    }
    // Same rule as test runs: only the project's environment hosts, never metadata hosts.
    let envs = oryn_db::projects::list_environments(&s.pool, id).await?;
    let hosts: Vec<String> = envs
        .iter()
        .flat_map(|e| {
            [&e.base_url, &e.api_base_url]
                .into_iter()
                .filter_map(|u| u.as_deref().and_then(|u| url::Url::parse(u).ok()))
                .filter_map(|u| u.host_str().map(str::to_string))
                .chain(e.allowed_hosts.iter().cloned())
                .collect::<Vec<_>>()
        })
        .collect();
    let policy = oryn_api_engine::NetworkPolicy {
        allowed_hosts: hosts,
        block_private: false,
    };
    if !target.host_str().is_some_and(|h| policy.host_allowed(h)) {
        return Err(ApiError::bad(
            "url host is not one of this project's environment hosts",
        ));
    }
    let (base, token) = runner()?;
    let mut rb = s
        .http
        .post(format!("{base}/v1/recordings"))
        .json(&json!({"protocol": "oryn.browser/v1", "url": req.url, "browser": "chromium"}));
    if let Some(t) = token {
        rb = rb.bearer_auth(t);
    }
    let resp = rb
        .send()
        .await
        .map_err(|e| ApiError::Unavailable(format!("recorder unreachable: {e}")))?;
    let status = resp.status();
    let body: Value = resp.json().await.unwrap_or_default();
    if !status.is_success() {
        return Err(ApiError::Unavailable(format!(
            "recorder refused: {}",
            body["error"]["message"]
                .as_str()
                .or(body["error"].as_str())
                .unwrap_or("unknown error")
        )));
    }
    let rid = body["recording_id"]
        .as_str()
        .or(body["id"].as_str())
        .map(String::from);
    let r = oryn_db::testing::create_recording(&s.pool, id, &req.url, rid.as_deref(), p.user_id())
        .await?;
    Ok((StatusCode::CREATED, Json(r)))
}

#[utoipa::path(get, path = "/api/v1/projects/{id}/recordings/{rec_id}", tag = "testing", responses((status = 200, body = Recording)))]
async fn get_recording(
    State(s): State<AppState>,
    auth: Authed,
    Path((id, rec_id)): Path<(Uuid, Uuid)>,
) -> ApiResult<Json<Recording>> {
    project_ctx(&s, &auth, id)
        .await?
        .require(Permission::ProjectRead)?;
    Ok(Json(
        oryn_db::testing::get_recording(&s.pool, id, rec_id).await?,
    ))
}

#[derive(Serialize, ToSchema)]
pub struct StoppedRecording {
    pub recording: Recording,
    /// Gherkin generated from the recorded events (redacted; library steps only).
    pub gherkin: String,
}

/// Stop recording; events are converted to library steps. Typed passwords arrive
/// redacted from the runner and are replaced by a `{{password}}` variable.
#[utoipa::path(post, path = "/api/v1/projects/{id}/recordings/{rec_id}/stop", tag = "testing", responses((status = 200, body = StoppedRecording)))]
async fn stop_recording(
    State(s): State<AppState>,
    auth: Authed,
    Path((id, rec_id)): Path<(Uuid, Uuid)>,
) -> ApiResult<Json<StoppedRecording>> {
    project_ctx(&s, &auth, id)
        .await?
        .require(Permission::TestWrite)?;
    let rec = oryn_db::testing::get_recording(&s.pool, id, rec_id).await?;
    let rid = rec
        .runner_recording_id
        .clone()
        .ok_or_else(|| ApiError::Conflict("recording has no runner session".into()))?;
    let (base, token) = runner()?;
    let mut rb = s.http.post(format!("{base}/v1/recordings/{rid}/stop"));
    if let Some(t) = token {
        rb = rb.bearer_auth(t);
    }
    let body: Value = rb
        .send()
        .await
        .map_err(|e| ApiError::Unavailable(format!("recorder unreachable: {e}")))?
        .json()
        .await
        .unwrap_or_default();
    let events = body.get("events").cloned().unwrap_or_else(|| json!([]));
    oryn_db::testing::update_recording(&s.pool, rec_id, "stopped", &events).await?;
    let gherkin = events_to_gherkin(&rec.url, &events);
    let recording = oryn_db::testing::get_recording(&s.pool, id, rec_id).await?;
    Ok(Json(StoppedRecording { recording, gherkin }))
}

pub fn strip_keyword(line: &str) -> &str {
    for k in ["Given ", "When ", "Then ", "And ", "But ", "* "] {
        if let Some(rest) = line.strip_prefix(k) {
            return rest.trim_start();
        }
    }
    line
}

fn q(s: &str) -> String {
    s.replace('"', "'")
}

pub fn events_to_gherkin(start_url: &str, events: &Value) -> String {
    let start_path = url::Url::parse(start_url)
        .map(|u| u.path().to_string())
        .unwrap_or_else(|_| "/".into());
    let mut steps: Vec<String> = vec![];
    for e in events.as_array().map(Vec::as_slice).unwrap_or(&[]) {
        let target = e["target"].as_str().unwrap_or("");
        let value = || {
            if e["redacted"].as_bool().unwrap_or(false) {
                "{{password}}".to_string()
            } else {
                q(e["value"].as_str().unwrap_or(""))
            }
        };
        let line = match e["action"].as_str().unwrap_or("") {
            "goto" => {
                let u = e["url"].as_str().unwrap_or("/");
                if steps.is_empty() {
                    format!("    Given I open \"{}\"", q(u))
                } else {
                    format!("    And I open \"{}\"", q(u))
                }
            }
            "click" if !target.is_empty() => format!("    When I click \"{}\"", q(target)),
            "fill" if !target.is_empty() => {
                format!("    When I fill \"{}\" with \"{}\"", q(target), value())
            }
            "select" if !target.is_empty() => {
                format!("    When I select \"{}\" from \"{}\"", value(), q(target))
            }
            "check" if !target.is_empty() => format!("    When I check \"{}\"", q(target)),
            "uncheck" if !target.is_empty() => format!("    When I uncheck \"{}\"", q(target)),
            "press" if !target.is_empty() => format!(
                "    When I press \"{}\" in \"{}\"",
                q(e["key"].as_str().unwrap_or("Enter")),
                q(target)
            ),
            "press" => format!(
                "    When I press \"{}\"",
                q(e["key"].as_str().unwrap_or("Enter"))
            ),
            _ => continue,
        };
        if steps.last() != Some(&line) {
            steps.push(line);
        }
    }
    if steps.is_empty() {
        steps.push(format!("    Given I open \"{}\"", q(&start_path)));
    }
    format!(
        "Feature: Recorded flow\n\n  @browser @recorded\n  Scenario: Recorded on {}\n{}\n    # Add Then steps that state what should be true.\n",
        q(&start_path),
        steps.join("\n")
    )
}

#[cfg(test)]
mod tests {
    #[test]
    fn recording_to_gherkin() {
        let ev = serde_json::json!([
            {"action":"goto","url":"/login"},
            {"action":"fill","target":"Email","value":"demo@northwind.test"},
            {"action":"fill","target":"Password","value":"{{secret}}","redacted":true},
            {"action":"click","target":"Sign in"}
        ]);
        let g = super::events_to_gherkin("http://localhost:4000/login", &ev);
        assert!(g.contains("Given I open \"/login\""));
        assert!(g.contains("I fill \"Password\" with \"{{password}}\""));
        assert!(!g.contains("{{secret}}"));
        let (v, _) = super::compile_view(&g);
        assert!(v.ok, "{:?}", v.errors);
    }

    #[test]
    fn step_body_keywords_stripped() {
        assert_eq!(super::strip_keyword("Given I open \"/\""), "I open \"/\"");
        assert_eq!(super::strip_keyword("I click \"x\""), "I click \"x\"");
    }
}

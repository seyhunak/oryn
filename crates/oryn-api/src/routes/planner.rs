//! AI Test Planner. Plans are proposals: nothing runs until a person with
//! `test:approve` approves items, which turns them into approved scenarios.

use crate::auth::{project_ctx, Authed, ProjectCtx};
use crate::error::{ApiError, ApiResult};
use crate::routes::testing::save;
use crate::state::AppState;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use oryn_db::governance::audit;
use oryn_db::testing::{PlanItemInput, TestPlan, TestPlanItem};
use oryn_domain::{Permission, ScenarioStatus};
use oryn_intelligence::analysis::Analysis;
use oryn_intelligence::planner::{self, DraftScenario, PlanDraft, PlanInput, RequirementInput};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::time::Duration;
use utoipa::ToSchema;
use utoipa_axum::{router::OpenApiRouter, routes};
use uuid::Uuid;

pub fn routes() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(list_plans, create_plan))
        .routes(routes!(get_plan))
        .routes(routes!(update_item))
        .routes(routes!(approve_plan))
        .routes(routes!(reject_plan))
}

#[utoipa::path(get, path = "/api/v1/projects/{id}/plans", tag = "planner", responses((status = 200, body = Vec<TestPlan>)))]
async fn list_plans(
    State(s): State<AppState>,
    auth: Authed,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Vec<TestPlan>>> {
    project_ctx(&s, &auth, id)
        .await?
        .require(Permission::ProjectRead)?;
    Ok(Json(oryn_db::testing::list_plans(&s.pool, id).await?))
}

#[derive(Deserialize, ToSchema, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum PlanMode {
    /// AI when a provider is configured, heuristic otherwise.
    Auto,
    Ai,
    Heuristic,
}

#[derive(Deserialize, ToSchema)]
pub struct CreatePlan {
    pub mode: Option<PlanMode>,
    /// What to focus on, in your words. Treated as data by the model.
    #[serde(default)]
    pub instructions: String,
    /// Requirements to plan for; all requirements when empty.
    #[serde(default)]
    pub requirement_ids: Vec<Uuid>,
    pub max_scenarios: Option<usize>,
}

#[derive(Serialize, ToSchema)]
pub struct PlanDetail {
    pub plan: TestPlan,
    pub items: Vec<TestPlanItem>,
}

async fn plan_input(s: &AppState, p: &ProjectCtx, req: &CreatePlan) -> ApiResult<PlanInput> {
    let all = oryn_db::projects::list_requirements(&s.pool, p.id()).await?;
    let reqs: Vec<RequirementInput> = all
        .into_iter()
        .filter(|r| req.requirement_ids.is_empty() || req.requirement_ids.contains(&r.id))
        .map(|r| RequirementInput {
            key: r.key,
            title: r.title,
            body: r.body,
        })
        .collect();
    let ctx = oryn_db::projects::get_context(&s.pool, p.id()).await?;
    let analysis: Analysis = ctx
        .as_ref()
        .and_then(|c| serde_json::from_value(c.analysis.clone()).ok())
        .unwrap_or_default();
    let mut words: Vec<String> = reqs
        .iter()
        .flat_map(|r| {
            r.title
                .split_whitespace()
                .map(|w| {
                    w.trim_matches(|c: char| !c.is_alphanumeric())
                        .to_ascii_lowercase()
                })
                .collect::<Vec<_>>()
        })
        .filter(|w| w.len() > 3)
        .collect();
    words.sort();
    words.dedup();
    words.truncate(20);
    let snippets = if words.is_empty() {
        vec![]
    } else {
        oryn_db::projects::search_chunks_any(&s.pool, p.id(), &words, 12).await?
    };
    let existing = oryn_db::testing::list_scenarios(&s.pool, p.id(), &Default::default())
        .await?
        .into_iter()
        .map(|x| x.name)
        .collect();
    Ok(PlanInput {
        project_name: p.project.name.clone(),
        instructions: req.instructions.clone(),
        requirements: reqs,
        routes: analysis.routes,
        context: snippets
            .into_iter()
            .map(|c| (c.source_path, c.content))
            .collect(),
        existing_scenarios: existing,
        app_summary: ctx.map(|c| c.summary).unwrap_or_default(),
        max_scenarios: req.max_scenarios.unwrap_or(0).min(40),
    })
}

fn items_of(d: &PlanDraft) -> Vec<PlanItemInput> {
    d.scenarios
        .iter()
        .map(|sc| PlanItemInput {
            title: sc.title.chars().take(200).collect(),
            category: sc.category.clone(),
            priority: sc.priority.clone(),
            risk: sc.risk.clone(),
            rationale: sc.rationale.clone(),
            requirement_keys: sc.requirement_keys.clone(),
            gherkin: sc.gherkin.clone(),
            executable: sc.valid,
            diagnostics: json!(sc.diagnostics),
        })
        .collect()
}

/// Generate a test plan (status `draft`). With AI, the model sees requirements,
/// detected routes and redacted repository snippets, all fenced as untrusted data,
/// and must answer in the plan JSON schema; every scenario is compiled against the
/// step library before it is shown.
#[utoipa::path(post, path = "/api/v1/projects/{id}/plans", tag = "planner", responses((status = 201, body = PlanDetail)))]
async fn create_plan(
    State(s): State<AppState>,
    auth: Authed,
    Path(id): Path<Uuid>,
    Json(req): Json<CreatePlan>,
) -> ApiResult<(StatusCode, Json<PlanDetail>)> {
    let p = project_ctx(&s, &auth, id).await?;
    p.require(Permission::TestWrite)?;
    if req.instructions.len() > 20_000 {
        return Err(ApiError::bad(
            "instructions are limited to 20,000 characters",
        ));
    }
    let mode = req.mode.unwrap_or(PlanMode::Auto);
    let input = plan_input(&s, &p, &req).await?;
    let ai = if mode == PlanMode::Heuristic {
        None
    } else {
        s.ai(p.ws_id()).await?
    };
    if mode == PlanMode::Ai && ai.is_none() {
        return Err(ApiError::unprocessable("no AI provider is configured for this workspace; add one under Settings, or use the heuristic planner"));
    }
    let (draft, ai_request_id) = match ai {
        Some(ai) => {
            p.require(Permission::AiUse)?;
            s.limiter
                .check(
                    &format!("ai-plan:{}", p.user_id()),
                    30,
                    Duration::from_secs(3600),
                )
                .await?;
            let request = planner::ai_plan_request(&input, &ai.model);
            let (res, rid) = s
                .ai_call(p.ws_id(), Some(id), Some(p.user_id()), &ai, request)
                .await?;
            let v = res
                .json
                .ok_or_else(|| ApiError::Unavailable("model returned no plan".into()))?;
            let mut d = planner::parse_ai_plan(&v);
            if d.title.trim().is_empty() {
                d.title = format!("{}: test plan", p.project.name);
            }
            (d, Some(rid))
        }
        None => (planner::heuristic_plan(&input), None),
    };
    if draft.scenarios.is_empty() {
        return Err(ApiError::unprocessable(format!(
            "the planner produced no scenarios. {}",
            draft
                .coverage_gaps
                .first()
                .cloned()
                .unwrap_or_else(|| "Import requirements or a repository first.".into())
        )));
    }
    let plan_id = oryn_db::testing::create_plan(
        &s.pool,
        id,
        &draft.title,
        &draft.objective,
        &req.instructions,
        &draft.source,
        &draft.rationale,
        &json!(draft.risks),
        &json!(draft.coverage_gaps),
        ai_request_id,
        &items_of(&draft),
        p.user_id(),
    )
    .await?;
    audit(
        &s.pool,
        Some(p.ws_id()),
        &p.actor(),
        "plan.created",
        Some("test_plan"),
        Some(plan_id),
        json!({"source": draft.source, "scenarios": draft.scenarios.len()}),
    )
    .await;
    Ok((
        StatusCode::CREATED,
        Json(PlanDetail {
            plan: oryn_db::testing::get_plan(&s.pool, id, plan_id).await?,
            items: oryn_db::testing::plan_items(&s.pool, plan_id).await?,
        }),
    ))
}

#[utoipa::path(get, path = "/api/v1/projects/{id}/plans/{plan_id}", tag = "planner", responses((status = 200, body = PlanDetail)))]
async fn get_plan(
    State(s): State<AppState>,
    auth: Authed,
    Path((id, plan_id)): Path<(Uuid, Uuid)>,
) -> ApiResult<Json<PlanDetail>> {
    project_ctx(&s, &auth, id)
        .await?
        .require(Permission::ProjectRead)?;
    Ok(Json(PlanDetail {
        plan: oryn_db::testing::get_plan(&s.pool, id, plan_id).await?,
        items: oryn_db::testing::plan_items(&s.pool, plan_id).await?,
    }))
}

#[derive(Deserialize, ToSchema)]
pub struct UpdateItem {
    /// Edited Gherkin; recompiled on save.
    pub gherkin: Option<String>,
    /// `proposed`, `accepted` or `rejected`.
    pub status: Option<String>,
}

#[utoipa::path(patch, path = "/api/v1/projects/{id}/plans/{plan_id}/items/{item_id}", tag = "planner", responses((status = 200, body = TestPlanItem)))]
async fn update_item(
    State(s): State<AppState>,
    auth: Authed,
    Path((id, plan_id, item_id)): Path<(Uuid, Uuid, Uuid)>,
    Json(req): Json<UpdateItem>,
) -> ApiResult<Json<TestPlanItem>> {
    let p = project_ctx(&s, &auth, id).await?;
    p.require(Permission::TestWrite)?;
    let plan = oryn_db::testing::get_plan(&s.pool, id, plan_id).await?;
    if plan.status != "draft" {
        return Err(ApiError::Conflict(format!(
            "this plan is {}; only draft plans can be edited",
            plan.status
        )));
    }
    if let Some(st) = &req.status {
        if !["proposed", "accepted", "rejected"].contains(&st.as_str()) {
            return Err(ApiError::bad(
                "status must be proposed, accepted or rejected",
            ));
        }
    }
    let (exe, diags) = match &req.gherkin {
        Some(g) => {
            let mut d = DraftScenario {
                title: String::new(),
                category: String::new(),
                priority: String::new(),
                risk: String::new(),
                rationale: String::new(),
                requirement_keys: vec![],
                gherkin: g.clone(),
                valid: false,
                diagnostics: vec![],
            };
            planner::validate(&mut d);
            (Some(d.valid), Some(json!(d.diagnostics)))
        }
        None => (None, None),
    };
    oryn_db::testing::update_plan_item(
        &s.pool,
        plan_id,
        item_id,
        req.gherkin.as_deref(),
        exe,
        diags.as_ref(),
        req.status.as_deref(),
        None,
    )
    .await?;
    let item = oryn_db::testing::plan_items(&s.pool, plan_id)
        .await?
        .into_iter()
        .find(|i| i.id == item_id)
        .ok_or_else(|| ApiError::not_found("plan item"))?;
    Ok(Json(item))
}

#[derive(Deserialize, ToSchema)]
pub struct ApprovePlan {
    pub comment: Option<String>,
}

#[derive(Serialize, ToSchema)]
pub struct ApproveResult {
    pub suites_created: usize,
    pub scenarios_approved: usize,
    pub skipped: Vec<String>,
}

/// Approve a plan: every `accepted` item (or every non-rejected item when none are
/// marked accepted) that compiles becomes its own suite with approved scenarios,
/// linked to the item's requirement keys. Requires `test:approve`.
#[utoipa::path(post, path = "/api/v1/projects/{id}/plans/{plan_id}/approve", tag = "planner", responses((status = 200, body = ApproveResult)))]
async fn approve_plan(
    State(s): State<AppState>,
    auth: Authed,
    Path((id, plan_id)): Path<(Uuid, Uuid)>,
    Json(req): Json<ApprovePlan>,
) -> ApiResult<Json<ApproveResult>> {
    let p = project_ctx(&s, &auth, id).await?;
    p.require(Permission::TestApprove)?;
    let plan = oryn_db::testing::get_plan(&s.pool, id, plan_id).await?;
    if plan.status != "draft" {
        return Err(ApiError::Conflict(format!(
            "this plan is already {}",
            plan.status
        )));
    }
    let items = oryn_db::testing::plan_items(&s.pool, plan_id).await?;
    let any_accepted = items.iter().any(|i| i.status == "accepted");
    let chosen: Vec<&TestPlanItem> = items
        .iter()
        .filter(|i| {
            if any_accepted {
                i.status == "accepted"
            } else {
                i.status != "rejected"
            }
        })
        .collect();
    // Claim the plan first so two reviewers cannot both materialise it.
    if !oryn_db::testing::transition_plan(&s.pool, id, plan_id, "draft", "approved", p.user_id())
        .await?
    {
        return Err(ApiError::Conflict(
            "this plan was approved or rejected meanwhile".into(),
        ));
    }
    let mut created: Vec<Uuid> = vec![];
    let outcome = materialise_plan(&s, &p, id, plan_id, &plan, chosen, &mut created).await;
    let (suites, approved, skipped) = match outcome {
        Result::Ok(v) => v,
        Err(e) => {
            // Undo what was created so a retry starts from a clean draft.
            for sid in &created {
                let _ = oryn_db::testing::delete_suite(&s.pool, id, *sid).await;
            }
            let _ = oryn_db::testing::transition_plan(
                &s.pool,
                id,
                plan_id,
                "approved",
                "draft",
                p.user_id(),
            )
            .await;
            return Err(e);
        }
    };
    oryn_db::governance::record_approval(
        &s.pool,
        p.ws_id(),
        Some(id),
        "test_plan",
        plan_id,
        "approved",
        p.user_id(),
        req.comment.as_deref(),
    )
    .await?;
    audit(
        &s.pool,
        Some(p.ws_id()),
        &p.actor(),
        "plan.approved",
        Some("test_plan"),
        Some(plan_id),
        json!({"suites": suites, "scenarios": approved, "skipped": skipped.len()}),
    )
    .await;
    Ok(Json(ApproveResult {
        suites_created: suites,
        scenarios_approved: approved,
        skipped,
    }))
}

/// Create one approved suite per chosen plan item. Records every suite it creates in
/// `created`, so the caller can roll back on failure.
async fn materialise_plan(
    s: &AppState,
    p: &crate::auth::ProjectCtx,
    id: Uuid,
    plan_id: Uuid,
    plan: &oryn_db::testing::TestPlan,
    chosen: Vec<&TestPlanItem>,
    created: &mut Vec<Uuid>,
) -> ApiResult<(usize, usize, Vec<String>)> {
    let reqs = oryn_db::projects::list_requirements(&s.pool, id).await?;
    let (mut suites, mut approved, mut skipped) = (0usize, 0usize, vec![]);
    for it in chosen {
        if !it.executable {
            skipped.push(format!("{}: has unsupported steps", it.title));
            continue;
        }
        let (suite_id, view) = save(
            s,
            id,
            None,
            Some(it.title.clone()),
            &format!("From test plan \"{}\" ({})", plan.title, plan.source),
            None,
            &it.gherkin,
            ScenarioStatus::Approved,
            &plan.source,
            Some(p.user_id()),
        )
        .await?;
        created.push(suite_id);
        let scenarios = oryn_db::testing::list_scenarios(
            &s.pool,
            id,
            &oryn_db::testing::ScenarioFilter {
                suite_id: Some(suite_id),
                ..Default::default()
            },
        )
        .await?;
        let ids: Vec<Uuid> = scenarios.iter().map(|x| x.id).collect();
        oryn_db::testing::set_scenario_status(&s.pool, id, &ids, "approved").await?;
        for key in &it.requirement_keys {
            if let Some(r) = reqs.iter().find(|r| r.key.eq_ignore_ascii_case(key)) {
                for sid in &ids {
                    oryn_db::projects::link_requirement(&s.pool, id, r.id, *sid, "plan").await?;
                }
            }
        }
        oryn_db::testing::update_plan_item(
            &s.pool,
            plan_id,
            it.id,
            None,
            None,
            None,
            Some("accepted"),
            ids.first().copied(),
        )
        .await?;
        suites += 1;
        approved += view.scenarios.len();
    }
    Ok((suites, approved, skipped))
}

#[utoipa::path(post, path = "/api/v1/projects/{id}/plans/{plan_id}/reject", tag = "planner", responses((status = 200, body = crate::routes::Ok)))]
async fn reject_plan(
    State(s): State<AppState>,
    auth: Authed,
    Path((id, plan_id)): Path<(Uuid, Uuid)>,
    Json(req): Json<ApprovePlan>,
) -> ApiResult<Json<crate::routes::Ok>> {
    let p = project_ctx(&s, &auth, id).await?;
    p.require(Permission::TestApprove)?;
    oryn_db::testing::get_plan(&s.pool, id, plan_id).await?;
    oryn_db::testing::set_plan_status(&s.pool, id, plan_id, "rejected", p.user_id()).await?;
    oryn_db::governance::record_approval(
        &s.pool,
        p.ws_id(),
        Some(id),
        "test_plan",
        plan_id,
        "rejected",
        p.user_id(),
        req.comment.as_deref(),
    )
    .await?;
    audit(
        &s.pool,
        Some(p.ws_id()),
        &p.actor(),
        "plan.rejected",
        Some("test_plan"),
        Some(plan_id),
        json!({}),
    )
    .await;
    Ok(crate::routes::ok())
}

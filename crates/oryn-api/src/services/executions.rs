//! Turning approved scenarios into queued jobs.

use crate::auth::ProjectCtx;
use crate::error::{ApiError, ApiResult};
use crate::state::AppState;
use oryn_db::execution::{Execution, NewExecution, NewJob};
use oryn_db::governance::audit;
use oryn_db::testing::{Scenario, ScenarioFilter};
use oryn_domain::job::{
    JobEnvironment, JobOptions, JobSpec, PlannedStep, ScreenshotMode, JOB_PROTOCOL,
};
use oryn_domain::{Browser, ExecStep};
use serde::Deserialize;
use serde_json::json;
use std::collections::BTreeMap;
use utoipa::ToSchema;
use uuid::Uuid;

#[derive(Debug, Clone, Default, Deserialize, ToSchema)]
pub struct RunRequest {
    /// Run every runnable scenario of this suite.
    pub suite_id: Option<Uuid>,
    /// Or exactly these scenarios.
    #[serde(default)]
    pub scenario_ids: Vec<Uuid>,
    /// Or scenarios carrying any of these tags (e.g. `@smoke`).
    #[serde(default)]
    pub tags: Vec<String>,
    /// Defaults to the project's default environment.
    pub environment_id: Option<Uuid>,
    pub browser: Option<Browser>,
    pub title: Option<String>,
    /// Total attempts per scenario (1 = no retry). Errors and timeouts are retried; failures only with `retry_on_failure`.
    pub max_attempts: Option<i32>,
    #[serde(default)]
    pub retry_on_failure: bool,
    #[serde(default)]
    pub screenshots_every_step: bool,
    #[serde(default)]
    pub record_video: bool,
    #[serde(default)]
    pub record_trace: bool,
    pub commit_sha: Option<String>,
    pub branch: Option<String>,
    /// `manual`, `api`, `ci`, `schedule`, `regression` or `rerun`.
    pub trigger: Option<String>,
    #[serde(skip)]
    pub rerun_of: Option<Uuid>,
}

/// Scenarios eligible to run: executable, and approved (or draft when the workspace
/// does not require approval).
pub fn runnable(sc: &Scenario, require_approval: bool) -> bool {
    sc.executable && (sc.status == "approved" || (!require_approval && sc.status == "draft"))
}

pub async fn start(s: &AppState, p: &ProjectCtx, req: RunRequest) -> ApiResult<Execution> {
    let ws = oryn_db::identity::get_workspace(&s.pool, p.ws_id()).await?;
    let filter = ScenarioFilter {
        suite_id: req.suite_id,
        ..Default::default()
    };
    let all = oryn_db::testing::list_scenarios(&s.pool, p.id(), &filter).await?;
    let tags: Vec<String> = req
        .tags
        .iter()
        .map(|t| {
            if t.starts_with('@') {
                t.clone()
            } else {
                format!("@{t}")
            }
        })
        .collect();
    let selected: Vec<&Scenario> = all
        .iter()
        .filter(|sc| req.scenario_ids.is_empty() || req.scenario_ids.contains(&sc.id))
        .filter(|sc| {
            tags.is_empty()
                || sc
                    .tags
                    .iter()
                    .any(|t| tags.iter().any(|w| w.eq_ignore_ascii_case(t)))
        })
        .collect();
    let (run, blocked): (Vec<&Scenario>, Vec<&Scenario>) = selected
        .into_iter()
        .partition(|sc| runnable(sc, ws.require_approval));
    if run.is_empty() {
        let why = if blocked.is_empty() {
            "No scenarios match the selection.".to_string()
        } else if ws.require_approval {
            format!("{} matching scenario(s) are not approved or have unsupported steps. Approve them in Test Studio first.", blocked.len())
        } else {
            format!(
                "{} matching scenario(s) have unsupported steps.",
                blocked.len()
            )
        };
        return Err(ApiError::unprocessable(why));
    }
    if run.len() > 2000 {
        return Err(ApiError::bad("an execution is limited to 2000 scenarios"));
    }

    let env_row = match req.environment_id {
        Some(e) => oryn_db::projects::get_environment(&s.pool, p.id(), e).await?,
        None => oryn_db::projects::default_environment(&s.pool, p.id())
            .await?
            .ok_or_else(|| ApiError::unprocessable("this project has no environment; add one with the application's base URL under Projects → Environments"))?,
    };
    let mut variables = BTreeMap::new();
    if let Some(o) = env_row.variables.as_object() {
        for (k, v) in o {
            variables.insert(
                k.clone(),
                v.as_str()
                    .map(String::from)
                    .unwrap_or_else(|| v.to_string()),
            );
        }
    }
    let mut secret_names: Vec<String> = env_row
        .secrets
        .as_object()
        .map(|o| o.keys().cloned().collect())
        .unwrap_or_default();
    secret_names.sort();
    let environment = JobEnvironment {
        name: env_row.name.clone(),
        base_url: env_row.base_url.clone(),
        api_base_url: env_row.api_base_url.clone(),
        variables,
        secret_names,
        allowed_hosts: env_row.allowed_hosts.clone(),
    };
    let options = JobOptions {
        browser: req.browser.unwrap_or(Browser::Chromium),
        screenshots: if req.screenshots_every_step {
            ScreenshotMode::EveryStep
        } else {
            ScreenshotMode::OnFailure
        },
        record_video: req.record_video,
        record_trace: req.record_trace,
        ..Default::default()
    };
    let max_attempts = req.max_attempts.unwrap_or(2).clamp(1, 5);
    let execution_id = oryn_db::new_id();
    let mut jobs = Vec::with_capacity(run.len());
    for sc in &run {
        let rows = oryn_db::testing::scenario_steps(&s.pool, sc.id).await?;
        let mut steps = Vec::with_capacity(rows.len());
        for r in rows {
            let step: ExecStep = serde_json::from_value(r.step).map_err(|e| {
                ApiError::Internal(anyhow::anyhow!(
                    "stored step for {} is invalid: {e}",
                    sc.name
                ))
            })?;
            steps.push(PlannedStep {
                index: r.position as usize,
                keyword: r.keyword,
                text: r.text,
                step,
            });
        }
        let job_id = oryn_db::new_id();
        let spec = JobSpec {
            protocol: JOB_PROTOCOL.into(),
            job_id,
            execution_id,
            project_id: p.id(),
            scenario_id: Some(sc.id),
            scenario_name: sc.name.clone(),
            attempt: 1,
            steps,
            environment: environment.clone(),
            options: options.clone(),
        };
        jobs.push(NewJob {
            id: job_id,
            scenario_id: Some(sc.id),
            scenario_name: sc.name.clone(),
            kind: sc.kind.clone(),
            spec: serde_json::to_value(&spec).map_err(|e| ApiError::Internal(e.into()))?,
            max_attempts,
            retry_on_failure: req.retry_on_failure,
        });
    }
    let trigger = req.trigger.clone().unwrap_or_else(|| "manual".into());
    if !["manual", "api", "ci", "schedule", "regression", "rerun"].contains(&trigger.as_str()) {
        return Err(ApiError::bad("unknown trigger"));
    }
    let title = req
        .title
        .clone()
        .filter(|t| !t.trim().is_empty())
        .unwrap_or_else(|| match (req.suite_id, run.len()) {
            (Some(_), _) => run[0].suite_name.clone(),
            (None, 1) => run[0].name.clone(),
            (None, n) => format!("{n} scenarios"),
        });
    let new = NewExecution {
        id: execution_id,
        workspace_id: p.ws_id(),
        project_id: p.id(),
        environment_id: Some(env_row.id),
        environment_name: env_row.name.clone(),
        suite_id: req.suite_id,
        title: title.chars().take(200).collect(),
        trigger: trigger.clone(),
        browser: options.browser.to_string(),
        rerun_of: req.rerun_of,
        commit_sha: req.commit_sha.clone(),
        branch: req.branch.clone(),
        triggered_by: Some(p.user_id()),
    };
    oryn_db::execution::create_execution(&s.pool, &new, &jobs).await?;
    audit(
        &s.pool,
        Some(p.ws_id()),
        &p.actor(),
        "execution.created",
        Some("execution"),
        Some(execution_id),
        json!({"jobs": jobs.len(), "trigger": trigger, "skipped_unapproved": blocked.len()}),
    )
    .await;
    Ok(oryn_db::execution::get_execution(&s.pool, execution_id).await?)
}

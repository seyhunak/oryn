//! Failure investigation: deterministic observations + classification, optional AI interpretation.

use crate::error::{ApiError, ApiResult};
use crate::state::AppState;
use oryn_db::ai::Investigation;
use oryn_intelligence::investigator::{self, InvestigationInput, StepFact};
use serde_json::json;
use uuid::Uuid;

fn host_port(u: &str) -> Option<String> {
    let u = url::Url::parse(u).ok()?;
    let h = u.host_str()?.to_ascii_lowercase();
    Some(match u.port() {
        Some(p) => format!("{h}:{p}"),
        None => h,
    })
}

pub async fn build_input(
    s: &AppState,
    job_id: Uuid,
    passed_on_retry: bool,
) -> ApiResult<(InvestigationInput, oryn_db::execution::Job, String)> {
    let job = oryn_db::execution::get_job(&s.pool, job_id).await?;
    let steps = oryn_db::execution::job_steps(&s.pool, job_id).await?;
    let ev = oryn_db::execution::job_evidence(&s.pool, job_id).await?;
    let spec = oryn_db::execution::job_spec(&s.pool, job_id).await?;
    let history = match job.scenario_id {
        Some(sid) => oryn_db::execution::scenario_history(&s.pool, sid, 20)
            .await?
            .into_iter()
            .filter(|r| {
                !r.superseded && r.execution_id != job.execution_id && r.finished_at.is_some()
            })
            .map(|r| r.status)
            .collect(),
        None => vec![],
    };
    let mut base_hosts = vec![];
    for k in ["base_url", "api_base_url"] {
        if let Some(h) = spec["environment"][k].as_str().and_then(host_port) {
            base_hosts.push(h);
        }
    }
    let gherkin = steps
        .iter()
        .map(|st| format!("{} {}", st.keyword.trim(), st.text))
        .collect::<Vec<_>>()
        .join("\n");
    let input = InvestigationInput {
        scenario_name: job.scenario_name.clone(),
        job_status: job.status.clone(),
        error: job.error.clone(),
        attempt: job.attempt,
        steps: steps
            .into_iter()
            .map(|st| StepFact {
                position: st.position,
                keyword: st.keyword,
                text: st.text,
                status: st.status,
                message: st.message,
                expected: st.expected,
                actual: st.actual,
                observation: st.observation,
                artifact: st.artifact_name,
            })
            .collect(),
        logs: ev.logs,
        network: ev.network,
        history,
        passed_on_retry,
        base_hosts,
    };
    Ok((input, job, gherkin))
}

/// Create an investigation for a job. `ai` = Some((user)) asks the configured model for an
/// interpretation; the deterministic category is kept unless it was `unknown`.
pub async fn investigate(
    s: &AppState,
    job_id: Uuid,
    passed_on_retry: bool,
    ai_for: Option<Option<Uuid>>,
) -> ApiResult<Investigation> {
    let (input, job, gherkin) = build_input(s, job_id, passed_on_retry).await?;
    let d = investigator::classify(&input);
    let mut category = d.category.clone();
    let mut interpretation = None;
    let mut ai_request_id = None;
    if let Some(user) = ai_for {
        let ws = oryn_db::projects::get_project(&s.pool, job.project_id)
            .await?
            .workspace_id;
        let ai = s.ai(ws).await?.ok_or_else(|| {
            ApiError::unprocessable("no AI provider is configured for this workspace")
        })?;
        let req = investigator::ai_request(&input, &d, Some(&gherkin), &ai.model);
        let (res, rid) = s.ai_call(ws, Some(job.project_id), user, &ai, req).await?;
        ai_request_id = Some(rid);
        if let Some(v) = res.json {
            if category == "unknown" {
                if let Some(c) = v["category"].as_str() {
                    category = c.to_string();
                }
            }
            interpretation = Some(
                json!({"source": "ai", "model": res.model, "label": "AI interpretation", "output": v}),
            );
        }
    }
    let inv = oryn_db::ai::insert_investigation(
        &s.pool,
        job_id,
        job.execution_id,
        job.project_id,
        &category,
        &d.category,
        d.confidence,
        &d.summary,
        &d.root_cause,
        &json!({"rule": d.rule, "items": d.observations}),
        interpretation.as_ref(),
        &json!(d.investigate),
        &json!(d.remediate),
        ai_request_id,
        ai_for.flatten(),
    )
    .await?;
    Ok(inv)
}

/// Called after a job completes: investigate final failures, and the failed attempt
/// of a scenario that passed on retry (recorded as flaky).
pub fn after_completion(s: AppState, job_id: Uuid) {
    tokio::spawn(async move {
        let Ok(job) = oryn_db::execution::get_job(&s.pool, job_id).await else {
            return;
        };
        if job.superseded {
            return; // a retry follows; investigate the final attempt instead
        }
        let target = match job.status.as_str() {
            "failed" | "error" | "timed_out" => Some((job_id, false)),
            "passed" if job.attempt > 1 => {
                let jobs = oryn_db::execution::execution_jobs(&s.pool, job.execution_id)
                    .await
                    .unwrap_or_default();
                jobs.into_iter()
                    .filter(|j| {
                        j.scenario_id == job.scenario_id
                            && j.superseded
                            && matches!(j.status.as_str(), "failed" | "error" | "timed_out")
                    })
                    .max_by_key(|j| j.attempt)
                    .map(|j| (j.id, true))
            }
            _ => None,
        };
        if let Some((jid, flaky)) = target {
            if let Err(e) = investigate(&s, jid, flaky, None).await {
                tracing::warn!(error = %e, job = %jid, "automatic investigation failed");
            }
        }
    });
}

//! Database integration tests. Require DATABASE_URL (a Postgres with pgvector);
//! `sqlx::test` creates an isolated database per test and runs migrations.

use chrono::Utc;
use oryn_db::execution::{self, NewExecution, NewJob};
use oryn_db::{identity, projects, PgPool};
use oryn_domain::{JobResult, JobStatus, StepResult, StepStatus, JOB_PROTOCOL};
use uuid::Uuid;

async fn fixture(pool: &PgPool, jobs: usize, max_attempts: i32) -> (Uuid, Uuid, Vec<Uuid>) {
    let u = identity::create_user(pool, "a@b.test", "A", Some("x"), false)
        .await
        .unwrap();
    let ws = identity::create_workspace(pool, "WS", u.id).await.unwrap();
    let p = projects::create_project(pool, ws.id, "Proj", None, "workspace", "thu", u.id)
        .await
        .unwrap();
    let e = NewExecution {
        id: Uuid::now_v7(),
        workspace_id: ws.id,
        project_id: p.id,
        environment_id: None,
        environment_name: "local".into(),
        suite_id: None,
        title: "t".into(),
        trigger: "manual".into(),
        browser: "chromium".into(),
        rerun_of: None,
        commit_sha: None,
        branch: None,
        triggered_by: Some(u.id),
    };
    let js: Vec<NewJob> = (0..jobs)
        .map(|i| {
            let id = Uuid::now_v7();
            NewJob {
                id,
                scenario_id: None,
                scenario_name: format!("s{i}"),
                kind: "api".into(),
                spec: serde_json::json!({"job_id": id, "attempt": 1}),
                max_attempts,
                retry_on_failure: false,
            }
        })
        .collect();
    let ids = js.iter().map(|j| j.id).collect();
    execution::create_execution(pool, &e, &js).await.unwrap();
    (ws.id, e.id, ids)
}

async fn worker(pool: &PgPool, ws: Option<Uuid>, name: &str) -> Uuid {
    execution::register_worker(
        pool,
        ws,
        name,
        &format!("hash-{name}-{}", Uuid::new_v4()),
        "0.1",
        "h",
        "linux",
        4,
        &[],
        &[],
    )
    .await
    .unwrap()
}

fn result(job: Uuid, status: JobStatus) -> JobResult {
    JobResult {
        protocol: JOB_PROTOCOL.into(),
        job_id: job,
        status,
        started_at: Utc::now(),
        finished_at: Utc::now(),
        duration_ms: 5,
        steps: vec![StepResult {
            index: 0,
            keyword: "Then".into(),
            text: "x".into(),
            status: if status == JobStatus::Passed {
                StepStatus::Passed
            } else {
                StepStatus::Failed
            },
            started_at: Utc::now(),
            duration_ms: 1,
            observation: None,
            message: None,
            expected: None,
            actual: None,
            artifact: None,
        }],
        logs: vec![],
        network: vec![],
        artifacts: vec![],
        browser_version: None,
        error: None,
    }
}

fn no_retry(_: JobStatus, _: i32, _: i32, _: bool) -> Option<u64> {
    None
}
fn retry_errors(s: JobStatus, a: i32, m: i32, _: bool) -> Option<u64> {
    (matches!(s, JobStatus::Error | JobStatus::TimedOut) && a < m).then_some(0)
}

#[sqlx::test(migrator = "oryn_db::MIGRATOR")]
async fn concurrent_claims_are_exclusive(pool: PgPool) {
    let (ws, _, ids) = fixture(&pool, 20, 1).await;
    let mut handles = vec![];
    for i in 0..6 {
        let pool = pool.clone();
        let w = worker(&pool, Some(ws), &format!("w{i}")).await;
        handles.push(tokio::spawn(async move {
            let mut got = vec![];
            while let Some(j) = execution::claim_job(&pool, w, Some(ws), &["api".into()], 60)
                .await
                .unwrap()
            {
                got.push(j.id);
            }
            got
        }));
    }
    let mut all = vec![];
    for h in handles {
        all.extend(h.await.unwrap());
    }
    all.sort();
    let n = all.len();
    all.dedup();
    assert_eq!(n, 20);
    assert_eq!(all.len(), ids.len());
}

#[sqlx::test(migrator = "oryn_db::MIGRATOR")]
async fn workspace_isolation(pool: PgPool) {
    let (ws, _, _) = fixture(&pool, 1, 1).await;
    let other = worker(&pool, None, "shared").await;
    let foreign_ws = identity::create_workspace(
        &pool,
        "Other",
        identity::create_user(&pool, "z@z.test", "Z", None, false)
            .await
            .unwrap()
            .id,
    )
    .await
    .unwrap();
    let foreign = worker(&pool, Some(foreign_ws.id), "foreign").await;
    assert!(
        execution::claim_job(&pool, foreign, Some(foreign_ws.id), &["api".into()], 60)
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        execution::claim_job(&pool, other, None, &["browser".into()], 60)
            .await
            .unwrap()
            .is_none(),
        "capability mismatch"
    );
    assert!(
        execution::claim_job(&pool, other, None, &["api".into()], 60)
            .await
            .unwrap()
            .is_some(),
        "shared worker serves any workspace"
    );
    let _ = ws;
}

#[sqlx::test(migrator = "oryn_db::MIGRATOR")]
async fn lifecycle_retry_and_recompute(pool: PgPool) {
    let (ws, exec, _) = fixture(&pool, 2, 2).await;
    let w = worker(&pool, Some(ws), "w").await;
    let a = execution::claim_job(&pool, w, Some(ws), &["api".into()], 60)
        .await
        .unwrap()
        .unwrap();
    execution::start_job(&pool, a.id, w).await.unwrap();
    assert!(!execution::heartbeat_job(&pool, a.id, w, 60).await.unwrap());
    let c = execution::complete_job(&pool, a.id, w, &result(a.id, JobStatus::Passed), &no_retry)
        .await
        .unwrap();
    assert!(!c.execution_finished);
    assert_eq!(execution::job_steps(&pool, a.id).await.unwrap().len(), 1);

    let b = execution::claim_job(&pool, w, Some(ws), &["api".into()], 60)
        .await
        .unwrap()
        .unwrap();
    let c = execution::complete_job(
        &pool,
        b.id,
        w,
        &result(b.id, JobStatus::Error),
        &retry_errors,
    )
    .await
    .unwrap();
    assert!(c.retried_job.is_some(), "error retried");
    assert!(!c.execution_finished);
    let retry = execution::claim_job(&pool, w, Some(ws), &["api".into()], 60)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(retry.attempt, 2);
    assert_eq!(retry.spec["attempt"], 2);
    let c = execution::complete_job(
        &pool,
        retry.id,
        w,
        &result(retry.id, JobStatus::Passed),
        &retry_errors,
    )
    .await
    .unwrap();
    assert!(c.execution_finished);
    let e = execution::get_execution(&pool, exec).await.unwrap();
    assert_eq!(e.status, "passed");
    assert_eq!(
        (e.total, e.passed, e.errored),
        (2, 2, 0),
        "superseded attempt not counted"
    );
    assert!(
        execution::complete_job(
            &pool,
            retry.id,
            w,
            &result(retry.id, JobStatus::Passed),
            &no_retry
        )
        .await
        .is_err(),
        "double completion rejected"
    );
    let jobs = execution::execution_jobs(&pool, exec).await.unwrap();
    assert_eq!(jobs.len(), 3, "history kept");
}

#[sqlx::test(migrator = "oryn_db::MIGRATOR")]
async fn cancellation_and_lease_expiry(pool: PgPool) {
    let (ws, exec, _) = fixture(&pool, 3, 2).await;
    let w = worker(&pool, Some(ws), "w").await;
    let a = execution::claim_job(&pool, w, Some(ws), &["api".into()], 60)
        .await
        .unwrap()
        .unwrap();
    let (st, done) = execution::cancel_execution(&pool, exec).await.unwrap();
    assert_eq!(st.as_str(), "running");
    assert!(!done);
    assert!(
        execution::heartbeat_job(&pool, a.id, w, 60).await.unwrap(),
        "worker sees cancel request"
    );
    let mut r = result(a.id, JobStatus::Cancelled);
    r.steps.clear();
    let c = execution::complete_job(&pool, a.id, w, &r, &retry_errors)
        .await
        .unwrap();
    assert!(c.execution_finished);
    assert_eq!(
        execution::get_execution(&pool, exec).await.unwrap().status,
        "cancelled"
    );

    // Lease expiry: claim with a zero-second lease, reap → retried, then timed out.
    let (ws2, exec2, _) = fixture_named(&pool).await;
    let w2 = worker(&pool, Some(ws2), "w2").await;
    let j = execution::claim_job(&pool, w2, Some(ws2), &["api".into()], 0)
        .await
        .unwrap()
        .unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    execution::reap_expired_leases(&pool).await.unwrap();
    assert_eq!(
        execution::get_job(&pool, j.id).await.unwrap().status,
        "timed_out"
    );
    let j2 = execution::claim_job(&pool, w2, Some(ws2), &["api".into()], 0)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(j2.attempt, 2);
    tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    let finished = execution::reap_expired_leases(&pool).await.unwrap();
    assert_eq!(finished.len(), 1);
    assert_eq!(
        execution::get_execution(&pool, exec2).await.unwrap().status,
        "error"
    );
}

async fn fixture_named(pool: &PgPool) -> (Uuid, Uuid, Vec<Uuid>) {
    let u = identity::create_user(pool, "c@d.test", "C", None, false)
        .await
        .unwrap();
    let ws = identity::create_workspace(pool, "WS2", u.id).await.unwrap();
    let p = projects::create_project(pool, ws.id, "P2", None, "workspace", "thu", u.id)
        .await
        .unwrap();
    let id = Uuid::now_v7();
    let e = NewExecution {
        id: Uuid::now_v7(),
        workspace_id: ws.id,
        project_id: p.id,
        environment_id: None,
        environment_name: "x".into(),
        suite_id: None,
        title: "t".into(),
        trigger: "manual".into(),
        browser: "chromium".into(),
        rerun_of: None,
        commit_sha: None,
        branch: None,
        triggered_by: None,
    };
    execution::create_execution(
        pool,
        &e,
        &[NewJob {
            id,
            scenario_id: None,
            scenario_name: "s".into(),
            kind: "api".into(),
            spec: serde_json::json!({}),
            max_attempts: 2,
            retry_on_failure: false,
        }],
    )
    .await
    .unwrap();
    (ws.id, e.id, vec![id])
}

#[sqlx::test(migrator = "oryn_db::MIGRATOR")]
async fn role_matrix_matches_code(pool: PgPool) {
    use oryn_domain::{Permission, Role};
    let rows = identity::role_permission_matrix(&pool).await.unwrap();
    for role in Role::ALL {
        let mut db: Vec<String> = rows
            .iter()
            .filter(|(r, _)| r == role.as_str())
            .map(|(_, p)| p.clone())
            .collect();
        let mut code: Vec<String> = role
            .permissions()
            .iter()
            .map(|p: &Permission| p.as_str().to_string())
            .collect();
        db.sort();
        code.sort();
        assert_eq!(db, code, "role {role}");
    }
}

/// Every value the code writes must satisfy the schema's CHECK constraints.
#[sqlx::test(migrator = "oryn_db::MIGRATOR")]
async fn checks_accept_values_the_code_writes(pool: PgPool) {
    let def = |name: &'static str| {
        let pool = pool.clone();
        async move {
            sqlx::query_scalar::<_, String>(
                "SELECT pg_get_constraintdef(oid) FROM pg_constraint WHERE conname = $1",
            )
            .bind(name)
            .fetch_one(&pool)
            .await
            .unwrap()
        }
    };
    let vis = def("projects_visibility_check").await;
    assert!(vis.contains("'private'"), "{vis}");
    let cat = def("test_plan_items_category_check").await;
    // The categories the planners emit (oryn-intelligence planner.rs).
    for c in [
        "functional",
        "negative",
        "edge_case",
        "integration",
        "smoke",
        "regression",
        "security",
        "accessibility",
    ] {
        assert!(cat.contains(&format!("'{c}'")), "{c} missing from {cat}");
    }
    let events: String = sqlx::query_scalar(
        "SELECT column_default FROM information_schema.columns WHERE table_name = 'integrations' AND column_name = 'events'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(events.contains("execution.finished"), "{events}");
}

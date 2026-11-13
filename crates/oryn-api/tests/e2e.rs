//! End-to-end vertical slice through the HTTP API with a real database and the real
//! Rust execution engine standing in for a remote worker (no browser needed: the
//! scenario is an API test against a local stub application).
//!
//! Requires DATABASE_URL (Postgres with pgvector). `sqlx::test` creates an isolated database.

use axum::body::Body;
use axum::http::{header, Request, StatusCode};
use axum::routing::get;
use axum::{Json, Router};
use http_body_util::BodyExt;
use oryn_api::{app, config::Config, AppState};
use oryn_db::PgPool;
use oryn_domain::job::JobSpec;
use oryn_execution::ScenarioRunner;
use serde_json::{json, Value};
use tower::ServiceExt;

struct Client {
    app: Router,
    cookie: Option<String>,
    bearer: Option<String>,
}

impl Client {
    async fn call(
        &self,
        method: &str,
        path: &str,
        body: Option<Value>,
    ) -> (StatusCode, Value, Option<String>) {
        let mut b = Request::builder()
            .method(method)
            .uri(path)
            .header("x-oryn-csrf", "1");
        if let Some(c) = &self.cookie {
            b = b.header(header::COOKIE, c);
        }
        if let Some(t) = &self.bearer {
            b = b.header(header::AUTHORIZATION, format!("Bearer {t}"));
        }
        let req = match body {
            Some(v) => b
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(v.to_string()))
                .unwrap(),
            None => b.body(Body::empty()).unwrap(),
        };
        let resp = self.app.clone().oneshot(req).await.unwrap();
        let status = resp.status();
        let cookie = resp
            .headers()
            .get(header::SET_COOKIE)
            .and_then(|v| v.to_str().ok())
            .map(|v| v.split(';').next().unwrap().to_string());
        let bytes = resp.into_body().collect().await.unwrap().to_bytes();
        let v = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
        (status, v, cookie)
    }
    async fn ok(&self, method: &str, path: &str, body: Option<Value>) -> Value {
        let (s, v, _) = self.call(method, path, body).await;
        assert!(s.is_success(), "{method} {path} → {s}: {v}");
        v
    }
}

async fn stub_app() -> String {
    let r = Router::new().route(
        "/api/health",
        get(|| async { Json(json!({"status": "ok", "items": [1, 2, 3]})) }),
    );
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let a = l.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(l, r).await.unwrap() });
    format!("http://{a}")
}

const FEATURE: &str = r#"Feature: Health API

  @api
  Scenario: Health responds
    When I send a GET request to "/api/health"
    Then the response status should be 200
    And the response JSON "$.status" should equal "ok"
    And the response JSON "$.items" should have length 3

  @api
  Scenario: Health has a wrong expectation
    When I send a GET request to "/api/health"
    Then the response JSON "$.status" should equal "degraded"
"#;

#[sqlx::test(migrator = "oryn_db::MIGRATOR")]
async fn vertical_slice(pool: PgPool) {
    let dir = tempfile::tempdir().unwrap();
    let state = AppState::new(Config::for_tests("unused", dir.path()), pool.clone())
        .await
        .unwrap();
    let app = app(state);
    let mut c = Client {
        app: app.clone(),
        cookie: None,
        bearer: None,
    };

    // Unauthenticated access is refused.
    let (s, _, _) = c.call("GET", "/api/v1/auth/me", None).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);

    // Register (first user = instance admin, gets a workspace).
    let (s, me, cookie) = c.call("POST", "/api/v1/auth/register", Some(json!({"email": "owner@acme.test", "name": "Owner", "password": "correct horse battery"}))).await;
    assert_eq!(s, StatusCode::CREATED, "{me}");
    c.cookie = cookie;
    let ws = me["workspaces"][0]["id"].as_str().unwrap().to_string();
    assert_eq!(me["workspaces"][0]["role"], "owner");

    // CSRF: cookie-authenticated writes without the header are refused.
    let req = Request::builder()
        .method("POST")
        .uri(format!("/api/v1/workspaces/{ws}/projects"))
        .header(header::COOKIE, c.cookie.clone().unwrap())
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(json!({"name": "x"}).to_string()))
        .unwrap();
    assert_eq!(
        app.clone().oneshot(req).await.unwrap().status(),
        StatusCode::FORBIDDEN
    );

    // Project + environment with a secret.
    let base = stub_app().await;
    let p = c
        .ok(
            "POST",
            &format!("/api/v1/workspaces/{ws}/projects"),
            Some(json!({"name": "Northwind"})),
        )
        .await;
    let pid = p["id"].as_str().unwrap().to_string();
    let env = c.ok("POST", &format!("/api/v1/projects/{pid}/environments"), Some(json!({
        "name": "local", "base_url": base, "is_default": true, "variables": {"region": "eu"}, "secrets": {"api_key": "s3cr3t-value"}
    }))).await;
    assert_eq!(env["secret_names"], json!(["api_key"]));
    assert!(
        !env.to_string().contains("s3cr3t-value"),
        "secret values are never returned"
    );

    // Requirements import + traceability via tags.
    let imp = c
        .ok(
            "POST",
            &format!("/api/v1/projects/{pid}/requirements/import"),
            Some(json!({"content": "## US-1 Health\n- `GET /api/health` returns ok (HTTP 200)\n"})),
        )
        .await;
    assert_eq!(imp["created"][0]["key"], "US-1");

    // Validate then save the suite; scenarios start as drafts.
    let v = c
        .ok(
            "POST",
            &format!("/api/v1/projects/{pid}/gherkin/validate"),
            Some(json!({"source": "Feature: X\n  Scenario: Y\n    Given I fly\n"})),
        )
        .await;
    assert_eq!(v["ok"], false);
    let tagged = FEATURE.replace(
        "@api\n  Scenario: Health responds",
        "@api @US-1\n  Scenario: Health responds",
    );
    let saved = c
        .ok(
            "POST",
            &format!("/api/v1/projects/{pid}/suites"),
            Some(json!({"source": tagged})),
        )
        .await;
    assert_eq!(saved["validation"]["ok"], true, "{saved}");
    let suite_id = saved["suite_id"].as_str().unwrap().to_string();
    let scenarios = c
        .ok(
            "GET",
            &format!("/api/v1/projects/{pid}/scenarios?suite_id={suite_id}"),
            None,
        )
        .await;
    let ids: Vec<String> = scenarios
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["id"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(ids.len(), 2);
    assert!(scenarios
        .as_array()
        .unwrap()
        .iter()
        .all(|s| s["status"] == "draft"));

    // Workspaces require approval by default: running drafts is refused.
    let (s, body, _) = c
        .call(
            "POST",
            &format!("/api/v1/projects/{pid}/executions"),
            Some(json!({"suite_id": suite_id})),
        )
        .await;
    assert_eq!(s, StatusCode::UNPROCESSABLE_ENTITY, "{body}");

    // Approve and run.
    c.ok(
        "POST",
        &format!("/api/v1/projects/{pid}/scenarios/status"),
        Some(json!({"scenario_ids": ids, "status": "approved"})),
    )
    .await;
    let exe = c
        .ok(
            "POST",
            &format!("/api/v1/projects/{pid}/executions"),
            Some(json!({"suite_id": suite_id, "max_attempts": 1})),
        )
        .await;
    let eid = exe["id"].as_str().unwrap().to_string();
    assert_eq!(exe["status"], "queued");
    assert_eq!(exe["total"], 2);

    // A worker registers with the bootstrap token, claims, executes with the real engine, reports.
    let w = Client {
        app: app.clone(),
        cookie: None,
        bearer: None,
    };
    let creds = w.ok("POST", "/api/v1/worker/register", Some(json!({"bootstrap_token": "bootstrap-test-token", "name": "ci-worker", "capabilities": [{"capability": "api"}]}))).await;
    let w = Client {
        app: app.clone(),
        cookie: None,
        bearer: Some(creds["token"].as_str().unwrap().to_string()),
    };
    let runner = ScenarioRunner::new(None);
    let mut completed = 0;
    loop {
        let (s, claimed, _) = w
            .call(
                "POST",
                "/api/v1/worker/claim",
                Some(json!({"kinds": ["api"]})),
            )
            .await;
        if s == StatusCode::NO_CONTENT {
            break;
        }
        assert_eq!(s, StatusCode::OK, "{claimed}");
        assert_eq!(
            claimed["secrets"]["api_key"], "s3cr3t-value",
            "secrets are decrypted for the claiming worker only"
        );
        let job_id = claimed["job_id"].as_str().unwrap().to_string();
        w.ok("POST", &format!("/api/v1/worker/jobs/{job_id}/start"), None)
            .await;
        let hb = w
            .ok(
                "POST",
                &format!("/api/v1/worker/jobs/{job_id}/heartbeat"),
                None,
            )
            .await;
        assert_eq!(hb["cancel_requested"], false);
        let mut spec: JobSpec = serde_json::from_value(claimed["spec"].clone()).unwrap();
        for (k, v) in claimed["secrets"].as_object().unwrap() {
            spec.environment
                .variables
                .insert(k.clone(), v.as_str().unwrap().to_string());
        }
        let out = runner
            .run(&spec, tokio_util::sync::CancellationToken::new())
            .await;
        for a in &out.artifacts {
            let req = Request::builder()
                .method("POST")
                .uri(format!(
                    "/api/v1/worker/jobs/{job_id}/artifacts?name={}&kind={}&content_type={}",
                    a.name, a.kind, a.content_type
                ))
                .header(
                    header::AUTHORIZATION,
                    format!("Bearer {}", w.bearer.clone().unwrap()),
                )
                .body(Body::from(a.bytes.clone()))
                .unwrap();
            assert_eq!(
                app.clone().oneshot(req).await.unwrap().status(),
                StatusCode::CREATED
            );
        }
        w.ok(
            "POST",
            &format!("/api/v1/worker/jobs/{job_id}/complete"),
            Some(serde_json::to_value(&out.result).unwrap()),
        )
        .await;
        completed += 1;
    }
    assert_eq!(completed, 2);

    // The verdict is the engine's: one pass, one failure with expected vs actual.
    let detail = c
        .ok("GET", &format!("/api/v1/executions/{eid}"), None)
        .await;
    assert_eq!(detail["execution"]["status"], "failed", "{detail}");
    assert_eq!(detail["execution"]["passed"], 1);
    assert_eq!(detail["execution"]["failed"], 1);
    let failed_job = detail["jobs"]
        .as_array()
        .unwrap()
        .iter()
        .find(|j| j["status"] == "failed")
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();
    let job = c
        .ok("GET", &format!("/api/v1/jobs/{failed_job}"), None)
        .await;
    let step = job["steps"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["status"] == "failed")
        .unwrap();
    assert!(step["expected"].as_str().unwrap().contains("degraded"));
    assert!(
        !job.to_string().contains("s3cr3t-value"),
        "evidence is redacted"
    );
    assert!(
        !detail["artifacts"].as_array().unwrap().is_empty(),
        "response evidence was uploaded"
    );
    let aid = detail["artifacts"][0]["id"].as_str().unwrap();
    let (s, _, _) = c
        .call("GET", &format!("/api/v1/artifacts/{aid}/content"), None)
        .await;
    assert_eq!(s, StatusCode::OK);

    // Deterministic investigation is created automatically for the final failure.
    let mut inv = Value::Null;
    for _ in 0..50 {
        let d = c
            .ok("GET", &format!("/api/v1/executions/{eid}"), None)
            .await;
        if let Some(i) = d["investigations"].as_array().and_then(|a| a.first()) {
            inv = i.clone();
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    assert_eq!(inv["category"], "application_defect", "{inv}");
    assert!(
        inv["interpretation"].is_null(),
        "no AI configured → no interpretation"
    );

    // Traceability: US-1 is covered by the tagged scenario, which passed.
    let tr = c
        .ok("GET", &format!("/api/v1/projects/{pid}/traceability"), None)
        .await;
    assert_eq!(tr["coverage"]["covered"], 1, "{tr}");

    // Metrics come from the run.
    let ov = c
        .ok("GET", &format!("/api/v1/workspaces/{ws}/overview"), None)
        .await;
    assert_eq!(ov["jobs_passed"], 1);
    assert_eq!(ov["jobs_failed"], 1);
    assert_eq!(ov["pass_rate"], 0.5);

    // SSE stream closes after the terminal state.
    let req = Request::builder()
        .uri(format!("/api/v1/executions/{eid}/events"))
        .header(header::COOKIE, c.cookie.clone().unwrap())
        .body(Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let text = String::from_utf8(
        resp.into_body()
            .collect()
            .await
            .unwrap()
            .to_bytes()
            .to_vec(),
    )
    .unwrap();
    assert!(
        text.contains("event: job") && text.contains("event: end"),
        "{text}"
    );

    // Repository intelligence: import a local source tree, detect routes, select regressions.
    std::fs::create_dir_all(dir.path().join("app/src")).unwrap();
    std::fs::write(
        dir.path().join("app/package.json"),
        r#"{"name":"stub","dependencies":{"express":"4.19.0"}}"#,
    )
    .unwrap();
    std::fs::write(
        dir.path().join("app/src/health.js"),
        "app.get('/api/health', (req, res) => res.json({status: 'ok'}));\n",
    )
    .unwrap();
    let repo = c
        .ok(
            "POST",
            &format!("/api/v1/projects/{pid}/repositories"),
            Some(json!({"source": "local", "path": "app"})),
        )
        .await;
    let mut ready = false;
    for _ in 0..100 {
        let list = c
            .ok("GET", &format!("/api/v1/projects/{pid}/repositories"), None)
            .await;
        let r = list
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["id"] == repo["id"])
            .unwrap()
            .clone();
        assert_ne!(r["status"], "failed", "{r}");
        if r["status"] == "ready" {
            ready = true;
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    assert!(ready);
    let ctx = c
        .ok("GET", &format!("/api/v1/projects/{pid}/context"), None)
        .await;
    assert!(ctx.to_string().contains("/api/health"), "{ctx}");
    c.ok(
        "GET",
        &format!("/api/v1/projects/{pid}/context/search?q=health"),
        None,
    )
    .await;
    let sel = c
        .ok(
            "POST",
            &format!("/api/v1/projects/{pid}/regression/select"),
            Some(json!({"changed_files": ["src/health.js"]})),
        )
        .await;
    assert_eq!(sel["selections"].as_array().unwrap().len(), 2, "{sel}");
    assert!(sel["selections"][0]["reasons"][0]
        .as_str()
        .unwrap()
        .contains("/api/health"));

    // Every analytics read works on real data.
    let trends = c
        .ok(
            "GET",
            &format!("/api/v1/workspaces/{ws}/trends?days=7"),
            None,
        )
        .await;
    assert_eq!(trends.as_array().unwrap().len(), 7);
    c.ok("GET", &format!("/api/v1/workspaces/{ws}/flaky"), None)
        .await;
    let fails = c
        .ok("GET", &format!("/api/v1/workspaces/{ws}/failures"), None)
        .await;
    assert_eq!(fails.as_array().unwrap().len(), 1);
    let q = c
        .ok("GET", &format!("/api/v1/projects/{pid}/quality"), None)
        .await;
    assert_eq!(q["coverage"]["covered"], 1, "{q}");
    let recs = c
        .ok(
            "POST",
            &format!("/api/v1/projects/{pid}/recommendations/refresh"),
            None,
        )
        .await;
    assert_eq!(
        recs,
        json!([]),
        "nothing to recommend yet: US-1 is covered and verified"
    );
    c.ok(
        "POST",
        &format!("/api/v1/projects/{pid}/requirements"),
        Some(json!({"key": "US-9", "title": "Export", "body": "CSV export"})),
    )
    .await;
    let recs = c
        .ok(
            "POST",
            &format!("/api/v1/projects/{pid}/recommendations/refresh"),
            None,
        )
        .await;
    assert!(recs.to_string().contains("US-9 has no test"), "{recs}");
    c.ok("GET", &format!("/api/v1/workspaces/{ws}/executions"), None)
        .await;
    c.ok("GET", &format!("/api/v1/workspaces/{ws}/workers"), None)
        .await;
    c.ok("GET", &format!("/api/v1/workspaces/{ws}/queue"), None)
        .await;
    c.ok("GET", "/api/v1/step-library", None).await;
    let spec = c.ok("GET", "/api/openapi.json", None).await;
    assert!(spec["paths"].as_object().unwrap().len() >= 90);

    // RBAC: a viewer can read but not run; an API token is capped by its role.
    let (_, viewer_me, viewer_cookie) = Client { app: app.clone(), cookie: None, bearer: None }
        .call("POST", "/api/v1/auth/register", Some(json!({"email": "viewer@acme.test", "name": "Vera", "password": "another long password"}))).await;
    assert!(viewer_me["user"]["is_instance_admin"] == false);
    c.ok(
        "POST",
        &format!("/api/v1/workspaces/{ws}/members"),
        Some(json!({"email": "viewer@acme.test", "role": "viewer"})),
    )
    .await;
    // Teams: membership is listable.
    let team = c
        .ok(
            "POST",
            &format!("/api/v1/workspaces/{ws}/teams"),
            Some(json!({"name": "QA"})),
        )
        .await;
    let tid = team["id"].as_str().unwrap();
    let vid = viewer_me["user"]["id"].as_str().unwrap();
    c.ok(
        "PUT",
        &format!("/api/v1/workspaces/{ws}/teams/{tid}/members/{vid}"),
        None,
    )
    .await;
    let tm = c
        .ok(
            "GET",
            &format!("/api/v1/workspaces/{ws}/teams/{tid}/members"),
            None,
        )
        .await;
    assert_eq!(tm.as_array().unwrap().len(), 1);
    assert_eq!(tm[0]["email"], "viewer@acme.test");
    let v = Client {
        app: app.clone(),
        cookie: viewer_cookie,
        bearer: None,
    };
    v.ok("GET", &format!("/api/v1/executions/{eid}"), None)
        .await;
    // Private projects: visible to admins and explicit grants only.
    let private = c
        .ok(
            "POST",
            &format!("/api/v1/workspaces/{ws}/projects"),
            Some(json!({"name": "Secret", "visibility": "private"})),
        )
        .await;
    assert_eq!(private["visibility"], "private");
    let ppid = private["id"].as_str().unwrap();
    let (st, _, _) = v
        .call("GET", &format!("/api/v1/projects/{ppid}"), None)
        .await;
    assert!(
        st == 403 || st == 404,
        "viewer must not see a private project, got {st}"
    );
    let visible = v
        .ok("GET", &format!("/api/v1/workspaces/{ws}/projects"), None)
        .await;
    assert!(!visible.to_string().contains("Secret"));
    let (s, _, _) = v
        .call(
            "POST",
            &format!("/api/v1/projects/{pid}/executions"),
            Some(json!({"suite_id": suite_id})),
        )
        .await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    let (s, _, _) = v
        .call(
            "POST",
            &format!("/api/v1/workspaces/{ws}/members"),
            Some(json!({"email": "owner@acme.test", "role": "owner"})),
        )
        .await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    let tok = c
        .ok(
            "POST",
            &format!("/api/v1/workspaces/{ws}/tokens"),
            Some(json!({"name": "ci", "role": "viewer"})),
        )
        .await;
    let t = Client {
        app: app.clone(),
        cookie: None,
        bearer: Some(tok["secret"].as_str().unwrap().to_string()),
    };
    t.ok("GET", &format!("/api/v1/projects/{pid}"), None).await;
    let (s, _, _) = t
        .call(
            "POST",
            &format!("/api/v1/projects/{pid}/executions"),
            Some(json!({"suite_id": suite_id})),
        )
        .await;
    assert_eq!(s, StatusCode::FORBIDDEN, "token role ceiling applies");

    // Re-run only failures.
    let rr = c
        .ok(
            "POST",
            &format!("/api/v1/executions/{eid}/rerun"),
            Some(json!({"failed_only": true})),
        )
        .await;
    assert_eq!(rr["total"], 1);
    assert_eq!(rr["trigger"], "rerun");
    assert_eq!(
        rr["suite_id"],
        json!(suite_id),
        "a re-run stays attributed to its suite"
    );
    assert_eq!(rr["rerun_of"], json!(eid));

    // Audit trail recorded the important actions.
    let audit = c
        .ok(
            "GET",
            &format!("/api/v1/workspaces/{ws}/audit?per_page=100"),
            None,
        )
        .await;
    let actions: Vec<&str> = audit["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|a| a["action"].as_str().unwrap())
        .collect();
    for a in [
        "project.created",
        "environment.created",
        "suite.created",
        "scenario.approved",
        "execution.created",
        "member.added",
        "token.created",
    ] {
        assert!(actions.contains(&a), "missing audit {a}: {actions:?}");
    }
}

#[sqlx::test(migrator = "oryn_db::MIGRATOR")]
async fn heuristic_planner_and_approval(pool: PgPool) {
    let dir = tempfile::tempdir().unwrap();
    let state = AppState::new(Config::for_tests("unused", dir.path()), pool.clone())
        .await
        .unwrap();
    let mut c = Client {
        app: app(state),
        cookie: None,
        bearer: None,
    };
    let (_, me, cookie) = c
        .call(
            "POST",
            "/api/v1/auth/register",
            Some(json!({"email": "a@acme.test", "name": "A", "password": "correct horse battery"})),
        )
        .await;
    c.cookie = cookie;
    let ws = me["workspaces"][0]["id"].as_str().unwrap().to_string();
    let p = c
        .ok(
            "POST",
            &format!("/api/v1/workspaces/{ws}/projects"),
            Some(json!({"name": "Shop"})),
        )
        .await;
    let pid = p["id"].as_str().unwrap();
    c.ok("POST", &format!("/api/v1/projects/{pid}/requirements/import"), Some(json!({
        "content": "## US-2 Login\n- **AC-2.1** The page `/login` has fields labelled **Email** and **Password** and a **Sign in** button.\n- **AC-2.6** `GET /api/products` returns the catalogue (HTTP 200).\n"
    }))).await;

    // No provider configured: AI mode is refused, auto falls back to the heuristic planner.
    let (s, _, _) = c
        .call(
            "POST",
            &format!("/api/v1/projects/{pid}/plans"),
            Some(json!({"mode": "ai"})),
        )
        .await;
    assert_eq!(s, StatusCode::UNPROCESSABLE_ENTITY);
    let plan = c
        .ok(
            "POST",
            &format!("/api/v1/projects/{pid}/plans"),
            Some(json!({})),
        )
        .await;
    assert_eq!(plan["plan"]["source"], "heuristic");
    assert_eq!(plan["plan"]["status"], "draft");
    let items = plan["items"].as_array().unwrap();
    assert_eq!(items.len(), 2);
    assert!(items.iter().all(|i| i["executable"] == true), "{items:?}");
    let plan_id = plan["plan"]["id"].as_str().unwrap();

    // Edit an item into something unsupported: it becomes non-executable and is skipped at approval.
    let bad = c
        .ok(
            "PATCH",
            &format!(
                "/api/v1/projects/{pid}/plans/{plan_id}/items/{}",
                items[1]["id"].as_str().unwrap()
            ),
            Some(json!({"gherkin": "Feature: X\n  Scenario: Y\n    Given I teleport\n"})),
        )
        .await;
    assert_eq!(bad["executable"], false);
    let res = c
        .ok(
            "POST",
            &format!("/api/v1/projects/{pid}/plans/{plan_id}/approve"),
            Some(json!({"comment": "looks right"})),
        )
        .await;
    assert_eq!(res["suites_created"], 1);
    assert_eq!(res["skipped"].as_array().unwrap().len(), 1);
    let scenarios = c
        .ok("GET", &format!("/api/v1/projects/{pid}/scenarios"), None)
        .await;
    assert!(scenarios
        .as_array()
        .unwrap()
        .iter()
        .all(|s| s["status"] == "approved" && s["origin"] == "heuristic"));
    let tr = c
        .ok("GET", &format!("/api/v1/projects/{pid}/traceability"), None)
        .await;
    assert_eq!(tr["coverage"]["covered"], 1);

    // Mock provider: AI planning works end to end and usage is recorded.
    c.ok(
        "POST",
        &format!("/api/v1/workspaces/{ws}/ai/providers"),
        Some(json!({"kind": "mock", "name": "Mock", "is_default": true})),
    )
    .await;
    let ai_plan = c
        .ok(
            "POST",
            &format!("/api/v1/projects/{pid}/plans"),
            Some(json!({"mode": "ai", "instructions": "cover the api"})),
        )
        .await;
    assert_eq!(ai_plan["plan"]["source"], "ai");
    assert!(ai_plan["plan"]["ai_request_id"].is_string());
    let usage = c
        .ok("GET", &format!("/api/v1/workspaces/{ws}/ai/usage"), None)
        .await;
    assert_eq!(usage["total_requests"], 1);
    let ov = c
        .ok("GET", &format!("/api/v1/workspaces/{ws}/overview"), None)
        .await;
    assert!(ov["pass_rate"].is_null(), "no runs → no invented pass rate");
}

#[sqlx::test(migrator = "oryn_db::MIGRATOR")]
async fn account_lockout_counts_failures_only(pool: PgPool) {
    let dir = tempfile::tempdir().unwrap();
    let state = AppState::new(Config::for_tests("unused", dir.path()), pool)
        .await
        .unwrap();
    let c = Client {
        app: app(state),
        cookie: None,
        bearer: None,
    };
    let creds = json!({"email": "l@acme.test", "password": "correct horse battery"});
    c.ok(
        "POST",
        "/api/v1/auth/register",
        Some(json!({"email": "l@acme.test", "name": "L", "password": "correct horse battery"})),
    )
    .await;
    // Successful sign-ins never lock the account.
    for _ in 0..12 {
        c.ok("POST", "/api/v1/auth/login", Some(creds.clone()))
            .await;
    }
    // Ten failures do.
    for _ in 0..10 {
        let (s, _, _) = c
            .call(
                "POST",
                "/api/v1/auth/login",
                Some(json!({"email": "l@acme.test", "password": "wrong"})),
            )
            .await;
        assert_eq!(s, StatusCode::UNAUTHORIZED);
    }
    let (s, _, _) = c.call("POST", "/api/v1/auth/login", Some(creds)).await;
    assert_eq!(
        s,
        StatusCode::TOO_MANY_REQUESTS,
        "the correct password is refused while locked"
    );
}

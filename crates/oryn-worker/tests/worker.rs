//! The real worker loop against a real control plane over TCP.

use axum::routing::get;
use axum::{Json, Router};
use oryn_api::{app, config::Config, AppState};
use oryn_db::PgPool;
use oryn_worker::{Worker, WorkerConfig};
use serde_json::{json, Value};
use std::time::Duration;
use tokio_util::sync::CancellationToken;

async fn serve(r: Router) -> String {
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let a = l.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(l, r).await.unwrap() });
    format!("http://{a}")
}

struct User {
    base: String,
    cookie: String,
    http: reqwest::Client,
}

impl User {
    async fn call(&self, method: reqwest::Method, path: &str, body: Option<Value>) -> Value {
        let mut r = self
            .http
            .request(method.clone(), format!("{}{path}", self.base))
            .header("cookie", &self.cookie)
            .header("x-oryn-csrf", "1");
        if let Some(b) = body {
            r = r.json(&b);
        }
        let resp = r.send().await.unwrap();
        let s = resp.status();
        let v: Value = resp.json().await.unwrap_or(Value::Null);
        assert!(s.is_success(), "{method} {path} → {s}: {v}");
        v
    }
}

#[sqlx::test(migrator = "oryn_db::MIGRATOR")]
async fn worker_runs_jobs_and_stops_when_revoked(pool: PgPool) {
    let dir = tempfile::tempdir().unwrap();
    let state = AppState::new(Config::for_tests("unused", dir.path()), pool)
        .await
        .unwrap();
    let api = serve(app(state)).await;
    let target =
        serve(Router::new().route("/api/ping", get(|| async { Json(json!({"pong": true})) })))
            .await;

    let http = reqwest::Client::new();
    let resp = http
        .post(format!("{api}/api/v1/auth/register"))
        .json(&json!({"email": "o@acme.test", "name": "O", "password": "correct horse battery"}))
        .send()
        .await
        .unwrap();
    let cookie = resp.headers()["set-cookie"]
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_string();
    let me: Value = resp.json().await.unwrap();
    let ws = me["workspaces"][0]["id"].as_str().unwrap().to_string();
    let u = User {
        base: api.clone(),
        cookie,
        http,
    };
    use reqwest::Method as M;
    let pid = u
        .call(
            M::POST,
            &format!("/api/v1/workspaces/{ws}/projects"),
            Some(json!({"name": "P"})),
        )
        .await["id"]
        .as_str()
        .unwrap()
        .to_string();
    u.call(
        M::POST,
        &format!("/api/v1/projects/{pid}/environments"),
        Some(json!({"name": "local", "base_url": target, "is_default": true})),
    )
    .await;
    let saved = u
        .call(M::POST, &format!("/api/v1/projects/{pid}/suites"), Some(json!({"source": "Feature: Ping\n\n  Scenario: Pong\n    When I send a GET request to \"/api/ping\"\n    Then the response status should be 200\n    And the response JSON \"$.pong\" should equal \"true\"\n\n  Scenario: Pong again\n    When I send a GET request to \"/api/ping\"\n    Then the response status should be 200\n"})))
        .await;
    let suite = saved["suite_id"].as_str().unwrap().to_string();
    let ids: Vec<Value> = u
        .call(
            M::GET,
            &format!("/api/v1/projects/{pid}/scenarios?suite_id={suite}"),
            None,
        )
        .await
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["id"].clone())
        .collect();
    u.call(
        M::POST,
        &format!("/api/v1/projects/{pid}/scenarios/status"),
        Some(json!({"scenario_ids": ids, "status": "approved"})),
    )
    .await;
    let eid = u
        .call(
            M::POST,
            &format!("/api/v1/projects/{pid}/executions"),
            Some(json!({"suite_id": suite})),
        )
        .await["id"]
        .as_str()
        .unwrap()
        .to_string();

    // Self-register with the bootstrap token and persist the token for restarts.
    let token_file = dir.path().join("worker/token");
    let cfg = WorkerConfig {
        api_url: api.clone(),
        token: None,
        bootstrap_token: Some("bootstrap-test-token".into()),
        token_file: Some(token_file.clone()),
        name: "test-worker".into(),
        concurrency: 2,
        labels: vec![],
        runner_url: None,
        runner_token: None,
        poll_interval: Duration::from_millis(100),
        heartbeat_interval: Duration::from_millis(300),
    };
    let worker = Worker::connect(cfg.clone()).await.unwrap();
    assert!(std::fs::read_to_string(&token_file)
        .unwrap()
        .starts_with("oryn_wrk_"));
    let shutdown = CancellationToken::new();
    let handle = tokio::spawn(worker.run(shutdown.clone()));

    let mut exe = Value::Null;
    for _ in 0..100 {
        exe = u
            .call(M::GET, &format!("/api/v1/executions/{eid}"), None)
            .await;
        if exe["execution"]["status"] == "passed" || exe["execution"]["status"] == "failed" {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert_eq!(exe["execution"]["status"], "passed", "{exe}");
    assert_eq!(exe["execution"]["passed"], 2);

    // The worker is visible with its capabilities; API-only because no runner is configured.
    let workers = u
        .call(M::GET, &format!("/api/v1/workspaces/{ws}/workers"), None)
        .await;
    let w = workers
        .as_array()
        .unwrap()
        .iter()
        .find(|w| w["name"] == "test-worker")
        .unwrap()
        .clone();
    assert_eq!(w["status"], "online", "{w}");
    assert_eq!(w["concurrency"], 2, "{w}");
    let detail = u
        .call(
            M::GET,
            &format!(
                "/api/v1/workspaces/{ws}/workers/{}",
                w["id"].as_str().unwrap()
            ),
            None,
        )
        .await;
    assert!(
        detail.to_string().contains("\"capability\":\"api\""),
        "{detail}"
    );

    // Graceful shutdown.
    shutdown.cancel();
    tokio::time::timeout(Duration::from_secs(5), handle)
        .await
        .unwrap()
        .unwrap()
        .unwrap();

    // A restarted worker reuses the persisted identity; revoking it stops the worker.
    let worker = Worker::connect(cfg).await.unwrap();
    let handle = tokio::spawn(worker.run(CancellationToken::new()));
    let wid = w["id"].as_str().unwrap();
    u.call(
        M::DELETE,
        &format!("/api/v1/workspaces/{ws}/workers/{wid}"),
        None,
    )
    .await;
    let res = tokio::time::timeout(Duration::from_secs(5), handle)
        .await
        .unwrap()
        .unwrap();
    assert!(res.unwrap_err().to_string().contains("rejected"));
}

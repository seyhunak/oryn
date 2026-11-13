//! The ORYN worker. A worker is a stateless process that:
//!
//! 1. authenticates with a worker token (or registers itself once with the instance
//!    bootstrap token),
//! 2. advertises what it can run (`api` always; `browser:<name>` for every browser its
//!    Playwright runner reports available),
//! 3. leases jobs (`oryn.job/v1`), runs them with the Rust [`ScenarioRunner`], uploads
//!    evidence and reports the deterministic result,
//! 4. renews each job lease while it runs and stops the job when the control plane asks.
//!
//! The worker never decides whether a scenario should be retried; the control plane does.
//! It has no database access and no inbound ports.

use oryn_domain::job::JobSpec;
use oryn_execution::{BrowserAdapter, HttpBrowserAdapter, JobOutput, ScenarioRunner};
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Semaphore;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Debug, Clone)]
pub struct WorkerConfig {
    pub api_url: String,
    /// An `oryn_wrk_` token. When absent the worker registers with `bootstrap_token`.
    pub token: Option<String>,
    pub bootstrap_token: Option<String>,
    /// Where a self-registered token is persisted so restarts reuse the same identity.
    pub token_file: Option<std::path::PathBuf>,
    pub name: String,
    pub concurrency: usize,
    pub labels: Vec<String>,
    pub runner_url: Option<String>,
    pub runner_token: Option<String>,
    pub poll_interval: Duration,
    pub heartbeat_interval: Duration,
}

impl WorkerConfig {
    pub fn from_env() -> anyhow::Result<Self> {
        let var = |k: &str| std::env::var(k).ok().filter(|v| !v.trim().is_empty());
        let host = hostname();
        Ok(Self {
            api_url: var("ORYN_API_URL")
                .unwrap_or_else(|| "http://localhost:8080".into())
                .trim_end_matches('/')
                .to_string(),
            token: var("ORYN_WORKER_TOKEN"),
            bootstrap_token: var("ORYN_WORKER_BOOTSTRAP_TOKEN"),
            token_file: var("ORYN_WORKER_TOKEN_FILE").map(Into::into),
            name: var("ORYN_WORKER_NAME").unwrap_or_else(|| format!("worker-{host}")),
            concurrency: var("ORYN_WORKER_CONCURRENCY")
                .and_then(|v| v.parse().ok())
                .unwrap_or(2)
                .clamp(1, 64),
            labels: var("ORYN_WORKER_LABELS")
                .map(|v| {
                    v.split(',')
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty())
                        .collect()
                })
                .unwrap_or_default(),
            runner_url: var("ORYN_RUNNER_URL"),
            runner_token: var("ORYN_RUNNER_TOKEN"),
            poll_interval: Duration::from_millis(
                var("ORYN_WORKER_POLL_MS")
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(2000),
            ),
            heartbeat_interval: Duration::from_secs(15),
        })
    }
}

fn hostname() -> String {
    std::env::var("HOSTNAME")
        .ok()
        .or_else(|| std::fs::read_to_string("/etc/hostname").ok())
        .or_else(|| {
            std::process::Command::new("hostname")
                .output()
                .ok()
                .and_then(|o| String::from_utf8(o.stdout).ok())
        })
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "unknown".into())
}

#[derive(Debug, thiserror::Error)]
pub enum WorkerError {
    #[error("the control plane rejected the worker token (revoked or wrong); stopping")]
    Unauthorized,
    #[error("control plane request failed: {0}")]
    Http(String),
}

#[derive(Debug, Deserialize)]
struct Claimed {
    job_id: Uuid,
    execution_id: Uuid,
    attempt: i32,
    lease_secs: i64,
    spec: Value,
    #[serde(default)]
    secrets: serde_json::Map<String, Value>,
}

/// Thin client for the worker protocol (`/api/v1/worker/*`).
#[derive(Clone)]
pub struct ControlPlane {
    base: String,
    token: String,
    http: reqwest::Client,
}

impl ControlPlane {
    pub fn new(base: &str, token: String) -> Self {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(60))
            .user_agent(concat!("oryn-worker/", env!("CARGO_PKG_VERSION")))
            .build()
            .expect("http client");
        Self {
            base: base.trim_end_matches('/').to_string(),
            token,
            http,
        }
    }

    async fn post(&self, path: &str, body: Option<Value>) -> Result<(u16, Value), WorkerError> {
        let mut r = self
            .http
            .post(format!("{}/api/v1/worker{path}", self.base))
            .bearer_auth(&self.token);
        if let Some(b) = body {
            r = r.json(&b);
        }
        let resp = r
            .send()
            .await
            .map_err(|e| WorkerError::Http(e.to_string()))?;
        let status = resp.status().as_u16();
        if status == 401 {
            return Err(WorkerError::Unauthorized);
        }
        let v = resp.json::<Value>().await.unwrap_or(Value::Null);
        if status >= 400 {
            return Err(WorkerError::Http(format!("{path} → {status}: {v}")));
        }
        Ok((status, v))
    }

    pub async fn heartbeat(
        &self,
        active: i32,
        draining: bool,
        caps: &[Value],
    ) -> Result<(), WorkerError> {
        let mut body = json!({"active_jobs": active, "draining": draining, "version": VERSION, "hostname": hostname(), "os": std::env::consts::OS});
        if !caps.is_empty() {
            body["capabilities"] = json!(caps);
        }
        self.post("/heartbeat", Some(body)).await.map(|_| ())
    }

    async fn claim(&self, kinds: &[&str]) -> Result<Option<Claimed>, WorkerError> {
        let (status, v) = self.post("/claim", Some(json!({"kinds": kinds}))).await?;
        if status == 204 {
            return Ok(None);
        }
        serde_json::from_value(v)
            .map(Some)
            .map_err(|e| WorkerError::Http(format!("malformed claim: {e}")))
    }

    async fn upload(
        &self,
        job: Uuid,
        a: &oryn_execution::ArtifactPayload,
    ) -> Result<(), WorkerError> {
        let resp = self
            .http
            .post(format!("{}/api/v1/worker/jobs/{job}/artifacts", self.base))
            .bearer_auth(&self.token)
            .query(&[
                ("name", a.name.clone()),
                ("kind", a.kind.to_string()),
                ("content_type", a.content_type.clone()),
            ])
            .body(a.bytes.clone())
            .send()
            .await
            .map_err(|e| WorkerError::Http(e.to_string()))?;
        match resp.status().as_u16() {
            401 => Err(WorkerError::Unauthorized),
            s if s >= 400 => Err(WorkerError::Http(format!("artifact upload → {s}"))),
            _ => Ok(()),
        }
    }
}

/// Register with the bootstrap token; returns the new worker token.
pub async fn register(cfg: &WorkerConfig, caps: &[Value]) -> anyhow::Result<String> {
    let bootstrap = cfg.bootstrap_token.clone().ok_or_else(|| {
        anyhow::anyhow!("set ORYN_WORKER_TOKEN, or ORYN_WORKER_BOOTSTRAP_TOKEN to self-register")
    })?;
    let resp = reqwest::Client::new()
        .post(format!("{}/api/v1/worker/register", cfg.api_url))
        .json(&json!({"bootstrap_token": bootstrap, "name": cfg.name, "version": VERSION, "hostname": hostname(), "os": std::env::consts::OS,
                      "concurrency": cfg.concurrency, "labels": cfg.labels, "capabilities": caps}))
        .send()
        .await?;
    if !resp.status().is_success() {
        anyhow::bail!(
            "registration failed: {} {}",
            resp.status(),
            resp.text().await.unwrap_or_default()
        );
    }
    let v: Value = resp.json().await?;
    v["token"]
        .as_str()
        .map(String::from)
        .ok_or_else(|| anyhow::anyhow!("registration response has no token"))
}

/// What this worker can run, from the browser runner's health report.
pub async fn probe(browser: Option<&dyn BrowserAdapter>) -> (Vec<Value>, bool) {
    let mut caps = vec![json!({"capability": "api", "version": VERSION, "available": true})];
    let mut any_browser = false;
    if let Some(b) = browser {
        match b.health().await {
            Ok(h) => {
                for br in h.browsers {
                    any_browser |= br.available;
                    caps.push(json!({"capability": format!("browser:{}", br.name), "version": br.version, "available": br.available}));
                }
            }
            Err(e) => {
                tracing::warn!(error = %e, "browser runner unavailable; only API jobs will be claimed")
            }
        }
    }
    (caps, any_browser)
}

pub struct Worker {
    cfg: WorkerConfig,
    cp: ControlPlane,
    runner: Arc<ScenarioRunner>,
    browser: Option<Arc<dyn BrowserAdapter>>,
    active: Arc<AtomicI32>,
    browsers_ok: Arc<AtomicBool>,
}

impl Worker {
    /// Resolve credentials (token, token file, or registration) and probe capabilities.
    pub async fn connect(cfg: WorkerConfig) -> anyhow::Result<Self> {
        let browser: Option<Arc<dyn BrowserAdapter>> = cfg.runner_url.as_ref().map(|u| {
            Arc::new(HttpBrowserAdapter::new(
                u.clone(),
                cfg.runner_token.clone(),
                Duration::from_secs(330),
            )) as Arc<dyn BrowserAdapter>
        });
        let (caps, any_browser) = probe(browser.as_deref()).await;
        let token = match (&cfg.token, &cfg.token_file) {
            (Some(t), _) => t.clone(),
            (None, Some(f)) if f.exists() => std::fs::read_to_string(f)?.trim().to_string(),
            _ => {
                let t = register(&cfg, &caps).await?;
                if let Some(f) = &cfg.token_file {
                    write_secret(f, &t)?;
                }
                tracing::info!(name = %cfg.name, "registered with the control plane");
                t
            }
        };
        let cp = ControlPlane::new(&cfg.api_url, token);
        cp.heartbeat(0, false, &caps).await?;
        Ok(Self {
            runner: Arc::new(ScenarioRunner::new(browser.clone())),
            browser,
            cp,
            cfg,
            active: Arc::new(AtomicI32::new(0)),
            browsers_ok: Arc::new(AtomicBool::new(any_browser)),
        })
    }

    pub fn control_plane(&self) -> &ControlPlane {
        &self.cp
    }

    /// Run until `shutdown` fires; then stop claiming, let running jobs finish and report draining.
    pub async fn run(self, shutdown: CancellationToken) -> anyhow::Result<()> {
        let fatal = CancellationToken::new();
        let slots = Arc::new(Semaphore::new(self.cfg.concurrency));
        let jobs_cancel = CancellationToken::new();

        // Worker heartbeat + capability refresh.
        let hb = {
            let (cp, active, browsers_ok, browser, interval, stop, fatal) = (
                self.cp.clone(),
                self.active.clone(),
                self.browsers_ok.clone(),
                self.browser.clone(),
                self.cfg.heartbeat_interval,
                shutdown.clone(),
                fatal.clone(),
            );
            tokio::spawn(async move {
                loop {
                    tokio::select! {
                        _ = stop.cancelled() => break,
                        _ = fatal.cancelled() => break,
                        _ = tokio::time::sleep(interval) => {}
                    }
                    let (caps, ok) = probe(browser.as_deref()).await;
                    browsers_ok.store(ok, Ordering::Relaxed);
                    match cp
                        .heartbeat(active.load(Ordering::Relaxed), false, &caps)
                        .await
                    {
                        Err(WorkerError::Unauthorized) => {
                            tracing::error!("worker token rejected; shutting down");
                            fatal.cancel();
                        }
                        Err(e) => tracing::warn!(error = %e, "heartbeat failed"),
                        Ok(()) => {}
                    }
                }
            })
        };

        tracing::info!(name = %self.cfg.name, concurrency = self.cfg.concurrency, browsers = self.browsers_ok.load(Ordering::Relaxed), "worker started");
        let mut tasks = tokio::task::JoinSet::new();
        loop {
            if shutdown.is_cancelled() || fatal.is_cancelled() {
                break;
            }
            let permit = tokio::select! {
                p = slots.clone().acquire_owned() => p?,
                _ = shutdown.cancelled() => break,
                _ = fatal.cancelled() => break,
            };
            let kinds: Vec<&str> = if self.browsers_ok.load(Ordering::Relaxed) {
                vec!["api", "browser", "mixed"]
            } else {
                vec!["api"]
            };
            match self.cp.claim(&kinds).await {
                Ok(Some(job)) => {
                    let (cp, runner, active, cancel_all) = (
                        self.cp.clone(),
                        self.runner.clone(),
                        self.active.clone(),
                        jobs_cancel.clone(),
                    );
                    active.fetch_add(1, Ordering::Relaxed);
                    tasks.spawn(async move {
                        let id = job.job_id;
                        if let Err(e) = execute(&cp, &runner, job, cancel_all).await {
                            tracing::error!(job_id = %id, error = %e, "job failed to report; the lease will expire and the control plane will requeue it");
                        }
                        active.fetch_sub(1, Ordering::Relaxed);
                        drop(permit);
                    });
                    continue;
                }
                Ok(None) => drop(permit),
                Err(WorkerError::Unauthorized) => {
                    tracing::error!("worker token rejected; shutting down");
                    fatal.cancel();
                    break;
                }
                Err(e) => {
                    drop(permit);
                    tracing::warn!(error = %e, "claim failed");
                }
            }
            while tasks.try_join_next().is_some() {}
            tokio::select! {
                _ = tokio::time::sleep(self.cfg.poll_interval) => {}
                _ = shutdown.cancelled() => break,
                _ = fatal.cancelled() => break,
            }
        }

        let running = self.active.load(Ordering::Relaxed);
        if running > 0 {
            tracing::info!(running, "draining: waiting for running jobs");
            let _ = self
                .cp
                .heartbeat(running, true, &probe(self.browser.as_deref()).await.0)
                .await;
        }
        if fatal.is_cancelled() {
            jobs_cancel.cancel();
        }
        while tasks.join_next().await.is_some() {}
        hb.abort();
        let _ = self.cp.heartbeat(0, true, &[]).await;
        if fatal.is_cancelled() {
            anyhow::bail!(WorkerError::Unauthorized);
        }
        tracing::info!("worker stopped");
        Ok(())
    }
}

/// Run one leased job end to end.
async fn execute(
    cp: &ControlPlane,
    runner: &ScenarioRunner,
    job: Claimed,
    cancel_all: CancellationToken,
) -> Result<(), WorkerError> {
    let id = job.job_id;
    let mut spec: JobSpec = match serde_json::from_value(job.spec) {
        Ok(s) => s,
        Err(e) => {
            // A spec this worker cannot read is an infrastructure error, reported as such.
            cp.post(
                &format!("/jobs/{id}/complete"),
                Some(protocol_error(id, &format!("unreadable job spec: {e}"))),
            )
            .await?;
            return Ok(());
        }
    };
    // Secrets go into variables for this run only; the runner redacts their values from all evidence.
    for (k, v) in job.secrets {
        if let Some(s) = v.as_str() {
            spec.environment.variables.insert(k, s.to_string());
        }
    }
    cp.post(&format!("/jobs/{id}/start"), None).await?;

    let cancel = cancel_all.child_token();
    let lease = Duration::from_secs((job.lease_secs.max(6) / 3) as u64);
    let renew = {
        let (cp, cancel) = (cp.clone(), cancel.clone());
        tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = cancel.cancelled() => break,
                    _ = tokio::time::sleep(lease) => {}
                }
                match cp.post(&format!("/jobs/{id}/heartbeat"), None).await {
                    Ok((_, v)) if v["cancel_requested"] == true => {
                        tracing::info!(job_id = %id, "cancellation requested");
                        cancel.cancel();
                        break;
                    }
                    Ok(_) => {}
                    Err(WorkerError::Http(e)) if e.contains("→ 409") || e.contains("→ 404") => {
                        tracing::warn!(job_id = %id, "lease lost; stopping job");
                        cancel.cancel();
                        break;
                    }
                    Err(e) => tracing::warn!(job_id = %id, error = %e, "lease renewal failed"),
                }
            }
        })
    };

    tracing::info!(job_id = %id, scenario_id = ?spec.scenario_id, execution_id = %job.execution_id, attempt = job.attempt, "running");
    let JobOutput { result, artifacts } = runner.run(&spec, cancel.clone()).await;
    renew.abort();
    for a in &artifacts {
        if let Err(e) = cp.upload(id, a).await {
            tracing::warn!(job_id = %id, artifact = %a.name, error = %e, "artifact upload failed");
        }
    }
    let body = serde_json::to_value(&result).map_err(|e| WorkerError::Http(e.to_string()))?;
    let mut last = None;
    for attempt in 0..5u32 {
        match cp
            .post(&format!("/jobs/{id}/complete"), Some(body.clone()))
            .await
        {
            Ok(_) => {
                tracing::info!(job_id = %id, status = ?result.status, duration_ms = result.duration_ms, "reported");
                return Ok(());
            }
            Err(WorkerError::Unauthorized) => return Err(WorkerError::Unauthorized),
            Err(e) => {
                last = Some(e);
                tokio::time::sleep(Duration::from_millis(500 * 2u64.pow(attempt))).await;
            }
        }
    }
    Err(last.unwrap())
}

fn protocol_error(id: Uuid, msg: &str) -> Value {
    let now = chrono::Utc::now();
    serde_json::to_value(oryn_domain::job::JobResult {
        protocol: oryn_domain::job::JOB_PROTOCOL.into(),
        job_id: id,
        status: oryn_domain::JobStatus::Error,
        started_at: now,
        finished_at: now,
        duration_ms: 0,
        steps: vec![],
        logs: vec![],
        network: vec![],
        artifacts: vec![],
        browser_version: None,
        error: Some(msg.to_string()),
    })
    .expect("serialisable")
}

fn write_secret(path: &std::path::Path, value: &str) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(path, value)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}

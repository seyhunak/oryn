//! Shared application state and small cross-cutting services (rate limiting, AI access).

use crate::config::Config;
use crate::error::{ApiError, ApiResult};
use oryn_artifacts::{ArtifactStore, LocalArtifactStore};
use oryn_auth::SecretBox;
use oryn_db::PgPool;
use oryn_model_gateway::{Gateway, GatewayResult, Pricing};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use uuid::Uuid;

#[derive(Clone)]
pub struct AppState(pub Arc<Inner>);

pub struct Inner {
    pub pool: PgPool,
    pub cfg: Config,
    pub secrets: SecretBox,
    pub artifacts: Arc<dyn ArtifactStore>,
    pub limiter: RateLimiter,
    /// For operator-configured endpoints (OIDC issuer, recorder): may be internal.
    pub http: reqwest::Client,
    /// For destinations users choose (webhooks, repository hosts): every connection is
    /// resolved and checked once by `oryn_auth::net`, so DNS rebinding cannot redirect it.
    pub outbound: reqwest::Client,
    pub started: Instant,
}

impl std::ops::Deref for AppState {
    type Target = Inner;
    fn deref(&self) -> &Inner {
        &self.0
    }
}

impl AppState {
    pub async fn new(cfg: Config, pool: PgPool) -> anyhow::Result<Self> {
        let secrets = SecretBox::new(&cfg.secret_key).map_err(|e| anyhow::anyhow!("{e}"))?;
        std::fs::create_dir_all(&cfg.artifact_dir)?;
        std::fs::create_dir_all(&cfg.workspace_dir)?;
        let artifacts: Arc<dyn ArtifactStore> = Arc::new(LocalArtifactStore::new(
            cfg.artifact_dir.clone(),
            cfg.max_artifact_bytes,
        ));
        let redis = match &cfg.redis_url {
            Some(url) => {
                match redis::Client::open(url.as_str()) {
                    Ok(c) => match tokio::time::timeout(
                        Duration::from_secs(3),
                        redis::aio::ConnectionManager::new(c),
                    )
                    .await
                    {
                        Ok(Ok(m)) => Some(m),
                        _ => {
                            tracing::warn!("Redis unavailable; rate limiting falls back to in-process counters");
                            None
                        }
                    },
                    Err(e) => {
                        tracing::warn!(error = %e, "invalid REDIS_URL; using in-process rate limiting");
                        None
                    }
                }
            }
            None => None,
        };
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(15))
            .redirect(reqwest::redirect::Policy::none())
            .user_agent(concat!("oryn/", env!("CARGO_PKG_VERSION")))
            .build()?;
        let outbound = oryn_auth::net::guarded_client(cfg.allow_private_network)
            .timeout(Duration::from_secs(15))
            .user_agent(concat!("oryn/", env!("CARGO_PKG_VERSION")))
            .build()?;
        // AI providers use the same guard (their base_url is user-supplied).
        oryn_model_gateway::set_outbound_guard(cfg.allow_private_network);
        Ok(Self(Arc::new(Inner {
            pool,
            cfg,
            secrets,
            artifacts,
            limiter: RateLimiter::new(redis),
            http,
            outbound,
            started: Instant::now(),
        })))
    }

    pub fn seal(&self, plaintext: &str) -> String {
        self.secrets.encrypt(plaintext)
    }

    pub fn open(&self, sealed: &str) -> ApiResult<String> {
        self.secrets.decrypt(sealed).map_err(|_| {
            ApiError::Internal(anyhow::anyhow!(
                "stored secret could not be decrypted; was ORYN_SECRET_KEY changed?"
            ))
        })
    }
}

/// Fixed-window rate limiter. Uses Redis when available (shared across replicas),
/// in-process counters otherwise.
pub struct RateLimiter {
    redis: Option<redis::aio::ConnectionManager>,
    local: Mutex<HashMap<String, (Instant, u32)>>,
}

impl RateLimiter {
    fn new(redis: Option<redis::aio::ConnectionManager>) -> Self {
        Self {
            redis,
            local: Mutex::new(HashMap::new()),
        }
    }

    pub fn backend(&self) -> &'static str {
        if self.redis.is_some() {
            "redis"
        } else {
            "memory"
        }
    }

    pub async fn redis_ok(&self) -> Option<bool> {
        let mut c = self.redis.clone()?;
        Some(
            redis::cmd("PING")
                .query_async::<String>(&mut c)
                .await
                .is_ok(),
        )
    }

    /// Hits recorded for `key` in its current window, without recording one.
    pub async fn peek(&self, key: &str, window: Duration) -> u32 {
        if let Some(mut c) = self.redis.clone() {
            let k = format!("oryn:rl:{key}");
            if let Ok(n) = redis::cmd("GET")
                .arg(&k)
                .query_async::<Option<u32>>(&mut c)
                .await
            {
                return n.unwrap_or(0);
            }
        }
        let m = self.local.lock().unwrap();
        match m.get(key) {
            Some((t, n)) if Instant::now().duration_since(*t) < window => *n,
            _ => 0,
        }
    }

    /// Returns Err(RateLimited) when `key` exceeded `limit` hits within `window`.
    pub async fn check(&self, key: &str, limit: u32, window: Duration) -> ApiResult<()> {
        if let Some(mut c) = self.redis.clone() {
            let k = format!("oryn:rl:{key}");
            let res: Result<(u32,), _> = redis::pipe()
                .atomic()
                .incr(&k, 1)
                .expire(&k, window.as_secs() as i64)
                .ignore()
                .query_async(&mut c)
                .await;
            if let Ok((n,)) = res {
                return if n > limit {
                    Err(ApiError::RateLimited)
                } else {
                    Ok(())
                };
            }
        }
        let mut m = self.local.lock().unwrap();
        let now = Instant::now();
        if m.len() > 50_000 {
            m.retain(|_, (t, _)| now.duration_since(*t) < window);
        }
        let e = m.entry(key.to_string()).or_insert((now, 0));
        if now.duration_since(e.0) >= window {
            *e = (now, 0);
        }
        e.1 += 1;
        if e.1 > limit {
            Err(ApiError::RateLimited)
        } else {
            Ok(())
        }
    }
}

/// A configured model ready to call, resolved from the workspace's default provider.
pub struct AiHandle {
    pub gateway: Gateway,
    pub provider_id: Uuid,
    pub provider_kind: String,
    pub model: String,
}

impl AppState {
    /// Resolve the workspace's default enabled provider and its default model.
    /// `Ok(None)` means AI is not configured; callers fall back to deterministic behaviour.
    pub async fn ai(&self, ws: Uuid) -> ApiResult<Option<AiHandle>> {
        let Some(p) = oryn_db::ai::default_provider(&self.pool, ws).await? else {
            return Ok(None);
        };
        if !p.enabled {
            return Ok(None);
        }
        Ok(Some(self.ai_provider(&p).await?))
    }

    /// Build a handle for one stored provider (its default model, else the first, else a built-in default).
    pub async fn ai_provider(&self, p: &oryn_db::ai::AiProvider) -> ApiResult<AiHandle> {
        let models = oryn_db::ai::list_models(&self.pool, p.id).await?;
        let model = models.iter().find(|m| m.is_default).or(models.first());
        let kind: oryn_domain::AiProviderKind = p
            .kind
            .parse()
            .map_err(|_| ApiError::Internal(anyhow::anyhow!("unknown provider kind {}", p.kind)))?;
        let model_name = model
            .map(|m| m.name.clone())
            .unwrap_or_else(|| default_model(kind).to_string());
        let pricing = model
            .map(|m| Pricing {
                input_per_mtok: m.input_cost_per_mtok,
                output_per_mtok: m.output_cost_per_mtok,
            })
            .unwrap_or_default();
        let key = match &p.api_key_encrypted {
            Some(s) => Some(self.open(s)?),
            None => None,
        };
        let provider = oryn_model_gateway::providers::build(
            kind,
            key,
            p.base_url.clone(),
            p.api_version.clone(),
        )
        .map_err(|e| ApiError::unprocessable(e.to_string()))?;
        Ok(AiHandle {
            gateway: Gateway::new(provider, pricing),
            provider_id: p.id,
            provider_kind: p.kind.clone(),
            model: model_name,
        })
    }

    /// Run a model call and record it (tokens, cost, latency, failure) in `ai_requests`.
    pub async fn ai_call(
        &self,
        ws: Uuid,
        project: Option<Uuid>,
        user: Option<Uuid>,
        ai: &AiHandle,
        req: oryn_model_gateway::CompletionRequest,
    ) -> ApiResult<(GatewayResult, Uuid)> {
        let purpose = req.purpose.clone();
        match ai.gateway.run(req).await {
            Ok(r) => {
                let id = oryn_db::ai::record_request(
                    &self.pool,
                    ws,
                    project,
                    Some(ai.provider_id),
                    &ai.provider_kind,
                    &r.model,
                    &purpose,
                    true,
                    r.attempts as i32,
                    r.latency_ms as i64,
                    None,
                    r.input_tokens as i32,
                    r.output_tokens as i32,
                    r.cost_usd,
                    user,
                )
                .await?;
                Ok((r, id))
            }
            Err((e, attempts, latency)) => {
                let msg = e.to_string();
                let _ = oryn_db::ai::record_request(
                    &self.pool,
                    ws,
                    project,
                    Some(ai.provider_id),
                    &ai.provider_kind,
                    &ai.model,
                    &purpose,
                    false,
                    attempts as i32,
                    latency as i64,
                    Some(&msg),
                    0,
                    0,
                    0.0,
                    user,
                )
                .await;
                Err(ApiError::Unavailable(format!("model call failed: {msg}")))
            }
        }
    }
}

pub fn default_model(kind: oryn_domain::AiProviderKind) -> &'static str {
    use oryn_domain::AiProviderKind as K;
    match kind {
        K::Anthropic => "claude-opus-5-5",
        K::Openai | K::AzureOpenai | K::OpenaiCompatible => "gpt-4.1",
        K::Gemini => "gemini-2.5-pro",
        K::Ollama => "llama3.1",
        K::Mock => "mock-1",
    }
}

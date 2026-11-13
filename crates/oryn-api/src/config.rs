//! Runtime configuration, read once from the environment at startup.

use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct OidcConfig {
    pub issuer: String,
    pub client_id: String,
    pub client_secret: Option<String>,
    pub scopes: String,
    pub display_name: String,
}

#[derive(Debug, Clone)]
pub struct Config {
    pub bind: String,
    pub database_url: String,
    pub redis_url: Option<String>,
    pub secret_key: String,
    pub public_url: String,
    pub cookie_secure: bool,
    pub session_ttl_hours: i64,
    pub allow_registration: bool,
    pub worker_bootstrap_token: Option<String>,
    pub artifact_dir: PathBuf,
    pub workspace_dir: PathBuf,
    pub max_artifact_bytes: usize,
    pub max_upload_bytes: usize,
    /// Directory under which `local` repository imports are allowed. Unset disables local import.
    pub local_import_root: Option<PathBuf>,
    /// Allow repository hosts and webhooks on private networks (self-hosted GitLab, internal Slack proxies).
    pub allow_private_network: bool,
    pub lease_secs: i64,
    pub oidc: Option<OidcConfig>,
    pub cors_origins: Vec<String>,
    /// Peers whose `X-Forwarded-For` header is believed (reverse proxies, the bundled web server).
    pub trusted_proxies: Vec<ipnet::IpNet>,
}

fn var(k: &str) -> Option<String> {
    std::env::var(k).ok().filter(|v| !v.trim().is_empty())
}

fn flag(k: &str, default: bool) -> bool {
    var(k)
        .map(|v| matches!(v.to_ascii_lowercase().as_str(), "1" | "true" | "yes" | "on"))
        .unwrap_or(default)
}

impl Config {
    pub fn from_env() -> anyhow::Result<Self> {
        let secret_key = var("ORYN_SECRET_KEY").ok_or_else(|| {
            anyhow::anyhow!("ORYN_SECRET_KEY is required (32+ characters); see .env.example")
        })?;
        if secret_key.len() < 32 {
            anyhow::bail!("ORYN_SECRET_KEY must be at least 32 characters");
        }
        let oidc = match (var("ORYN_OIDC_ISSUER"), var("ORYN_OIDC_CLIENT_ID")) {
            (Some(issuer), Some(client_id)) => Some(OidcConfig {
                issuer: issuer.trim_end_matches('/').to_string(),
                client_id,
                client_secret: var("ORYN_OIDC_CLIENT_SECRET"),
                scopes: var("ORYN_OIDC_SCOPES").unwrap_or_else(|| "openid email profile".into()),
                display_name: var("ORYN_OIDC_NAME").unwrap_or_else(|| "Single sign-on".into()),
            }),
            _ => None,
        };
        Ok(Self {
            bind: var("ORYN_BIND").unwrap_or_else(|| "0.0.0.0:8080".into()),
            database_url: var("DATABASE_URL")
                .ok_or_else(|| anyhow::anyhow!("DATABASE_URL is required"))?,
            redis_url: var("REDIS_URL"),
            secret_key,
            public_url: var("ORYN_PUBLIC_URL")
                .unwrap_or_else(|| "http://localhost:3000".into())
                .trim_end_matches('/')
                .to_string(),
            cookie_secure: flag("ORYN_COOKIE_SECURE", false),
            session_ttl_hours: var("ORYN_SESSION_TTL_HOURS")
                .and_then(|v| v.parse().ok())
                .unwrap_or(24 * 7),
            allow_registration: flag("ORYN_ALLOW_REGISTRATION", true),
            worker_bootstrap_token: var("ORYN_WORKER_BOOTSTRAP_TOKEN"),
            artifact_dir: var("ORYN_ARTIFACT_DIR")
                .unwrap_or_else(|| "./data/artifacts".into())
                .into(),
            workspace_dir: var("ORYN_WORKSPACE_DIR")
                .unwrap_or_else(|| "./data/repos".into())
                .into(),
            max_artifact_bytes: var("ORYN_MAX_ARTIFACT_BYTES")
                .and_then(|v| v.parse().ok())
                .unwrap_or(100 * 1024 * 1024),
            max_upload_bytes: var("ORYN_MAX_UPLOAD_BYTES")
                .and_then(|v| v.parse().ok())
                .unwrap_or(200 * 1024 * 1024),
            local_import_root: var("ORYN_LOCAL_IMPORT_ROOT").map(PathBuf::from),
            allow_private_network: flag("ORYN_ALLOW_PRIVATE_NETWORK", false),
            lease_secs: var("ORYN_JOB_LEASE_SECS")
                .and_then(|v| v.parse().ok())
                .unwrap_or(60),
            oidc,
            cors_origins: var("ORYN_CORS_ORIGINS")
                .map(|v| v.split(',').map(|s| s.trim().to_string()).collect())
                .unwrap_or_default(),
            trusted_proxies: parse_trusted_proxies(var("ORYN_TRUSTED_PROXIES").as_deref())?,
        })
    }

    /// Configuration for tests.
    pub fn for_tests(database_url: &str, dir: &std::path::Path) -> Self {
        Self {
            bind: "127.0.0.1:0".into(),
            database_url: database_url.into(),
            redis_url: None,
            secret_key: "test-secret-key-test-secret-key-0123456789".into(),
            public_url: "http://localhost:3000".into(),
            cookie_secure: false,
            session_ttl_hours: 24,
            allow_registration: true,
            worker_bootstrap_token: Some("bootstrap-test-token".into()),
            artifact_dir: dir.join("artifacts"),
            workspace_dir: dir.join("repos"),
            max_artifact_bytes: 10 * 1024 * 1024,
            max_upload_bytes: 20 * 1024 * 1024,
            local_import_root: Some(dir.to_path_buf()),
            allow_private_network: true,
            lease_secs: 30,
            oidc: None,
            cors_origins: vec![],
            trusted_proxies: parse_trusted_proxies(None).expect("defaults parse"),
        }
    }
}

/// Default: loopback and private ranges, where a bundled or sidecar proxy lives.
/// `none` trusts no proxy; otherwise a comma-separated list of IPs or CIDRs.
pub fn parse_trusted_proxies(v: Option<&str>) -> anyhow::Result<Vec<ipnet::IpNet>> {
    const DEFAULT: &str = "127.0.0.0/8,::1/128,10.0.0.0/8,172.16.0.0/12,192.168.0.0/16,fc00::/7";
    let raw = match v.map(str::trim) {
        Some("none") => return Ok(vec![]),
        Some(list) if !list.is_empty() => list,
        _ => DEFAULT,
    };
    raw.split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| {
            s.parse::<ipnet::IpNet>()
                .or_else(|_| s.parse::<std::net::IpAddr>().map(ipnet::IpNet::from))
                .map_err(|_| anyhow::anyhow!("ORYN_TRUSTED_PROXIES: {s} is not an IP or CIDR"))
        })
        .collect()
}

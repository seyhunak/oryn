//! Network policy for test traffic (SSRF protection).
//!
//! Test jobs may only reach hosts declared on their environment. Cloud metadata
//! endpoints are always blocked, and when `block_private` is set, hosts that
//! resolve to private, loopback or link-local addresses are refused too.

use std::net::IpAddr;

#[derive(Debug, Clone, Default)]
pub struct NetworkPolicy {
    /// Allowed hostnames (exact, or `*.example.com`). Empty = any host.
    pub allowed_hosts: Vec<String>,
    /// Refuse private / loopback / link-local destinations.
    pub block_private: bool,
}

const METADATA_HOSTS: &[&str] = &[
    "169.254.169.254",
    "metadata.google.internal",
    "metadata",
    "100.100.100.200",
    "fd00:ec2::254",
];

impl NetworkPolicy {
    pub fn permissive_local() -> Self {
        Self {
            allowed_hosts: vec![],
            block_private: false,
        }
    }

    pub fn host_allowed(&self, host: &str) -> bool {
        let host = host
            .trim_start_matches('[')
            .trim_end_matches(']')
            .to_ascii_lowercase();
        if METADATA_HOSTS.contains(&host.as_str()) {
            return false;
        }
        if self.allowed_hosts.is_empty() {
            return true;
        }
        self.allowed_hosts.iter().any(|a| {
            let a = a.to_ascii_lowercase();
            match a.strip_prefix("*.") {
                Some(suffix) => host.ends_with(&format!(".{suffix}")) || host == suffix,
                None => a == host,
            }
        })
    }

    pub async fn check(&self, url: &url::Url) -> Result<(), String> {
        if !matches!(url.scheme(), "http" | "https") {
            return Err(format!(
                "Blocked by network policy: scheme {} is not allowed",
                url.scheme()
            ));
        }
        let host = url.host_str().ok_or("URL has no host")?;
        if !self.host_allowed(host) {
            return Err(format!(
                "Blocked by network policy: host {host} is not in this environment's allowed hosts"
            ));
        }
        if self.block_private {
            let port = url.port_or_known_default().unwrap_or(80);
            let addrs = tokio::net::lookup_host((host, port))
                .await
                .map_err(|e| format!("DNS lookup failed for {host}: {e}"))?;
            for a in addrs {
                if is_private(a.ip()) {
                    return Err(format!(
                        "Blocked by network policy: {host} resolves to a private address"
                    ));
                }
            }
        }
        Ok(())
    }
}

pub fn is_private(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            v4.is_private()
                || v4.is_loopback()
                || v4.is_link_local()
                || v4.is_unspecified()
                || v4.octets()[0] == 100 && (64..128).contains(&v4.octets()[1])
        }
        IpAddr::V6(v6) => {
            v6.is_loopback()
                || v6.is_unspecified()
                || (v6.segments()[0] & 0xfe00) == 0xfc00
                || (v6.segments()[0] & 0xffc0) == 0xfe80
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allowlist() {
        let p = NetworkPolicy {
            allowed_hosts: vec!["localhost".into(), "*.example.com".into()],
            block_private: false,
        };
        assert!(p.host_allowed("localhost"));
        assert!(p.host_allowed("api.example.com"));
        assert!(!p.host_allowed("evil.com"));
        assert!(!NetworkPolicy::permissive_local().host_allowed("169.254.169.254"));
        assert!(is_private("10.0.0.1".parse().unwrap()));
        assert!(!is_private("8.8.8.8".parse().unwrap()));
    }

    #[tokio::test]
    async fn blocks_private_when_asked() {
        let p = NetworkPolicy {
            allowed_hosts: vec![],
            block_private: true,
        };
        assert!(p
            .check(&url::Url::parse("http://127.0.0.1/").unwrap())
            .await
            .is_err());
        assert!(p
            .check(&url::Url::parse("file:///etc/passwd").unwrap())
            .await
            .is_err());
    }
}

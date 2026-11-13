//! Outbound network guard for destinations users choose (webhooks, repository hosts,
//! AI provider endpoints).
//!
//! Checking a hostname and then letting the HTTP client resolve it again leaves a
//! window for DNS rebinding: the name can answer with a public address for the check
//! and a private one for the connection. [`GuardedResolver`] closes it by being the
//! client's resolver: it resolves once, refuses the whole name if any address is
//! disallowed, and hands the client only the addresses it checked. Clients that cannot
//! take a resolver (the `git` CLI) use [`resolve_checked`] and pin its answer.
//!
//! IP-literal hosts are never resolved by HTTP clients, so URL validation must call
//! [`check_ip`] on them; [`check_url_host`] does both.

use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;

/// Cloud metadata addresses. Refused even when private networks are allowed.
pub fn is_metadata(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v) => v.octets() == [169, 254, 169, 254] || v.octets() == [100, 100, 100, 200],
        IpAddr::V6(v) => {
            v.segments() == [0xfd00, 0x0ec2, 0, 0, 0, 0, 0, 0x0254]
                || v.to_ipv4_mapped()
                    .is_some_and(|m| is_metadata(IpAddr::V4(m)))
        }
    }
}

/// Loopback, private, link-local, carrier-grade NAT, unspecified, broadcast and
/// multicast addresses, including their IPv4-mapped IPv6 forms.
pub fn is_private(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v) => {
            v.is_private()
                || v.is_loopback()
                || v.is_link_local()
                || v.is_unspecified()
                || v.is_broadcast()
                || v.is_multicast()
                || (v.octets()[0] == 100 && (v.octets()[1] & 0xc0) == 64)
                || v.octets()[0] == 0
        }
        IpAddr::V6(v) => {
            v.is_loopback()
                || v.is_unspecified()
                || v.is_multicast()
                || (v.segments()[0] & 0xfe00) == 0xfc00
                || (v.segments()[0] & 0xffc0) == 0xfe80
                || v.to_ipv4_mapped()
                    .is_some_and(|m| is_private(IpAddr::V4(m)))
        }
    }
}

/// Whether one address may be contacted.
pub fn check_ip(ip: IpAddr, allow_private: bool) -> Result<(), String> {
    if is_metadata(ip) {
        return Err("cloud metadata endpoints are never allowed".into());
    }
    if !allow_private && is_private(ip) {
        return Err(format!(
            "{ip} is a private address; set ORYN_ALLOW_PRIVATE_NETWORK=true to allow it"
        ));
    }
    Ok(())
}

/// Resolve `host` once and accept it only if every address is allowed.
pub async fn resolve_checked(
    host: &str,
    port: u16,
    allow_private: bool,
) -> Result<Vec<SocketAddr>, String> {
    let host = host.trim_start_matches('[').trim_end_matches(']');
    if let Ok(ip) = host.parse::<IpAddr>() {
        check_ip(ip, allow_private)?;
        return Ok(vec![SocketAddr::new(ip, port)]);
    }
    let addrs: Vec<SocketAddr> = tokio::net::lookup_host((host, port))
        .await
        .map_err(|e| format!("cannot resolve {host}: {e}"))?
        .collect();
    if addrs.is_empty() {
        return Err(format!("cannot resolve {host}"));
    }
    for a in &addrs {
        check_ip(a.ip(), allow_private).map_err(|e| format!("{host} resolves to {e}"))?;
    }
    Ok(addrs)
}

/// Validate a URL's host without connecting: literals are checked directly, names are
/// resolved and checked. Use it for early, friendly errors; the connection itself must
/// still go through a guarded client.
pub async fn check_url_host(u: &url::Url, allow_private: bool) -> Result<Vec<SocketAddr>, String> {
    let host = u.host_str().ok_or("URL has no host")?;
    resolve_checked(
        host,
        u.port_or_known_default().unwrap_or(443),
        allow_private,
    )
    .await
}

/// A reqwest resolver that only ever yields checked addresses.
#[derive(Debug, Clone, Copy)]
pub struct GuardedResolver {
    pub allow_private: bool,
}

impl reqwest::dns::Resolve for GuardedResolver {
    fn resolve(&self, name: reqwest::dns::Name) -> reqwest::dns::Resolving {
        let allow_private = self.allow_private;
        Box::pin(async move {
            // reqwest substitutes the URL's port; 0 is a placeholder.
            let addrs = resolve_checked(name.as_str(), 0, allow_private)
                .await
                .map_err(|e| -> Box<dyn std::error::Error + Send + Sync> { e.into() })?;
            Ok(Box::new(addrs.into_iter()) as reqwest::dns::Addrs)
        })
    }
}

/// A client builder whose every connection is checked and pinned. Redirects are not
/// followed, since a redirect to an IP literal would bypass the resolver.
pub fn guarded_client(allow_private: bool) -> reqwest::ClientBuilder {
    reqwest::Client::builder()
        .dns_resolver(Arc::new(GuardedResolver { allow_private }))
        .redirect(reqwest::redirect::Policy::none())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_addresses() {
        for p in [
            "127.0.0.1",
            "10.1.2.3",
            "172.20.0.1",
            "192.168.1.1",
            "100.64.0.1",
            "0.0.0.0",
            "::1",
            "fc00::1",
            "fe80::1",
            "::ffff:10.0.0.1",
        ] {
            assert!(is_private(p.parse().unwrap()), "{p}");
        }
        for p in ["1.1.1.1", "140.82.112.3", "2606:4700::1111"] {
            assert!(!is_private(p.parse().unwrap()), "{p}");
        }
        for p in [
            "169.254.169.254",
            "100.100.100.200",
            "fd00:ec2::254",
            "::ffff:169.254.169.254",
        ] {
            assert!(check_ip(p.parse().unwrap(), true).is_err(), "{p}");
        }
        assert!(check_ip("10.0.0.5".parse().unwrap(), true).is_ok());
        assert!(check_ip("10.0.0.5".parse().unwrap(), false).is_err());
    }

    #[tokio::test]
    async fn names_are_checked_after_resolution() {
        // "localhost" is a name, so only resolution reveals it is loopback.
        assert!(resolve_checked("localhost", 80, false).await.is_err());
        assert!(resolve_checked("localhost", 80, true).await.is_ok());
        assert!(resolve_checked("[::1]", 80, false).await.is_err());
    }

    /// The rebinding case: the client's own resolution is the checked one, so a name
    /// that points at a private address can never be connected to.
    #[tokio::test]
    async fn guarded_client_refuses_private_destinations() {
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = l.local_addr().unwrap().port();
        tokio::spawn(async move {
            use tokio::io::{AsyncReadExt, AsyncWriteExt};
            while let Ok((mut sock, _)) = l.accept().await {
                let mut buf = [0u8; 1024];
                let _ = sock.read(&mut buf).await;
                let _ = sock
                    .write_all(
                        b"HTTP/1.1 200 OK\r\ncontent-length: 2\r\nconnection: close\r\n\r\nok",
                    )
                    .await;
            }
        });
        let c = guarded_client(false).build().unwrap();
        let err = c
            .get(format!("http://localhost:{port}/"))
            .send()
            .await
            .unwrap_err();
        let mut chain = err.to_string();
        let mut src = std::error::Error::source(&err);
        while let Some(s) = src {
            chain.push_str(&format!(": {s}"));
            src = s.source();
        }
        assert!(chain.contains("private address"), "{chain}");
        // With private networks allowed the same request connects.
        let ok = guarded_client(true).build().unwrap();
        let r = ok
            .get(format!("http://localhost:{port}/"))
            .send()
            .await
            .unwrap();
        assert_eq!(r.status(), 200);
    }
}

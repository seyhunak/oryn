//! Safe repository import: validated ZIP extraction and restricted `git clone`.

use std::io::Read;
use std::path::{Component, Path, PathBuf};
use std::time::Duration;

#[derive(Debug, thiserror::Error)]
pub enum ImportError {
    #[error("archive rejected: {0}")]
    Rejected(String),
    #[error("repository URL rejected: {0}")]
    BadUrl(String),
    #[error("clone failed: {0}")]
    Clone(String),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

#[derive(Debug, Clone, Copy)]
pub struct ZipLimits {
    pub max_entries: usize,
    pub max_total_bytes: u64,
    pub max_file_bytes: u64,
    /// Maximum uncompressed/compressed ratio per entry (zip-bomb guard).
    pub max_ratio: u64,
}

impl Default for ZipLimits {
    fn default() -> Self {
        Self {
            max_entries: 20_000,
            max_total_bytes: 500 * 1024 * 1024,
            max_file_bytes: 50 * 1024 * 1024,
            max_ratio: 200,
        }
    }
}

/// Validate a path inside an archive: relative, no `..`, no drive or root prefix.
fn safe_entry_path(name: &str) -> Option<PathBuf> {
    if name.contains('\0') || name.contains('\\') {
        return None;
    }
    let p = Path::new(name);
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            Component::Normal(s) => out.push(s),
            Component::CurDir => {}
            _ => return None,
        }
    }
    if out.as_os_str().is_empty() {
        None
    } else {
        Some(out)
    }
}

/// Extract a ZIP into `dest` (which must be empty or absent). Rejects path traversal,
/// absolute paths, symlinks, oversize entries, and suspicious compression ratios.
/// When every entry shares one top-level folder (GitHub-style archives) it is stripped.
/// Returns (files written, bytes written).
pub fn extract_zip(
    bytes: &[u8],
    dest: &Path,
    limits: ZipLimits,
) -> Result<(usize, u64), ImportError> {
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes))
        .map_err(|e| ImportError::Rejected(format!("not a valid ZIP: {e}")))?;
    if archive.len() > limits.max_entries {
        return Err(ImportError::Rejected(format!(
            "{} entries exceeds the limit of {}",
            archive.len(),
            limits.max_entries
        )));
    }

    // Pass 1: validate everything before writing anything.
    let mut declared: u64 = 0;
    let mut names = Vec::with_capacity(archive.len());
    for i in 0..archive.len() {
        let f = archive
            .by_index_raw(i)
            .map_err(|e| ImportError::Rejected(e.to_string()))?;
        let name = f.name().to_string();
        if f.is_symlink() {
            return Err(ImportError::Rejected(format!(
                "symbolic link not allowed: {name}"
            )));
        }
        let path = safe_entry_path(&name)
            .ok_or_else(|| ImportError::Rejected(format!("unsafe path: {name}")))?;
        if !f.is_dir() {
            if f.size() > limits.max_file_bytes {
                return Err(ImportError::Rejected(format!(
                    "{name} is larger than {} bytes",
                    limits.max_file_bytes
                )));
            }
            if f.compressed_size() > 0
                && f.size() / f.compressed_size().max(1) > limits.max_ratio
                && f.size() > 1_000_000
            {
                return Err(ImportError::Rejected(format!(
                    "{name} has a suspicious compression ratio"
                )));
            }
            declared += f.size();
        }
        names.push((path, f.is_dir()));
    }
    if declared > limits.max_total_bytes {
        return Err(ImportError::Rejected(format!(
            "uncompressed size {declared} exceeds {}",
            limits.max_total_bytes
        )));
    }
    let strip = common_root(&names);

    std::fs::create_dir_all(dest)?;
    let dest = dest.canonicalize()?;
    let mut written: u64 = 0;
    let mut files = 0;
    for (i, (path, is_dir)) in names.iter().enumerate() {
        let rel = match &strip {
            Some(root) => match path.strip_prefix(root) {
                Ok(r) if !r.as_os_str().is_empty() => r.to_path_buf(),
                _ => continue,
            },
            None => path.clone(),
        };
        let target = dest.join(&rel);
        if !target.starts_with(&dest) {
            return Err(ImportError::Rejected(format!(
                "unsafe path: {}",
                rel.display()
            )));
        }
        if *is_dir {
            std::fs::create_dir_all(&target)?;
            continue;
        }
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut f = archive
            .by_index(i)
            .map_err(|e| ImportError::Rejected(e.to_string()))?;
        // Enforce limits on actual decompressed bytes, not just the header's claim.
        let mut buf = Vec::new();
        (&mut f)
            .take(limits.max_file_bytes + 1)
            .read_to_end(&mut buf)?;
        if buf.len() as u64 > limits.max_file_bytes {
            return Err(ImportError::Rejected(format!(
                "{} expands beyond its limit",
                rel.display()
            )));
        }
        written += buf.len() as u64;
        if written > limits.max_total_bytes {
            return Err(ImportError::Rejected(
                "archive expands beyond the total size limit".into(),
            ));
        }
        std::fs::write(&target, buf)?;
        files += 1;
    }
    Ok((files, written))
}

fn common_root(names: &[(PathBuf, bool)]) -> Option<PathBuf> {
    let first = names
        .iter()
        .find(|(_, d)| !d)?
        .0
        .components()
        .next()?
        .as_os_str()
        .to_owned();
    let all = names.iter().all(|(p, is_dir)| {
        let mut c = p.components();
        c.next().map(|x| x.as_os_str() == first).unwrap_or(false) && (*is_dir || c.next().is_some())
    });
    all.then(|| PathBuf::from(first))
}

/// Allowed git URL: https only, no credentials, no local paths, host must be public
/// unless explicitly allowed (self-hosted GitLab on a private network).
pub fn validate_git_url(raw: &str, allow_private: bool) -> Result<url::Url, ImportError> {
    let u = url::Url::parse(raw).map_err(|e| ImportError::BadUrl(e.to_string()))?;
    if u.scheme() != "https" && !(allow_private && u.scheme() == "http") {
        return Err(ImportError::BadUrl(
            "only https:// repository URLs are accepted".into(),
        ));
    }
    if !u.username().is_empty() || u.password().is_some() {
        return Err(ImportError::BadUrl(
            "credentials in the URL are not accepted; use a repository connection".into(),
        ));
    }
    let host = u
        .host_str()
        .ok_or_else(|| ImportError::BadUrl("missing host".into()))?;
    if raw.contains("..") || raw.starts_with('-') {
        return Err(ImportError::BadUrl("unsupported characters in URL".into()));
    }
    if host == "169.254.169.254" || host == "metadata.google.internal" {
        return Err(ImportError::BadUrl(
            "metadata endpoints are never allowed".into(),
        ));
    }
    if !allow_private {
        let lower = host.to_ascii_lowercase();
        let private = lower == "localhost"
            || lower.ends_with(".local")
            || lower.ends_with(".internal")
            || lower
                .parse::<std::net::IpAddr>()
                .map(|ip| match ip {
                    std::net::IpAddr::V4(v) => {
                        v.is_private() || v.is_loopback() || v.is_link_local() || v.is_unspecified()
                    }
                    std::net::IpAddr::V6(v) => {
                        v.is_loopback()
                            || v.is_unspecified()
                            || (v.segments()[0] & 0xfe00) == 0xfc00
                    }
                })
                .unwrap_or(false);
        if private {
            return Err(ImportError::BadUrl(
                "private network hosts are blocked by policy".into(),
            ));
        }
    }
    Ok(u)
}

fn valid_branch(b: &str) -> bool {
    !b.is_empty()
        && b.len() < 200
        && !b.starts_with('-')
        && !b.contains("..")
        && b.chars()
            .all(|c| c.is_ascii_alphanumeric() || "-_./".contains(c))
}

/// Shallow-clone `url` into `dest` with hooks, submodules, and non-https transports disabled.
/// The token is passed as an HTTP header through environment config, never on argv.
/// Returns the HEAD commit SHA.
/// Pin git to addresses that were resolved and checked here, so the name cannot be
/// re-resolved (DNS rebinding) to a private or metadata address. Values are curl
/// `CURLOPT_RESOLVE` entries: `host:port:addr[,addr]`, IPv6 in brackets.
pub fn curl_resolve_entry(host: &str, port: u16, addrs: &[std::net::SocketAddr]) -> String {
    let list: Vec<String> = addrs
        .iter()
        .map(|a| match a.ip() {
            std::net::IpAddr::V6(v6) => format!("[{v6}]"),
            ip => ip.to_string(),
        })
        .collect();
    format!("{host}:{port}:{}", list.join(","))
}

pub async fn clone_repo(
    url: &url::Url,
    branch: Option<&str>,
    token: Option<&str>,
    dest: &Path,
    timeout: Duration,
    allow_private: bool,
) -> Result<String, ImportError> {
    if let Some(b) = branch {
        if !valid_branch(b) {
            return Err(ImportError::BadUrl(format!("invalid branch name: {b}")));
        }
    }
    if dest.exists() {
        std::fs::remove_dir_all(dest)?;
    }
    std::fs::create_dir_all(dest.parent().unwrap_or(dest))?;
    let host = url
        .host_str()
        .ok_or_else(|| ImportError::BadUrl("missing host".into()))?
        .to_string();
    let port = url.port_or_known_default().unwrap_or(443);
    let addrs = oryn_auth::net::resolve_checked(&host, port, allow_private)
        .await
        .map_err(ImportError::BadUrl)?;
    let mut cmd = tokio::process::Command::new("git");
    cmd.args([
        "-c",
        "core.hooksPath=/dev/null",
        "-c",
        "protocol.allow=never",
        "-c",
        "protocol.https.allow=always",
        "-c",
        "protocol.http.allow=always",
        "-c",
        "submodule.recurse=false",
        "-c",
        "core.symlinks=false",
        // A redirect would make git resolve a new name without our check.
        "-c",
        "http.followRedirects=false",
    ])
    .arg("-c")
    .arg(format!(
        "http.curloptResolve={}",
        curl_resolve_entry(host.trim_matches(|c| c == '[' || c == ']'), port, &addrs)
    ))
    .args(["clone", "--depth", "1", "--single-branch", "--no-tags"]);
    if let Some(b) = branch {
        cmd.args(["--branch", b]);
    }
    cmd.arg("--").arg(url.as_str()).arg(dest);
    cmd.env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_ASKPASS", "/bin/false")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("HOME", dest.parent().unwrap_or(dest));
    if let Some(t) = token {
        use base64::Engine as _;
        let basic = base64::engine::general_purpose::STANDARD.encode(format!("oauth2:{t}"));
        cmd.env("GIT_CONFIG_COUNT", "1")
            .env("GIT_CONFIG_KEY_0", "http.extraHeader")
            .env(
                "GIT_CONFIG_VALUE_0",
                format!("Authorization: Basic {basic}"),
            );
    }
    cmd.stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true);
    let child = cmd
        .spawn()
        .map_err(|e| ImportError::Clone(format!("git is not available: {e}")))?;
    let out = tokio::time::timeout(timeout, child.wait_with_output())
        .await
        .map_err(|_| ImportError::Clone("clone timed out".into()))??;
    if !out.status.success() {
        let msg = oryn_auth::redact::redact(&String::from_utf8_lossy(&out.stderr));
        return Err(ImportError::Clone(
            msg.lines()
                .rfind(|l| !l.trim().is_empty())
                .unwrap_or("git clone failed")
                .to_string(),
        ));
    }
    let head = tokio::process::Command::new("git")
        .arg("-C")
        .arg(dest)
        .args(["rev-parse", "HEAD"])
        .output()
        .await?;
    // Drop .git: analysis reads the tree, and no git metadata should be served later.
    let _ = std::fs::remove_dir_all(dest.join(".git"));
    Ok(String::from_utf8_lossy(&head.stdout).trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn zip_of(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut buf = std::io::Cursor::new(Vec::new());
        {
            let mut w = zip::ZipWriter::new(&mut buf);
            let opts = zip::write::SimpleFileOptions::default();
            for (n, c) in entries {
                w.start_file(*n, opts).unwrap();
                w.write_all(c).unwrap();
            }
            w.finish().unwrap();
        }
        buf.into_inner()
    }

    #[test]
    fn extracts_and_strips_root() {
        let z = zip_of(&[
            ("repo-main/src/a.js", b"x"),
            ("repo-main/README.md", b"# r"),
        ]);
        let d = tempfile::tempdir().unwrap();
        let (n, _) = extract_zip(&z, &d.path().join("out"), ZipLimits::default()).unwrap();
        assert_eq!(n, 2);
        assert!(d.path().join("out/src/a.js").exists());
    }

    #[test]
    fn rejects_traversal_and_bombs() {
        let d = tempfile::tempdir().unwrap();
        for bad in ["../evil.sh", "/etc/passwd", "a/../../b"] {
            let z = zip_of(&[(bad, b"x")]);
            assert!(
                extract_zip(&z, &d.path().join("o"), ZipLimits::default()).is_err(),
                "{bad}"
            );
        }
        let big = vec![b'a'; 3_000_000];
        let z = zip_of(&[("big.txt", &big)]);
        assert!(extract_zip(
            &z,
            &d.path().join("o2"),
            ZipLimits {
                max_file_bytes: 1_000_000,
                ..Default::default()
            }
        )
        .is_err());
    }

    #[test]
    fn git_url_policy() {
        assert!(validate_git_url("https://github.com/acme/app.git", false).is_ok());
        assert!(validate_git_url("http://github.com/acme/app.git", false).is_err());
        assert!(validate_git_url("https://user:pw@github.com/a/b", false).is_err());
        assert!(validate_git_url("https://127.0.0.1/a/b", false).is_err());
        assert!(validate_git_url("https://10.0.0.5/a/b", true).is_ok());
        assert!(validate_git_url("https://169.254.169.254/x", true).is_err());
        assert!(validate_git_url("file:///etc", true).is_err());
        assert!(!valid_branch("--upload-pack=x"));
    }

    #[test]
    fn pins_checked_addresses_for_git() {
        let a = [
            "140.82.112.3:443".parse().unwrap(),
            "[2606:50c0::1]:443".parse().unwrap(),
        ];
        assert_eq!(
            curl_resolve_entry("github.com", 443, &a),
            "github.com:443:140.82.112.3,[2606:50c0::1]"
        );
    }

    #[tokio::test]
    async fn clone_refuses_names_that_resolve_to_private_addresses() {
        let dir = std::env::temp_dir().join(format!("oryn-clone-{}", std::process::id()));
        let u = url::Url::parse("https://localhost/acme/app.git").unwrap();
        let err = clone_repo(
            &u,
            None,
            None,
            &dir.join("r"),
            Duration::from_secs(5),
            false,
        )
        .await
        .unwrap_err();
        assert!(err.to_string().contains("private address"), "{err}");
    }
}

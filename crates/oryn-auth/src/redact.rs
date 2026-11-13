//! Redaction of credentials and personal data from free text.
//!
//! Applied to logs, evidence and every prompt before it leaves the process for an
//! LLM provider. It is defence in depth, not a guarantee: secrets declared on an
//! environment are additionally redacted by exact value.

use once_cell::sync::Lazy;
use regex::Regex;

static PATTERNS: Lazy<Vec<(Regex, &'static str)>> = Lazy::new(|| {
    vec![
        (Regex::new(r"(?i)\b(bearer)\s+[A-Za-z0-9\-._~+/]{8,}=*").unwrap(), "$1 [REDACTED]"),
        (Regex::new(r"\bsk-(?:ant-|proj-)?[A-Za-z0-9_\-]{16,}").unwrap(), "[REDACTED_API_KEY]"),
        (Regex::new(r"\bgh[pousr]_[A-Za-z0-9]{20,}").unwrap(), "[REDACTED_GITHUB_TOKEN]"),
        (Regex::new(r"\bglpat-[A-Za-z0-9_\-]{20,}").unwrap(), "[REDACTED_GITLAB_TOKEN]"),
        (Regex::new(r"\bAKIA[0-9A-Z]{16}\b").unwrap(), "[REDACTED_AWS_KEY]"),
        (Regex::new(r"\boryn_[a-z]+_[A-Za-z0-9_\-]{20,}").unwrap(), "[REDACTED_ORYN_TOKEN]"),
        (Regex::new(r"eyJ[A-Za-z0-9_\-]{10,}\.[A-Za-z0-9_\-]{10,}\.[A-Za-z0-9_\-]{10,}").unwrap(), "[REDACTED_JWT]"),
        (
            Regex::new(r#"(?i)("?(?:password|passwd|secret|api[_-]?key|access[_-]?token|refresh[_-]?token|client[_-]?secret)"?\s*[:=]\s*)"?[^"\s,}&]+"?"#).unwrap(),
            "$1\"[REDACTED]\"",
        ),
        (Regex::new(r"\b(?:\d[ -]?){13,16}\b").unwrap(), "[REDACTED_CARD]"),
    ]
});

/// Redact well-known secret shapes.
pub fn redact(input: &str) -> String {
    let mut out = input.to_string();
    for (re, rep) in PATTERNS.iter() {
        out = re.replace_all(&out, *rep).into_owned();
    }
    out
}

/// Redact well-known shapes plus exact secret values (e.g. environment secrets).
pub fn redact_with(input: &str, secrets: &[String]) -> String {
    let mut out = redact(input);
    for s in secrets.iter().filter(|s| s.len() >= 4) {
        out = out.replace(s.as_str(), "[REDACTED]");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacts_common_shapes() {
        let r = redact("Authorization: Bearer abcdefghijklmnop123");
        assert!(!r.contains("abcdefghijklmnop123"), "{r}");
        let r = redact(r#"{"password":"hunter22","user":"a"}"#);
        assert!(!r.contains("hunter22"), "{r}");
        assert!(r.contains("\"user\":\"a\""));
        let r = redact("key sk-proj-abcdefghijklmnopqrstuv");
        assert!(r.contains("[REDACTED_API_KEY]"));
        let r = redact("token=ghp_abcdefghijklmnopqrstuvwxyz0123");
        assert!(!r.contains("ghp_abcdefghij"));
        assert!(redact("card 4242 4242 4242 4242 ok").contains("[REDACTED_CARD]"));
        assert_eq!(redact("nothing secret here"), "nothing secret here");
    }

    #[test]
    fn exact_values() {
        let r = redact_with("login with s3cr3t-value", &["s3cr3t-value".to_string()]);
        assert_eq!(r, "login with [REDACTED]");
    }
}

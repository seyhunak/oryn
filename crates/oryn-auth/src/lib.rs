//! Security primitives for ORYN.
//!
//! * Passwords: Argon2id with per-password salts.
//! * Tokens: 256-bit random opaque tokens; only the SHA-256 hash is stored.
//! * Secrets at rest: AES-256-GCM with a key derived from `ORYN_SECRET_KEY`.
//! * Webhooks: HMAC-SHA256 signatures.
//! * Redaction: strips credentials from text before it is stored or sent to an LLM.

pub mod net;
pub mod redact;

use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Nonce};
use argon2::password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use argon2::Argon2;
use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use base64::Engine;
use hmac::{Hmac, Mac};
use rand::RngCore;
use sha2::{Digest, Sha256};

#[derive(Debug, thiserror::Error)]
pub enum AuthError {
    #[error("password hashing failed")]
    Hash,
    #[error("secret decryption failed")]
    Decrypt,
    #[error("invalid secret encoding")]
    Encoding,
    #[error("secret key must be at least 32 bytes")]
    WeakKey,
}

pub fn hash_password(password: &str) -> Result<String, AuthError> {
    let salt = SaltString::generate(&mut rand::rngs::OsRng);
    Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map(|h| h.to_string())
        .map_err(|_| AuthError::Hash)
}

pub fn verify_password(password: &str, hash: &str) -> bool {
    match PasswordHash::new(hash) {
        Ok(parsed) => Argon2::default()
            .verify_password(password.as_bytes(), &parsed)
            .is_ok(),
        Err(_) => false,
    }
}

/// Generate a URL-safe random token. `prefix` is prepended verbatim, so pass it
/// with its separator (`"oryn_pat_"`), or `""` for a bare token.
pub fn generate_token(prefix: &str) -> String {
    let mut bytes = [0u8; 32];
    rand::rngs::OsRng.fill_bytes(&mut bytes);
    format!("{prefix}{}", URL_SAFE_NO_PAD.encode(bytes))
}

/// SHA-256 hex digest used to store tokens.
pub fn hash_token(token: &str) -> String {
    hex::encode(Sha256::digest(token.as_bytes()))
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

/// Constant-time comparison.
pub fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// HMAC-SHA256 signature, hex-encoded, as sent in `X-Oryn-Signature: sha256=<hex>`.
pub fn sign_hmac(secret: &[u8], payload: &[u8]) -> String {
    let mut mac =
        <Hmac<Sha256> as Mac>::new_from_slice(secret).expect("hmac accepts any key length");
    mac.update(payload);
    hex::encode(mac.finalize().into_bytes())
}

/// Symmetric encryption for secrets stored in the database (API keys, env secrets).
#[derive(Clone)]
pub struct SecretBox {
    cipher: Aes256Gcm,
}

impl SecretBox {
    pub fn new(master_key: &str) -> Result<Self, AuthError> {
        if master_key.len() < 32 {
            return Err(AuthError::WeakKey);
        }
        let key = Sha256::digest(master_key.as_bytes());
        let cipher = Aes256Gcm::new_from_slice(&key).map_err(|_| AuthError::WeakKey)?;
        Ok(Self { cipher })
    }

    /// Returns `v1:<base64(nonce || ciphertext)>`.
    pub fn encrypt(&self, plaintext: &str) -> String {
        let mut nonce = [0u8; 12];
        rand::rngs::OsRng.fill_bytes(&mut nonce);
        let ct = self
            .cipher
            .encrypt(Nonce::from_slice(&nonce), plaintext.as_bytes())
            .expect("aes-gcm encryption cannot fail for valid key");
        let mut out = nonce.to_vec();
        out.extend(ct);
        format!("v1:{}", STANDARD.encode(out))
    }

    pub fn decrypt(&self, sealed: &str) -> Result<String, AuthError> {
        let b64 = sealed.strip_prefix("v1:").ok_or(AuthError::Encoding)?;
        let raw = STANDARD.decode(b64).map_err(|_| AuthError::Encoding)?;
        if raw.len() < 13 {
            return Err(AuthError::Encoding);
        }
        let (nonce, ct) = raw.split_at(12);
        let pt = self
            .cipher
            .decrypt(Nonce::from_slice(nonce), ct)
            .map_err(|_| AuthError::Decrypt)?;
        String::from_utf8(pt).map_err(|_| AuthError::Encoding)
    }
}

/// Mask a secret for display: keeps the last 4 characters.
pub fn mask(secret: &str) -> String {
    let n = secret.chars().count();
    if n <= 4 {
        return "••••".to_string();
    }
    let tail: String = secret.chars().skip(n - 4).collect();
    format!("••••{tail}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn password_roundtrip() {
        let h = hash_password("correct horse battery").unwrap();
        assert!(verify_password("correct horse battery", &h));
        assert!(!verify_password("wrong", &h));
        assert!(!verify_password("x", "not-a-hash"));
    }

    #[test]
    fn tokens() {
        let t = generate_token("oryn_ses_");
        // Prefix then exactly 43 random characters: no extra separator.
        assert!(t.starts_with("oryn_ses_"));
        assert_eq!(t.len(), "oryn_ses_".len() + 43);
        assert_eq!(generate_token("").len(), 43); // 32 random bytes, no prefix
        assert_eq!(hash_token(&t).len(), 64);
        assert_ne!(generate_token("a"), generate_token("a"));
    }

    #[test]
    fn secret_box() {
        let sb = SecretBox::new("0123456789abcdef0123456789abcdef").unwrap();
        let sealed = sb.encrypt("sk-live-abc");
        assert!(sealed.starts_with("v1:"));
        assert_eq!(sb.decrypt(&sealed).unwrap(), "sk-live-abc");
        let other = SecretBox::new("ffffffffffffffffffffffffffffffff").unwrap();
        assert!(other.decrypt(&sealed).is_err());
        assert!(SecretBox::new("short").is_err());
    }

    #[test]
    fn hmac_known() {
        assert_eq!(sign_hmac(b"k", b"p").len(), 64);
        assert!(constant_time_eq(b"abc", b"abc"));
        assert!(!constant_time_eq(b"abc", b"abd"));
        assert_eq!(mask("sk-1234567"), "••••4567");
    }
}

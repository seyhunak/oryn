use crate::auth::{cookie_value, Authed, ClientIp, SESSION_COOKIE};
use crate::error::{ApiError, ApiResult};
use crate::routes::{ok, Ok};
use crate::state::AppState;
use axum::extract::{Query, State};
use axum::http::{header, request::Parts, HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Redirect, Response};
use axum::Json;
use base64::Engine as _;
use oryn_db::governance::{audit, Actor};
use oryn_db::identity::{User, WorkspaceWithRole};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::Digest;
use std::time::Duration;
use utoipa::ToSchema;
use utoipa_axum::{router::OpenApiRouter, routes};

pub fn routes() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(providers))
        .routes(routes!(register))
        .routes(routes!(login))
        .routes(routes!(logout))
        .routes(routes!(me))
        .routes(routes!(oidc_login))
        .routes(routes!(oidc_callback))
}

#[derive(Serialize, ToSchema)]
pub struct AuthProviders {
    pub password: bool,
    pub registration_open: bool,
    pub oidc: Option<String>,
    /// True until the first account exists; the first account becomes the instance admin.
    pub needs_setup: bool,
}

#[utoipa::path(get, path = "/api/v1/auth/providers", tag = "auth", responses((status = 200, body = AuthProviders)))]
async fn providers(State(s): State<AppState>) -> ApiResult<Json<AuthProviders>> {
    let users = oryn_db::identity::count_users(&s.pool).await?;
    Ok(Json(AuthProviders {
        password: true,
        registration_open: s.cfg.allow_registration || users == 0,
        oidc: s.cfg.oidc.as_ref().map(|o| o.display_name.clone()),
        needs_setup: users == 0,
    }))
}

#[derive(Deserialize, ToSchema)]
pub struct RegisterReq {
    pub email: String,
    pub name: String,
    pub password: String,
    /// Name of the first workspace; defaults to "<name>'s workspace".
    pub workspace_name: Option<String>,
}

#[derive(Deserialize, ToSchema)]
pub struct LoginReq {
    pub email: String,
    pub password: String,
}

#[derive(Serialize, ToSchema)]
pub struct Me {
    pub user: User,
    pub workspaces: Vec<WorkspaceWithRole>,
}

fn valid_email(e: &str) -> bool {
    let e = e.trim();
    e.len() <= 254
        && e.split_once('@')
            .map(|(l, d)| {
                !l.is_empty() && d.contains('.') && !d.starts_with('.') && !d.ends_with('.')
            })
            .unwrap_or(false)
        && !e.contains(char::is_whitespace)
}

fn session_cookie(s: &AppState, token: &str, max_age: i64) -> HeaderValue {
    let secure = if s.cfg.cookie_secure { "; Secure" } else { "" };
    HeaderValue::from_str(&format!(
        "{SESSION_COOKIE}={token}; HttpOnly; SameSite=Lax; Path=/; Max-Age={max_age}{secure}"
    ))
    .expect("cookie header")
}

async fn start_session(
    s: &AppState,
    user: &User,
    headers: &HeaderMap,
    ip: Option<String>,
) -> ApiResult<HeaderValue> {
    let token = oryn_auth::generate_token("oryn_ses_");
    let ua = headers
        .get(header::USER_AGENT)
        .and_then(|v| v.to_str().ok())
        .map(|v| v.chars().take(300).collect::<String>());
    oryn_db::identity::create_session(
        &s.pool,
        user.id,
        &oryn_auth::hash_token(&token),
        s.cfg.session_ttl_hours,
        ua.as_deref(),
        ip.as_deref(),
    )
    .await?;
    Ok(session_cookie(s, &token, s.cfg.session_ttl_hours * 3600))
}

async fn me_payload(s: &AppState, user: User) -> ApiResult<Me> {
    let workspaces = oryn_db::identity::list_user_workspaces(&s.pool, user.id).await?;
    Ok(Me { user, workspaces })
}

#[utoipa::path(post, path = "/api/v1/auth/register", tag = "auth", responses((status = 201, body = Me), (status = 409), (status = 403)))]
async fn register(
    State(s): State<AppState>,
    ClientIp(ip): ClientIp,
    headers: HeaderMap,
    Json(req): Json<RegisterReq>,
) -> ApiResult<Response> {
    s.limiter
        .check(
            &format!("register:{}", ip.clone().unwrap_or_default()),
            10,
            Duration::from_secs(600),
        )
        .await?;
    let users = oryn_db::identity::count_users(&s.pool).await?;
    if users > 0 && !s.cfg.allow_registration {
        return Err(ApiError::forbidden(
            "registration is closed on this instance; ask an admin to add you",
        ));
    }
    let email = req.email.trim().to_ascii_lowercase();
    if !valid_email(&email) {
        return Err(ApiError::bad("enter a valid email address"));
    }
    let name = req.name.trim();
    if name.is_empty() || name.len() > 120 {
        return Err(ApiError::bad("name must be 1 to 120 characters"));
    }
    if req.password.len() < 10 || req.password.len() > 256 {
        return Err(ApiError::bad("password must be at least 10 characters"));
    }
    let hash = oryn_auth::hash_password(&req.password)
        .map_err(|e| ApiError::Internal(anyhow::anyhow!("{e}")))?;
    let user = oryn_db::identity::create_user(&s.pool, &email, name, Some(&hash), users == 0)
        .await
        .map_err(|e| match e {
            oryn_db::DbError::Conflict(_) => {
                ApiError::Conflict("an account with that email already exists".into())
            }
            e => e.into(),
        })?;
    let ws_name = req
        .workspace_name
        .filter(|w| !w.trim().is_empty())
        .unwrap_or_else(|| format!("{name}'s workspace"));
    let ws = oryn_db::identity::create_workspace(&s.pool, ws_name.trim(), user.id).await?;
    let actor = Actor {
        kind: "user",
        id: Some(user.id),
        label: user.email.clone(),
        ip: ip.clone(),
    };
    audit(
        &s.pool,
        Some(ws.id),
        &actor,
        "user.registered",
        Some("user"),
        Some(user.id),
        json!({"instance_admin": users == 0}),
    )
    .await;
    let cookie = start_session(&s, &user, &headers, ip).await?;
    let body = me_payload(&s, user).await?;
    Ok((
        StatusCode::CREATED,
        [(header::SET_COOKIE, cookie)],
        Json(body),
    )
        .into_response())
}

/// Failed password attempts per account allowed within [`LOCKOUT_WINDOW`].
const LOCKOUT_FAILURES: u32 = 10;
const LOCKOUT_WINDOW: Duration = Duration::from_secs(900);

#[utoipa::path(post, path = "/api/v1/auth/login", tag = "auth", responses((status = 200, body = Me), (status = 401), (status = 429)))]
async fn login(
    State(s): State<AppState>,
    ClientIp(ip): ClientIp,
    headers: HeaderMap,
    Json(req): Json<LoginReq>,
) -> ApiResult<Response> {
    let email = req.email.trim().to_ascii_lowercase();
    s.limiter
        .check(
            &format!("login:{}", ip.clone().unwrap_or_default()),
            30,
            Duration::from_secs(300),
        )
        .await?;
    // Per-account lockout counts failed attempts only, so legitimate sign-ins (and
    // automation using the same account) never lock an account out.
    let fail_key = format!("login-fail:{email}");
    if s.limiter.peek(&fail_key, LOCKOUT_WINDOW).await >= LOCKOUT_FAILURES {
        return Err(ApiError::RateLimited);
    }
    let user = oryn_db::identity::find_user_by_email(&s.pool, &email).await?;
    // Verify against a dummy hash when the user does not exist, so timing does not reveal accounts.
    static DUMMY: once_cell::sync::Lazy<String> = once_cell::sync::Lazy::new(|| {
        oryn_auth::hash_password("oryn-timing-equaliser").unwrap_or_default()
    });
    let ok = match &user {
        Some(u) => u
            .password_hash
            .as_deref()
            .map(|h| oryn_auth::verify_password(&req.password, h))
            .unwrap_or(false),
        None => {
            let _ = oryn_auth::verify_password(&req.password, &DUMMY);
            false
        }
    };
    let actor = Actor {
        kind: "user",
        id: user.as_ref().map(|u| u.id),
        label: email.clone(),
        ip: ip.clone(),
    };
    if !ok {
        let _ = s
            .limiter
            .check(&fail_key, LOCKOUT_FAILURES, LOCKOUT_WINDOW)
            .await;
        audit(
            &s.pool,
            None,
            &actor,
            "auth.login_failed",
            None,
            None,
            json!({}),
        )
        .await;
        return Err(ApiError::Unauthorized);
    }
    let user = user.expect("checked");
    audit(
        &s.pool,
        None,
        &actor,
        "auth.login",
        Some("user"),
        Some(user.id),
        json!({"method": "password"}),
    )
    .await;
    let cookie = start_session(&s, &user, &headers, ip).await?;
    let body = me_payload(&s, user).await?;
    Ok(([(header::SET_COOKIE, cookie)], Json(body)).into_response())
}

struct RawParts(Parts);
impl<S: Send + Sync> axum::extract::FromRequestParts<S> for RawParts {
    type Rejection = std::convert::Infallible;
    async fn from_request_parts(parts: &mut Parts, _: &S) -> Result<Self, Self::Rejection> {
        Ok(RawParts(parts.clone()))
    }
}

#[utoipa::path(post, path = "/api/v1/auth/logout", tag = "auth", responses((status = 200, body = Ok)))]
async fn logout(State(s): State<AppState>, RawParts(parts): RawParts) -> ApiResult<Response> {
    if let Some(t) = cookie_value(&parts, SESSION_COOKIE) {
        oryn_db::identity::delete_session(&s.pool, &oryn_auth::hash_token(&t)).await?;
    }
    Ok(([(header::SET_COOKIE, session_cookie(&s, "", 0))], ok()).into_response())
}

#[utoipa::path(get, path = "/api/v1/auth/me", tag = "auth", responses((status = 200, body = Me), (status = 401)))]
async fn me(State(s): State<AppState>, auth: Authed) -> ApiResult<Json<Me>> {
    Ok(Json(me_payload(&s, auth.user).await?))
}

// ── OIDC (authorization code + PKCE) ──

#[derive(Deserialize)]
struct Discovery {
    authorization_endpoint: String,
    token_endpoint: String,
    userinfo_endpoint: String,
}

async fn discover(s: &AppState) -> ApiResult<(crate::config::OidcConfig, Discovery)> {
    let cfg = s
        .cfg
        .oidc
        .clone()
        .ok_or_else(|| ApiError::not_found("OIDC provider"))?;
    let d: Discovery = s
        .http
        .get(format!("{}/.well-known/openid-configuration", cfg.issuer))
        .send()
        .await
        .and_then(|r| r.error_for_status())
        .map_err(|e| ApiError::Unavailable(format!("OIDC discovery failed: {e}")))?
        .json()
        .await
        .map_err(|e| ApiError::Unavailable(format!("OIDC discovery document invalid: {e}")))?;
    Ok((cfg, d))
}

fn b64url(bytes: &[u8]) -> String {
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

fn redirect_uri(s: &AppState) -> String {
    format!("{}/api/v1/auth/oidc/callback", s.cfg.public_url)
}

#[derive(Deserialize, utoipa::IntoParams)]
struct OidcLoginQuery {
    /// Relative path to return to after sign-in.
    next: Option<String>,
}

#[utoipa::path(get, path = "/api/v1/auth/oidc/login", tag = "auth", params(OidcLoginQuery), responses((status = 303)))]
async fn oidc_login(
    State(s): State<AppState>,
    Query(q): Query<OidcLoginQuery>,
) -> ApiResult<Redirect> {
    let (cfg, d) = discover(&s).await?;
    let state = oryn_auth::generate_token("");
    let nonce = oryn_auth::generate_token("");
    let verifier = oryn_auth::generate_token("");
    let challenge = b64url(&sha2::Sha256::digest(verifier.as_bytes()));
    let next = q
        .next
        .filter(|n| n.starts_with('/') && !n.starts_with("//"));
    oryn_db::identity::put_oidc_state(&s.pool, &state, &nonce, &verifier, next.as_deref()).await?;
    let mut url = url::Url::parse(&d.authorization_endpoint)
        .map_err(|e| ApiError::Unavailable(e.to_string()))?;
    url.query_pairs_mut()
        .append_pair("response_type", "code")
        .append_pair("client_id", &cfg.client_id)
        .append_pair("redirect_uri", &redirect_uri(&s))
        .append_pair("scope", &cfg.scopes)
        .append_pair("state", &state)
        .append_pair("nonce", &nonce)
        .append_pair("code_challenge", &challenge)
        .append_pair("code_challenge_method", "S256");
    Ok(Redirect::to(url.as_str()))
}

#[derive(Deserialize, utoipa::IntoParams)]
struct OidcCallback {
    code: Option<String>,
    state: Option<String>,
    error: Option<String>,
}

#[utoipa::path(get, path = "/api/v1/auth/oidc/callback", tag = "auth", params(OidcCallback), responses((status = 303)))]
async fn oidc_callback(
    State(s): State<AppState>,
    ClientIp(ip): ClientIp,
    headers: HeaderMap,
    Query(q): Query<OidcCallback>,
) -> ApiResult<Response> {
    let fail = |m: &str| {
        Redirect::to(&format!(
            "{}/login?error={}",
            s.cfg.public_url,
            urlencode(m)
        ))
        .into_response()
    };
    if let Some(e) = q.error {
        return Ok(fail(&e));
    }
    let (Some(code), Some(state)) = (q.code, q.state) else {
        return Ok(fail("missing_code"));
    };
    let Some((nonce, verifier, next)) = oryn_db::identity::take_oidc_state(&s.pool, &state).await?
    else {
        return Ok(fail("invalid_state"));
    };
    let (cfg, d) = discover(&s).await?;
    let mut form = vec![
        ("grant_type", "authorization_code".to_string()),
        ("code", code),
        ("redirect_uri", redirect_uri(&s)),
        ("client_id", cfg.client_id.clone()),
        ("code_verifier", verifier),
    ];
    if let Some(sec) = &cfg.client_secret {
        form.push(("client_secret", sec.clone()));
    }
    let tok: serde_json::Value = match s.http.post(&d.token_endpoint).form(&form).send().await {
        Result::Ok(r) if r.status().is_success() => r.json().await.unwrap_or_default(),
        _ => return Ok(fail("token_exchange_failed")),
    };
    let Some(access) = tok["access_token"].as_str() else {
        return Ok(fail("no_access_token"));
    };
    let claims = match tok["id_token"]
        .as_str()
        .ok_or("no_id_token")
        .and_then(|t| validate_id_token(t, &cfg.issuer, &cfg.client_id, &nonce))
    {
        Result::Ok(c) => c,
        Err(e) => return Ok(fail(e)),
    };
    let info: serde_json::Value = match s
        .http
        .get(&d.userinfo_endpoint)
        .bearer_auth(access)
        .send()
        .await
    {
        Result::Ok(r) if r.status().is_success() => r.json().await.unwrap_or_default(),
        _ => return Ok(fail("userinfo_failed")),
    };
    let (Some(sub), Some(email)) = (info["sub"].as_str(), info["email"].as_str()) else {
        return Ok(fail("missing_claims"));
    };
    // OIDC Core 5.3.2: the UserInfo subject must be the ID token's subject.
    if claims["sub"].as_str() != Some(sub) {
        return Ok(fail("subject_mismatch"));
    }
    // Accounts are matched by email, so the provider must vouch for it.
    if info["email_verified"] != json!(true) && claims["email_verified"] != json!(true) {
        return Ok(fail("email_not_verified"));
    }
    let name = info["name"]
        .as_str()
        .or(info["preferred_username"].as_str())
        .unwrap_or(email);
    let user = match oryn_db::identity::upsert_oidc_user(
        &s.pool,
        &cfg.issuer,
        sub,
        &email.to_ascii_lowercase(),
        name,
        s.cfg.allow_registration,
    )
    .await?
    {
        oryn_db::identity::OidcLink::User(u) => u,
        oryn_db::identity::OidcLink::RegistrationClosed => return Ok(fail("registration_closed")),
        oryn_db::identity::OidcLink::LinkedElsewhere => {
            return Ok(fail("account_linked_elsewhere"))
        }
    };
    if oryn_db::identity::list_user_workspaces(&s.pool, user.id)
        .await?
        .is_empty()
    {
        oryn_db::identity::create_workspace(&s.pool, &format!("{name}'s workspace"), user.id)
            .await?;
    }
    audit(
        &s.pool,
        None,
        &Actor {
            kind: "user",
            id: Some(user.id),
            label: user.email.clone(),
            ip: ip.clone(),
        },
        "auth.login",
        Some("user"),
        Some(user.id),
        json!({"method": "oidc"}),
    )
    .await;
    let cookie = start_session(&s, &user, &headers, ip).await?;
    let dest = format!("{}{}", s.cfg.public_url, next.unwrap_or_else(|| "/".into()));
    Ok(([(header::SET_COOKIE, cookie)], Redirect::to(&dest)).into_response())
}

/// Validate the claims of an ID token received directly from the token endpoint.
/// The token arrives over a TLS connection ORYN opened to the issuer, so per
/// OIDC Core 3.1.3.7 (6) TLS server validation stands in for the signature check;
/// issuer, audience, authorised party, expiry and nonce are still enforced.
fn validate_id_token(
    token: &str,
    issuer: &str,
    client_id: &str,
    nonce: &str,
) -> Result<serde_json::Value, &'static str> {
    let payload = token.split('.').nth(1).ok_or("malformed_id_token")?;
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(payload.trim_end_matches('='))
        .map_err(|_| "malformed_id_token")?;
    let c: serde_json::Value = serde_json::from_slice(&bytes).map_err(|_| "malformed_id_token")?;
    let norm = |v: &str| v.trim_end_matches('/').to_string();
    if c["iss"].as_str().map(norm) != Some(norm(issuer)) {
        return Err("id_token_issuer");
    }
    let aud_ok = match &c["aud"] {
        serde_json::Value::String(a) => a == client_id,
        serde_json::Value::Array(list) => list.iter().any(|a| a.as_str() == Some(client_id)),
        _ => false,
    };
    if !aud_ok {
        return Err("id_token_audience");
    }
    if let Some(azp) = c["azp"].as_str() {
        if azp != client_id {
            return Err("id_token_audience");
        }
    }
    let now = chrono::Utc::now().timestamp();
    // One minute of clock skew.
    if c["exp"].as_i64().is_none_or(|exp| exp + 60 < now) {
        return Err("id_token_expired");
    }
    if c["nonce"].as_str() != Some(nonce) {
        return Err("id_token_nonce");
    }
    Ok(c)
}

fn urlencode(s: &str) -> String {
    url::form_urlencoded::byte_serialize(s.as_bytes()).collect()
}

#[cfg(test)]
mod tests {
    use super::validate_id_token;
    use base64::Engine as _;

    fn token(claims: serde_json::Value) -> String {
        let e = |v: &[u8]| base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(v);
        format!(
            "{}.{}.sig",
            e(br#"{"alg":"RS256"}"#),
            e(claims.to_string().as_bytes())
        )
    }

    #[test]
    fn id_token_claims_are_enforced() {
        let exp = chrono::Utc::now().timestamp() + 300;
        let good = serde_json::json!({"iss": "https://idp.test/", "aud": ["oryn"], "exp": exp, "nonce": "n1", "sub": "u"});
        assert!(validate_id_token(&token(good.clone()), "https://idp.test", "oryn", "n1").is_ok());
        let bad = |k: &str, v: serde_json::Value| {
            let mut c = good.clone();
            c[k] = v;
            validate_id_token(&token(c), "https://idp.test", "oryn", "n1").unwrap_err()
        };
        assert_eq!(bad("iss", "https://evil.test".into()), "id_token_issuer");
        assert_eq!(bad("aud", "someone-else".into()), "id_token_audience");
        assert_eq!(bad("azp", "someone-else".into()), "id_token_audience");
        assert_eq!(bad("exp", (exp - 3600).into()), "id_token_expired");
        assert_eq!(bad("nonce", "replayed".into()), "id_token_nonce");
        assert_eq!(
            validate_id_token("garbage", "https://idp.test", "oryn", "n1").unwrap_err(),
            "malformed_id_token"
        );
    }
}

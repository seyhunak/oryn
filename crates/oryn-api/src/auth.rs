//! Authentication (session cookie or bearer API token) and authorisation contexts.
//!
//! Every handler that touches workspace data resolves a [`WsCtx`] or [`ProjectCtx`]
//! and calls `require(Permission)`. RBAC is enforced here, on the server; the UI only
//! mirrors it.

use crate::error::{ApiError, ApiResult};
use crate::state::AppState;
use axum::extract::FromRequestParts;
use axum::http::{header, request::Parts, Method};
use oryn_db::governance::Actor;
use oryn_db::identity::User;
use oryn_db::projects::Project;
use oryn_domain::{Permission, Role};
use uuid::Uuid;

pub const SESSION_COOKIE: &str = "oryn_session";
/// Cookie-authenticated mutating requests must send this header (CSRF defence:
/// a cross-site form cannot set custom headers).
pub const CSRF_HEADER: &str = "x-oryn-csrf";

#[derive(Debug, Clone)]
pub enum Via {
    Session,
    /// API token scoped to one workspace with a role ceiling.
    Token {
        workspace_id: Uuid,
        role: Role,
    },
}

#[derive(Debug, Clone)]
pub struct Authed {
    pub user: User,
    pub via: Via,
    pub ip: Option<String>,
}

impl Authed {
    pub fn actor(&self) -> Actor {
        Actor {
            kind: match self.via {
                Via::Session => "user",
                Via::Token { .. } => "api_token",
            },
            id: Some(self.user.id),
            label: self.user.email.clone(),
            ip: self.ip.clone(),
        }
    }
}

pub fn cookie_value(parts: &Parts, name: &str) -> Option<String> {
    parts
        .headers
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(';'))
        .find_map(|kv| {
            let (k, v) = kv.trim().split_once('=')?;
            (k == name).then(|| v.to_string())
        })
}

/// The client's address. `X-Forwarded-For` is believed only when the direct peer is a
/// trusted proxy; the client is then the right-most hop that is not itself trusted.
pub fn client_ip(parts: &Parts, trusted: &[ipnet::IpNet]) -> Option<String> {
    let peer = parts
        .extensions
        .get::<axum::extract::ConnectInfo<std::net::SocketAddr>>()
        .map(|c| c.0.ip());
    resolve_client_ip(
        peer,
        parts
            .headers
            .get("x-forwarded-for")
            .and_then(|v| v.to_str().ok()),
        trusted,
    )
    .map(|ip| ip.to_string())
}

pub fn resolve_client_ip(
    peer: Option<std::net::IpAddr>,
    forwarded_for: Option<&str>,
    trusted: &[ipnet::IpNet],
) -> Option<std::net::IpAddr> {
    let is_trusted = |ip: &std::net::IpAddr| trusted.iter().any(|n| n.contains(ip));
    let peer = peer?;
    if !is_trusted(&peer) {
        return Some(peer);
    }
    let Some(xff) = forwarded_for else {
        return Some(peer);
    };
    let hops: Vec<std::net::IpAddr> = xff
        .split(',')
        .filter_map(|h| h.trim().parse().ok())
        .collect();
    Some(
        hops.iter()
            .rev()
            .find(|ip| !is_trusted(ip))
            .or(hops.first())
            .copied()
            .unwrap_or(peer),
    )
}

/// Extractor for the client's address under the trusted-proxy rule.
pub struct ClientIp(pub Option<String>);

impl FromRequestParts<AppState> for ClientIp {
    type Rejection = std::convert::Infallible;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        Ok(ClientIp(client_ip(parts, &state.cfg.trusted_proxies)))
    }
}

#[cfg(test)]
mod ip_tests {
    use super::resolve_client_ip;
    use crate::config::parse_trusted_proxies;

    #[test]
    fn forwarded_for_is_only_believed_from_trusted_peers() {
        let t = parse_trusted_proxies(None).unwrap();
        let public = "203.0.113.9".parse().ok();
        // An internet client cannot spoof its address.
        assert_eq!(resolve_client_ip(public, Some("1.2.3.4"), &t), public);
        // Behind the bundled proxy, the right-most untrusted hop is the client.
        let proxy = "172.18.0.5".parse().ok();
        assert_eq!(
            resolve_client_ip(proxy, Some("1.2.3.4, 198.51.100.7"), &t),
            "198.51.100.7".parse().ok()
        );
        assert_eq!(resolve_client_ip(proxy, None, &t), proxy);
        // With no trusted proxies the header is ignored entirely.
        assert_eq!(resolve_client_ip(proxy, Some("1.2.3.4"), &[]), proxy);
        assert!(parse_trusted_proxies(Some("nonsense")).is_err());
        assert!(parse_trusted_proxies(Some("none")).unwrap().is_empty());
    }
}

impl FromRequestParts<AppState> for Authed {
    type Rejection = ApiError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let ip = client_ip(parts, &state.cfg.trusted_proxies);
        if let Some(bearer) = parts
            .headers
            .get(header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer "))
        {
            let bearer = bearer.trim();
            if !bearer.starts_with("oryn_pat_") {
                return Err(ApiError::Unauthorized);
            }
            let Some((user, ws, role)) =
                oryn_db::identity::api_token_user(&state.pool, &oryn_auth::hash_token(bearer))
                    .await?
            else {
                return Err(ApiError::Unauthorized);
            };
            let role: Role = role.parse().map_err(|_| ApiError::Unauthorized)?;
            return Ok(Authed {
                user,
                via: Via::Token {
                    workspace_id: ws,
                    role,
                },
                ip,
            });
        }
        let token = cookie_value(parts, SESSION_COOKIE).ok_or(ApiError::Unauthorized)?;
        let user = oryn_db::identity::session_user(&state.pool, &oryn_auth::hash_token(&token))
            .await?
            .ok_or(ApiError::Unauthorized)?;
        if !matches!(parts.method, Method::GET | Method::HEAD | Method::OPTIONS)
            && parts.headers.get(CSRF_HEADER).is_none()
        {
            return Err(ApiError::forbidden("missing CSRF header"));
        }
        Ok(Authed {
            user,
            via: Via::Session,
            ip,
        })
    }
}

/// The caller's effective role inside one workspace.
#[derive(Debug, Clone)]
pub struct WsCtx {
    pub auth: Authed,
    pub workspace_id: Uuid,
    pub role: Role,
}

impl WsCtx {
    pub fn require(&self, p: Permission) -> ApiResult<()> {
        if self.role.can(p) {
            Ok(())
        } else {
            Err(ApiError::forbidden(format!(
                "your role ({}) does not grant {}",
                self.role, p
            )))
        }
    }
    pub fn user_id(&self) -> Uuid {
        self.auth.user.id
    }
    pub fn actor(&self) -> Actor {
        self.auth.actor()
    }
}

fn min_role(a: Role, b: Role) -> Role {
    if a.rank() <= b.rank() {
        a
    } else {
        b
    }
}

pub async fn ws_ctx(state: &AppState, auth: &Authed, ws: Uuid) -> ApiResult<WsCtx> {
    let member = oryn_db::identity::membership_role(&state.pool, ws, auth.user.id).await?;
    // Non-members get 404, not 403: workspace existence is not disclosed.
    let member: Role = member
        .ok_or_else(|| ApiError::not_found("workspace"))?
        .parse()
        .map_err(|_| ApiError::not_found("workspace"))?;
    let role = match &auth.via {
        Via::Session => member,
        Via::Token { workspace_id, role } => {
            if *workspace_id != ws {
                return Err(ApiError::not_found("workspace"));
            }
            min_role(member, *role)
        }
    };
    Ok(WsCtx {
        auth: auth.clone(),
        workspace_id: ws,
        role,
    })
}

#[derive(Debug, Clone)]
pub struct ProjectCtx {
    pub ws: WsCtx,
    pub project: Project,
    pub role: Role,
}

impl ProjectCtx {
    pub fn require(&self, p: Permission) -> ApiResult<()> {
        if self.role.can(p) {
            Ok(())
        } else {
            Err(ApiError::forbidden(format!(
                "your role on this project ({}) does not grant {}",
                self.role, p
            )))
        }
    }
    pub fn id(&self) -> Uuid {
        self.project.id
    }
    pub fn ws_id(&self) -> Uuid {
        self.project.workspace_id
    }
    pub fn user_id(&self) -> Uuid {
        self.ws.auth.user.id
    }
    pub fn actor(&self) -> Actor {
        self.ws.auth.actor()
    }
}

/// Effective project role:
/// * workspace Owner/Admin: their workspace role;
/// * `workspace`-visible projects: the higher of the workspace role and any project grant;
/// * `private` projects: only an explicit project grant (user or team), capped by the token ceiling.
pub async fn project_ctx(
    state: &AppState,
    auth: &Authed,
    project_id: Uuid,
) -> ApiResult<ProjectCtx> {
    let project = oryn_db::projects::get_project(&state.pool, project_id)
        .await
        .map_err(|_| ApiError::not_found("project"))?;
    let ws = ws_ctx(state, auth, project.workspace_id)
        .await
        .map_err(|_| ApiError::not_found("project"))?;
    let grant: Option<Role> =
        oryn_db::projects::project_role(&state.pool, project_id, auth.user.id)
            .await?
            .and_then(|r| r.parse().ok());
    let role = if ws.role.rank() >= Role::Admin.rank() {
        ws.role
    } else if project.visibility == "private" {
        let g = grant.ok_or_else(|| ApiError::not_found("project"))?;
        match &auth.via {
            Via::Token { role, .. } => min_role(g, *role),
            Via::Session => g,
        }
    } else {
        match grant {
            Some(g) if g.rank() > ws.role.rank() => match &auth.via {
                Via::Token { role, .. } => min_role(g, *role),
                Via::Session => g,
            },
            _ => ws.role,
        }
    };
    Ok(ProjectCtx { ws, project, role })
}

/// Project ids the caller can see in a workspace.
pub async fn visible_projects(state: &AppState, ws: &WsCtx) -> ApiResult<Vec<Uuid>> {
    let see_all = ws.role.rank() >= Role::Admin.rank();
    Ok(
        oryn_db::projects::list_projects(&state.pool, ws.workspace_id, ws.user_id(), see_all)
            .await?
            .into_iter()
            .map(|p| p.id)
            .collect(),
    )
}

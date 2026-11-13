use crate::auth::{project_ctx, ws_ctx, Authed};
use crate::error::{ApiError, ApiResult};
use crate::routes::{ok, Ok};
use crate::services::repos::{repo_dir, spawn_import, Source};
use crate::state::AppState;
use axum::body::Bytes;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::Json;
use oryn_db::governance::audit;
use oryn_db::projects::{
    ApplicationContext, ApplicationModule, ContextChunk, Repository, RepositoryConnection,
};
use oryn_domain::Permission;
use oryn_intelligence::import::validate_git_url;
use serde::{Deserialize, Serialize};
use serde_json::json;
use utoipa::{IntoParams, ToSchema};
use utoipa_axum::{router::OpenApiRouter, routes};
use uuid::Uuid;

pub fn routes() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(list_connections, create_connection))
        .routes(routes!(delete_connection))
        .routes(routes!(verify_connection))
        .routes(routes!(list_repositories, add_repository))
        .routes(routes!(upload_zip))
        .routes(routes!(reanalyze))
        .routes(routes!(delete_repository))
        .routes(routes!(get_context))
        .routes(routes!(search_context))
}

#[utoipa::path(get, path = "/api/v1/workspaces/{ws}/connections", tag = "repositories", responses((status = 200, body = Vec<RepositoryConnection>)))]
async fn list_connections(
    State(s): State<AppState>,
    auth: Authed,
    Path(ws): Path<Uuid>,
) -> ApiResult<Json<Vec<RepositoryConnection>>> {
    ws_ctx(&s, &auth, ws)
        .await?
        .require(Permission::WorkspaceRead)?;
    Ok(Json(
        oryn_db::projects::list_connections(&s.pool, ws).await?,
    ))
}

#[derive(Deserialize, ToSchema)]
pub struct CreateConnection {
    /// `github` or `gitlab`.
    pub kind: String,
    pub name: String,
    /// Web base URL. Defaults to https://github.com or https://gitlab.com.
    pub base_url: Option<String>,
    /// Personal access token with read access to repositories. Encrypted at rest.
    pub token: Option<String>,
}

#[utoipa::path(post, path = "/api/v1/workspaces/{ws}/connections", tag = "repositories", responses((status = 201, body = RepositoryConnection)))]
async fn create_connection(
    State(s): State<AppState>,
    auth: Authed,
    Path(ws): Path<Uuid>,
    Json(req): Json<CreateConnection>,
) -> ApiResult<(StatusCode, Json<RepositoryConnection>)> {
    let c = ws_ctx(&s, &auth, ws).await?;
    c.require(Permission::SecretsManage)?;
    let default = match req.kind.as_str() {
        "github" => "https://github.com",
        "gitlab" => "https://gitlab.com",
        _ => return Err(ApiError::bad("kind must be github or gitlab")),
    };
    let base = req
        .base_url
        .filter(|b| !b.trim().is_empty())
        .unwrap_or_else(|| default.into());
    validate_git_url(&base, s.cfg.allow_private_network)
        .map_err(|e| ApiError::bad(e.to_string()))?;
    let token = req.token.filter(|t| !t.is_empty()).map(|t| s.seal(&t));
    let conn = oryn_db::projects::create_connection(
        &s.pool,
        ws,
        &req.kind,
        req.name.trim(),
        base.trim_end_matches('/'),
        token.as_deref(),
        c.user_id(),
    )
    .await?;
    audit(
        &s.pool,
        Some(ws),
        &c.actor(),
        "connection.created",
        Some("connection"),
        Some(conn.id),
        json!({"kind": conn.kind, "base_url": conn.base_url}),
    )
    .await;
    Ok((StatusCode::CREATED, Json(conn)))
}

#[utoipa::path(delete, path = "/api/v1/workspaces/{ws}/connections/{id}", tag = "repositories", responses((status = 200, body = Ok)))]
async fn delete_connection(
    State(s): State<AppState>,
    auth: Authed,
    Path((ws, id)): Path<(Uuid, Uuid)>,
) -> ApiResult<Json<Ok>> {
    let c = ws_ctx(&s, &auth, ws).await?;
    c.require(Permission::SecretsManage)?;
    oryn_db::projects::delete_connection(&s.pool, ws, id).await?;
    audit(
        &s.pool,
        Some(ws),
        &c.actor(),
        "connection.deleted",
        Some("connection"),
        Some(id),
        json!({}),
    )
    .await;
    Ok(ok())
}

#[derive(Serialize, ToSchema)]
pub struct VerifyResult {
    pub ok: bool,
    pub account: Option<String>,
    pub message: String,
}

fn api_base(conn: &RepositoryConnection) -> String {
    match conn.kind.as_str() {
        "github" if conn.base_url == "https://github.com" => "https://api.github.com".into(),
        "github" => format!("{}/api/v3", conn.base_url),
        _ => format!("{}/api/v4", conn.base_url),
    }
}

/// Check the stored token against the provider's "current user" endpoint.
#[utoipa::path(post, path = "/api/v1/workspaces/{ws}/connections/{id}/verify", tag = "repositories", responses((status = 200, body = VerifyResult)))]
async fn verify_connection(
    State(s): State<AppState>,
    auth: Authed,
    Path((ws, id)): Path<(Uuid, Uuid)>,
) -> ApiResult<Json<VerifyResult>> {
    ws_ctx(&s, &auth, ws)
        .await?
        .require(Permission::WorkspaceRead)?;
    let conn = oryn_db::projects::get_connection(&s.pool, ws, id).await?;
    let Some(sealed) = &conn.token_encrypted else {
        return Ok(Json(VerifyResult {
            ok: true,
            account: None,
            message: "No token stored; only public repositories can be imported.".into(),
        }));
    };
    let token = s.open(sealed)?;
    let url = format!("{}/user", api_base(&conn));
    // IP-literal hosts never reach the guarded resolver, so check them here.
    if let Err(e) = crate::services::notify::check_outbound(&url, s.cfg.allow_private_network).await
    {
        return Ok(Json(VerifyResult {
            ok: false,
            account: None,
            message: e,
        }));
    }
    let mut rb = s.outbound.get(&url);
    rb = if conn.kind == "github" {
        rb.bearer_auth(&token)
            .header("accept", "application/vnd.github+json")
    } else {
        rb.header("PRIVATE-TOKEN", &token)
    };
    match rb.send().await {
        Result::Ok(r) if r.status().is_success() => {
            let v: serde_json::Value = r.json().await.unwrap_or_default();
            let account = v["login"]
                .as_str()
                .or(v["username"].as_str())
                .map(String::from);
            Ok(Json(VerifyResult {
                ok: true,
                account,
                message: "Token accepted.".into(),
            }))
        }
        Result::Ok(r) => Ok(Json(VerifyResult {
            ok: false,
            account: None,
            message: format!(
                "The provider rejected the token (HTTP {}).",
                r.status().as_u16()
            ),
        })),
        Err(e) => Ok(Json(VerifyResult {
            ok: false,
            account: None,
            message: format!("Could not reach {}: {e}", api_base(&conn)),
        })),
    }
}

#[utoipa::path(get, path = "/api/v1/projects/{id}/repositories", tag = "repositories", responses((status = 200, body = Vec<Repository>)))]
async fn list_repositories(
    State(s): State<AppState>,
    auth: Authed,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Vec<Repository>>> {
    project_ctx(&s, &auth, id)
        .await?
        .require(Permission::ProjectRead)?;
    Ok(Json(
        oryn_db::projects::list_repositories(&s.pool, id).await?,
    ))
}

#[derive(Deserialize, ToSchema)]
pub struct AddRepository {
    /// `github`, `gitlab`, `git` (any https URL) or `local`.
    pub source: String,
    /// Full https clone URL (for `git`, or instead of `repo`).
    pub url: Option<String>,
    /// `owner/name` on the connection's host (github/gitlab).
    pub repo: Option<String>,
    pub branch: Option<String>,
    pub connection_id: Option<Uuid>,
    /// For `local`: a directory under the server's ORYN_LOCAL_IMPORT_ROOT.
    pub path: Option<String>,
}

#[utoipa::path(post, path = "/api/v1/projects/{id}/repositories", tag = "repositories", responses((status = 202, body = Repository)))]
async fn add_repository(
    State(s): State<AppState>,
    auth: Authed,
    Path(id): Path<Uuid>,
    Json(req): Json<AddRepository>,
) -> ApiResult<(StatusCode, Json<Repository>)> {
    let p = project_ctx(&s, &auth, id).await?;
    p.require(Permission::ProjectWrite)?;
    let repo_id = oryn_db::new_id();
    let (source, url, local_path, src) = match req.source.as_str() {
        "github" | "gitlab" | "git" => {
            let (conn, token) = match req.connection_id {
                Some(cid) => {
                    let c = oryn_db::projects::get_connection(&s.pool, p.ws_id(), cid).await?;
                    let t = match &c.token_encrypted {
                        Some(x) => Some(s.open(x)?),
                        None => None,
                    };
                    (Some(c), t)
                }
                None => (None, None),
            };
            let raw = match (&req.url, &req.repo, &conn) {
                (Some(u), _, _) => u.clone(),
                (None, Some(r), Some(c)) => format!("{}/{}.git", c.base_url, r.trim_matches('/')),
                (None, Some(r), None) if req.source == "github" => {
                    format!("https://github.com/{}.git", r.trim_matches('/'))
                }
                (None, Some(r), None) if req.source == "gitlab" => {
                    format!("https://gitlab.com/{}.git", r.trim_matches('/'))
                }
                _ => {
                    return Err(ApiError::bad(
                        "give a clone URL, or owner/name with a connection",
                    ))
                }
            };
            let u = validate_git_url(&raw, s.cfg.allow_private_network)
                .map_err(|e| ApiError::bad(e.to_string()))?;
            if let Some(c) = &conn {
                let host = url::Url::parse(&c.base_url)
                    .ok()
                    .and_then(|b| b.host_str().map(String::from));
                if host.as_deref() != u.host_str() {
                    return Err(ApiError::bad("the URL's host does not match the connection; the token is only sent to its own host"));
                }
            }
            let dir = repo_dir(&s, repo_id);
            (
                req.source.clone(),
                Some(u.to_string()),
                dir,
                Source::Git {
                    url: u,
                    branch: req.branch.clone(),
                    token,
                },
            )
        }
        "local" => {
            let root = s.cfg.local_import_root.clone().ok_or_else(|| {
                ApiError::forbidden(
                    "local import is disabled; set ORYN_LOCAL_IMPORT_ROOT on the server",
                )
            })?;
            let rel = req
                .path
                .clone()
                .ok_or_else(|| ApiError::bad("path is required for local import"))?;
            let root = root
                .canonicalize()
                .map_err(|_| ApiError::bad("ORYN_LOCAL_IMPORT_ROOT does not exist"))?;
            let candidate = if std::path::Path::new(&rel).is_absolute() {
                std::path::PathBuf::from(&rel)
            } else {
                root.join(&rel)
            };
            let path = candidate
                .canonicalize()
                .map_err(|_| ApiError::bad("path does not exist"))?;
            if !path.starts_with(&root) || !path.is_dir() {
                return Err(ApiError::forbidden(
                    "path must be a directory inside ORYN_LOCAL_IMPORT_ROOT",
                ));
            }
            ("local".to_string(), None, path, Source::Local)
        }
        _ => {
            return Err(ApiError::bad(
                "source must be github, gitlab, git or local; upload ZIPs to /repositories/upload",
            ))
        }
    };
    let repo = oryn_db::projects::create_repository(
        &s.pool,
        repo_id,
        id,
        &source,
        url.as_deref(),
        req.branch.as_deref(),
        &local_path.to_string_lossy(),
        req.connection_id,
    )
    .await?;
    audit(
        &s.pool,
        Some(p.ws_id()),
        &p.actor(),
        "repository.imported",
        Some("repository"),
        Some(repo_id),
        json!({"source": source, "url": url}),
    )
    .await;
    spawn_import(s.clone(), id, repo_id, local_path, src);
    Ok((StatusCode::ACCEPTED, Json(repo)))
}

#[derive(Deserialize, IntoParams)]
pub struct UploadQuery {
    /// Original file name, for display.
    pub name: Option<String>,
}

/// Upload a ZIP archive (request body is the raw archive). Validated before extraction:
/// no path traversal, absolute paths or symlinks; size and compression-ratio limits.
#[utoipa::path(post, path = "/api/v1/projects/{id}/repositories/upload", tag = "repositories", params(UploadQuery),
    request_body(content = Vec<u8>, content_type = "application/zip"), responses((status = 202, body = Repository)))]
async fn upload_zip(
    State(s): State<AppState>,
    auth: Authed,
    Path(id): Path<Uuid>,
    Query(q): Query<UploadQuery>,
    body: Bytes,
) -> ApiResult<(StatusCode, Json<Repository>)> {
    let p = project_ctx(&s, &auth, id).await?;
    p.require(Permission::ProjectWrite)?;
    if body.len() < 22 || &body[..2] != b"PK" {
        return Err(ApiError::bad("the upload is not a ZIP archive"));
    }
    let repo_id = oryn_db::new_id();
    let dir = repo_dir(&s, repo_id);
    let name = q.name.map(|n| oryn_artifacts::sanitize_name(&n));
    let repo = oryn_db::projects::create_repository(
        &s.pool,
        repo_id,
        id,
        "zip",
        name.as_deref(),
        None,
        &dir.to_string_lossy(),
        None,
    )
    .await?;
    audit(
        &s.pool,
        Some(p.ws_id()),
        &p.actor(),
        "repository.imported",
        Some("repository"),
        Some(repo_id),
        json!({"source": "zip", "bytes": body.len()}),
    )
    .await;
    spawn_import(s.clone(), id, repo_id, dir, Source::Zip(body.to_vec()));
    Ok((StatusCode::ACCEPTED, Json(repo)))
}

/// Re-run analysis. Git sources are re-cloned to pick up new commits; the change set
/// compares file hashes with the previous run.
#[utoipa::path(post, path = "/api/v1/projects/{id}/repositories/{repo_id}/analyze", tag = "repositories", responses((status = 202, body = Ok)))]
async fn reanalyze(
    State(s): State<AppState>,
    auth: Authed,
    Path((id, repo_id)): Path<(Uuid, Uuid)>,
) -> ApiResult<(StatusCode, Json<Ok>)> {
    let p = project_ctx(&s, &auth, id).await?;
    p.require(Permission::ProjectWrite)?;
    let repo = oryn_db::projects::get_repository(&s.pool, id, repo_id).await?;
    if matches!(repo.status.as_str(), "importing" | "analyzing") {
        return Err(ApiError::Conflict(
            "this repository is already being processed".into(),
        ));
    }
    let src = match repo.source.as_str() {
        "github" | "gitlab" | "git" => {
            let u = validate_git_url(
                repo.url.as_deref().unwrap_or(""),
                s.cfg.allow_private_network,
            )
            .map_err(|e| ApiError::bad(e.to_string()))?;
            let token = match repo.connection_id {
                Some(cid) => match oryn_db::projects::get_connection(&s.pool, p.ws_id(), cid)
                    .await?
                    .token_encrypted
                {
                    Some(t) => Some(s.open(&t)?),
                    None => None,
                },
                None => None,
            };
            Source::Git {
                url: u,
                branch: repo.default_branch.clone(),
                token,
            }
        }
        _ => Source::Local,
    };
    spawn_import(s.clone(), id, repo_id, repo.local_path.clone().into(), src);
    Ok((StatusCode::ACCEPTED, ok()))
}

#[utoipa::path(delete, path = "/api/v1/projects/{id}/repositories/{repo_id}", tag = "repositories", responses((status = 200, body = Ok)))]
async fn delete_repository(
    State(s): State<AppState>,
    auth: Authed,
    Path((id, repo_id)): Path<(Uuid, Uuid)>,
) -> ApiResult<Json<Ok>> {
    let p = project_ctx(&s, &auth, id).await?;
    p.require(Permission::ProjectWrite)?;
    let repo = oryn_db::projects::get_repository(&s.pool, id, repo_id).await?;
    oryn_db::projects::delete_repository(&s.pool, id, repo_id).await?;
    // Only remove directories ORYN created; local imports are never touched.
    if repo.source != "local" {
        let dir = repo_dir(&s, repo_id);
        let _ = tokio::fs::remove_dir_all(dir).await;
    }
    audit(
        &s.pool,
        Some(p.ws_id()),
        &p.actor(),
        "repository.deleted",
        Some("repository"),
        Some(repo_id),
        json!({}),
    )
    .await;
    Ok(ok())
}

#[derive(Serialize, ToSchema)]
pub struct ContextView {
    pub context: Option<ApplicationContext>,
    pub modules: Vec<ApplicationModule>,
}

#[utoipa::path(get, path = "/api/v1/projects/{id}/context", tag = "repositories", responses((status = 200, body = ContextView)))]
async fn get_context(
    State(s): State<AppState>,
    auth: Authed,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<ContextView>> {
    project_ctx(&s, &auth, id)
        .await?
        .require(Permission::ProjectRead)?;
    Ok(Json(ContextView {
        context: oryn_db::projects::get_context(&s.pool, id).await?,
        modules: oryn_db::projects::list_modules(&s.pool, id).await?,
    }))
}

#[derive(Deserialize, IntoParams)]
pub struct SearchQuery {
    pub q: String,
    pub limit: Option<i64>,
}

/// Full-text search over the analysed repository content (secrets redacted at index time).
#[utoipa::path(get, path = "/api/v1/projects/{id}/context/search", tag = "repositories", params(SearchQuery), responses((status = 200, body = Vec<ContextChunk>)))]
async fn search_context(
    State(s): State<AppState>,
    auth: Authed,
    Path(id): Path<Uuid>,
    Query(q): Query<SearchQuery>,
) -> ApiResult<Json<Vec<ContextChunk>>> {
    project_ctx(&s, &auth, id)
        .await?
        .require(Permission::ProjectRead)?;
    if q.q.trim().is_empty() {
        return Ok(Json(vec![]));
    }
    Ok(Json(
        oryn_db::projects::search_chunks(
            &s.pool,
            id,
            q.q.trim(),
            q.limit.unwrap_or(20).clamp(1, 50),
        )
        .await?,
    ))
}

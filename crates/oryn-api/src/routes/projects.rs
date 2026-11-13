use crate::auth::{project_ctx, ws_ctx, Authed};
use crate::error::{ApiError, ApiResult};
use crate::routes::{ok, Ok};
use crate::state::AppState;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use oryn_db::governance::audit;
use oryn_db::projects::{Environment, Project, ProjectPermission, ProjectSummary, TestData};
use oryn_domain::{Permission, Role};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use utoipa::ToSchema;
use utoipa_axum::{router::OpenApiRouter, routes};
use uuid::Uuid;

pub fn routes() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(list_projects, create_project))
        .routes(routes!(get_project, update_project, delete_project))
        .routes(routes!(list_permissions, add_permission))
        .routes(routes!(delete_permission))
        .routes(routes!(list_environments, create_environment))
        .routes(routes!(update_environment, delete_environment))
        .routes(routes!(list_test_data, put_test_data))
        .routes(routes!(delete_test_data))
}

const COLORS: &[&str] = &["mon", "tue", "wed", "thu", "fri", "sat", "sun"];

#[utoipa::path(get, path = "/api/v1/workspaces/{ws}/projects", tag = "projects", responses((status = 200, body = Vec<ProjectSummary>)))]
async fn list_projects(
    State(s): State<AppState>,
    auth: Authed,
    Path(ws): Path<Uuid>,
) -> ApiResult<Json<Vec<ProjectSummary>>> {
    let c = ws_ctx(&s, &auth, ws).await?;
    let see_all = c.role.rank() >= Role::Admin.rank();
    Ok(Json(
        oryn_db::projects::list_projects(&s.pool, ws, c.user_id(), see_all).await?,
    ))
}

#[derive(Deserialize, ToSchema)]
pub struct CreateProject {
    pub name: String,
    pub description: Option<String>,
    /// `workspace` (all members) or `private` (explicit grants only).
    pub visibility: Option<String>,
    /// Identity tint: one of mon, tue, wed, thu, fri, sat, sun.
    pub color: Option<String>,
}

fn check_visibility(v: &str) -> ApiResult<()> {
    if v == "workspace" || v == "private" {
        Ok(())
    } else {
        Err(ApiError::bad("visibility must be workspace or private"))
    }
}

#[utoipa::path(post, path = "/api/v1/workspaces/{ws}/projects", tag = "projects", responses((status = 201, body = Project)))]
async fn create_project(
    State(s): State<AppState>,
    auth: Authed,
    Path(ws): Path<Uuid>,
    Json(req): Json<CreateProject>,
) -> ApiResult<(StatusCode, Json<Project>)> {
    let c = ws_ctx(&s, &auth, ws).await?;
    c.require(Permission::ProjectWrite)?;
    let name = req.name.trim();
    if name.is_empty() || name.len() > 100 {
        return Err(ApiError::bad("name must be 1 to 100 characters"));
    }
    let vis = req.visibility.unwrap_or_else(|| "workspace".into());
    check_visibility(&vis)?;
    let existing = oryn_db::projects::list_projects(&s.pool, ws, c.user_id(), true)
        .await?
        .len();
    let color = req
        .color
        .filter(|c| COLORS.contains(&c.as_str()))
        .unwrap_or_else(|| COLORS[existing % COLORS.len()].to_string());
    let p = oryn_db::projects::create_project(
        &s.pool,
        ws,
        name,
        req.description.as_deref(),
        &vis,
        &color,
        c.user_id(),
    )
    .await?;
    if vis == "private" {
        // The creator keeps access to their private project.
        oryn_db::projects::add_project_permission(
            &s.pool,
            p.id,
            Some(c.user_id()),
            None,
            c.role.as_str(),
        )
        .await?;
    }
    audit(
        &s.pool,
        Some(ws),
        &c.actor(),
        "project.created",
        Some("project"),
        Some(p.id),
        json!({"name": name, "visibility": vis}),
    )
    .await;
    Ok((StatusCode::CREATED, Json(p)))
}

#[derive(Serialize, ToSchema)]
pub struct ProjectDetail {
    #[serde(flatten)]
    pub project: Project,
    pub role: String,
    pub permissions: Vec<String>,
}

#[utoipa::path(get, path = "/api/v1/projects/{id}", tag = "projects", responses((status = 200, body = ProjectDetail)))]
async fn get_project(
    State(s): State<AppState>,
    auth: Authed,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<ProjectDetail>> {
    let p = project_ctx(&s, &auth, id).await?;
    p.require(Permission::ProjectRead)?;
    Ok(Json(ProjectDetail {
        role: p.role.to_string(),
        permissions: p.role.permissions().iter().map(|x| x.to_string()).collect(),
        project: p.project,
    }))
}

#[derive(Deserialize, ToSchema)]
pub struct UpdateProject {
    pub name: Option<String>,
    pub description: Option<String>,
    pub visibility: Option<String>,
    pub color: Option<String>,
}

#[utoipa::path(patch, path = "/api/v1/projects/{id}", tag = "projects", responses((status = 200, body = Project)))]
async fn update_project(
    State(s): State<AppState>,
    auth: Authed,
    Path(id): Path<Uuid>,
    Json(req): Json<UpdateProject>,
) -> ApiResult<Json<Project>> {
    let p = project_ctx(&s, &auth, id).await?;
    p.require(Permission::ProjectWrite)?;
    if let Some(v) = &req.visibility {
        check_visibility(v)?;
    }
    if let Some(c) = &req.color {
        if !COLORS.contains(&c.as_str()) {
            return Err(ApiError::bad("unknown colour"));
        }
    }
    let out = oryn_db::projects::update_project(
        &s.pool,
        id,
        req.name.as_deref().map(str::trim),
        req.description.as_deref(),
        req.visibility.as_deref(),
        req.color.as_deref(),
    )
    .await?;
    audit(
        &s.pool,
        Some(p.ws_id()),
        &p.actor(),
        "project.updated",
        Some("project"),
        Some(id),
        json!({"name": req.name, "visibility": req.visibility}),
    )
    .await;
    Ok(Json(out))
}

#[utoipa::path(delete, path = "/api/v1/projects/{id}", tag = "projects", responses((status = 200, body = Ok)))]
async fn delete_project(
    State(s): State<AppState>,
    auth: Authed,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Ok>> {
    let p = project_ctx(&s, &auth, id).await?;
    p.require(Permission::ProjectDelete)?;
    oryn_db::projects::soft_delete_project(&s.pool, id).await?;
    audit(
        &s.pool,
        Some(p.ws_id()),
        &p.actor(),
        "project.deleted",
        Some("project"),
        Some(id),
        json!({"name": p.project.name}),
    )
    .await;
    Ok(ok())
}

#[utoipa::path(get, path = "/api/v1/projects/{id}/permissions", tag = "projects", responses((status = 200, body = Vec<ProjectPermission>)))]
async fn list_permissions(
    State(s): State<AppState>,
    auth: Authed,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Vec<ProjectPermission>>> {
    project_ctx(&s, &auth, id)
        .await?
        .require(Permission::ProjectRead)?;
    Ok(Json(
        oryn_db::projects::list_project_permissions(&s.pool, id).await?,
    ))
}

#[derive(Deserialize, ToSchema)]
pub struct AddPermission {
    pub user_id: Option<Uuid>,
    pub team_id: Option<Uuid>,
    pub role: Role,
}

#[utoipa::path(post, path = "/api/v1/projects/{id}/permissions", tag = "projects", responses((status = 201, body = ProjectPermission)))]
async fn add_permission(
    State(s): State<AppState>,
    auth: Authed,
    Path(id): Path<Uuid>,
    Json(req): Json<AddPermission>,
) -> ApiResult<(StatusCode, Json<ProjectPermission>)> {
    let p = project_ctx(&s, &auth, id).await?;
    p.require(Permission::MembersManage)?;
    if req.user_id.is_some() == req.team_id.is_some() {
        return Err(ApiError::bad("give exactly one of user_id or team_id"));
    }
    if !p.role.can_assign(req.role) {
        return Err(ApiError::forbidden(
            "you cannot grant a role above your own",
        ));
    }
    if let Some(u) = req.user_id {
        if oryn_db::identity::membership_role(&s.pool, p.ws_id(), u)
            .await?
            .is_none()
        {
            return Err(ApiError::not_found("workspace member"));
        }
    }
    let out = oryn_db::projects::add_project_permission(
        &s.pool,
        id,
        req.user_id,
        req.team_id,
        req.role.as_str(),
    )
    .await?;
    audit(
        &s.pool,
        Some(p.ws_id()),
        &p.actor(),
        "project.permission_added",
        Some("project"),
        Some(id),
        json!({"user_id": req.user_id, "team_id": req.team_id, "role": req.role}),
    )
    .await;
    Ok((StatusCode::CREATED, Json(out)))
}

#[utoipa::path(delete, path = "/api/v1/projects/{id}/permissions/{perm_id}", tag = "projects", responses((status = 200, body = Ok)))]
async fn delete_permission(
    State(s): State<AppState>,
    auth: Authed,
    Path((id, perm_id)): Path<(Uuid, Uuid)>,
) -> ApiResult<Json<Ok>> {
    let p = project_ctx(&s, &auth, id).await?;
    p.require(Permission::MembersManage)?;
    oryn_db::projects::delete_project_permission(&s.pool, id, perm_id).await?;
    audit(
        &s.pool,
        Some(p.ws_id()),
        &p.actor(),
        "project.permission_removed",
        Some("project"),
        Some(id),
        json!({"permission_id": perm_id}),
    )
    .await;
    Ok(ok())
}

// ── environments ──

#[utoipa::path(get, path = "/api/v1/projects/{id}/environments", tag = "projects", responses((status = 200, body = Vec<Environment>)))]
async fn list_environments(
    State(s): State<AppState>,
    auth: Authed,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Vec<Environment>>> {
    project_ctx(&s, &auth, id)
        .await?
        .require(Permission::ProjectRead)?;
    Ok(Json(
        oryn_db::projects::list_environments(&s.pool, id)
            .await?
            .into_iter()
            .map(Environment::from)
            .collect(),
    ))
}

#[derive(Deserialize, ToSchema)]
pub struct EnvironmentReq {
    pub name: String,
    pub base_url: Option<String>,
    pub api_base_url: Option<String>,
    /// Extra hosts steps may reach (base URL hosts are always allowed). `*.example.com` wildcards allowed.
    #[serde(default)]
    pub allowed_hosts: Vec<String>,
    /// Plain variables, available as `{{name}}` in steps.
    #[serde(default)]
    #[schema(value_type = Object)]
    pub variables: Map<String, Value>,
    /// Secrets: a string sets the value, `null` keeps the stored value, omitting a name deletes it.
    /// Values are encrypted at rest and never returned.
    #[serde(default)]
    #[schema(value_type = Object)]
    pub secrets: Option<Map<String, Value>>,
    #[serde(default)]
    pub is_default: bool,
}

fn check_url(u: &Option<String>, field: &str) -> ApiResult<()> {
    if let Some(u) = u.as_deref().filter(|x| !x.is_empty()) {
        let parsed =
            url::Url::parse(u).map_err(|_| ApiError::bad(format!("{field} is not a valid URL")))?;
        if parsed.scheme() != "http" && parsed.scheme() != "https" {
            return Err(ApiError::bad(format!("{field} must be http or https")));
        }
    }
    Ok(())
}

fn valid_name(n: &str) -> bool {
    !n.is_empty()
        && n.len() <= 64
        && n.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == '.')
}

async fn save_env(
    s: &AppState,
    auth: &Authed,
    id: Uuid,
    env_id: Option<Uuid>,
    req: EnvironmentReq,
) -> ApiResult<Environment> {
    let p = project_ctx(s, auth, id).await?;
    p.require(Permission::ProjectWrite)?;
    check_url(&req.base_url, "base_url")?;
    check_url(&req.api_base_url, "api_base_url")?;
    if req.name.trim().is_empty() {
        return Err(ApiError::bad("environment name is required"));
    }
    for k in req.variables.keys() {
        if !valid_name(k) {
            return Err(ApiError::bad(format!("invalid variable name: {k}")));
        }
    }
    let existing = match env_id {
        Some(e) => Some(oryn_db::projects::get_environment(&s.pool, id, e).await?),
        None => None,
    };
    let old_secrets = existing
        .as_ref()
        .map(|e| e.secrets.clone())
        .unwrap_or_else(|| json!({}));
    let secrets = match req.secrets {
        None => old_secrets.clone(),
        Some(m) => {
            p.require(Permission::SecretsManage)?;
            let mut out = Map::new();
            for (k, v) in m {
                if !valid_name(&k) {
                    return Err(ApiError::bad(format!("invalid secret name: {k}")));
                }
                match v {
                    Value::Null => {
                        if let Some(old) = old_secrets.get(&k) {
                            out.insert(k, old.clone());
                        }
                    }
                    Value::String(sv) => {
                        out.insert(k, Value::String(s.seal(&sv)));
                    }
                    _ => return Err(ApiError::bad("secret values must be strings")),
                }
            }
            Value::Object(out)
        }
    };
    let hosts: Vec<String> = req
        .allowed_hosts
        .into_iter()
        .map(|h| h.trim().to_ascii_lowercase())
        .filter(|h| !h.is_empty())
        .collect();
    let row = oryn_db::projects::upsert_environment(
        &s.pool,
        id,
        env_id,
        req.name.trim(),
        req.base_url.as_deref().filter(|x| !x.is_empty()),
        req.api_base_url.as_deref().filter(|x| !x.is_empty()),
        &hosts,
        &Value::Object(req.variables),
        &secrets,
        req.is_default,
    )
    .await?;
    let env = Environment::from(row);
    audit(
        &s.pool,
        Some(p.ws_id()),
        &p.actor(),
        if env_id.is_some() {
            "environment.updated"
        } else {
            "environment.created"
        },
        Some("environment"),
        Some(env.id),
        json!({"name": env.name, "secret_names": env.secret_names}),
    )
    .await;
    Ok(env)
}

#[utoipa::path(post, path = "/api/v1/projects/{id}/environments", tag = "projects", responses((status = 201, body = Environment)))]
async fn create_environment(
    State(s): State<AppState>,
    auth: Authed,
    Path(id): Path<Uuid>,
    Json(req): Json<EnvironmentReq>,
) -> ApiResult<(StatusCode, Json<Environment>)> {
    Ok((
        StatusCode::CREATED,
        Json(save_env(&s, &auth, id, None, req).await?),
    ))
}

#[utoipa::path(put, path = "/api/v1/projects/{id}/environments/{env_id}", tag = "projects", responses((status = 200, body = Environment)))]
async fn update_environment(
    State(s): State<AppState>,
    auth: Authed,
    Path((id, env_id)): Path<(Uuid, Uuid)>,
    Json(req): Json<EnvironmentReq>,
) -> ApiResult<Json<Environment>> {
    Ok(Json(save_env(&s, &auth, id, Some(env_id), req).await?))
}

#[utoipa::path(delete, path = "/api/v1/projects/{id}/environments/{env_id}", tag = "projects", responses((status = 200, body = Ok)))]
async fn delete_environment(
    State(s): State<AppState>,
    auth: Authed,
    Path((id, env_id)): Path<(Uuid, Uuid)>,
) -> ApiResult<Json<Ok>> {
    let p = project_ctx(&s, &auth, id).await?;
    p.require(Permission::ProjectWrite)?;
    oryn_db::projects::delete_environment(&s.pool, id, env_id).await?;
    audit(
        &s.pool,
        Some(p.ws_id()),
        &p.actor(),
        "environment.deleted",
        Some("environment"),
        Some(env_id),
        json!({}),
    )
    .await;
    Ok(ok())
}

// ── test data ──

#[utoipa::path(get, path = "/api/v1/projects/{id}/test-data", tag = "projects", responses((status = 200, body = Vec<TestData>)))]
async fn list_test_data(
    State(s): State<AppState>,
    auth: Authed,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Vec<TestData>>> {
    project_ctx(&s, &auth, id)
        .await?
        .require(Permission::ProjectRead)?;
    Ok(Json(oryn_db::projects::list_test_data(&s.pool, id).await?))
}

#[derive(Deserialize, ToSchema)]
pub struct TestDataReq {
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[schema(value_type = Object)]
    pub data: Value,
}

#[utoipa::path(post, path = "/api/v1/projects/{id}/test-data", tag = "projects", responses((status = 200, body = TestData)))]
async fn put_test_data(
    State(s): State<AppState>,
    auth: Authed,
    Path(id): Path<Uuid>,
    Json(req): Json<TestDataReq>,
) -> ApiResult<Json<TestData>> {
    project_ctx(&s, &auth, id)
        .await?
        .require(Permission::TestWrite)?;
    if !valid_name(req.name.trim()) {
        return Err(ApiError::bad("data set names use letters, digits, - _ ."));
    }
    if serde_json::to_vec(&req.data).map(|v| v.len()).unwrap_or(0) > 1_000_000 {
        return Err(ApiError::bad("test data is limited to 1 MB"));
    }
    Ok(Json(
        oryn_db::projects::upsert_test_data(
            &s.pool,
            id,
            req.name.trim(),
            &req.description,
            &req.data,
        )
        .await?,
    ))
}

#[utoipa::path(delete, path = "/api/v1/projects/{id}/test-data/{data_id}", tag = "projects", responses((status = 200, body = Ok)))]
async fn delete_test_data(
    State(s): State<AppState>,
    auth: Authed,
    Path((id, data_id)): Path<(Uuid, Uuid)>,
) -> ApiResult<Json<Ok>> {
    project_ctx(&s, &auth, id)
        .await?
        .require(Permission::TestWrite)?;
    oryn_db::projects::delete_test_data(&s.pool, id, data_id).await?;
    Ok(ok())
}

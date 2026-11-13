use crate::auth::{ws_ctx, Authed};
use crate::error::{ApiError, ApiResult};
use crate::routes::{ok, Ok, Paged};
use crate::state::AppState;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::Json;
use chrono::{Duration, Utc};
use oryn_db::governance::{audit, AuditEvent};
use oryn_db::identity::{ApiToken, Member, Team, Workspace, WorkspaceWithRole};
use oryn_db::Page;
use oryn_domain::{Permission, Role};
use serde::{Deserialize, Serialize};
use serde_json::json;
use utoipa::{IntoParams, ToSchema};
use utoipa_axum::{router::OpenApiRouter, routes};
use uuid::Uuid;

pub fn routes() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(list_workspaces, create_workspace))
        .routes(routes!(get_workspace, update_workspace))
        .routes(routes!(list_members, add_member))
        .routes(routes!(set_member_role, remove_member))
        .routes(routes!(list_teams, create_team))
        .routes(routes!(delete_team))
        .routes(routes!(list_team_members))
        .routes(routes!(add_team_member, remove_team_member))
        .routes(routes!(list_tokens, create_token))
        .routes(routes!(revoke_token))
        .routes(routes!(list_audit))
        .routes(routes!(roles))
}

#[utoipa::path(get, path = "/api/v1/workspaces", tag = "workspaces", responses((status = 200, body = Vec<WorkspaceWithRole>)))]
async fn list_workspaces(
    State(s): State<AppState>,
    auth: Authed,
) -> ApiResult<Json<Vec<WorkspaceWithRole>>> {
    let mut all = oryn_db::identity::list_user_workspaces(&s.pool, auth.user.id).await?;
    if let crate::auth::Via::Token { workspace_id, .. } = auth.via {
        all.retain(|w| w.id == workspace_id);
    }
    Ok(Json(all))
}

#[derive(Deserialize, ToSchema)]
pub struct CreateWorkspace {
    pub name: String,
}

#[utoipa::path(post, path = "/api/v1/workspaces", tag = "workspaces", responses((status = 201, body = Workspace)))]
async fn create_workspace(
    State(s): State<AppState>,
    auth: Authed,
    Json(req): Json<CreateWorkspace>,
) -> ApiResult<(StatusCode, Json<Workspace>)> {
    if matches!(auth.via, crate::auth::Via::Token { .. }) {
        return Err(ApiError::forbidden("API tokens cannot create workspaces"));
    }
    let name = req.name.trim();
    if name.is_empty() || name.len() > 100 {
        return Err(ApiError::bad("name must be 1 to 100 characters"));
    }
    let ws = oryn_db::identity::create_workspace(&s.pool, name, auth.user.id).await?;
    audit(
        &s.pool,
        Some(ws.id),
        &auth.actor(),
        "workspace.created",
        Some("workspace"),
        Some(ws.id),
        json!({"name": name}),
    )
    .await;
    Ok((StatusCode::CREATED, Json(ws)))
}

#[derive(Serialize, ToSchema)]
pub struct WorkspaceDetail {
    #[serde(flatten)]
    pub workspace: Workspace,
    pub role: String,
    pub permissions: Vec<String>,
}

#[utoipa::path(get, path = "/api/v1/workspaces/{ws}", tag = "workspaces", responses((status = 200, body = WorkspaceDetail)))]
async fn get_workspace(
    State(s): State<AppState>,
    auth: Authed,
    Path(ws): Path<Uuid>,
) -> ApiResult<Json<WorkspaceDetail>> {
    let c = ws_ctx(&s, &auth, ws).await?;
    let workspace = oryn_db::identity::get_workspace(&s.pool, ws).await?;
    Ok(Json(WorkspaceDetail {
        workspace,
        role: c.role.to_string(),
        permissions: c.role.permissions().iter().map(|p| p.to_string()).collect(),
    }))
}

#[derive(Deserialize, ToSchema)]
pub struct UpdateWorkspace {
    pub name: Option<String>,
    /// When true, only approved scenarios can run.
    pub require_approval: Option<bool>,
}

#[utoipa::path(patch, path = "/api/v1/workspaces/{ws}", tag = "workspaces", responses((status = 200, body = Workspace)))]
async fn update_workspace(
    State(s): State<AppState>,
    auth: Authed,
    Path(ws): Path<Uuid>,
    Json(req): Json<UpdateWorkspace>,
) -> ApiResult<Json<Workspace>> {
    let c = ws_ctx(&s, &auth, ws).await?;
    c.require(Permission::WorkspaceManage)?;
    let w = oryn_db::identity::update_workspace(
        &s.pool,
        ws,
        req.name.as_deref().map(str::trim),
        req.require_approval,
    )
    .await?;
    audit(
        &s.pool,
        Some(ws),
        &c.actor(),
        "workspace.updated",
        Some("workspace"),
        Some(ws),
        json!({"name": req.name, "require_approval": req.require_approval}),
    )
    .await;
    Ok(Json(w))
}

#[utoipa::path(get, path = "/api/v1/workspaces/{ws}/members", tag = "workspaces", responses((status = 200, body = Vec<Member>)))]
async fn list_members(
    State(s): State<AppState>,
    auth: Authed,
    Path(ws): Path<Uuid>,
) -> ApiResult<Json<Vec<Member>>> {
    ws_ctx(&s, &auth, ws)
        .await?
        .require(Permission::WorkspaceRead)?;
    Ok(Json(oryn_db::identity::list_members(&s.pool, ws).await?))
}

#[derive(Deserialize, ToSchema)]
pub struct AddMember {
    pub email: String,
    pub role: Role,
}

#[utoipa::path(post, path = "/api/v1/workspaces/{ws}/members", tag = "workspaces", responses((status = 201, body = Ok), (status = 404, description = "No account with that email")))]
async fn add_member(
    State(s): State<AppState>,
    auth: Authed,
    Path(ws): Path<Uuid>,
    Json(req): Json<AddMember>,
) -> ApiResult<(StatusCode, Json<Ok>)> {
    let c = ws_ctx(&s, &auth, ws).await?;
    if !c.role.can_assign(req.role) {
        return Err(ApiError::forbidden(format!(
            "a {} cannot grant the {} role",
            c.role, req.role
        )));
    }
    let user =
        oryn_db::identity::find_user_by_email(&s.pool, &req.email.trim().to_ascii_lowercase())
            .await?
            .ok_or_else(|| {
                ApiError::not_found("an account with that email (they must register first)")
            })?;
    oryn_db::identity::add_member(&s.pool, ws, user.id, req.role.as_str()).await?;
    audit(
        &s.pool,
        Some(ws),
        &c.actor(),
        "member.added",
        Some("user"),
        Some(user.id),
        json!({"role": req.role}),
    )
    .await;
    Ok((StatusCode::CREATED, ok()))
}

#[derive(Deserialize, ToSchema)]
pub struct SetRole {
    pub role: Role,
}

async fn guard_member_change(
    s: &AppState,
    c: &crate::auth::WsCtx,
    ws: Uuid,
    target: Uuid,
    new_role: Option<Role>,
) -> ApiResult<()> {
    let current: Role = oryn_db::identity::membership_role(&s.pool, ws, target)
        .await?
        .ok_or_else(|| ApiError::not_found("member"))?
        .parse()
        .map_err(|_| ApiError::not_found("member"))?;
    if !c.role.can_assign(current) || new_role.map(|r| !c.role.can_assign(r)).unwrap_or(false) {
        return Err(ApiError::forbidden(
            "you cannot change a member whose role is above yours",
        ));
    }
    if current == Role::Owner
        && new_role != Some(Role::Owner)
        && oryn_db::identity::count_owners(&s.pool, ws).await? <= 1
    {
        return Err(ApiError::Conflict(
            "a workspace must keep at least one owner".into(),
        ));
    }
    Ok(())
}

#[utoipa::path(patch, path = "/api/v1/workspaces/{ws}/members/{user_id}", tag = "workspaces", responses((status = 200, body = Ok)))]
async fn set_member_role(
    State(s): State<AppState>,
    auth: Authed,
    Path((ws, user_id)): Path<(Uuid, Uuid)>,
    Json(req): Json<SetRole>,
) -> ApiResult<Json<Ok>> {
    let c = ws_ctx(&s, &auth, ws).await?;
    c.require(Permission::MembersManage)?;
    guard_member_change(&s, &c, ws, user_id, Some(req.role)).await?;
    oryn_db::identity::set_member_role(&s.pool, ws, user_id, req.role.as_str()).await?;
    audit(
        &s.pool,
        Some(ws),
        &c.actor(),
        "member.role_changed",
        Some("user"),
        Some(user_id),
        json!({"role": req.role}),
    )
    .await;
    Ok(ok())
}

#[utoipa::path(delete, path = "/api/v1/workspaces/{ws}/members/{user_id}", tag = "workspaces", responses((status = 200, body = Ok)))]
async fn remove_member(
    State(s): State<AppState>,
    auth: Authed,
    Path((ws, user_id)): Path<(Uuid, Uuid)>,
) -> ApiResult<Json<Ok>> {
    let c = ws_ctx(&s, &auth, ws).await?;
    if user_id != c.user_id() {
        c.require(Permission::MembersManage)?;
    }
    guard_member_change(&s, &c, ws, user_id, None)
        .await
        .or_else(|e| {
            if user_id == c.user_id() && !matches!(e, ApiError::Conflict(_)) {
                Result::Ok(())
            } else {
                Err(e)
            }
        })?;
    oryn_db::identity::remove_member(&s.pool, ws, user_id).await?;
    audit(
        &s.pool,
        Some(ws),
        &c.actor(),
        "member.removed",
        Some("user"),
        Some(user_id),
        json!({}),
    )
    .await;
    Ok(ok())
}

#[utoipa::path(get, path = "/api/v1/workspaces/{ws}/teams", tag = "workspaces", responses((status = 200, body = Vec<Team>)))]
async fn list_teams(
    State(s): State<AppState>,
    auth: Authed,
    Path(ws): Path<Uuid>,
) -> ApiResult<Json<Vec<Team>>> {
    ws_ctx(&s, &auth, ws)
        .await?
        .require(Permission::WorkspaceRead)?;
    Ok(Json(oryn_db::identity::list_teams(&s.pool, ws).await?))
}

#[derive(Deserialize, ToSchema)]
pub struct CreateTeam {
    pub name: String,
    pub description: Option<String>,
}

#[utoipa::path(post, path = "/api/v1/workspaces/{ws}/teams", tag = "workspaces", responses((status = 201, body = Team)))]
async fn create_team(
    State(s): State<AppState>,
    auth: Authed,
    Path(ws): Path<Uuid>,
    Json(req): Json<CreateTeam>,
) -> ApiResult<(StatusCode, Json<Team>)> {
    let c = ws_ctx(&s, &auth, ws).await?;
    c.require(Permission::MembersManage)?;
    if req.name.trim().is_empty() {
        return Err(ApiError::bad("team name is required"));
    }
    let t =
        oryn_db::identity::create_team(&s.pool, ws, req.name.trim(), req.description.as_deref())
            .await?;
    audit(
        &s.pool,
        Some(ws),
        &c.actor(),
        "team.created",
        Some("team"),
        Some(t.id),
        json!({"name": t.name}),
    )
    .await;
    Ok((StatusCode::CREATED, Json(t)))
}

#[utoipa::path(delete, path = "/api/v1/workspaces/{ws}/teams/{team_id}", tag = "workspaces", responses((status = 200, body = Ok)))]
async fn delete_team(
    State(s): State<AppState>,
    auth: Authed,
    Path((ws, team_id)): Path<(Uuid, Uuid)>,
) -> ApiResult<Json<Ok>> {
    let c = ws_ctx(&s, &auth, ws).await?;
    c.require(Permission::MembersManage)?;
    oryn_db::identity::delete_team(&s.pool, ws, team_id).await?;
    audit(
        &s.pool,
        Some(ws),
        &c.actor(),
        "team.deleted",
        Some("team"),
        Some(team_id),
        json!({}),
    )
    .await;
    Ok(ok())
}

#[utoipa::path(get, path = "/api/v1/workspaces/{ws}/teams/{team_id}/members", tag = "workspaces", responses((status = 200, body = Vec<Member>)))]
async fn list_team_members(
    State(s): State<AppState>,
    auth: Authed,
    Path((ws, team_id)): Path<(Uuid, Uuid)>,
) -> ApiResult<Json<Vec<Member>>> {
    ws_ctx(&s, &auth, ws)
        .await?
        .require(Permission::WorkspaceRead)?;
    Ok(Json(
        oryn_db::identity::list_team_members(&s.pool, ws, team_id).await?,
    ))
}

#[utoipa::path(put, path = "/api/v1/workspaces/{ws}/teams/{team_id}/members/{user_id}", tag = "workspaces", responses((status = 200, body = Ok)))]
async fn add_team_member(
    State(s): State<AppState>,
    auth: Authed,
    Path((ws, team_id, user_id)): Path<(Uuid, Uuid, Uuid)>,
) -> ApiResult<Json<Ok>> {
    let c = ws_ctx(&s, &auth, ws).await?;
    c.require(Permission::MembersManage)?;
    if oryn_db::identity::membership_role(&s.pool, ws, user_id)
        .await?
        .is_none()
    {
        return Err(ApiError::not_found("member"));
    }
    oryn_db::identity::set_team_member(&s.pool, ws, team_id, user_id, true).await?;
    Ok(ok())
}

#[utoipa::path(delete, path = "/api/v1/workspaces/{ws}/teams/{team_id}/members/{user_id}", tag = "workspaces", responses((status = 200, body = Ok)))]
async fn remove_team_member(
    State(s): State<AppState>,
    auth: Authed,
    Path((ws, team_id, user_id)): Path<(Uuid, Uuid, Uuid)>,
) -> ApiResult<Json<Ok>> {
    let c = ws_ctx(&s, &auth, ws).await?;
    c.require(Permission::MembersManage)?;
    oryn_db::identity::set_team_member(&s.pool, ws, team_id, user_id, false).await?;
    Ok(ok())
}

#[utoipa::path(get, path = "/api/v1/workspaces/{ws}/tokens", tag = "workspaces", responses((status = 200, body = Vec<ApiToken>)))]
async fn list_tokens(
    State(s): State<AppState>,
    auth: Authed,
    Path(ws): Path<Uuid>,
) -> ApiResult<Json<Vec<ApiToken>>> {
    let c = ws_ctx(&s, &auth, ws).await?;
    let mut t = oryn_db::identity::list_api_tokens(&s.pool, ws).await?;
    if !c.role.can(Permission::MembersManage) {
        t.retain(|x| x.user_id == c.user_id());
    }
    Ok(Json(t))
}

#[derive(Deserialize, ToSchema)]
pub struct CreateToken {
    pub name: String,
    /// Role ceiling; capped by your own workspace role at use time.
    pub role: Role,
    pub expires_in_days: Option<i64>,
}

#[derive(Serialize, ToSchema)]
pub struct CreatedToken {
    pub token: ApiToken,
    /// Shown once. Store it now; only its hash is kept.
    pub secret: String,
}

#[utoipa::path(post, path = "/api/v1/workspaces/{ws}/tokens", tag = "workspaces", responses((status = 201, body = CreatedToken)))]
async fn create_token(
    State(s): State<AppState>,
    auth: Authed,
    Path(ws): Path<Uuid>,
    Json(req): Json<CreateToken>,
) -> ApiResult<(StatusCode, Json<CreatedToken>)> {
    let c = ws_ctx(&s, &auth, ws).await?;
    if matches!(c.auth.via, crate::auth::Via::Token { .. }) {
        return Err(ApiError::forbidden("API tokens cannot mint other tokens"));
    }
    if req.role.rank() > c.role.rank() {
        return Err(ApiError::forbidden("a token cannot exceed your own role"));
    }
    if req.name.trim().is_empty() {
        return Err(ApiError::bad("token name is required"));
    }
    let secret = oryn_auth::generate_token("oryn_pat_");
    let expires = req
        .expires_in_days
        .filter(|d| *d > 0)
        .map(|d| Utc::now() + Duration::days(d.min(3650)));
    let token = oryn_db::identity::create_api_token(
        &s.pool,
        ws,
        c.user_id(),
        req.name.trim(),
        req.role.as_str(),
        &oryn_auth::hash_token(&secret),
        expires,
    )
    .await?;
    audit(
        &s.pool,
        Some(ws),
        &c.actor(),
        "token.created",
        Some("api_token"),
        Some(token.id),
        json!({"name": token.name, "role": token.role}),
    )
    .await;
    Ok((StatusCode::CREATED, Json(CreatedToken { token, secret })))
}

#[utoipa::path(delete, path = "/api/v1/workspaces/{ws}/tokens/{id}", tag = "workspaces", responses((status = 200, body = Ok)))]
async fn revoke_token(
    State(s): State<AppState>,
    auth: Authed,
    Path((ws, id)): Path<(Uuid, Uuid)>,
) -> ApiResult<Json<Ok>> {
    let c = ws_ctx(&s, &auth, ws).await?;
    let tokens = oryn_db::identity::list_api_tokens(&s.pool, ws).await?;
    let t = tokens
        .iter()
        .find(|t| t.id == id)
        .ok_or_else(|| ApiError::not_found("token"))?;
    if t.user_id != c.user_id() {
        c.require(Permission::MembersManage)?;
    }
    oryn_db::identity::revoke_api_token(&s.pool, ws, id).await?;
    audit(
        &s.pool,
        Some(ws),
        &c.actor(),
        "token.revoked",
        Some("api_token"),
        Some(id),
        json!({}),
    )
    .await;
    Ok(ok())
}

#[derive(Deserialize, IntoParams)]
pub struct AuditQuery {
    pub action: Option<String>,
    pub page: Option<i64>,
    pub per_page: Option<i64>,
}

#[utoipa::path(get, path = "/api/v1/workspaces/{ws}/audit", tag = "workspaces", params(AuditQuery), responses((status = 200, body = Paged<AuditEvent>)))]
async fn list_audit(
    State(s): State<AppState>,
    auth: Authed,
    Path(ws): Path<Uuid>,
    Query(q): Query<AuditQuery>,
) -> ApiResult<Json<Paged<AuditEvent>>> {
    ws_ctx(&s, &auth, ws)
        .await?
        .require(Permission::AuditRead)?;
    let page = Page::new(q.page, q.per_page);
    let (items, total) =
        oryn_db::governance::list_audit(&s.pool, ws, q.action.as_deref(), page).await?;
    Ok(Json(Paged {
        items,
        total,
        page: page.offset / page.limit.max(1) + 1,
        per_page: page.limit,
    }))
}

#[derive(Serialize, ToSchema)]
pub struct RoleInfo {
    pub role: String,
    pub permissions: Vec<String>,
}

/// The role → permission matrix enforced by the server.
#[utoipa::path(get, path = "/api/v1/roles", tag = "workspaces", responses((status = 200, body = Vec<RoleInfo>)))]
async fn roles(_auth: Authed) -> Json<Vec<RoleInfo>> {
    Json(
        [Role::Owner, Role::Admin, Role::Engineer, Role::Viewer]
            .iter()
            .map(|r| RoleInfo {
                role: r.to_string(),
                permissions: r.permissions().iter().map(|p| p.to_string()).collect(),
            })
            .collect(),
    )
}

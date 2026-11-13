//! Users, sessions, workspaces, membership, teams and API tokens.

use chrono::{DateTime, Utc};
use serde::Serialize;
use sqlx::{FromRow, PgPool};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::{new_id, opt, slugify, DbError, DbResult};

#[derive(Debug, Clone, FromRow, Serialize, ToSchema)]
pub struct User {
    pub id: Uuid,
    pub email: String,
    pub name: String,
    #[serde(skip)]
    pub password_hash: Option<String>,
    pub is_instance_admin: bool,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, FromRow, Serialize, ToSchema)]
pub struct Workspace {
    pub id: Uuid,
    pub name: String,
    pub slug: String,
    pub require_approval: bool,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, FromRow, Serialize, ToSchema)]
pub struct WorkspaceWithRole {
    pub id: Uuid,
    pub name: String,
    pub slug: String,
    pub require_approval: bool,
    pub role: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, FromRow, Serialize, ToSchema)]
pub struct Member {
    pub user_id: Uuid,
    pub email: String,
    pub name: String,
    pub role: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, FromRow, Serialize, ToSchema)]
pub struct Team {
    pub id: Uuid,
    pub workspace_id: Uuid,
    pub name: String,
    pub description: Option<String>,
    pub member_count: i64,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, FromRow, Serialize, ToSchema)]
pub struct ApiToken {
    pub id: Uuid,
    pub workspace_id: Uuid,
    pub user_id: Uuid,
    pub name: String,
    pub role: String,
    pub created_at: DateTime<Utc>,
    pub last_used_at: Option<DateTime<Utc>>,
    pub expires_at: Option<DateTime<Utc>>,
    pub revoked_at: Option<DateTime<Utc>>,
}

const USER_COLS: &str = "id, email, name, password_hash, is_instance_admin, created_at";

pub async fn count_users(pool: &PgPool) -> DbResult<i64> {
    Ok(
        sqlx::query_scalar("SELECT count(*) FROM users WHERE deleted_at IS NULL")
            .fetch_one(pool)
            .await?,
    )
}

pub async fn create_user(
    pool: &PgPool,
    email: &str,
    name: &str,
    password_hash: Option<&str>,
    instance_admin: bool,
) -> DbResult<User> {
    sqlx::query_as::<_, User>(&format!(
        "INSERT INTO users (id, email, name, password_hash, is_instance_admin) VALUES ($1,$2,$3,$4,$5) RETURNING {USER_COLS}"
    ))
    .bind(new_id())
    .bind(email.trim().to_lowercase())
    .bind(name.trim())
    .bind(password_hash)
    .bind(instance_admin)
    .fetch_one(pool)
    .await
    .map_err(|e| DbError::from_sqlx(e, "an account with this email"))
}

pub async fn find_user_by_email(pool: &PgPool, email: &str) -> DbResult<Option<User>> {
    Ok(sqlx::query_as::<_, User>(&format!(
        "SELECT {USER_COLS} FROM users WHERE lower(email) = lower($1) AND deleted_at IS NULL"
    ))
    .bind(email.trim())
    .fetch_optional(pool)
    .await?)
}

pub async fn get_user(pool: &PgPool, id: Uuid) -> DbResult<User> {
    opt(sqlx::query_as::<_, User>(&format!(
        "SELECT {USER_COLS} FROM users WHERE id = $1 AND deleted_at IS NULL"
    ))
    .bind(id)
    .fetch_one(pool)
    .await)
}

/// Result of matching an OIDC identity to a local account.
#[derive(Debug)]
pub enum OidcLink {
    User(User),
    /// No account exists and self-registration is disabled.
    RegistrationClosed,
    /// An account with this email is already linked to a different OIDC identity.
    LinkedElsewhere,
}

/// Find the account for an OIDC identity. Call only with a verified email: a local
/// account with that email is linked on first sign-in, unless it is already linked
/// to another identity. A new account is created only when `allow_create` is set.
pub async fn upsert_oidc_user(
    pool: &PgPool,
    issuer: &str,
    subject: &str,
    email: &str,
    name: &str,
    allow_create: bool,
) -> DbResult<OidcLink> {
    if let Some(u) = sqlx::query_as::<_, User>(&format!(
        "SELECT {USER_COLS} FROM users WHERE oidc_issuer = $1 AND oidc_subject = $2 AND deleted_at IS NULL"
    ))
    .bind(issuer)
    .bind(subject)
    .fetch_optional(pool)
    .await?
    {
        return Ok(OidcLink::User(u));
    }
    if let Some(u) = find_user_by_email(pool, email).await? {
        // Link only an account that is not yet tied to any OIDC identity.
        let linked = sqlx::query(
            "UPDATE users SET oidc_issuer = $1, oidc_subject = $2, updated_at = now()
             WHERE id = $3 AND oidc_subject IS NULL",
        )
        .bind(issuer)
        .bind(subject)
        .bind(u.id)
        .execute(pool)
        .await?
        .rows_affected();
        return Ok(if linked == 1 {
            OidcLink::User(u)
        } else {
            OidcLink::LinkedElsewhere
        });
    }
    if !allow_create {
        return Ok(OidcLink::RegistrationClosed);
    }
    let u = create_user(pool, email, name, None, false).await?;
    sqlx::query("UPDATE users SET oidc_issuer = $1, oidc_subject = $2 WHERE id = $3")
        .bind(issuer)
        .bind(subject)
        .bind(u.id)
        .execute(pool)
        .await?;
    Ok(OidcLink::User(u))
}

// ── sessions ──
pub async fn create_session(
    pool: &PgPool,
    user_id: Uuid,
    token_hash: &str,
    ttl_hours: i64,
    ua: Option<&str>,
    ip: Option<&str>,
) -> DbResult<DateTime<Utc>> {
    let expires = Utc::now() + chrono::Duration::hours(ttl_hours);
    sqlx::query("INSERT INTO sessions (id, user_id, token_hash, expires_at, user_agent, ip) VALUES ($1,$2,$3,$4,$5,$6)")
        .bind(new_id())
        .bind(user_id)
        .bind(token_hash)
        .bind(expires)
        .bind(ua)
        .bind(ip)
        .execute(pool)
        .await?;
    Ok(expires)
}

/// Resolve a session token hash to its user, sliding `last_seen_at`.
pub async fn session_user(pool: &PgPool, token_hash: &str) -> DbResult<Option<User>> {
    Ok(sqlx::query_as::<_, User>(
        "WITH s AS (UPDATE sessions SET last_seen_at = now() WHERE token_hash = $1 AND expires_at > now() RETURNING user_id)
         SELECT u.id, u.email, u.name, u.password_hash, u.is_instance_admin, u.created_at
         FROM users u JOIN s ON s.user_id = u.id WHERE u.deleted_at IS NULL",
    )
    .bind(token_hash)
    .fetch_optional(pool)
    .await?)
}

pub async fn delete_session(pool: &PgPool, token_hash: &str) -> DbResult<()> {
    sqlx::query("DELETE FROM sessions WHERE token_hash = $1")
        .bind(token_hash)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn purge_expired_sessions(pool: &PgPool) -> DbResult<u64> {
    Ok(sqlx::query("DELETE FROM sessions WHERE expires_at < now()")
        .execute(pool)
        .await?
        .rows_affected())
}

// ── workspaces ──
pub async fn create_workspace(pool: &PgPool, name: &str, owner: Uuid) -> DbResult<Workspace> {
    let mut tx = pool.begin().await?;
    let base = slugify(name);
    let taken: i64 = sqlx::query_scalar("SELECT count(*) FROM workspaces WHERE (slug = $1 OR slug LIKE $1 || '-%') AND deleted_at IS NULL")
        .bind(&base)
        .fetch_one(&mut *tx)
        .await?;
    let slug = if taken == 0 {
        base
    } else {
        format!("{base}-{}", taken + 1)
    };
    let ws = sqlx::query_as::<_, Workspace>(
        "INSERT INTO workspaces (id, name, slug, created_by) VALUES ($1,$2,$3,$4)
         RETURNING id, name, slug, require_approval, created_at",
    )
    .bind(new_id())
    .bind(name.trim())
    .bind(slug)
    .bind(owner)
    .fetch_one(&mut *tx)
    .await
    .map_err(|e| DbError::from_sqlx(e, "workspace"))?;
    sqlx::query(
        "INSERT INTO workspace_members (workspace_id, user_id, role) VALUES ($1,$2,'owner')",
    )
    .bind(ws.id)
    .bind(owner)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(ws)
}

pub async fn list_user_workspaces(
    pool: &PgPool,
    user_id: Uuid,
) -> DbResult<Vec<WorkspaceWithRole>> {
    Ok(sqlx::query_as::<_, WorkspaceWithRole>(
        "SELECT w.id, w.name, w.slug, w.require_approval, m.role, w.created_at
         FROM workspaces w JOIN workspace_members m ON m.workspace_id = w.id
         WHERE m.user_id = $1 AND w.deleted_at IS NULL ORDER BY w.created_at",
    )
    .bind(user_id)
    .fetch_all(pool)
    .await?)
}

pub async fn get_workspace(pool: &PgPool, id: Uuid) -> DbResult<Workspace> {
    opt(sqlx::query_as::<_, Workspace>(
        "SELECT id, name, slug, require_approval, created_at FROM workspaces WHERE id = $1 AND deleted_at IS NULL",
    )
    .bind(id)
    .fetch_one(pool)
    .await)
}

pub async fn update_workspace(
    pool: &PgPool,
    id: Uuid,
    name: Option<&str>,
    require_approval: Option<bool>,
) -> DbResult<Workspace> {
    opt(sqlx::query_as::<_, Workspace>(
        "UPDATE workspaces SET name = COALESCE($2, name), require_approval = COALESCE($3, require_approval), updated_at = now()
         WHERE id = $1 AND deleted_at IS NULL RETURNING id, name, slug, require_approval, created_at",
    )
    .bind(id)
    .bind(name)
    .bind(require_approval)
    .fetch_one(pool)
    .await)
}

pub async fn soft_delete_workspace(pool: &PgPool, id: Uuid) -> DbResult<()> {
    sqlx::query("UPDATE workspaces SET deleted_at = now() WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn membership_role(
    pool: &PgPool,
    workspace_id: Uuid,
    user_id: Uuid,
) -> DbResult<Option<String>> {
    Ok(sqlx::query_scalar(
        "SELECT m.role FROM workspace_members m JOIN workspaces w ON w.id = m.workspace_id
         WHERE m.workspace_id = $1 AND m.user_id = $2 AND w.deleted_at IS NULL",
    )
    .bind(workspace_id)
    .bind(user_id)
    .fetch_optional(pool)
    .await?)
}

pub async fn list_members(pool: &PgPool, workspace_id: Uuid) -> DbResult<Vec<Member>> {
    Ok(sqlx::query_as::<_, Member>(
        "SELECT u.id AS user_id, u.email, u.name, m.role, m.created_at FROM workspace_members m
         JOIN users u ON u.id = m.user_id WHERE m.workspace_id = $1 AND u.deleted_at IS NULL ORDER BY m.created_at",
    )
    .bind(workspace_id)
    .fetch_all(pool)
    .await?)
}

pub async fn add_member(
    pool: &PgPool,
    workspace_id: Uuid,
    user_id: Uuid,
    role: &str,
) -> DbResult<()> {
    sqlx::query("INSERT INTO workspace_members (workspace_id, user_id, role) VALUES ($1,$2,$3)")
        .bind(workspace_id)
        .bind(user_id)
        .bind(role)
        .execute(pool)
        .await
        .map_err(|e| DbError::from_sqlx(e, "membership"))?;
    Ok(())
}

pub async fn set_member_role(
    pool: &PgPool,
    workspace_id: Uuid,
    user_id: Uuid,
    role: &str,
) -> DbResult<()> {
    let r = sqlx::query(
        "UPDATE workspace_members SET role = $3 WHERE workspace_id = $1 AND user_id = $2",
    )
    .bind(workspace_id)
    .bind(user_id)
    .bind(role)
    .execute(pool)
    .await?;
    if r.rows_affected() == 0 {
        return Err(DbError::NotFound);
    }
    Ok(())
}

pub async fn count_owners(pool: &PgPool, workspace_id: Uuid) -> DbResult<i64> {
    Ok(sqlx::query_scalar(
        "SELECT count(*) FROM workspace_members WHERE workspace_id = $1 AND role = 'owner'",
    )
    .bind(workspace_id)
    .fetch_one(pool)
    .await?)
}

pub async fn remove_member(pool: &PgPool, workspace_id: Uuid, user_id: Uuid) -> DbResult<()> {
    sqlx::query("DELETE FROM workspace_members WHERE workspace_id = $1 AND user_id = $2")
        .bind(workspace_id)
        .bind(user_id)
        .execute(pool)
        .await?;
    Ok(())
}

// ── teams ──
pub async fn list_teams(pool: &PgPool, workspace_id: Uuid) -> DbResult<Vec<Team>> {
    Ok(sqlx::query_as::<_, Team>(
        "SELECT t.id, t.workspace_id, t.name, t.description, (SELECT count(*) FROM team_members tm WHERE tm.team_id = t.id) AS member_count, t.created_at
         FROM teams t WHERE t.workspace_id = $1 ORDER BY t.name",
    )
    .bind(workspace_id)
    .fetch_all(pool)
    .await?)
}

/// Members of one team, as workspace members (their workspace role included).
pub async fn list_team_members(
    pool: &PgPool,
    workspace_id: Uuid,
    team_id: Uuid,
) -> DbResult<Vec<Member>> {
    Ok(sqlx::query_as::<_, Member>(
        "SELECT u.id AS user_id, u.email, u.name, m.role, m.created_at
         FROM team_members tm
         JOIN teams t ON t.id = tm.team_id AND t.workspace_id = $1
         JOIN workspace_members m ON m.workspace_id = t.workspace_id AND m.user_id = tm.user_id
         JOIN users u ON u.id = tm.user_id
         WHERE tm.team_id = $2 AND u.deleted_at IS NULL ORDER BY u.name",
    )
    .bind(workspace_id)
    .bind(team_id)
    .fetch_all(pool)
    .await?)
}

pub async fn create_team(
    pool: &PgPool,
    workspace_id: Uuid,
    name: &str,
    description: Option<&str>,
) -> DbResult<Team> {
    sqlx::query_as::<_, Team>(
        "INSERT INTO teams (id, workspace_id, name, description) VALUES ($1,$2,$3,$4)
         RETURNING id, workspace_id, name, description, 0::bigint AS member_count, created_at",
    )
    .bind(new_id())
    .bind(workspace_id)
    .bind(name.trim())
    .bind(description)
    .fetch_one(pool)
    .await
    .map_err(|e| DbError::from_sqlx(e, "team"))
}

pub async fn delete_team(pool: &PgPool, workspace_id: Uuid, team_id: Uuid) -> DbResult<()> {
    sqlx::query("DELETE FROM teams WHERE id = $1 AND workspace_id = $2")
        .bind(team_id)
        .bind(workspace_id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn set_team_member(
    pool: &PgPool,
    workspace_id: Uuid,
    team_id: Uuid,
    user_id: Uuid,
    present: bool,
) -> DbResult<()> {
    let ok: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM teams t JOIN workspace_members m ON m.workspace_id = t.workspace_id
          WHERE t.id = $1 AND t.workspace_id = $2 AND m.user_id = $3)",
    )
    .bind(team_id)
    .bind(workspace_id)
    .bind(user_id)
    .fetch_one(pool)
    .await?;
    if !ok {
        return Err(DbError::NotFound);
    }
    if present {
        sqlx::query(
            "INSERT INTO team_members (team_id, user_id) VALUES ($1,$2) ON CONFLICT DO NOTHING",
        )
        .bind(team_id)
        .bind(user_id)
        .execute(pool)
        .await?;
    } else {
        sqlx::query("DELETE FROM team_members WHERE team_id = $1 AND user_id = $2")
            .bind(team_id)
            .bind(user_id)
            .execute(pool)
            .await?;
    }
    Ok(())
}

// ── API tokens ──
pub async fn create_api_token(
    pool: &PgPool,
    workspace_id: Uuid,
    user_id: Uuid,
    name: &str,
    role: &str,
    token_hash: &str,
    expires_at: Option<DateTime<Utc>>,
) -> DbResult<ApiToken> {
    Ok(sqlx::query_as::<_, ApiToken>(
        "INSERT INTO api_tokens (id, workspace_id, user_id, name, role, token_hash, expires_at) VALUES ($1,$2,$3,$4,$5,$6,$7)
         RETURNING id, workspace_id, user_id, name, role, created_at, last_used_at, expires_at, revoked_at",
    )
    .bind(new_id())
    .bind(workspace_id)
    .bind(user_id)
    .bind(name)
    .bind(role)
    .bind(token_hash)
    .bind(expires_at)
    .fetch_one(pool)
    .await?)
}

pub async fn list_api_tokens(pool: &PgPool, workspace_id: Uuid) -> DbResult<Vec<ApiToken>> {
    Ok(sqlx::query_as::<_, ApiToken>(
        "SELECT id, workspace_id, user_id, name, role, created_at, last_used_at, expires_at, revoked_at FROM api_tokens
         WHERE workspace_id = $1 ORDER BY created_at DESC",
    )
    .bind(workspace_id)
    .fetch_all(pool)
    .await?)
}

pub async fn revoke_api_token(pool: &PgPool, workspace_id: Uuid, id: Uuid) -> DbResult<()> {
    sqlx::query("UPDATE api_tokens SET revoked_at = now() WHERE id = $1 AND workspace_id = $2 AND revoked_at IS NULL")
        .bind(id)
        .bind(workspace_id)
        .execute(pool)
        .await?;
    Ok(())
}

/// Resolve an API token hash. Returns (user, workspace_id, token role). The effective role
/// is the lower of the token role and the user's current membership role (checked by caller).
pub async fn api_token_user(
    pool: &PgPool,
    token_hash: &str,
) -> DbResult<Option<(User, Uuid, String)>> {
    let row: Option<(Uuid, Uuid, String)> = sqlx::query_as(
        "UPDATE api_tokens SET last_used_at = now() WHERE token_hash = $1 AND revoked_at IS NULL
         AND (expires_at IS NULL OR expires_at > now()) RETURNING user_id, workspace_id, role",
    )
    .bind(token_hash)
    .fetch_optional(pool)
    .await?;
    match row {
        None => Ok(None),
        Some((uid, ws, role)) => Ok(Some((get_user(pool, uid).await?, ws, role))),
    }
}

// ── OIDC state ──
pub async fn put_oidc_state(
    pool: &PgPool,
    state: &str,
    nonce: &str,
    verifier: &str,
    redirect: Option<&str>,
) -> DbResult<()> {
    sqlx::query("DELETE FROM oidc_states WHERE created_at < now() - interval '15 minutes'")
        .execute(pool)
        .await?;
    sqlx::query(
        "INSERT INTO oidc_states (state, nonce, pkce_verifier, redirect_to) VALUES ($1,$2,$3,$4)",
    )
    .bind(state)
    .bind(nonce)
    .bind(verifier)
    .bind(redirect)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn take_oidc_state(
    pool: &PgPool,
    state: &str,
) -> DbResult<Option<(String, String, Option<String>)>> {
    Ok(sqlx::query_as(
        "DELETE FROM oidc_states WHERE state = $1 AND created_at > now() - interval '15 minutes' RETURNING nonce, pkce_verifier, redirect_to",
    )
    .bind(state)
    .fetch_optional(pool)
    .await?)
}

/// Role → permission matrix as stored in the database (reference data).
pub async fn role_permission_matrix(pool: &PgPool) -> DbResult<Vec<(String, String)>> {
    Ok(
        sqlx::query_as("SELECT role, permission FROM role_permissions ORDER BY role, permission")
            .fetch_all(pool)
            .await?,
    )
}

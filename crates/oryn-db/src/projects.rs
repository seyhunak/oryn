//! Projects, access control, environments, repositories, application context and requirements.

use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::Value;
use sqlx::{FromRow, PgPool};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::{new_id, opt, slugify, DbError, DbResult};

#[derive(Debug, Clone, FromRow, Serialize, ToSchema)]
pub struct Project {
    pub id: Uuid,
    pub workspace_id: Uuid,
    pub name: String,
    pub slug: String,
    pub description: Option<String>,
    pub visibility: String,
    pub color: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, FromRow, Serialize, ToSchema)]
pub struct ProjectSummary {
    pub id: Uuid,
    pub workspace_id: Uuid,
    pub name: String,
    pub slug: String,
    pub description: Option<String>,
    pub visibility: String,
    pub color: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub scenario_count: i64,
    pub suite_count: i64,
    pub last_execution_status: Option<String>,
    pub last_execution_at: Option<DateTime<Utc>>,
    pub repository_status: Option<String>,
}

const PROJECT_COLS: &str =
    "id, workspace_id, name, slug, description, visibility, color, created_at, updated_at";

pub async fn create_project(
    pool: &PgPool,
    workspace_id: Uuid,
    name: &str,
    description: Option<&str>,
    visibility: &str,
    color: &str,
    user: Uuid,
) -> DbResult<Project> {
    let mut slug = slugify(name);
    let n: i64 = sqlx::query_scalar("SELECT count(*) FROM projects WHERE workspace_id = $1 AND (slug = $2 OR slug LIKE $2 || '-%') AND deleted_at IS NULL")
        .bind(workspace_id)
        .bind(&slug)
        .fetch_one(pool)
        .await?;
    if n > 0 {
        slug = format!("{slug}-{}", n + 1);
    }
    sqlx::query_as::<_, Project>(&format!(
        "INSERT INTO projects (id, workspace_id, name, slug, description, visibility, color, created_by) VALUES ($1,$2,$3,$4,$5,$6,$7,$8) RETURNING {PROJECT_COLS}"
    ))
    .bind(new_id())
    .bind(workspace_id)
    .bind(name.trim())
    .bind(slug)
    .bind(description)
    .bind(visibility)
    .bind(color)
    .bind(user)
    .fetch_one(pool)
    .await
    .map_err(|e| DbError::from_sqlx(e, "project"))
}

/// Projects visible to a user: workspace-visible ones, plus restricted ones where the user
/// (or one of their teams) has a project permission, plus all when `see_all` (admins).
pub async fn list_projects(
    pool: &PgPool,
    workspace_id: Uuid,
    user_id: Uuid,
    see_all: bool,
) -> DbResult<Vec<ProjectSummary>> {
    Ok(sqlx::query_as::<_, ProjectSummary>(
        "SELECT p.id, p.workspace_id, p.name, p.slug, p.description, p.visibility, p.color, p.created_at, p.updated_at,
            (SELECT count(*) FROM test_scenarios s WHERE s.project_id = p.id AND s.deleted_at IS NULL) AS scenario_count,
            (SELECT count(*) FROM test_suites s WHERE s.project_id = p.id AND s.deleted_at IS NULL) AS suite_count,
            le.status AS last_execution_status, le.created_at AS last_execution_at,
            (SELECT r.status FROM repositories r WHERE r.project_id = p.id ORDER BY r.created_at DESC LIMIT 1) AS repository_status
         FROM projects p
         LEFT JOIN LATERAL (SELECT e.status, e.created_at FROM test_executions e WHERE e.project_id = p.id ORDER BY e.created_at DESC LIMIT 1) le ON true
         WHERE p.workspace_id = $1 AND p.deleted_at IS NULL AND (
            $3 OR p.visibility = 'workspace' OR EXISTS (
              SELECT 1 FROM project_permissions pp LEFT JOIN team_members tm ON tm.team_id = pp.team_id
              WHERE pp.project_id = p.id AND (pp.user_id = $2 OR tm.user_id = $2)))
         ORDER BY p.name",
    )
    .bind(workspace_id)
    .bind(user_id)
    .bind(see_all)
    .fetch_all(pool)
    .await?)
}

pub async fn get_project(pool: &PgPool, id: Uuid) -> DbResult<Project> {
    opt(sqlx::query_as::<_, Project>(&format!(
        "SELECT {PROJECT_COLS} FROM projects WHERE id = $1 AND deleted_at IS NULL"
    ))
    .bind(id)
    .fetch_one(pool)
    .await)
}

/// Explicit project-level role for a user (direct or via team), highest wins.
pub async fn project_role(
    pool: &PgPool,
    project_id: Uuid,
    user_id: Uuid,
) -> DbResult<Option<String>> {
    Ok(sqlx::query_scalar(
        "SELECT pp.role FROM project_permissions pp JOIN roles r ON r.name = pp.role
         LEFT JOIN team_members tm ON tm.team_id = pp.team_id
         WHERE pp.project_id = $1 AND (pp.user_id = $2 OR tm.user_id = $2) ORDER BY r.rank DESC LIMIT 1",
    )
    .bind(project_id)
    .bind(user_id)
    .fetch_optional(pool)
    .await?)
}

#[derive(Debug, Clone, FromRow, Serialize, ToSchema)]
pub struct ProjectPermission {
    pub id: Uuid,
    pub project_id: Uuid,
    pub user_id: Option<Uuid>,
    pub team_id: Option<Uuid>,
    pub role: String,
    pub created_at: DateTime<Utc>,
}

pub async fn list_project_permissions(
    pool: &PgPool,
    project_id: Uuid,
) -> DbResult<Vec<ProjectPermission>> {
    Ok(sqlx::query_as::<_, ProjectPermission>("SELECT id, project_id, user_id, team_id, role, created_at FROM project_permissions WHERE project_id = $1")
        .bind(project_id)
        .fetch_all(pool)
        .await?)
}

pub async fn add_project_permission(
    pool: &PgPool,
    project_id: Uuid,
    user_id: Option<Uuid>,
    team_id: Option<Uuid>,
    role: &str,
) -> DbResult<ProjectPermission> {
    Ok(sqlx::query_as::<_, ProjectPermission>(
        "INSERT INTO project_permissions (id, project_id, user_id, team_id, role) VALUES ($1,$2,$3,$4,$5)
         RETURNING id, project_id, user_id, team_id, role, created_at",
    )
    .bind(new_id())
    .bind(project_id)
    .bind(user_id)
    .bind(team_id)
    .bind(role)
    .fetch_one(pool)
    .await?)
}

pub async fn delete_project_permission(pool: &PgPool, project_id: Uuid, id: Uuid) -> DbResult<()> {
    sqlx::query("DELETE FROM project_permissions WHERE id = $1 AND project_id = $2")
        .bind(id)
        .bind(project_id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn update_project(
    pool: &PgPool,
    id: Uuid,
    name: Option<&str>,
    description: Option<&str>,
    visibility: Option<&str>,
    color: Option<&str>,
) -> DbResult<Project> {
    opt(sqlx::query_as::<_, Project>(&format!(
        "UPDATE projects SET name = COALESCE($2, name), description = COALESCE($3, description), visibility = COALESCE($4, visibility),
         color = COALESCE($5, color), updated_at = now() WHERE id = $1 AND deleted_at IS NULL RETURNING {PROJECT_COLS}"
    ))
    .bind(id)
    .bind(name)
    .bind(description)
    .bind(visibility)
    .bind(color)
    .fetch_one(pool)
    .await)
}

pub async fn soft_delete_project(pool: &PgPool, id: Uuid) -> DbResult<()> {
    sqlx::query("UPDATE projects SET deleted_at = now() WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

// ── environments ──
#[derive(Debug, Clone, FromRow)]
pub struct EnvironmentRow {
    pub id: Uuid,
    pub project_id: Uuid,
    pub name: String,
    pub base_url: Option<String>,
    pub api_base_url: Option<String>,
    pub allowed_hosts: Vec<String>,
    pub variables: Value,
    pub secrets: Value,
    pub is_default: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Environment as returned by the API: secret *names* only.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct Environment {
    pub id: Uuid,
    pub project_id: Uuid,
    pub name: String,
    pub base_url: Option<String>,
    pub api_base_url: Option<String>,
    pub allowed_hosts: Vec<String>,
    #[schema(value_type = Object)]
    pub variables: Value,
    pub secret_names: Vec<String>,
    pub is_default: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl From<EnvironmentRow> for Environment {
    fn from(r: EnvironmentRow) -> Self {
        let mut secret_names: Vec<String> = r
            .secrets
            .as_object()
            .map(|o| o.keys().cloned().collect())
            .unwrap_or_default();
        secret_names.sort();
        Self {
            id: r.id,
            project_id: r.project_id,
            name: r.name,
            base_url: r.base_url,
            api_base_url: r.api_base_url,
            allowed_hosts: r.allowed_hosts,
            variables: r.variables,
            secret_names,
            is_default: r.is_default,
            created_at: r.created_at,
            updated_at: r.updated_at,
        }
    }
}

const ENV_COLS: &str = "id, project_id, name, base_url, api_base_url, allowed_hosts, variables, secrets, is_default, created_at, updated_at";

pub async fn list_environments(pool: &PgPool, project_id: Uuid) -> DbResult<Vec<EnvironmentRow>> {
    Ok(sqlx::query_as::<_, EnvironmentRow>(&format!("SELECT {ENV_COLS} FROM test_environments WHERE project_id = $1 ORDER BY is_default DESC, name"))
        .bind(project_id)
        .fetch_all(pool)
        .await?)
}

pub async fn get_environment(
    pool: &PgPool,
    project_id: Uuid,
    id: Uuid,
) -> DbResult<EnvironmentRow> {
    opt(sqlx::query_as::<_, EnvironmentRow>(&format!(
        "SELECT {ENV_COLS} FROM test_environments WHERE id = $1 AND project_id = $2"
    ))
    .bind(id)
    .bind(project_id)
    .fetch_one(pool)
    .await)
}

pub async fn default_environment(
    pool: &PgPool,
    project_id: Uuid,
) -> DbResult<Option<EnvironmentRow>> {
    Ok(sqlx::query_as::<_, EnvironmentRow>(&format!(
        "SELECT {ENV_COLS} FROM test_environments WHERE project_id = $1 ORDER BY is_default DESC, created_at LIMIT 1"
    ))
    .bind(project_id)
    .fetch_optional(pool)
    .await?)
}

#[allow(clippy::too_many_arguments)]
pub async fn upsert_environment(
    pool: &PgPool,
    project_id: Uuid,
    id: Option<Uuid>,
    name: &str,
    base_url: Option<&str>,
    api_base_url: Option<&str>,
    allowed_hosts: &[String],
    variables: &Value,
    secrets: &Value,
    is_default: bool,
) -> DbResult<EnvironmentRow> {
    let mut tx = pool.begin().await?;
    if is_default {
        sqlx::query("UPDATE test_environments SET is_default = false WHERE project_id = $1")
            .bind(project_id)
            .execute(&mut *tx)
            .await?;
    }
    let row = match id {
        None => sqlx::query_as::<_, EnvironmentRow>(&format!(
            "INSERT INTO test_environments (id, project_id, name, base_url, api_base_url, allowed_hosts, variables, secrets, is_default)
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9) RETURNING {ENV_COLS}"
        ))
        .bind(new_id())
        .bind(project_id)
        .bind(name)
        .bind(base_url)
        .bind(api_base_url)
        .bind(allowed_hosts)
        .bind(variables)
        .bind(secrets)
        .bind(is_default)
        .fetch_one(&mut *tx)
        .await
        .map_err(|e| DbError::from_sqlx(e, "environment"))?,
        Some(id) => opt(sqlx::query_as::<_, EnvironmentRow>(&format!(
            "UPDATE test_environments SET name=$3, base_url=$4, api_base_url=$5, allowed_hosts=$6, variables=$7, secrets=$8, is_default=$9, updated_at=now()
             WHERE id=$1 AND project_id=$2 RETURNING {ENV_COLS}"
        ))
        .bind(id)
        .bind(project_id)
        .bind(name)
        .bind(base_url)
        .bind(api_base_url)
        .bind(allowed_hosts)
        .bind(variables)
        .bind(secrets)
        .bind(is_default)
        .fetch_one(&mut *tx)
        .await)?,
    };
    tx.commit().await?;
    Ok(row)
}

pub async fn delete_environment(pool: &PgPool, project_id: Uuid, id: Uuid) -> DbResult<()> {
    sqlx::query("DELETE FROM test_environments WHERE id = $1 AND project_id = $2")
        .bind(id)
        .bind(project_id)
        .execute(pool)
        .await?;
    Ok(())
}

// ── repositories ──
#[derive(Debug, Clone, FromRow, Serialize, ToSchema)]
pub struct Repository {
    pub id: Uuid,
    pub project_id: Uuid,
    pub source: String,
    pub url: Option<String>,
    pub default_branch: Option<String>,
    #[serde(skip)]
    pub local_path: String,
    pub last_commit: Option<String>,
    pub status: String,
    pub error: Option<String>,
    pub size_bytes: Option<i64>,
    pub file_count: Option<i32>,
    pub last_analyzed_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub connection_id: Option<Uuid>,
}

const REPO_COLS: &str = "id, project_id, source, url, default_branch, local_path, last_commit, status, error, size_bytes, file_count, last_analyzed_at, created_at, connection_id";

pub async fn create_repository(
    pool: &PgPool,
    id: Uuid,
    project_id: Uuid,
    source: &str,
    url: Option<&str>,
    branch: Option<&str>,
    local_path: &str,
    connection_id: Option<Uuid>,
) -> DbResult<Repository> {
    Ok(sqlx::query_as::<_, Repository>(&format!(
        "INSERT INTO repositories (id, project_id, source, url, default_branch, local_path, status, connection_id) VALUES ($1,$2,$3,$4,$5,$6,'importing',$7) RETURNING {REPO_COLS}"
    ))
    .bind(id)
    .bind(project_id)
    .bind(source)
    .bind(url)
    .bind(branch)
    .bind(local_path)
    .bind(connection_id)
    .fetch_one(pool)
    .await?)
}

pub async fn list_repositories(pool: &PgPool, project_id: Uuid) -> DbResult<Vec<Repository>> {
    Ok(sqlx::query_as::<_, Repository>(&format!(
        "SELECT {REPO_COLS} FROM repositories WHERE project_id = $1 ORDER BY created_at DESC"
    ))
    .bind(project_id)
    .fetch_all(pool)
    .await?)
}

pub async fn get_repository(pool: &PgPool, project_id: Uuid, id: Uuid) -> DbResult<Repository> {
    opt(sqlx::query_as::<_, Repository>(&format!(
        "SELECT {REPO_COLS} FROM repositories WHERE id = $1 AND project_id = $2"
    ))
    .bind(id)
    .bind(project_id)
    .fetch_one(pool)
    .await)
}

pub async fn set_repository_status(
    pool: &PgPool,
    id: Uuid,
    status: &str,
    error: Option<&str>,
    commit: Option<&str>,
    size: Option<i64>,
    files: Option<i32>,
) -> DbResult<()> {
    sqlx::query(
        "UPDATE repositories SET status = $2, error = $3, last_commit = COALESCE($4, last_commit), size_bytes = COALESCE($5, size_bytes),
         file_count = COALESCE($6, file_count), last_analyzed_at = CASE WHEN $2 = 'ready' THEN now() ELSE last_analyzed_at END, updated_at = now() WHERE id = $1",
    )
    .bind(id)
    .bind(status)
    .bind(error)
    .bind(commit)
    .bind(size)
    .bind(files)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn delete_repository(pool: &PgPool, project_id: Uuid, id: Uuid) -> DbResult<()> {
    sqlx::query("DELETE FROM repositories WHERE id = $1 AND project_id = $2")
        .bind(id)
        .bind(project_id)
        .execute(pool)
        .await?;
    Ok(())
}

#[derive(Debug, Clone, FromRow, Serialize, ToSchema)]
pub struct RepositoryConnection {
    pub id: Uuid,
    pub workspace_id: Uuid,
    pub kind: String,
    pub name: String,
    pub base_url: String,
    #[serde(skip)]
    pub token_encrypted: Option<String>,
    pub created_at: DateTime<Utc>,
}

pub async fn create_connection(
    pool: &PgPool,
    workspace_id: Uuid,
    kind: &str,
    name: &str,
    base_url: &str,
    token: Option<&str>,
    user: Uuid,
) -> DbResult<RepositoryConnection> {
    Ok(sqlx::query_as::<_, RepositoryConnection>(
        "INSERT INTO repository_connections (id, workspace_id, kind, name, base_url, token_encrypted, created_by) VALUES ($1,$2,$3,$4,$5,$6,$7)
         RETURNING id, workspace_id, kind, name, base_url, token_encrypted, created_at",
    )
    .bind(new_id())
    .bind(workspace_id)
    .bind(kind)
    .bind(name)
    .bind(base_url)
    .bind(token)
    .bind(user)
    .fetch_one(pool)
    .await?)
}

pub async fn list_connections(
    pool: &PgPool,
    workspace_id: Uuid,
) -> DbResult<Vec<RepositoryConnection>> {
    Ok(sqlx::query_as::<_, RepositoryConnection>(
        "SELECT id, workspace_id, kind, name, base_url, token_encrypted, created_at FROM repository_connections WHERE workspace_id = $1 ORDER BY created_at",
    )
    .bind(workspace_id)
    .fetch_all(pool)
    .await?)
}

pub async fn get_connection(
    pool: &PgPool,
    workspace_id: Uuid,
    id: Uuid,
) -> DbResult<RepositoryConnection> {
    opt(sqlx::query_as::<_, RepositoryConnection>(
        "SELECT id, workspace_id, kind, name, base_url, token_encrypted, created_at FROM repository_connections WHERE workspace_id = $1 AND id = $2",
    )
    .bind(workspace_id)
    .bind(id)
    .fetch_one(pool)
    .await)
}

pub async fn delete_connection(pool: &PgPool, workspace_id: Uuid, id: Uuid) -> DbResult<()> {
    sqlx::query("DELETE FROM repository_connections WHERE workspace_id = $1 AND id = $2")
        .bind(workspace_id)
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

// ── application context ──
#[derive(Debug, Clone, FromRow, Serialize, ToSchema)]
pub struct ApplicationContext {
    pub id: Uuid,
    pub project_id: Uuid,
    pub repository_id: Option<Uuid>,
    pub version: i32,
    pub summary: String,
    #[schema(value_type = Object)]
    pub analysis: Value,
    #[serde(skip)]
    pub file_hashes: Value,
    pub commit_sha: Option<String>,
    pub analyzed_at: DateTime<Utc>,
    pub files_changed_last_run: i32,
}

pub async fn get_context(pool: &PgPool, project_id: Uuid) -> DbResult<Option<ApplicationContext>> {
    Ok(sqlx::query_as::<_, ApplicationContext>(
        "SELECT id, project_id, repository_id, version, summary, analysis, file_hashes, commit_sha, analyzed_at, files_changed_last_run
         FROM application_contexts WHERE project_id = $1",
    )
    .bind(project_id)
    .fetch_optional(pool)
    .await?)
}

#[allow(clippy::too_many_arguments)]
pub async fn upsert_context(
    pool: &PgPool,
    project_id: Uuid,
    repository_id: Option<Uuid>,
    summary: &str,
    analysis: &Value,
    hashes: &Value,
    commit: Option<&str>,
    changed: i32,
) -> DbResult<ApplicationContext> {
    Ok(sqlx::query_as::<_, ApplicationContext>(
        "INSERT INTO application_contexts (id, project_id, repository_id, summary, analysis, file_hashes, commit_sha, files_changed_last_run)
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8)
         ON CONFLICT (project_id) DO UPDATE SET repository_id = EXCLUDED.repository_id, summary = EXCLUDED.summary, analysis = EXCLUDED.analysis,
           file_hashes = EXCLUDED.file_hashes, commit_sha = EXCLUDED.commit_sha, files_changed_last_run = EXCLUDED.files_changed_last_run,
           version = application_contexts.version + 1, analyzed_at = now()
         RETURNING id, project_id, repository_id, version, summary, analysis, file_hashes, commit_sha, analyzed_at, files_changed_last_run",
    )
    .bind(new_id())
    .bind(project_id)
    .bind(repository_id)
    .bind(summary)
    .bind(analysis)
    .bind(hashes)
    .bind(commit)
    .bind(changed)
    .fetch_one(pool)
    .await?)
}

#[derive(Debug, Clone, FromRow, Serialize, ToSchema)]
pub struct ApplicationModule {
    pub id: Uuid,
    pub project_id: Uuid,
    pub name: String,
    pub path_prefix: String,
    pub kind: String,
    pub description: Option<String>,
    pub file_count: i32,
}

pub async fn replace_modules(
    pool: &PgPool,
    project_id: Uuid,
    modules: &[(String, String, String, Option<String>, i32)],
) -> DbResult<()> {
    let mut tx = pool.begin().await?;
    let prefixes: Vec<String> = modules.iter().map(|m| m.1.clone()).collect();
    sqlx::query(
        "DELETE FROM application_modules WHERE project_id = $1 AND NOT (path_prefix = ANY($2))",
    )
    .bind(project_id)
    .bind(&prefixes)
    .execute(&mut *tx)
    .await?;
    for (name, prefix, kind, desc, count) in modules {
        sqlx::query(
            "INSERT INTO application_modules (id, project_id, name, path_prefix, kind, description, file_count) VALUES ($1,$2,$3,$4,$5,$6,$7)
             ON CONFLICT (project_id, path_prefix) DO UPDATE SET name = EXCLUDED.name, kind = EXCLUDED.kind, description = EXCLUDED.description, file_count = EXCLUDED.file_count",
        )
        .bind(new_id())
        .bind(project_id)
        .bind(name)
        .bind(prefix)
        .bind(kind)
        .bind(desc)
        .bind(count)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    Ok(())
}

pub async fn list_modules(pool: &PgPool, project_id: Uuid) -> DbResult<Vec<ApplicationModule>> {
    Ok(sqlx::query_as::<_, ApplicationModule>(
        "SELECT id, project_id, name, path_prefix, kind, description, file_count FROM application_modules WHERE project_id = $1 ORDER BY path_prefix",
    )
    .bind(project_id)
    .fetch_all(pool)
    .await?)
}

/// Replace the retrieval chunks of the given source paths (incremental analysis).
pub async fn replace_chunks(
    pool: &PgPool,
    project_id: Uuid,
    paths: &[String],
    chunks: &[(String, String)],
) -> DbResult<()> {
    let mut tx = pool.begin().await?;
    sqlx::query("DELETE FROM context_chunks WHERE project_id = $1 AND source_path = ANY($2)")
        .bind(project_id)
        .bind(paths)
        .execute(&mut *tx)
        .await?;
    for (path, content) in chunks {
        sqlx::query("INSERT INTO context_chunks (id, project_id, source_path, content) VALUES ($1,$2,$3,$4)")
            .bind(new_id())
            .bind(project_id)
            .bind(path)
            .bind(content)
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;
    Ok(())
}

#[derive(Debug, Clone, FromRow, Serialize, ToSchema)]
pub struct ContextChunk {
    pub source_path: String,
    pub content: String,
    pub rank: f32,
}

/// Full-text retrieval over the project's context chunks.
pub async fn search_chunks(
    pool: &PgPool,
    project_id: Uuid,
    query: &str,
    limit: i64,
) -> DbResult<Vec<ContextChunk>> {
    Ok(sqlx::query_as::<_, ContextChunk>(
        "SELECT source_path, content, ts_rank(tsv, q)::real AS rank FROM context_chunks, websearch_to_tsquery('english', $2) q
         WHERE project_id = $1 AND tsv @@ q ORDER BY rank DESC LIMIT $3",
    )
    .bind(project_id)
    .bind(query)
    .bind(limit)
    .fetch_all(pool)
    .await?)
}

/// Fallback retrieval: any chunk containing one of the given words, preferring ones mentioning more.
pub async fn search_chunks_any(
    pool: &PgPool,
    project_id: Uuid,
    words: &[String],
    limit: i64,
) -> DbResult<Vec<ContextChunk>> {
    let q = words.join(" | ");
    if q.is_empty() {
        return Ok(vec![]);
    }
    Ok(sqlx::query_as::<_, ContextChunk>(
        "SELECT source_path, content, ts_rank(tsv, to_tsquery('english', $2))::real AS rank FROM context_chunks
         WHERE project_id = $1 AND tsv @@ to_tsquery('english', $2) ORDER BY rank DESC LIMIT $3",
    )
    .bind(project_id)
    .bind(q)
    .bind(limit)
    .fetch_all(pool)
    .await
    .unwrap_or_default())
}

// ── requirements ──
#[derive(Debug, Clone, FromRow, Serialize, ToSchema)]
pub struct Requirement {
    pub id: Uuid,
    pub project_id: Uuid,
    pub key: String,
    pub title: String,
    pub body: String,
    pub kind: String,
    pub source: String,
    pub parent_id: Option<Uuid>,
    pub module_id: Option<Uuid>,
    pub priority: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub linked_scenarios: i64,
}

const REQ_SELECT: &str = "SELECT r.id, r.project_id, r.key, r.title, r.body, r.kind, r.source, r.parent_id, r.module_id, r.priority, r.created_at, r.updated_at,
  (SELECT count(*) FROM requirement_links l JOIN test_scenarios s ON s.id = l.scenario_id AND s.deleted_at IS NULL WHERE l.requirement_id = r.id) AS linked_scenarios
  FROM requirements r";

pub async fn list_requirements(pool: &PgPool, project_id: Uuid) -> DbResult<Vec<Requirement>> {
    Ok(sqlx::query_as::<_, Requirement>(&format!(
        "{REQ_SELECT} WHERE r.project_id = $1 AND r.deleted_at IS NULL ORDER BY r.created_at"
    ))
    .bind(project_id)
    .fetch_all(pool)
    .await?)
}

pub async fn get_requirement(pool: &PgPool, project_id: Uuid, id: Uuid) -> DbResult<Requirement> {
    opt(sqlx::query_as::<_, Requirement>(&format!(
        "{REQ_SELECT} WHERE r.id = $1 AND r.project_id = $2 AND r.deleted_at IS NULL"
    ))
    .bind(id)
    .bind(project_id)
    .fetch_one(pool)
    .await)
}

pub async fn next_requirement_key(pool: &PgPool, project_id: Uuid) -> DbResult<String> {
    let n: i64 = sqlx::query_scalar("SELECT count(*) FROM requirements WHERE project_id = $1")
        .bind(project_id)
        .fetch_one(pool)
        .await?;
    Ok(format!("REQ-{}", n + 1))
}

#[allow(clippy::too_many_arguments)]
pub async fn create_requirement(
    pool: &PgPool,
    project_id: Uuid,
    key: Option<&str>,
    title: &str,
    body: &str,
    kind: &str,
    source: &str,
    priority: &str,
    parent: Option<Uuid>,
    user: Option<Uuid>,
) -> DbResult<Requirement> {
    let key = match key {
        Some(k) if !k.trim().is_empty() => k.trim().to_string(),
        _ => next_requirement_key(pool, project_id).await?,
    };
    let id = new_id();
    sqlx::query(
        "INSERT INTO requirements (id, project_id, key, title, body, kind, source, priority, parent_id, created_by) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10)",
    )
    .bind(id)
    .bind(project_id)
    .bind(&key)
    .bind(title)
    .bind(body)
    .bind(kind)
    .bind(source)
    .bind(priority)
    .bind(parent)
    .bind(user)
    .execute(pool)
    .await
    .map_err(|e| DbError::from_sqlx(e, &format!("requirement {key}")))?;
    get_requirement(pool, project_id, id).await
}

pub async fn update_requirement(
    pool: &PgPool,
    project_id: Uuid,
    id: Uuid,
    title: Option<&str>,
    body: Option<&str>,
    kind: Option<&str>,
    priority: Option<&str>,
) -> DbResult<Requirement> {
    let r = sqlx::query(
        "UPDATE requirements SET title = COALESCE($3,title), body = COALESCE($4,body), kind = COALESCE($5,kind), priority = COALESCE($6,priority), updated_at = now()
         WHERE id = $1 AND project_id = $2 AND deleted_at IS NULL",
    )
    .bind(id)
    .bind(project_id)
    .bind(title)
    .bind(body)
    .bind(kind)
    .bind(priority)
    .execute(pool)
    .await?;
    if r.rows_affected() == 0 {
        return Err(DbError::NotFound);
    }
    get_requirement(pool, project_id, id).await
}

pub async fn delete_requirement(pool: &PgPool, project_id: Uuid, id: Uuid) -> DbResult<()> {
    sqlx::query("UPDATE requirements SET deleted_at = now() WHERE id = $1 AND project_id = $2")
        .bind(id)
        .bind(project_id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn link_requirement(
    pool: &PgPool,
    project_id: Uuid,
    requirement_id: Uuid,
    scenario_id: Uuid,
    source: &str,
) -> DbResult<()> {
    let ok: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM requirements r, test_scenarios s WHERE r.id = $1 AND s.id = $2 AND r.project_id = $3 AND s.project_id = $3)",
    )
    .bind(requirement_id)
    .bind(scenario_id)
    .bind(project_id)
    .fetch_one(pool)
    .await?;
    if !ok {
        return Err(DbError::NotFound);
    }
    sqlx::query("INSERT INTO requirement_links (requirement_id, scenario_id, source) VALUES ($1,$2,$3) ON CONFLICT DO NOTHING")
        .bind(requirement_id)
        .bind(scenario_id)
        .bind(source)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn unlink_requirement(
    pool: &PgPool,
    requirement_id: Uuid,
    scenario_id: Uuid,
) -> DbResult<()> {
    sqlx::query("DELETE FROM requirement_links WHERE requirement_id = $1 AND scenario_id = $2")
        .bind(requirement_id)
        .bind(scenario_id)
        .execute(pool)
        .await?;
    Ok(())
}

#[derive(Debug, Clone, FromRow, Serialize, ToSchema)]
pub struct TraceLink {
    pub requirement_id: Uuid,
    pub requirement_key: String,
    pub scenario_id: Uuid,
    pub scenario_name: String,
    pub source: String,
}

pub async fn trace_links(pool: &PgPool, project_id: Uuid) -> DbResult<Vec<TraceLink>> {
    Ok(sqlx::query_as::<_, TraceLink>(
        "SELECT r.id AS requirement_id, r.key AS requirement_key, s.id AS scenario_id, s.name AS scenario_name, l.source
         FROM requirement_links l JOIN requirements r ON r.id = l.requirement_id JOIN test_scenarios s ON s.id = l.scenario_id
         WHERE r.project_id = $1 AND r.deleted_at IS NULL AND s.deleted_at IS NULL ORDER BY r.key, s.name",
    )
    .bind(project_id)
    .fetch_all(pool)
    .await?)
}

// ── test data ──
#[derive(Debug, Clone, FromRow, Serialize, ToSchema)]
pub struct TestData {
    pub id: Uuid,
    pub project_id: Uuid,
    pub name: String,
    pub description: String,
    #[schema(value_type = Object)]
    pub data: Value,
    pub updated_at: DateTime<Utc>,
}

pub async fn list_test_data(pool: &PgPool, project_id: Uuid) -> DbResult<Vec<TestData>> {
    Ok(sqlx::query_as::<_, TestData>("SELECT id, project_id, name, description, data, updated_at FROM test_data WHERE project_id = $1 ORDER BY name")
        .bind(project_id)
        .fetch_all(pool)
        .await?)
}

pub async fn upsert_test_data(
    pool: &PgPool,
    project_id: Uuid,
    name: &str,
    description: &str,
    data: &Value,
) -> DbResult<TestData> {
    Ok(sqlx::query_as::<_, TestData>(
        "INSERT INTO test_data (id, project_id, name, description, data) VALUES ($1,$2,$3,$4,$5)
         ON CONFLICT (project_id, name) DO UPDATE SET description = EXCLUDED.description, data = EXCLUDED.data, updated_at = now()
         RETURNING id, project_id, name, description, data, updated_at",
    )
    .bind(new_id())
    .bind(project_id)
    .bind(name)
    .bind(description)
    .bind(data)
    .fetch_one(pool)
    .await?)
}

pub async fn delete_test_data(pool: &PgPool, project_id: Uuid, id: Uuid) -> DbResult<()> {
    sqlx::query("DELETE FROM test_data WHERE id = $1 AND project_id = $2")
        .bind(id)
        .bind(project_id)
        .execute(pool)
        .await?;
    Ok(())
}

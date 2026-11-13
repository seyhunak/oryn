//! Repository import and analysis pipeline (runs in a background task).

use crate::state::AppState;
use oryn_intelligence::analysis::analyze_dir;
use oryn_intelligence::import::{clone_repo, extract_zip, ZipLimits};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Duration;
use uuid::Uuid;

pub enum Source {
    Git {
        url: url::Url,
        branch: Option<String>,
        token: Option<String>,
    },
    Zip(Vec<u8>),
    /// Analysed in place; never copied or modified.
    Local,
}

pub fn repo_dir(state: &AppState, repo_id: Uuid) -> PathBuf {
    state.cfg.workspace_dir.join(repo_id.to_string())
}

/// Import (clone/extract) then analyse. Status transitions are recorded on the repository row.
pub fn spawn_import(
    state: AppState,
    project_id: Uuid,
    repo_id: Uuid,
    local_path: PathBuf,
    source: Source,
) {
    tokio::spawn(async move {
        let set = |status: &'static str, err: Option<String>| {
            let pool = state.pool.clone();
            async move {
                let _ = oryn_db::projects::set_repository_status(
                    &pool,
                    repo_id,
                    status,
                    err.as_deref(),
                    None,
                    None,
                    None,
                )
                .await;
            }
        };
        set("importing", None).await;
        let commit = match source {
            Source::Git { url, branch, token } => match clone_repo(
                &url,
                branch.as_deref(),
                token.as_deref(),
                &local_path,
                Duration::from_secs(300),
                state.cfg.allow_private_network,
            )
            .await
            {
                Ok(c) => Some(c),
                Err(e) => return set("failed", Some(e.to_string())).await,
            },
            Source::Zip(bytes) => {
                let dest = local_path.clone();
                let res = tokio::task::spawn_blocking(move || {
                    if dest.exists() {
                        let _ = std::fs::remove_dir_all(&dest);
                    }
                    extract_zip(&bytes, &dest, ZipLimits::default())
                })
                .await;
                match res {
                    Ok(Ok(_)) => None,
                    Ok(Err(e)) => return set("failed", Some(e.to_string())).await,
                    Err(e) => {
                        return set("failed", Some(format!("extraction task failed: {e}"))).await
                    }
                }
            }
            Source::Local => None,
        };
        if let Err(e) = analyze(&state, project_id, repo_id, &local_path, commit).await {
            set("failed", Some(e)).await;
        }
    });
}

/// Analyse a repository already on disk and refresh the project's application context.
pub async fn analyze(
    state: &AppState,
    project_id: Uuid,
    repo_id: Uuid,
    path: &std::path::Path,
    commit: Option<String>,
) -> Result<(), String> {
    let pool = &state.pool;
    let _ = oryn_db::projects::set_repository_status(
        pool,
        repo_id,
        "analyzing",
        None,
        commit.as_deref(),
        None,
        None,
    )
    .await;
    let previous: BTreeMap<String, String> = oryn_db::projects::get_context(pool, project_id)
        .await
        .ok()
        .flatten()
        .filter(|c| c.repository_id == Some(repo_id))
        .and_then(|c| serde_json::from_value(c.file_hashes).ok())
        .unwrap_or_default();
    let root = path.to_path_buf();
    let prev = previous.clone();
    let out = tokio::task::spawn_blocking(move || analyze_dir(&root, &prev))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| format!("analysis failed: {e}"))?;

    let analysis = serde_json::to_value(&out.analysis).map_err(|e| e.to_string())?;
    let hashes = serde_json::to_value(&out.hashes).map_err(|e| e.to_string())?;
    let changed = out.analysis.changes.total() as i32;
    oryn_db::projects::upsert_context(
        pool,
        project_id,
        Some(repo_id),
        &out.summary,
        &analysis,
        &hashes,
        commit.as_deref(),
        changed,
    )
    .await
    .map_err(|e| e.to_string())?;
    let modules: Vec<(String, String, String, Option<String>, i32)> = out
        .analysis
        .modules
        .iter()
        .map(|m| {
            (
                m.name.clone(),
                m.path_prefix.clone(),
                m.kind.clone(),
                None,
                m.file_count as i32,
            )
        })
        .collect();
    oryn_db::projects::replace_modules(pool, project_id, &modules)
        .await
        .map_err(|e| e.to_string())?;
    let mut paths: Vec<String> = out.hashes.keys().cloned().collect();
    paths.extend(previous.keys().cloned());
    oryn_db::projects::replace_chunks(pool, project_id, &paths, &out.chunks)
        .await
        .map_err(|e| e.to_string())?;
    oryn_db::projects::set_repository_status(
        pool,
        repo_id,
        "ready",
        None,
        commit.as_deref(),
        Some(out.analysis.total_bytes as i64),
        Some(out.analysis.file_count as i32),
    )
    .await
    .map_err(|e| e.to_string())?;
    tracing::info!(%project_id, %repo_id, files = out.analysis.file_count, changed, "repository analysed");
    Ok(())
}

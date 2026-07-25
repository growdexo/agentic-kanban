use std::path::PathBuf;

use axum::{
    Router,
    extract::{Query, State},
    response::Json as ResponseJson,
    routing::get,
};
use db::models::repo::Repo;
use deployment::Deployment;
use serde::Deserialize;
use services::services::{
    filesystem::{DirectoryEntry, DirectoryListResponse, FilesystemError, FilesystemService},
    worktree_manager::WorktreeManager,
};
use utils::response::ApiResponse;

use crate::{DeploymentImpl, error::ApiError};

#[derive(Debug, Deserialize)]
pub struct ListDirectoryQuery {
    path: Option<String>,
}

async fn filesystem_allowed_roots(deployment: &DeploymentImpl) -> Result<Vec<PathBuf>, ApiError> {
    let mut roots = vec![
        FilesystemService::get_home_directory(),
        WorktreeManager::get_worktree_base_dir(),
        WorktreeManager::get_default_worktree_base_dir(),
        utils::assets::asset_dir(),
        utils::cache_dir(),
    ];

    if let Some(parent) = utils::assets::config_path().parent() {
        roots.push(parent.to_path_buf());
    }

    if let Some(workspace_dir) = deployment.config().read().await.workspace_dir.clone() {
        let workspace_dir = utils::path::expand_tilde(&workspace_dir);
        roots.push(workspace_dir.clone());
        roots.push(workspace_dir.join(".agentic-kanban-workspaces"));
    }

    roots.extend(
        Repo::list_all(&deployment.db().pool)
            .await?
            .into_iter()
            .map(|repo| repo.path),
    );

    roots.retain(|path| path.exists());
    roots.sort();
    roots.dedup();
    Ok(roots)
}

fn filesystem_error_response<T>(
    err: FilesystemError,
) -> Result<ResponseJson<ApiResponse<T>>, ApiError> {
    match err {
        FilesystemError::DirectoryDoesNotExist => {
            Ok(ResponseJson(ApiResponse::error("Directory does not exist")))
        }
        FilesystemError::PathIsNotDirectory => {
            Ok(ResponseJson(ApiResponse::error("Path is not a directory")))
        }
        FilesystemError::AccessDenied => Err(ApiError::Forbidden(
            "Path is outside allowed directories".to_string(),
        )),
        FilesystemError::Io(e) => {
            tracing::error!("Failed to read directory: {}", e);
            Ok(ResponseJson(ApiResponse::error(&format!(
                "Failed to read directory: {}",
                e
            ))))
        }
    }
}

pub async fn list_directory(
    State(deployment): State<DeploymentImpl>,
    Query(query): Query<ListDirectoryQuery>,
) -> Result<ResponseJson<ApiResponse<DirectoryListResponse>>, ApiError> {
    let allowed_roots = filesystem_allowed_roots(&deployment).await?;
    match deployment
        .filesystem()
        .list_directory_with_allowlist(query.path, &allowed_roots)
        .await
    {
        Ok(response) => Ok(ResponseJson(ApiResponse::success(response))),
        Err(err) => filesystem_error_response(err),
    }
}

pub async fn list_git_repos(
    State(deployment): State<DeploymentImpl>,
    Query(query): Query<ListDirectoryQuery>,
) -> Result<ResponseJson<ApiResponse<Vec<DirectoryEntry>>>, ApiError> {
    let res = if let Some(ref path) = query.path {
        let allowed_roots = filesystem_allowed_roots(&deployment).await?;
        deployment
            .filesystem()
            .list_git_repos_with_allowlist(path.clone(), &allowed_roots, 800, 1200, Some(3))
            .await
    } else {
        deployment
            .filesystem()
            .list_common_git_repos(800, 1200, Some(4))
            .await
    };
    match res {
        Ok(response) => Ok(ResponseJson(ApiResponse::success(response))),
        Err(err) => filesystem_error_response(err),
    }
}

pub fn router() -> Router<DeploymentImpl> {
    Router::new()
        .route("/filesystem/directory", get(list_directory))
        .route("/filesystem/git-repos", get(list_git_repos))
}

use std::path::Path;

use axum::{Extension, Json, extract::State, response::Json as ResponseJson};
use db::models::{
    merge::{Merge, MergeStatus},
    repo::{Repo, RepoError},
    task::{Task, TaskStatus},
    workspace::{Workspace, WorkspaceError},
    workspace_repo::WorkspaceRepo,
};
use deployment::Deployment;
use git::{GitCliError, GitServiceError};
use git2::BranchType;
use serde::{Deserialize, Serialize};
use services::services::container::ContainerService;
use ts_rs::TS;
use utils::response::ApiResponse;
use uuid::Uuid;

use crate::{DeploymentImpl, error::ApiError};

#[derive(Debug, Deserialize, Serialize, TS)]
pub struct MergeTaskAttemptRequest {
    pub repo_id: Uuid,
}

#[derive(Debug, Deserialize, Serialize, TS)]
pub struct PushTaskAttemptRequest {
    pub repo_id: Uuid,
}

#[axum::debug_handler]
pub async fn merge_task_attempt(
    Extension(workspace): Extension<Workspace>,
    State(deployment): State<DeploymentImpl>,
    Json(request): Json<MergeTaskAttemptRequest>,
) -> Result<ResponseJson<ApiResponse<()>>, ApiError> {
    let pool = &deployment.db().pool;

    let workspace_repo =
        WorkspaceRepo::find_by_workspace_and_repo_id(pool, workspace.id, request.repo_id)
            .await?
            .ok_or(RepoError::NotFound)?;

    let repo = Repo::find_by_id(pool, workspace_repo.repo_id)
        .await?
        .ok_or(RepoError::NotFound)?;

    // Prevent direct merge when there's an open PR for this repo
    let merges = Merge::find_by_workspace_and_repo_id(pool, workspace.id, request.repo_id).await?;
    let has_open_pr = merges
        .iter()
        .any(|m| matches!(m, Merge::Pr(pr) if matches!(pr.pr_info.status, MergeStatus::Open)));
    if has_open_pr {
        return Err(ApiError::BadRequest(
            "Cannot merge directly when a pull request is open for this repository.".to_string(),
        ));
    }

    // Prevent direct merge into remote branches - users must create a PR instead
    let target_branch_type = deployment
        .git()
        .find_branch_type(&repo.path, &workspace_repo.target_branch)?;
    if target_branch_type == BranchType::Remote {
        return Err(ApiError::BadRequest(
            "Cannot merge directly into a remote branch. Please create a pull request instead."
                .to_string(),
        ));
    }

    let container_ref = deployment
        .container()
        .ensure_container_exists(&workspace)
        .await?;
    let workspace_path = Path::new(&container_ref);
    let worktree_path = workspace_path.join(repo.name);

    let task = workspace
        .parent_task(pool)
        .await?
        .ok_or(ApiError::Workspace(WorkspaceError::TaskNotFound))?;
    let task_uuid_str = task.id.to_string();
    let first_uuid_section = task_uuid_str.split('-').next().unwrap_or(&task_uuid_str);

    let mut commit_message = format!("{} (vibe-kanban {})", task.title, first_uuid_section);

    // Add description on next line if it exists
    if let Some(description) = &task.description
        && !description.trim().is_empty()
    {
        commit_message.push_str("\n\n");
        commit_message.push_str(description);
    }

    let merge_commit_id = deployment.git().merge_changes(
        &repo.path,
        &worktree_path,
        &workspace.branch,
        &workspace_repo.target_branch,
        &commit_message,
    )?;

    Merge::create_direct(
        pool,
        workspace.id,
        workspace_repo.repo_id,
        &workspace_repo.target_branch,
        &merge_commit_id,
    )
    .await?;
    Task::update_status(pool, task.id, TaskStatus::Done).await?;
    if !workspace.pinned
        && let Err(e) = deployment.container().archive_workspace(workspace.id).await
    {
        tracing::error!("Failed to archive workspace {}: {}", workspace.id, e);
    }

    deployment
        .track_if_analytics_allowed(
            "task_attempt_merged",
            serde_json::json!({
                "task_id": task.id.to_string(),
                "workspace_id": workspace.id.to_string(),
            }),
        )
        .await;

    Ok(ResponseJson(ApiResponse::success(())))
}

pub async fn push_task_attempt_branch(
    Extension(workspace): Extension<Workspace>,
    State(deployment): State<DeploymentImpl>,
    Json(request): Json<PushTaskAttemptRequest>,
) -> Result<ResponseJson<ApiResponse<(), PushError>>, ApiError> {
    let pool = &deployment.db().pool;

    let workspace_repo =
        WorkspaceRepo::find_by_workspace_and_repo_id(pool, workspace.id, request.repo_id)
            .await?
            .ok_or(RepoError::NotFound)?;

    let repo = Repo::find_by_id(pool, workspace_repo.repo_id)
        .await?
        .ok_or(RepoError::NotFound)?;

    let container_ref = deployment
        .container()
        .ensure_container_exists(&workspace)
        .await?;
    let workspace_path = Path::new(&container_ref);
    let worktree_path = workspace_path.join(&repo.name);

    match deployment
        .git()
        .push_to_remote(&worktree_path, &workspace.branch, false)
    {
        Ok(_) => Ok(ResponseJson(ApiResponse::success(()))),
        Err(GitServiceError::GitCLI(GitCliError::PushRejected(_))) => Ok(ResponseJson(
            ApiResponse::error_with_data(PushError::ForcePushRequired),
        )),
        Err(e) => Err(ApiError::GitService(e)),
    }
}

pub async fn force_push_task_attempt_branch(
    Extension(workspace): Extension<Workspace>,
    State(deployment): State<DeploymentImpl>,
    Json(request): Json<PushTaskAttemptRequest>,
) -> Result<ResponseJson<ApiResponse<(), PushError>>, ApiError> {
    let pool = &deployment.db().pool;

    let workspace_repo =
        WorkspaceRepo::find_by_workspace_and_repo_id(pool, workspace.id, request.repo_id)
            .await?
            .ok_or(RepoError::NotFound)?;

    let repo = Repo::find_by_id(pool, workspace_repo.repo_id)
        .await?
        .ok_or(RepoError::NotFound)?;

    let container_ref = deployment
        .container()
        .ensure_container_exists(&workspace)
        .await?;
    let workspace_path = Path::new(&container_ref);
    let worktree_path = workspace_path.join(&repo.name);

    deployment
        .git()
        .push_to_remote(&worktree_path, &workspace.branch, true)?;
    Ok(ResponseJson(ApiResponse::success(())))
}

#[derive(Debug, Serialize, Deserialize, TS)]
#[serde(tag = "type", rename_all = "snake_case")]
#[ts(tag = "type", rename_all = "snake_case")]
pub enum PushError {
    ForcePushRequired,
}

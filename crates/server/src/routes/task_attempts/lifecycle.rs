use std::path::PathBuf;

use api_types::CreateWorkspaceRequest;
use axum::{
    Extension, Json,
    extract::{Path as AxumPath, Query, State},
    http::StatusCode,
    response::Json as ResponseJson,
};
use db::models::{
    coding_agent_turn::CodingAgentTurn,
    execution_process::{ExecutionProcess, ExecutionProcessStatus},
    repo::{Repo, RepoError},
    task::{Task, TaskRelationships},
    workspace::{CreateWorkspace, Workspace, WorkspaceError},
    workspace_repo::{CreateWorkspaceRepo, RepoWithTargetBranch, WorkspaceRepo},
};
use deployment::Deployment;
use executors::profile::ExecutorProfileId;
use git::GitService;
use serde::{Deserialize, Serialize};
use services::services::{
    container::ContainerService, diff_stream, remote_client::RemoteClientError, remote_sync,
    workspace_manager::WorkspaceManager,
};
use sqlx::Error as SqlxError;
use ts_rs::TS;
use utils::response::ApiResponse;
use uuid::Uuid;

use crate::{DeploymentImpl, error::ApiError};

#[derive(Debug, Deserialize)]
pub struct TaskAttemptQuery {
    pub task_id: Option<Uuid>,
}

#[derive(Debug, Deserialize, TS)]
pub struct UpdateWorkspace {
    pub archived: Option<bool>,
    pub pinned: Option<bool>,
    pub name: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct DeleteWorkspaceQuery {
    #[serde(default)]
    pub delete_remote: bool,
    #[serde(default)]
    pub delete_branches: bool,
}

#[derive(Debug, Deserialize)]
pub struct LinkWorkspaceRequest {
    pub project_id: Uuid,
    pub issue_id: Uuid,
}

pub async fn get_task_attempts(
    State(deployment): State<DeploymentImpl>,
    Query(query): Query<TaskAttemptQuery>,
) -> Result<ResponseJson<ApiResponse<Vec<Workspace>>>, ApiError> {
    let pool = &deployment.db().pool;
    let workspaces = Workspace::fetch_all(pool, query.task_id).await?;
    Ok(ResponseJson(ApiResponse::success(workspaces)))
}

pub async fn get_workspace_count(
    State(deployment): State<DeploymentImpl>,
) -> Result<ResponseJson<ApiResponse<i64>>, ApiError> {
    let pool = &deployment.db().pool;
    let count = Workspace::count_all(pool).await?;
    Ok(ResponseJson(ApiResponse::success(count)))
}

pub async fn get_task_attempt(
    Extension(workspace): Extension<Workspace>,
) -> Result<ResponseJson<ApiResponse<Workspace>>, ApiError> {
    Ok(ResponseJson(ApiResponse::success(workspace)))
}

pub async fn update_workspace(
    Extension(workspace): Extension<Workspace>,
    State(deployment): State<DeploymentImpl>,
    Json(request): Json<UpdateWorkspace>,
) -> Result<ResponseJson<ApiResponse<Workspace>>, ApiError> {
    let pool = &deployment.db().pool;
    let is_archiving = request.archived == Some(true) && !workspace.archived;

    Workspace::update(
        pool,
        workspace.id,
        request.archived,
        request.pinned,
        request.name.as_deref(),
    )
    .await?;
    let updated = Workspace::find_by_id(pool, workspace.id)
        .await?
        .ok_or(WorkspaceError::TaskNotFound)?;

    // Sync to remote if archived or name changed
    if (request.archived.is_some() || request.name.is_some())
        && let Ok(client) = deployment.remote_client()
    {
        let ws = updated.clone();
        let name = request.name.clone();
        let archived = request.archived;
        let stats =
            diff_stream::compute_diff_stats(&deployment.db().pool, deployment.git(), &ws).await;
        tokio::spawn(async move {
            remote_sync::sync_workspace_to_remote(
                &client,
                ws.id,
                name.map(Some),
                archived,
                stats.as_ref(),
            )
            .await;
        });
    }

    if is_archiving && let Err(e) = deployment.container().archive_workspace(workspace.id).await {
        tracing::error!("Failed to archive workspace {}: {}", workspace.id, e);
    }

    Ok(ResponseJson(ApiResponse::success(updated)))
}

#[derive(Debug, Serialize, Deserialize, ts_rs::TS)]
pub struct CreateTaskAttemptBody {
    pub task_id: Uuid,
    pub executor_profile_id: ExecutorProfileId,
    pub repos: Vec<WorkspaceRepoInput>,
}

#[derive(Debug, Serialize, Deserialize, ts_rs::TS)]
pub struct WorkspaceRepoInput {
    pub repo_id: Uuid,
    pub target_branch: String,
}

#[axum::debug_handler]
pub async fn create_task_attempt(
    State(deployment): State<DeploymentImpl>,
    Json(payload): Json<CreateTaskAttemptBody>,
) -> Result<ResponseJson<ApiResponse<Workspace>>, ApiError> {
    let executor_profile_id = payload.executor_profile_id.clone();

    if payload.repos.is_empty() {
        return Err(ApiError::BadRequest(
            "At least one repository is required".to_string(),
        ));
    }

    let pool = &deployment.db().pool;
    let task = Task::find_by_id(&deployment.db().pool, payload.task_id)
        .await?
        .ok_or(SqlxError::RowNotFound)?;

    // Compute agent_working_dir based on repo count:
    // - Single repo: join repo name with default_working_dir (if set), or just repo name
    // - Multiple repos: use None (agent runs in workspace root)
    let agent_working_dir = if payload.repos.len() == 1 {
        let repo = Repo::find_by_id(pool, payload.repos[0].repo_id)
            .await?
            .ok_or(RepoError::NotFound)?;
        match repo.default_working_dir {
            Some(subdir) => {
                let path = PathBuf::from(&repo.name).join(&subdir);
                Some(path.to_string_lossy().to_string())
            }
            None => Some(repo.name),
        }
    } else {
        None
    };

    let attempt_id = Uuid::new_v4();
    let git_branch_name = deployment
        .container()
        .git_branch_from_workspace(&attempt_id, &task.title)
        .await;

    let workspace = Workspace::create(
        pool,
        &CreateWorkspace {
            branch: git_branch_name.clone(),
            agent_working_dir,
        },
        attempt_id,
        payload.task_id,
    )
    .await?;

    let workspace_repos: Vec<CreateWorkspaceRepo> = payload
        .repos
        .iter()
        .map(|r| CreateWorkspaceRepo {
            repo_id: r.repo_id,
            target_branch: r.target_branch.clone(),
        })
        .collect();

    WorkspaceRepo::create_many(pool, workspace.id, &workspace_repos).await?;
    if let Err(err) = deployment
        .container()
        .start_workspace(&workspace, executor_profile_id.clone())
        .await
    {
        tracing::error!("Failed to start task attempt: {}", err);
    }

    deployment
        .track_if_analytics_allowed(
            "task_attempt_started",
            serde_json::json!({
                "task_id": workspace.task_id.to_string(),
                "variant": &executor_profile_id.variant,
                "executor": &executor_profile_id.executor,
                "workspace_id": workspace.id.to_string(),
                "repository_count": payload.repos.len(),
            }),
        )
        .await;

    tracing::info!("Created attempt for task {}", task.id);

    Ok(ResponseJson(ApiResponse::success(workspace)))
}

pub async fn get_task_attempt_children(
    Extension(workspace): Extension<Workspace>,
    State(deployment): State<DeploymentImpl>,
) -> Result<ResponseJson<ApiResponse<TaskRelationships>>, StatusCode> {
    match Task::find_relationships_for_workspace(&deployment.db().pool, &workspace).await {
        Ok(relationships) => {
            deployment
                .track_if_analytics_allowed(
                    "task_attempt_children_viewed",
                    serde_json::json!({
                        "workspace_id": workspace.id.to_string(),
                        "children_count": relationships.children.len(),
                        "parent_count": if relationships.parent_task.is_some() { 1 } else { 0 },
                    }),
                )
                .await;

            Ok(ResponseJson(ApiResponse::success(relationships)))
        }
        Err(e) => {
            tracing::error!(
                "Failed to fetch relationships for task attempt {}: {}",
                workspace.id,
                e
            );
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

pub async fn get_task_attempt_repos(
    Extension(workspace): Extension<Workspace>,
    State(deployment): State<DeploymentImpl>,
) -> Result<ResponseJson<ApiResponse<Vec<RepoWithTargetBranch>>>, ApiError> {
    let pool = &deployment.db().pool;

    let repos =
        WorkspaceRepo::find_repos_with_target_branch_for_workspace(pool, workspace.id).await?;

    Ok(ResponseJson(ApiResponse::success(repos)))
}

pub async fn get_first_user_message(
    Extension(workspace): Extension<Workspace>,
    State(deployment): State<DeploymentImpl>,
) -> Result<ResponseJson<ApiResponse<Option<String>>>, ApiError> {
    let pool = &deployment.db().pool;

    let message = Workspace::get_first_user_message(pool, workspace.id).await?;

    Ok(ResponseJson(ApiResponse::success(message)))
}

pub async fn delete_workspace(
    Extension(workspace): Extension<Workspace>,
    State(deployment): State<DeploymentImpl>,
    Query(query): Query<DeleteWorkspaceQuery>,
) -> Result<(StatusCode, ResponseJson<ApiResponse<()>>), ApiError> {
    let pool = &deployment.db().pool;

    // Check for running execution processes
    if ExecutionProcess::has_running_non_dev_server_processes_for_workspace(pool, workspace.id)
        .await?
    {
        return Err(ApiError::Conflict(
            "Cannot delete workspace while processes are running. Stop all processes first."
                .to_string(),
        ));
    }

    // Stop any running dev servers for this workspace
    let dev_servers =
        ExecutionProcess::find_running_dev_servers_by_workspace(pool, workspace.id).await?;

    for dev_server in dev_servers {
        tracing::info!(
            "Stopping dev server {} before deleting workspace {}",
            dev_server.id,
            workspace.id
        );

        if let Err(e) = deployment
            .container()
            .stop_execution(&dev_server, ExecutionProcessStatus::Killed)
            .await
        {
            tracing::error!(
                "Failed to stop dev server {} for workspace {}: {}",
                dev_server.id,
                workspace.id,
                e
            );
        }
    }

    // Gather data needed for background cleanup
    let workspace_dir = workspace.container_ref.clone().map(PathBuf::from);
    let repositories = WorkspaceRepo::find_repos_for_workspace(pool, workspace.id).await?;

    // Nullify parent_workspace_id for any child tasks before deletion
    let children_affected = Task::nullify_children_by_workspace_id(pool, workspace.id).await?;
    if children_affected > 0 {
        tracing::info!(
            "Nullified {} child task references before deleting workspace {}",
            children_affected,
            workspace.id
        );
    }

    // Delete workspace from database (FK CASCADE will handle sessions, execution_processes, etc.)
    let rows_affected = Workspace::delete(pool, workspace.id).await?;

    if rows_affected == 0 {
        return Err(ApiError::Database(SqlxError::RowNotFound));
    }

    deployment
        .track_if_analytics_allowed(
            "workspace_deleted",
            serde_json::json!({
                "workspace_id": workspace.id.to_string(),
                "task_id": workspace.task_id.to_string(),
            }),
        )
        .await;

    // Attempt remote workspace deletion if requested
    if query.delete_remote {
        if let Ok(client) = deployment.remote_client() {
            match client.delete_workspace(workspace.id).await {
                Ok(()) => {
                    tracing::info!("Deleted remote workspace for {}", workspace.id);
                }
                Err(e) => {
                    tracing::warn!(
                        "Failed to delete remote workspace for {}: {}",
                        workspace.id,
                        e
                    );
                }
            }
        } else {
            tracing::debug!(
                "Remote client not available, skipping remote deletion for {}",
                workspace.id
            );
        }
    }

    // Spawn background cleanup task for filesystem resources
    if let Some(workspace_dir) = workspace_dir {
        let workspace_id = workspace.id;
        let delete_branches = query.delete_branches;
        let branch_name = workspace.branch.clone();
        let repo_paths: Vec<PathBuf> = repositories.iter().map(|r| r.path.clone()).collect();

        tokio::spawn(async move {
            tracing::info!(
                "Starting background cleanup for workspace {} at {}",
                workspace_id,
                workspace_dir.display()
            );

            if let Err(e) = WorkspaceManager::cleanup_workspace(&workspace_dir, &repositories).await
            {
                tracing::error!(
                    "Background workspace cleanup failed for {} at {}: {}",
                    workspace_id,
                    workspace_dir.display(),
                    e
                );
            } else {
                tracing::info!(
                    "Background cleanup completed for workspace {}",
                    workspace_id
                );
            }

            if delete_branches {
                let git_service = GitService::new();
                for repo_path in repo_paths {
                    match git_service.delete_branch(&repo_path, &branch_name) {
                        Ok(()) => {
                            tracing::info!(
                                "Deleted branch '{}' from repo {:?}",
                                branch_name,
                                repo_path
                            );
                        }
                        Err(e) => {
                            tracing::warn!(
                                "Failed to delete branch '{}' from repo {:?}: {}",
                                branch_name,
                                repo_path,
                                e
                            );
                        }
                    }
                }
            }
        });
    }

    // Return 202 Accepted to indicate deletion was scheduled
    Ok((StatusCode::ACCEPTED, ResponseJson(ApiResponse::success(()))))
}

/// Mark all coding agent turns for a workspace as seen
#[axum::debug_handler]
pub async fn mark_seen(
    Extension(workspace): Extension<Workspace>,
    State(deployment): State<DeploymentImpl>,
) -> Result<ResponseJson<ApiResponse<()>>, ApiError> {
    let pool = &deployment.db().pool;

    CodingAgentTurn::mark_seen_by_workspace_id(pool, workspace.id).await?;

    Ok(ResponseJson(ApiResponse::success(())))
}

pub async fn link_workspace(
    Extension(workspace): Extension<Workspace>,
    State(deployment): State<DeploymentImpl>,
    Json(payload): Json<LinkWorkspaceRequest>,
) -> Result<ResponseJson<ApiResponse<()>>, ApiError> {
    let client = deployment.remote_client()?;

    let stats =
        diff_stream::compute_diff_stats(&deployment.db().pool, deployment.git(), &workspace).await;

    client
        .create_workspace(CreateWorkspaceRequest {
            project_id: payload.project_id,
            local_workspace_id: workspace.id,
            issue_id: payload.issue_id,
            name: workspace.name.clone(),
            archived: Some(workspace.archived),
            files_changed: stats.as_ref().map(|s| s.files_changed as i32),
            lines_added: stats.as_ref().map(|s| s.lines_added as i32),
            lines_removed: stats.as_ref().map(|s| s.lines_removed as i32),
        })
        .await?;

    Ok(ResponseJson(ApiResponse::success(())))
}

/// Unlinks a local workspace from the remote server by deleting the remote workspace.
pub async fn unlink_workspace(
    AxumPath(workspace_id): AxumPath<uuid::Uuid>,
    State(deployment): State<DeploymentImpl>,
) -> Result<ResponseJson<ApiResponse<()>>, ApiError> {
    let client = deployment.remote_client()?;

    match client.delete_workspace(workspace_id).await {
        Ok(()) => Ok(ResponseJson(ApiResponse::success(()))),
        Err(RemoteClientError::Http { status: 404, .. }) => {
            Ok(ResponseJson(ApiResponse::success(())))
        }
        Err(e) => Err(e.into()),
    }
}

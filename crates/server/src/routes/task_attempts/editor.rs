use std::path::Path;

use axum::{Extension, Json, extract::State, response::Json as ResponseJson};
use db::models::{workspace::Workspace, workspace_repo::WorkspaceRepo};
use deployment::Deployment;
use serde::Serialize;
use services::services::container::ContainerService;
use ts_rs::TS;
use utils::response::ApiResponse;

use crate::{DeploymentImpl, error::ApiError};

#[derive(serde::Deserialize, TS)]
pub struct OpenEditorRequest {
    editor_type: Option<String>,
    file_path: Option<String>,
}

#[derive(Debug, Serialize, TS)]
pub struct OpenEditorResponse {
    pub url: Option<String>,
}

pub async fn open_task_attempt_in_editor(
    Extension(workspace): Extension<Workspace>,
    State(deployment): State<DeploymentImpl>,
    Json(payload): Json<OpenEditorRequest>,
) -> Result<ResponseJson<ApiResponse<OpenEditorResponse>>, ApiError> {
    let container_ref = deployment
        .container()
        .ensure_container_exists(&workspace)
        .await?;

    Workspace::touch(&deployment.db().pool, workspace.id).await?;

    let workspace_path = Path::new(&container_ref);

    // For single-repo projects, open from the repo directory
    let workspace_repos =
        WorkspaceRepo::find_repos_for_workspace(&deployment.db().pool, workspace.id).await?;
    let workspace_path = if workspace_repos.len() == 1 && payload.file_path.is_none() {
        workspace_path.join(&workspace_repos[0].name)
    } else {
        workspace_path.to_path_buf()
    };

    // If a specific file path is provided, use it; otherwise use the base path
    let path = if let Some(file_path) = payload.file_path.as_ref() {
        workspace_path.join(file_path)
    } else {
        workspace_path
    };

    let editor_config = {
        let config = deployment.config().read().await;
        let editor_type_str = payload.editor_type.as_deref();
        config.editor.with_override(editor_type_str)
    };

    match editor_config.open_file(path.as_path()).await {
        Ok(url) => {
            tracing::info!(
                "Opened editor for task attempt {} at path: {}{}",
                workspace.id,
                path.display(),
                if url.is_some() { " (remote mode)" } else { "" }
            );

            deployment
                .track_if_analytics_allowed(
                    "task_attempt_editor_opened",
                    serde_json::json!({
                        "workspace_id": workspace.id.to_string(),
                        "editor_type": payload.editor_type.as_ref(),
                        "remote_mode": url.is_some(),
                    }),
                )
                .await;

            Ok(ResponseJson(ApiResponse::success(OpenEditorResponse {
                url,
            })))
        }
        Err(e) => {
            tracing::error!(
                "Failed to open editor for attempt {}: {:?}",
                workspace.id,
                e
            );
            Err(ApiError::EditorOpen(e))
        }
    }
}

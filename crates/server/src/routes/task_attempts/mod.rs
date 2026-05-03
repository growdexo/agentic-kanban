pub mod codex_setup;
pub mod cursor_setup;
pub mod gh_cli_setup;
pub mod images;
pub mod pr;
pub mod workspace_summary;

mod branch;
mod diff;
mod editor;
mod execution;
mod lifecycle;
mod merge;

pub use branch::{
    AbortConflictsRequest, BranchStatus, ChangeTargetBranchRequest, ChangeTargetBranchResponse,
    ContinueRebaseRequest, GitOperationError, RebaseTaskAttemptRequest, RenameBranchError,
    RenameBranchRequest, RenameBranchResponse, RepoBranchStatus, abort_conflicts_task_attempt,
    change_target_branch, continue_rebase_task_attempt, get_task_attempt_branch_status,
    rebase_task_attempt, rename_branch,
};
pub use diff::{
    DiffStreamQuery, WorkspaceStreamQuery, stream_task_attempt_diff_ws, stream_workspaces_ws,
};
pub use editor::{OpenEditorRequest, OpenEditorResponse, open_task_attempt_in_editor};
pub use execution::{
    RunAgentSetupRequest, RunAgentSetupResponse, RunScriptError, gh_cli_setup_handler,
    run_agent_setup, run_archive_script, run_cleanup_script, run_setup_script, start_dev_server,
    stop_task_attempt_execution,
};
pub use lifecycle::{
    CreateTaskAttemptBody, DeleteWorkspaceQuery, LinkWorkspaceRequest, TaskAttemptQuery,
    UpdateWorkspace, WorkspaceRepoInput, create_task_attempt, delete_workspace,
    get_first_user_message, get_task_attempt, get_task_attempt_children, get_task_attempt_repos,
    get_task_attempts, get_workspace_count, link_workspace, mark_seen, unlink_workspace,
    update_workspace,
};
pub use merge::{
    MergeTaskAttemptRequest, PushError, PushTaskAttemptRequest, force_push_task_attempt_branch,
    merge_task_attempt, push_task_attempt_branch,
};

use axum::{
    Router,
    middleware::from_fn_with_state,
    routing::{get, post, put},
};

use crate::{DeploymentImpl, middleware::load_workspace_middleware};

pub fn router(deployment: &DeploymentImpl) -> Router<DeploymentImpl> {
    let task_attempt_id_router = Router::new()
        .route("/unlink", post(unlink_workspace))
        .merge(
            Router::new()
                .route(
                    "/",
                    get(get_task_attempt)
                        .put(update_workspace)
                        .delete(delete_workspace),
                )
                .route("/run-agent-setup", post(run_agent_setup))
                .route("/gh-cli-setup", post(gh_cli_setup_handler))
                .route("/start-dev-server", post(start_dev_server))
                .route("/run-setup-script", post(run_setup_script))
                .route("/run-cleanup-script", post(run_cleanup_script))
                .route("/run-archive-script", post(run_archive_script))
                .route("/branch-status", get(get_task_attempt_branch_status))
                .route("/diff/ws", get(stream_task_attempt_diff_ws))
                .route("/merge", post(merge_task_attempt))
                .route("/push", post(push_task_attempt_branch))
                .route("/push/force", post(force_push_task_attempt_branch))
                .route("/rebase", post(rebase_task_attempt))
                .route("/rebase/continue", post(continue_rebase_task_attempt))
                .route("/conflicts/abort", post(abort_conflicts_task_attempt))
                .route("/pr", post(pr::create_pr))
                .route("/pr/attach", post(pr::attach_existing_pr))
                .route("/pr/comments", get(pr::get_pr_comments))
                .route("/open-editor", post(open_task_attempt_in_editor))
                .route("/children", get(get_task_attempt_children))
                .route("/stop", post(stop_task_attempt_execution))
                .route("/change-target-branch", post(change_target_branch))
                .route("/rename-branch", post(rename_branch))
                .route("/repos", get(get_task_attempt_repos))
                .route("/first-message", get(get_first_user_message))
                .route("/mark-seen", put(mark_seen))
                .route("/link", post(link_workspace))
                .layer(from_fn_with_state(
                    deployment.clone(),
                    load_workspace_middleware,
                )),
        );

    let task_attempts_router = Router::new()
        .route("/", get(get_task_attempts).post(create_task_attempt))
        .route("/from-pr", post(pr::create_workspace_from_pr))
        .route("/count", get(get_workspace_count))
        .route("/stream/ws", get(stream_workspaces_ws))
        .route("/summary", post(workspace_summary::get_workspace_summaries))
        .nest("/{id}", task_attempt_id_router)
        .nest("/{id}/images", images::router(deployment));

    Router::new().nest("/task-attempts", task_attempts_router)
}

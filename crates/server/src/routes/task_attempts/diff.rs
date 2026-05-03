use axum::{
    Extension,
    extract::{
        Query, State,
        ws::{WebSocket, WebSocketUpgrade},
    },
    response::IntoResponse,
};
use db::models::workspace::Workspace;
use deployment::Deployment;
use serde::Deserialize;
use services::services::container::ContainerService;

use crate::DeploymentImpl;

#[derive(Debug, Deserialize)]
pub struct DiffStreamQuery {
    #[serde(default)]
    pub stats_only: bool,
}

#[derive(Debug, Deserialize)]
pub struct WorkspaceStreamQuery {
    pub archived: Option<bool>,
    pub limit: Option<i64>,
}

#[axum::debug_handler]
pub async fn stream_task_attempt_diff_ws(
    ws: WebSocketUpgrade,
    Query(params): Query<DiffStreamQuery>,
    Extension(workspace): Extension<Workspace>,
    State(deployment): State<DeploymentImpl>,
) -> impl IntoResponse {
    let _ = Workspace::touch(&deployment.db().pool, workspace.id).await;

    let stats_only = params.stats_only;
    ws.on_upgrade(move |socket| async move {
        if let Err(e) = handle_task_attempt_diff_ws(socket, deployment, workspace, stats_only).await
        {
            tracing::warn!("diff WS closed: {}", e);
        }
    })
}

async fn handle_task_attempt_diff_ws(
    socket: WebSocket,
    deployment: DeploymentImpl,
    workspace: Workspace,
    stats_only: bool,
) -> anyhow::Result<()> {
    use futures_util::{SinkExt, StreamExt, TryStreamExt};
    use utils::log_msg::LogMsg;

    let stream = deployment
        .container()
        .stream_diff(&workspace, stats_only)
        .await?;

    let mut stream = stream.map_ok(|msg: LogMsg| msg.to_ws_message_unchecked());

    let (mut sender, mut receiver) = socket.split();

    loop {
        tokio::select! {
            // Wait for next stream item
            item = stream.next() => {
                match item {
                    Some(Ok(msg)) => {
                        if sender.send(msg).await.is_err() {
                            break;
                        }
                    }
                    Some(Err(e)) => {
                        tracing::error!("stream error: {}", e);
                        break;
                    }
                    None => break,
                }
            }
            // Detect client disconnection
            msg = receiver.next() => {
                if msg.is_none() {
                    break;
                }
            }
        }
    }
    Ok(())
}

pub async fn stream_workspaces_ws(
    ws: WebSocketUpgrade,
    Query(query): Query<WorkspaceStreamQuery>,
    State(deployment): State<DeploymentImpl>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| async move {
        if let Err(e) = handle_workspaces_ws(socket, deployment, query.archived, query.limit).await
        {
            tracing::warn!("workspaces WS closed: {}", e);
        }
    })
}

async fn handle_workspaces_ws(
    socket: WebSocket,
    deployment: DeploymentImpl,
    archived: Option<bool>,
    limit: Option<i64>,
) -> anyhow::Result<()> {
    use futures_util::{SinkExt, StreamExt, TryStreamExt};

    let mut stream = deployment
        .events()
        .stream_workspaces_raw(archived, limit)
        .await?
        .map_ok(|msg| msg.to_ws_message_unchecked());

    let (mut sender, mut receiver) = socket.split();

    loop {
        tokio::select! {
            item = stream.next() => {
                match item {
                    Some(Ok(msg)) => {
                        if sender.send(msg).await.is_err() {
                            break;
                        }
                    }
                    Some(Err(e)) => {
                        tracing::error!("stream error: {}", e);
                        break;
                    }
                    None => break,
                }
            }
            msg = receiver.next() => {
                if msg.is_none() {
                    break;
                }
            }
        }
    }
    Ok(())
}

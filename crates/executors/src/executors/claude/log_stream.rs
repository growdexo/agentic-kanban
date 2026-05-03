use std::{path::Path, sync::Arc};

use futures::StreamExt;
use workspace_utils::{log_msg::LogMsg, msg_store::MsgStore};

use super::{
    json::ClaudeJson,
    normalize_logs::{ClaudeLogProcessor, HistoryStrategy},
};
use crate::logs::{
    NormalizedEntry, NormalizedEntryType,
    utils::{EntryIndexProvider, patch::ConversationPatch},
};

impl ClaudeLogProcessor {
    /// Process raw logs and convert them to normalized entries with patches
    pub fn process_logs(
        msg_store: Arc<MsgStore>,
        current_dir: &Path,
        entry_index_provider: EntryIndexProvider,
        strategy: HistoryStrategy,
    ) {
        let current_dir_clone = current_dir.to_owned();
        tokio::spawn(async move {
            let mut stream = msg_store.history_plus_stream();
            let mut buffer = String::new();
            let worktree_path = current_dir_clone.to_string_lossy().to_string();
            let mut session_id_extracted = false;
            let mut processor = Self::new_with_strategy(strategy);
            // Track pending assistant UUID - only committed when we see a Result message
            let mut pending_assistant_uuid: Option<String> = None;

            while let Some(Ok(msg)) = stream.next().await {
                let chunk = match msg {
                    LogMsg::Stdout(x) => x,
                    LogMsg::JsonPatch(_)
                    | LogMsg::SessionId(_)
                    | LogMsg::MessageId(_)
                    | LogMsg::Stderr(_)
                    | LogMsg::Ready => continue,
                    LogMsg::Finished => break,
                };

                buffer.push_str(&chunk);

                // Process complete JSON lines
                for line in buffer
                    .split_inclusive('\n')
                    .filter(|l| l.ends_with('\n'))
                    .map(str::to_owned)
                    .collect::<Vec<_>>()
                {
                    let trimmed = line.trim();
                    if trimmed.is_empty() {
                        continue;
                    }

                    // Filter out claude-code-router service messages
                    if trimmed.starts_with("Service not running, starting service")
                        || trimmed
                            .contains("claude code router service has been successfully stopped")
                    {
                        continue;
                    }

                    match serde_json::from_str::<ClaudeJson>(trimmed) {
                        Ok(claude_json) => {
                            if !session_id_extracted
                                && let Some(session_id) = Self::extract_session_id(&claude_json)
                            {
                                msg_store.push_session_id(session_id);
                                session_id_extracted = true;
                            }

                            // Track message UUIDs for --resume-session-at:
                            // - User messages: always valid, push immediately and clear pending
                            // - Assistant messages: may have incomplete tool calls, store as pending
                            // - Result messages: confirms assistant turn is complete, commit pending
                            match &claude_json {
                                ClaudeJson::User { uuid, .. } => {
                                    pending_assistant_uuid = None;
                                    if let Some(uuid) = uuid {
                                        msg_store.push_message_id(uuid.clone());
                                    }
                                }
                                ClaudeJson::Assistant { uuid, .. } => {
                                    pending_assistant_uuid = uuid.clone();
                                }
                                ClaudeJson::Result { .. } => {
                                    if let Some(uuid) = pending_assistant_uuid.take() {
                                        msg_store.push_message_id(uuid);
                                    }
                                }
                                _ => {}
                            }

                            let patches = processor.normalize_entries(
                                &claude_json,
                                &worktree_path,
                                &entry_index_provider,
                            );
                            for patch in patches {
                                msg_store.push_patch(patch);
                            }
                        }
                        Err(_) => {
                            // Handle non-JSON output as raw system message
                            if !trimmed.is_empty() {
                                let entry = NormalizedEntry {
                                    timestamp: None,
                                    entry_type: NormalizedEntryType::SystemMessage,
                                    content: trimmed.to_string(),
                                    metadata: None,
                                };

                                let patch_id = entry_index_provider.next();
                                let patch =
                                    ConversationPatch::add_normalized_entry(patch_id, entry);
                                msg_store.push_patch(patch);
                            }
                        }
                    }
                }

                // Keep the partial line in the buffer
                buffer = buffer.rsplit('\n').next().unwrap_or("").to_owned();
            }

            // Handle any remaining content in buffer
            if !buffer.trim().is_empty() {
                let entry = NormalizedEntry {
                    timestamp: None,
                    entry_type: NormalizedEntryType::SystemMessage,
                    content: buffer.trim().to_string(),
                    metadata: None,
                };

                let patch_id = entry_index_provider.next();
                let patch = ConversationPatch::add_normalized_entry(patch_id, entry);
                msg_store.push_patch(patch);
            }
        });
    }

    /// Extract session ID from Claude JSON
    pub(super) fn extract_session_id(claude_json: &ClaudeJson) -> Option<String> {
        match claude_json {
            ClaudeJson::System { .. } => None, // session might not have been initialized yet
            ClaudeJson::Assistant { session_id, .. } => session_id.clone(),
            ClaudeJson::User { session_id, .. } => session_id.clone(),
            ClaudeJson::ToolUse { session_id, .. } => session_id.clone(),
            ClaudeJson::ToolResult { session_id, .. } => session_id.clone(),
            ClaudeJson::Result { session_id, .. } => session_id.clone(),
            ClaudeJson::StreamEvent { .. } => None, // session might not have been initialized yet
            ClaudeJson::ApprovalResponse { .. } => None,
            ClaudeJson::ControlRequest { .. } => None,
            ClaudeJson::ControlResponse { .. } => None,
            ClaudeJson::ControlCancelRequest { .. } => None,
            ClaudeJson::Unknown { .. } => None,
        }
    }
}

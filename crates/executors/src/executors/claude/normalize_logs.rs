use std::collections::HashMap;

use workspace_utils::approvals::ApprovalStatus;

use super::{
    json::{ClaudeContentItem, ClaudeJson, ClaudeMessage, ClaudeStreamEvent, ClaudeToolData},
    streaming::StreamingMessageState,
    tool_display::{AmpBashResult, ClaudeToolWithInput},
};
use crate::logs::{
    ActionType, NormalizedEntry, NormalizedEntryError, NormalizedEntryType, ToolStatus,
    utils::{EntryIndexProvider, patch::ConversationPatch},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HistoryStrategy {
    // Claude-code format
    Default,
    // Amp threads format which includes logs from previous executions
    AmpResume,
}

/// Default context window for models (used until we get actual value from result)
const DEFAULT_CLAUDE_CONTEXT_WINDOW: u32 = 200_000;

/// Handles log processing and interpretation for Claude executor
pub struct ClaudeLogProcessor {
    model_name: Option<String>,
    // Map tool_use_id -> structured info for follow-up ToolResult replacement
    tool_map: HashMap<String, ClaudeToolCallInfo>,
    // Strategy controlling how to handle history and user messages
    strategy: HistoryStrategy,
    streaming_messages: HashMap<String, StreamingMessageState>,
    streaming_message_id: Option<String>,
    last_assistant_message: Option<String>,
    // Main model name (excluding subagents). Only used internally for context window tracking.
    main_model_name: Option<String>,
    main_model_context_window: u32,
    context_tokens_used: u32,
}

#[derive(Debug, Clone)]
struct ClaudeToolCallInfo {
    entry_index: usize,
    tool_name: String,
    tool_data: ClaudeToolData,
    content: String,
}

impl ClaudeLogProcessor {
    #[cfg(test)]
    fn new() -> Self {
        Self::new_with_strategy(HistoryStrategy::Default)
    }

    pub(super) fn new_with_strategy(strategy: HistoryStrategy) -> Self {
        Self {
            model_name: None,
            main_model_name: None,
            tool_map: HashMap::new(),
            strategy,
            streaming_messages: HashMap::new(),
            streaming_message_id: None,
            last_assistant_message: None,
            main_model_context_window: DEFAULT_CLAUDE_CONTEXT_WINDOW,
            context_tokens_used: 0,
        }
    }

    /// Generate warning entry if API key source is ANTHROPIC_API_KEY
    fn warn_if_unmanaged_key(src: &Option<String>) -> Option<NormalizedEntry> {
        match src.as_deref() {
            Some("ANTHROPIC_API_KEY") => {
                tracing::warn!(
                    "ANTHROPIC_API_KEY env variable detected, your Anthropic subscription is not being used"
                );
                Some(NormalizedEntry {
                    timestamp: None,
                    entry_type: NormalizedEntryType::ErrorMessage { error_type: NormalizedEntryError::Other,
                    },
                    content: "Claude Code + ANTHROPIC_API_KEY detected. Usage will be billed via Anthropic pay-as-you-go instead of your Claude subscription. If this is unintended, please select the `disable_api_key` checkbox in the conding-agent-configurations settings page.".to_string(),
                    metadata: None,
                })
            }
            _ => None,
        }
    }

    /// Convert Claude JSON to normalized patches
    pub(super) fn normalize_entries(
        &mut self,
        claude_json: &ClaudeJson,
        worktree_path: &str,
        entry_index_provider: &EntryIndexProvider,
    ) -> Vec<json_patch::Patch> {
        let mut patches = Vec::new();
        match claude_json {
            ClaudeJson::System {
                subtype,
                api_key_source,
                model,
                status,
                ..
            } => {
                // emit billing warning if required
                if let Some(warning) = Self::warn_if_unmanaged_key(api_key_source) {
                    let idx = entry_index_provider.next();
                    patches.push(ConversationPatch::add_normalized_entry(idx, warning));
                }

                // keep the existing behaviour for the normal system message
                match subtype.as_deref() {
                    Some("init") => {
                        if self.main_model_name.is_none() {
                            // this name matches the model names in the usage report in the result message
                            if let Some(model) = model {
                                self.main_model_name = Some(model.clone());
                            }
                        }
                        // Skip system init messages because it doesn't contain the actual model that will be used in assistant messages in case of claude-code-router.
                        // We'll send system initialized message with first assistant message that has a model field.
                    }
                    Some("status") => {
                        if let Some(status) = status {
                            patches.push(add_system_message(status.clone(), entry_index_provider));
                        }
                    }
                    Some("compact_boundary") => {}
                    Some(subtype) => {
                        let entry = NormalizedEntry {
                            timestamp: None,
                            entry_type: NormalizedEntryType::SystemMessage,
                            content: format!("System: {subtype}"),
                            metadata: Some(
                                serde_json::to_value(claude_json)
                                    .unwrap_or(serde_json::Value::Null),
                            ),
                        };
                        let idx = entry_index_provider.next();
                        patches.push(ConversationPatch::add_normalized_entry(idx, entry));
                    }
                    None => {
                        let entry = NormalizedEntry {
                            timestamp: None,
                            entry_type: NormalizedEntryType::SystemMessage,
                            content: "System message".to_string(),
                            metadata: Some(
                                serde_json::to_value(claude_json)
                                    .unwrap_or(serde_json::Value::Null),
                            ),
                        };
                        let idx = entry_index_provider.next();
                        patches.push(ConversationPatch::add_normalized_entry(idx, entry));
                    }
                }
            }
            ClaudeJson::Assistant { message, .. } => {
                if let Some(patch) = extract_model_name(self, message, entry_index_provider) {
                    patches.push(patch);
                }

                let mut streaming_message_state = message
                    .id
                    .as_ref()
                    .and_then(|id| self.streaming_messages.remove(id));

                for (content_index, item) in message.content.items().enumerate() {
                    let entry_index = streaming_message_state
                        .as_mut()
                        .and_then(|state| state.content_entry_index(content_index));

                    match item {
                        ClaudeContentItem::ToolUse { id, tool_data } => {
                            let tool_name = tool_data.get_name().to_string();
                            let action_type = Self::extract_action_type(tool_data, worktree_path);
                            let content_text = Self::generate_concise_content(
                                tool_data,
                                &action_type,
                                worktree_path,
                            );

                            // Create metadata with tool_call_id for approval matching
                            let mut metadata =
                                serde_json::to_value(item).unwrap_or(serde_json::Value::Null);
                            if let Some(obj) = metadata.as_object_mut() {
                                obj.insert(
                                    "tool_call_id".to_string(),
                                    serde_json::Value::String(id.clone()),
                                );
                            }

                            let entry = NormalizedEntry {
                                timestamp: None,
                                entry_type: NormalizedEntryType::ToolUse {
                                    tool_name: tool_name.clone(),
                                    action_type,
                                    status: ToolStatus::Created,
                                },
                                content: content_text.clone(),
                                metadata: Some(metadata),
                            };
                            let is_new = entry_index.is_none();
                            let id_num = entry_index.unwrap_or_else(|| entry_index_provider.next());
                            self.tool_map.insert(
                                id.clone(),
                                ClaudeToolCallInfo {
                                    entry_index: id_num,
                                    tool_name: tool_name.clone(),
                                    tool_data: tool_data.clone(),
                                    content: content_text,
                                },
                            );
                            let patch = if is_new {
                                ConversationPatch::add_normalized_entry(id_num, entry)
                            } else {
                                ConversationPatch::replace(id_num, entry)
                            };
                            patches.push(patch);
                        }
                        ClaudeContentItem::Text { .. } | ClaudeContentItem::Thinking { .. } => {
                            if let Some(entry) = Self::content_item_to_normalized_entry(
                                item,
                                &message.role,
                                worktree_path,
                                &mut self.last_assistant_message,
                            ) {
                                let is_new = entry_index.is_none();
                                let idx =
                                    entry_index.unwrap_or_else(|| entry_index_provider.next());
                                let patch = if is_new {
                                    ConversationPatch::add_normalized_entry(idx, entry)
                                } else {
                                    ConversationPatch::replace(idx, entry)
                                };
                                patches.push(patch);
                            }
                        }
                        ClaudeContentItem::ToolResult { .. } => {}
                    }
                }
            }
            ClaudeJson::User {
                message,
                is_synthetic,
                is_replay,
                ..
            } => {
                // Skip replay messages entirely - they're historical context from resumed sessions
                if *is_replay {
                    return patches;
                }

                if matches!(self.strategy, HistoryStrategy::AmpResume)
                    && message
                        .content
                        .items()
                        .any(|c| matches!(c, ClaudeContentItem::Text { .. }))
                {
                    let cur = entry_index_provider.current();
                    if cur > 0 {
                        for _ in 0..cur {
                            patches.push(ConversationPatch::remove_diff(0.to_string()));
                        }
                        entry_index_provider.reset();
                        self.tool_map.clear();
                    }

                    for item in message.content.items() {
                        if let ClaudeContentItem::Text { text } = item {
                            let entry = NormalizedEntry {
                                timestamp: None,
                                entry_type: NormalizedEntryType::UserMessage,
                                content: text.clone(),
                                metadata: Some(
                                    serde_json::to_value(item).unwrap_or(serde_json::Value::Null),
                                ),
                            };
                            let id = entry_index_provider.next();
                            patches.push(ConversationPatch::add_normalized_entry(id, entry));
                        }
                    }
                }

                if *is_synthetic {
                    for item in message.content.items() {
                        if let ClaudeContentItem::Text { text } = item {
                            let entry = NormalizedEntry {
                                timestamp: None,
                                entry_type: NormalizedEntryType::SystemMessage,
                                content: text.clone(),
                                metadata: None,
                            };
                            let id = entry_index_provider.next();
                            patches.push(ConversationPatch::add_normalized_entry(id, entry));
                        }
                    }
                }

                if let Some(mut text) = message.content.as_text().cloned() {
                    if text.starts_with("<local-command-stdout>")
                        && text.ends_with("</local-command-stdout>")
                    {
                        text = text
                            .trim_start_matches("<local-command-stdout>")
                            .trim_end_matches("</local-command-stdout>")
                            .to_string();
                    }
                    patches.push(add_system_message(text.clone(), entry_index_provider));
                }

                for item in message.content.items() {
                    if let ClaudeContentItem::ToolResult {
                        tool_use_id,
                        content,
                        is_error,
                    } = item
                        && let Some(info) = self.tool_map.get(tool_use_id).cloned()
                    {
                        let is_command = matches!(info.tool_data, ClaudeToolData::Bash { .. });

                        let _display_tool_name = if is_command {
                            info.tool_name.clone()
                        } else {
                            let raw_name = info.tool_data.get_name().to_string();
                            if raw_name.starts_with("mcp__") {
                                let parts: Vec<&str> = raw_name.split("__").collect();
                                if parts.len() >= 3 {
                                    format!("mcp:{}:{}", parts[1], parts[2])
                                } else {
                                    raw_name
                                }
                            } else {
                                raw_name
                            }
                        };

                        if is_command {
                            let content_str = if let Some(s) = content.as_str() {
                                s.to_string()
                            } else {
                                content.to_string()
                            };

                            let result = if let Ok(result) =
                                serde_json::from_str::<AmpBashResult>(&content_str)
                            {
                                Some(crate::logs::CommandRunResult {
                                    exit_status: Some(crate::logs::CommandExitStatus::ExitCode {
                                        code: result.exit_code,
                                    }),
                                    output: Some(result.output),
                                })
                            } else {
                                Some(crate::logs::CommandRunResult {
                                    exit_status: (*is_error).map(|is_error| {
                                        crate::logs::CommandExitStatus::Success {
                                            success: !is_error,
                                        }
                                    }),
                                    output: Some(content_str),
                                })
                            };

                            let status = if is_error.unwrap_or(false) {
                                ToolStatus::Failed
                            } else {
                                ToolStatus::Success
                            };

                            let entry = NormalizedEntry {
                                timestamp: None,
                                entry_type: NormalizedEntryType::ToolUse {
                                    tool_name: info.tool_name.clone(),
                                    action_type: ActionType::CommandRun {
                                        command: info.content.clone(),
                                        result,
                                    },
                                    status,
                                },
                                content: info.content.clone(),
                                metadata: None,
                            };
                            patches.push(ConversationPatch::replace(info.entry_index, entry));
                        } else if matches!(info.tool_data, ClaudeToolData::Task { .. }) {
                            // Handle Task tool results - capture subagent output
                            let (res_type, res_value) =
                                Self::normalize_claude_tool_result_value(content);

                            let status = if is_error.unwrap_or(false) {
                                ToolStatus::Failed
                            } else {
                                ToolStatus::Success
                            };

                            // Extract subagent_type from the original tool_data
                            let subagent_type =
                                if let ClaudeToolData::Task { subagent_type, .. } = &info.tool_data
                                {
                                    subagent_type.clone()
                                } else {
                                    None
                                };

                            let entry = NormalizedEntry {
                                timestamp: None,
                                entry_type: NormalizedEntryType::ToolUse {
                                    tool_name: info.tool_name.clone(),
                                    action_type: ActionType::TaskCreate {
                                        description: info.content.clone(),
                                        subagent_type,
                                        result: Some(crate::logs::ToolResult {
                                            r#type: res_type,
                                            value: res_value,
                                        }),
                                    },
                                    status,
                                },
                                content: info.content.clone(),
                                metadata: None,
                            };
                            patches.push(ConversationPatch::replace(info.entry_index, entry));
                        } else if matches!(
                            info.tool_data,
                            ClaudeToolData::Unknown { .. }
                                | ClaudeToolData::Oracle { .. }
                                | ClaudeToolData::Mermaid { .. }
                                | ClaudeToolData::CodebaseSearchAgent { .. }
                                | ClaudeToolData::NotebookEdit { .. }
                        ) {
                            let (res_type, res_value) =
                                Self::normalize_claude_tool_result_value(content);

                            let args_to_show = serde_json::to_value(&info.tool_data)
                                .ok()
                                .and_then(|v| serde_json::from_value::<ClaudeToolWithInput>(v).ok())
                                .map(|w| w.input)
                                .unwrap_or(serde_json::Value::Null);

                            let tool_name = info.tool_data.get_name().to_string();
                            let is_mcp = tool_name.starts_with("mcp__");
                            let label = if is_mcp {
                                let parts: Vec<&str> = tool_name.split("__").collect();
                                if parts.len() >= 3 {
                                    format!("mcp:{}:{}", parts[1], parts[2])
                                } else {
                                    tool_name.clone()
                                }
                            } else {
                                tool_name.clone()
                            };

                            let status = if is_error.unwrap_or(false) {
                                ToolStatus::Failed
                            } else {
                                ToolStatus::Success
                            };

                            let entry = NormalizedEntry {
                                timestamp: None,
                                entry_type: NormalizedEntryType::ToolUse {
                                    tool_name: label.clone(),
                                    action_type: ActionType::Tool {
                                        tool_name: label,
                                        arguments: Some(args_to_show),
                                        result: Some(crate::logs::ToolResult {
                                            r#type: res_type,
                                            value: res_value,
                                        }),
                                    },
                                    status,
                                },
                                content: info.content.clone(),
                                metadata: None,
                            };
                            patches.push(ConversationPatch::replace(info.entry_index, entry));
                        }
                        // Note: With control protocol, denials are handled via protocol messages
                        // rather than error content parsing
                    }
                }
            }
            ClaudeJson::ToolUse { tool_data, .. } => {
                let tool_name = tool_data.get_name();
                let action_type = Self::extract_action_type(tool_data, worktree_path);
                let content =
                    Self::generate_concise_content(tool_data, &action_type, worktree_path);

                let entry = NormalizedEntry {
                    timestamp: None,
                    entry_type: NormalizedEntryType::ToolUse {
                        tool_name: tool_name.to_string(),
                        action_type,
                        status: ToolStatus::Created,
                    },
                    content,
                    metadata: Some(
                        serde_json::to_value(claude_json).unwrap_or(serde_json::Value::Null),
                    ),
                };
                let idx = entry_index_provider.next();
                patches.push(ConversationPatch::add_normalized_entry(idx, entry));
            }
            ClaudeJson::ToolResult { .. } => {
                // Add proper ToolResult support to NormalizedEntry when the type system supports it
            }
            ClaudeJson::StreamEvent {
                event,
                parent_tool_use_id,
                ..
            } => match event {
                ClaudeStreamEvent::MessageStart { message } => {
                    if message.role == "assistant" {
                        if let Some(patch) = extract_model_name(self, message, entry_index_provider)
                        {
                            patches.push(patch);
                        }

                        if let Some(message_id) = message.id.clone() {
                            self.streaming_messages.insert(
                                message_id.clone(),
                                StreamingMessageState::new(message.role.clone()),
                            );
                            self.streaming_message_id = Some(message_id);
                        } else {
                            self.streaming_message_id = None;
                        }
                    } else {
                        self.streaming_message_id = None;
                    }
                }
                ClaudeStreamEvent::ContentBlockStart {
                    index,
                    content_block,
                } => {
                    if let Some(state) = self
                        .streaming_message_id
                        .as_ref()
                        .and_then(|id| self.streaming_messages.get_mut(id))
                    {
                        state.content_block_start(*index, content_block.clone());
                    }
                }
                ClaudeStreamEvent::ContentBlockDelta { index, delta } => {
                    if let Some(state) = self
                        .streaming_message_id
                        .as_ref()
                        .and_then(|id| self.streaming_messages.get_mut(id))
                        && let Some(patch) = state.apply_content_block_delta(
                            *index,
                            delta,
                            worktree_path,
                            entry_index_provider,
                            &mut self.last_assistant_message,
                        )
                    {
                        patches.push(patch);
                    }
                }
                ClaudeStreamEvent::ContentBlockStop { .. } => {}
                ClaudeStreamEvent::MessageDelta { usage, .. } => {
                    // do not report context token usage for subagents
                    if parent_tool_use_id.is_none()
                        && let Some(usage) = usage
                    {
                        let input_tokens = usage.input_tokens.unwrap_or(0)
                            + usage.cache_creation_input_tokens.unwrap_or(0)
                            + usage.cache_read_input_tokens.unwrap_or(0);
                        let output_tokens = usage.output_tokens.unwrap_or(0);
                        let total_tokens = input_tokens + output_tokens;
                        self.context_tokens_used = total_tokens as u32;

                        patches.push(self.add_token_usage_entry(entry_index_provider));
                    }
                }
                ClaudeStreamEvent::MessageStop => {
                    if let Some(message_id) = self.streaming_message_id.take() {
                        let _ = self.streaming_messages.remove(&message_id);
                    }
                }
                ClaudeStreamEvent::Unknown => {}
            },
            ClaudeJson::Result {
                is_error,
                model_usage,
                subtype,
                result,
                ..
            } => {
                // get the real model context window and correct the context usage entry
                if let Some(context_window) = model_usage.as_ref().and_then(|model_usage| {
                    self.main_model_name
                        .as_ref()
                        .and_then(|name| model_usage.get(name))
                        .and_then(|usage| usage.context_window)
                }) {
                    self.main_model_context_window = context_window;
                    patches.push(self.add_token_usage_entry(entry_index_provider));
                }

                if matches!(self.strategy, HistoryStrategy::AmpResume) && is_error.unwrap_or(false)
                {
                    let entry = NormalizedEntry {
                        timestamp: None,
                        entry_type: NormalizedEntryType::ErrorMessage {
                            error_type: NormalizedEntryError::Other,
                        },
                        content: serde_json::to_string(claude_json)
                            .unwrap_or_else(|_| "error".to_string()),
                        metadata: Some(
                            serde_json::to_value(claude_json).unwrap_or(serde_json::Value::Null),
                        ),
                    };
                    let idx = entry_index_provider.next();
                    patches.push(ConversationPatch::add_normalized_entry(idx, entry));
                } else if matches!(subtype.as_deref(), Some("success"))
                    && let Some(text) = result.as_ref().and_then(|v| v.as_str())
                    && (self.last_assistant_message.is_none()
                        || matches!(&self.last_assistant_message, Some(message) if !message.contains(text)))
                {
                    let entry = NormalizedEntry {
                        timestamp: None,
                        entry_type: NormalizedEntryType::AssistantMessage,
                        content: text.to_string(),
                        metadata: Some(
                            serde_json::to_value(claude_json).unwrap_or(serde_json::Value::Null),
                        ),
                    };
                    let idx = entry_index_provider.next();
                    patches.push(ConversationPatch::add_normalized_entry(idx, entry));
                }
            }
            ClaudeJson::ApprovalResponse {
                call_id: _,
                tool_name,
                approval_status,
            } => {
                // Convert denials and timeouts to visible entries (matching Codex behavior)
                let entry_opt = match approval_status {
                    ApprovalStatus::Pending => None,
                    ApprovalStatus::Approved => None,
                    ApprovalStatus::Denied { reason } => Some(NormalizedEntry {
                        timestamp: None,
                        entry_type: NormalizedEntryType::UserFeedback {
                            denied_tool: tool_name.clone(),
                        },
                        content: reason
                            .as_ref()
                            .map(|s| s.trim().to_string())
                            .filter(|s| !s.is_empty())
                            .unwrap_or_else(|| "User denied this tool use request".to_string()),
                        metadata: None,
                    }),
                    ApprovalStatus::TimedOut => Some(NormalizedEntry {
                        timestamp: None,
                        entry_type: NormalizedEntryType::ErrorMessage {
                            error_type: NormalizedEntryError::Other,
                        },
                        content: format!("Approval timed out for tool {tool_name}"),
                        metadata: None,
                    }),
                };

                if let Some(entry) = entry_opt {
                    let idx = entry_index_provider.next();
                    patches.push(ConversationPatch::add_normalized_entry(idx, entry));
                }
            }
            ClaudeJson::Unknown { data } => {
                let entry = NormalizedEntry {
                    timestamp: None,
                    entry_type: NormalizedEntryType::SystemMessage,
                    content: format!(
                        "Unrecognized JSON message: {}",
                        serde_json::to_value(data).unwrap_or_default()
                    ),
                    metadata: None,
                };
                let idx = entry_index_provider.next();
                patches.push(ConversationPatch::add_normalized_entry(idx, entry));
            }
            ClaudeJson::ControlRequest { .. }
            | ClaudeJson::ControlResponse { .. }
            | ClaudeJson::ControlCancelRequest { .. } => {}
        }
        patches
    }
    fn add_token_usage_entry(
        &mut self,
        entry_index_provider: &EntryIndexProvider,
    ) -> json_patch::Patch {
        let entry = NormalizedEntry {
            timestamp: None,
            entry_type: NormalizedEntryType::TokenUsageInfo(crate::logs::TokenUsageInfo {
                total_tokens: self.context_tokens_used,
                model_context_window: self.main_model_context_window,
            }),
            content: format!(
                "Tokens used: {} / Context window: {}",
                self.context_tokens_used, self.main_model_context_window
            ),
            metadata: None,
        };
        let idx = entry_index_provider.next();
        ConversationPatch::add_normalized_entry(idx, entry)
    }
}

fn add_system_message(
    content: String,
    entry_index_provider: &EntryIndexProvider,
) -> json_patch::Patch {
    let entry = NormalizedEntry {
        timestamp: None,
        entry_type: NormalizedEntryType::SystemMessage,
        content,
        metadata: None,
    };
    let id = entry_index_provider.next();
    ConversationPatch::add_normalized_entry(id, entry)
}

fn extract_model_name(
    processor: &mut ClaudeLogProcessor,
    message: &ClaudeMessage,
    entry_index_provider: &EntryIndexProvider,
) -> Option<json_patch::Patch> {
    if processor.model_name.is_none()
        && let Some(model) = message.model.as_ref()
    {
        processor.model_name = Some(model.clone());
        let entry = NormalizedEntry {
            timestamp: None,
            entry_type: NormalizedEntryType::SystemMessage,
            content: format!("System initialized with model: {model}"),
            metadata: None,
        };
        let id = entry_index_provider.next();
        Some(ConversationPatch::add_normalized_entry(id, entry))
    } else {
        None
    }
}

#[cfg(test)]
mod tests;

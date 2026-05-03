use std::{collections::HashMap, path::PathBuf};

use serde::{Deserialize, Serialize};
use workspace_utils::approvals::ApprovalStatus;

use super::types::{ControlRequestType, ControlResponseType};

// Data structures for parsing Claude's JSON output format
#[derive(Deserialize, Serialize, Debug, Clone)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClaudeJson {
    System {
        subtype: Option<String>,
        session_id: Option<String>,
        cwd: Option<String>,
        tools: Option<Vec<serde_json::Value>>,
        model: Option<String>,
        #[serde(default, rename = "apiKeySource")]
        api_key_source: Option<String>,
        status: Option<String>,
        #[serde(default)]
        slash_commands: Vec<String>,
        #[serde(default)]
        plugins: Vec<ClaudePlugin>,
    },
    Assistant {
        message: ClaudeMessage,
        session_id: Option<String>,
        #[serde(default)]
        uuid: Option<String>,
    },
    User {
        message: ClaudeMessage,
        session_id: Option<String>,
        #[serde(default)]
        uuid: Option<String>,
        #[serde(default, rename = "isSynthetic")]
        is_synthetic: bool,
        #[serde(default, rename = "isReplay")]
        is_replay: bool,
    },
    ToolUse {
        tool_name: String,
        #[serde(flatten)]
        tool_data: ClaudeToolData,
        session_id: Option<String>,
    },
    ToolResult {
        result: serde_json::Value,
        is_error: Option<bool>,
        session_id: Option<String>,
    },
    StreamEvent {
        event: ClaudeStreamEvent,
        #[serde(default)]
        session_id: Option<String>,
        #[serde(default)]
        parent_tool_use_id: Option<String>,
        #[serde(default)]
        uuid: Option<String>,
    },
    Result {
        #[serde(default)]
        subtype: Option<String>,
        #[serde(default, alias = "isError")]
        is_error: Option<bool>,
        #[serde(default, alias = "durationMs")]
        duration_ms: Option<u64>,
        #[serde(default)]
        result: Option<serde_json::Value>,
        #[serde(default)]
        error: Option<String>,
        #[serde(default, alias = "numTurns")]
        num_turns: Option<u32>,
        #[serde(default, alias = "sessionId")]
        session_id: Option<String>,
        #[serde(default, alias = "modelUsage")]
        model_usage: Option<HashMap<String, ClaudeModelUsage>>,
        #[serde(default)]
        usage: Option<ClaudeUsage>,
    },
    ApprovalResponse {
        call_id: String,
        tool_name: String,
        approval_status: ApprovalStatus,
    },
    ControlRequest {
        request_id: String,
        request: ControlRequestType,
    },
    ControlResponse {
        response: ControlResponseType,
    },
    ControlCancelRequest {
        request_id: String,
    },
    // Catch-all for unknown message types
    #[serde(untagged)]
    Unknown {
        #[serde(flatten)]
        data: HashMap<String, serde_json::Value>,
    },
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ClaudePlugin {
    pub name: String,
    pub path: PathBuf,
}

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq)]
pub struct ClaudeMessage {
    pub id: Option<String>,
    #[serde(rename = "type")]
    pub message_type: Option<String>,
    pub role: String,
    pub model: Option<String>,
    pub content: ClaudeMessageContent,
    pub stop_reason: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq)]
#[serde(untagged)]
pub enum ClaudeMessageContent {
    Array(Vec<ClaudeContentItem>),
    Text(String),
}

impl ClaudeMessageContent {
    pub(super) fn items(&self) -> impl Iterator<Item = &ClaudeContentItem> {
        match self {
            ClaudeMessageContent::Array(items) => items.iter(),
            ClaudeMessageContent::Text(_) => [].iter(),
        }
    }

    pub(super) fn as_text(&self) -> Option<&String> {
        match self {
            ClaudeMessageContent::Text(s) => Some(s),
            _ => None,
        }
    }
}

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq)]
#[serde(tag = "type")]
pub enum ClaudeContentItem {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "thinking")]
    Thinking { thinking: String },
    #[serde(rename = "tool_use")]
    ToolUse {
        id: String,
        #[serde(flatten)]
        tool_data: ClaudeToolData,
    },
    #[serde(rename = "tool_result")]
    ToolResult {
        tool_use_id: String,
        content: serde_json::Value,
        is_error: Option<bool>,
    },
}

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq)]
#[serde(tag = "type")]
pub enum ClaudeStreamEvent {
    #[serde(rename = "message_start")]
    MessageStart { message: ClaudeMessage },
    #[serde(rename = "content_block_start")]
    ContentBlockStart {
        index: usize,
        content_block: ClaudeContentItem,
    },
    #[serde(rename = "content_block_delta")]
    ContentBlockDelta {
        index: usize,
        delta: ClaudeContentBlockDelta,
    },
    #[serde(rename = "content_block_stop")]
    ContentBlockStop { index: usize },
    #[serde(rename = "message_delta")]
    MessageDelta {
        #[serde(default)]
        delta: Option<ClaudeMessageDelta>,
        #[serde(default)]
        usage: Option<ClaudeUsage>,
    },
    #[serde(rename = "message_stop")]
    MessageStop,
    #[serde(other)]
    Unknown,
}

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq)]
#[serde(tag = "type")]
pub enum ClaudeContentBlockDelta {
    #[serde(rename = "text_delta")]
    TextDelta { text: String },
    #[serde(rename = "thinking_delta")]
    ThinkingDelta { thinking: String },
    #[serde(other)]
    Unknown,
}

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq, Default)]
pub struct ClaudeMessageDelta {
    #[serde(default)]
    pub stop_reason: Option<String>,
    #[serde(default)]
    pub stop_sequence: Option<String>,
}

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq, Default)]
pub struct ClaudeUsage {
    #[serde(default)]
    pub input_tokens: Option<u64>,
    #[serde(default)]
    pub output_tokens: Option<u64>,
    #[serde(default, rename = "cache_creation_input_tokens")]
    pub cache_creation_input_tokens: Option<u64>,
    #[serde(default, rename = "cache_read_input_tokens")]
    pub cache_read_input_tokens: Option<u64>,
    #[serde(default)]
    pub service_tier: Option<String>,
}

/// Per-model usage statistics from result message
#[derive(Deserialize, Serialize, Debug, Clone, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct ClaudeModelUsage {
    #[serde(default)]
    pub context_window: Option<u32>,
}

/// Structured tool data for Claude tools based on real samples
#[derive(Deserialize, Serialize, Debug, Clone, PartialEq)]
#[serde(tag = "name", content = "input")]
pub enum ClaudeToolData {
    #[serde(rename = "TodoWrite", alias = "todo_write")]
    TodoWrite {
        todos: Vec<ClaudeTodoItem>,
    },
    #[serde(rename = "Task", alias = "task")]
    Task {
        subagent_type: Option<String>,
        description: Option<String>,
        prompt: Option<String>,
    },
    #[serde(rename = "Glob", alias = "glob")]
    Glob {
        #[serde(alias = "filePattern")]
        pattern: String,
        #[serde(default)]
        path: Option<String>,
        #[serde(default)]
        limit: Option<u32>,
    },
    #[serde(rename = "LS", alias = "list_directory", alias = "ls")]
    LS {
        path: String,
    },
    #[serde(rename = "Read", alias = "read")]
    Read {
        #[serde(alias = "path")]
        file_path: String,
    },
    #[serde(rename = "Bash", alias = "bash")]
    Bash {
        #[serde(alias = "cmd", alias = "command_line")]
        command: String,
        #[serde(default)]
        description: Option<String>,
    },
    #[serde(rename = "Grep", alias = "grep")]
    Grep {
        pattern: String,
        #[serde(default)]
        output_mode: Option<String>,
        #[serde(default)]
        path: Option<String>,
    },
    ExitPlanMode {
        plan: String,
    },
    #[serde(rename = "Edit", alias = "edit_file")]
    Edit {
        #[serde(alias = "path")]
        file_path: String,
        #[serde(alias = "old_str")]
        old_string: Option<String>,
        #[serde(alias = "new_str")]
        new_string: Option<String>,
    },
    #[serde(rename = "MultiEdit", alias = "multi_edit")]
    MultiEdit {
        #[serde(alias = "path")]
        file_path: String,
        edits: Vec<ClaudeEditItem>,
    },
    #[serde(rename = "Write", alias = "create_file", alias = "write_file")]
    Write {
        #[serde(alias = "path")]
        file_path: String,
        content: String,
    },
    #[serde(rename = "NotebookEdit", alias = "notebook_edit")]
    NotebookEdit {
        notebook_path: String,
        new_source: String,
        edit_mode: String,
        #[serde(default)]
        cell_id: Option<String>,
    },
    #[serde(rename = "WebFetch", alias = "read_web_page")]
    WebFetch {
        url: String,
        #[serde(default)]
        prompt: Option<String>,
    },
    #[serde(rename = "WebSearch", alias = "web_search")]
    WebSearch {
        query: String,
        #[serde(default)]
        num_results: Option<u32>,
    },
    // Amp-only utilities for better UX
    #[serde(rename = "Oracle", alias = "oracle")]
    Oracle {
        #[serde(default)]
        task: Option<String>,
        #[serde(default)]
        files: Option<Vec<String>>,
        #[serde(default)]
        context: Option<String>,
    },
    #[serde(rename = "Mermaid", alias = "mermaid")]
    Mermaid {
        code: String,
    },
    #[serde(rename = "CodebaseSearchAgent", alias = "codebase_search_agent")]
    CodebaseSearchAgent {
        #[serde(default)]
        query: Option<String>,
        #[serde(default)]
        path: Option<String>,
        #[serde(default)]
        include: Option<Vec<String>>,
        #[serde(default)]
        exclude: Option<Vec<String>>,
        #[serde(default)]
        limit: Option<u32>,
    },
    #[serde(rename = "UndoEdit", alias = "undo_edit")]
    UndoEdit {
        #[serde(default, alias = "file_path")]
        path: Option<String>,
        #[serde(default)]
        steps: Option<u32>,
    },
    #[serde(rename = "TodoRead", alias = "todo_read")]
    TodoRead {},
    #[serde(untagged)]
    Unknown {
        #[serde(flatten)]
        data: std::collections::HashMap<String, serde_json::Value>,
    },
}

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq)]
pub struct ClaudeTodoItem {
    #[serde(default)]
    pub id: Option<String>,
    pub content: String,
    pub status: String,
    #[serde(default)]
    pub priority: Option<String>,
}

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq)]
pub struct ClaudeEditItem {
    pub old_string: Option<String>,
    pub new_string: Option<String>,
}

impl ClaudeToolData {
    pub fn get_name(&self) -> &str {
        match self {
            ClaudeToolData::TodoWrite { .. } => "TodoWrite",
            ClaudeToolData::Task { .. } => "Task",
            ClaudeToolData::Glob { .. } => "Glob",
            ClaudeToolData::LS { .. } => "LS",
            ClaudeToolData::Read { .. } => "Read",
            ClaudeToolData::Bash { .. } => "Bash",
            ClaudeToolData::Grep { .. } => "Grep",
            ClaudeToolData::ExitPlanMode { .. } => "ExitPlanMode",
            ClaudeToolData::Edit { .. } => "Edit",
            ClaudeToolData::MultiEdit { .. } => "MultiEdit",
            ClaudeToolData::Write { .. } => "Write",
            ClaudeToolData::NotebookEdit { .. } => "NotebookEdit",
            ClaudeToolData::WebFetch { .. } => "WebFetch",
            ClaudeToolData::WebSearch { .. } => "WebSearch",
            ClaudeToolData::TodoRead { .. } => "TodoRead",
            ClaudeToolData::Oracle { .. } => "Oracle",
            ClaudeToolData::Mermaid { .. } => "Mermaid",
            ClaudeToolData::CodebaseSearchAgent { .. } => "CodebaseSearchAgent",
            ClaudeToolData::UndoEdit { .. } => "UndoEdit",
            ClaudeToolData::Unknown { data } => data
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown"),
        }
    }
}

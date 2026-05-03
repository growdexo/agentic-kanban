use super::*;
use crate::logs::utils::{EntryIndexProvider, patch::extract_normalized_entry_from_patch};
use crate::{
    executors::claude::{ClaudeJson, ClaudeToolData},
    logs::{ActionType, NormalizedEntry, NormalizedEntryType},
};
use workspace_utils::path::make_path_relative;

fn patches_to_entries(patches: &[json_patch::Patch]) -> Vec<NormalizedEntry> {
    patches
        .iter()
        .filter_map(|patch| extract_normalized_entry_from_patch(patch).map(|(_, entry)| entry))
        .collect()
}

fn normalize_helper(
    processor: &mut ClaudeLogProcessor,
    json: &ClaudeJson,
    worktree: &str,
) -> Vec<NormalizedEntry> {
    let provider = EntryIndexProvider::test_new();
    let patches = processor.normalize_entries(json, worktree, &provider);
    patches_to_entries(&patches)
}

fn normalize(json: &ClaudeJson, worktree: &str) -> Vec<NormalizedEntry> {
    let mut processor = ClaudeLogProcessor::new();
    normalize_helper(&mut processor, json, worktree)
}

#[test]
fn test_claude_json_parsing() {
    let system_json =
        r#"{"type":"system","subtype":"init","session_id":"abc123","model":"claude-sonnet-4"}"#;
    let parsed: ClaudeJson = serde_json::from_str(system_json).unwrap();

    // System messages no longer extract session_id
    assert_eq!(ClaudeLogProcessor::extract_session_id(&parsed), None);

    let entries = normalize(&parsed, "");
    assert_eq!(entries.len(), 0);

    let assistant_json = r#"
        {"type":"assistant","message":{"type":"message","role":"assistant","model":"claude-sonnet-4-20250514","content":[{"type":"text","text":"Hi! I'm Claude Code."}]}}"#;
    let parsed: ClaudeJson = serde_json::from_str(assistant_json).unwrap();
    let entries = normalize(&parsed, "");

    assert_eq!(entries.len(), 2);
    assert!(matches!(
        entries[0].entry_type,
        NormalizedEntryType::SystemMessage
    ));
    assert_eq!(
        entries[0].content,
        "System initialized with model: claude-sonnet-4-20250514"
    );
}

#[test]
fn test_assistant_message_parsing() {
    let assistant_json = r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"Hello world"}]},"session_id":"abc123"}"#;
    let parsed: ClaudeJson = serde_json::from_str(assistant_json).unwrap();

    let entries = normalize(&parsed, "");
    assert_eq!(entries.len(), 1);
    assert!(matches!(
        entries[0].entry_type,
        NormalizedEntryType::AssistantMessage
    ));
    assert_eq!(entries[0].content, "Hello world");
}

#[test]
fn test_result_message_emits_final_text_if_not_seen() {
    let result_json = r#"{"type":"result","subtype":"success","is_error":false,"duration_ms":6059,"result":"Final result"}"#;
    let parsed: ClaudeJson = serde_json::from_str(result_json).unwrap();

    let entries = normalize(&parsed, "");
    assert_eq!(entries.len(), 1);
    assert!(matches!(
        entries[0].entry_type,
        NormalizedEntryType::AssistantMessage
    ));
    assert_eq!(entries[0].content, "Final result");
}

#[test]
fn test_thinking_content() {
    let thinking_json = r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"thinking","thinking":"Let me think about this..."}]}}"#;
    let parsed: ClaudeJson = serde_json::from_str(thinking_json).unwrap();

    let entries = normalize(&parsed, "");
    assert_eq!(entries.len(), 1);
    assert!(matches!(
        entries[0].entry_type,
        NormalizedEntryType::Thinking
    ));
    assert_eq!(entries[0].content, "Let me think about this...");
}

#[test]
fn test_todo_tool_empty_list() {
    // Test TodoWrite with empty todo list
    let empty_data = ClaudeToolData::TodoWrite { todos: vec![] };

    let action_type = ClaudeLogProcessor::extract_action_type(&empty_data, "/tmp/test-worktree");
    let result = ClaudeLogProcessor::generate_concise_content(
        &empty_data,
        &action_type,
        "/tmp/test-worktree",
    );

    assert_eq!(result, "TODO list updated");
}

#[test]
fn test_glob_tool_content_extraction() {
    // Test Glob with pattern and path
    let glob_data = ClaudeToolData::Glob {
        pattern: "**/*.ts".to_string(),
        path: Some("/tmp/test-worktree/src".to_string()),
        limit: None,
    };

    let action_type = ClaudeLogProcessor::extract_action_type(&glob_data, "/tmp/test-worktree");
    let result = ClaudeLogProcessor::generate_concise_content(
        &glob_data,
        &action_type,
        "/tmp/test-worktree",
    );

    assert_eq!(result, "**/*.ts");
}

#[test]
fn test_glob_tool_pattern_only() {
    // Test Glob with pattern only
    let glob_data = ClaudeToolData::Glob {
        pattern: "*.js".to_string(),
        path: None,
        limit: None,
    };

    let action_type = ClaudeLogProcessor::extract_action_type(&glob_data, "/tmp/test-worktree");
    let result = ClaudeLogProcessor::generate_concise_content(
        &glob_data,
        &action_type,
        "/tmp/test-worktree",
    );

    assert_eq!(result, "*.js");
}

#[test]
fn test_ls_tool_content_extraction() {
    // Test LS with path
    let ls_data = ClaudeToolData::LS {
        path: "/tmp/test-worktree/components".to_string(),
    };

    let action_type = ClaudeLogProcessor::extract_action_type(&ls_data, "/tmp/test-worktree");
    let result =
        ClaudeLogProcessor::generate_concise_content(&ls_data, &action_type, "/tmp/test-worktree");

    assert_eq!(result, "List directory: components");
}

#[test]
fn test_path_relative_conversion() {
    // Test with relative path (should remain unchanged)
    let relative_result = make_path_relative("src/main.rs", "/tmp/test-worktree");
    assert_eq!(relative_result, "src/main.rs");

    // Test with absolute path (should become relative if possible)
    let test_worktree = "/tmp/test-worktree";
    let absolute_path = format!("{test_worktree}/src/main.rs");
    let absolute_result = make_path_relative(&absolute_path, test_worktree);
    assert_eq!(absolute_result, "src/main.rs");
}

#[tokio::test]
async fn test_streaming_patch_generation() {
    use std::sync::Arc;

    use workspace_utils::msg_store::MsgStore;

    let msg_store = Arc::new(MsgStore::new());
    let current_dir = std::path::PathBuf::from("/tmp/test-worktree");

    // Push some test messages
    msg_store
        .push_stdout(r#"{"type":"system","subtype":"init","session_id":"test123"}"#.to_string());
    msg_store.push_stdout(r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"Hello"}]}}"#.to_string());
    msg_store.push_finished();

    // Start normalization (this spawns async task)
    ClaudeLogProcessor::process_logs(
        msg_store.clone(),
        &current_dir,
        EntryIndexProvider::start_from(&msg_store),
        HistoryStrategy::Default,
    );

    // Give some time for async processing
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    // Check that the history now contains patch messages
    let history = msg_store.get_history();
    let patch_count = history
        .iter()
        .filter(|msg| matches!(msg, workspace_utils::log_msg::LogMsg::JsonPatch(_)))
        .count();
    assert!(
        patch_count > 0,
        "Expected JsonPatch messages to be generated from streaming processing"
    );
}

#[test]
fn test_session_id_extraction() {
    let system_json = r#"{"type":"system","session_id":"test-session-123"}"#;
    let parsed: ClaudeJson = serde_json::from_str(system_json).unwrap();

    // System messages no longer extract session_id
    assert_eq!(ClaudeLogProcessor::extract_session_id(&parsed), None);

    let tool_use_json =
        r#"{"type":"tool_use","tool_name":"read","input":{},"session_id":"another-session"}"#;
    let parsed_tool: ClaudeJson = serde_json::from_str(tool_use_json).unwrap();

    assert_eq!(
        ClaudeLogProcessor::extract_session_id(&parsed_tool),
        Some("another-session".to_string())
    );
}

#[test]
fn test_amp_tool_aliases_create_file_and_edit_file() {
    // Amp "create_file" should deserialize into Write with alias field "path"
    let assistant_with_create = r#"{
            "type":"assistant",
            "message":{
                "role":"assistant",
                "content":[
                    {"type":"tool_use","id":"t1","name":"create_file","input":{"path":"/tmp/work/src/new.txt","content":"hello"}}
                ]
            }
        }"#;
    let parsed: ClaudeJson = serde_json::from_str(assistant_with_create).unwrap();
    let entries = normalize(&parsed, "/tmp/work");
    assert_eq!(entries.len(), 1);
    match &entries[0].entry_type {
        NormalizedEntryType::ToolUse { action_type, .. } => match action_type {
            ActionType::FileEdit { path, .. } => assert_eq!(path, "src/new.txt"),
            other => panic!("Expected FileEdit, got {other:?}"),
        },
        other => panic!("Expected ToolUse, got {other:?}"),
    }

    // Amp "edit_file" should deserialize into Edit with aliases for path/old_str/new_str
    let assistant_with_edit = r#"{
            "type":"assistant",
            "message":{
                "role":"assistant",
                "content":[
                    {"type":"tool_use","id":"t2","name":"edit_file","input":{"path":"/tmp/work/README.md","old_str":"foo","new_str":"bar"}}
                ]
            }
        }"#;
    let parsed_edit: ClaudeJson = serde_json::from_str(assistant_with_edit).unwrap();
    let entries = normalize(&parsed_edit, "/tmp/work");
    assert_eq!(entries.len(), 1);
    match &entries[0].entry_type {
        NormalizedEntryType::ToolUse { action_type, .. } => match action_type {
            ActionType::FileEdit { path, .. } => assert_eq!(path, "README.md"),
            other => panic!("Expected FileEdit, got {other:?}"),
        },
        other => panic!("Expected ToolUse, got {other:?}"),
    }
}

#[test]
fn test_amp_tool_aliases_oracle_mermaid_codebase_undo() {
    // Oracle with task
    let oracle_json = r#"{
            "type":"assistant",
            "message":{
                "role":"assistant",
                "content":[
                    {"type":"tool_use","id":"t1","name":"oracle","input":{"task":"Assess project status"}}
                ]
            }
        }"#;
    let parsed: ClaudeJson = serde_json::from_str(oracle_json).unwrap();
    let entries = normalize(&parsed, "/tmp/work");
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].content, "Oracle: `Assess project status`");

    // Mermaid with code
    let mermaid_json = r#"{
            "type":"assistant",
            "message":{
                "role":"assistant",
                "content":[
                    {"type":"tool_use","id":"t2","name":"mermaid","input":{"code":"graph TD; A-->B;"}}
                ]
            }
        }"#;
    let parsed: ClaudeJson = serde_json::from_str(mermaid_json).unwrap();
    let entries = normalize(&parsed, "/tmp/work");
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].content, "Mermaid diagram");

    // CodebaseSearchAgent with query
    let csa_json = r#"{
            "type":"assistant",
            "message":{
                "role":"assistant",
                "content":[
                    {"type":"tool_use","id":"t3","name":"codebase_search_agent","input":{"query":"TODO markers"}}
                ]
            }
        }"#;
    let parsed: ClaudeJson = serde_json::from_str(csa_json).unwrap();
    let entries = normalize(&parsed, "/tmp/work");
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].content, "Codebase search: `TODO markers`");

    // UndoEdit shows file path when available
    let undo_json = r#"{
            "type":"assistant",
            "message":{
                "role":"assistant",
                "content":[
                    {"type":"tool_use","id":"t4","name":"undo_edit","input":{"path":"README.md"}}
                ]
            }
        }"#;
    let parsed: ClaudeJson = serde_json::from_str(undo_json).unwrap();
    let entries = normalize(&parsed, "/tmp/work");
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].content, "Undo edit: `README.md`");
}

#[test]
fn test_amp_bash_and_task_content() {
    // Bash with alias field cmd
    let bash_json = r#"{
            "type":"assistant",
            "message":{
                "role":"assistant",
                "content":[
                    {"type":"tool_use","id":"t1","name":"bash","input":{"cmd":"echo hello"}}
                ]
            }
        }"#;
    let parsed: ClaudeJson = serde_json::from_str(bash_json).unwrap();
    let entries = normalize(&parsed, "/tmp/work");
    assert_eq!(entries.len(), 1);
    // Content should display the command
    assert_eq!(entries[0].content, "echo hello");

    // Task content should include description/prompt wrapped in backticks
    let task_json = r#"{
            "type":"assistant",
            "message":{
                "role":"assistant",
                "content":[
                    {"type":"tool_use","id":"t2","name":"task","input":{"subagent_type":"Task","prompt":"Add header to README"}}
                ]
            }
        }"#;
    let parsed: ClaudeJson = serde_json::from_str(task_json).unwrap();
    let entries = normalize(&parsed, "/tmp/work");
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].content, "Task: `Add header to README`");
}

#[test]
fn test_task_description_or_prompt_backticks() {
    // When description present, use it
    let with_desc = r#"{
            "type":"assistant",
            "message":{
                "role":"assistant",
                "content":[
                    {"type":"tool_use","id":"t3","name":"Task","input":{
                        "subagent_type":"Task",
                        "prompt":"Fallback prompt",
                        "description":"Primary description"
                    }}
                ]
            }
        }"#;
    let parsed: ClaudeJson = serde_json::from_str(with_desc).unwrap();
    let entries = normalize(&parsed, "/tmp/work");
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].content, "Task: `Primary description`");

    // When description missing, fall back to prompt
    let no_desc = r#"{
            "type":"assistant",
            "message":{
                "role":"assistant",
                "content":[
                    {"type":"tool_use","id":"t4","name":"Task","input":{
                        "subagent_type":"Task",
                        "prompt":"Only prompt"
                    }}
                ]
            }
        }"#;
    let parsed: ClaudeJson = serde_json::from_str(no_desc).unwrap();
    let entries = normalize(&parsed, "/tmp/work");
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].content, "Task: `Only prompt`");
}

#[test]
fn test_tool_result_parsing_ignored() {
    let tool_result_json = r#"{"type":"tool_result","result":"File content here","is_error":false,"session_id":"test123"}"#;
    let parsed: ClaudeJson = serde_json::from_str(tool_result_json).unwrap();

    // Test session ID extraction from ToolResult still works
    assert_eq!(
        ClaudeLogProcessor::extract_session_id(&parsed),
        Some("test123".to_string())
    );

    // ToolResult messages should be ignored (produce no entries) until proper support is added
    let entries = normalize(&parsed, "");
    assert_eq!(entries.len(), 0);
}

#[test]
fn test_content_item_tool_result_ignored() {
    let assistant_with_tool_result = r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"tool_result","tool_use_id":"tool_123","content":"Operation completed","is_error":false}]}}"#;
    let parsed: ClaudeJson = serde_json::from_str(assistant_with_tool_result).unwrap();

    // ToolResult content items should be ignored (produce no entries) until proper support is added
    let entries = normalize(&parsed, "");
    assert_eq!(entries.len(), 0);
}

#[test]
fn test_api_key_source_warning() {
    // Test with ANTHROPIC_API_KEY - should generate warning
    let system_with_env_key = r#"{"type":"system","subtype":"init","apiKeySource":"ANTHROPIC_API_KEY","session_id":"test123"}"#;
    let parsed: ClaudeJson = serde_json::from_str(system_with_env_key).unwrap();
    let entries = normalize(&parsed, "");

    assert_eq!(entries.len(), 1);
    assert!(matches!(
        entries[0].entry_type,
        NormalizedEntryType::ErrorMessage {
            error_type: NormalizedEntryError::Other,
        },
    ));
    assert_eq!(
        entries[0].content,
        "Claude Code + ANTHROPIC_API_KEY detected. Usage will be billed via Anthropic pay-as-you-go instead of your Claude subscription. If this is unintended, please select the `disable_api_key` checkbox in the conding-agent-configurations settings page."
    );

    // Test with managed API key source - should not generate warning
    let system_with_managed_key = r#"{"type":"system","subtype":"init","apiKeySource":"/login managed key","session_id":"test123"}"#;
    let parsed_managed: ClaudeJson = serde_json::from_str(system_with_managed_key).unwrap();
    let entries_managed = normalize(&parsed_managed, "");

    assert_eq!(entries_managed.len(), 0); // No warning for managed key

    // Test with other apiKeySource values - should not generate warning
    let system_other_key =
        r#"{"type":"system","subtype":"init","apiKeySource":"OTHER_KEY","session_id":"test123"}"#;
    let parsed_other: ClaudeJson = serde_json::from_str(system_other_key).unwrap();
    let entries_other = normalize(&parsed_other, "");

    assert_eq!(entries_other.len(), 0); // No warning for other keys

    // Test with missing apiKeySource - should not generate warning
    let system_no_key = r#"{"type":"system","subtype":"init","session_id":"test123"}"#;
    let parsed_no_key: ClaudeJson = serde_json::from_str(system_no_key).unwrap();
    let entries_no_key = normalize(&parsed_no_key, "");

    assert_eq!(entries_no_key.len(), 0); // No warning when field is missing
}

#[test]
fn test_mixed_content_with_thinking_ignores_tool_result() {
    let complex_assistant_json = r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"thinking","thinking":"I need to read the file first"},{"type":"text","text":"I'll help you with that"},{"type":"tool_result","tool_use_id":"tool_789","content":"Success","is_error":false}]}}"#;
    let parsed: ClaudeJson = serde_json::from_str(complex_assistant_json).unwrap();

    let entries = normalize(&parsed, "");
    // Only thinking and text entries should be processed, tool_result ignored
    assert_eq!(entries.len(), 2);

    // Check thinking entry
    assert!(matches!(
        entries[0].entry_type,
        NormalizedEntryType::Thinking
    ));
    assert_eq!(entries[0].content, "I need to read the file first");

    // Check assistant message
    assert!(matches!(
        entries[1].entry_type,
        NormalizedEntryType::AssistantMessage
    ));
    assert_eq!(entries[1].content, "I'll help you with that");

    // ToolResult entry is ignored - no third entry
}

#[test]
fn test_control_request_with_permission_suggestions() {
    let control_request_json = r#"{"type":"control_request","request_id":"f559d907-b139-475b-addd-79c05591eb99","request":{"subtype":"can_use_tool","tool_name":"Bash","input":{"command":"./gradlew :web:testApi","timeout":300000,"description":"Run API tests"},"permission_suggestions":[{"type":"addRules","rules":[{"toolName":"Bash","ruleContent":"./gradlew :web:testApi:"}],"behavior":"allow","destination":"localSettings"}],"tool_use_id":"toolu_014PR3WXsJfiftSCbjcjEbeM"}}"#;
    let parsed: ClaudeJson = serde_json::from_str(control_request_json).unwrap();
    assert!(matches!(parsed, ClaudeJson::ControlRequest { .. }));
}

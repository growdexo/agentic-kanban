pub mod client;
pub mod json;
mod log_stream;
mod normalize_logs;
pub mod protocol;
pub mod slash_commands;
pub mod types;

mod executor;
mod streaming;
mod tool_display;

pub use executor::ClaudeCode;
use executor::base_command;
pub use json::{
    ClaudeContentBlockDelta, ClaudeContentItem, ClaudeEditItem, ClaudeJson, ClaudeMessage,
    ClaudeMessageContent, ClaudeMessageDelta, ClaudeModelUsage, ClaudePlugin, ClaudeStreamEvent,
    ClaudeTodoItem, ClaudeToolData, ClaudeUsage,
};
pub use normalize_logs::{ClaudeLogProcessor, HistoryStrategy};

use std::collections::HashMap;

use super::{
    json::{ClaudeContentBlockDelta, ClaudeContentItem},
    normalize_logs::ClaudeLogProcessor,
};
use crate::logs::utils::{EntryIndexProvider, patch::ConversationPatch};

pub(super) struct StreamingMessageState {
    role: String,
    contents: HashMap<usize, StreamingContentState>,
}

impl StreamingMessageState {
    pub(super) fn new(role: String) -> Self {
        Self {
            role,
            contents: HashMap::new(),
        }
    }

    pub(super) fn content_block_start(&mut self, index: usize, content_block: ClaudeContentItem) {
        if let Some(state) = StreamingContentState::from_content_block(content_block) {
            self.contents.insert(index, state);
        }
    }

    pub(super) fn apply_content_block_delta(
        &mut self,
        index: usize,
        delta: &ClaudeContentBlockDelta,
        worktree_path: &str,
        entry_index_provider: &EntryIndexProvider,
        last_assistant_message: &mut Option<String>,
    ) -> Option<json_patch::Patch> {
        if let std::collections::hash_map::Entry::Vacant(e) = self.contents.entry(index) {
            let new_state = StreamingContentState::from_delta(delta)?;
            e.insert(new_state);
        }

        let entry_state = self.contents.get_mut(&index)?;
        entry_state.apply_content_delta(delta);

        let content_item = entry_state.to_content_item();
        let entry = ClaudeLogProcessor::content_item_to_normalized_entry(
            &content_item,
            &self.role,
            worktree_path,
            last_assistant_message,
        )?;

        if let Some(existing_index) = entry_state.entry_index {
            Some(ConversationPatch::replace(existing_index, entry))
        } else {
            let entry_index = entry_index_provider.next();
            entry_state.entry_index = Some(entry_index);
            Some(ConversationPatch::add_normalized_entry(entry_index, entry))
        }
    }

    pub(super) fn content_entry_index(&self, content_index: usize) -> Option<usize> {
        self.contents
            .get(&content_index)
            .and_then(|s| s.entry_index)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum StreamingContentKind {
    Text,
    Thinking,
}

struct StreamingContentState {
    kind: StreamingContentKind,
    buffer: String,
    entry_index: Option<usize>,
}

impl StreamingContentState {
    fn from_content_block(content_block: ClaudeContentItem) -> Option<Self> {
        match content_block {
            ClaudeContentItem::Text { text } => Some(Self {
                kind: StreamingContentKind::Text,
                buffer: text,
                entry_index: None,
            }),
            ClaudeContentItem::Thinking { thinking } => Some(Self {
                kind: StreamingContentKind::Thinking,
                buffer: thinking,
                entry_index: None,
            }),
            _ => None,
        }
    }

    fn from_delta(delta: &ClaudeContentBlockDelta) -> Option<Self> {
        match delta {
            ClaudeContentBlockDelta::TextDelta { .. } => Some(Self {
                kind: StreamingContentKind::Text,
                buffer: String::new(),
                entry_index: None,
            }),
            ClaudeContentBlockDelta::ThinkingDelta { .. } => Some(Self {
                kind: StreamingContentKind::Thinking,
                buffer: String::new(),
                entry_index: None,
            }),
            _ => None,
        }
    }

    fn apply_content_delta(&mut self, delta: &ClaudeContentBlockDelta) {
        match (self.kind, delta) {
            (StreamingContentKind::Text, ClaudeContentBlockDelta::TextDelta { text }) => {
                self.buffer.push_str(text);
            }
            (
                StreamingContentKind::Thinking,
                ClaudeContentBlockDelta::ThinkingDelta { thinking },
            ) => {
                self.buffer.push_str(thinking);
            }
            _ => {
                tracing::warn!(
                    "Mismatched content types: delta {:?}, kind {:?}",
                    delta,
                    self.kind
                );
            }
        }
    }

    fn to_content_item(&self) -> ClaudeContentItem {
        match self.kind {
            StreamingContentKind::Text => ClaudeContentItem::Text {
                text: self.buffer.clone(),
            },
            StreamingContentKind::Thinking => ClaudeContentItem::Thinking {
                thinking: self.buffer.clone(),
            },
        }
    }
}

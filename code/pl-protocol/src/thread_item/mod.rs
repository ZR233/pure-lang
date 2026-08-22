//! Durable Thread timeline items with category-specific lifecycle payloads.

mod agent;
mod content;
mod inference;
mod tool;

use serde::{Deserialize, Serialize};

use crate::TurnState;

pub use agent::*;
pub use content::*;
pub use inference::*;
pub use tool::*;

/// Timeline 的唯一持久条目。`ordinal` 在首次插入时固定。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ThreadItem {
    pub id: String,
    pub thread_id: String,
    pub turn_id: String,
    pub ordinal: u64,
    pub revision: u64,
    pub created_at: i64,
    pub updated_at: i64,
    state: ThreadItemState,
}

impl ThreadItem {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        id: String,
        thread_id: String,
        turn_id: String,
        ordinal: u64,
        revision: u64,
        created_at: i64,
        updated_at: i64,
        state: ThreadItemState,
    ) -> Self {
        Self {
            id,
            thread_id,
            turn_id,
            ordinal,
            revision,
            created_at,
            updated_at,
            state,
        }
    }

    pub fn completed_user_message(
        id: String,
        thread_id: String,
        turn_id: String,
        text: String,
        attachments: Vec<ThreadAttachment>,
        completed_at: i64,
    ) -> Self {
        Self::new(
            id,
            thread_id,
            turn_id,
            0,
            0,
            completed_at,
            completed_at,
            ThreadItemState::Text(ThreadTextItem::new(
                ThreadTextChannel::User,
                text,
                attachments,
                ThreadContentLifecycle::completed(completed_at),
            )),
        )
    }

    pub fn state(&self) -> &ThreadItemState {
        &self.state
    }

    pub fn kind(&self) -> ThreadItemKind {
        self.state.kind()
    }

    pub fn is_terminal(&self) -> bool {
        self.state.is_terminal()
    }

    pub fn terminal_at(&self) -> Option<i64> {
        self.state.terminal_at()
    }

    pub fn failure(&self) -> Option<&str> {
        self.state.failure()
    }

    pub fn text(&self) -> Option<&ThreadTextItem> {
        match &self.state {
            ThreadItemState::Text(value) => Some(value),
            ThreadItemState::Thinking(_)
            | ThreadItemState::Tool(_)
            | ThreadItemState::Agent(_)
            | ThreadItemState::Turn(_)
            | ThreadItemState::Inference(_)
            | ThreadItemState::Plan(_)
            | ThreadItemState::File(_)
            | ThreadItemState::ContextCompaction(_) => None,
        }
    }

    pub fn thinking(&self) -> Option<&ThreadThinkingItem> {
        match &self.state {
            ThreadItemState::Thinking(value) => Some(value),
            ThreadItemState::Text(_)
            | ThreadItemState::Tool(_)
            | ThreadItemState::Agent(_)
            | ThreadItemState::Turn(_)
            | ThreadItemState::Inference(_)
            | ThreadItemState::Plan(_)
            | ThreadItemState::File(_)
            | ThreadItemState::ContextCompaction(_) => None,
        }
    }

    pub fn plan(&self) -> Option<&ThreadPlanItem> {
        match &self.state {
            ThreadItemState::Plan(value) => Some(value),
            ThreadItemState::Text(_)
            | ThreadItemState::Thinking(_)
            | ThreadItemState::Tool(_)
            | ThreadItemState::Agent(_)
            | ThreadItemState::Turn(_)
            | ThreadItemState::Inference(_)
            | ThreadItemState::File(_)
            | ThreadItemState::ContextCompaction(_) => None,
        }
    }

    pub fn tool(&self) -> Option<&ThreadToolItem> {
        match &self.state {
            ThreadItemState::Tool(value) => Some(value),
            ThreadItemState::Text(_)
            | ThreadItemState::Thinking(_)
            | ThreadItemState::Agent(_)
            | ThreadItemState::Turn(_)
            | ThreadItemState::Inference(_)
            | ThreadItemState::Plan(_)
            | ThreadItemState::File(_)
            | ThreadItemState::ContextCompaction(_) => None,
        }
    }

    pub fn apply_delta(&mut self, delta: &ThreadItemDelta) -> Result<bool, ThreadItemDeltaError> {
        if delta.item_id != self.id {
            return Err(ThreadItemDeltaError::WrongItem);
        }
        if delta.revision <= self.revision {
            return Ok(false);
        }
        let result = match (&mut self.state, &delta.delta) {
            (ThreadItemState::Text(item), ThreadItemDeltaState::Text { delta }) => {
                item.append(delta)
            }
            (
                ThreadItemState::Thinking(item),
                ThreadItemDeltaState::ThinkingSummary { chunk_index, delta },
            ) => item.append_summary(*chunk_index, delta),
            (
                ThreadItemState::Thinking(item),
                ThreadItemDeltaState::ThinkingContent { chunk_index, delta },
            ) => item.append_content(*chunk_index, delta),
            (ThreadItemState::Plan(item), ThreadItemDeltaState::Plan { delta }) => {
                item.append(delta)
            }
            (ThreadItemState::Tool(item), ThreadItemDeltaState::ToolArguments { delta }) => {
                item.append_arguments(delta)
            }
            (ThreadItemState::Tool(item), ThreadItemDeltaState::ToolResult { delta }) => {
                item.append_result(delta)
            }
            _ => Err("delta kind does not match thread item state"),
        };
        result.map_err(ThreadItemDeltaError::Illegal)?;
        self.revision = delta.revision;
        Ok(true)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", content = "data", rename_all = "camelCase")]
pub enum ThreadItemState {
    Text(ThreadTextItem),
    Thinking(ThreadThinkingItem),
    Tool(ThreadToolItem),
    Agent(ThreadAgentItem),
    Turn(ThreadTurnItem),
    Inference(ThreadInferenceItem),
    Plan(ThreadPlanItem),
    File(ThreadFileItem),
    ContextCompaction(ThreadContextCompactionItem),
}

impl ThreadItemState {
    pub fn kind(&self) -> ThreadItemKind {
        match self {
            Self::Text(_) => ThreadItemKind::Text,
            Self::Thinking(_) => ThreadItemKind::Thinking,
            Self::Tool(_) => ThreadItemKind::Tool,
            Self::Agent(_) => ThreadItemKind::Agent,
            Self::Turn(_) => ThreadItemKind::Turn,
            Self::Inference(_) => ThreadItemKind::Inference,
            Self::Plan(_) => ThreadItemKind::Plan,
            Self::File(_) => ThreadItemKind::File,
            Self::ContextCompaction(_) => ThreadItemKind::ContextCompaction,
        }
    }

    pub fn is_terminal(&self) -> bool {
        match self {
            Self::Text(value) => value.lifecycle().is_terminal(),
            Self::Thinking(value) => value.lifecycle().is_terminal(),
            Self::Tool(value) => value.state().is_terminal(),
            Self::Agent(value) => value.state().is_terminal(),
            Self::Turn(value) => value.state().is_terminal(),
            Self::Inference(value) => value.state().is_terminal(),
            Self::Plan(value) => value.lifecycle().is_terminal(),
            Self::File(_) | Self::ContextCompaction(_) => true,
        }
    }

    pub fn terminal_at(&self) -> Option<i64> {
        match self {
            Self::Text(value) => value.lifecycle().terminal_at(),
            Self::Thinking(value) => value.lifecycle().terminal_at(),
            Self::Tool(value) => value.state().terminal_at(),
            Self::Agent(value) => value.state().terminal_at(),
            Self::Turn(value) => value.state().completed_at(),
            Self::Inference(value) => value.state().terminal_at(),
            Self::Plan(value) => value.lifecycle().terminal_at(),
            Self::File(value) => Some(value.completed_at()),
            Self::ContextCompaction(value) => Some(value.compacted_at()),
        }
    }

    pub fn failure(&self) -> Option<&str> {
        match self {
            Self::Text(value) => value.lifecycle().failure(),
            Self::Thinking(value) => value.lifecycle().failure(),
            Self::Tool(value) => value.state().failure(),
            Self::Agent(value) => value.state().failure(),
            Self::Turn(value) => value
                .state()
                .failure()
                .map(|failure| failure.message.as_str()),
            Self::Inference(value) => value.state().failure(),
            Self::Plan(value) => value.lifecycle().failure(),
            Self::File(_) | Self::ContextCompaction(_) => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ThreadTurnItem {
    state: TurnState,
}

impl ThreadTurnItem {
    pub fn new(state: TurnState) -> Self {
        Self { state }
    }

    pub fn state(&self) -> &TurnState {
        &self.state
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ThreadItemKind {
    Text,
    Thinking,
    Tool,
    Agent,
    Turn,
    Inference,
    Plan,
    File,
    ContextCompaction,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ThreadAttachment {
    pub id: String,
    pub media_type: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub filename: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub width: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub height: Option<u32>,
    pub byte_size: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ThreadItemDelta {
    pub item_id: String,
    pub revision: u64,
    pub delta: ThreadItemDeltaState,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", content = "data", rename_all = "camelCase")]
pub enum ThreadItemDeltaState {
    Text { delta: String },
    ThinkingSummary { chunk_index: u32, delta: String },
    ThinkingContent { chunk_index: u32, delta: String },
    Plan { delta: String },
    ToolArguments { delta: String },
    ToolResult { delta: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ThreadItemDeltaError {
    WrongItem,
    Illegal(&'static str),
}

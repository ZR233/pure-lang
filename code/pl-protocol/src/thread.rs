use std::fmt;
use std::hash::{Hash, Hasher};

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::{
    InteractionRequest, McpHealthSnapshot, RuntimeCostAmount, ThreadItem, ThreadItemDelta,
    TodoListSnapshot, Turn,
};

pub const THREAD_SCHEMA_VERSION: u32 = 9;

/// 一个 agent 独占的对话和执行队列。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Thread {
    pub id: String,
    pub project_id: String,
    pub title: String,
    pub mode: ThreadMode,
    pub root_thread_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_thread_id: Option<String>,
    pub role: String,
    pub agent_path: String,
    pub status: ThreadStatus,
    pub created_at: i64,
    pub updated_at: i64,
    #[serde(default)]
    pub archived: bool,
}

impl Thread {
    pub fn placeholder(id: impl Into<String>) -> Self {
        let id = id.into();
        Self {
            project_id: String::new(),
            title: String::new(),
            mode: ThreadMode::simple(),
            root_thread_id: id.clone(),
            parent_thread_id: None,
            role: String::new(),
            agent_path: String::new(),
            status: ThreadStatus::Idle,
            created_at: 0,
            updated_at: 0,
            archived: false,
            id,
        }
    }
}

/// Skill 完整名称构成的动态模式 ID。
#[derive(Clone)]
pub struct ModeId(String);

impl ModeId {
    pub const SIMPLE: &'static str = "mode.simple";
    pub const TASK: &'static str = "mode.task";

    pub fn new(value: impl Into<String>) -> Result<Self, crate::UnknownLabelError> {
        let canonical = value.into();
        let custom = canonical.strip_prefix("mode.").unwrap_or_default();
        if custom.is_empty()
            || canonical.len() > 64
            || !custom.bytes().all(|byte| {
                byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'-' | b'_')
            })
        {
            return Err(crate::UnknownLabelError::new("ModeId", canonical));
        }
        Ok(Self(canonical))
    }

    pub fn simple() -> Self {
        Self(Self::SIMPLE.to_string())
    }

    pub fn task() -> Self {
        Self(Self::TASK.to_string())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn label(&self) -> &str {
        self.as_str()
    }

    pub fn from_label(label: &str) -> Result<Self, crate::UnknownLabelError> {
        Self::new(label)
    }
}

impl Default for ModeId {
    fn default() -> Self {
        Self::simple()
    }
}

impl fmt::Debug for ModeId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_tuple("ModeId").field(&self.0).finish()
    }
}

impl fmt::Display for ModeId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl PartialEq for ModeId {
    fn eq(&self, other: &Self) -> bool {
        self.as_str() == other.as_str()
    }
}

impl Eq for ModeId {}

impl PartialOrd for ModeId {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for ModeId {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.as_str().cmp(other.as_str())
    }
}

impl Hash for ModeId {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.as_str().hash(state);
    }
}

impl Serialize for ModeId {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for ModeId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::new(value).map_err(serde::de::Error::custom)
    }
}

pub type ThreadMode = ModeId;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ThreadStatus {
    Idle,
    Queued,
    Running,
    WaitingTool,
    WaitingInteraction,
    Cancelling,
    Closing,
    Closed,
    Faulted,
}

/// Timeline 记录是否仍属于后续模型的有效上下文。
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ThreadContextDisposition {
    #[default]
    Active,
    RolledBack,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ThreadSnapshot {
    pub schema_version: u32,
    pub revision: u64,
    pub thread: Thread,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_turn: Option<Turn>,
    #[serde(default)]
    pub items: Vec<ThreadItem>,
    #[serde(default)]
    pub interactions: Vec<InteractionRequest>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runtime: Option<ThreadRuntimeSnapshot>,
}

impl ThreadSnapshot {
    pub fn empty(thread_id: impl Into<String>) -> Self {
        Self {
            schema_version: THREAD_SCHEMA_VERSION,
            revision: 0,
            thread: Thread::placeholder(thread_id),
            active_turn: None,
            items: Vec::new(),
            interactions: Vec::new(),
            runtime: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ThreadRuntimeSnapshot {
    pub thread_id: String,
    pub usage: ThreadRuntimeUsage,
    #[serde(default)]
    pub turn_completion_tokens: u64,
    #[serde(default)]
    pub turn_decode_millis: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub todo: Option<TodoListSnapshot>,
    #[serde(default)]
    pub active_skills: Vec<String>,
    #[serde(default)]
    pub active_mcp_servers: Vec<String>,
    #[serde(default)]
    pub active_lsp_servers: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub progress: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mcp_health: Option<McpHealthSnapshot>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workflow: Option<crate::WorkflowRuntimeSnapshot>,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ThreadRuntimeUsage {
    pub model: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_window: Option<u64>,
    pub latest_context_tokens: u64,
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub cached_prompt_tokens: u64,
    #[serde(default)]
    pub cache_write_tokens: u64,
    #[serde(default)]
    pub cache_miss_tokens: u64,
    #[serde(default)]
    pub reasoning_tokens: u64,
    #[serde(default)]
    pub inference_count: u64,
    pub total_tokens: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_hit_rate: Option<f64>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub estimated_costs: Vec<RuntimeCostAmount>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub estimated_cache_savings: Vec<RuntimeCostAmount>,
    #[serde(default)]
    pub has_unpriced_usage: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt_generation: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt_cache_policy: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prefix_changed_reason: Option<crate::PromptPrefixChangedReason>,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    tag = "type"
)]
pub enum ThreadNotification {
    TurnStarted {
        turn: Turn,
    },
    TurnUpdated {
        turn: Turn,
    },
    TurnCompleted {
        turn: Turn,
    },
    ItemStarted {
        item: Box<ThreadItem>,
    },
    ItemDelta {
        delta: ThreadItemDelta,
    },
    ItemCompleted {
        item: Box<ThreadItem>,
    },
    InteractionChanged {
        interaction: Box<InteractionRequest>,
    },
    ThreadRuntimeUpdated {
        runtime: Box<ThreadRuntimeSnapshot>,
    },
    Lagged {
        dropped: u64,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ThreadNotificationEnvelope {
    pub thread_id: String,
    pub revision: u64,
    pub emitted_at: i64,
    pub notification: ThreadNotification,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    tag = "type"
)]
pub enum ThreadSubscriptionUpdate {
    Snapshot {
        snapshot: Box<ThreadSnapshot>,
    },
    Notification {
        notification: Box<ThreadNotificationEnvelope>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ThreadSubscriptionRequest {
    pub thread_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ThreadTurnPage {
    pub turns: Vec<ThreadTurnHistory>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ThreadTurnHistory {
    pub turn: Turn,
    pub items: Vec<ThreadItem>,
    #[serde(default)]
    pub context_disposition: ThreadContextDisposition,
}

crate::impl_labeled_enum!(
    ThreadStatus,
    "ThreadStatus",
    [
        ThreadStatus::Idle => "idle",
        ThreadStatus::Queued => "queued",
        ThreadStatus::Running => "running",
        ThreadStatus::WaitingTool => "waitingTool",
        ThreadStatus::WaitingInteraction => "waitingInteraction",
        ThreadStatus::Cancelling => "cancelling",
        ThreadStatus::Closing => "closing",
        ThreadStatus::Closed => "closed",
        ThreadStatus::Faulted => "faulted",
    ]
);

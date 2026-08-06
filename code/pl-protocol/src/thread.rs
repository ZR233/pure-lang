use serde::{Deserialize, Serialize};

use crate::{
    InteractionRequest, McpHealthSnapshot, RuntimeCostAmount, TodoListSnapshot, TokenUsageSnapshot,
};

pub const THREAD_SCHEMA_VERSION: u32 = 1;

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
            mode: ThreadMode::Simple,
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

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ThreadMode {
    Simple,
    Task,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ThreadStatus {
    Idle,
    Running,
    Waiting,
    Completed,
    Failed,
    Closed,
}

/// Thread 中一次由明确输入启动的执行。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Turn {
    pub id: String,
    pub thread_id: String,
    pub state: TurnState,
    pub started_at: Option<i64>,
    pub updated_at: i64,
    pub completed_at: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    tag = "status"
)]
pub enum TurnState {
    Queued,
    InProgress { phase: TurnPhase },
    Completed,
    Failed { reason: String },
    Interrupted { reason: String },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum TurnPhase {
    Preparing,
    Thinking,
    Responding,
    Planning,
    RunningTool,
    WaitingInteraction,
    Persisting,
}

/// Timeline 的唯一持久条目。`ordinal` 在首次插入时固定。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ThreadItem {
    pub id: String,
    pub thread_id: String,
    pub turn_id: String,
    pub ordinal: u64,
    #[serde(default)]
    pub revision: u64,
    pub status: ThreadItemStatus,
    pub created_at: i64,
    pub updated_at: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub completed_at: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    pub content: ThreadItemContent,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage: Option<TokenUsageSnapshot>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    tag = "type"
)]
pub enum ThreadItemContent {
    UserMessage {
        #[serde(default, skip_serializing_if = "String::is_empty")]
        text: String,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        attachments: Vec<ThreadAttachment>,
    },
    AgentMessage {
        channel: AgentMessageChannel,
        #[serde(default, skip_serializing_if = "String::is_empty")]
        text: String,
    },
    Reasoning {
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        summary: Vec<String>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        content: Vec<String>,
    },
    Plan {
        #[serde(default, skip_serializing_if = "String::is_empty")]
        content: String,
    },
    ToolCall {
        tool: ThreadToolCall,
    },
    File {
        path: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        media_type: Option<String>,
    },
    /// 模型输入审计记录；Bridge 不得向 Flutter 暴露。
    ContextPatch {
        generation: u64,
        fixed_prefix_hash: String,
        tool_schema_hash: String,
        context_hash: String,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        changed_section_ids: Vec<String>,
        prefix_changed_reason: crate::PromptPrefixChangedReason,
    },
    /// 仅供模型上下文重建，Bridge 不得向 Flutter 暴露。
    ContextCompaction {
        before_tokens: u64,
        after_tokens: u64,
        compacted_at: i64,
    },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum AgentMessageChannel {
    Commentary,
    Final,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ThreadItemStatus {
    Started,
    Streaming,
    AwaitingApproval,
    Approved,
    Denied,
    Running,
    Completed,
    Failed,
    Interrupted,
    BudgetLimited,
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ThreadToolCall {
    pub tool_call_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub call_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_item_id: Option<String>,
    pub name: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub arguments: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub output_artifacts: Vec<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
    #[serde(default)]
    pub timed_out: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub working_directory: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub denial_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ThreadItemDelta {
    pub item_id: String,
    pub revision: u64,
    pub field: ThreadItemDeltaField,
    pub delta: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chunk_index: Option<u32>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ThreadItemDeltaField {
    Text,
    #[serde(rename = "reasoning.summary")]
    ReasoningSummary,
    #[serde(rename = "reasoning.content")]
    ReasoningContent,
    PlanContent,
    #[serde(rename = "tool.arguments")]
    ToolArguments,
    #[serde(rename = "tool.result")]
    ToolResult,
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
}

use serde::Deserialize;
use serde::Serialize;
use tokio::sync::broadcast;

use pl_protocol::{
    AgentRuntimeDelta, AgentStatus, BudgetLimitKind, BudgetUsage, ErrorSeverity,
    InteractionChangedEvent, PlanLifecycleEvent, SkillActivation, SubAgentActivityKind,
    TodoListSnapshot, TokenUsageSnapshot,
};

pub type AgentEventSender = broadcast::Sender<AgentEvent>;
pub type AgentEventReceiver = broadcast::Receiver<AgentEvent>;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum AgentEvent {
    TracePartStarted {
        item: TracePart,
    },
    TracePartDelta {
        event: TracePartDeltaEvent,
    },
    TracePartCompleted {
        item: TracePart,
    },
    TracePartFailed {
        item: TracePart,
        error: String,
    },
    InteractionChanged {
        event: InteractionChangedEvent,
    },
    AgentStateChanged {
        id: String,
        path: String,
        #[serde(rename = "parentPath")]
        parent_path: Option<String>,
        role: String,
        task: String,
        status: AgentStatus,
        summary: Option<String>,
        depth: u32,
        error: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reason: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        budget_limit_kind: Option<BudgetLimitKind>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        budget_usage: Option<BudgetUsage>,
        #[serde(rename = "updatedAt")]
        updated_at: i64,
    },
    AgentRuntimeUpdated {
        delta: AgentRuntimeDelta,
    },
    SkillActivated {
        activation: SkillActivation,
    },
    SubAgentActivity {
        call_id: String,
        occurred_at: i64,
        agent_id: Option<String>,
        path: Option<String>,
        parent_path: Option<String>,
        kind: SubAgentActivityKind,
        status: Option<AgentStatus>,
        message: Option<String>,
        timed_out: Option<bool>,
        error: Option<String>,
    },
    TodoListUpdated {
        snapshot: TodoListSnapshot,
    },
    TurnInterrupted {
        reason: String,
    },
    TurnBudgetLimited {
        reason: String,
        limit_kind: BudgetLimitKind,
        usage: BudgetUsage,
    },
    Done,
    Error {
        message: String,
        severity: ErrorSeverity,
    },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum TracePartKind {
    Text,
    Thinking,
    Tool,
    Agent,
    Turn,
    Inference,
    Plan,
}

impl TracePartKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Text => "text",
            Self::Thinking => "thinking",
            Self::Tool => "tool",
            Self::Agent => "agent",
            Self::Turn => "turn",
            Self::Inference => "inference",
            Self::Plan => "plan",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum TracePartStatus {
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

impl TracePartStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Started => "started",
            Self::Streaming => "streaming",
            Self::AwaitingApproval => "awaitingApproval",
            Self::Approved => "approved",
            Self::Denied => "denied",
            Self::Running => "running",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Interrupted => "interrupted",
            Self::BudgetLimited => "budgetLimited",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub enum TracePartSource {
    #[default]
    Model,
    Runtime,
}

impl TracePartSource {
    pub fn is_model(&self) -> bool {
        matches!(self, Self::Model)
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum TraceTextChannel {
    User,
    Commentary,
    Final,
}

impl TraceTextChannel {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Commentary => "commentary",
            Self::Final => "final",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TraceThinkingChunk {
    pub chunk_index: u32,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TraceToolPart {
    pub tool_call_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub call_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_item_id: Option<String>,
    pub name: String,
    pub arguments: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
    pub timed_out: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub output_artifacts: Vec<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_metrics: Option<TraceToolOutputMetrics>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub working_directory: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub denial_reason: Option<String>,
}

/// 工具完整输出、模型视图和 artifact 的字节统计。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TraceToolOutputMetrics {
    pub raw_bytes: u64,
    pub model_visible_bytes: u64,
    pub artifact_bytes: u64,
    pub result_hash: String,
    pub cache_hit: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TraceAgentPart {
    pub id: String,
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_path: Option<String>,
    pub role: String,
    pub task: String,
    pub status: AgentStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    pub depth: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TraceInferencePart {
    pub inference_id: String,
    pub model: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TraceAttachment {
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
pub struct TracePart {
    pub turn_id: String,
    pub item_id: String,
    #[serde(alias = "sequence")]
    pub started_sequence: u64,
    #[serde(default)]
    pub revision: u64,
    pub kind: TracePartKind,
    pub status: TracePartStatus,
    pub created_at: i64,
    pub updated_at: i64,
    #[serde(default, skip_serializing_if = "TracePartSource::is_model")]
    pub source: TracePartSource,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text_channel: Option<TraceTextChannel>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub content: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub attachments: Vec<TraceAttachment>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub thinking_chunks: Vec<TraceThinkingChunk>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool: Option<TraceToolPart>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent: Option<TraceAgentPart>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inference: Option<TraceInferencePart>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage: Option<TokenUsageSnapshot>,
}

impl TracePart {
    pub fn text(
        turn_id: impl Into<String>,
        item_id: impl Into<String>,
        sequence: u64,
        text_channel: TraceTextChannel,
        content: impl Into<String>,
        status: TracePartStatus,
        timestamp: i64,
    ) -> Self {
        Self {
            turn_id: turn_id.into(),
            item_id: item_id.into(),
            started_sequence: sequence,
            revision: 0,
            kind: TracePartKind::Text,
            status,
            created_at: timestamp,
            updated_at: timestamp,
            source: TracePartSource::Model,
            text_channel: Some(text_channel),
            content: content.into(),
            attachments: Vec::new(),
            thinking_chunks: Vec::new(),
            tool: None,
            agent: None,
            inference: None,
            usage: None,
        }
    }

    pub fn runtime_commentary(
        turn_id: impl Into<String>,
        item_id: impl Into<String>,
        sequence: u64,
        content: impl Into<String>,
        timestamp: i64,
    ) -> Self {
        Self {
            turn_id: turn_id.into(),
            item_id: item_id.into(),
            started_sequence: sequence,
            revision: 0,
            kind: TracePartKind::Text,
            status: TracePartStatus::Completed,
            created_at: timestamp,
            updated_at: timestamp,
            source: TracePartSource::Runtime,
            text_channel: Some(TraceTextChannel::Commentary),
            content: content.into(),
            attachments: Vec::new(),
            thinking_chunks: Vec::new(),
            tool: None,
            agent: None,
            inference: None,
            usage: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    tag = "type"
)]
pub enum TraceDelta {
    Text {
        text_channel: TraceTextChannel,
        delta: String,
    },
    Thinking {
        chunk_index: u32,
        delta: String,
    },
    ToolArguments {
        delta: String,
    },
    ToolResult {
        delta: String,
    },
    Plan {
        delta: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TracePartDeltaEvent {
    pub turn_id: String,
    pub item_id: String,
    #[serde(alias = "sequence")]
    pub started_sequence: u64,
    #[serde(default)]
    pub revision: u64,
    pub kind: TracePartKind,
    pub status: TracePartStatus,
    pub created_at: i64,
    pub updated_at: i64,
    pub delta: TraceDelta,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TracePartDelta {
    pub item_id: String,
    pub field: TracePartDeltaField,
    pub delta: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chunk_index: Option<u32>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum TracePartDeltaField {
    Content,
    PlanContent,
    ThinkingChunk,
    ToolArguments,
    ToolResult,
}

/// Append-only internal trace event for core diagnostics.
///
/// Studio UI may only receive these events after `pl-core` maps them into
/// durable message/part snapshots or live-only part deltas.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TraceEvent {
    pub session_id: String,
    pub sequence: u64,
    pub timestamp: i64,
    pub kind: TraceEventKind,
}

/// Snapshot of tool names enabled for a single turn.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct EnabledToolsEvent {
    pub turn_id: String,
    pub tools: Vec<String>,
}

/// Item-first trace events for turn, inference, text, thinking, tool and agent lifecycle.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    tag = "type"
)]
pub enum TraceEventKind {
    TracePartStarted { item: TracePart },
    TracePartDelta { event: TracePartDeltaEvent },
    TracePartCompleted { item: TracePart },
    TracePartFailed { item: TracePart, error: String },
    PlanLifecycleChanged { event: PlanLifecycleEvent },
    InteractionChanged { event: InteractionChangedEvent },
    SkillActivated { activation: SkillActivation },
    EnabledToolsRecorded { event: EnabledToolsEvent },
}

#[cfg(test)]
mod tests;

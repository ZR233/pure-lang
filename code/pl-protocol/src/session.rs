use serde::{Deserialize, Serialize};

use crate::{
    AgentStatus, BudgetLimitKind, BudgetUsage, ErrorSeverity, InteractionChangedEvent,
    InteractionRequest, McpHealthSnapshot, PlanLifecycleEvent, RuntimeCostAmount, SkillActivation,
    SubAgentActivityKind, TodoListSnapshot, TokenUsageSnapshot,
};

pub const SESSION_EVENT_SCHEMA_VERSION: u32 = 3;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SessionEventEnvelope {
    pub event_id: String,
    pub session_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_agent_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub turn_id: Option<String>,
    pub emitted_at: i64,
    pub position: SessionEventPosition,
    pub kind: SessionEventKind,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    tag = "persistence"
)]
pub enum SessionEventPosition {
    Durable { sequence: u64 },
    Transient { revision: u64 },
}

impl SessionEventPosition {
    pub fn durable_sequence(self) -> Option<u64> {
        match self {
            Self::Durable { sequence } => Some(sequence),
            Self::Transient { revision: _ } => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    tag = "type"
)]
pub enum SessionStreamFrame {
    Snapshot { snapshot: Box<SessionViewSnapshot> },
    Event { event: Box<SessionEventEnvelope> },
    ResyncRequired { reason: SessionResyncReason },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    tag = "type"
)]
pub enum SessionResyncReason {
    Lagged {
        events: u64,
    },
    CursorExpired {
        requested: u64,
        oldest_available: u64,
    },
    ReplayLimitExceeded {
        available: u64,
        limit: u64,
    },
    RevisionGap {
        part_id: String,
        expected: u64,
        actual: u64,
    },
    ProjectionInvariant {
        message: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SessionSubscriptionRequest {
    pub session_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub after_sequence: Option<u64>,
}

impl SessionSubscriptionRequest {
    pub fn new(session_id: impl Into<String>) -> Self {
        Self {
            session_id: session_id.into(),
            after_sequence: None,
        }
    }

    pub fn after(mut self, sequence: u64) -> Self {
        self.after_sequence = Some(sequence);
        self
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    tag = "type"
)]
pub enum SessionEventKind {
    TurnChanged {
        turn: SessionTurn,
    },
    MessageChanged {
        message: Box<SessionMessage>,
    },
    MessageRemoved {
        message_id: String,
    },
    PartChanged {
        part: Box<SessionPart>,
    },
    PartRemoved {
        message_id: String,
        part_id: String,
    },
    PartDelta {
        delta: SessionPartDelta,
    },
    InteractionChanged {
        event: Box<InteractionChangedEvent>,
    },
    AgentChanged {
        agent: SessionAgentSnapshot,
    },
    TimelineEventAppended {
        event: SessionTimelineEvent,
    },
    RuntimeChanged {
        runtime: Box<SessionRuntimeSnapshot>,
    },
    SkillActivated {
        activation: SkillActivation,
    },
    PlanChanged {
        event: PlanLifecycleEvent,
    },
    ContextCompacted {
        compaction: SessionContextCompaction,
    },
    ErrorOccurred {
        message: String,
        severity: ErrorSeverity,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SessionViewSnapshot {
    pub schema_version: u32,
    pub session_id: String,
    pub through_sequence: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner: Option<SessionOwnerSnapshot>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub turn: Option<SessionTurn>,
    #[serde(default)]
    pub messages: Vec<SessionMessage>,
    #[serde(default)]
    pub parts: Vec<SessionPart>,
    #[serde(default)]
    pub interactions: Vec<InteractionRequest>,
    #[serde(default)]
    pub agents: Vec<SessionAgentSnapshot>,
    #[serde(default)]
    pub timeline_events: Vec<SessionTimelineEvent>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runtime: Option<SessionRuntimeSnapshot>,
    #[serde(default)]
    pub activated_skills: Vec<SkillActivation>,
    #[serde(default)]
    pub plan_events: Vec<PlanLifecycleEvent>,
}

impl SessionViewSnapshot {
    pub fn empty(session_id: impl Into<String>) -> Self {
        Self {
            schema_version: SESSION_EVENT_SCHEMA_VERSION,
            session_id: session_id.into(),
            through_sequence: 0,
            owner: None,
            turn: None,
            messages: Vec::new(),
            parts: Vec::new(),
            interactions: Vec::new(),
            agents: Vec::new(),
            timeline_events: Vec::new(),
            runtime: None,
            activated_skills: Vec::new(),
            plan_events: Vec::new(),
        }
    }
}

/// 单一 session 的稳定 owner 身份。
///
/// Session timeline、Todo、context、skills 与 interaction 都必须来自这个 agent。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SessionOwnerSnapshot {
    pub agent_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SessionMessage {
    pub message_id: String,
    pub session_id: String,
    pub turn_id: String,
    pub role: SessionMessageRole,
    pub status: SessionMessageStatus,
    pub created_at: i64,
    pub updated_at: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub completed_at: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(default = "empty_object", skip_serializing_if = "is_empty_object")]
    pub metadata: serde_json::Value,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum SessionMessageRole {
    User,
    Assistant,
    System,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum SessionMessageStatus {
    Queued,
    Streaming,
    Completed,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SessionPart {
    pub part_id: String,
    pub message_id: String,
    pub session_id: String,
    pub turn_id: String,
    pub order: u64,
    #[serde(default)]
    pub revision: u64,
    pub status: SessionPartStatus,
    pub created_at: i64,
    pub updated_at: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub completed_at: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    pub content: SessionPartContent,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage: Option<TokenUsageSnapshot>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub synthetic: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub ignored: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    tag = "type"
)]
pub enum SessionPartContent {
    Text {
        channel: SessionTextChannel,
        #[serde(default, skip_serializing_if = "String::is_empty")]
        text: String,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        attachments: Vec<SessionAttachment>,
    },
    Reasoning {
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        summary: Vec<String>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        content: Vec<String>,
    },
    Tool {
        tool: SessionToolPart,
    },
    Agent {
        agent: SessionAgentPart,
    },
    Turn,
    Inference {
        inference_id: String,
        model: String,
    },
    Plan {
        #[serde(default, skip_serializing_if = "String::is_empty")]
        content: String,
    },
    File {
        path: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        media_type: Option<String>,
    },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum SessionPartStatus {
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

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum SessionTextChannel {
    User,
    Commentary,
    Final,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SessionAttachment {
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
pub struct SessionToolPart {
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub activity_group_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SessionAgentPart {
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
pub struct SessionPartDelta {
    pub part_id: String,
    pub revision: u64,
    pub field: SessionPartDeltaField,
    pub delta: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chunk_index: Option<u32>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum SessionPartDeltaField {
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SessionTurn {
    pub turn_id: String,
    pub session_id: String,
    pub state: SessionTurnState,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    tag = "status"
)]
pub enum SessionTurnState {
    Queued,
    InProgress { activity: SessionTurnActivity },
    Completed,
    Failed { reason: String },
    Cancelled { reason: String },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum SessionTurnActivity {
    Preparing,
    Thinking,
    Responding,
    Planning,
    RunningTool,
    WaitingForApproval,
    WaitingForUserInput,
    WaitingForPlanConfirmation,
    Persisting,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SessionAgentSnapshot {
    pub id: String,
    pub session_id: String,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub budget_limit_kind: Option<BudgetLimitKind>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub budget_usage: Option<BudgetUsage>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runtime_usage: Option<SessionRuntimeUsage>,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SessionTimelineEvent {
    pub event_id: String,
    pub session_id: String,
    pub sequence: u64,
    pub created_at: i64,
    pub kind: SessionTimelineEventKind,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    tag = "type"
)]
pub enum SessionTimelineEventKind {
    SubAgentActivity {
        call_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        agent_id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        path: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        parent_path: Option<String>,
        kind: SubAgentActivityKind,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        status: Option<AgentStatus>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        message: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        timed_out: Option<bool>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        error: Option<String>,
    },
    TodoListChanged {
        snapshot: TodoListSnapshot,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SessionRuntimeSnapshot {
    pub session_id: String,
    pub usage: SessionRuntimeUsage,
    #[serde(default)]
    pub active_skills: Vec<String>,
    #[serde(default)]
    pub active_mcp_servers: Vec<String>,
    #[serde(default)]
    pub active_lsp_servers: Vec<String>,
    #[serde(default)]
    pub agent_count: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mcp_health: Option<McpHealthSnapshot>,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SessionRuntimeUsage {
    pub model: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_window: Option<u64>,
    pub latest_context_tokens: u64,
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub cached_prompt_tokens: u64,
    pub total_tokens: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_hit_rate: Option<f64>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub estimated_costs: Vec<RuntimeCostAmount>,
    #[serde(default)]
    pub has_unpriced_usage: bool,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SessionContextCompaction {
    pub before_tokens: u64,
    pub after_tokens: u64,
    pub compacted_at: i64,
}

fn empty_object() -> serde_json::Value {
    serde_json::Value::Object(serde_json::Map::new())
}

fn is_empty_object(value: &serde_json::Value) -> bool {
    value.as_object().is_some_and(serde_json::Map::is_empty)
}

fn is_false(value: &bool) -> bool {
    !*value
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;

    #[test]
    fn empty_snapshot_round_trips() {
        let snapshot = SessionViewSnapshot::empty("session-1");
        let encoded = serde_json::to_value(&snapshot).expect("encode");
        let decoded = serde_json::from_value(encoded).expect("decode");
        assert_eq!(snapshot, decoded);
    }

    #[test]
    fn durable_position_exposes_cursor() {
        assert_eq!(
            SessionEventPosition::Durable { sequence: 7 }.durable_sequence(),
            Some(7)
        );
        assert_eq!(
            SessionEventPosition::Transient { revision: 2 }.durable_sequence(),
            None
        );
    }
}

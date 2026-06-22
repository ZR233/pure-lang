use serde::{Deserialize, Serialize};

use crate::{
    AgentStatus, BudgetLimitKind, BudgetUsage, InteractionChangedEvent, PlanLifecycleEvent,
    RuntimeCostAmount, SkillActivation, TokenUsageSnapshot,
};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StudioEventEnvelope {
    pub event_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub turn_id: Option<String>,
    pub sequence: u64,
    pub created_at: i64,
    pub kind: StudioEventKind,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    tag = "type"
)]
pub enum StudioEventKind {
    TurnChanged {
        turn: StudioTurn,
    },
    MessageUpdated {
        message: Box<StudioMessage>,
    },
    MessageRemoved {
        message_id: String,
    },
    MessagePartUpdated {
        part: Box<StudioPart>,
    },
    MessagePartRemoved {
        message_id: String,
        part_id: String,
    },
    MessagePartDelta {
        delta: StudioPartDelta,
    },
    InteractionChanged {
        event: Box<InteractionChangedEvent>,
    },
    AgentChanged {
        agent: StudioAgentSnapshot,
    },
    AgentTimelineChanged {
        event: StudioAgentTimelineEvent,
    },
    SessionRuntimeChanged {
        runtime: StudioSessionRuntime,
    },
    SkillActivated {
        activation: SkillActivation,
    },
    PlanLifecycleChanged {
        event: PlanLifecycleEvent,
    },
    SessionHandoffChanged {
        handoff: StudioSessionHandoff,
    },
    SessionListChanged {
        project_id: String,
        sessions: Vec<StudioSessionSummary>,
    },
    McpHealthChanged {
        health: StudioMcpHealth,
    },
    LspHealthChanged {
        health: StudioLspHealth,
    },
    Stale {
        lagged_events: u64,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct StudioMessage {
    pub message_id: String,
    pub session_id: String,
    pub turn_id: String,
    pub role: StudioMessageRole,
    pub status: StudioMessageStatus,
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
pub enum StudioMessageRole {
    User,
    Assistant,
    System,
}

impl StudioMessageRole {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Assistant => "assistant",
            Self::System => "system",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum StudioMessageStatus {
    Queued,
    Streaming,
    Completed,
    Failed,
    Cancelled,
}

impl StudioMessageStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Streaming => "streaming",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct StudioPart {
    pub part_id: String,
    pub message_id: String,
    pub session_id: String,
    pub turn_id: String,
    pub part_type: StudioPartType,
    pub order: u64,
    pub status: StudioPartStatus,
    pub created_at: i64,
    pub updated_at: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub completed_at: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text_channel: Option<StudioTextChannel>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub text: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub attachments: Vec<StudioAttachment>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool: Option<StudioToolPart>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent: Option<StudioAgentPart>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inference: Option<StudioInferencePart>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plan: Option<StudioPlanPart>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file: Option<StudioFilePart>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage: Option<TokenUsageSnapshot>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub synthetic: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub ignored: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum StudioPartType {
    Text,
    Reasoning,
    Tool,
    Agent,
    Turn,
    Inference,
    Plan,
    File,
}

impl StudioPartType {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Text => "text",
            Self::Reasoning => "reasoning",
            Self::Tool => "tool",
            Self::Agent => "agent",
            Self::Turn => "turn",
            Self::Inference => "inference",
            Self::Plan => "plan",
            Self::File => "file",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum StudioPartStatus {
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

impl StudioPartStatus {
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

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum StudioTextChannel {
    User,
    Commentary,
    Final,
}

impl StudioTextChannel {
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
pub struct StudioAttachment {
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
pub struct StudioToolPart {
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
pub struct StudioAgentPart {
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
pub struct StudioInferencePart {
    pub inference_id: String,
    pub model: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct StudioPlanPart {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct StudioFilePart {
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub media_type: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct StudioPartDelta {
    pub session_id: String,
    pub message_id: String,
    pub part_id: String,
    pub field: StudioPartDeltaField,
    pub delta: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chunk_index: Option<u32>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum StudioPartDeltaField {
    Text,
    ReasoningText,
    PlanContent,
    #[serde(rename = "tool.arguments")]
    ToolArguments,
    #[serde(rename = "tool.result")]
    ToolResult,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct StudioTurn {
    pub turn_id: String,
    pub session_id: String,
    pub status: StudioTurnStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum StudioTurnStatus {
    Queued,
    ContextLoading,
    WaitingForModel,
    Streaming,
    WaitingForInteraction,
    RunningTool,
    Persisting,
    Completed,
    Failed,
    Cancelled,
}

impl StudioTurnStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::ContextLoading => "contextLoading",
            Self::WaitingForModel => "waitingForModel",
            Self::Streaming => "streaming",
            Self::WaitingForInteraction => "waitingForInteraction",
            Self::RunningTool => "runningTool",
            Self::Persisting => "persisting",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StudioAgentSnapshot {
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
    pub runtime_usage: Option<StudioRuntimeUsage>,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StudioAgentTimelineEvent {
    pub event_id: String,
    pub session_id: String,
    pub sequence: u64,
    pub created_at: i64,
    pub kind: StudioAgentTimelineEventKind,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    tag = "type"
)]
pub enum StudioAgentTimelineEventKind {
    SpawnBegin {
        call_id: String,
        sender_path: String,
        task_name: String,
        prompt: String,
        role: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        model: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reasoning_effort: Option<String>,
    },
    SpawnEnd {
        call_id: String,
        sender_path: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        agent_id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        path: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        role: Option<String>,
        status: AgentStatus,
        prompt: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        error: Option<String>,
    },
    InteractionBegin {
        call_id: String,
        sender_path: String,
        receiver_path: String,
        prompt: String,
    },
    InteractionEnd {
        call_id: String,
        sender_path: String,
        receiver_path: String,
        status: AgentStatus,
        prompt: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        error: Option<String>,
    },
    WaitingBegin {
        call_id: String,
        sender_path: String,
    },
    WaitingEnd {
        call_id: String,
        sender_path: String,
        timed_out: bool,
    },
    CloseBegin {
        call_id: String,
        sender_path: String,
        receiver_path: String,
    },
    CloseEnd {
        call_id: String,
        sender_path: String,
        receiver_path: String,
        status: AgentStatus,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        error: Option<String>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StudioSessionRuntime {
    pub session_id: String,
    pub usage: StudioRuntimeUsage,
    #[serde(default)]
    pub active_skills: Vec<String>,
    #[serde(default)]
    pub active_mcp_servers: Vec<String>,
    #[serde(default)]
    pub active_lsp_servers: Vec<String>,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StudioRuntimeUsage {
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StudioSessionHandoff {
    pub origin_session_id: String,
    pub target_session_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_session: Option<StudioSessionSummary>,
    pub kind: String,
    pub status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plan_id: Option<String>,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct StudioSessionSummary {
    pub id: String,
    pub project_id: String,
    pub title: String,
    pub mode: String,
    pub updated_at: i64,
    pub visibility: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_session_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StudioMcpHealth {
    pub mcp_servers: Vec<StudioMcpServer>,
    pub active_mcp_servers: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StudioLspHealth {
    pub lsp_servers: Vec<StudioLspServer>,
    pub active_lsp_servers: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct StudioKeyValue {
    pub key: String,
    pub value: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StudioMcpServer {
    pub id: String,
    pub enabled: bool,
    pub transport: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env: Vec<StudioKeyValue>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bearer_token_env_var: Option<String>,
    #[serde(default)]
    pub headers: Vec<StudioKeyValue>,
    pub endpoint: String,
    pub source_kind: String,
    pub source_label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_detail: Option<String>,
    pub status_kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status_message: Option<String>,
    pub mutation_policy: String,
    pub availability_kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub availability_message: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_checked_at: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_count: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StudioLspServer {
    pub id: String,
    pub display_name: String,
    #[serde(default)]
    pub extensions: Vec<String>,
    #[serde(default)]
    pub language_ids: Vec<String>,
    pub availability_kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub availability_message: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_checked_at: Option<i64>,
    pub diagnostic_count: u64,
    pub activity_kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub activity_title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub activity_message: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub activity_percentage: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_error_at: Option<i64>,
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
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn studio_turn_status_is_camel_case() {
        assert_eq!(
            serde_json::to_value(StudioTurnStatus::WaitingForModel).unwrap(),
            serde_json::json!("waitingForModel")
        );
    }

    #[test]
    fn studio_part_delta_field_allows_dotted_tool_paths() {
        assert_eq!(
            serde_json::to_value(StudioPartDeltaField::ToolArguments).unwrap(),
            serde_json::json!("tool.arguments")
        );
    }

    #[test]
    fn studio_event_kind_fields_are_camel_case() {
        assert_eq!(
            serde_json::to_value(StudioEventKind::Stale { lagged_events: 2 }).unwrap(),
            serde_json::json!({
                "type": "stale",
                "laggedEvents": 2
            })
        );
        assert_eq!(
            serde_json::to_value(StudioEventKind::SessionListChanged {
                project_id: "project-1".to_string(),
                sessions: Vec::new()
            })
            .unwrap(),
            serde_json::json!({
                "type": "sessionListChanged",
                "projectId": "project-1",
                "sessions": []
            })
        );
    }
}

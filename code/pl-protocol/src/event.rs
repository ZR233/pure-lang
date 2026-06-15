use std::collections::HashMap;

use serde::Deserialize;
use serde::Serialize;
use tokio::sync::broadcast;

use crate::InteractionChangedEvent;

pub type AgentEventSender = broadcast::Sender<AgentEvent>;
pub type AgentEventReceiver = broadcast::Receiver<AgentEvent>;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct UserQuestionOption {
    pub label: String,
    pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct UserQuestion {
    pub id: String,
    pub header: String,
    pub question: String,
    #[serde(default)]
    pub is_other: bool,
    #[serde(default)]
    pub is_secret: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub options: Option<Vec<UserQuestionOption>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct UserInputRequest {
    pub request_id: String,
    pub tool_id: String,
    pub questions: Vec<UserQuestion>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct UserInputAnswer {
    pub answers: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct UserInputResponse {
    pub answers: HashMap<String, UserInputAnswer>,
}

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
    CollabAgentSpawnBegin {
        call_id: String,
        started_at: i64,
        sender_path: String,
        task_name: String,
        prompt: String,
        role: String,
        model: Option<String>,
        reasoning_effort: Option<String>,
    },
    CollabAgentSpawnEnd {
        call_id: String,
        completed_at: i64,
        sender_path: String,
        agent_id: Option<String>,
        path: Option<String>,
        role: Option<String>,
        status: AgentStatus,
        prompt: String,
        error: Option<String>,
    },
    CollabAgentInteractionBegin {
        call_id: String,
        started_at: i64,
        sender_path: String,
        receiver_path: String,
        prompt: String,
    },
    CollabAgentInteractionEnd {
        call_id: String,
        completed_at: i64,
        sender_path: String,
        receiver_path: String,
        status: AgentStatus,
        prompt: String,
        error: Option<String>,
    },
    CollabWaitingBegin {
        call_id: String,
        started_at: i64,
        sender_path: String,
    },
    CollabWaitingEnd {
        call_id: String,
        completed_at: i64,
        sender_path: String,
        timed_out: bool,
    },
    CollabCloseBegin {
        call_id: String,
        started_at: i64,
        sender_path: String,
        receiver_path: String,
    },
    CollabCloseEnd {
        call_id: String,
        completed_at: i64,
        sender_path: String,
        receiver_path: String,
        status: AgentStatus,
        error: Option<String>,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub working_directory: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub denial_reason: Option<String>,
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
    pub kind: TracePartKind,
    pub status: TracePartStatus,
    pub created_at: i64,
    pub updated_at: i64,
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
            kind: TracePartKind::Text,
            status,
            created_at: timestamp,
            updated_at: timestamp,
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

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum PlanLifecycleState {
    PendingConfirmation,
    Accepted,
    Implementing,
    Implemented,
    ImplementationFailed,
    ContinuedPlanning,
    Dismissed,
    Cancelled,
}

impl PlanLifecycleState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::PendingConfirmation => "pendingConfirmation",
            Self::Accepted => "accepted",
            Self::Implementing => "implementing",
            Self::Implemented => "implemented",
            Self::ImplementationFailed => "implementationFailed",
            Self::ContinuedPlanning => "continuedPlanning",
            Self::Dismissed => "dismissed",
            Self::Cancelled => "cancelled",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PlanLifecycleEvent {
    pub plan_id: String,
    pub state: PlanLifecycleState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub turn_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TracePartDeltaEvent {
    pub turn_id: String,
    pub item_id: String,
    #[serde(alias = "sequence")]
    pub started_sequence: u64,
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

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ErrorSeverity {
    Transient,
    Recoverable,
    Fatal,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum AgentStatus {
    Queued,
    Running,
    Waiting,
    Completed,
    Errored,
    Interrupted,
    Shutdown,
    NotFound,
}

impl AgentStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Running => "running",
            Self::Waiting => "waiting",
            Self::Completed => "completed",
            Self::Errored => "errored",
            Self::Interrupted => "interrupted",
            Self::Shutdown => "shutdown",
            Self::NotFound => "notFound",
        }
    }

    pub fn is_final(self) -> bool {
        matches!(
            self,
            Self::Completed | Self::Errored | Self::Shutdown | Self::NotFound
        )
    }
}

/// Kind of runtime budget that stopped a turn or agent.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum BudgetLimitKind {
    ModelStep,
    ToolCall,
    Wait,
    WallClock,
    AgentCount,
    AgentDepth,
    Finalization,
}

impl BudgetLimitKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ModelStep => "modelStep",
            Self::ToolCall => "toolCall",
            Self::Wait => "wait",
            Self::WallClock => "wallClock",
            Self::AgentCount => "agentCount",
            Self::AgentDepth => "agentDepth",
            Self::Finalization => "finalization",
        }
    }
}

/// Snapshot of consumed turn budgets.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BudgetUsage {
    pub model_steps: u32,
    pub tool_calls: u32,
    pub wait_calls: u32,
    pub elapsed_ms: u64,
}

/// Estimated runtime cost in a single currency.
///
/// Costs are local estimates derived from configured per-million-token prices.
/// Different currencies must remain separate and are never converted or summed
/// together.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeCostAmount {
    pub currency: String,
    pub amount: f64,
}

/// Cumulative runtime usage snapshot.
///
/// Used by Studio DTOs to expose the current usage total for either a session
/// or a single agent. `estimated_costs` is grouped by currency.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeUsageSnapshot {
    pub model: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_window: Option<u64>,
    pub latest_context_tokens: u64,
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub cached_prompt_tokens: u64,
    pub total_tokens: u64,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub estimated_costs: Vec<RuntimeCostAmount>,
    #[serde(default)]
    pub has_unpriced_usage: bool,
    pub updated_at: i64,
}

/// Per-inference runtime usage attributed to a root or child agent.
///
/// `inference_id` is stable for a model call and is used by Studio persistence
/// as an idempotency key.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AgentRuntimeDelta {
    pub inference_id: String,
    pub agent_id: String,
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_path: Option<String>,
    pub role: String,
    pub model: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_window: Option<u64>,
    pub usage: TokenUsageSnapshot,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub estimated_costs: Vec<RuntimeCostAmount>,
    #[serde(default)]
    pub has_unpriced_usage: bool,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum OutputStream {
    Stdout,
    Stderr,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PipelineStage {
    IntentAnalysis,
    Planning,
    CodeGeneration,
    Verification,
    Integration,
}

/// Token usage snapshot for trace events.
///
/// Lightweight copy of `pl_model::TokenUsage` to avoid coupling `pl-protocol` to `pl-model`.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TokenUsageSnapshot {
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub cached_prompt_tokens: u64,
    pub total_tokens: u64,
}

/// Append-only timeline event for structured session lifecycle tracking.
///
/// Each event belongs to a session and carries a monotonic sequence number
/// for causal ordering. The `kind` field discriminates the event type.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TraceEvent {
    pub session_id: String,
    pub sequence: u64,
    pub timestamp: i64,
    pub kind: TraceEventKind,
}

/// Snapshot of tool names enabled for a single turn.
///
/// This is stored in the SQLite timeline for diagnostics and is not shown as a
/// user-facing timeline item.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct EnabledToolsEvent {
    pub turn_id: String,
    pub mode: String,
    pub tools: Vec<String>,
}

/// Successful skill activation fact for a session.
///
/// Emitted when `skill_view` successfully reads a skill document or support
/// file and that content has entered the model-visible context.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SkillActivation {
    pub name: String,
    pub source: String,
    pub path: String,
    pub turn_id: String,
    pub tool_call_id: String,
    pub activated_at: i64,
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
mod tests {
    use super::*;
    use crate::{
        InteractionKind, InteractionPayload, InteractionRequest, InteractionScope,
        InteractionStatus,
    };
    use pretty_assertions::assert_eq;

    #[test]
    fn serializes_enabled_tools_trace_event_as_camel_case() {
        let event = TraceEventKind::EnabledToolsRecorded {
            event: EnabledToolsEvent {
                turn_id: "turn-1".to_string(),
                mode: "auto".to_string(),
                tools: vec!["bash".to_string(), "lsp_query".to_string()],
            },
        };

        let json = serde_json::to_value(event).unwrap();

        assert_eq!(
            json,
            serde_json::json!({
                "type": "enabledToolsRecorded",
                "event": {
                    "turnId": "turn-1",
                    "mode": "auto",
                    "tools": ["bash", "lsp_query"]
                }
            })
        );
    }

    #[test]
    fn serializes_interaction_changed_as_camel_case() {
        let event = AgentEvent::InteractionChanged {
            event: InteractionChangedEvent {
                interaction: InteractionRequest {
                    interaction_id: "call-1".to_string(),
                    kind: InteractionKind::ToolApproval,
                    status: InteractionStatus::Pending,
                    scope: InteractionScope {
                        session_id: "session-1".to_string(),
                        turn_id: "turn-1".to_string(),
                        item_id: Some("call-1".to_string()),
                        tool_id: Some("call-1".to_string()),
                        agent_path: None,
                    },
                    payload: InteractionPayload::ToolApproval {
                        name: "bash".to_string(),
                        arguments: serde_json::json!({"command": "echo hi"}),
                        working_directory: Some("C:/project".to_string()),
                        parent_agent_id: None,
                    },
                    created_at: 1_779_688_800,
                    updated_at: 1_779_688_800,
                    resolved_at: None,
                    resolution: None,
                },
            },
        };

        let json = serde_json::to_value(event).unwrap();

        assert_eq!(
            json,
            serde_json::json!({
                "interactionChanged": {
                    "event": {
                        "interaction": {
                            "interactionId": "call-1",
                            "kind": "toolApproval",
                            "status": "pending",
                            "scope": {
                                "sessionId": "session-1",
                                "turnId": "turn-1",
                                "itemId": "call-1",
                                "toolId": "call-1"
                            },
                            "payload": {
                                "type": "toolApproval",
                                "name": "bash",
                                "arguments": {"command": "echo hi"},
                                "workingDirectory": "C:/project"
                            },
                            "createdAt": 1779688800,
                            "updatedAt": 1779688800
                        }
                    }
                }
            })
        );
    }

    #[test]
    fn user_input_defaults_and_response_shape_match_codex_style() {
        let question: UserQuestion = serde_json::from_value(serde_json::json!({
            "id": "notes",
            "header": "Notes",
            "question": "Anything else?"
        }))
        .unwrap();
        let response = UserInputResponse {
            answers: HashMap::from([(
                "notes".to_string(),
                UserInputAnswer {
                    answers: vec!["Ship it".to_string()],
                },
            )]),
        };

        assert_eq!(
            question,
            UserQuestion {
                id: "notes".to_string(),
                header: "Notes".to_string(),
                question: "Anything else?".to_string(),
                is_other: false,
                is_secret: false,
                options: None,
            }
        );
        assert_eq!(
            serde_json::to_value(response).unwrap(),
            serde_json::json!({
                "answers": {
                    "notes": {
                        "answers": ["Ship it"]
                    }
                }
            })
        );
    }

    fn trace_text_part() -> TracePart {
        TracePart::text(
            "turn-1",
            "item-1",
            0,
            TraceTextChannel::Final,
            "hello",
            TracePartStatus::Completed,
            1_779_688_800,
        )
    }

    #[test]
    fn serializes_trace_part_started_as_camel_case() {
        let event = TraceEvent {
            session_id: "sess-1".to_string(),
            sequence: 0,
            timestamp: 1_779_688_800,
            kind: TraceEventKind::TracePartStarted {
                item: trace_text_part(),
            },
        };

        let json = serde_json::to_value(event).unwrap();

        assert_eq!(
            json,
            serde_json::json!({
                "sessionId": "sess-1",
                "sequence": 0,
                "timestamp": 1779688800,
                "kind": {
                    "type": "tracePartStarted",
                    "item": {
                        "turnId": "turn-1",
                        "itemId": "item-1",
                        "startedSequence": 0,
                        "kind": "text",
                        "status": "completed",
                        "createdAt": 1779688800,
                        "updatedAt": 1779688800,
                        "textChannel": "final",
                        "content": "hello"
                    }
                }
            })
        );
    }

    #[test]
    fn serializes_turn_interrupted_as_camel_case() {
        let event = AgentEvent::TurnInterrupted {
            reason: "stopped by user".to_string(),
        };

        let json = serde_json::to_value(event).unwrap();

        assert_eq!(
            json,
            serde_json::json!({
                "turnInterrupted": {
                    "reason": "stopped by user"
                }
            })
        );
    }

    #[test]
    fn serializes_turn_budget_limited_as_camel_case() {
        let event = AgentEvent::TurnBudgetLimited {
            reason: "budget limited".to_string(),
            limit_kind: BudgetLimitKind::ToolCall,
            usage: BudgetUsage {
                model_steps: 3,
                tool_calls: 121,
                wait_calls: 2,
                elapsed_ms: 42,
            },
        };

        let json = serde_json::to_value(event).unwrap();

        assert_eq!(
            json,
            serde_json::json!({
                "turnBudgetLimited": {
                    "reason": "budget limited",
                    "limitKind": "toolCall",
                    "usage": {
                        "modelSteps": 3,
                        "toolCalls": 121,
                        "waitCalls": 2,
                        "elapsedMs": 42
                    }
                }
            })
        );
    }

    #[test]
    fn serializes_agent_runtime_updated_as_camel_case() {
        let event = AgentEvent::AgentRuntimeUpdated {
            delta: AgentRuntimeDelta {
                inference_id: "inf-1".to_string(),
                agent_id: "agent-1".to_string(),
                path: "/root/research".to_string(),
                parent_path: Some("/root".to_string()),
                role: "explorer".to_string(),
                model: "deepseek-v4-flash".to_string(),
                context_window: Some(1_000_000),
                usage: TokenUsageSnapshot {
                    prompt_tokens: 100,
                    completion_tokens: 20,
                    cached_prompt_tokens: 40,
                    total_tokens: 120,
                },
                estimated_costs: vec![RuntimeCostAmount {
                    currency: "CNY".to_string(),
                    amount: 0.001,
                }],
                has_unpriced_usage: false,
                updated_at: 1_779_688_800,
            },
        };

        let json = serde_json::to_value(event).unwrap();

        assert_eq!(
            json,
            serde_json::json!({
                "agentRuntimeUpdated": {
                    "delta": {
                        "inferenceId": "inf-1",
                        "agentId": "agent-1",
                        "path": "/root/research",
                        "parentPath": "/root",
                        "role": "explorer",
                        "model": "deepseek-v4-flash",
                        "contextWindow": 1000000,
                        "usage": {
                            "promptTokens": 100,
                            "completionTokens": 20,
                            "cachedPromptTokens": 40,
                            "totalTokens": 120
                        },
                        "estimatedCosts": [
                            {
                                "currency": "CNY",
                                "amount": 0.001
                            }
                        ],
                        "hasUnpricedUsage": false,
                        "updatedAt": 1779688800
                    }
                }
            })
        );
    }

    #[test]
    fn serializes_skill_activation_events_as_camel_case() {
        let activation = SkillActivation {
            name: "rust-flow".to_string(),
            source: "project".to_string(),
            path: "skills/rust-flow".to_string(),
            turn_id: "turn-1".to_string(),
            tool_call_id: "turn-1-call-1".to_string(),
            activated_at: 1_779_688_800,
        };

        assert_eq!(
            serde_json::to_value(AgentEvent::SkillActivated {
                activation: activation.clone()
            })
            .unwrap(),
            serde_json::json!({
                "skillActivated": {
                    "activation": {
                        "name": "rust-flow",
                        "source": "project",
                        "path": "skills/rust-flow",
                        "turnId": "turn-1",
                        "toolCallId": "turn-1-call-1",
                        "activatedAt": 1779688800
                    }
                }
            })
        );
        assert_eq!(
            serde_json::to_value(TraceEventKind::SkillActivated { activation }).unwrap(),
            serde_json::json!({
                "type": "skillActivated",
                "activation": {
                    "name": "rust-flow",
                    "source": "project",
                    "path": "skills/rust-flow",
                    "turnId": "turn-1",
                    "toolCallId": "turn-1-call-1",
                    "activatedAt": 1779688800
                }
            })
        );
    }

    #[test]
    fn serializes_collab_agent_spawn_events_as_camel_case() {
        let begin = serde_json::to_value(AgentEvent::CollabAgentSpawnBegin {
            call_id: "call-1".to_string(),
            started_at: 1_779_688_800,
            sender_path: "/root".to_string(),
            task_name: "scan_crate".to_string(),
            prompt: "scan crate".to_string(),
            role: "executor".to_string(),
            model: Some("deepseek-v4-flash".to_string()),
            reasoning_effort: Some("high".to_string()),
        })
        .unwrap();
        let end = serde_json::to_value(AgentEvent::CollabAgentSpawnEnd {
            call_id: "call-1".to_string(),
            completed_at: 1_779_688_801,
            sender_path: "/root".to_string(),
            agent_id: Some("agent-1".to_string()),
            path: Some("/root/scan_crate".to_string()),
            role: Some("executor".to_string()),
            status: AgentStatus::Queued,
            prompt: "scan crate".to_string(),
            error: None,
        })
        .unwrap();

        assert_eq!(
            begin,
            serde_json::json!({
                "collabAgentSpawnBegin": {
                    "callId": "call-1",
                    "startedAt": 1779688800,
                    "senderPath": "/root",
                    "taskName": "scan_crate",
                    "prompt": "scan crate",
                    "role": "executor",
                    "model": "deepseek-v4-flash",
                    "reasoningEffort": "high"
                }
            })
        );
        assert_eq!(
            end,
            serde_json::json!({
                "collabAgentSpawnEnd": {
                    "callId": "call-1",
                    "completedAt": 1779688801,
                    "senderPath": "/root",
                    "agentId": "agent-1",
                    "path": "/root/scan_crate",
                    "role": "executor",
                    "status": "queued",
                    "prompt": "scan crate",
                    "error": null
                }
            })
        );
    }

    #[test]
    fn serializes_trace_delta_as_camel_case() {
        let event = AgentEvent::TracePartDelta {
            event: TracePartDeltaEvent {
                turn_id: "turn-1".to_string(),
                item_id: "item-1".to_string(),
                started_sequence: 2,
                kind: TracePartKind::Thinking,
                status: TracePartStatus::Streaming,
                created_at: 1_779_688_800,
                updated_at: 1_779_688_801,
                delta: TraceDelta::Thinking {
                    chunk_index: 1,
                    delta: "思考".to_string(),
                },
            },
        };

        let json = serde_json::to_value(event).unwrap();

        assert_eq!(
            json,
            serde_json::json!({
                "tracePartDelta": {
                    "event": {
                        "turnId": "turn-1",
                        "itemId": "item-1",
                        "startedSequence": 2,
                        "kind": "thinking",
                        "status": "streaming",
                        "createdAt": 1779688800,
                        "updatedAt": 1779688801,
                        "delta": {
                            "type": "thinking",
                            "chunkIndex": 1,
                            "delta": "思考"
                        }
                    }
                }
            })
        );
    }

    #[test]
    fn serializes_plan_timeline_item_and_delta_as_camel_case() {
        let item = TracePart {
            turn_id: "turn-1".to_string(),
            item_id: "turn-1-plan".to_string(),
            started_sequence: 1,
            kind: TracePartKind::Plan,
            status: TracePartStatus::Streaming,
            created_at: 1_779_688_800,
            updated_at: 1_779_688_800,
            text_channel: None,
            content: "# Plan".to_string(),
            attachments: Vec::new(),
            thinking_chunks: Vec::new(),
            tool: None,
            agent: None,
            inference: None,
            usage: None,
        };
        let delta = TracePartDeltaEvent {
            turn_id: "turn-1".to_string(),
            item_id: "turn-1-plan".to_string(),
            started_sequence: 2,
            kind: TracePartKind::Plan,
            status: TracePartStatus::Streaming,
            created_at: 1_779_688_800,
            updated_at: 1_779_688_801,
            delta: TraceDelta::Plan {
                delta: "\n- step".to_string(),
            },
        };

        assert_eq!(
            serde_json::to_value(item).unwrap(),
            serde_json::json!({
                "turnId": "turn-1",
                "itemId": "turn-1-plan",
                "startedSequence": 1,
                "kind": "plan",
                "status": "streaming",
                "createdAt": 1779688800,
                "updatedAt": 1779688800,
                "content": "# Plan"
            })
        );
        assert_eq!(
            serde_json::to_value(delta).unwrap(),
            serde_json::json!({
                "turnId": "turn-1",
                "itemId": "turn-1-plan",
                "startedSequence": 2,
                "kind": "plan",
                "status": "streaming",
                "createdAt": 1779688800,
                "updatedAt": 1779688801,
                "delta": {
                    "type": "plan",
                    "delta": "\n- step"
                }
            })
        );
    }

    #[test]
    fn deserializes_legacy_timeline_sequence_as_started_sequence() {
        let item = serde_json::from_value::<TracePart>(serde_json::json!({
            "turnId": "turn-1",
            "itemId": "turn-1-plan",
            "sequence": 7,
            "kind": "plan",
            "status": "streaming",
            "createdAt": 1779688800,
            "updatedAt": 1779688800,
            "content": "# Plan"
        }))
        .unwrap();
        let delta = serde_json::from_value::<TracePartDeltaEvent>(serde_json::json!({
            "turnId": "turn-1",
            "itemId": "turn-1-plan",
            "sequence": 7,
            "kind": "plan",
            "status": "streaming",
            "createdAt": 1779688800,
            "updatedAt": 1779688801,
            "delta": {
                "type": "plan",
                "delta": "\n- step"
            }
        }))
        .unwrap();

        assert_eq!(item.started_sequence, 7);
        assert_eq!(delta.started_sequence, 7);
        assert_eq!(
            serde_json::to_value(item).unwrap()["startedSequence"],
            serde_json::json!(7)
        );
        assert_eq!(
            serde_json::to_value(delta).unwrap()["startedSequence"],
            serde_json::json!(7)
        );
    }

    #[test]
    fn serializes_plan_lifecycle_trace_event_as_camel_case() {
        let event = TraceEvent {
            session_id: "sess-1".to_string(),
            sequence: 3,
            timestamp: 1_779_688_802,
            kind: TraceEventKind::PlanLifecycleChanged {
                event: PlanLifecycleEvent {
                    plan_id: "turn-1-plan".to_string(),
                    state: PlanLifecycleState::ImplementationFailed,
                    turn_id: Some("turn-2".to_string()),
                    reason: Some("provider error".to_string()),
                    updated_at: 1_779_688_802,
                },
            },
        };

        assert_eq!(
            serde_json::to_value(event).unwrap(),
            serde_json::json!({
                "sessionId": "sess-1",
                "sequence": 3,
                "timestamp": 1779688802,
                "kind": {
                    "type": "planLifecycleChanged",
                    "event": {
                        "planId": "turn-1-plan",
                        "state": "implementationFailed",
                        "turnId": "turn-2",
                        "reason": "provider error",
                        "updatedAt": 1779688802
                    }
                }
            })
        );
    }
}

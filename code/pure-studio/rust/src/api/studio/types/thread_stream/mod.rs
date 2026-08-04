//! Typed FRB mirror of the canonical Thread → Turn → Item stream protocol.
//!
//! Open JSON leaves are converted to explicitly named `*_json` strings at the bridge boundary.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq)]
pub enum BridgeThreadSubscriptionUpdate {
    Snapshot {
        snapshot: Box<BridgeThreadSnapshot>,
    },
    Notification {
        notification: Box<BridgeThreadNotificationEnvelope>,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct BridgeThreadNotificationEnvelope {
    pub thread_id: String,
    pub revision: u64,
    pub emitted_at: i64,
    pub notification: BridgeThreadNotification,
}

#[derive(Debug, Clone, PartialEq)]
pub enum BridgeThreadNotification {
    TurnStarted {
        turn: BridgeTurn,
    },
    TurnUpdated {
        turn: BridgeTurn,
    },
    TurnCompleted {
        turn: BridgeTurn,
    },
    ItemStarted {
        item: BridgeThreadItem,
    },
    ItemDelta {
        delta: BridgeThreadItemDelta,
    },
    ItemCompleted {
        item: BridgeThreadItem,
    },
    InteractionChanged {
        interaction: BridgeInteractionRequest,
    },
    ThreadRuntimeUpdated {
        runtime: BridgeThreadRuntimeSnapshot,
    },
    Lagged {
        dropped: u64,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct BridgeThreadSnapshot {
    pub schema_version: u32,
    pub revision: u64,
    pub thread: BridgeThread,
    pub active_turn: Option<BridgeTurn>,
    pub items: Vec<BridgeThreadItem>,
    pub interactions: Vec<BridgeInteractionRequest>,
    pub runtime: Option<BridgeThreadRuntimeSnapshot>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BridgeThread {
    pub id: String,
    pub project_id: String,
    pub title: String,
    pub mode: BridgeThreadMode,
    pub root_thread_id: String,
    pub parent_thread_id: Option<String>,
    pub role: String,
    pub agent_path: String,
    pub status: BridgeThreadStatus,
    pub created_at: i64,
    pub updated_at: i64,
    pub archived: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum BridgeThreadMode {
    Simple,
    Task,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum BridgeThreadStatus {
    Idle,
    Running,
    Waiting,
    Completed,
    Failed,
    Closed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BridgeTurn {
    pub id: String,
    pub thread_id: String,
    pub state: BridgeTurnState,
    pub started_at: Option<i64>,
    pub updated_at: i64,
    pub completed_at: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BridgeTurnState {
    Queued,
    InProgress { phase: BridgeTurnPhase },
    Completed,
    Failed { reason: String },
    Interrupted { reason: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BridgeTurnPhase {
    Preparing,
    Thinking,
    Responding,
    Planning,
    RunningTool,
    WaitingInteraction,
    Persisting,
}

#[derive(Debug, Clone, PartialEq)]
pub struct BridgeThreadItem {
    pub id: String,
    pub thread_id: String,
    pub turn_id: String,
    pub ordinal: u64,
    pub revision: u64,
    pub status: BridgeThreadItemStatus,
    pub created_at: i64,
    pub updated_at: i64,
    pub completed_at: Option<i64>,
    pub error: Option<String>,
    pub content: BridgeThreadItemContent,
    pub usage: Option<BridgeTokenUsageSnapshot>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum BridgeThreadItemContent {
    UserMessage {
        text: String,
        attachments: Vec<BridgeThreadAttachment>,
    },
    AgentMessage {
        channel: BridgeAgentMessageChannel,
        text: String,
    },
    Reasoning {
        summary: Vec<String>,
        content: Vec<String>,
    },
    Plan {
        content: String,
    },
    ToolCall {
        tool: BridgeThreadToolCall,
    },
    File {
        path: String,
        media_type: Option<String>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BridgeAgentMessageChannel {
    Commentary,
    Final,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BridgeThreadItemStatus {
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BridgeThreadAttachment {
    pub id: String,
    pub media_type: String,
    pub filename: Option<String>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub byte_size: u64,
    pub data_url: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct BridgeThreadToolCall {
    pub tool_call_id: String,
    pub call_id: Option<String>,
    pub provider_item_id: Option<String>,
    pub name: String,
    pub arguments: String,
    pub result: Option<String>,
    pub output_artifacts_json: Vec<String>,
    pub exit_code: Option<i32>,
    pub timed_out: bool,
    pub working_directory: Option<String>,
    pub denial_reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BridgeThreadItemDelta {
    pub item_id: String,
    pub revision: u64,
    pub field: BridgeThreadItemDeltaField,
    pub delta: String,
    pub chunk_index: Option<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BridgeThreadItemDeltaField {
    Text,
    ReasoningSummary,
    ReasoningContent,
    PlanContent,
    ToolArguments,
    ToolResult,
}

#[derive(Debug, Clone, PartialEq)]
pub struct BridgeThreadRuntimeSnapshot {
    pub thread_id: String,
    pub usage: BridgeThreadRuntimeUsage,
    pub todo: Option<BridgeTodoListSnapshot>,
    pub active_skills: Vec<String>,
    pub active_mcp_servers: Vec<String>,
    pub active_lsp_servers: Vec<String>,
    pub progress: Option<String>,
    pub mcp_health: Option<BridgeThreadMcpHealthSnapshot>,
    pub updated_at: i64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct BridgeThreadRuntimeUsage {
    pub model: String,
    pub context_window: Option<u64>,
    pub latest_context_tokens: u64,
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub cached_prompt_tokens: u64,
    pub total_tokens: u64,
    pub cache_hit_rate: Option<f64>,
    pub estimated_costs: Vec<BridgeRuntimeCostAmount>,
    pub has_unpriced_usage: bool,
    pub updated_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BridgeInteractionRequest {
    pub interaction_id: String,
    pub kind: BridgeInteractionKind,
    pub status: BridgeInteractionStatus,
    pub scope: BridgeInteractionScope,
    pub payload: BridgeInteractionPayload,
    pub created_at: i64,
    pub updated_at: i64,
    pub resolved_at: Option<i64>,
    pub resolution: Option<BridgeInteractionResolution>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BridgeInteractionKind {
    UserInput,
    ToolApproval,
    PlanConfirmation,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BridgeInteractionStatus {
    Pending,
    Resolved,
    Cancelled,
    Expired,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BridgeInteractionScope {
    pub thread_id: String,
    pub turn_id: String,
    pub item_id: Option<String>,
    pub tool_id: Option<String>,
    pub agent_path: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BridgeInteractionPayload {
    UserInput {
        questions: Vec<BridgeUserQuestion>,
    },
    ToolApproval {
        name: String,
        arguments_json: String,
        working_directory: Option<String>,
        parent_agent_id: Option<String>,
    },
    PlanConfirmation {
        plan_id: String,
        content: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BridgeUserQuestion {
    pub id: String,
    pub header: String,
    pub question: String,
    pub is_other: bool,
    pub is_secret: bool,
    pub options: Option<Vec<BridgeUserQuestionOption>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BridgeUserQuestionOption {
    pub label: String,
    pub description: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BridgeInteractionResolution {
    UserInput {
        answers: Vec<BridgeUserInputAnswer>,
    },
    ToolApproval {
        decision: BridgeToolApprovalResolution,
        reason: Option<String>,
    },
    PlanConfirmation {
        decision: BridgePlanConfirmationResolution,
        content: Option<String>,
        reason: Option<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BridgeUserInputAnswer {
    pub question_id: String,
    pub answers: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BridgeToolApprovalResolution {
    Approved,
    Denied,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BridgePlanConfirmationResolution {
    ImplementFreshContext,
    ContinuePlanning,
    Dismiss,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BridgeTodoListSnapshot {
    pub call_id: String,
    pub agent_id: Option<String>,
    pub path: Option<String>,
    pub parent_path: Option<String>,
    pub explanation: Option<String>,
    pub items: Vec<BridgeTodoItem>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BridgeTodoItem {
    pub step: String,
    pub status: BridgeTodoStatus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BridgeTodoStatus {
    Pending,
    InProgress,
    Completed,
}

#[derive(Debug, Clone, PartialEq)]
pub struct BridgeRuntimeCostAmount {
    pub currency: String,
    pub amount: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BridgeTokenUsageSnapshot {
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub cached_prompt_tokens: u64,
    pub total_tokens: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BridgeThreadMcpHealthSnapshot {
    pub generation: u64,
    pub servers: Vec<BridgeThreadMcpAvailabilityDescriptor>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BridgeThreadMcpAvailabilityDescriptor {
    pub server: BridgeThreadMcpServerDescriptor,
    pub availability: String,
    pub message: Option<String>,
    pub last_checked_at: Option<i64>,
    pub tool_count: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BridgeThreadMcpServerDescriptor {
    pub id: String,
    pub source: String,
    pub transport: String,
    pub endpoint: String,
    pub built_in: bool,
}

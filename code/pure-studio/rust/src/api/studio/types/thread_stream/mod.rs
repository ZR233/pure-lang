//! Typed FRB mirror of the canonical Thread → Turn → Item stream protocol.
//!
//! Open JSON leaves are converted to explicitly named `*_json` strings at the bridge boundary.

use serde::{Deserialize, Serialize};

pub mod item;

pub use item::*;

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
        turn: Box<BridgeTurn>,
    },
    TurnUpdated {
        turn: Box<BridgeTurn>,
    },
    TurnCompleted {
        turn: Box<BridgeTurn>,
    },
    ItemStarted {
        item: Box<BridgeThreadItem>,
    },
    ItemDelta {
        delta: Box<BridgeThreadItemDelta>,
    },
    ItemCompleted {
        item: Box<BridgeThreadItem>,
    },
    InteractionChanged {
        interaction: Box<BridgeInteractionRequest>,
    },
    ThreadRuntimeUpdated {
        runtime: Box<BridgeThreadRuntimeSnapshot>,
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
    /// 更旧历史的回源锚点（窗口首 Turn 的 id，before 语义）；None 表示无更旧内容。
    pub history_cursor: Option<String>,
    pub interactions: Vec<BridgeInteractionRequest>,
    pub runtime: Option<BridgeThreadRuntimeSnapshot>,
    pub runtime_availability: BridgeThreadRuntimeAvailability,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BridgeThreadRuntimeAvailability {
    Active,
    Inactive,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BridgeThread {
    pub id: String,
    pub project_id: String,
    pub title: String,
    /// 完整 Mode Skill ID，例如 `mode.simple` 或 `mode.release`。
    pub mode: String,
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
pub enum BridgeThreadStatus {
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BridgeTurn {
    pub id: String,
    pub thread_id: String,
    pub revision: u64,
    pub state: BridgeTurnState,
    pub updated_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BridgeTurnFailureDto {
    pub category: String,
    pub provider_kind: Option<String>,
    pub code: Option<String>,
    pub http_status: Option<u16>,
    pub message: String,
    pub retryable: bool,
    pub retry_after_ms: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BridgeTurnState {
    Queued {
        queued_at: i64,
    },
    Running {
        started_at: i64,
        phase: BridgeTurnPhase,
    },
    Completed {
        started_at: Option<i64>,
        completed_at: i64,
        completion: BridgeTurnCompletion,
    },
    Cancelled {
        started_at: Option<i64>,
        requested_at: i64,
        completed_at: i64,
        cause: BridgeTurnCancellationCause,
    },
    Failed {
        started_at: Option<i64>,
        completed_at: i64,
        failure: BridgeTurnFailureDto,
    },
    BudgetLimited {
        started_at: Option<i64>,
        completed_at: i64,
        limit: BridgeTurnBudgetLimit,
        rollover: BridgeTurnRolloverOutcome,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BridgeTurnPhase {
    Preparing,
    Thinking,
    Responding,
    Planning,
    RunningTool,
    Persisting,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BridgeTurnCompletion {
    Normal,
    InteractionRequested,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BridgeTurnCancellationCause {
    UserRequested,
    RuntimeShutdown,
    AgentClosed,
    Recovery,
    Coalesced { target_turn_id: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BridgeTurnRolloverOutcome {
    NotAttempted,
    Succeeded,
    Failed { error: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BridgeTurnBudgetLimit {
    pub kind: BridgeTurnBudgetLimitKind,
    pub usage: BridgeTurnBudgetUsage,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BridgeTurnBudgetLimitKind {
    ModelStep,
    ToolCall,
    Wait,
    WallClock,
    AgentCount,
    AgentDepth,
    Finalization,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BridgeTurnBudgetUsage {
    pub model_steps: u32,
    pub tool_calls: u32,
    pub wait_calls: u32,
    pub elapsed_ms: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct BridgeThreadRuntimeSnapshot {
    pub thread_id: String,
    pub usage: BridgeThreadRuntimeUsage,
    pub turn_completion_tokens: u64,
    pub turn_decode_millis: u64,
    pub todo: Option<BridgeTodoListSnapshot>,
    pub active_skills: Vec<String>,
    pub active_mcp_servers: Vec<String>,
    pub active_lsp_servers: Vec<String>,
    pub progress: Option<String>,
    pub mcp_health: Option<BridgeThreadMcpHealthSnapshot>,
    pub workflow: Option<BridgeWorkflowRuntimeSnapshot>,
    pub updated_at: i64,
}

/// 通用工作流面板所需的 canonical 投影。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BridgeWorkflowRuntimeSnapshot {
    pub revision: u64,
    pub current_run: Option<BridgeWorkflowRun>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BridgeWorkflowRun {
    pub lineage_id: String,
    pub run_id: String,
    pub definition: BridgeWorkflowDefinition,
    pub definition_hash: String,
    pub mode: BridgeModeInstructionSnapshot,
    pub lifecycle: BridgeWorkflowRunLifecycle,
    pub current_stage_id: String,
    pub compiled_at: i64,
    pub updated_at: i64,
    pub history_tail: Vec<BridgeWorkflowTransitionRecord>,
    pub archived_transition_count: u64,
    pub archived_transition_digest: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BridgeWorkflowDefinition {
    pub title: String,
    pub goal: String,
    pub initial_stage_id: String,
    pub stages: Vec<BridgeWorkflowStage>,
    pub transitions: Vec<BridgeWorkflowTransition>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BridgeWorkflowStage {
    pub id: String,
    pub title: String,
    pub instructions: String,
    pub completion_criteria: Vec<String>,
    pub terminal: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BridgeWorkflowTransition {
    pub from_stage_id: String,
    pub to_stage_id: String,
    pub when: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BridgeModeInstructionSnapshot {
    pub mode_id: String,
    pub display_name: String,
    pub source: String,
    pub provider_id: String,
    pub revision: String,
    pub content_hash: String,
    pub content: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BridgeWorkflowRunLifecycle {
    Active,
    Terminal,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BridgeWorkflowTransitionRecord {
    pub revision: u64,
    pub from_stage_id: String,
    pub to_stage_id: String,
    pub reason: String,
    pub summary: String,
    pub evidence: Vec<String>,
    pub turn_id: String,
    pub call_id: String,
    pub transitioned_at: i64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct BridgeThreadRuntimeUsage {
    pub model: String,
    pub context_window: Option<u64>,
    pub latest_context_tokens: u64,
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub cached_prompt_tokens: u64,
    pub cache_write_tokens: u64,
    pub cache_miss_tokens: u64,
    pub reasoning_tokens: u64,
    pub inference_count: u64,
    pub total_tokens: u64,
    pub cache_hit_rate: Option<f64>,
    pub estimated_costs: Vec<BridgeRuntimeCostAmount>,
    pub estimated_cache_savings: Vec<BridgeRuntimeCostAmount>,
    pub has_unpriced_usage: bool,
    pub prompt_generation: Option<u64>,
    pub prompt_cache_policy: Option<String>,
    pub prefix_changed_reason: Option<BridgePromptPrefixChangedReason>,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BridgePromptPrefixChangedReason {
    Initial,
    PromptScopeChanged,
    ProviderChanged,
    ModelChanged,
    BaseInstructionsChanged,
    GlobalInstructionsChanged,
    ModeRoleChanged,
    SkillCatalogChanged,
    WorkspaceInstructionsChanged,
    RequestPropertiesChanged,
    FixedPrefixChanged,
    ToolSchemaChanged,
    ContextCompacted,
    ContextAppended,
    ContextRecovered,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BridgeInteractionRequest {
    pub interaction_id: String,
    pub scope: BridgeInteractionScope,
    pub revision: u64,
    pub content: BridgeInteractionContent,
    pub created_at: i64,
    pub updated_at: i64,
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
pub enum BridgeInteractionContent {
    UserInput {
        questions: Vec<BridgeUserQuestion>,
        state: BridgeUserInputInteractionState,
    },
    ToolApproval {
        name: String,
        arguments_json: String,
        working_directory: Option<String>,
        parent_agent_id: Option<String>,
        state: BridgeToolApprovalInteractionState,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BridgeUserInputInteractionState {
    Pending {
        operation_id: String,
    },
    Resolved {
        operation_id: String,
        resolved_at: i64,
        answers: Vec<BridgeUserInputAnswer>,
    },
    Cancelled {
        operation_id: String,
        cancelled_at: i64,
        reason: String,
    },
    Expired {
        operation_id: String,
        expired_at: i64,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BridgeToolApprovalInteractionState {
    Pending {
        operation_id: String,
    },
    Resolved {
        operation_id: String,
        resolved_at: i64,
        decision: BridgeToolApprovalResolution,
        reason: Option<String>,
    },
    Cancelled {
        operation_id: String,
        cancelled_at: i64,
        reason: String,
    },
    Expired {
        operation_id: String,
        expired_at: i64,
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
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

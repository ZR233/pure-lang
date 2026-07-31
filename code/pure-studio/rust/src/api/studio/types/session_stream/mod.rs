/// Typed FRB mirror of the canonical session stream protocol.
///
/// Types in this module deliberately contain no `pl-protocol` or
/// `serde_json::Value` fields. Open JSON leaves are serialized by the bridge
/// converter into explicitly named `*_json` strings.

#[derive(Debug, Clone, PartialEq)]
pub enum BridgeSessionStreamFrame {
    Snapshot { snapshot: BridgeSessionViewSnapshot },
    Event { event: BridgeSessionEventEnvelope },
    ResyncRequired { reason: BridgeSessionResyncReason },
}

#[derive(Debug, Clone, PartialEq)]
pub struct BridgeSessionEventEnvelope {
    pub event_id: String,
    pub session_id: String,
    pub source_agent_id: Option<String>,
    pub turn_id: Option<String>,
    pub emitted_at: i64,
    pub position: BridgeSessionEventPosition,
    pub kind: BridgeSessionEventKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BridgeSessionEventPosition {
    Durable { sequence: u64 },
    Transient { revision: u64 },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BridgeSessionResyncReason {
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

#[derive(Debug, Clone, PartialEq)]
pub enum BridgeSessionEventKind {
    TurnChanged {
        turn: BridgeSessionTurn,
    },
    MessageChanged {
        message: BridgeSessionMessage,
    },
    MessageRemoved {
        message_id: String,
    },
    PartChanged {
        part: BridgeSessionPart,
    },
    PartRemoved {
        message_id: String,
        part_id: String,
    },
    PartDelta {
        delta: BridgeSessionPartDelta,
    },
    InteractionChanged {
        interaction: BridgeInteractionRequest,
    },
    AgentChanged {
        agent: BridgeSessionAgentSnapshot,
    },
    TimelineEventAppended {
        event: BridgeSessionTimelineEvent,
    },
    RuntimeChanged {
        runtime: BridgeSessionRuntimeSnapshot,
    },
    SkillActivated {
        activation: BridgeSkillActivation,
    },
    PlanChanged {
        event: BridgePlanLifecycleEvent,
    },
    ContextCompacted {
        compaction: BridgeSessionContextCompaction,
    },
    ErrorOccurred {
        message: String,
        severity: BridgeErrorSeverity,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct BridgeSessionViewSnapshot {
    pub schema_version: u32,
    pub session_id: String,
    pub through_sequence: u64,
    pub owner: Option<BridgeSessionOwnerSnapshot>,
    pub turn: Option<BridgeSessionTurn>,
    pub messages: Vec<BridgeSessionMessage>,
    pub parts: Vec<BridgeSessionPart>,
    pub interactions: Vec<BridgeInteractionRequest>,
    pub agents: Vec<BridgeSessionAgentSnapshot>,
    pub timeline_events: Vec<BridgeSessionTimelineEvent>,
    pub runtime: Option<BridgeSessionRuntimeSnapshot>,
    pub activated_skills: Vec<BridgeSkillActivation>,
    pub plan_events: Vec<BridgePlanLifecycleEvent>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BridgeSessionOwnerSnapshot {
    pub agent_id: String,
    pub role: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BridgeSessionMessage {
    pub message_id: String,
    pub session_id: String,
    pub turn_id: String,
    pub role: BridgeSessionMessageRole,
    pub status: BridgeSessionMessageStatus,
    pub created_at: i64,
    pub updated_at: i64,
    pub completed_at: Option<i64>,
    pub error: Option<String>,
    pub metadata_json: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BridgeSessionMessageRole {
    User,
    Assistant,
    System,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BridgeSessionMessageStatus {
    Queued,
    Streaming,
    Completed,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, PartialEq)]
pub struct BridgeSessionPart {
    pub part_id: String,
    pub message_id: String,
    pub session_id: String,
    pub turn_id: String,
    pub order: u64,
    pub revision: u64,
    pub status: BridgeSessionPartStatus,
    pub created_at: i64,
    pub updated_at: i64,
    pub completed_at: Option<i64>,
    pub error: Option<String>,
    pub content: BridgeSessionPartContent,
    pub usage: Option<BridgeTokenUsageSnapshot>,
    pub synthetic: bool,
    pub ignored: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum BridgeSessionPartContent {
    Text {
        channel: BridgeSessionTextChannel,
        text: String,
        attachments: Vec<BridgeSessionAttachment>,
    },
    Reasoning {
        summary: Vec<String>,
        content: Vec<String>,
    },
    Tool {
        tool: BridgeSessionToolPart,
    },
    Agent {
        agent: BridgeSessionAgentPart,
    },
    Turn,
    Inference {
        inference_id: String,
        model: String,
    },
    Plan {
        content: String,
    },
    File {
        path: String,
        media_type: Option<String>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BridgeSessionPartStatus {
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BridgeSessionTextChannel {
    User,
    Commentary,
    Final,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BridgeSessionAttachment {
    pub id: String,
    pub media_type: String,
    pub filename: Option<String>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub byte_size: u64,
    pub data_url: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BridgeSessionToolPart {
    pub tool_call_id: String,
    pub call_id: Option<String>,
    pub provider_item_id: Option<String>,
    pub name: String,
    pub arguments_json: String,
    pub result: Option<String>,
    pub output_artifacts_json: Vec<String>,
    pub exit_code: Option<i32>,
    pub timed_out: bool,
    pub working_directory: Option<String>,
    pub denial_reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BridgeSessionAgentPart {
    pub id: String,
    pub path: String,
    pub parent_path: Option<String>,
    pub role: String,
    pub task: String,
    pub status: BridgeAgentStatus,
    pub summary: Option<String>,
    pub depth: u32,
    pub error: Option<String>,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BridgeSessionPartDelta {
    pub part_id: String,
    pub revision: u64,
    pub field: BridgeSessionPartDeltaField,
    pub delta: String,
    pub chunk_index: Option<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BridgeSessionPartDeltaField {
    Text,
    ReasoningSummary,
    ReasoningContent,
    PlanContent,
    ToolArguments,
    ToolResult,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BridgeSessionTurn {
    pub turn_id: String,
    pub session_id: String,
    pub state: BridgeSessionTurnState,
    pub updated_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BridgeSessionTurnState {
    Queued,
    InProgress { activity: BridgeSessionTurnActivity },
    Completed,
    Failed { reason: String },
    Cancelled { reason: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BridgeSessionTurnActivity {
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

#[derive(Debug, Clone, PartialEq)]
pub struct BridgeSessionAgentSnapshot {
    pub id: String,
    pub session_id: String,
    pub path: String,
    pub parent_path: Option<String>,
    pub role: String,
    pub task: String,
    pub status: BridgeAgentStatus,
    pub summary: Option<String>,
    pub depth: u32,
    pub error: Option<String>,
    pub reason: Option<String>,
    pub budget_limit_kind: Option<BridgeBudgetLimitKind>,
    pub budget_usage: Option<BridgeBudgetUsage>,
    pub runtime_usage: Option<BridgeSessionRuntimeUsage>,
    pub updated_at: i64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct BridgeSessionTimelineEvent {
    pub event_id: String,
    pub session_id: String,
    pub sequence: u64,
    pub created_at: i64,
    pub kind: BridgeSessionTimelineEventKind,
}

#[derive(Debug, Clone, PartialEq)]
pub enum BridgeSessionTimelineEventKind {
    SubAgentActivity {
        call_id: String,
        agent_id: Option<String>,
        path: Option<String>,
        parent_path: Option<String>,
        kind: BridgeSubAgentActivityKind,
        status: Option<BridgeAgentStatus>,
        message: Option<String>,
        timed_out: Option<bool>,
        error: Option<String>,
    },
    TodoListChanged {
        snapshot: BridgeTodoListSnapshot,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct BridgeSessionRuntimeSnapshot {
    pub session_id: String,
    pub usage: BridgeSessionRuntimeUsage,
    pub active_skills: Vec<String>,
    pub active_mcp_servers: Vec<String>,
    pub active_lsp_servers: Vec<String>,
    pub agent_count: u32,
    pub mcp_health: Option<BridgeSessionMcpHealthSnapshot>,
    pub updated_at: i64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct BridgeSessionRuntimeUsage {
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
pub struct BridgeSessionContextCompaction {
    pub before_tokens: u64,
    pub after_tokens: u64,
    pub compacted_at: i64,
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
    pub session_id: String,
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
pub struct BridgePlanLifecycleEvent {
    pub plan_id: String,
    pub state: BridgePlanLifecycleState,
    pub turn_id: Option<String>,
    pub reason: Option<String>,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BridgePlanLifecycleState {
    PendingConfirmation,
    Accepted,
    Implementing,
    Implemented,
    ImplementationFailed,
    ContinuedPlanning,
    Dismissed,
    Cancelled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BridgeErrorSeverity {
    Transient,
    Recoverable,
    Fatal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BridgeAgentStatus {
    Queued,
    Running,
    Waiting,
    Completed,
    Errored,
    Interrupted,
    Shutdown,
    NotFound,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BridgeSubAgentActivityKind {
    Spawned,
    MessageQueued,
    FollowupStarted,
    WaitCompleted,
    Closed,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BridgeBudgetLimitKind {
    ModelStep,
    ToolCall,
    Wait,
    WallClock,
    AgentCount,
    AgentDepth,
    Finalization,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BridgeBudgetUsage {
    pub model_steps: u32,
    pub tool_calls: u32,
    pub wait_calls: u32,
    pub elapsed_ms: u64,
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
pub struct BridgeSkillActivation {
    pub name: String,
    pub source: String,
    pub path: String,
    pub turn_id: String,
    pub tool_call_id: String,
    pub activated_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BridgeSessionMcpHealthSnapshot {
    pub generation: u64,
    pub servers: Vec<BridgeSessionMcpAvailabilityDescriptor>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BridgeSessionMcpAvailabilityDescriptor {
    pub server: BridgeSessionMcpServerDescriptor,
    pub availability: String,
    pub message: Option<String>,
    pub last_checked_at: Option<i64>,
    pub tool_count: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BridgeSessionMcpServerDescriptor {
    pub id: String,
    pub source: String,
    pub transport: String,
    pub endpoint: String,
    pub built_in: bool,
}

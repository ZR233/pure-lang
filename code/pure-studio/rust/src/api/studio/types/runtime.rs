use serde::{Deserialize, Serialize};
// ── Observed resource state payloads ──

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BridgeUninitializedResource {
    pub revision: u64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BridgeLoadingResource {
    pub revision: u64,
    pub operation: BridgeStateOperation,
    pub operation_id: String,
    pub started_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BridgeReadyResource {
    pub revision: u64,
    pub updated_at: i64,
    pub last_checked_at: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BridgeRefreshingResource {
    pub revision: u64,
    pub operation: BridgeStateOperation,
    pub operation_id: String,
    pub started_at: i64,
    pub last_checked_at: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BridgeStaleResource {
    pub revision: u64,
    pub stale_at: i64,
    pub last_checked_at: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BridgeDegradedResource {
    pub revision: u64,
    pub failed_at: i64,
    pub last_checked_at: Option<i64>,
    pub operation: BridgeStateOperation,
    pub error: BridgeStateError,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BridgeFailedResource {
    pub revision: u64,
    pub failed_at: i64,
    pub operation: BridgeStateOperation,
    pub error: BridgeStateError,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BridgeStoppedResource {
    pub revision: u64,
    pub stopped_at: i64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum BridgeStateOperation {
    Initialize,
    Activate,
    Reload,
    Reconcile,
    Discover,
    Check,
    Probe,
    Repair,
    Reset,
    Shutdown,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BridgeStateError {
    pub code: String,
    pub message: String,
    pub retryable: bool,
}

// ── Runtime types ──

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeSnapshot {
    pub revision: u64,
    pub state: BridgeRuntimeState,
    pub active_turns: Vec<BridgeActiveTurn>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", content = "data", rename_all = "camelCase")]
pub enum BridgeRuntimeState {
    Uninitialized(BridgeRuntimeTimestamp),
    Initializing(BridgeRuntimeTimestamp),
    Ready(BridgeRuntimeTimestamp),
    ShuttingDown(BridgeRuntimeTimestamp),
    Stopped(BridgeRuntimeTimestamp),
    Failed(BridgeFailedRuntimeState),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BridgeRuntimeTimestamp {
    pub at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BridgeFailedRuntimeState {
    pub failed_at: i64,
    pub error: BridgeStateError,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BridgeActiveTurn {
    pub thread_id: String,
    pub turn_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum BridgeRecoveryIssueScope {
    Application,
    Project,
    Thread,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum BridgeRecoveryIssueCategory {
    ProcessLease,
    AgentState,
    Repository,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum BridgeRecoveryIssueAction {
    Retry,
    CleanupThread,
    RemoveProject,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BridgeStudioRecoveryIssueDto {
    pub id: String,
    pub scope: BridgeRecoveryIssueScope,
    pub category: BridgeRecoveryIssueCategory,
    pub available_actions: Vec<BridgeRecoveryIssueAction>,
    pub project_id: Option<String>,
    pub thread_id: Option<String>,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BridgeAgentProgressDto {
    pub stage: String,
    pub summary: String,
    pub next_step: String,
    pub revision: u64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", content = "data", rename_all = "camelCase")]
pub enum BridgeAgentState {
    Idle(BridgeIdleAgent),
    Queued(BridgeQueuedAgent),
    Running(BridgeRunningAgent),
    WaitingTool(BridgeWaitingToolAgent),
    WaitingInteraction(BridgeWaitingInteractionAgent),
    Cancelling(BridgeCancellingAgent),
    Closing(BridgeClosingAgent),
    Closed(BridgeClosedAgent),
    Faulted(BridgeFaultedAgent),
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct BridgeIdleAgent {}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BridgeQueuedAgent {
    pub turn_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BridgeRunningAgent {
    pub turn_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BridgeWaitingToolAgent {
    pub turn_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BridgeWaitingInteractionAgent {
    pub turn_id: String,
    pub interaction_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BridgeCancellingAgent {
    pub turn_id: String,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct BridgeClosingAgent {}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct BridgeClosedAgent {}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BridgeFaultedAgent {
    pub error: BridgeStateError,
    pub diagnostic_turn_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BridgeAgentDirectoryEntryDto {
    pub id: String,
    pub thread_id: String,
    pub root_thread_id: String,
    pub path: String,
    pub parent_path: Option<String>,
    pub role: String,
    pub task: String,
    pub summary: Option<String>,
    pub depth: u32,
    pub state: BridgeAgentState,
    pub progress: Option<BridgeAgentProgressDto>,
    pub updated_at: i64,
    pub summary_age_seconds: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BridgeMcpHealthDto {
    pub active_mcp_servers: Vec<String>,
    pub mcp_servers: Vec<BridgeMcpServerDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BridgeMcpServerDto {
    pub id: String,
    pub transport: String,
    pub endpoint: String,
    pub source_kind: String,
    pub mutation_policy: String,
    pub state: BridgeMcpServerState,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", content = "data", rename_all = "camelCase")]
pub enum BridgeMcpServerState {
    Disabled {
        message: String,
    },
    MissingCredential {
        message: String,
    },
    Checking {
        message: String,
    },
    Available {
        checked_at: i64,
        tool_count: u64,
    },
    Unavailable {
        checked_at: i64,
        error: BridgeStateError,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BridgeLspHealthDto {
    pub active_lsp_servers: Vec<String>,
    pub lsp_servers: Vec<BridgeLspServerDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BridgeLspServerDto {
    pub id: String,
    pub display_name: String,
    pub extensions: Vec<String>,
    pub language_ids: Vec<String>,
    pub state: BridgeLspServerState,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", content = "data", rename_all = "camelCase")]
pub enum BridgeLspServerState {
    Checking {
        message: String,
    },
    Available {
        checked_at: i64,
        diagnostic_count: u64,
        activity: BridgeLspActivity,
    },
    Unavailable {
        checked_at: i64,
        error: BridgeStateError,
    },
    Disabled {
        message: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", content = "data", rename_all = "camelCase")]
pub enum BridgeLspActivity {
    Idle,
    Busy {
        title: Option<String>,
        message: Option<String>,
        percentage: Option<u32>,
    },
    Indexing {
        title: Option<String>,
        message: Option<String>,
        percentage: Option<u32>,
    },
}

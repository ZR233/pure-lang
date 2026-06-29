use serde::{Deserialize, Serialize};

// ── Runtime types ──

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeSnapshot {
    pub status: BridgeRuntimeStatus,
    pub active_turns: Vec<BridgeActiveTurn>,
    pub updated_at: i64,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum BridgeRuntimeStatus {
    Uninitialized,
    Initializing,
    Ready,
    ShuttingDown,
    Stopped,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BridgeActiveTurn {
    pub session_id: String,
    pub turn_id: String,
}

// ── Event types ──

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BridgeEventEnvelope {
    pub event_id: String,
    pub session_id: Option<String>,
    pub turn_id: Option<String>,
    pub sequence: u64,
    pub created_at: i64,
    pub payload: BridgeEventPayload,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum BridgeEventPayload {
    TurnChanged {
        turn: BridgeStudioTurnDto,
    },
    MessageUpdated {
        message: BridgeStudioMessageDto,
    },
    MessageRemoved {
        message_id: String,
    },
    MessagePartUpdated {
        part: Box<BridgeStudioPartDto>,
    },
    MessagePartRemoved {
        message_id: String,
        part_id: String,
    },
    MessagePartDelta {
        delta: BridgeStudioPartDeltaDto,
    },
    InteractionChanged {
        event: BridgeInteractionChangedDto,
    },
    AgentChanged {
        agent: Box<BridgeAgentSnapshotDto>,
    },
    AgentTimelineChanged {
        event: BridgeAgentTimelineEventDto,
    },
    SessionRuntimeChanged {
        runtime: Box<BridgeSessionRuntimeDto>,
    },
    SkillActivated {
        activation: BridgeSkillActivationDto,
    },
    PlanLifecycleChanged {
        event: BridgePlanLifecycleDto,
    },
    SessionListChanged {
        project_id: String,
        sessions: Vec<SessionDto>,
    },
    McpHealthChanged {
        health: BridgeMcpHealthDto,
    },
    LspHealthChanged {
        health: BridgeLspHealthDto,
    },
    Stale {
        lagged_events: u64,
    },
}

// ── DTO types ──

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BridgeStudioTurnDto {
    pub turn_id: String,
    pub session_id: String,
    pub status: String,
    pub reason: Option<String>,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BridgeStudioMessageDto {
    pub message_id: String,
    pub session_id: String,
    pub turn_id: String,
    pub role: String,
    pub status: String,
    pub created_at: i64,
    pub updated_at: i64,
    pub completed_at: Option<i64>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BridgeStudioPartDto {
    pub part_id: String,
    pub message_id: String,
    pub session_id: String,
    pub turn_id: String,
    pub part_type: String,
    pub order: u32,
    pub revision: u64,
    pub status: String,
    pub created_at: i64,
    pub updated_at: i64,
    pub completed_at: Option<i64>,
    pub error: Option<String>,
    pub text_channel: Option<String>,
    pub activity_group_id: Option<String>,
    pub text: Option<String>,
    pub tool: Option<BridgeStudioToolPartDto>,
    pub agent: Option<BridgeStudioAgentPartDto>,
    pub plan: Option<BridgeStudioPlanPartDto>,
    pub synthetic: bool,
    pub ignored: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BridgeStudioToolPartDto {
    pub tool_call_id: Option<String>,
    pub call_id: String,
    pub provider_item_id: Option<String>,
    pub name: String,
    pub arguments: String,
    pub result: Option<String>,
    pub exit_code: Option<i32>,
    pub timed_out: bool,
    pub working_directory: Option<String>,
    pub denial_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BridgeStudioAgentPartDto {
    pub id: String,
    pub path: String,
    pub parent_path: Option<String>,
    pub role: String,
    pub task: String,
    pub status: String,
    pub summary: Option<String>,
    pub depth: u32,
    pub error: Option<String>,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BridgeStudioPlanPartDto {
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BridgeStudioPartDeltaDto {
    pub part_id: String,
    pub revision: u64,
    pub field: String,
    pub delta: String,
    pub chunk_index: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BridgeInteractionChangedDto {
    pub interaction_id: String,
    pub kind: String,
    pub status: String,
    pub session_id: String,
    pub turn_id: Option<String>,
    pub item_id: Option<String>,
    pub tool_id: Option<String>,
    pub agent_path: Option<String>,
    pub payload: BridgeInteractionPayloadDto,
    pub created_at: i64,
    pub updated_at: i64,
    pub resolved_at: Option<i64>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum BridgeInteractionPayloadDto {
    UserInput {
        questions: Vec<BridgeUserQuestionDto>,
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BridgeUserQuestionDto {
    pub id: String,
    pub header: String,
    pub question: String,
    pub is_other: bool,
    pub is_secret: bool,
    pub options: Option<Vec<BridgeUserQuestionOptionDto>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BridgeUserQuestionOptionDto {
    pub label: String,
    pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BridgeAgentSnapshotDto {
    pub id: String,
    pub session_id: String,
    pub path: String,
    pub parent_path: Option<String>,
    pub role: String,
    pub task: String,
    pub status: String,
    pub summary: Option<String>,
    pub depth: u32,
    pub error: Option<String>,
    pub reason: Option<String>,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BridgeAgentTimelineEventDto {
    pub event_id: String,
    pub session_id: String,
    pub sequence: u64,
    pub created_at: i64,
    pub payload: BridgeAgentTimelinePayloadDto,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum BridgeAgentTimelinePayloadDto {
    SpawnBegin {
        call_id: String,
        sender_path: String,
        task_name: String,
        prompt: String,
        role: String,
        model: Option<String>,
        reasoning_effort: Option<String>,
    },
    SpawnEnd {
        call_id: String,
        sender_path: String,
        agent_id: Option<String>,
        path: Option<String>,
        role: Option<String>,
        status: String,
        prompt: String,
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
        status: String,
        prompt: String,
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
        status: String,
        error: Option<String>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BridgeSessionRuntimeDto {
    pub session_id: String,
    pub model: Option<String>,
    pub context_window: Option<u32>,
    pub latest_context_tokens: Option<u32>,
    pub prompt_tokens: Option<u32>,
    pub completion_tokens: Option<u32>,
    pub cached_prompt_tokens: Option<u32>,
    pub total_tokens: Option<u32>,
    pub estimated_costs: Vec<BridgeRuntimeCostAmountDto>,
    pub has_unpriced_usage: bool,
    pub active_skills: Vec<BridgeSkillActivationDto>,
    pub active_mcp_servers: Vec<String>,
    pub active_lsp_servers: Vec<String>,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BridgeRuntimeCostAmountDto {
    pub currency: String,
    pub amount: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BridgeSkillActivationDto {
    pub name: String,
    pub source: String,
    pub path: String,
    pub turn_id: String,
    pub tool_call_id: Option<String>,
    pub activated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BridgePlanLifecycleDto {
    pub plan_id: String,
    pub state: String,
    pub turn_id: String,
    pub reason: Option<String>,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BridgeMcpHealthDto {
    pub active_mcp_servers: u32,
    pub mcp_servers: Vec<BridgeMcpServerDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BridgeMcpServerDto {
    pub id: String,
    pub enabled: bool,
    pub transport: String,
    pub command: Option<String>,
    pub url: Option<String>,
    pub endpoint: Option<String>,
    pub status_kind: String,
    pub availability_kind: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BridgeLspHealthDto {
    pub active_lsp_servers: u32,
}

// ── Response types ──

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BridgeStudioSnapshotResponse {
    pub projects: Vec<ProjectDto>,
    pub selected_project_id: Option<String>,
    pub sessions: Vec<SessionDto>,
    pub selected_session_id: Option<String>,
    pub agent_events: Vec<BridgeAgentTimelineEventDto>,
    pub agents: Vec<BridgeAgentSnapshotDto>,
    pub interactions: Vec<BridgeInteractionChangedDto>,
    pub session_runtime: Option<BridgeSessionRuntimeDto>,
    pub config_json: String,
    pub general_settings_json: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProjectDto {
    pub id: String,
    pub name: String,
    pub path: String,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SessionDto {
    pub id: String,
    pub project_id: String,
    pub title: String,
    pub mode: String,
    pub updated_at: i64,
    pub visibility: String,
    pub parent_session_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BridgeStudioEventsResponse {
    pub session_id: String,
    pub events: Vec<BridgeEventEnvelope>,
    pub next_sequence: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BridgeSessionStateResponse {
    pub session_id: String,
    pub session: SessionDto,
    pub sessions: Vec<SessionDto>,
    pub messages: Vec<BridgeStudioMessageProjectionDto>,
    pub parts: Vec<BridgeStudioPartProjectionDto>,
    pub events: Vec<BridgeEventEnvelope>,
    pub event_next_sequence: u64,
    pub agents: Vec<BridgeAgentSnapshotDto>,
    pub agent_events: Vec<BridgeAgentTimelineEventDto>,
    pub interactions: Vec<BridgeInteractionChangedDto>,
    pub session_runtime: Option<BridgeSessionRuntimeDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BridgeStudioMessageProjectionDto {
    pub message: BridgeStudioMessageDto,
    pub sequence: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BridgeStudioPartProjectionDto {
    pub part: BridgeStudioPartDto,
    pub sequence: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SubmitPromptResponse {
    pub session_id: String,
    pub turn_id: String,
    pub cursor: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct StopPromptResponse {
    pub session_id: String,
    pub stopped: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ResolveInteractionResponse {
    pub session_id: String,
    pub interaction: BridgeInteractionChangedDto,
    pub sessions: Vec<SessionDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ProviderUsagesResponse {
    pub usages: Vec<ProviderUsageDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ProviderUsageDto {
    pub provider_id: String,
    pub updated_at: i64,
    pub status: String,
    pub usage_kind: String,
    pub message: Option<String>,
    pub balance: Option<DeepSeekBalanceDto>,
    pub coding_plan: Option<ZhipuCodingPlanUsageDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DeepSeekBalanceDto {
    pub is_available: bool,
    pub balances: Vec<DeepSeekBalanceInfoDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DeepSeekBalanceInfoDto {
    pub currency: String,
    pub total_balance: String,
    pub granted_balance: String,
    pub topped_up_balance: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ZhipuCodingPlanUsageDto {
    pub level: String,
    pub limits: Vec<ZhipuQuotaLimitDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ZhipuQuotaLimitDto {
    pub window: String,
    pub label: String,
    pub percentage: f64,
    pub current_value: u32,
    pub total: u32,
    pub remaining: u32,
    pub next_reset_at: Option<String>,
    pub usage_details: Option<Vec<ZhipuToolUsageDetailDto>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ZhipuToolUsageDetailDto {
    pub name: String,
    pub current_value: u32,
    pub total: u32,
    pub percentage: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SkillsResponse {
    pub skills: Vec<SkillSummaryDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SkillSummaryDto {
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ConfigSavedResponse {
    pub saved: bool,
}

// ── Input types ──

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderSettingsInput {
    pub default_provider_id: String,
    pub providers: Vec<ProviderInput>,
    pub roles: Vec<RoleInput>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderInput {
    pub id: String,
    pub template_kind: String,
    pub name: String,
    pub base_url: String,
    pub bearer_token: String,
    pub default_model: String,
    pub custom_models: Vec<ProviderModelInput>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderModelInput {
    pub slug: String,
    pub display_name: String,
    pub reasoning_efforts: Vec<String>,
    pub base_instructions: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RoleInput {
    pub key: String,
    pub provider: String,
    pub model: String,
    pub effort: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstructionsSettingsInput {
    pub base_override: String,
    pub developer: String,
    pub user: String,
    pub project_doc_max_bytes: usize,
    pub project_doc_fallback_filenames: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillsSettingsInput {
    pub enabled: bool,
    pub auto_learn: bool,
    pub system_enabled: bool,
    pub project_dir: String,
    pub user_dir: String,
    pub external_dirs: Vec<String>,
    pub disabled: Vec<String>,
    pub auto_learn_min_tool_calls: u32,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpSettingsInput {
    pub servers: Vec<McpServerInput>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpServerInput {
    pub id: String,
    pub enabled: bool,
    pub transport: String,
    pub endpoint: String,
}

// ── BridgeEventEnvelope helpers ──

impl BridgeEventEnvelope {
    pub fn stale(session_id: Option<String>, lagged_events: u64) -> Self {
        Self {
            event_id: {
                use std::time::{SystemTime, UNIX_EPOCH};
                let nanos = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_nanos();
                let suffix = format!("{nanos:x}");
                format!("bridge-stale-{suffix}")
            },
            session_id,
            turn_id: None,
            sequence: 0,
            created_at: {
                use std::time::{SystemTime, UNIX_EPOCH};
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs() as i64
            },
            payload: BridgeEventPayload::Stale { lagged_events },
        }
    }
}

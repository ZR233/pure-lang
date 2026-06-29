use std::future::Future;
use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use pl_core::{
    BuiltinMcpServerState, CompileMode, McpServerConfig, McpServerTransport, ModelRole,
    PermissionMode, ProviderEdit, ProviderModelEdit, ProviderSettingsEdit, ProviderTemplateKind,
    ProviderUsageData, ProviderUsageState, RoleEdit, SessionRecord,
    StudioResolveInteractionResponse as CoreResolveInteractionResponse, StudioRuntime,
    StudioRuntimeSnapshot as CoreRuntimeSnapshot, StudioSubmitPromptOptions,
    StudioSubmitPromptRequest, ZhipuQuotaWindow, is_builtin_mcp_server_id,
};
use pl_protocol::{
    InteractionPayload, InteractionRequest, InteractionResolution, RuntimeCostAmount,
    SkillActivation, StudioAgentSnapshot, StudioAgentTimelineEvent, StudioAgentTimelineEventKind,
    StudioEventEnvelope, StudioEventKind, StudioLspHealth, StudioMcpHealth, StudioMessage,
    StudioPart, StudioPartDelta, StudioPartDeltaField, StudioSessionRuntime, StudioTurn,
};
use serde::{Deserialize, Serialize};

use crate::frb_generated::StreamSink;

static BRIDGE: OnceLock<BridgeRuntime> = OnceLock::new();

struct BridgeRuntime {
    tokio: tokio::runtime::Runtime,
    studio: StudioRuntime,
}

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
        runtime: BridgeSessionRuntimeDto,
    },
    SkillActivated {
        activation: BridgeSkillActivationDto,
    },
    PlanLifecycleChanged {
        event: BridgePlanLifecycleDto,
    },
    SessionHandoffChanged,
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BridgeStudioPartDto {
    pub part_id: String,
    pub message_id: String,
    pub session_id: String,
    pub turn_id: String,
    pub part_type: String,
    pub order: u64,
    pub revision: u64,
    pub status: String,
    pub created_at: i64,
    pub updated_at: i64,
    pub completed_at: Option<i64>,
    pub error: Option<String>,
    pub text_channel: Option<String>,
    pub text: String,
    pub tool: Option<BridgeStudioToolPartDto>,
    pub agent: Option<BridgeStudioAgentPartDto>,
    pub plan: Option<BridgeStudioPlanPartDto>,
    pub synthetic: bool,
    pub ignored: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BridgeStudioToolPartDto {
    pub tool_call_id: String,
    pub call_id: Option<String>,
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
    pub chunk_index: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BridgeInteractionChangedDto {
    pub interaction_id: String,
    pub kind: String,
    pub status: String,
    pub session_id: String,
    pub turn_id: String,
    pub item_id: Option<String>,
    pub tool_id: Option<String>,
    pub agent_path: Option<String>,
    pub payload: BridgeInteractionPayloadDto,
    pub created_at: i64,
    pub updated_at: i64,
    pub resolved_at: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BridgeAgentTimelineEventDto {
    pub event_id: String,
    pub session_id: String,
    pub sequence: u64,
    pub created_at: i64,
    pub payload: BridgeAgentTimelinePayloadDto,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
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
    pub model: String,
    pub context_window: Option<u64>,
    pub latest_context_tokens: u64,
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub cached_prompt_tokens: u64,
    pub total_tokens: u64,
    pub estimated_costs: Vec<BridgeRuntimeCostAmountDto>,
    pub has_unpriced_usage: bool,
    pub active_skills: Vec<String>,
    pub active_mcp_servers: Vec<String>,
    pub active_lsp_servers: Vec<String>,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BridgeRuntimeCostAmountDto {
    pub currency: String,
    pub amount: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BridgeSkillActivationDto {
    pub name: String,
    pub source: String,
    pub path: String,
    pub turn_id: String,
    pub tool_call_id: String,
    pub activated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BridgePlanLifecycleDto {
    pub plan_id: String,
    pub state: String,
    pub turn_id: Option<String>,
    pub reason: Option<String>,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BridgeMcpHealthDto {
    pub active_mcp_servers: Vec<String>,
    pub mcp_servers: Vec<BridgeMcpServerDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BridgeMcpServerDto {
    pub id: String,
    pub enabled: bool,
    pub transport: String,
    pub command: Option<String>,
    pub url: Option<String>,
    pub endpoint: String,
    pub status_kind: String,
    pub availability_kind: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BridgeLspHealthDto {
    pub active_lsp_servers: Vec<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
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

#[derive(Debug, Clone, Serialize, PartialEq)]
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

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BridgeStudioEventsResponse {
    pub session_id: String,
    pub events: Vec<BridgeEventEnvelope>,
    pub next_sequence: u64,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
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

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BridgeStudioMessageProjectionDto {
    pub message: BridgeStudioMessageDto,
    pub sequence: u64,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BridgeStudioPartProjectionDto {
    pub part: BridgeStudioPartDto,
    pub sequence: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SubmitPromptResponse {
    pub session_id: String,
    pub turn_id: String,
    pub cursor: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct StopPromptResponse {
    pub session_id: String,
    pub stopped: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ResolveInteractionResponse {
    pub session_id: String,
    pub interaction: BridgeInteractionChangedDto,
    pub sessions: Vec<SessionDto>,
}

/// Provider 用量查询返回体。
///
/// 与 Studio provider usage wire 格式保持 camelCase，供 Flutter 列表卡片渲染。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ProviderUsagesResponse {
    pub usages: Vec<ProviderUsageDto>,
}

/// 单个 Provider 的用量状态。
///
/// status/usage_kind 是 Dart 层路由字段，复杂 provider payload 保持结构化字段。
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

/// DeepSeek 余额用量。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DeepSeekBalanceDto {
    pub is_available: bool,
    pub balances: Vec<DeepSeekBalanceInfoDto>,
}

/// DeepSeek 单币种余额明细。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DeepSeekBalanceInfoDto {
    pub currency: String,
    pub total_balance: String,
    pub granted_balance: String,
    pub topped_up_balance: String,
}

/// 智谱 Coding Plan 用量。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ZhipuCodingPlanUsageDto {
    pub level: Option<String>,
    pub limits: Vec<ZhipuQuotaLimitDto>,
}

/// 智谱 Coding Plan 单个时间窗口额度。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ZhipuQuotaLimitDto {
    pub window: String,
    pub label: String,
    pub percentage: f64,
    pub current_value: Option<f64>,
    pub total: Option<f64>,
    pub remaining: Option<f64>,
    pub next_reset_at: Option<i64>,
    pub usage_details: Vec<ZhipuToolUsageDetailDto>,
}

/// 智谱 Coding Plan 工具级用量明细。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ZhipuToolUsageDetailDto {
    pub name: String,
    pub current_value: Option<f64>,
    pub total: Option<f64>,
    pub percentage: Option<f64>,
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
pub struct SettingsDraftResponse {
    pub section: String,
    pub saved: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ConfigSavedResponse {
    pub saved: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ProviderSettingsInput {
    default_provider_id: Option<String>,
    providers: Vec<ProviderInput>,
    roles: Vec<RoleInput>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ProviderInput {
    id: String,
    template_kind: String,
    name: String,
    base_url: String,
    bearer_token: String,
    default_model: String,
    custom_models: Vec<ProviderModelInput>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ProviderModelInput {
    slug: String,
    display_name: String,
    reasoning_efforts: Vec<String>,
    base_instructions: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RoleInput {
    key: String,
    provider: String,
    model: String,
    effort: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct InstructionsSettingsInput {
    base_override: String,
    developer: String,
    user: String,
    project_doc_max_bytes: usize,
    project_doc_fallback_filenames: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SkillsSettingsInput {
    enabled: bool,
    auto_learn: bool,
    system_enabled: bool,
    project_dir: String,
    user_dir: String,
    external_dirs: Vec<String>,
    disabled: Vec<String>,
    auto_learn_min_tool_calls: u32,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct McpSettingsInput {
    servers: Vec<McpServerInput>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct McpServerInput {
    id: String,
    enabled: bool,
    transport: String,
    endpoint: String,
}

pub fn initialize_runtime() -> Result<RuntimeSnapshot> {
    let bridge = bridge()?;
    bridge.block_on(async {
        bridge
            .studio
            .initialize_runtime()
            .await
            .map(runtime_snapshot)
    })
}

pub fn start_runtime() -> Result<RuntimeSnapshot> {
    let bridge = bridge()?;
    bridge.block_on(async { bridge.studio.start_runtime().await.map(runtime_snapshot) })
}

pub fn shutdown_runtime() -> Result<RuntimeSnapshot> {
    let bridge = bridge()?;
    bridge.block_on(async { bridge.studio.shutdown_runtime().await.map(runtime_snapshot) })
}

pub fn bootstrap_studio() -> Result<BridgeStudioSnapshotResponse> {
    let bridge = bridge()?;
    bridge.block_on(async { bootstrap_studio_inner(bridge).await })
}

pub fn open_project(path: String) -> Result<BridgeStudioSnapshotResponse> {
    let bridge = bridge()?;
    bridge.block_on(async {
        let project = bridge.studio.open_project(path).await?;
        bridge
            .studio
            .reconcile_lsp_runtime_for_project(&project.id)
            .await?;
        let _ = bridge.studio.ensure_project_sessions(&project.id).await?;
        studio_snapshot_inner(bridge, Some(project.id), None).await
    })
}

pub fn select_project(project_id: String) -> Result<BridgeStudioSnapshotResponse> {
    let bridge = bridge()?;
    bridge.block_on(async { studio_snapshot_inner(bridge, Some(project_id), None).await })
}

pub fn archive_project(
    project_id: String,
    selected_project_id: Option<String>,
) -> Result<BridgeStudioSnapshotResponse> {
    let bridge = bridge()?;
    bridge.block_on(async {
        bridge
            .studio
            .archive_project(&project_id)
            .await?
            .context("selected project not found")?;
        let projects = bridge.studio.list_projects().await?;
        let next_project_id = selected_project_id
            .filter(|id| id != &project_id && projects.iter().any(|project| project.id == *id))
            .or_else(|| projects.first().map(|project| project.id.clone()));
        studio_snapshot_from_projects_inner(bridge, projects, next_project_id, None).await
    })
}

pub fn create_session(
    project_id: String,
    title: Option<String>,
) -> Result<BridgeStudioSnapshotResponse> {
    let bridge = bridge()?;
    bridge.block_on(async {
        let title = title
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| "新会话".to_string());
        let session = bridge.studio.create_session(&project_id, &title).await?;
        studio_snapshot_inner(bridge, Some(project_id), Some(session.id)).await
    })
}

pub fn archive_session(
    session_id: String,
    selected_session_id: Option<String>,
) -> Result<BridgeStudioSnapshotResponse> {
    let bridge = bridge()?;
    bridge.block_on(async {
        let archived = bridge
            .studio
            .archive_session(session_id.clone())
            .await?
            .context("selected session not found")?;
        let sessions = bridge
            .studio
            .store()
            .list_sessions(&archived.project_id)
            .await?;
        let next_session_id = selected_session_id
            .filter(|id| id != &session_id && sessions.iter().any(|session| session.id == *id))
            .or_else(|| sessions.first().map(|session| session.id.clone()));
        studio_snapshot_inner(bridge, Some(archived.project_id), next_session_id).await
    })
}

pub fn set_session_mode(session_id: String, mode: String) -> Result<BridgeSessionStateResponse> {
    let bridge = bridge()?;
    bridge.block_on(async {
        bridge
            .studio
            .set_session_mode(&session_id, CompileMode::from_label(&mode))
            .await?;
        load_session_state_inner(bridge, session_id).await
    })
}

pub fn set_model_role(
    role_key: String,
    provider_id: String,
    model: String,
    effort: Option<String>,
    selected_session_id: Option<String>,
) -> Result<BridgeStudioSnapshotResponse> {
    let bridge = bridge()?;
    bridge.block_on(async {
        let role = ModelRole::from_key(role_key.trim())
            .with_context(|| format!("unsupported model role: {role_key}"))?;
        bridge
            .studio
            .set_model_role(role, &provider_id, &model, effort.as_deref())?;
        let selected_session_id = selected_session_id.filter(|value| !value.trim().is_empty());
        let selected_project_id = match selected_session_id.as_deref() {
            Some(session_id) => Some(
                bridge
                    .studio
                    .store()
                    .read_session(session_id)
                    .await?
                    .context("selected session not found")?
                    .project_id,
            ),
            None => None,
        };
        studio_snapshot_inner(bridge, selected_project_id, selected_session_id).await
    })
}

pub fn save_runtime_permission_mode(mode: String) -> Result<ConfigSavedResponse> {
    let bridge = bridge()?;
    bridge.block_on(async {
        let mut config = bridge.studio.config_store().load_or_default()?;
        config.runtime.permission_mode = PermissionMode::from_label(&mode);
        bridge.studio.config_store().save(&config)?;
        Ok(ConfigSavedResponse { saved: true })
    })
}

pub fn save_provider_settings(settings_json: String) -> Result<BridgeStudioSnapshotResponse> {
    let bridge = bridge()?;
    bridge.block_on(async {
        let input: ProviderSettingsInput =
            serde_json::from_str(&settings_json).context("invalid provider settings json")?;
        let current = bridge.studio.config_store().load_or_default()?;
        let next = provider_settings_edit(input, &current)?.to_config(&current)?;
        bridge.studio.config_store().save(&next)?;
        studio_snapshot_inner(bridge, None, None).await
    })
}

pub fn save_instructions_settings(settings_json: String) -> Result<BridgeStudioSnapshotResponse> {
    let bridge = bridge()?;
    bridge.block_on(async {
        let input: InstructionsSettingsInput =
            serde_json::from_str(&settings_json).context("invalid instructions settings json")?;
        let mut config = bridge.studio.config_store().load_or_default()?;
        config.instructions.base_override = input.base_override.trim().to_string();
        config.instructions.developer = input.developer.trim().to_string();
        config.instructions.user = input.user.trim().to_string();
        config.instructions.project_doc_max_bytes = input.project_doc_max_bytes;
        config.instructions.project_doc_fallback_filenames = input
            .project_doc_fallback_filenames
            .into_iter()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .collect();
        config.validate()?;
        bridge.studio.config_store().save(&config)?;
        studio_snapshot_inner(bridge, None, None).await
    })
}

pub fn save_skills_settings(settings_json: String) -> Result<BridgeStudioSnapshotResponse> {
    let bridge = bridge()?;
    bridge.block_on(async {
        let input: SkillsSettingsInput =
            serde_json::from_str(&settings_json).context("invalid skills settings json")?;
        let mut config = bridge.studio.config_store().load_or_default()?;
        config.skills.enabled = input.enabled;
        config.skills.auto_learn = input.auto_learn;
        config.skills.system.enabled = input.system_enabled;
        config.skills.project_dir = input.project_dir.trim().to_string();
        config.skills.user_dir = input.user_dir.trim().to_string();
        config.skills.external_dirs = input
            .external_dirs
            .into_iter()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .collect();
        config.skills.disabled = normalized_string_list(input.disabled);
        config.skills.auto_learn_min_tool_calls = input.auto_learn_min_tool_calls;
        config.validate()?;
        bridge.studio.config_store().save(&config)?;
        studio_snapshot_inner(bridge, None, None).await
    })
}

pub fn save_mcp_settings(settings_json: String) -> Result<BridgeStudioSnapshotResponse> {
    let bridge = bridge()?;
    bridge.block_on(async {
        let input: McpSettingsInput =
            serde_json::from_str(&settings_json).context("invalid mcp settings json")?;
        let mut config = bridge.studio.config_store().load_or_default()?;
        let mut next_servers = config.mcp_servers.clone();
        let mut next_builtin = config.builtin_mcp_servers.clone();
        for server in input.servers {
            let server_id = server.id.trim().to_string();
            if server_id.is_empty() {
                continue;
            }
            if is_builtin_mcp_server_id(&server_id) {
                next_builtin.insert(
                    server_id,
                    BuiltinMcpServerState {
                        enabled: server.enabled,
                    },
                );
                continue;
            }
            let mut config = next_servers
                .remove(&server_id)
                .unwrap_or_else(|| McpServerConfig {
                    transport: mcp_transport_from_label(&server.transport),
                    ..Default::default()
                });
            config.enabled = server.enabled;
            if !server.transport.trim().is_empty() {
                config.transport = mcp_transport_from_label(&server.transport);
            }
            let endpoint = server.endpoint.trim();
            match config.transport {
                McpServerTransport::Stdio => {
                    config.command = (!endpoint.is_empty()).then(|| endpoint.to_string());
                }
                McpServerTransport::StreamableHttp => {
                    config.url = (!endpoint.is_empty()).then(|| endpoint.to_string());
                }
            }
            next_servers.insert(server_id, config);
        }
        config.mcp_servers = next_servers;
        config.builtin_mcp_servers = next_builtin;
        config.validate()?;
        bridge.studio.config_store().save(&config)?;
        studio_snapshot_inner(bridge, None, None).await
    })
}

pub fn save_general_settings(settings_json: String) -> Result<BridgeStudioSnapshotResponse> {
    let bridge = bridge()?;
    bridge.block_on(async {
        let draft: serde_json::Value =
            serde_json::from_str(&settings_json).context("invalid general settings json")?;
        let normalized = serde_json::to_string(&draft)?;
        bridge
            .studio
            .store()
            .save_setting("flutterSettings:general", &normalized)
            .await?;
        studio_snapshot_inner(bridge, None, None).await
    })
}

pub fn load_provider_usages() -> Result<ProviderUsagesResponse> {
    let bridge = bridge()?;
    bridge.block_on(async {
        let usages = bridge
            .studio
            .provider_usages()
            .await?
            .into_iter()
            .map(provider_usage_dto)
            .collect();
        Ok(ProviderUsagesResponse { usages })
    })
}

pub fn list_discovered_skills(project_id: String) -> Result<SkillsResponse> {
    let bridge = bridge()?;
    bridge.block_on(async {
        let catalog = bridge.studio.discovered_skills(&project_id).await?;
        Ok(SkillsResponse {
            skills: catalog
                .skills
                .into_iter()
                .map(|skill| SkillSummaryDto { name: skill.name })
                .collect(),
        })
    })
}

pub fn save_studio_settings_draft(
    section: String,
    draft_json: String,
) -> Result<SettingsDraftResponse> {
    let bridge = bridge()?;
    bridge.block_on(async {
        let draft: serde_json::Value =
            serde_json::from_str(&draft_json).context("invalid settings draft json")?;
        let normalized = serde_json::to_string(&draft)?;
        bridge
            .studio
            .store()
            .save_setting(&studio_settings_draft_key(&section), &normalized)
            .await?;
        Ok(SettingsDraftResponse {
            section,
            saved: true,
        })
    })
}

fn provider_settings_edit(
    input: ProviderSettingsInput,
    current: &pl_core::PureConfig,
) -> Result<ProviderSettingsEdit> {
    Ok(ProviderSettingsEdit {
        default_provider: input.default_provider_id,
        providers: input
            .providers
            .into_iter()
            .map(|provider| provider_edit(provider, current))
            .collect::<Result<Vec<_>>>()?,
        roles: input.roles.into_iter().map(role_edit).collect(),
    })
}

fn provider_edit(input: ProviderInput, current: &pl_core::PureConfig) -> Result<ProviderEdit> {
    let kind = ProviderTemplateKind::from_key(&input.template_kind)
        .with_context(|| format!("unsupported provider template: {}", input.template_kind))?;
    let current_token = current
        .providers
        .get(&input.id)
        .and_then(|provider| provider.bearer_token.clone());
    let bearer_token = if input.bearer_token.trim().is_empty() {
        current_token
    } else {
        Some(input.bearer_token)
    };
    Ok(ProviderEdit {
        key: input.id,
        kind,
        name: input.name,
        base_url: Some(input.base_url),
        bearer_token,
        default_model: input.default_model,
        custom_models: input
            .custom_models
            .into_iter()
            .map(provider_model_edit)
            .collect(),
    })
}

fn provider_model_edit(input: ProviderModelInput) -> ProviderModelEdit {
    ProviderModelEdit {
        slug: input.slug,
        display_name: input.display_name,
        reasoning_efforts: input.reasoning_efforts,
        base_instructions: input.base_instructions.unwrap_or_default(),
    }
}

fn provider_usage_dto(record: pl_core::ProviderUsageRecord) -> ProviderUsageDto {
    match record.state {
        ProviderUsageState::Unsupported => ProviderUsageDto {
            provider_id: record.provider_id,
            updated_at: record.updated_at,
            status: "unsupported".to_string(),
            usage_kind: "unsupported".to_string(),
            message: None,
            balance: None,
            coding_plan: None,
        },
        ProviderUsageState::MissingCredential => ProviderUsageDto {
            provider_id: record.provider_id,
            updated_at: record.updated_at,
            status: "missingCredential".to_string(),
            usage_kind: "unknown".to_string(),
            message: Some("provider API key is not configured".to_string()),
            balance: None,
            coding_plan: None,
        },
        ProviderUsageState::Failed(message) => ProviderUsageDto {
            provider_id: record.provider_id,
            updated_at: record.updated_at,
            status: "failed".to_string(),
            usage_kind: "unknown".to_string(),
            message: Some(message),
            balance: None,
            coding_plan: None,
        },
        ProviderUsageState::Ready(ProviderUsageData::DeepSeekBalance(balance)) => {
            ProviderUsageDto {
                provider_id: record.provider_id,
                updated_at: record.updated_at,
                status: "ready".to_string(),
                usage_kind: "deepseekBalance".to_string(),
                message: None,
                balance: Some(DeepSeekBalanceDto {
                    is_available: balance.is_available,
                    balances: balance
                        .balances
                        .into_iter()
                        .map(|item| DeepSeekBalanceInfoDto {
                            currency: item.currency,
                            total_balance: item.total_balance,
                            granted_balance: item.granted_balance,
                            topped_up_balance: item.topped_up_balance,
                        })
                        .collect(),
                }),
                coding_plan: None,
            }
        }
        ProviderUsageState::Ready(ProviderUsageData::ZhipuCodingPlan(usage)) => ProviderUsageDto {
            provider_id: record.provider_id,
            updated_at: record.updated_at,
            status: "ready".to_string(),
            usage_kind: "zhipuCodingPlan".to_string(),
            message: None,
            balance: None,
            coding_plan: Some(ZhipuCodingPlanUsageDto {
                level: usage.level,
                limits: usage
                    .limits
                    .into_iter()
                    .map(|limit| {
                        let (window, label) = zhipu_window_labels(&limit.window);
                        ZhipuQuotaLimitDto {
                            window: window.to_string(),
                            label: label.to_string(),
                            percentage: limit.percentage,
                            current_value: limit.current_value,
                            total: limit.total,
                            remaining: limit.remaining,
                            next_reset_at: limit.next_reset_at,
                            usage_details: limit
                                .usage_details
                                .into_iter()
                                .map(|detail| ZhipuToolUsageDetailDto {
                                    name: detail.name,
                                    current_value: detail.current_value,
                                    total: detail.total,
                                    percentage: detail.percentage,
                                })
                                .collect(),
                        }
                    })
                    .collect(),
            }),
        },
    }
}

fn zhipu_window_labels(window: &ZhipuQuotaWindow) -> (&'static str, &str) {
    match window {
        ZhipuQuotaWindow::FiveHour => ("fiveHour", "5h"),
        ZhipuQuotaWindow::Weekly => ("weekly", "7d"),
        ZhipuQuotaWindow::McpMonthly => ("mcpMonthly", "MCP"),
        ZhipuQuotaWindow::Other(label) => ("other", label.as_str()),
    }
}

fn role_edit(input: RoleInput) -> RoleEdit {
    RoleEdit {
        key: input.key,
        provider: input.provider,
        model: input.model,
        effort: input.effort,
    }
}

pub fn submit_prompt(
    session_id: String,
    prompt: String,
    attachment_ids: Vec<String>,
) -> Result<SubmitPromptResponse> {
    let bridge = bridge()?;
    bridge.block_on(async {
        let response = bridge
            .studio
            .submit_prompt(StudioSubmitPromptRequest {
                session_id,
                prompt,
                attachment_ids,
                options: StudioSubmitPromptOptions::default(),
            })
            .await?;
        Ok(SubmitPromptResponse {
            session_id: response.session_id,
            turn_id: response.turn_id,
            cursor: response.cursor,
        })
    })
}

pub fn stop_prompt(session_id: String) -> Result<StopPromptResponse> {
    let bridge = bridge()?;
    bridge.block_on(async {
        let response = bridge.studio.stop_prompt(session_id).await?;
        Ok(StopPromptResponse {
            session_id: response.session_id,
            stopped: response.stopped,
        })
    })
}

pub fn resolve_interaction(
    interaction_id: String,
    resolution_json: String,
) -> Result<ResolveInteractionResponse> {
    let bridge = bridge()?;
    bridge.block_on(async {
        let resolution: InteractionResolution = serde_json::from_str(&resolution_json)
            .context("invalid interaction resolution json")?;
        let response = bridge
            .studio
            .resolve_interaction(interaction_id, resolution)
            .await?;
        Ok(resolve_interaction_response(response))
    })
}

pub fn load_session_state(session_id: String) -> Result<BridgeSessionStateResponse> {
    let bridge = bridge()?;
    bridge.block_on(async { load_session_state_inner(bridge, session_id).await })
}

pub fn load_studio_events(
    session_id: String,
    after_sequence: Option<i64>,
    limit: Option<i64>,
) -> Result<BridgeStudioEventsResponse> {
    let bridge = bridge()?;
    bridge.block_on(async {
        let events = bridge
            .studio
            .store()
            .load_studio_events(&session_id, after_sequence, limit)
            .await?;
        let events = events
            .into_iter()
            .filter(bridge_visible_event)
            .map(BridgeEventEnvelope::from)
            .collect::<Vec<_>>();
        let next_sequence = bridge
            .studio
            .store()
            .next_studio_event_sequence(&session_id)
            .await? as u64;
        Ok(BridgeStudioEventsResponse {
            session_id,
            events,
            next_sequence,
        })
    })
}

pub fn subscribe_session_events(
    session_id: String,
    sink: StreamSink<BridgeEventEnvelope>,
) -> Result<()> {
    let bridge = bridge()?;
    let stale_session_id = session_id.clone();
    let mut events = bridge.studio.events().subscribe_session(session_id);
    bridge.tokio.spawn(async move {
        loop {
            match events.recv().await {
                Ok(event) => {
                    if !bridge_visible_event(&event) {
                        continue;
                    }
                    if sink.add(BridgeEventEnvelope::from(event)).is_err() {
                        break;
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(lagged_events)) => {
                    if sink
                        .add(BridgeEventEnvelope::stale(
                            Some(stale_session_id.clone()),
                            lagged_events,
                        ))
                        .is_err()
                    {
                        break;
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            }
        }
    });
    Ok(())
}

pub fn subscribe_global_events(sink: StreamSink<BridgeEventEnvelope>) -> Result<()> {
    let bridge = bridge()?;
    let mut events = bridge.studio.events().subscribe_global();
    bridge.tokio.spawn(async move {
        loop {
            match events.recv().await {
                Ok(event) => {
                    if !bridge_visible_event(&event) {
                        continue;
                    }
                    if sink.add(BridgeEventEnvelope::from(event)).is_err() {
                        break;
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(lagged_events)) => {
                    if sink
                        .add(BridgeEventEnvelope::stale(None, lagged_events))
                        .is_err()
                    {
                        break;
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            }
        }
    });
    Ok(())
}

impl BridgeRuntime {
    fn new() -> Result<Self> {
        let tokio = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .thread_name("pl-studio-bridge")
            .build()?;
        let studio = tokio.block_on(StudioRuntime::default_app())?;
        Ok(Self { tokio, studio })
    }

    fn block_on<T>(&self, future: impl Future<Output = T>) -> T {
        self.tokio.block_on(future)
    }
}

fn bridge() -> Result<&'static BridgeRuntime> {
    if let Some(runtime) = BRIDGE.get() {
        return Ok(runtime);
    }
    let runtime = BridgeRuntime::new()?;
    let _ = BRIDGE.set(runtime);
    BRIDGE
        .get()
        .context("Studio bridge runtime was not initialized")
}

async fn bootstrap_studio_inner(
    bridge: &'static BridgeRuntime,
) -> Result<BridgeStudioSnapshotResponse> {
    let mut projects = bridge.studio.list_projects().await?;
    if projects.is_empty()
        && !bridge.studio.store().has_projects().await?
        && let Ok(cwd) = std::env::current_dir()
    {
        projects.push(bridge.studio.open_project(cwd).await?);
    }
    let selected_project_id = projects.first().map(|project| project.id.clone());
    studio_snapshot_from_projects_inner(bridge, projects, selected_project_id, None).await
}

async fn studio_snapshot_inner(
    bridge: &'static BridgeRuntime,
    requested_project_id: Option<String>,
    requested_session_id: Option<String>,
) -> Result<BridgeStudioSnapshotResponse> {
    let projects = bridge.studio.list_projects().await?;
    studio_snapshot_from_projects_inner(
        bridge,
        projects,
        requested_project_id,
        requested_session_id,
    )
    .await
}

async fn studio_snapshot_from_projects_inner(
    bridge: &'static BridgeRuntime,
    projects: Vec<pl_core::ProjectRecord>,
    requested_project_id: Option<String>,
    requested_session_id: Option<String>,
) -> Result<BridgeStudioSnapshotResponse> {
    let selected_project = requested_project_id
        .as_deref()
        .and_then(|project_id| {
            projects
                .iter()
                .find(|project| project.id == project_id)
                .cloned()
        })
        .or_else(|| projects.first().cloned());
    let selected_project_id = selected_project.as_ref().map(|project| project.id.clone());
    let mut sessions = Vec::new();
    let mut selected_session_id = None;
    let mut agent_events = Vec::new();
    let mut agents = Vec::new();
    let mut interactions = Vec::new();

    if let Some(project) = selected_project {
        bridge
            .studio
            .reconcile_lsp_runtime_for_project(&project.id)
            .await?;
        sessions = bridge.studio.ensure_project_sessions(&project.id).await?;
        selected_session_id = requested_session_id
            .filter(|session_id| sessions.iter().any(|session| session.id == *session_id))
            .or_else(|| sessions.first().map(|session| session.id.clone()));
        if let Some(session_id) = selected_session_id.as_deref() {
            agent_events = bridge.studio.store().list_agent_events(session_id).await?;
            agents = bridge.studio.store().list_agents(session_id).await?;
            interactions = bridge
                .studio
                .store()
                .list_pending_interactions(session_id)
                .await?;
        }
    }
    let session_runtime = match selected_session_id.as_deref() {
        Some(session_id) => Some(bridge_session_runtime_view(bridge, session_id).await?),
        None => None,
    };
    let config_json = serde_json::to_string(&bridge.studio.config_store().load_or_default()?)?;
    let general_settings = bridge
        .studio
        .store()
        .load_setting("flutterSettings:general")
        .await?
        .and_then(|value| serde_json::from_str::<serde_json::Value>(&value).ok())
        .unwrap_or_else(|| serde_json::json!({}));
    let general_settings_json = serde_json::to_string(&general_settings)?;

    Ok(BridgeStudioSnapshotResponse {
        projects: projects.into_iter().map(project_dto).collect(),
        selected_project_id,
        sessions: sessions.into_iter().map(session_dto).collect(),
        selected_session_id,
        agent_events: agent_events
            .into_iter()
            .map(agent_event_bridge_dto)
            .collect::<Result<Vec<_>>>()?,
        agents: agents.into_iter().map(agent_bridge_dto).collect(),
        interactions: interactions
            .into_iter()
            .map(interaction_request_bridge_dto)
            .collect(),
        session_runtime,
        config_json,
        general_settings_json,
    })
}

async fn load_session_state_inner(
    bridge: &'static BridgeRuntime,
    session_id: String,
) -> Result<BridgeSessionStateResponse> {
    let session = bridge
        .studio
        .store()
        .read_session(&session_id)
        .await?
        .context("selected session not found")?;
    let events = bridge
        .studio
        .store()
        .load_studio_events(&session_id, None, None)
        .await?
        .into_iter()
        .filter(bridge_visible_event)
        .filter(is_session_state_event)
        .map(BridgeEventEnvelope::from)
        .collect();
    let messages = bridge
        .studio
        .store()
        .load_studio_messages(&session_id)
        .await?
        .into_iter()
        .map(|record| BridgeStudioMessageProjectionDto {
            message: bridge_message(record.message),
            sequence: record.sequence.max(0) as u64,
        })
        .collect();
    let parts = bridge
        .studio
        .store()
        .load_message_parts(&session_id)
        .await?
        .into_iter()
        .map(|record| BridgeStudioPartProjectionDto {
            part: bridge_part(record.part),
            sequence: record.sequence.max(0) as u64,
        })
        .collect();
    let event_next_sequence = bridge
        .studio
        .store()
        .next_studio_event_sequence(&session_id)
        .await? as u64;
    let sessions = bridge
        .studio
        .store()
        .list_sessions(&session.project_id)
        .await?
        .into_iter()
        .map(session_dto)
        .collect();
    let agents = bridge
        .studio
        .store()
        .list_agents(&session_id)
        .await?
        .into_iter()
        .map(agent_bridge_dto)
        .collect();
    let agent_events = bridge
        .studio
        .store()
        .list_agent_events(&session_id)
        .await?
        .into_iter()
        .map(agent_event_bridge_dto)
        .collect::<Result<Vec<_>>>()?;
    let interactions = bridge
        .studio
        .store()
        .list_pending_interactions(&session_id)
        .await?
        .into_iter()
        .map(interaction_request_bridge_dto)
        .collect();
    let session_runtime = bridge_session_runtime_view(bridge, &session_id).await.ok();

    Ok(BridgeSessionStateResponse {
        session_id: session_id.clone(),
        session: session_dto(session),
        sessions,
        messages,
        parts,
        events,
        event_next_sequence,
        agents,
        agent_events,
        interactions,
        session_runtime,
    })
}

fn is_session_state_event(event: &StudioEventEnvelope) -> bool {
    match &event.kind {
        StudioEventKind::MessageUpdated { .. }
        | StudioEventKind::MessageRemoved { .. }
        | StudioEventKind::MessagePartUpdated { .. }
        | StudioEventKind::MessagePartRemoved { .. }
        | StudioEventKind::MessagePartDelta { .. }
        | StudioEventKind::SessionHandoffChanged { .. }
        | StudioEventKind::Stale { .. } => false,
        StudioEventKind::TurnChanged { .. }
        | StudioEventKind::InteractionChanged { .. }
        | StudioEventKind::PlanLifecycleChanged { .. }
        | StudioEventKind::SessionRuntimeChanged { .. }
        | StudioEventKind::AgentChanged { .. }
        | StudioEventKind::AgentTimelineChanged { .. }
        | StudioEventKind::SkillActivated { .. }
        | StudioEventKind::SessionListChanged { .. }
        | StudioEventKind::McpHealthChanged { .. }
        | StudioEventKind::LspHealthChanged { .. } => true,
    }
}

fn bridge_visible_event(event: &StudioEventEnvelope) -> bool {
    !matches!(event.kind, StudioEventKind::SessionHandoffChanged { .. })
}

fn project_dto(project: pl_core::ProjectRecord) -> ProjectDto {
    ProjectDto {
        id: project.id,
        name: project.name,
        path: project.path,
        updated_at: project.updated_at,
    }
}

fn session_dto(session: SessionRecord) -> SessionDto {
    SessionDto {
        id: session.id,
        project_id: session.project_id,
        title: session.title,
        mode: session.mode,
        updated_at: session.updated_at,
        visibility: session.visibility.as_str().to_string(),
        parent_session_id: session.parent_session_id,
    }
}

fn agent_bridge_dto(agent: pl_core::StudioAgentSnapshotRecord) -> BridgeAgentSnapshotDto {
    BridgeAgentSnapshotDto {
        id: agent.id,
        session_id: agent.session_id,
        path: agent.path,
        parent_path: agent.parent_path,
        role: agent.role,
        task: agent.task,
        status: agent.status.as_str().to_string(),
        summary: agent.summary,
        depth: agent.depth as u32,
        error: agent.error,
        reason: agent.reason,
        updated_at: agent.updated_at,
    }
}

fn agent_event_bridge_dto(
    event: pl_core::StudioAgentTimelineEventRecord,
) -> Result<BridgeAgentTimelineEventDto> {
    let payload = serde_json::from_str::<StudioAgentTimelineEvent>(&event.payload_json)
        .with_context(|| format!("invalid agent timeline payload: {}", event.event_id))
        .map(|event| bridge_agent_timeline_payload(event.kind))?;
    Ok(BridgeAgentTimelineEventDto {
        event_id: event.event_id,
        session_id: event.session_id,
        sequence: event.sequence.max(0) as u64,
        created_at: event.created_at,
        payload,
    })
}

async fn bridge_session_runtime_view(
    bridge: &'static BridgeRuntime,
    session_id: &str,
) -> Result<BridgeSessionRuntimeDto> {
    let runtime = bridge.studio.session_runtime(session_id).await?;
    let active_skills = bridge
        .studio
        .store()
        .list_session_skill_names(session_id)
        .await?;
    Ok(BridgeSessionRuntimeDto {
        session_id: runtime.session_id,
        model: runtime.model,
        context_window: runtime.context_window,
        latest_context_tokens: runtime.latest_context_tokens,
        prompt_tokens: runtime.prompt_tokens,
        completion_tokens: runtime.completion_tokens,
        cached_prompt_tokens: runtime.cached_prompt_tokens,
        total_tokens: runtime.total_tokens,
        estimated_costs: runtime
            .estimated_costs
            .into_iter()
            .map(bridge_cost_amount)
            .collect(),
        has_unpriced_usage: runtime.has_unpriced_usage,
        active_skills,
        active_mcp_servers: bridge.studio.mcp_runtime().available_server_names().await,
        active_lsp_servers: bridge.studio.lsp_runtime().active_server_names().await,
        updated_at: runtime.updated_at,
    })
}

fn interaction_request_bridge_dto(interaction: InteractionRequest) -> BridgeInteractionChangedDto {
    bridge_interaction_changed(pl_protocol::InteractionChangedEvent { interaction })
}

fn resolve_interaction_response(
    response: CoreResolveInteractionResponse,
) -> ResolveInteractionResponse {
    ResolveInteractionResponse {
        session_id: response.session_id,
        interaction: bridge_interaction_changed(pl_protocol::InteractionChangedEvent {
            interaction: response.interaction,
        }),
        sessions: response.sessions.into_iter().map(session_dto).collect(),
    }
}

fn runtime_snapshot(snapshot: CoreRuntimeSnapshot) -> RuntimeSnapshot {
    RuntimeSnapshot {
        status: match snapshot.status {
            pl_core::StudioRuntimeStatus::Uninitialized => BridgeRuntimeStatus::Uninitialized,
            pl_core::StudioRuntimeStatus::Initializing => BridgeRuntimeStatus::Initializing,
            pl_core::StudioRuntimeStatus::Ready => BridgeRuntimeStatus::Ready,
            pl_core::StudioRuntimeStatus::ShuttingDown => BridgeRuntimeStatus::ShuttingDown,
            pl_core::StudioRuntimeStatus::Stopped => BridgeRuntimeStatus::Stopped,
            pl_core::StudioRuntimeStatus::Failed => BridgeRuntimeStatus::Failed,
        },
        active_turns: snapshot
            .active_turns
            .into_iter()
            .map(|turn| BridgeActiveTurn {
                session_id: turn.session_id,
                turn_id: turn.turn_id,
            })
            .collect(),
        updated_at: snapshot.updated_at,
        error: snapshot.error,
    }
}

impl From<StudioEventEnvelope> for BridgeEventEnvelope {
    fn from(event: StudioEventEnvelope) -> Self {
        let payload = bridge_event_payload(event.kind);
        Self {
            event_id: event.event_id,
            session_id: event.session_id,
            turn_id: event.turn_id,
            sequence: event.sequence,
            created_at: event.created_at,
            payload,
        }
    }
}

impl BridgeEventEnvelope {
    fn stale(session_id: Option<String>, lagged_events: u64) -> Self {
        Self {
            event_id: format!("bridge-stale-{}", unix_nanos_hex()),
            session_id,
            turn_id: None,
            sequence: 0,
            created_at: unix_seconds(),
            payload: BridgeEventPayload::Stale { lagged_events },
        }
    }
}

fn bridge_event_payload(kind: StudioEventKind) -> BridgeEventPayload {
    match kind {
        StudioEventKind::TurnChanged { turn } => BridgeEventPayload::TurnChanged {
            turn: bridge_turn(turn),
        },
        StudioEventKind::MessageUpdated { message } => BridgeEventPayload::MessageUpdated {
            message: bridge_message(*message),
        },
        StudioEventKind::MessageRemoved { message_id } => {
            BridgeEventPayload::MessageRemoved { message_id }
        }
        StudioEventKind::MessagePartUpdated { part } => BridgeEventPayload::MessagePartUpdated {
            part: Box::new(bridge_part(*part)),
        },
        StudioEventKind::MessagePartRemoved {
            message_id,
            part_id,
        } => BridgeEventPayload::MessagePartRemoved {
            message_id,
            part_id,
        },
        StudioEventKind::MessagePartDelta { delta } => BridgeEventPayload::MessagePartDelta {
            delta: bridge_part_delta(delta),
        },
        StudioEventKind::InteractionChanged { event } => BridgeEventPayload::InteractionChanged {
            event: bridge_interaction_changed(*event),
        },
        StudioEventKind::AgentChanged { agent } => BridgeEventPayload::AgentChanged {
            agent: Box::new(bridge_agent_snapshot(agent)),
        },
        StudioEventKind::AgentTimelineChanged { event } => {
            BridgeEventPayload::AgentTimelineChanged {
                event: bridge_agent_timeline_event(event),
            }
        }
        StudioEventKind::SessionRuntimeChanged { runtime } => {
            BridgeEventPayload::SessionRuntimeChanged {
                runtime: bridge_session_runtime(runtime),
            }
        }
        StudioEventKind::SkillActivated { activation } => BridgeEventPayload::SkillActivated {
            activation: bridge_skill_activation(activation),
        },
        StudioEventKind::PlanLifecycleChanged { event } => {
            BridgeEventPayload::PlanLifecycleChanged {
                event: BridgePlanLifecycleDto {
                    plan_id: event.plan_id,
                    state: event.state.as_str().to_string(),
                    turn_id: event.turn_id,
                    reason: event.reason,
                    updated_at: event.updated_at,
                },
            }
        }
        StudioEventKind::SessionHandoffChanged { .. } => BridgeEventPayload::SessionHandoffChanged,
        StudioEventKind::SessionListChanged {
            project_id,
            sessions,
        } => BridgeEventPayload::SessionListChanged {
            project_id,
            sessions: sessions.into_iter().map(session_summary_dto).collect(),
        },
        StudioEventKind::McpHealthChanged { health } => BridgeEventPayload::McpHealthChanged {
            health: bridge_mcp_health(health),
        },
        StudioEventKind::LspHealthChanged { health } => BridgeEventPayload::LspHealthChanged {
            health: bridge_lsp_health(health),
        },
        StudioEventKind::Stale { lagged_events } => BridgeEventPayload::Stale { lagged_events },
    }
}

fn bridge_turn(turn: StudioTurn) -> BridgeStudioTurnDto {
    BridgeStudioTurnDto {
        turn_id: turn.turn_id,
        session_id: turn.session_id,
        status: turn.status.as_str().to_string(),
        reason: turn.reason,
        updated_at: turn.updated_at,
    }
}

fn bridge_message(message: StudioMessage) -> BridgeStudioMessageDto {
    BridgeStudioMessageDto {
        message_id: message.message_id,
        session_id: message.session_id,
        turn_id: message.turn_id,
        role: message.role.as_str().to_string(),
        status: message.status.as_str().to_string(),
        created_at: message.created_at,
        updated_at: message.updated_at,
        completed_at: message.completed_at,
        error: message.error,
    }
}

fn bridge_part(part: StudioPart) -> BridgeStudioPartDto {
    BridgeStudioPartDto {
        part_id: part.part_id,
        message_id: part.message_id,
        session_id: part.session_id,
        turn_id: part.turn_id,
        part_type: part.part_type.as_str().to_string(),
        order: part.order,
        revision: part.revision,
        status: part.status.as_str().to_string(),
        created_at: part.created_at,
        updated_at: part.updated_at,
        completed_at: part.completed_at,
        error: part.error,
        text_channel: part
            .text_channel
            .map(|channel| channel.as_str().to_string()),
        text: part.text,
        tool: part.tool.map(|tool| BridgeStudioToolPartDto {
            tool_call_id: tool.tool_call_id,
            call_id: tool.call_id,
            provider_item_id: tool.provider_item_id,
            name: tool.name,
            arguments: tool.arguments,
            result: tool.result,
            exit_code: tool.exit_code,
            timed_out: tool.timed_out,
            working_directory: tool.working_directory,
            denial_reason: tool.denial_reason,
        }),
        agent: part.agent.map(|agent| BridgeStudioAgentPartDto {
            id: agent.id,
            path: agent.path,
            parent_path: agent.parent_path,
            role: agent.role,
            task: agent.task,
            status: agent.status.as_str().to_string(),
            summary: agent.summary,
            depth: agent.depth,
            error: agent.error,
            reason: agent.reason,
        }),
        plan: part.plan.map(|plan| BridgeStudioPlanPartDto {
            content: plan.content,
        }),
        synthetic: part.synthetic,
        ignored: part.ignored,
    }
}

fn bridge_part_delta(delta: StudioPartDelta) -> BridgeStudioPartDeltaDto {
    BridgeStudioPartDeltaDto {
        part_id: delta.part_id,
        revision: delta.revision,
        field: bridge_part_delta_field(delta.field),
        delta: delta.delta,
        chunk_index: delta.chunk_index,
    }
}

fn bridge_part_delta_field(field: StudioPartDeltaField) -> String {
    match field {
        StudioPartDeltaField::Text => "text".to_string(),
        StudioPartDeltaField::ReasoningSummary => "reasoning.summary".to_string(),
        StudioPartDeltaField::PlanContent => "planContent".to_string(),
        StudioPartDeltaField::ToolArguments => "tool.arguments".to_string(),
        StudioPartDeltaField::ToolResult => "tool.result".to_string(),
    }
}

fn bridge_interaction_changed(
    event: pl_protocol::InteractionChangedEvent,
) -> BridgeInteractionChangedDto {
    let interaction = event.interaction;
    BridgeInteractionChangedDto {
        interaction_id: interaction.interaction_id,
        kind: interaction.kind.as_str().to_string(),
        status: interaction.status.as_str().to_string(),
        session_id: interaction.scope.session_id,
        turn_id: interaction.scope.turn_id,
        item_id: interaction.scope.item_id,
        tool_id: interaction.scope.tool_id,
        agent_path: interaction.scope.agent_path,
        payload: bridge_interaction_payload(interaction.payload),
        created_at: interaction.created_at,
        updated_at: interaction.updated_at,
        resolved_at: interaction.resolved_at,
    }
}

fn bridge_interaction_payload(payload: InteractionPayload) -> BridgeInteractionPayloadDto {
    match payload {
        InteractionPayload::UserInput { questions } => BridgeInteractionPayloadDto::UserInput {
            questions: questions
                .into_iter()
                .map(|question| BridgeUserQuestionDto {
                    id: question.id,
                    header: question.header,
                    question: question.question,
                    is_other: question.is_other,
                    is_secret: question.is_secret,
                    options: question.options.map(|options| {
                        options
                            .into_iter()
                            .map(|option| BridgeUserQuestionOptionDto {
                                label: option.label,
                                description: option.description,
                            })
                            .collect()
                    }),
                })
                .collect(),
        },
        InteractionPayload::ToolApproval {
            name,
            arguments,
            working_directory,
            parent_agent_id,
        } => BridgeInteractionPayloadDto::ToolApproval {
            name,
            arguments_json: serde_json::to_string(&arguments).unwrap_or_else(|_| "{}".into()),
            working_directory,
            parent_agent_id,
        },
        InteractionPayload::PlanConfirmation { plan_id, content } => {
            BridgeInteractionPayloadDto::PlanConfirmation { plan_id, content }
        }
    }
}

fn bridge_agent_snapshot(agent: StudioAgentSnapshot) -> BridgeAgentSnapshotDto {
    BridgeAgentSnapshotDto {
        id: agent.id,
        session_id: agent.session_id,
        path: agent.path,
        parent_path: agent.parent_path,
        role: agent.role,
        task: agent.task,
        status: agent.status.as_str().to_string(),
        summary: agent.summary,
        depth: agent.depth,
        error: agent.error,
        reason: agent.reason,
        updated_at: agent.updated_at,
    }
}

fn bridge_agent_timeline_event(event: StudioAgentTimelineEvent) -> BridgeAgentTimelineEventDto {
    BridgeAgentTimelineEventDto {
        event_id: event.event_id,
        session_id: event.session_id,
        sequence: event.sequence,
        created_at: event.created_at,
        payload: bridge_agent_timeline_payload(event.kind),
    }
}

fn bridge_agent_timeline_payload(
    kind: StudioAgentTimelineEventKind,
) -> BridgeAgentTimelinePayloadDto {
    match kind {
        StudioAgentTimelineEventKind::SpawnBegin {
            call_id,
            sender_path,
            task_name,
            prompt,
            role,
            model,
            reasoning_effort,
        } => BridgeAgentTimelinePayloadDto::SpawnBegin {
            call_id,
            sender_path,
            task_name,
            prompt,
            role,
            model,
            reasoning_effort,
        },
        StudioAgentTimelineEventKind::SpawnEnd {
            call_id,
            sender_path,
            agent_id,
            path,
            role,
            status,
            prompt,
            error,
        } => BridgeAgentTimelinePayloadDto::SpawnEnd {
            call_id,
            sender_path,
            agent_id,
            path,
            role,
            status: status.as_str().to_string(),
            prompt,
            error,
        },
        StudioAgentTimelineEventKind::InteractionBegin {
            call_id,
            sender_path,
            receiver_path,
            prompt,
        } => BridgeAgentTimelinePayloadDto::InteractionBegin {
            call_id,
            sender_path,
            receiver_path,
            prompt,
        },
        StudioAgentTimelineEventKind::InteractionEnd {
            call_id,
            sender_path,
            receiver_path,
            status,
            prompt,
            error,
        } => BridgeAgentTimelinePayloadDto::InteractionEnd {
            call_id,
            sender_path,
            receiver_path,
            status: status.as_str().to_string(),
            prompt,
            error,
        },
        StudioAgentTimelineEventKind::WaitingBegin {
            call_id,
            sender_path,
        } => BridgeAgentTimelinePayloadDto::WaitingBegin {
            call_id,
            sender_path,
        },
        StudioAgentTimelineEventKind::WaitingEnd {
            call_id,
            sender_path,
            timed_out,
        } => BridgeAgentTimelinePayloadDto::WaitingEnd {
            call_id,
            sender_path,
            timed_out,
        },
        StudioAgentTimelineEventKind::CloseBegin {
            call_id,
            sender_path,
            receiver_path,
        } => BridgeAgentTimelinePayloadDto::CloseBegin {
            call_id,
            sender_path,
            receiver_path,
        },
        StudioAgentTimelineEventKind::CloseEnd {
            call_id,
            sender_path,
            receiver_path,
            status,
            error,
        } => BridgeAgentTimelinePayloadDto::CloseEnd {
            call_id,
            sender_path,
            receiver_path,
            status: status.as_str().to_string(),
            error,
        },
    }
}

fn bridge_session_runtime(runtime: StudioSessionRuntime) -> BridgeSessionRuntimeDto {
    BridgeSessionRuntimeDto {
        session_id: runtime.session_id,
        model: runtime.usage.model,
        context_window: runtime.usage.context_window,
        latest_context_tokens: runtime.usage.latest_context_tokens,
        prompt_tokens: runtime.usage.prompt_tokens,
        completion_tokens: runtime.usage.completion_tokens,
        cached_prompt_tokens: runtime.usage.cached_prompt_tokens,
        total_tokens: runtime.usage.total_tokens,
        estimated_costs: runtime
            .usage
            .estimated_costs
            .into_iter()
            .map(bridge_cost_amount)
            .collect(),
        has_unpriced_usage: runtime.usage.has_unpriced_usage,
        active_skills: runtime.active_skills,
        active_mcp_servers: runtime.active_mcp_servers,
        active_lsp_servers: runtime.active_lsp_servers,
        updated_at: runtime.updated_at,
    }
}

fn bridge_cost_amount(cost: RuntimeCostAmount) -> BridgeRuntimeCostAmountDto {
    BridgeRuntimeCostAmountDto {
        currency: cost.currency,
        amount: cost.amount,
    }
}

fn bridge_skill_activation(activation: SkillActivation) -> BridgeSkillActivationDto {
    BridgeSkillActivationDto {
        name: activation.name,
        source: activation.source,
        path: activation.path,
        turn_id: activation.turn_id,
        tool_call_id: activation.tool_call_id,
        activated_at: activation.activated_at,
    }
}

fn session_summary_dto(session: pl_protocol::StudioSessionSummary) -> SessionDto {
    SessionDto {
        id: session.id,
        project_id: session.project_id,
        title: session.title,
        mode: session.mode,
        updated_at: session.updated_at,
        visibility: session.visibility,
        parent_session_id: session.parent_session_id,
    }
}

fn bridge_mcp_health(health: StudioMcpHealth) -> BridgeMcpHealthDto {
    BridgeMcpHealthDto {
        active_mcp_servers: health.active_mcp_servers,
        mcp_servers: health
            .mcp_servers
            .into_iter()
            .map(|server| BridgeMcpServerDto {
                id: server.id,
                enabled: server.enabled,
                transport: server.transport,
                command: server.command,
                url: server.url,
                endpoint: server.endpoint,
                status_kind: server.status_kind,
                availability_kind: server.availability_kind,
            })
            .collect(),
    }
}

fn bridge_lsp_health(health: StudioLspHealth) -> BridgeLspHealthDto {
    BridgeLspHealthDto {
        active_lsp_servers: health.active_lsp_servers,
    }
}

fn studio_settings_draft_key(section: &str) -> String {
    format!("flutterSettingsDraft:{section}")
}

fn normalized_string_list(values: Vec<String>) -> Vec<String> {
    let mut values = values
        .into_iter()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>();
    values.sort();
    values.dedup();
    values
}

fn mcp_transport_from_label(label: &str) -> McpServerTransport {
    match label.trim() {
        "streamableHttp" | "streamable_http" | "http" => McpServerTransport::StreamableHttp,
        _ => McpServerTransport::Stdio,
    }
}

fn unix_nanos_hex() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| format!("{:x}", duration.as_nanos()))
        .unwrap_or_else(|_| "0".to_string())
}

fn unix_seconds() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use pl_core::McpServerTransport;
    use pl_protocol::StudioEventKind;
    use pretty_assertions::assert_eq;

    use super::{
        BridgeEventEnvelope, BridgeEventPayload, BridgeSessionStateResponse,
        BridgeStudioEventsResponse, BridgeStudioSnapshotResponse, ConfigSavedResponse,
        ProviderUsagesResponse, ResolveInteractionResponse, SettingsDraftResponse, SkillsResponse,
        StopPromptResponse, SubmitPromptResponse,
    };

    #[test]
    fn bridge_event_envelope_uses_typed_payload() {
        let event = pl_protocol::StudioEventEnvelope {
            event_id: "event-1".to_string(),
            project_id: None,
            session_id: Some("session-1".to_string()),
            turn_id: Some("turn-1".to_string()),
            sequence: 7,
            created_at: 10,
            kind: StudioEventKind::Stale { lagged_events: 2 },
        };

        let envelope = BridgeEventEnvelope::from(event);

        assert_eq!(envelope.session_id.as_deref(), Some("session-1"));
        assert_eq!(envelope.sequence, 7);
        assert_eq!(
            envelope.payload,
            BridgeEventPayload::Stale { lagged_events: 2 }
        );
    }

    #[test]
    fn bridge_filters_legacy_session_handoff_events() {
        let event = pl_protocol::StudioEventEnvelope {
            event_id: "event-1".to_string(),
            project_id: None,
            session_id: Some("session-1".to_string()),
            turn_id: Some("turn-1".to_string()),
            sequence: 7,
            created_at: 10,
            kind: StudioEventKind::SessionHandoffChanged {
                handoff: pl_protocol::StudioSessionHandoff {
                    origin_session_id: "session-1".to_string(),
                    target_session_id: "session-2".to_string(),
                    target_session: None,
                    kind: "planImplementation".to_string(),
                    status: "completed".to_string(),
                    plan_id: None,
                    updated_at: 10,
                },
            },
        };

        assert!(!super::bridge_visible_event(&event));
    }

    #[test]
    fn archive_project_api_is_exposed_to_flutter() {
        let _api: fn(String, Option<String>) -> anyhow::Result<BridgeStudioSnapshotResponse> =
            super::archive_project;
    }

    #[test]
    fn list_discovered_skills_api_is_exposed_to_flutter() {
        let _api: fn(String) -> anyhow::Result<SkillsResponse> = super::list_discovered_skills;
    }

    #[test]
    fn small_command_responses_are_typed_for_flutter() {
        let _runtime_permission: fn(String) -> anyhow::Result<ConfigSavedResponse> =
            super::save_runtime_permission_mode;
        let _provider_usages: fn() -> anyhow::Result<ProviderUsagesResponse> =
            super::load_provider_usages;
        let _submit: fn(String, String, Vec<String>) -> anyhow::Result<SubmitPromptResponse> =
            super::submit_prompt;
        let _stop: fn(String) -> anyhow::Result<StopPromptResponse> = super::stop_prompt;
        let _resolve: fn(String, String) -> anyhow::Result<ResolveInteractionResponse> =
            super::resolve_interaction;
        let _draft: fn(String, String) -> anyhow::Result<SettingsDraftResponse> =
            super::save_studio_settings_draft;
    }

    #[test]
    fn load_studio_events_api_returns_typed_bridge_events() {
        let _api: fn(
            String,
            Option<i64>,
            Option<i64>,
        ) -> anyhow::Result<BridgeStudioEventsResponse> = super::load_studio_events;
    }

    #[test]
    fn typed_settings_apis_are_exposed_to_flutter() {
        let _session: fn(String) -> anyhow::Result<BridgeSessionStateResponse> =
            super::load_session_state;
        let _instructions: fn(String) -> anyhow::Result<BridgeStudioSnapshotResponse> =
            super::save_instructions_settings;
        let _skills: fn(String) -> anyhow::Result<BridgeStudioSnapshotResponse> =
            super::save_skills_settings;
        let _mcp: fn(String) -> anyhow::Result<BridgeStudioSnapshotResponse> =
            super::save_mcp_settings;
        let _general: fn(String) -> anyhow::Result<BridgeStudioSnapshotResponse> =
            super::save_general_settings;
    }

    #[test]
    fn mcp_transport_label_accepts_ui_values() {
        assert_eq!(
            super::mcp_transport_from_label("streamableHttp"),
            McpServerTransport::StreamableHttp
        );
        assert_eq!(
            super::mcp_transport_from_label("streamable_http"),
            McpServerTransport::StreamableHttp
        );
        assert_eq!(
            super::mcp_transport_from_label("http"),
            McpServerTransport::StreamableHttp
        );
        assert_eq!(
            super::mcp_transport_from_label("stdio"),
            McpServerTransport::Stdio
        );
    }

    #[test]
    fn normalized_string_list_trims_sorts_and_deduplicates() {
        assert_eq!(
            super::normalized_string_list(vec![
                " beta ".to_string(),
                String::new(),
                "alpha".to_string(),
                "beta".to_string(),
            ]),
            vec!["alpha".to_string(), "beta".to_string()]
        );
    }
}

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
    AgentStatus, BudgetLimitKind, BudgetUsage, InteractionRequest, InteractionResolution,
    RuntimeUsageSnapshot, StudioEventEnvelope, StudioEventKind, StudioMessage, StudioPart,
};
use serde::{Deserialize, Serialize};

use crate::frb_generated::StreamSink;

static BRIDGE: OnceLock<BridgeRuntime> = OnceLock::new();

struct BridgeRuntime {
    tokio: tokio::runtime::Runtime,
    studio: StudioRuntime,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JsonResponse {
    pub json: String,
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BridgeEventEnvelope {
    pub event_id: String,
    pub session_id: Option<String>,
    pub turn_id: Option<String>,
    pub sequence: u64,
    pub created_at: i64,
    pub kind_type: String,
    pub payload_json: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct BootstrapResponse {
    projects: Vec<ProjectDto>,
    selected_project_id: Option<String>,
    sessions: Vec<SessionDto>,
    selected_session_id: Option<String>,
    agent_events: Vec<AgentTimelineEventDto>,
    agents: Vec<AgentDto>,
    interactions: Vec<InteractionRequest>,
    session_runtime: Option<SessionRuntimeDto>,
    config: serde_json::Value,
    general_settings: serde_json::Value,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ProjectDto {
    id: String,
    name: String,
    path: String,
    updated_at: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct SessionDto {
    id: String,
    project_id: String,
    title: String,
    mode: String,
    updated_at: i64,
    visibility: String,
    parent_session_id: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct AgentDto {
    id: String,
    session_id: String,
    path: String,
    parent_path: Option<String>,
    role: String,
    task: String,
    status: AgentStatus,
    summary: Option<String>,
    depth: i32,
    error: Option<String>,
    reason: Option<String>,
    budget_limit_kind: Option<BudgetLimitKind>,
    budget_usage: Option<BudgetUsage>,
    runtime_usage: Option<RuntimeUsageSnapshot>,
    updated_at: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct AgentTimelineEventDto {
    event_id: String,
    session_id: String,
    sequence: i64,
    kind: String,
    agent_id: Option<String>,
    path: Option<String>,
    parent_path: Option<String>,
    payload_json: String,
    created_at: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct SessionRuntimeDto {
    session_id: String,
    model: String,
    context_window: Option<u64>,
    latest_context_tokens: u64,
    prompt_tokens: u64,
    completion_tokens: u64,
    cached_prompt_tokens: u64,
    total_tokens: u64,
    estimated_costs: Vec<pl_protocol::RuntimeCostAmount>,
    has_unpriced_usage: bool,
    active_skills: Vec<String>,
    active_mcp_servers: Vec<String>,
    active_lsp_servers: Vec<String>,
    updated_at: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct StudioEventsResponse {
    session_id: String,
    events: Vec<StudioEventEnvelope>,
    next_sequence: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct SessionStateResponse {
    session_id: String,
    session: SessionDto,
    sessions: Vec<SessionDto>,
    messages: Vec<StudioMessageProjectionDto>,
    parts: Vec<StudioPartProjectionDto>,
    events: Vec<StudioEventEnvelope>,
    event_next_sequence: u64,
    agents: Vec<AgentDto>,
    agent_events: Vec<AgentTimelineEventDto>,
    interactions: Vec<InteractionRequest>,
    session_runtime: Option<SessionRuntimeDto>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct StudioMessageProjectionDto {
    message: StudioMessage,
    sequence: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct StudioPartProjectionDto {
    part: StudioPart,
    sequence: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct SubmitPromptResponse {
    session_id: String,
    turn_id: String,
    cursor: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct StopPromptResponse {
    session_id: String,
    stopped: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ResolveInteractionResponse {
    session_id: String,
    interaction: InteractionRequest,
    sessions: Vec<SessionDto>,
}

/// Provider 用量查询返回体。
///
/// 与 Studio provider usage wire 格式保持 camelCase，供 Flutter 列表卡片渲染。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ProviderUsagesResponse {
    usages: Vec<ProviderUsageDto>,
}

/// 单个 Provider 的用量状态。
///
/// status/usage_kind 是 Dart 层路由字段，复杂 provider payload 保持结构化字段。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ProviderUsageDto {
    provider_id: String,
    updated_at: i64,
    status: String,
    usage_kind: String,
    message: Option<String>,
    balance: Option<DeepSeekBalanceDto>,
    coding_plan: Option<ZhipuCodingPlanUsageDto>,
}

/// DeepSeek 余额用量。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct DeepSeekBalanceDto {
    is_available: bool,
    balances: Vec<DeepSeekBalanceInfoDto>,
}

/// DeepSeek 单币种余额明细。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct DeepSeekBalanceInfoDto {
    currency: String,
    total_balance: String,
    granted_balance: String,
    topped_up_balance: String,
}

/// 智谱 Coding Plan 用量。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ZhipuCodingPlanUsageDto {
    level: Option<String>,
    limits: Vec<ZhipuQuotaLimitDto>,
}

/// 智谱 Coding Plan 单个时间窗口额度。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ZhipuQuotaLimitDto {
    window: String,
    label: String,
    percentage: f64,
    current_value: Option<f64>,
    total: Option<f64>,
    remaining: Option<f64>,
    next_reset_at: Option<i64>,
    usage_details: Vec<ZhipuToolUsageDetailDto>,
}

/// 智谱 Coding Plan 工具级用量明细。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ZhipuToolUsageDetailDto {
    name: String,
    current_value: Option<f64>,
    total: Option<f64>,
    percentage: Option<f64>,
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

pub fn bootstrap_studio() -> Result<JsonResponse> {
    let bridge = bridge()?;
    bridge.block_on(async { json_response(bootstrap_studio_inner(bridge).await?) })
}

pub fn open_project(path: String) -> Result<JsonResponse> {
    let bridge = bridge()?;
    bridge.block_on(async {
        let project = bridge.studio.open_project(path).await?;
        bridge
            .studio
            .reconcile_lsp_runtime_for_project(&project.id)
            .await?;
        let _ = bridge.studio.ensure_project_sessions(&project.id).await?;
        json_response(studio_snapshot_inner(bridge, Some(project.id), None).await?)
    })
}

pub fn select_project(project_id: String) -> Result<JsonResponse> {
    let bridge = bridge()?;
    bridge.block_on(async {
        json_response(studio_snapshot_inner(bridge, Some(project_id), None).await?)
    })
}

pub fn archive_project(
    project_id: String,
    selected_project_id: Option<String>,
) -> Result<JsonResponse> {
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
        json_response(
            studio_snapshot_from_projects_inner(bridge, projects, next_project_id, None).await?,
        )
    })
}

pub fn create_session(project_id: String, title: Option<String>) -> Result<JsonResponse> {
    let bridge = bridge()?;
    bridge.block_on(async {
        let title = title
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| "新会话".to_string());
        let session = bridge.studio.create_session(&project_id, &title).await?;
        json_response(studio_snapshot_inner(bridge, Some(project_id), Some(session.id)).await?)
    })
}

pub fn archive_session(
    session_id: String,
    selected_session_id: Option<String>,
) -> Result<JsonResponse> {
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
        json_response(
            studio_snapshot_inner(bridge, Some(archived.project_id), next_session_id).await?,
        )
    })
}

pub fn set_session_mode(session_id: String, mode: String) -> Result<JsonResponse> {
    let bridge = bridge()?;
    bridge.block_on(async {
        bridge
            .studio
            .set_session_mode(&session_id, CompileMode::from_label(&mode))
            .await?;
        json_response(load_session_state_inner(bridge, session_id).await?)
    })
}

pub fn set_model_role(
    role_key: String,
    provider_id: String,
    model: String,
    effort: Option<String>,
    selected_session_id: Option<String>,
) -> Result<JsonResponse> {
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
        json_response(
            studio_snapshot_inner(bridge, selected_project_id, selected_session_id).await?,
        )
    })
}

pub fn save_runtime_permission_mode(mode: String) -> Result<JsonResponse> {
    let bridge = bridge()?;
    bridge.block_on(async {
        let mut config = bridge.studio.config_store().load_or_default()?;
        config.runtime.permission_mode = PermissionMode::from_label(&mode);
        bridge.studio.config_store().save(&config)?;
        json_response(serde_json::json!({ "config": config }))
    })
}

pub fn save_provider_settings(settings_json: String) -> Result<JsonResponse> {
    let bridge = bridge()?;
    bridge.block_on(async {
        let input: ProviderSettingsInput =
            serde_json::from_str(&settings_json).context("invalid provider settings json")?;
        let current = bridge.studio.config_store().load_or_default()?;
        let next = provider_settings_edit(input, &current)?.to_config(&current)?;
        bridge.studio.config_store().save(&next)?;
        json_response(studio_snapshot_inner(bridge, None, None).await?)
    })
}

pub fn save_instructions_settings(settings_json: String) -> Result<JsonResponse> {
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
        json_response(studio_snapshot_inner(bridge, None, None).await?)
    })
}

pub fn save_skills_settings(settings_json: String) -> Result<JsonResponse> {
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
        json_response(studio_snapshot_inner(bridge, None, None).await?)
    })
}

pub fn save_mcp_settings(settings_json: String) -> Result<JsonResponse> {
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
        json_response(studio_snapshot_inner(bridge, None, None).await?)
    })
}

pub fn save_general_settings(settings_json: String) -> Result<JsonResponse> {
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
        json_response(studio_snapshot_inner(bridge, None, None).await?)
    })
}

pub fn load_provider_usages() -> Result<JsonResponse> {
    let bridge = bridge()?;
    bridge.block_on(async {
        let usages = bridge
            .studio
            .provider_usages()
            .await?
            .into_iter()
            .map(provider_usage_dto)
            .collect();
        json_response(ProviderUsagesResponse { usages })
    })
}

pub fn list_discovered_skills(project_id: String) -> Result<JsonResponse> {
    let bridge = bridge()?;
    bridge.block_on(async {
        let catalog = bridge.studio.discovered_skills(&project_id).await?;
        json_response(catalog)
    })
}

pub fn save_studio_settings_draft(section: String, draft_json: String) -> Result<JsonResponse> {
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
        json_response(serde_json::json!({
            "section": section,
            "saved": true
        }))
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
) -> Result<JsonResponse> {
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
        json_response(SubmitPromptResponse {
            session_id: response.session_id,
            turn_id: response.turn_id,
            cursor: response.cursor,
        })
    })
}

pub fn stop_prompt(session_id: String) -> Result<JsonResponse> {
    let bridge = bridge()?;
    bridge.block_on(async {
        let response = bridge.studio.stop_prompt(session_id).await?;
        json_response(StopPromptResponse {
            session_id: response.session_id,
            stopped: response.stopped,
        })
    })
}

pub fn resolve_interaction(
    interaction_id: String,
    resolution_json: String,
) -> Result<JsonResponse> {
    let bridge = bridge()?;
    bridge.block_on(async {
        let resolution: InteractionResolution = serde_json::from_str(&resolution_json)
            .context("invalid interaction resolution json")?;
        let response = bridge
            .studio
            .resolve_interaction(interaction_id, resolution)
            .await?;
        json_response(resolve_interaction_response(response))
    })
}

pub fn load_session_state(session_id: String) -> Result<JsonResponse> {
    let bridge = bridge()?;
    bridge.block_on(async { json_response(load_session_state_inner(bridge, session_id).await?) })
}

pub fn load_studio_events(
    session_id: String,
    after_sequence: Option<i64>,
    limit: Option<i64>,
) -> Result<JsonResponse> {
    let bridge = bridge()?;
    bridge.block_on(async {
        let events = bridge
            .studio
            .store()
            .load_studio_events(&session_id, after_sequence, limit)
            .await?;
        let next_sequence = bridge
            .studio
            .store()
            .next_studio_event_sequence(&session_id)
            .await? as u64;
        json_response(StudioEventsResponse {
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

async fn bootstrap_studio_inner(bridge: &'static BridgeRuntime) -> Result<BootstrapResponse> {
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
) -> Result<BootstrapResponse> {
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
) -> Result<BootstrapResponse> {
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
        Some(session_id) => Some(session_runtime_dto(bridge, session_id).await?),
        None => None,
    };
    let config = serde_json::to_value(bridge.studio.config_store().load_or_default()?)?;
    let general_settings = bridge
        .studio
        .store()
        .load_setting("flutterSettings:general")
        .await?
        .and_then(|value| serde_json::from_str::<serde_json::Value>(&value).ok())
        .unwrap_or_else(|| serde_json::json!({}));

    Ok(BootstrapResponse {
        projects: projects.into_iter().map(project_dto).collect(),
        selected_project_id,
        sessions: sessions.into_iter().map(session_dto).collect(),
        selected_session_id,
        agent_events: agent_events.into_iter().map(agent_event_dto).collect(),
        agents: agents.into_iter().map(agent_dto).collect(),
        interactions,
        session_runtime,
        config,
        general_settings,
    })
}

async fn load_session_state_inner(
    bridge: &'static BridgeRuntime,
    session_id: String,
) -> Result<SessionStateResponse> {
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
        .filter(is_session_state_event)
        .collect();
    let messages = bridge
        .studio
        .store()
        .load_studio_messages(&session_id)
        .await?
        .into_iter()
        .map(|record| StudioMessageProjectionDto {
            message: record.message,
            sequence: record.sequence.max(0) as u64,
        })
        .collect();
    let parts = bridge
        .studio
        .store()
        .load_message_parts(&session_id)
        .await?
        .into_iter()
        .map(|record| StudioPartProjectionDto {
            part: record.part,
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
        .map(agent_dto)
        .collect();
    let agent_events = bridge
        .studio
        .store()
        .list_agent_events(&session_id)
        .await?
        .into_iter()
        .map(agent_event_dto)
        .collect();
    let interactions = bridge
        .studio
        .store()
        .list_pending_interactions(&session_id)
        .await?;
    let session_runtime = session_runtime_dto(bridge, &session_id).await.ok();

    Ok(SessionStateResponse {
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
        | StudioEventKind::Stale { .. } => false,
        StudioEventKind::TurnChanged { .. }
        | StudioEventKind::InteractionChanged { .. }
        | StudioEventKind::PlanLifecycleChanged { .. }
        | StudioEventKind::SessionRuntimeChanged { .. }
        | StudioEventKind::AgentChanged { .. }
        | StudioEventKind::AgentTimelineChanged { .. }
        | StudioEventKind::SkillActivated { .. }
        | StudioEventKind::SessionHandoffChanged { .. }
        | StudioEventKind::SessionListChanged { .. }
        | StudioEventKind::McpHealthChanged { .. }
        | StudioEventKind::LspHealthChanged { .. } => true,
    }
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

fn agent_dto(agent: pl_core::StudioAgentSnapshotRecord) -> AgentDto {
    AgentDto {
        id: agent.id,
        session_id: agent.session_id,
        path: agent.path,
        parent_path: agent.parent_path,
        role: agent.role,
        task: agent.task,
        status: agent.status,
        summary: agent.summary,
        depth: agent.depth,
        error: agent.error,
        reason: agent.reason,
        budget_limit_kind: agent.budget_limit_kind,
        budget_usage: agent.budget_usage,
        runtime_usage: agent.runtime_usage,
        updated_at: agent.updated_at,
    }
}

fn agent_event_dto(event: pl_core::StudioAgentTimelineEventRecord) -> AgentTimelineEventDto {
    AgentTimelineEventDto {
        event_id: event.event_id,
        session_id: event.session_id,
        sequence: event.sequence,
        kind: event.kind,
        agent_id: event.agent_id,
        path: event.path,
        parent_path: event.parent_path,
        payload_json: event.payload_json,
        created_at: event.created_at,
    }
}

async fn session_runtime_dto(
    bridge: &'static BridgeRuntime,
    session_id: &str,
) -> Result<SessionRuntimeDto> {
    let runtime = bridge.studio.session_runtime(session_id).await?;
    let active_skills = bridge
        .studio
        .store()
        .list_session_skill_names(session_id)
        .await?;
    Ok(SessionRuntimeDto {
        session_id: runtime.session_id,
        model: runtime.model,
        context_window: runtime.context_window,
        latest_context_tokens: runtime.latest_context_tokens,
        prompt_tokens: runtime.prompt_tokens,
        completion_tokens: runtime.completion_tokens,
        cached_prompt_tokens: runtime.cached_prompt_tokens,
        total_tokens: runtime.total_tokens,
        estimated_costs: runtime.estimated_costs,
        has_unpriced_usage: runtime.has_unpriced_usage,
        active_skills,
        active_mcp_servers: bridge.studio.mcp_runtime().available_server_names().await,
        active_lsp_servers: bridge.studio.lsp_runtime().active_server_names().await,
        updated_at: runtime.updated_at,
    })
}

fn resolve_interaction_response(
    response: CoreResolveInteractionResponse,
) -> ResolveInteractionResponse {
    ResolveInteractionResponse {
        session_id: response.session_id,
        interaction: response.interaction,
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
        let kind_type = serde_json::to_value(&event.kind)
            .ok()
            .and_then(|value| {
                value
                    .get("type")
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_string)
            })
            .unwrap_or_else(|| "unknown".to_string());
        let payload_json = serde_json::to_string(&event.kind).unwrap_or_else(|_| "{}".to_string());
        Self {
            event_id: event.event_id,
            session_id: event.session_id,
            turn_id: event.turn_id,
            sequence: event.sequence,
            created_at: event.created_at,
            kind_type,
            payload_json,
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
            kind_type: "stale".to_string(),
            payload_json: serde_json::json!({
                "type": "stale",
                "laggedEvents": lagged_events
            })
            .to_string(),
        }
    }
}

fn json_response(value: impl Serialize) -> Result<JsonResponse> {
    Ok(JsonResponse {
        json: serde_json::to_string(&value)?,
    })
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

    use super::{BridgeEventEnvelope, JsonResponse};

    #[test]
    fn bridge_event_envelope_uses_kind_type_and_payload_json() {
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

        assert_eq!(envelope.kind_type, "stale");
        assert_eq!(envelope.session_id.as_deref(), Some("session-1"));
        assert_eq!(envelope.sequence, 7);
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&envelope.payload_json).unwrap(),
            serde_json::json!({
                "type": "stale",
                "laggedEvents": 2
            })
        );
    }

    #[test]
    fn archive_project_api_is_exposed_to_flutter() {
        let _api: fn(String, Option<String>) -> anyhow::Result<JsonResponse> =
            super::archive_project;
    }

    #[test]
    fn list_discovered_skills_api_is_exposed_to_flutter() {
        let _api: fn(String) -> anyhow::Result<JsonResponse> = super::list_discovered_skills;
    }

    #[test]
    fn typed_settings_apis_are_exposed_to_flutter() {
        let _instructions: fn(String) -> anyhow::Result<JsonResponse> =
            super::save_instructions_settings;
        let _skills: fn(String) -> anyhow::Result<JsonResponse> = super::save_skills_settings;
        let _mcp: fn(String) -> anyhow::Result<JsonResponse> = super::save_mcp_settings;
        let _general: fn(String) -> anyhow::Result<JsonResponse> = super::save_general_settings;
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

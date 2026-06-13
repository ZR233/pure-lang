use pl_core::TimelineEventRecord;
use pl_protocol::{InteractionRequest, StudioEventEnvelope};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectDto {
    pub id: String,
    pub name: String,
    pub path: String,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionDto {
    pub id: String,
    pub project_id: String,
    pub title: String,
    pub mode: String,
    pub updated_at: i64,
    pub visibility: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentEventDto {
    pub event_id: String,
    pub session_id: String,
    pub sequence: i64,
    pub kind: String,
    pub agent_id: Option<String>,
    pub path: Option<String>,
    pub parent_path: Option<String>,
    pub payload: serde_json::Value,
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentDto {
    pub id: String,
    pub session_id: String,
    pub path: String,
    pub parent_path: Option<String>,
    pub role: String,
    pub task: String,
    pub status: String,
    pub summary: Option<String>,
    pub depth: i32,
    pub error: Option<String>,
    pub reason: Option<String>,
    pub budget_limit_kind: Option<String>,
    pub budget_usage: Option<BudgetUsageDto>,
    pub runtime_usage: Option<RuntimeUsageDto>,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BudgetUsageDto {
    pub model_steps: u32,
    pub tool_calls: u32,
    pub wait_calls: u32,
    pub elapsed_ms: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderDto {
    pub id: String,
    pub template_kind: String,
    pub name: String,
    pub subtitle: String,
    pub status: String,
    pub base_url: String,
    pub bearer_token: String,
    pub has_bearer_token: bool,
    pub default_model: String,
    pub model_count: String,
    pub updated_at: String,
    pub provider_kind: String,
    pub models: Vec<ModelDto>,
    pub default_models: Vec<ModelDto>,
    pub custom_models: Vec<ModelDto>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderUsagesDto {
    pub usages: Vec<ProviderUsageDto>,
}

#[derive(Debug, Clone, Serialize)]
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

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeepSeekBalanceDto {
    pub is_available: bool,
    pub balances: Vec<DeepSeekBalanceInfoDto>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeepSeekBalanceInfoDto {
    pub currency: String,
    pub total_balance: String,
    pub granted_balance: String,
    pub topped_up_balance: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ZhipuCodingPlanUsageDto {
    pub level: Option<String>,
    pub limits: Vec<ZhipuQuotaLimitDto>,
}

#[derive(Debug, Clone, Serialize)]
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

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ZhipuToolUsageDetailDto {
    pub name: String,
    pub current_value: Option<f64>,
    pub total: Option<f64>,
    pub percentage: Option<f64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderTemplateDto {
    pub id: String,
    pub name: String,
    pub base_url: String,
    pub default_model: String,
    pub provider_kind: String,
    pub default_models: Vec<ModelDto>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RoleDto {
    pub key: String,
    pub display_name: String,
    pub provider: String,
    pub model: String,
    pub effort: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelDto {
    pub slug: String,
    pub display_name: String,
    pub description: Option<String>,
    pub context_window: Option<u64>,
    pub max_context_window: Option<u64>,
    pub auto_compact_token_limit: Option<u64>,
    pub default_temperature: Option<f32>,
    pub max_output_tokens: Option<u64>,
    pub currency: Option<String>,
    #[serde(rename = "inputPricePerMTok")]
    pub input_price_per_mtok: Option<f64>,
    #[serde(rename = "outputPricePerMTok")]
    pub output_price_per_mtok: Option<f64>,
    #[serde(rename = "cacheReadPricePerMTok")]
    pub cache_read_price_per_mtok: Option<f64>,
    pub reasoning_efforts: Vec<String>,
    pub capabilities: ModelCapabilitiesDto,
    pub truncation_mode: String,
    pub truncation_limit: u64,
    pub base_instructions: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelCapabilitiesDto {
    pub streaming: bool,
    pub temperature: bool,
    pub reasoning: bool,
    pub web_search: bool,
    pub input: Vec<String>,
    pub output: Vec<String>,
    pub tools: ToolCapabilitiesDto,
    pub interleaved: Option<ReasoningInterleavedDto>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolCapabilitiesDto {
    pub function_calling: bool,
    pub parallel_tool_calls: bool,
    pub custom_tools: bool,
    pub freeform_tools: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReasoningInterleavedDto {
    pub field: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AttachmentDto {
    pub id: String,
    pub session_id: String,
    pub media_type: String,
    pub filename: Option<String>,
    pub byte_size: u64,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub created_at: i64,
    pub data_url: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigDto {
    pub toml: String,
    pub permission_mode: String,
    pub instructions: InstructionsDto,
    pub providers: Vec<ProviderDto>,
    pub roles: Vec<RoleDto>,
    pub templates: Vec<ProviderTemplateDto>,
    pub mcp_servers: Vec<McpServerDto>,
    pub config_exists: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstructionsDto {
    pub base_override: String,
    pub developer: String,
    pub user: String,
    pub project_doc_max_bytes: usize,
    pub project_doc_fallback_filenames: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct McpServerDto {
    pub id: String,
    pub enabled: bool,
    pub transport: String,
    pub command: Option<String>,
    pub args: Vec<String>,
    pub env: Vec<KeyValueDto>,
    pub cwd: Option<String>,
    pub url: Option<String>,
    pub bearer_token_env_var: Option<String>,
    pub headers: Vec<KeyValueDto>,
    pub endpoint: String,
    pub source_kind: String,
    pub source_label: String,
    pub source_detail: Option<String>,
    pub status_kind: String,
    pub status_message: Option<String>,
    pub mutation_policy: String,
    pub availability_kind: String,
    pub availability_message: Option<String>,
    pub last_checked_at: Option<i64>,
    pub tool_count: Option<usize>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct McpHealthUpdateDto {
    pub mcp_servers: Vec<McpServerDto>,
    pub active_mcp_servers: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LspServerDto {
    pub id: String,
    pub display_name: String,
    pub extensions: Vec<String>,
    pub language_ids: Vec<String>,
    pub availability_kind: String,
    pub availability_message: Option<String>,
    pub last_checked_at: Option<i64>,
    pub diagnostic_count: usize,
    pub activity_kind: String,
    pub activity_title: Option<String>,
    pub activity_message: Option<String>,
    pub activity_percentage: Option<u32>,
    pub last_error: Option<String>,
    pub last_error_at: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LspHealthUpdateDto {
    pub lsp_servers: Vec<LspServerDto>,
    pub active_lsp_servers: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KeyValueDto {
    pub key: String,
    pub value: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillDto {
    pub name: String,
    pub description: String,
    pub category: Option<String>,
    pub platforms: Vec<String>,
    pub scope: String,
    pub path: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiscoveredSkillsDto {
    pub project_dir: String,
    pub skills: Vec<SkillDto>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionRuntimeDto {
    pub session_id: String,
    pub usage: RuntimeUsageDto,
    pub active_skills: Vec<String>,
    pub active_mcp_servers: Vec<String>,
    pub active_lsp_servers: Vec<String>,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeUsageDto {
    pub model: String,
    pub context_window: Option<u64>,
    pub latest_context_tokens: u64,
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub cached_prompt_tokens: u64,
    pub total_tokens: u64,
    pub cache_hit_rate: Option<f64>,
    pub estimated_costs: Vec<RuntimeCostAmountDto>,
    pub has_unpriced_usage: bool,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeCostAmountDto {
    pub currency: String,
    pub amount: f64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderSettingsInput {
    pub default_provider_id: Option<String>,
    pub providers: Vec<ProviderInput>,
    #[serde(default)]
    pub roles: Vec<RoleInput>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstructionsInput {
    pub base_override: String,
    pub developer: String,
    pub user: String,
    pub project_doc_max_bytes: usize,
    pub project_doc_fallback_filenames: Vec<String>,
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
    pub custom_models: Vec<ModelInput>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelInput {
    pub slug: String,
    pub display_name: String,
    pub reasoning_efforts: Vec<String>,
    #[serde(default)]
    pub base_instructions: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RoleInput {
    pub key: String,
    pub provider: String,
    pub model: String,
    pub effort: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BootstrapDto {
    pub projects: Vec<ProjectDto>,
    pub selected_project_id: Option<String>,
    pub sessions: Vec<SessionDto>,
    pub selected_session_id: Option<String>,
    pub agent_events: Vec<AgentEventDto>,
    pub agents: Vec<AgentDto>,
    pub session_runtime: Option<SessionRuntimeDto>,
    pub interactions: Vec<InteractionRequest>,
    pub lsp_health: LspHealthUpdateDto,
    pub config: ConfigDto,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectSelectionDto {
    pub selected_project_id: Option<String>,
    pub projects: Vec<ProjectDto>,
    pub sessions: Vec<SessionDto>,
    pub selected_session_id: Option<String>,
    pub agent_events: Vec<AgentEventDto>,
    pub agents: Vec<AgentDto>,
    pub session_runtime: Option<SessionRuntimeDto>,
    pub interactions: Vec<InteractionRequest>,
    pub lsp_health: LspHealthUpdateDto,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionSelectionDto {
    pub session_id: String,
    pub sessions: Vec<SessionDto>,
    pub agent_events: Vec<AgentEventDto>,
    pub agents: Vec<AgentDto>,
    pub session_runtime: Option<SessionRuntimeDto>,
    pub interactions: Vec<InteractionRequest>,
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
    pub command: Option<String>,
    pub args: Vec<String>,
    pub env: Vec<KeyValueDto>,
    pub cwd: Option<String>,
    pub url: Option<String>,
    pub bearer_token_env_var: Option<String>,
    pub headers: Vec<KeyValueDto>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanStateDto {
    pub plan_id: String,
    pub state: String,
    pub turn_id: Option<String>,
    pub reason: Option<String>,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanLifecycleResponse {
    pub session_id: String,
    pub plan_states: Vec<PlanStateDto>,
    pub timeline_next_sequence: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TimelineEventDto {
    pub id: String,
    pub session_id: String,
    pub sequence: i64,
    pub created_at: i64,
    pub kind: String,
    pub payload: serde_json::Value,
}

impl From<TimelineEventRecord> for TimelineEventDto {
    fn from(record: TimelineEventRecord) -> Self {
        Self {
            id: record.id,
            session_id: record.session_id,
            sequence: record.sequence,
            created_at: record.created_at,
            kind: record.kind,
            payload: serde_json::from_str(&record.payload_json).unwrap_or(serde_json::Value::Null),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunPromptResponse {
    pub session_id: String,
    pub sessions: Vec<SessionDto>,
    pub agent_events: Vec<AgentEventDto>,
    pub agents: Vec<AgentDto>,
    pub session_runtime: SessionRuntimeDto,
    pub timeline_events: Vec<TimelineEventDto>,
    pub plan_states: Vec<PlanStateDto>,
    pub interactions: Vec<InteractionRequest>,
    pub timeline_next_sequence: u64,
    pub turn_status: String,
    pub turn_abort_reason: Option<String>,
    pub turn_error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SubmitPromptResponse {
    pub session_id: String,
    pub turn_id: String,
    pub cursor: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StopPromptResponse {
    pub session_id: String,
    pub stopped: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionTimelineDto {
    pub session_id: String,
    pub events: Vec<TimelineEventDto>,
    pub plan_states: Vec<PlanStateDto>,
    pub interactions: Vec<InteractionRequest>,
    pub next_sequence: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StudioEventsDto {
    pub session_id: String,
    pub events: Vec<StudioEventEnvelope>,
    pub next_sequence: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionStateDto {
    pub session_id: String,
    pub session: SessionDto,
    pub sessions: Vec<SessionDto>,
    pub agent_events: Vec<AgentEventDto>,
    pub agents: Vec<AgentDto>,
    pub session_runtime: SessionRuntimeDto,
    pub interactions: Vec<InteractionRequest>,
    pub timeline: SessionTimelineDto,
    pub events: Vec<StudioEventEnvelope>,
    pub event_next_sequence: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolveInteractionResponse {
    pub session_id: String,
    pub interaction: InteractionRequest,
    pub plan_lifecycle: Option<PlanLifecycleResponse>,
}

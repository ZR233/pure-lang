use pl_protocol::{AgentEvent, TimelineItem, UserQuestion};
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
    pub capabilities: Vec<String>,
    pub input_modalities: Vec<String>,
    pub truncation_mode: String,
    pub truncation_limit: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigDto {
    pub toml: String,
    pub permission_mode: String,
    pub providers: Vec<ProviderDto>,
    pub roles: Vec<RoleDto>,
    pub templates: Vec<ProviderTemplateDto>,
    pub mcp_servers: Vec<McpServerDto>,
    pub config_exists: bool,
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
    pub config: ConfigDto,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectSelectionDto {
    pub project_id: String,
    pub projects: Vec<ProjectDto>,
    pub sessions: Vec<SessionDto>,
    pub selected_session_id: Option<String>,
    pub agent_events: Vec<AgentEventDto>,
    pub agents: Vec<AgentDto>,
    pub session_runtime: Option<SessionRuntimeDto>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionSelectionDto {
    pub session_id: String,
    pub sessions: Vec<SessionDto>,
    pub agent_events: Vec<AgentEventDto>,
    pub agents: Vec<AgentDto>,
    pub session_runtime: Option<SessionRuntimeDto>,
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
pub struct RunPromptResponse {
    pub session_id: String,
    pub sessions: Vec<SessionDto>,
    pub agent_events: Vec<AgentEventDto>,
    pub agents: Vec<AgentDto>,
    pub session_runtime: SessionRuntimeDto,
    pub timeline_items: Vec<TimelineItem>,
    pub plan_states: Vec<PlanStateDto>,
    pub timeline_next_sequence: u64,
    pub turn_status: String,
    pub turn_abort_reason: Option<String>,
    pub turn_error: Option<String>,
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
    pub items: Vec<TimelineItem>,
    pub plan_states: Vec<PlanStateDto>,
    pub next_sequence: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentEventPayload {
    pub session_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeline_stale: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lagged_events: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub event: Option<AgentEvent>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeline_event: Option<AgentEventDto>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent: Option<AgentDto>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_runtime: Option<SessionRuntimeDto>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolApprovalRequestPayload {
    pub approval_id: String,
    pub session_id: String,
    pub name: String,
    pub arguments: serde_json::Value,
    pub working_directory: Option<String>,
    pub parent_agent_id: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolApprovalResolvedPayload {
    pub approval_id: String,
    pub decision: String,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UserInputRequestPayload {
    pub request_id: String,
    pub session_id: String,
    pub tool_id: String,
    pub questions: Vec<UserQuestion>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UserInputResolvedPayload {
    pub request_id: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PromptFailedPayload {
    pub session_id: Option<String>,
    pub message: String,
}

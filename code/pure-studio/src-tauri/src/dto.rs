use std::collections::HashMap;

use pl_protocol::AgentEvent;
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
pub struct MessageDto {
    pub role: String,
    pub content: String,
    pub reasoning_content: Option<String>,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub metadata: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SubagentEventDto {
    pub event_id: String,
    pub id: String,
    pub parent_id: Option<String>,
    pub role: String,
    pub task: String,
    pub status: String,
    pub summary: Option<String>,
    pub depth: i32,
    pub error: Option<String>,
    pub updated_at: i64,
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
    pub default_model: String,
    pub model_count: String,
    pub updated_at: String,
    pub wire_api: String,
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
    pub wire_api: String,
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
    pub providers: Vec<ProviderDto>,
    pub roles: Vec<RoleDto>,
    pub templates: Vec<ProviderTemplateDto>,
    pub config_exists: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionRuntimeDto {
    pub session_id: String,
    pub model: String,
    pub context_window: Option<u64>,
    pub latest_context_tokens: u64,
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub cached_prompt_tokens: u64,
    pub total_tokens: u64,
    pub cache_hit_rate: Option<f64>,
    pub currency: Option<String>,
    #[serde(rename = "inputPricePerMTok")]
    pub input_price_per_mtok: Option<f64>,
    #[serde(rename = "outputPricePerMTok")]
    pub output_price_per_mtok: Option<f64>,
    #[serde(rename = "cacheReadPricePerMTok")]
    pub cache_read_price_per_mtok: Option<f64>,
    pub estimated_cost: Option<f64>,
    pub active_skills: Vec<String>,
    pub active_mcp_servers: Vec<String>,
    pub updated_at: i64,
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
    pub wire_api: String,
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
    pub messages: Vec<MessageDto>,
    pub subagent_events: Vec<SubagentEventDto>,
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
    pub messages: Vec<MessageDto>,
    pub subagent_events: Vec<SubagentEventDto>,
    pub session_runtime: Option<SessionRuntimeDto>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionSelectionDto {
    pub session_id: String,
    pub sessions: Vec<SessionDto>,
    pub messages: Vec<MessageDto>,
    pub subagent_events: Vec<SubagentEventDto>,
    pub session_runtime: Option<SessionRuntimeDto>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunPromptResponse {
    pub session_id: String,
    pub messages: Vec<MessageDto>,
    pub sessions: Vec<SessionDto>,
    pub subagent_events: Vec<SubagentEventDto>,
    pub session_runtime: SessionRuntimeDto,
    pub timeline_items: Vec<TimelineItemDto>,
    pub turn_status: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StopPromptResponse {
    pub session_id: String,
    pub stopped: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageDto {
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub cached_prompt_tokens: u64,
    pub total_tokens: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TimelineItemDto {
    pub kind: String,
    pub sequence: u64,
    pub timestamp: i64,
    pub turn_id: Option<String>,
    pub tool_call_id: Option<String>,
    pub tool_name: Option<String>,
    pub tool_arguments: Option<String>,
    pub tool_status: Option<String>,
    pub tool_result: Option<String>,
    pub inference_model: Option<String>,
    pub inference_usage: Option<UsageDto>,
    pub turn_status: Option<String>,
    pub turn_model: Option<String>,
    pub turn_usage: Option<UsageDto>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionTimelineDto {
    pub session_id: String,
    pub items: Vec<TimelineItemDto>,
    pub next_sequence: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentEventPayload {
    pub session_id: String,
    pub event: AgentEvent,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolApprovalRequestPayload {
    pub approval_id: String,
    pub session_id: String,
    pub name: String,
    pub arguments: serde_json::Value,
    pub working_directory: Option<String>,
    pub parent_subagent_id: Option<String>,
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
pub struct PromptFailedPayload {
    pub session_id: Option<String>,
    pub message: String,
}

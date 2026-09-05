//! Studio 无密钥 canonical Settings 快照。

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// Secret-free canonical Settings snapshot.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StudioSettingsSnapshot {
    pub revision: u64,
    pub updated_at: i64,
    pub settings: StudioSettings,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StudioSettings {
    pub default_provider_id: Option<String>,
    pub providers: Vec<StudioProviderSettings>,
    pub roles: Vec<StudioRoleSettings>,
    pub permission_mode: String,
    pub instructions: StudioInstructionsSettings,
    pub skills: StudioSkillsSettings,
    pub mcp_servers: Vec<StudioMcpServerSettings>,
    pub general: StudioGeneralSettings,
    pub web_search: StudioWebSearchSettings,
    pub deepseek_web_search: StudioDeepSeekWebSearchSettings,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StudioProviderSettings {
    pub pricing_enabled: bool,
    pub id: String,
    pub template_kind: String,
    pub name: String,
    pub base_url: String,
    pub has_bearer_token: bool,
    pub credential_required: bool,
    pub capability_source: String,
    pub hosted_web_search: bool,
    pub hosted_web_search_dialect: String,
    pub standalone_web_search: Option<String>,
    pub prompt_cache_dialect: String,
    pub responses_programmatic_tool_calling: bool,
    pub default_model: String,
    pub custom_models: Vec<StudioCustomModelSettings>,
    pub model_connection_modes: Vec<StudioModelConnectionSettings>,
    pub catalog_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StudioCustomModelSettings {
    pub context_window: u64,
    pub max_output_tokens: u64,
    pub slug: String,
    pub display_name: String,
    pub reasoning_efforts: Vec<String>,
    pub base_instructions: String,
    pub wire_protocol: String,
    pub supported_connection_modes: Vec<String>,
    pub default_connection_mode: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StudioModelConnectionSettings {
    pub slug: String,
    pub connection_mode: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StudioRoleSettings {
    pub key: String,
    pub provider_id: String,
    pub model: String,
    pub effort: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StudioInstructionsSettings {
    pub base_override: String,
    pub developer: String,
    pub user: String,
    pub project_doc_max_bytes: u64,
    pub project_doc_fallback_filenames: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StudioSkillsSettings {
    pub enabled: bool,
    pub auto_learn: bool,
    pub system_enabled: bool,
    pub project_dir: String,
    pub user_dir: String,
    pub external_dirs: Vec<String>,
    pub disabled: Vec<String>,
    pub auto_learn_min_tool_calls: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StudioMcpServerSettings {
    pub id: String,
    pub transport: String,
    pub endpoint: String,
    pub configuration: StudioMcpServerConfiguration,
    pub source_kind: String,
    pub mutation_policy: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
#[serde(rename_all = "camelCase")]
pub enum StudioMcpServerConfiguration {
    Enabled,
    Disabled,
    MissingCredential,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StudioGeneralSettings {
    pub follow_system_theme: bool,
    pub follow_active_turn: bool,
    pub compact_timeline: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StudioWebSearchSettings {
    pub configured_mode: String,
    pub effective_mode: String,
    pub availability: String,
    pub selected: bool,
    pub context_size: Option<String>,
    pub allowed_domains: Vec<String>,
    pub country: Option<String>,
    pub region: Option<String>,
    pub city: Option<String>,
    pub timezone: Option<String>,
    pub provider_id: Option<String>,
    pub model: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StudioDeepSeekWebSearchSettings {
    pub configured_enabled: bool,
    pub effective_enabled: bool,
    pub availability: String,
    pub selected: bool,
    pub provider_id: Option<String>,
    pub model: Option<String>,
}

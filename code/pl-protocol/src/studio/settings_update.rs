//! Studio Settings 更新请求体。

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use super::settings::{StudioGeneralSettings, StudioInstructionsSettings, StudioSkillsSettings};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UpdatePermissionSettingsRequest {
    pub expected_revision: u64,
    pub mode: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UpdateInstructionsSettingsRequest {
    pub expected_revision: u64,
    pub settings: StudioInstructionsSettings,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UpdateSkillsSettingsRequest {
    pub expected_revision: u64,
    pub settings: StudioSkillsSettings,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UpdateGeneralSettingsRequest {
    pub expected_revision: u64,
    pub settings: StudioGeneralSettings,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UpdateWebSearchSettingsRequest {
    pub expected_revision: u64,
    pub mode: String,
    pub context_size: Option<String>,
    #[serde(default)]
    pub allowed_domains: Vec<String>,
    pub country: Option<String>,
    pub region: Option<String>,
    pub city: Option<String>,
    pub timezone: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UpdateDeepSeekWebSearchSettingsRequest {
    pub expected_revision: u64,
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SetModelRoleRequest {
    pub expected_revision: u64,
    pub role: String,
    pub provider_id: String,
    pub model: String,
    pub effort: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UpdateMcpSettingsRequest {
    pub expected_revision: u64,
    pub servers: Vec<McpServerUpdate>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct McpServerUpdate {
    pub id: String,
    pub enabled: bool,
    pub transport: String,
    pub endpoint: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UpdateProviderSettingsRequest {
    pub expected_revision: u64,
    pub default_provider_id: String,
    pub providers: Vec<ProviderSettingsUpdate>,
    pub roles: Vec<RoleSettingsUpdate>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProviderSettingsUpdate {
    pub id: String,
    pub original_id: Option<String>,
    pub template_kind: String,
    pub name: String,
    pub base_url: String,
    pub secret: ProviderSecretUpdate,
    pub capability_source: String,
    pub hosted_web_search: bool,
    pub hosted_web_search_dialect: String,
    pub standalone_web_search: Option<String>,
    pub prompt_cache_dialect: String,
    pub responses_programmatic_tool_calling: bool,
    pub default_model: String,
    pub custom_models: Vec<ProviderModelUpdate>,
    pub model_connection_modes: Vec<ProviderModelConnectionUpdate>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProviderModelUpdate {
    pub slug: String,
    pub display_name: String,
    pub reasoning_efforts: Vec<String>,
    pub base_instructions: Option<String>,
    pub wire_protocol: String,
    pub supported_connection_modes: Vec<String>,
    pub default_connection_mode: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProviderModelConnectionUpdate {
    pub slug: String,
    pub connection_mode: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RoleSettingsUpdate {
    pub key: String,
    pub provider: String,
    pub model: String,
    pub effort: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
#[serde(rename_all = "camelCase", tag = "action", deny_unknown_fields)]
pub enum ProviderSecretUpdate {
    Preserve,
    Replace { value: String },
    Clear,
}

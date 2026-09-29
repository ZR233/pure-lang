//! Studio Settings 更新请求体。

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use super::settings::{
    StudioGeneralSettings, StudioInstructionsSettings, StudioSettingsSnapshot, StudioSkillsSettings,
};

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
pub struct SetModeModelRouteRequest {
    pub expected_revision: u64,
    pub mode_id: String,
    pub provider_id: String,
    pub model: String,
    pub effort: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SetThreadModelRouteRequest {
    pub expected_model_route_revision: u64,
    pub expected_settings_revision: u64,
    pub provider_id: String,
    pub model: String,
    pub effort: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ThreadModelRouteUpdateResponse {
    #[schema(value_type = Object)]
    pub runtime: crate::ThreadRuntimeSnapshot,
    pub settings: StudioSettingsSnapshot,
    pub mode_default_saved: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub warning: Option<String>,
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
    #[serde(default)]
    pub mode_routes: Vec<ModeRouteSettingsUpdate>,
    pub roles: Vec<RoleSettingsUpdate>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ModeRouteSettingsUpdate {
    pub mode_id: String,
    pub provider: String,
    pub model: String,
    pub effort: String,
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
    pub pricing_enabled: bool,
    pub default_model: String,
    pub custom_models: Vec<ProviderModelUpdate>,
    pub model_connection_modes: Vec<ProviderModelConnectionUpdate>,
    pub model_auto_compact_limits: Vec<ProviderModelAutoCompactUpdate>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProviderModelUpdate {
    pub slug: String,
    pub display_name: String,
    pub wire_protocol: String,
    pub context_window: u64,
    pub max_output_tokens: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProviderModelConnectionUpdate {
    pub slug: String,
    pub connection_mode: String,
}

/// 某模型上下文压缩阈值用户覆盖的完整集合项；`limit` 必须为正整数 tokens。
///
/// 该列表是该 provider 实例的完整覆盖集合；未列出的模型使用模型默认值。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProviderModelAutoCompactUpdate {
    pub slug: String,
    pub limit: u64,
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

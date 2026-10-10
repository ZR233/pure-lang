//! Studio Settings 更新请求体。

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use super::settings::SettingsStateResponse;

/// One independently mutable settings field/resource.
///
/// The unit of a mutation is intentionally smaller than the settings
/// snapshot.  A caller must describe the one value it intends to change; the
/// runtime reads every other value from its canonical state.  Route model and
/// reasoning effort are separate variants because a selector must not replay a
/// stale route snapshot when the other selector changes.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
#[serde(rename_all = "camelCase", tag = "kind", deny_unknown_fields)]
pub enum SettingsFieldUpdate {
    InstructionBaseOverride {
        value: String,
    },
    InstructionDeveloper {
        value: String,
    },
    InstructionUser {
        value: String,
    },
    ProjectDocMaxBytes {
        value: u64,
    },
    ProjectDocFallbackFilenames {
        value: Vec<String>,
    },
    SkillsEnabled {
        value: bool,
    },
    SkillsAutoLearn {
        value: bool,
    },
    SkillsSystemEnabled {
        value: bool,
    },
    SkillsProjectDir {
        value: String,
    },
    SkillsUserDir {
        value: String,
    },
    SkillsExternalDirs {
        value: Vec<String>,
    },
    SkillsDisabled {
        value: Vec<String>,
    },
    SkillsAutoLearnMinToolCalls {
        value: u32,
    },
    McpServerEnabled {
        id: String,
        value: bool,
    },
    McpServerTransport {
        id: String,
        transport: String,
    },
    McpServerEndpoint {
        id: String,
        endpoint: String,
    },
    GeneralFollowActiveTurn {
        value: bool,
    },
    GeneralCompactTimeline {
        value: bool,
    },
    GeneralSidebarWidth {
        value: Option<u16>,
    },
    GeneralPinnedThreadIds {
        value: Vec<String>,
    },
    GeneralPinnedProjectIds {
        value: Vec<String>,
    },
    WebSearchMode {
        value: String,
    },
    WebSearchContextSize {
        value: Option<String>,
    },
    WebSearchAllowedDomains {
        value: Vec<String>,
    },
    WebSearchCountry {
        value: Option<String>,
    },
    WebSearchRegion {
        value: Option<String>,
    },
    WebSearchCity {
        value: Option<String>,
    },
    WebSearchTimezone {
        value: Option<String>,
    },
    DeepSeekWebSearchEnabled {
        value: bool,
    },
    ModeModel {
        mode_id: String,
        provider_id: String,
        model: String,
    },
    ModeReasoningEffort {
        mode_id: String,
        effort: Option<String>,
    },
    RoleModel {
        role: String,
        provider_id: String,
        model: String,
    },
    RoleReasoningEffort {
        role: String,
        effort: Option<String>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UpdateSettingsFieldRequest {
    pub expected_revision: u64,
    pub update: SettingsFieldUpdate,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UpdatePermissionSettingsRequest {
    pub expected_revision: u64,
    pub mode: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
/// Existing Thread 的模型路由是一个原子资源：模型、思考强度和其对应的
/// runtime route 必须在同一次 CAS 中通过联合校验。新会话/Mode 默认值和
/// Agent role 则使用上面的单字段更新，选择模型不会重放旧思考强度。
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
    pub settings: SettingsStateResponse,
    pub mode_default_saved: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub warning: Option<String>,
}

/// 更新一个 provider 的配置。
///
/// Provider、默认 provider、模式路由和角色路由是四个独立的配置意图。
/// 这个请求只允许改变一个 provider 资源；provider 内部的 endpoint、凭据和
/// 模型声明需要一起校验并原子提交，避免保存半个不可用 provider。它不会携带
/// 默认 provider、模式路由或角色路由，设置页也不能把其它集合的旧快照一起写回。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UpdateProviderRequest {
    pub expected_revision: u64,
    pub provider: ProviderSettingsUpdate,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RemoveProviderRequest {
    pub expected_revision: u64,
    pub provider_id: String,
    /// 删除仍被模式或角色路由引用的 provider 时，必须显式指定替换 provider。
    /// 未被引用时可以省略；删除逻辑不会因为界面旧快照而静默改写路由。
    #[serde(default)]
    pub replacement_provider_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SetDefaultProviderRequest {
    pub expected_revision: u64,
    pub provider_id: String,
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
#[serde(rename_all = "camelCase", tag = "action", deny_unknown_fields)]
pub enum ProviderSecretUpdate {
    Preserve,
    Replace { value: String },
    Clear,
}

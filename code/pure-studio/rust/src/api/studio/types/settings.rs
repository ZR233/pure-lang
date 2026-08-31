use serde::{Deserialize, Serialize};
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
    #[serde(default)]
    pub original_id: Option<String>,
    pub template_kind: String,
    pub name: String,
    pub base_url: String,
    pub secret: ProviderSecretInput,
    pub capability_source: String,
    pub hosted_web_search: bool,
    pub hosted_web_search_dialect: String,
    pub standalone_web_search: Option<String>,
    pub prompt_cache_dialect: String,
    pub responses_programmatic_tool_calling: bool,
    pub default_model: String,
    pub custom_models: Vec<ProviderModelInput>,
    pub model_connection_modes: Vec<ProviderModelConnectionInput>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderModelInput {
    pub slug: String,
    pub display_name: String,
    pub reasoning_efforts: Vec<String>,
    pub base_instructions: Option<String>,
    pub wire_protocol: String,
    pub supported_connection_modes: Vec<String>,
    pub default_connection_mode: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderModelConnectionInput {
    pub slug: String,
    pub connection_mode: String,
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

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", tag = "scope")]
pub enum McpResetInput {
    Server { server_id: String },
    All,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", tag = "scope")]
pub enum LspScopeInput {
    Server {
        project_id: String,
        server_id: String,
    },
    Workspace {
        project_id: String,
    },
    All,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", tag = "action")]
pub enum ProviderSecretInput {
    Preserve,
    Replace { value: String },
    Clear,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GeneralSettingsInput {
    pub follow_system_theme: bool,
    pub follow_active_turn: bool,
    pub compact_timeline: bool,
}

/// Web 搜索设置的 typed bridge 输入。
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WebSearchSettingsInput {
    pub mode: String,
    pub context_size: Option<String>,
    #[serde(default)]
    pub allowed_domains: Vec<String>,
    pub country: Option<String>,
    pub region: Option<String>,
    pub city: Option<String>,
    pub timezone: Option<String>,
}

/// DeepSeek 原生 Web 搜索开关的 typed bridge 输入。
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeepSeekWebSearchSettingsInput {
    pub enabled: bool,
}

/// Web 搜索配置、有效状态和自动 OpenAI backend 的 canonical bridge 快照。
#[derive(Debug, Clone, serde::Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BridgeWebSearchSettingsDto {
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

/// DeepSeek 原生 Web 搜索的 canonical bridge 快照。
#[derive(Debug, Clone, serde::Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BridgeDeepSeekWebSearchSettingsDto {
    pub configured_enabled: bool,
    pub effective_enabled: bool,
    pub availability: String,
    pub selected: bool,
    pub provider_id: Option<String>,
    pub model: Option<String>,
}

/// Studio 配置与本地界面设置的 canonical typed 快照。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BridgeStudioSettingsDto {
    pub default_provider_id: Option<String>,
    pub providers: Vec<BridgeProviderSettingsDto>,
    pub roles: Vec<BridgeRoleSettingsDto>,
    pub permission_mode: String,
    pub instructions: BridgeInstructionsSettingsDto,
    pub skills: BridgeSkillsSettingsDto,
    pub mcp_servers: Vec<BridgeMcpServerSettingsDto>,
    pub general: BridgeGeneralSettingsDto,
    pub web_search: BridgeWebSearchSettingsDto,
    pub deepseek_web_search: BridgeDeepSeekWebSearchSettingsDto,
}

/// 不含 secret 的 Provider canonical 设置视图。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BridgeProviderSettingsDto {
    pub id: String,
    pub template_kind: String,
    pub name: String,
    pub base_url: String,
    pub has_bearer_token: bool,
    pub capability_source: String,
    pub hosted_web_search: bool,
    pub hosted_web_search_dialect: String,
    pub standalone_web_search: Option<String>,
    pub prompt_cache_dialect: String,
    pub responses_programmatic_tool_calling: bool,
    pub default_model: String,
    pub custom_models: Vec<BridgeCustomModelSettingsDto>,
    pub model_connection_modes: Vec<BridgeModelConnectionSettingsDto>,
    pub catalog_id: Option<String>,
}

/// Provider 配置中由用户定义的模型，不复制内置 catalog 元数据。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BridgeCustomModelSettingsDto {
    pub slug: String,
    pub display_name: String,
    pub reasoning_efforts: Vec<String>,
    pub base_instructions: String,
    pub wire_protocol: String,
    pub supported_connection_modes: Vec<String>,
    pub default_connection_mode: String,
}

/// Provider 实例对 canonical catalog 模型的连接方式覆盖。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BridgeModelConnectionSettingsDto {
    pub slug: String,
    pub connection_mode: String,
}

/// 角色到 provider/model/effort 的 canonical 路由。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BridgeRoleSettingsDto {
    pub key: String,
    pub provider_id: String,
    pub model: String,
    pub effort: String,
}

/// Instructions 页的 canonical 设置。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BridgeInstructionsSettingsDto {
    pub base_override: String,
    pub developer: String,
    pub user: String,
    pub project_doc_max_bytes: u64,
    pub project_doc_fallback_filenames: Vec<String>,
}

/// Skills 页的 canonical 设置。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BridgeSkillsSettingsDto {
    pub enabled: bool,
    pub auto_learn: bool,
    pub system_enabled: bool,
    pub project_dir: String,
    pub user_dir: String,
    pub external_dirs: Vec<String>,
    pub disabled: Vec<String>,
    pub auto_learn_min_tool_calls: u32,
}

/// MCP 设置页的 canonical server 视图。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BridgeMcpServerSettingsDto {
    pub id: String,
    pub transport: String,
    pub endpoint: String,
    pub configuration: BridgeMcpServerConfiguration,
    pub source_kind: String,
    pub mutation_policy: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum BridgeMcpServerConfiguration {
    Enabled,
    Disabled,
    MissingCredential,
}

/// Flutter 本地通用设置的 typed 快照。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BridgeGeneralSettingsDto {
    pub follow_system_theme: bool,
    pub follow_active_turn: bool,
    pub compact_timeline: bool,
}

impl Default for BridgeGeneralSettingsDto {
    fn default() -> Self {
        Self {
            follow_system_theme: true,
            follow_active_turn: true,
            compact_timeline: false,
        }
    }
}

// ── Provider catalog output ──

#[derive(Debug, Clone)]
pub struct BridgeProviderCatalogSnapshot {
    pub schema_version: u32,
    pub revision: String,
    pub presets: Vec<BridgeProviderPresetDescriptor>,
    pub model_catalogs: Vec<BridgeModelCatalogDescriptor>,
}

#[derive(Debug, Clone)]
pub struct BridgeProviderPresetDescriptor {
    pub id: String,
    pub display_name: String,
    pub description: Option<String>,
    pub base_url: String,
    pub credential_label: String,
    pub credential_env: Option<String>,
    pub model_catalog_id: String,
    pub suggested_model: String,
    pub icon_key: Option<String>,
    pub service_capabilities: BridgeProviderServiceCapabilitiesDescriptor,
}

#[derive(Debug, Clone)]
pub struct BridgeProviderServiceCapabilitiesDescriptor {
    pub web_search: BridgeWebSearchProviderCapabilitiesDescriptor,
    pub prompt_cache_dialect: String,
    pub responses_programmatic_tool_calling: bool,
}

#[derive(Debug, Clone)]
pub struct BridgeWebSearchProviderCapabilitiesDescriptor {
    pub hosted_responses: bool,
    pub hosted_dialect: String,
    pub standalone: Option<String>,
}

#[derive(Debug, Clone)]
pub struct BridgeModelTransportDescriptor {
    pub protocol: String,
    pub connection_modes: Vec<BridgeProviderConnectionModeDescriptor>,
    pub default_connection_mode: String,
}

#[derive(Debug, Clone)]
pub struct BridgeProviderConnectionModeDescriptor {
    pub id: String,
    pub display_name: String,
}

#[derive(Debug, Clone)]
pub struct BridgeModelCatalogDescriptor {
    pub id: String,
    pub models: Vec<BridgeModelDescriptor>,
}

#[derive(Debug, Clone)]
pub struct BridgeModelDescriptor {
    pub id: String,
    pub display_name: String,
    pub description: Option<String>,
    pub context_window: Option<u64>,
    pub max_context_window: Option<u64>,
    pub max_output_tokens: Option<u64>,
    pub transport: BridgeModelTransportDescriptor,
    pub capabilities: BridgeModelCapabilities,
    pub reasoning: Option<BridgeModelReasoningDescriptor>,
    pub pricing: Option<BridgeModelPricing>,
}

#[derive(Debug, Clone)]
pub struct BridgeModelCapabilities {
    pub input: Vec<BridgeModelInputCapability>,
    pub output: Vec<BridgeModelModality>,
    pub streaming: bool,
    pub temperature: bool,
    pub reasoning: bool,
    pub web_search: bool,
    pub function_calling: bool,
    pub parallel_tool_calls: bool,
    pub custom_tools: bool,
    pub freeform_tools: bool,
}

#[derive(Debug, Clone, Copy)]
pub enum BridgeModelModality {
    Text,
    Image,
    Audio,
    Video,
    File,
}

#[derive(Debug, Clone, Copy)]
pub enum BridgeModelInputSource {
    Local,
    RemoteUrl,
}

#[derive(Debug, Clone)]
pub struct BridgeModelInputCapability {
    pub modality: BridgeModelModality,
    pub sources: Vec<BridgeModelInputSource>,
    pub max_count: Option<u32>,
    pub max_bytes: Option<u64>,
    pub max_total_bytes: Option<u64>,
    pub max_width: Option<u32>,
    pub max_height: Option<u32>,
    pub media_types: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct BridgeModelReasoningDescriptor {
    pub parameter: String,
    pub label: String,
    pub default_candidate: Option<String>,
    pub candidates: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct BridgeModelPricing {
    pub currency: String,
    pub input_per_mtok: Option<f64>,
    pub output_per_mtok: Option<f64>,
    pub cache_read_per_mtok: Option<f64>,
    pub cache_write_per_mtok: Option<f64>,
}

impl From<pl_protocol::ProviderCatalogSnapshot> for BridgeProviderCatalogSnapshot {
    fn from(snapshot: pl_protocol::ProviderCatalogSnapshot) -> Self {
        Self {
            schema_version: snapshot.schema_version,
            revision: snapshot.revision,
            presets: snapshot
                .presets
                .into_iter()
                .map(|preset| BridgeProviderPresetDescriptor {
                    id: preset.id,
                    display_name: preset.display_name,
                    description: preset.description,
                    base_url: preset.base_url,
                    credential_label: preset.credential.label,
                    credential_env: preset.credential.env_var,
                    model_catalog_id: preset.model_catalog_id,
                    suggested_model: preset.suggested_model,
                    icon_key: preset.icon_key,
                    service_capabilities: BridgeProviderServiceCapabilitiesDescriptor {
                        web_search: BridgeWebSearchProviderCapabilitiesDescriptor {
                            hosted_responses: preset
                                .service_capabilities
                                .web_search
                                .hosted_responses,
                            hosted_dialect: preset.service_capabilities.web_search.hosted_dialect,
                            standalone: preset.service_capabilities.web_search.standalone,
                        },
                        prompt_cache_dialect: preset.service_capabilities.prompt_cache_dialect,
                        responses_programmatic_tool_calling: preset
                            .service_capabilities
                            .responses_programmatic_tool_calling,
                    },
                })
                .collect(),
            model_catalogs: snapshot
                .model_catalogs
                .into_values()
                .map(|catalog| BridgeModelCatalogDescriptor {
                    id: catalog.id,
                    models: catalog
                        .models
                        .into_iter()
                        .map(BridgeModelDescriptor::from)
                        .collect(),
                })
                .collect(),
        }
    }
}

impl From<pl_protocol::ModelDescriptor> for BridgeModelDescriptor {
    fn from(model: pl_protocol::ModelDescriptor) -> Self {
        Self {
            id: model.id,
            display_name: model.display_name,
            description: model.description,
            context_window: model.context_window,
            max_context_window: model.max_context_window,
            max_output_tokens: model.max_output_tokens,
            transport: BridgeModelTransportDescriptor {
                protocol: model.transport.protocol,
                connection_modes: model
                    .transport
                    .connection_modes
                    .into_iter()
                    .map(|mode| BridgeProviderConnectionModeDescriptor {
                        id: mode.id,
                        display_name: mode.display_name,
                    })
                    .collect(),
                default_connection_mode: model.transport.default_connection_mode,
            },
            capabilities: BridgeModelCapabilities {
                input: model
                    .capabilities
                    .input
                    .into_iter()
                    .map(|capability| BridgeModelInputCapability {
                        modality: bridge_modality(capability.modality),
                        sources: capability
                            .sources
                            .into_iter()
                            .map(bridge_input_source)
                            .collect(),
                        max_count: capability.max_count,
                        max_bytes: capability.max_bytes,
                        max_total_bytes: capability.max_total_bytes,
                        max_width: capability.max_width,
                        max_height: capability.max_height,
                        media_types: capability.media_types,
                    })
                    .collect(),
                output: model
                    .capabilities
                    .output
                    .into_iter()
                    .map(bridge_modality)
                    .collect(),
                streaming: model.capabilities.streaming,
                temperature: model.capabilities.temperature,
                reasoning: model.capabilities.reasoning,
                web_search: model.capabilities.web_search,
                function_calling: model.capabilities.function_calling,
                parallel_tool_calls: model.capabilities.parallel_tool_calls,
                custom_tools: model.capabilities.custom_tools,
                freeform_tools: model.capabilities.freeform_tools,
            },
            reasoning: model
                .reasoning
                .map(|reasoning| BridgeModelReasoningDescriptor {
                    parameter: reasoning.parameter,
                    label: reasoning.label,
                    default_candidate: reasoning.default,
                    candidates: reasoning.candidates,
                }),
            pricing: model.pricing.map(|pricing| BridgeModelPricing {
                currency: pricing.currency,
                input_per_mtok: pricing.input_per_mtok,
                output_per_mtok: pricing.output_per_mtok,
                cache_read_per_mtok: pricing.cache_read_per_mtok,
                cache_write_per_mtok: pricing.cache_write_per_mtok,
            }),
        }
    }
}

fn bridge_modality(modality: pl_protocol::ModelModalityDto) -> BridgeModelModality {
    match modality {
        pl_protocol::ModelModalityDto::Text => BridgeModelModality::Text,
        pl_protocol::ModelModalityDto::Image => BridgeModelModality::Image,
        pl_protocol::ModelModalityDto::Audio => BridgeModelModality::Audio,
        pl_protocol::ModelModalityDto::Video => BridgeModelModality::Video,
        pl_protocol::ModelModalityDto::File => BridgeModelModality::File,
    }
}

fn bridge_input_source(source: pl_protocol::ModelInputSourceDto) -> BridgeModelInputSource {
    match source {
        pl_protocol::ModelInputSourceDto::Local => BridgeModelInputSource::Local,
        pl_protocol::ModelInputSourceDto::RemoteUrl => BridgeModelInputSource::RemoteUrl,
    }
}

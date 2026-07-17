use serde::Deserialize;
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
    pub wire_protocol: String,
    pub connection_mode: String,
    pub name: String,
    pub base_url: String,
    pub bearer_token: String,
    pub default_model: String,
    pub custom_models: Vec<ProviderModelInput>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderModelInput {
    pub slug: String,
    pub display_name: String,
    pub reasoning_efforts: Vec<String>,
    pub base_instructions: Option<String>,
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

/// Web 搜索配置、有效状态和自动 OpenAI backend 的 canonical bridge 快照。
#[derive(Debug, Clone, serde::Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BridgeWebSearchSettingsDto {
    pub configured_mode: String,
    pub effective_mode: String,
    pub availability: String,
    pub context_size: Option<String>,
    pub allowed_domains: Vec<String>,
    pub country: Option<String>,
    pub region: Option<String>,
    pub city: Option<String>,
    pub timezone: Option<String>,
    pub provider_id: Option<String>,
    pub model: Option<String>,
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
    pub transport: BridgeProviderTransportDescriptor,
    pub base_url: String,
    pub credential_label: String,
    pub credential_env: Option<String>,
    pub model_catalog_id: String,
    pub suggested_model: String,
    pub icon_key: Option<String>,
}

#[derive(Debug, Clone)]
pub struct BridgeProviderTransportDescriptor {
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
    pub modalities: Vec<String>,
    pub capabilities: BridgeModelCapabilities,
    pub reasoning: Option<BridgeModelReasoningDescriptor>,
    pub pricing: Option<BridgeModelPricing>,
}

#[derive(Debug, Clone)]
pub struct BridgeModelCapabilities {
    pub streaming: bool,
    pub temperature: bool,
    pub reasoning: bool,
    pub web_search: bool,
    pub function_calling: bool,
    pub parallel_tool_calls: bool,
    pub custom_tools: bool,
    pub freeform_tools: bool,
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
                    transport: BridgeProviderTransportDescriptor {
                        protocol: preset.transport.protocol,
                        connection_modes: preset
                            .transport
                            .connection_modes
                            .into_iter()
                            .map(|mode| BridgeProviderConnectionModeDescriptor {
                                id: mode.id,
                                display_name: mode.display_name,
                            })
                            .collect(),
                        default_connection_mode: preset.transport.default_connection_mode,
                    },
                    base_url: preset.base_url,
                    credential_label: preset.credential.label,
                    credential_env: preset.credential.env_var,
                    model_catalog_id: preset.model_catalog_id,
                    suggested_model: preset.suggested_model,
                    icon_key: preset.icon_key,
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
            modalities: model.modalities,
            capabilities: BridgeModelCapabilities {
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
            }),
        }
    }
}

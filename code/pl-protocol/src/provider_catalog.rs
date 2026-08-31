use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// Provider/模型目录跨产品传输协议版本。
pub const PROVIDER_CATALOG_SCHEMA_VERSION: u32 = 9;

/// 无敏感信息、可供 Web 与桌面端直接渲染的 Provider 目录快照。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ProviderCatalogSnapshot {
    pub schema_version: u32,
    pub revision: String,
    pub presets: Vec<ProviderPresetDescriptor>,
    pub model_catalogs: BTreeMap<String, ModelCatalogDescriptor>,
}

/// 一个可创建 Provider 实例的内置预设。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProviderPresetDescriptor {
    pub id: String,
    pub display_name: String,
    pub description: Option<String>,
    pub base_url: String,
    pub credential: CredentialDescriptorDto,
    pub model_catalog_id: String,
    pub suggested_model: String,
    pub icon_key: Option<String>,
    pub service_capabilities: ProviderServiceCapabilitiesDescriptor,
}

/// 无敏感信息的 Provider 外部服务能力。
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProviderServiceCapabilitiesDescriptor {
    pub web_search: WebSearchProviderCapabilitiesDescriptor,
    #[serde(default)]
    pub prompt_cache_dialect: String,
    #[serde(default)]
    pub responses_programmatic_tool_calling: bool,
}

/// UI 可直接渲染的 Web Search 服务能力。
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct WebSearchProviderCapabilitiesDescriptor {
    pub hosted_responses: bool,
    pub hosted_dialect: String,
    pub standalone: Option<String>,
}

/// 当前角色 Web Search configured/effective 路径的无密钥投影。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WebSearchResolutionDescriptor {
    pub configured_mode: String,
    pub effective_mode: String,
    pub availability: String,
    pub path: Option<String>,
    pub provider_id: Option<String>,
    pub model: Option<String>,
}

/// UI 可直接渲染的模型连接模式选项。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProviderConnectionModeDescriptor {
    pub id: String,
    pub display_name: String,
}

/// Provider 凭证输入提示；不包含真实 secret。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CredentialDescriptorDto {
    pub label: String,
    pub env_var: Option<String>,
}

/// 一组共享同一 provider 模型语义的内置目录。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ModelCatalogDescriptor {
    pub id: String,
    pub models: Vec<ModelDescriptor>,
}

/// UI 和外部宿主需要的模型元数据投影。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ModelDescriptor {
    pub id: String,
    pub display_name: String,
    pub description: Option<String>,
    pub context_window: Option<u64>,
    pub max_context_window: Option<u64>,
    pub max_output_tokens: Option<u64>,
    pub transport: ModelTransportDescriptor,
    pub capabilities: ModelCapabilitiesDto,
    pub reasoning: Option<ModelReasoningDescriptor>,
    pub pricing: Option<ModelPricingDto>,
}

/// UI 可直接渲染的模型 API 协议与连接策略。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ModelTransportDescriptor {
    pub protocol: String,
    pub connection_modes: Vec<ProviderConnectionModeDescriptor>,
    pub default_connection_mode: String,
}

/// UI 可直接判断的模型能力摘要。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ModelCapabilitiesDto {
    pub input: Vec<ModelInputCapabilityDto>,
    pub output: Vec<ModelModalityDto>,
    pub streaming: bool,
    pub temperature: bool,
    pub reasoning: bool,
    pub web_search: bool,
    pub function_calling: bool,
    pub parallel_tool_calls: bool,
    pub custom_tools: bool,
    pub freeform_tools: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ModelModalityDto {
    Text,
    Image,
    Audio,
    Video,
    File,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ModelInputSourceDto {
    Local,
    RemoteUrl,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ModelInputCapabilityDto {
    pub modality: ModelModalityDto,
    pub sources: Vec<ModelInputSourceDto>,
    pub max_count: Option<u32>,
    pub max_bytes: Option<u64>,
    pub max_total_bytes: Option<u64>,
    pub max_width: Option<u32>,
    pub max_height: Option<u32>,
    pub media_types: Vec<String>,
}

/// reasoning/effort 下拉的完整动态描述。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ModelReasoningDescriptor {
    pub parameter: String,
    pub label: String,
    pub default: Option<String>,
    pub candidates: Vec<String>,
}

/// 可选的模型价格投影，单位为每百万 token。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ModelPricingDto {
    pub currency: String,
    pub input_per_mtok: Option<f64>,
    pub output_per_mtok: Option<f64>,
    pub cache_read_per_mtok: Option<f64>,
    pub cache_write_per_mtok: Option<f64>,
}

//! Endpoint configuration, concrete native clients and independently supported services.
mod clients;
pub mod compatible;
pub mod deepseek;
pub(crate) mod files;
pub mod mimo;
pub mod openai;
pub mod zhipu;
pub use clients::ProviderClient;

use std::collections::HashMap;
use std::hash::{DefaultHasher, Hash, Hasher};

use serde::Deserialize;
use serde::Serialize;

use crate::runtime::transport_policy::RESPONSES_WEBSOCKET_PROFILE_REVISION;

// 公共能力类型字段（如 `WebSearchProviderCapabilities::hosted_dialect`）使用的
// pl-protocol 类型在此精确重导出，消费方只需依赖 pl-model。
pub use pl_protocol::HostedWebSearchDialect;

pub const ZHIPU_CODING_PLAN_BASE_URL: &str = "https://open.bigmodel.cn/api/v1";
pub(crate) const RESPONSES_WEBSOCKET_DIALECT: &str = "responses_websockets=2026-02-06";

/// Provider 可提供的独立 Web Search 协议。
///
/// 该类型描述服务能力，不代表 provider 身份；任何兼容 endpoint 都可显式声明。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StandaloneWebSearchDialect {
    OpenAiSearchApi,
}

impl StandaloneWebSearchDialect {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::OpenAiSearchApi => "open_ai_search_api",
        }
    }
}

impl std::str::FromStr for StandaloneWebSearchDialect {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "open_ai_search_api" => Ok(Self::OpenAiSearchApi),
            value => Err(format!(
                "unsupported standalone web search dialect: {value}"
            )),
        }
    }
}

/// Provider endpoint 可提供的 Web Search 服务能力。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct WebSearchProviderCapabilities {
    #[serde(default)]
    pub hosted_responses: bool,
    #[serde(default)]
    pub hosted_dialect: HostedWebSearchDialect,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub standalone: Option<StandaloneWebSearchDialect>,
}

/// 与具体产品无关的 Provider 外部服务能力。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderServiceCapabilities {
    #[serde(default)]
    pub files: FileUploadCapability,
    #[serde(default)]
    pub remote_compaction: bool,
    #[serde(default)]
    pub web_search: WebSearchProviderCapabilities,
    #[serde(default)]
    pub prompt_cache: PromptCacheProviderCapabilities,
    #[serde(default)]
    pub responses_tools: ResponsesHostedToolCapabilities,
}

/// Explicit upload dialect; custom endpoints must opt in.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum FileUploadCapability {
    #[default]
    None,
    DeepSeek,
}

/// Endpoint 对 Responses hosted tool 类型的支持。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResponsesHostedToolCapabilities {
    #[serde(default)]
    pub programmatic_tool_calling: bool,
}

/// Provider endpoint 的提示词缓存 dialect。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PromptCacheDialect {
    #[default]
    None,
    ImplicitPrefix,
    OpenAiPromptCacheKey,
}

impl PromptCacheDialect {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::ImplicitPrefix => "implicit_prefix",
            Self::OpenAiPromptCacheKey => "open_ai_prompt_cache_key",
        }
    }
}

impl std::str::FromStr for PromptCacheDialect {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "none" => Ok(Self::None),
            "implicit_prefix" => Ok(Self::ImplicitPrefix),
            "open_ai_prompt_cache_key" => Ok(Self::OpenAiPromptCacheKey),
            value => Err(format!("unsupported prompt cache dialect: {value}")),
        }
    }
}

/// Provider endpoint 可提供的提示词缓存能力。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PromptCacheProviderCapabilities {
    #[serde(default)]
    pub dialect: PromptCacheDialect,
}

/// 当前 provider、wire 与 model 合成后的缓存策略。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum EffectivePromptCachePolicy {
    #[default]
    None,
    ImplicitPrefix,
    OpenAiPromptCacheKey,
}

impl EffectivePromptCachePolicy {
    pub const fn label(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::ImplicitPrefix => "implicitPrefix",
            Self::OpenAiPromptCacheKey => "openAiPromptCacheKey",
        }
    }

    pub const fn uses_prompt_cache_key(self) -> bool {
        matches!(self, Self::OpenAiPromptCacheKey)
    }
}

impl ProviderServiceCapabilities {
    /// 返回同时支持 Responses hosted 与 OpenAI Search API 的能力集合。
    pub fn openai_web_search() -> Self {
        Self {
            remote_compaction: true,
            files: FileUploadCapability::None,
            web_search: WebSearchProviderCapabilities {
                hosted_responses: true,
                hosted_dialect: HostedWebSearchDialect::OpenAiResponses,
                standalone: Some(StandaloneWebSearchDialect::OpenAiSearchApi),
            },
            prompt_cache: PromptCacheProviderCapabilities {
                dialect: PromptCacheDialect::OpenAiPromptCacheKey,
            },
            responses_tools: ResponsesHostedToolCapabilities {
                programmatic_tool_calling: true,
            },
        }
    }
}

/// Explicit adapter selection. Custom endpoints never acquire a vendor identity from their URL.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ProviderAdapterKind {
    OpenAi,
    DeepSeek,
    Zhipu,
    MiMo,
    #[default]
    OpenAiCompatible,
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProviderEndpoint {
    #[serde(default)]
    pub adapter: ProviderAdapterKind,
    pub name: String,
    pub base_url: String,
    pub bearer_token: Option<String>,
    pub http_headers: Option<HashMap<String, String>>,
    #[serde(default)]
    pub tool_wire_policy: ToolWirePolicy,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub apply_patch_tool_type: Option<ApplyPatchToolType>,
    #[serde(default)]
    pub service_capabilities: ProviderServiceCapabilities,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderWireProtocol {
    Responses,
    #[default]
    ChatCompletions,
}

/// Provider 流式完成请求使用的连接方式。
///
/// 连接方式与 wire API 正交：OpenAI 的两个模式都使用 Responses，HTTP
/// 模式通过 SSE 返回事件，WebSocket 模式通过 `response.create` 帧返回事件。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderConnectionMode {
    WebSocket,
    #[default]
    Http,
}

/// 返回 transport 实现的确定性版本标识，供上层 catalog revision 纳入缓存失效。
///
/// 标识不进入配置或产品 DTO；Responses WebSocket 的值与实际握手 dialect
/// 常量同源，协议适配发生变化时 catalog ETag 会随二进制一起变化。
pub fn provider_transport_profile_revision(
    protocol: ProviderWireProtocol,
    mode: ProviderConnectionMode,
) -> &'static str {
    match (protocol, mode) {
        (ProviderWireProtocol::Responses, ProviderConnectionMode::WebSocket) => {
            RESPONSES_WEBSOCKET_PROFILE_REVISION
        }
        (ProviderWireProtocol::Responses, ProviderConnectionMode::Http) => "responses-http-v1",
        (ProviderWireProtocol::ChatCompletions, ProviderConnectionMode::Http) => {
            "chat-completions-http-v1"
        }
        (ProviderWireProtocol::ChatCompletions, ProviderConnectionMode::WebSocket) => {
            "unsupported-chat-completions-websocket"
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolWirePolicy {
    NativeCustomTools,
    #[default]
    FunctionFallback,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApplyPatchToolType {
    Freeform,
}

impl std::fmt::Debug for ProviderEndpoint {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ProviderEndpoint")
            .field("name", &self.name)
            .field("adapter", &self.adapter)
            .field("has_credential", &self.bearer_token.is_some())
            .finish_non_exhaustive()
    }
}

impl ProviderEndpoint {
    /// Selects a concrete adapter explicitly, without changing endpoint credentials.
    pub fn with_adapter(mut self, adapter: ProviderAdapterKind) -> Self {
        self.adapter = adapter;
        self
    }

    pub fn effective_prompt_cache_policy(
        &self,
        model: &crate::model::ModelInfo,
    ) -> EffectivePromptCachePolicy {
        match (
            model.binding.transport.protocol,
            self.service_capabilities.prompt_cache.dialect,
        ) {
            (_, PromptCacheDialect::ImplicitPrefix) => EffectivePromptCachePolicy::ImplicitPrefix,
            (ProviderWireProtocol::Responses, PromptCacheDialect::OpenAiPromptCacheKey) => {
                EffectivePromptCachePolicy::OpenAiPromptCacheKey
            }
            _ => EffectivePromptCachePolicy::None,
        }
    }

    pub fn openai(base_url: Option<String>) -> Self {
        let custom_endpoint = base_url.is_some();
        let mut service_capabilities = ProviderServiceCapabilities::openai_web_search();
        if custom_endpoint {
            service_capabilities.remote_compaction = false;
            service_capabilities.responses_tools = ResponsesHostedToolCapabilities::default();
        }
        Self {
            adapter: ProviderAdapterKind::OpenAi,
            name: "OpenAI".into(),
            base_url: base_url.unwrap_or_else(|| "https://api.openai.com/v1".into()),
            bearer_token: None,
            http_headers: None,
            tool_wire_policy: ToolWirePolicy::NativeCustomTools,
            apply_patch_tool_type: Some(ApplyPatchToolType::Freeform),
            service_capabilities,
        }
    }

    pub fn deepseek(base_url: Option<String>) -> Self {
        let files = if base_url
            .as_deref()
            .is_none_or(|url| url.trim_end_matches('/') == "https://api.deepseek.com")
        {
            FileUploadCapability::DeepSeek
        } else {
            FileUploadCapability::None
        };
        Self {
            adapter: ProviderAdapterKind::DeepSeek,
            name: "DeepSeek".into(),
            base_url: base_url.unwrap_or_else(|| "https://api.deepseek.com".into()),
            bearer_token: None,
            http_headers: None,
            tool_wire_policy: ToolWirePolicy::FunctionFallback,
            apply_patch_tool_type: None,
            service_capabilities: ProviderServiceCapabilities {
                files,
                web_search: WebSearchProviderCapabilities {
                    hosted_responses: true,
                    hosted_dialect: HostedWebSearchDialect::DeepSeekResponses,
                    standalone: None,
                },
                prompt_cache: PromptCacheProviderCapabilities {
                    dialect: PromptCacheDialect::ImplicitPrefix,
                },
                ..ProviderServiceCapabilities::default()
            },
        }
    }

    pub fn zhipu(base_url: Option<String>) -> Self {
        Self::compatible(
            "Zhipu",
            base_url.unwrap_or_else(|| "https://open.bigmodel.cn/api/paas/v4".into()),
        )
        .with_adapter(ProviderAdapterKind::Zhipu)
    }

    pub fn zhipu_coding_plan(base_url: Option<String>) -> Self {
        Self::compatible(
            "Zhipu Coding Plan",
            base_url.unwrap_or_else(|| ZHIPU_CODING_PLAN_BASE_URL.into()),
        )
        .with_adapter(ProviderAdapterKind::Zhipu)
    }

    /// 构造通用 OpenAI-compatible provider。
    ///
    /// 协议与连接模式属于绑定模型的 transport profile；endpoint 只采用最保守
    /// 的 function tool wire，不因兼容服务名称继承官方 OpenAI 能力。
    pub fn compatible(name: impl Into<String>, base_url: impl Into<String>) -> Self {
        Self {
            adapter: ProviderAdapterKind::OpenAiCompatible,
            name: name.into(),
            base_url: base_url.into(),
            bearer_token: None,
            http_headers: None,
            tool_wire_policy: ToolWirePolicy::FunctionFallback,
            apply_patch_tool_type: None,
            service_capabilities: ProviderServiceCapabilities::default(),
        }
    }

    pub fn uses_native_custom_tools(&self) -> bool {
        matches!(self.tool_wire_policy, ToolWirePolicy::NativeCustomTools)
    }

    /// 返回只用于进程内连接复用判定的 provider 指纹。
    ///
    /// 指纹覆盖 endpoint、凭证和 headers，但不会暴露这些原始值。配置发生
    /// 变化时，已有模型会话 会断开旧 WebSocket 并建立新连接。
    pub fn connection_fingerprint(&self) -> u64 {
        let mut hasher = DefaultHasher::new();
        self.base_url.trim_end_matches('/').hash(&mut hasher);
        self.bearer_token.hash(&mut hasher);
        if let Some(headers) = &self.http_headers {
            let mut headers = headers.iter().collect::<Vec<_>>();
            headers.sort_by(|left, right| left.0.cmp(right.0));
            for (name, value) in headers {
                name.hash(&mut hasher);
                value.hash(&mut hasher);
            }
        }
        let fingerprint = hasher.finish();
        if fingerprint == 0 { 1 } else { fingerprint }
    }
}

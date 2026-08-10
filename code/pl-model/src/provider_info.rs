use std::collections::HashMap;
use std::hash::{DefaultHasher, Hash, Hasher};

use serde::Deserialize;
use serde::Serialize;

use crate::transport_policy::RESPONSES_WEBSOCKET_PROFILE_REVISION;

pub const ZHIPU_CODING_PLAN_BASE_URL: &str = "https://open.bigmodel.cn/api/coding/paas/v4";
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub standalone: Option<StandaloneWebSearchDialect>,
}

/// 与具体产品无关的 Provider 外部服务能力。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderServiceCapabilities {
    #[serde(default)]
    pub web_search: WebSearchProviderCapabilities,
    #[serde(default)]
    pub prompt_cache: PromptCacheProviderCapabilities,
    #[serde(default)]
    pub responses_tools: ResponsesHostedToolCapabilities,
}

/// Endpoint 对 Responses hosted tool 类型的支持。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResponsesHostedToolCapabilities {
    #[serde(default)]
    pub tool_search: bool,
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
    OpenAiPromptCacheKey {
        cache_write_tokens: bool,
    },
}

impl EffectivePromptCachePolicy {
    pub const fn label(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::ImplicitPrefix => "implicitPrefix",
            Self::OpenAiPromptCacheKey { .. } => "openAiPromptCacheKey",
        }
    }

    pub const fn uses_prompt_cache_key(self) -> bool {
        matches!(self, Self::OpenAiPromptCacheKey { .. })
    }
}

impl ProviderServiceCapabilities {
    /// 返回同时支持 Responses hosted 与 OpenAI Search API 的能力集合。
    pub fn openai_web_search() -> Self {
        Self {
            web_search: WebSearchProviderCapabilities {
                hosted_responses: true,
                standalone: Some(StandaloneWebSearchDialect::OpenAiSearchApi),
            },
            prompt_cache: PromptCacheProviderCapabilities {
                dialect: PromptCacheDialect::OpenAiPromptCacheKey,
            },
            responses_tools: ResponsesHostedToolCapabilities {
                tool_search: true,
                programmatic_tool_calling: true,
            },
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProviderInfo {
    pub protocol: ProviderWireProtocol,
    pub connection_mode: ProviderConnectionMode,
    pub name: String,
    pub base_url: String,
    #[serde(default)]
    pub default_model: String,
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

impl ProviderInfo {
    pub fn default_provider() -> Self {
        Self::deepseek(None)
    }

    pub fn effective_prompt_cache_policy(
        &self,
        model: &crate::ModelInfo,
    ) -> EffectivePromptCachePolicy {
        match (
            self.protocol,
            self.service_capabilities.prompt_cache.dialect,
        ) {
            (ProviderWireProtocol::ChatCompletions, PromptCacheDialect::ImplicitPrefix) => {
                EffectivePromptCachePolicy::ImplicitPrefix
            }
            (ProviderWireProtocol::Responses, PromptCacheDialect::OpenAiPromptCacheKey) => {
                EffectivePromptCachePolicy::OpenAiPromptCacheKey {
                    cache_write_tokens: model.capabilities.prompt_cache.cache_write_tokens,
                }
            }
            _ => EffectivePromptCachePolicy::None,
        }
    }

    pub fn openai(base_url: Option<String>) -> Self {
        let custom_endpoint = base_url.is_some();
        let mut service_capabilities = ProviderServiceCapabilities::openai_web_search();
        if custom_endpoint {
            service_capabilities.responses_tools = ResponsesHostedToolCapabilities::default();
        }
        Self {
            protocol: ProviderWireProtocol::Responses,
            connection_mode: ProviderConnectionMode::WebSocket,
            name: "OpenAI".into(),
            base_url: base_url.unwrap_or_else(|| "https://api.openai.com/v1".into()),
            default_model: "gpt-5.6-sol".into(),
            bearer_token: None,
            http_headers: None,
            tool_wire_policy: ToolWirePolicy::NativeCustomTools,
            apply_patch_tool_type: Some(ApplyPatchToolType::Freeform),
            service_capabilities,
        }
    }

    pub fn deepseek(base_url: Option<String>) -> Self {
        Self {
            protocol: ProviderWireProtocol::ChatCompletions,
            connection_mode: ProviderConnectionMode::Http,
            name: "DeepSeek".into(),
            base_url: base_url.unwrap_or_else(|| "https://api.deepseek.com".into()),
            default_model: "deepseek-v4-flash".into(),
            bearer_token: None,
            http_headers: None,
            tool_wire_policy: ToolWirePolicy::FunctionFallback,
            apply_patch_tool_type: None,
            service_capabilities: ProviderServiceCapabilities {
                prompt_cache: PromptCacheProviderCapabilities {
                    dialect: PromptCacheDialect::ImplicitPrefix,
                },
                ..ProviderServiceCapabilities::default()
            },
        }
    }

    pub fn zhipu(base_url: Option<String>) -> Self {
        Self {
            protocol: ProviderWireProtocol::ChatCompletions,
            connection_mode: ProviderConnectionMode::Http,
            name: "Zhipu".into(),
            base_url: base_url.unwrap_or_else(|| "https://open.bigmodel.cn/api/paas/v4".into()),
            default_model: "glm-5.2".into(),
            bearer_token: None,
            http_headers: None,
            tool_wire_policy: ToolWirePolicy::FunctionFallback,
            apply_patch_tool_type: None,
            service_capabilities: ProviderServiceCapabilities::default(),
        }
    }

    pub fn zhipu_coding_plan(base_url: Option<String>) -> Self {
        Self {
            protocol: ProviderWireProtocol::ChatCompletions,
            connection_mode: ProviderConnectionMode::Http,
            name: "Zhipu Coding Plan".into(),
            base_url: base_url.unwrap_or_else(|| ZHIPU_CODING_PLAN_BASE_URL.into()),
            default_model: "glm-5.2".into(),
            bearer_token: None,
            http_headers: None,
            tool_wire_policy: ToolWirePolicy::FunctionFallback,
            apply_patch_tool_type: None,
            service_capabilities: ProviderServiceCapabilities::default(),
        }
    }

    pub fn openai_compatible_chat(
        name: impl Into<String>,
        base_url: impl Into<String>,
        default_model: impl Into<String>,
    ) -> Self {
        Self {
            protocol: ProviderWireProtocol::ChatCompletions,
            connection_mode: ProviderConnectionMode::Http,
            name: name.into(),
            base_url: base_url.into(),
            default_model: default_model.into(),
            bearer_token: None,
            http_headers: None,
            tool_wire_policy: ToolWirePolicy::FunctionFallback,
            apply_patch_tool_type: None,
            service_capabilities: ProviderServiceCapabilities::default(),
        }
    }

    /// 构造通用 Responses-compatible provider。
    ///
    /// 自定义兼容服务默认使用 HTTP，并采用最保守的 function tool wire；调用方
    /// 可显式选择 WebSocket，但不会因此继承官方 OpenAI 的 freeform tool 策略。
    pub fn responses_compatible(
        name: impl Into<String>,
        base_url: impl Into<String>,
        default_model: impl Into<String>,
    ) -> Self {
        Self {
            protocol: ProviderWireProtocol::Responses,
            connection_mode: ProviderConnectionMode::Http,
            name: name.into(),
            base_url: base_url.into(),
            default_model: default_model.into(),
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
    /// 变化时，已有 `AgentSession` 会断开旧 WebSocket 并建立新连接。
    pub fn connection_fingerprint(&self) -> u64 {
        let mut hasher = DefaultHasher::new();
        self.protocol.hash(&mut hasher);
        self.connection_mode.hash(&mut hasher);
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

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use pretty_assertions::assert_eq;

    use super::*;

    #[test]
    fn default_provider_is_deepseek() {
        let info = ProviderInfo::default_provider();

        assert_eq!(info.protocol, ProviderWireProtocol::ChatCompletions);
        assert_eq!(info.name, "DeepSeek");
        assert_eq!(info.default_model, "deepseek-v4-flash");
        assert_eq!(info.tool_wire_policy, ToolWirePolicy::FunctionFallback);
    }

    #[test]
    fn standalone_web_search_dialect_has_a_stable_wire_name() {
        let dialect = StandaloneWebSearchDialect::OpenAiSearchApi;

        assert_eq!(dialect.as_str(), "open_ai_search_api");
        assert_eq!(dialect.as_str().parse(), Ok(dialect));
        assert_eq!(
            "future_dialect".parse::<StandaloneWebSearchDialect>(),
            Err("unsupported standalone web search dialect: future_dialect".to_string())
        );
    }

    #[test]
    fn openai_uses_responses_profile_defaults() {
        let info = ProviderInfo::openai(None);

        assert_eq!(info.protocol, ProviderWireProtocol::Responses);
        assert_eq!(info.connection_mode, ProviderConnectionMode::WebSocket);
        assert_eq!(info.name, "OpenAI");
        assert_eq!(info.base_url, "https://api.openai.com/v1");
        assert_eq!(info.default_model, "gpt-5.6-sol");
        assert_eq!(info.tool_wire_policy, ToolWirePolicy::NativeCustomTools);
        assert_eq!(
            provider_transport_profile_revision(info.protocol, info.connection_mode),
            RESPONSES_WEBSOCKET_PROFILE_REVISION
        );
    }

    #[test]
    fn zhipu_uses_chat_completions_protocol() {
        let info = ProviderInfo::zhipu(None);

        assert_eq!(info.protocol, ProviderWireProtocol::ChatCompletions);
        assert_eq!(info.base_url, "https://open.bigmodel.cn/api/paas/v4");
        assert_eq!(info.default_model, "glm-5.2");
    }

    #[test]
    fn zhipu_coding_plan_uses_zhipu_runtime_with_coding_endpoint() {
        let info = ProviderInfo::zhipu_coding_plan(None);

        assert_eq!(info.protocol, ProviderWireProtocol::ChatCompletions);
        assert_eq!(info.name, "Zhipu Coding Plan");
        assert_eq!(info.base_url, ZHIPU_CODING_PLAN_BASE_URL);
        assert_eq!(info.default_model, "glm-5.2");
        assert_eq!(info.tool_wire_policy, ToolWirePolicy::FunctionFallback);
    }

    #[test]
    fn openai_compatible_chat_provider_can_express_mimo() {
        let info = ProviderInfo::openai_compatible_chat(
            "MiMo",
            "https://mimo.example.com/v1",
            "mimo-chat",
        );

        assert_eq!(info.protocol, ProviderWireProtocol::ChatCompletions);
        assert_eq!(info.connection_mode, ProviderConnectionMode::Http);
        assert_eq!(info.name, "MiMo");
        assert_eq!(info.base_url, "https://mimo.example.com/v1");
        assert_eq!(info.default_model, "mimo-chat");
        assert_eq!(info.tool_wire_policy, ToolWirePolicy::FunctionFallback);
    }

    #[test]
    fn custom_responses_provider_defaults_to_http_and_function_tools() {
        let info = ProviderInfo::responses_compatible(
            "Gateway",
            "https://gateway.example/v1",
            "gateway-model",
        );

        assert_eq!(info.protocol, ProviderWireProtocol::Responses);
        assert_eq!(info.connection_mode, ProviderConnectionMode::Http);
        assert_eq!(info.tool_wire_policy, ToolWirePolicy::FunctionFallback);
        assert_eq!(info.apply_patch_tool_type, None);
    }

    #[test]
    fn effective_prompt_cache_policy_requires_provider_wire_and_model_capability() {
        let deepseek = ProviderInfo::deepseek(None);
        let openai = ProviderInfo::openai(None);
        let compatible = ProviderInfo::responses_compatible(
            "Gateway",
            "https://gateway.example/v1",
            "gpt-5.6-sol",
        );
        let chat_compatible = ProviderInfo::openai_compatible_chat(
            "Chat Gateway",
            "https://chat.example/v1",
            "gpt-5.6-sol",
        );
        let deepseek_model = crate::default_models()
            .into_iter()
            .find(|model| model.slug == "deepseek-v4-flash")
            .unwrap();
        let openai_model = crate::default_models()
            .into_iter()
            .find(|model| model.slug == "gpt-5.6-sol")
            .unwrap();

        assert_eq!(
            deepseek.effective_prompt_cache_policy(&deepseek_model),
            EffectivePromptCachePolicy::ImplicitPrefix
        );
        assert_eq!(
            openai.effective_prompt_cache_policy(&openai_model),
            EffectivePromptCachePolicy::OpenAiPromptCacheKey {
                cache_write_tokens: true,
            }
        );
        assert_eq!(
            compatible.effective_prompt_cache_policy(&openai_model),
            EffectivePromptCachePolicy::None
        );
        assert_eq!(
            chat_compatible.effective_prompt_cache_policy(&openai_model),
            EffectivePromptCachePolicy::None
        );
    }

    #[test]
    fn websocket_fingerprint_changes_for_every_connection_credential() {
        let base = ProviderInfo::openai(None);
        let mut api_key = base.clone();
        api_key.bearer_token = Some("updated-secret".to_string());
        let mut endpoint = base.clone();
        endpoint.base_url = "https://proxy.example/v1".to_string();
        let mut headers = base.clone();
        headers.http_headers = Some(HashMap::from([(
            "x-tenant".to_string(),
            "tenant-b".to_string(),
        )]));
        let mut mode = base.clone();
        mode.connection_mode = ProviderConnectionMode::Http;

        let fingerprints = [
            base.connection_fingerprint(),
            api_key.connection_fingerprint(),
            endpoint.connection_fingerprint(),
            headers.connection_fingerprint(),
            mode.connection_fingerprint(),
        ];
        assert_eq!(
            fingerprints.iter().copied().collect::<HashSet<_>>().len(),
            5
        );
    }
}

use std::collections::HashMap;

use serde::Deserialize;
use serde::Serialize;
use serde_json::Map;
use serde_json::Value;

use crate::model::capabilities::{ModelCapabilities, ModelModality};
use crate::model::parameter::ModelParameter;
use crate::model::profile_error::ModelProfileError;
use crate::provider::{ProviderConnectionMode, ProviderWireProtocol};

/// 模型未显式声明默认压缩阈值时使用的十进制 token 数（258k）。
pub const DEFAULT_AUTO_COMPACT_TOKEN_LIMIT: u64 = 258_000;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelInfo {
    pub slug: String,
    pub display_name: String,
    pub description: Option<String>,

    pub context_window: Option<u64>,
    pub max_context_window: Option<u64>,
    pub auto_compact_token_limit: Option<u64>,

    pub default_temperature: Option<f32>,
    pub max_output_tokens: Option<u64>,
    pub pricing: super::pricing::ModelPricing,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub parameters: Vec<ModelParameter>,

    /// 模型使用的 API 协议、可用连接方式及默认连接方式。
    pub binding: ModelBinding,

    #[serde(default)]
    pub capabilities: ModelCapabilities,

    #[serde(default)]
    pub truncation_policy: TruncationPolicy,

    #[serde(default)]
    pub base_instructions: String,
}

/// Adapter binding kept separate from provider-neutral model metadata.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelBinding {
    pub transport: ModelTransportProfile,
    pub request: ModelRequestProfile,
}

impl ModelBinding {
    /// Changes wire API and its protocol-specific defaults together, preserving same-API options.
    pub fn set_transport(&mut self, transport: ModelTransportProfile) {
        let matches = matches!(
            (&self.request.protocol, transport.protocol),
            (
                ModelProtocolOptions::Responses(_),
                ProviderWireProtocol::Responses
            ) | (
                ModelProtocolOptions::ChatCompletions(_),
                ProviderWireProtocol::ChatCompletions
            )
        );
        if !matches {
            self.request.protocol = match transport.protocol {
                ProviderWireProtocol::Responses => {
                    ModelProtocolOptions::Responses(Default::default())
                }
                ProviderWireProtocol::ChatCompletions => {
                    ModelProtocolOptions::ChatCompletions(Default::default())
                }
            };
        }
        self.transport = transport;
    }
}

/// 模型拥有的 API wire 与连接策略。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelTransportProfile {
    pub protocol: ProviderWireProtocol,
    pub supported_connection_modes: Vec<ProviderConnectionMode>,
    pub default_connection_mode: ProviderConnectionMode,
}

impl ModelTransportProfile {
    pub fn responses_websocket() -> Self {
        Self {
            protocol: ProviderWireProtocol::Responses,
            supported_connection_modes: vec![
                ProviderConnectionMode::WebSocket,
                ProviderConnectionMode::Http,
            ],
            default_connection_mode: ProviderConnectionMode::WebSocket,
        }
    }

    pub fn responses_http() -> Self {
        Self {
            protocol: ProviderWireProtocol::Responses,
            supported_connection_modes: vec![ProviderConnectionMode::Http],
            default_connection_mode: ProviderConnectionMode::Http,
        }
    }

    pub fn chat_completions_http() -> Self {
        Self {
            protocol: ProviderWireProtocol::ChatCompletions,
            supported_connection_modes: vec![ProviderConnectionMode::Http],
            default_connection_mode: ProviderConnectionMode::Http,
        }
    }
}

impl Default for ModelTransportProfile {
    fn default() -> Self {
        Self::chat_completions_http()
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ModelRequestProfile {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_model: Option<String>,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub headers: HashMap<String, String>,
    #[serde(default, skip_serializing_if = "Map::is_empty")]
    pub body: Map<String, Value>,
    pub protocol: ModelProtocolOptions,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub media: Vec<ModelMediaInputProfile>,
    #[serde(default, skip_serializing_if = "MediaMixPolicy::is_default")]
    pub media_mix_policy: MediaMixPolicy,
}

impl ModelRequestProfile {
    /// 所有字段均未设置时返回 true（用于字段级合并判断「用户未提供」）。
    pub fn is_empty(&self) -> bool {
        self.api_model.is_none()
            && self.headers.is_empty()
            && self.body.is_empty()
            && self.protocol == ModelProtocolOptions::default()
            && self.media.is_empty()
            && self.media_mix_policy.is_default()
    }

    pub fn media_profile(&self, modality: ModelModality) -> Option<&ModelMediaInputProfile> {
        self.media
            .iter()
            .find(|profile| profile.modality == modality)
    }
}

/// Only options belonging to the selected wire API are representable.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "api", rename_all = "camelCase")]
pub enum ModelProtocolOptions {
    Responses(ResponsesRequestOptions),
    ChatCompletions(ChatRequestOptions),
}
impl Default for ModelProtocolOptions {
    fn default() -> Self {
        Self::ChatCompletions(ChatRequestOptions::default())
    }
}
/// Responses-only request controls.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResponsesRequestOptions {
    pub programmatic_tool_calling: bool,
    pub max_tokens_field: ResponsesMaxTokensField,
}
/// Chat-only request controls. Usage requests are independent of tool streaming.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatRequestOptions {
    pub parallel_tool_calls: bool,
    pub max_tokens_field: MaxTokensField,
    pub include_usage: bool,
    pub tool_stream: bool,
}
impl ModelRequestProfile {
    pub fn responses() -> Self {
        Self {
            protocol: ModelProtocolOptions::Responses(ResponsesRequestOptions::default()),
            ..Self::default()
        }
    }
    pub fn supports_programmatic_tool_calling(&self) -> bool {
        matches!(&self.protocol, ModelProtocolOptions::Responses(options) if options.programmatic_tool_calling)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelMediaInputProfile {
    pub modality: ModelModality,
    pub wire: MediaWireFormat,
    pub first_send: Vec<MediaRepresentation>,
    pub replay: Vec<MediaRepresentation>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MediaWireFormat {
    ChatImageUrl,
    ChatVideoUrl,
    ChatFileUrl,
    ResponsesInputImage,
}

impl MediaWireFormat {
    fn modality(self) -> ModelModality {
        match self {
            Self::ChatImageUrl | Self::ResponsesInputImage => ModelModality::Image,
            Self::ChatVideoUrl => ModelModality::Video,
            Self::ChatFileUrl => ModelModality::File,
        }
    }

    fn protocol(self) -> ProviderWireProtocol {
        match self {
            Self::ChatImageUrl | Self::ChatVideoUrl | Self::ChatFileUrl => {
                ProviderWireProtocol::ChatCompletions
            }
            Self::ResponsesInputImage => ProviderWireProtocol::Responses,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MediaRepresentation {
    RemoteUrl,
    ProviderFile,
    DataUrl,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MediaMixPolicy {
    #[default]
    Any,
    SingleModality,
}

impl MediaMixPolicy {
    fn is_default(&self) -> bool {
        *self == Self::Any
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MaxTokensField {
    #[default]
    MaxTokens,
    MaxCompletionTokens,
}

impl MaxTokensField {
    pub fn is_default(&self) -> bool {
        *self == Self::MaxTokens
    }
}

/// Controls how a Responses request serializes [`CompletionRequest::max_tokens`](crate::completion::CompletionRequest::max_tokens).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResponsesMaxTokensField {
    #[default]
    Omit,
    MaxOutputTokens,
    MaxTokens,
    MaxCompletionTokens,
}

impl ResponsesMaxTokensField {
    pub fn is_default(&self) -> bool {
        *self == Self::Omit
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TruncationPolicy {
    pub mode: TruncationMode,
    pub limit: u64,
}

impl Default for TruncationPolicy {
    fn default() -> Self {
        Self {
            mode: TruncationMode::Bytes,
            limit: 10_000,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TruncationMode {
    Bytes,
    Tokens,
}

impl ModelInfo {
    pub fn validate_media_contract(&self) -> Result<(), ModelProfileError> {
        let mut modalities = Vec::new();
        for capability in &self.capabilities.input {
            if modalities.contains(&capability.modality) {
                return Err(ModelProfileError::DuplicateInputModality {
                    model: self.slug.clone(),
                    modality: capability.modality,
                });
            }
            modalities.push(capability.modality);
            if capability.modality == ModelModality::Text {
                if !capability.sources.is_empty() {
                    return Err(ModelProfileError::TextInputDeclaresMediaSources {
                        model: self.slug.clone(),
                    });
                }
                continue;
            }
            let profile = self
                .binding
                .request
                .media_profile(capability.modality)
                .ok_or_else(|| ModelProfileError::ModalityWithoutMediaProfile {
                    model: self.slug.clone(),
                    modality: capability.modality,
                })?;
            if capability.sources.is_empty() {
                return Err(ModelProfileError::ModalityWithoutAdmittedSources {
                    model: self.slug.clone(),
                    modality: capability.modality,
                });
            }
            if profile.first_send.is_empty() || profile.replay.is_empty() {
                return Err(ModelProfileError::MissingSendOrReplayRepresentations {
                    model: self.slug.clone(),
                    modality: capability.modality,
                });
            }
            if profile.wire.modality() != profile.modality {
                return Err(ModelProfileError::WireModalityMismatch {
                    model: self.slug.clone(),
                    modality: capability.modality,
                    wire: profile.wire,
                });
            }
            if profile.wire.protocol() != self.binding.transport.protocol {
                return Err(ModelProfileError::WireProtocolMismatch {
                    model: self.slug.clone(),
                    wire: profile.wire,
                    protocol: self.binding.transport.protocol,
                });
            }
            if has_duplicates(&profile.first_send) || has_duplicates(&profile.replay) {
                return Err(ModelProfileError::RepeatedMediaRepresentation {
                    model: self.slug.clone(),
                    modality: capability.modality,
                });
            }
            if profile.replay.contains(&MediaRepresentation::RemoteUrl) {
                return Err(ModelProfileError::ReplayUsesRemoteUrl {
                    model: self.slug.clone(),
                    modality: capability.modality,
                });
            }
            if capability
                .sources
                .contains(&crate::model::ModelInputSource::Local)
                && !profile.first_send.iter().any(|representation| {
                    matches!(
                        representation,
                        MediaRepresentation::ProviderFile | MediaRepresentation::DataUrl
                    )
                })
            {
                return Err(ModelProfileError::LocalSourceWithoutDurableRepresentation {
                    model: self.slug.clone(),
                    modality: capability.modality,
                });
            }
            if profile.first_send.contains(&MediaRepresentation::RemoteUrl)
                && !capability
                    .sources
                    .contains(&crate::model::ModelInputSource::RemoteUrl)
            {
                return Err(ModelProfileError::RemoteUrlStrategyWithoutAdmission {
                    model: self.slug.clone(),
                    modality: capability.modality,
                });
            }
            if profile
                .replay
                .iter()
                .all(|representation| *representation == MediaRepresentation::RemoteUrl)
            {
                return Err(ModelProfileError::ReplayNotDurable {
                    model: self.slug.clone(),
                    modality: capability.modality,
                });
            }
        }
        let mut profile_modalities = Vec::new();
        for profile in &self.binding.request.media {
            if profile_modalities.contains(&profile.modality) {
                return Err(ModelProfileError::DuplicateMediaProfile {
                    model: self.slug.clone(),
                    modality: profile.modality,
                });
            }
            profile_modalities.push(profile.modality);
            if !self.capabilities.supports_input_modality(profile.modality) {
                return Err(ModelProfileError::MediaProfileForUndeclaredModality {
                    model: self.slug.clone(),
                    modality: profile.modality,
                });
            }
        }
        Ok(())
    }

    /// Validates a complete model binding and its price table before use.
    /// # Errors
    /// Returns the inconsistent transport, media or tariff contract, or a
    /// non-positive default auto-compact token limit.
    pub fn validate(&self) -> Result<(), ModelProfileError> {
        if [
            self.context_window,
            self.max_context_window,
            self.max_output_tokens,
        ]
        .contains(&Some(0))
            || matches!((self.context_window, self.max_context_window), (Some(current), Some(max)) if current > max)
            || self
                .default_temperature
                .is_some_and(|t| !t.is_finite() || t < 0.0)
        {
            return Err(ModelProfileError::InvalidBudget {
                model: self.slug.clone(),
            });
        }
        let mut names = std::collections::BTreeSet::new();
        for parameter in &self.parameters {
            let mut candidates = std::collections::BTreeSet::new();
            if parameter.name.trim().is_empty()
                || !names.insert(&parameter.name)
                || parameter.candidates.is_empty()
                || parameter
                    .candidates
                    .iter()
                    .any(|c| c.trim().is_empty() || !candidates.insert(c))
                || parameter.wire.len() != candidates.len()
                || parameter.wire.iter().any(|(candidate, wire)| {
                    !candidates.contains(candidate)
                        || wire.set.is_empty() && wire.remove.is_empty()
                        || wire
                            .set
                            .iter()
                            .map(|s| &s.path)
                            .chain(&wire.remove)
                            .any(|path| path.split('.').any(str::is_empty))
                })
            {
                return Err(ModelProfileError::InvalidParameter {
                    model: self.slug.clone(),
                });
            }
        }
        self.binding.transport.validate(&self.slug)?;
        if !matches!(
            (
                &self.binding.request.protocol,
                self.binding.transport.protocol
            ),
            (
                ModelProtocolOptions::Responses(_),
                ProviderWireProtocol::Responses
            ) | (
                ModelProtocolOptions::ChatCompletions(_),
                ProviderWireProtocol::ChatCompletions
            )
        ) {
            return Err(ModelProfileError::ProtocolOptionsMismatch {
                model: self.slug.clone(),
                protocol: self.binding.transport.protocol,
            });
        }
        if self.auto_compact_token_limit == Some(0) {
            return Err(ModelProfileError::InvalidAutoCompactTokenLimit {
                model: self.slug.clone(),
            });
        }
        self.pricing
            .validate()
            .map_err(|source| ModelProfileError::InvalidPricing {
                model: self.slug.clone(),
                source,
            })?;
        self.validate_media_contract()
    }

    pub fn resolved_context_window(&self) -> Option<u64> {
        self.context_window.or(self.max_context_window)
    }

    /// 模型默认上下文压缩阈值；未显式声明时为 [`DEFAULT_AUTO_COMPACT_TOKEN_LIMIT`]。
    pub fn default_auto_compact_token_limit(&self) -> u64 {
        self.auto_compact_token_limit
            .unwrap_or(DEFAULT_AUTO_COMPACT_TOKEN_LIMIT)
    }

    /// 上下文容量 90% 的安全上限；上下文未知时返回 `None`，保持不自动压缩。
    pub fn safe_auto_compact_token_limit(&self) -> Option<u64> {
        let context = self.resolved_context_window()?;
        // 用 u128 精确计算 floor(context * 90 / 100)，避免大 u64 相乘溢出。
        Some(((u128::from(context) * 90) / 100) as u64)
    }

    /// 在给定用户覆盖值下解析实际生效的压缩阈值。
    ///
    /// 生效值取「用户覆盖值或模型默认值」与上下文 90% 安全上限的较小值；
    /// 上下文未知时返回 `None`。用户覆盖不能绕过安全上限。
    pub fn resolved_auto_compact_limit_with(&self, override_limit: Option<u64>) -> Option<u64> {
        let safe = self.safe_auto_compact_token_limit()?;
        let selected = override_limit.unwrap_or_else(|| self.default_auto_compact_token_limit());
        Some(selected.min(safe))
    }

    /// 使用模型默认值（无用户覆盖）解析实际生效的压缩阈值。
    pub fn resolved_auto_compact_limit(&self) -> Option<u64> {
        self.resolved_auto_compact_limit_with(None)
    }

    /// Basic compatible model with configurable local 32K/4K budgets.
    pub fn compatible(slug: &str) -> Self {
        Self {
            slug: slug.to_string(),
            display_name: slug.to_string(),
            description: None,
            context_window: Some(32_000),
            max_context_window: Some(32_000),
            auto_compact_token_limit: None,
            default_temperature: None,
            max_output_tokens: Some(4096),
            pricing: super::pricing::ModelPricing::Unknown,
            parameters: Vec::new(),
            binding: ModelBinding {
                transport: ModelTransportProfile::default(),
                request: ModelRequestProfile {
                    protocol: ModelProtocolOptions::ChatCompletions(ChatRequestOptions {
                        include_usage: true,
                        ..Default::default()
                    }),
                    ..Default::default()
                },
            },
            capabilities: ModelCapabilities::text_only(),
            truncation_policy: TruncationPolicy {
                mode: TruncationMode::Bytes,
                limit: 10_000,
            },
            base_instructions: String::new(),
        }
    }

    /// 返回名为 "effort" 的参数声明，若模型未声明则返回 None。
    pub fn effort_parameter(&self) -> Option<&ModelParameter> {
        self.parameters
            .iter()
            .find(|parameter| parameter.name == "effort")
    }

    /// 返回 effort 参数的候选值字符串列表（GUI 下拉渲染用）。
    /// 若模型未声明 effort 参数，返回空 Vec。
    pub fn supported_efforts(&self) -> Vec<String> {
        self.effort_parameter()
            .map(|parameter| parameter.candidates.clone())
            .unwrap_or_default()
    }

    /// 返回 effort 的默认值（模型声明的候选值首项，且该首项是最弱强度），若模型未声明 effort 返回 None。
    pub fn default_effort(&self) -> Option<String> {
        self.effort_parameter()
            .and_then(|parameter| parameter.candidates.first().cloned())
    }
}

fn has_duplicates<T: PartialEq>(values: &[T]) -> bool {
    values
        .iter()
        .enumerate()
        .any(|(index, value)| values[..index].contains(value))
}

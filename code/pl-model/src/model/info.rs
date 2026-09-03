use std::collections::HashMap;

use serde::Deserialize;
use serde::Serialize;
use serde_json::Map;
use serde_json::Value;

use crate::model::capabilities::{ModelCapabilities, ModelModality};
use crate::model::parameter::ModelParameter;
use crate::model::profile_error::ModelProfileError;
use crate::provider::{ProviderConnectionMode, ProviderWireProtocol};

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
    pub currency: Option<String>,
    pub input_price_per_mtok: Option<f64>,
    pub output_price_per_mtok: Option<f64>,
    pub cache_read_price_per_mtok: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_write_price_per_mtok: Option<f64>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub parameters: Vec<ModelParameter>,

    /// 模型使用的 API 协议、可用连接方式及默认连接方式。
    pub transport: ModelTransportProfile,

    #[serde(default)]
    pub capabilities: ModelCapabilities,
    #[serde(default)]
    pub request_profile: ModelRequestProfile,

    #[serde(default)]
    pub truncation_policy: TruncationPolicy,

    #[serde(default)]
    pub base_instructions: String,

    #[serde(skip)]
    pub used_fallback: bool,
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
    #[serde(default, skip_serializing_if = "is_false")]
    pub chat_parallel_tool_calls: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub responses_programmatic_tool_calling: bool,
    #[serde(default, skip_serializing_if = "MaxTokensField::is_default")]
    pub max_tokens_field: MaxTokensField,
    #[serde(default, skip_serializing_if = "ResponsesMaxTokensField::is_default")]
    pub responses_max_tokens_field: ResponsesMaxTokensField,
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
            && !self.chat_parallel_tool_calls
            && !self.responses_programmatic_tool_calling
            && self.max_tokens_field.is_default()
            && self.responses_max_tokens_field.is_default()
            && self.media.is_empty()
            && self.media_mix_policy.is_default()
    }

    pub fn media_profile(&self, modality: ModelModality) -> Option<&ModelMediaInputProfile> {
        self.media
            .iter()
            .find(|profile| profile.modality == modality)
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

fn is_false(value: &bool) -> bool {
    !*value
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
                .request_profile
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
            if profile.wire.protocol() != self.transport.protocol {
                return Err(ModelProfileError::WireProtocolMismatch {
                    model: self.slug.clone(),
                    wire: profile.wire,
                    protocol: self.transport.protocol,
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
        for profile in &self.request_profile.media {
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

    pub fn resolved_context_window(&self) -> Option<u64> {
        self.context_window.or(self.max_context_window)
    }

    pub fn resolved_auto_compact_limit(&self) -> Option<u64> {
        let context = self.resolved_context_window()?;
        let default_limit = (context * 90) / 100;
        Some(
            self.auto_compact_token_limit
                .map_or(default_limit, |limit| limit.min(default_limit)),
        )
    }

    pub fn fallback(slug: &str) -> Self {
        Self {
            slug: slug.to_string(),
            display_name: slug.to_string(),
            description: None,
            context_window: Some(128_000),
            max_context_window: Some(128_000),
            auto_compact_token_limit: None,
            default_temperature: Some(0.3),
            max_output_tokens: Some(4096),
            currency: None,
            input_price_per_mtok: None,
            output_price_per_mtok: None,
            cache_read_price_per_mtok: None,
            cache_write_price_per_mtok: None,
            parameters: Vec::new(),
            transport: ModelTransportProfile::default(),
            capabilities: ModelCapabilities::text_only(),
            request_profile: ModelRequestProfile::default(),
            truncation_policy: TruncationPolicy {
                mode: TruncationMode::Bytes,
                limit: 10_000,
            },
            base_instructions: String::new(),
            used_fallback: true,
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

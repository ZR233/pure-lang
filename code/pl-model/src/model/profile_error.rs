//! 模型 transport 与媒体契约的校验错误。
//!
//! [`ModelTransportProfile::validate`](super::ModelTransportProfile::validate) 与
//! [`ModelInfo::validate_media_contract`](super::ModelInfo::validate_media_contract)
//! 在配置加载、保存与 runtime 构造时拒绝非法组合；变体携带 slug 与涉事模态等
//! 结构化上下文，消费方并入 `PureError::ConfigError`。

use thiserror::Error;

use crate::model::capabilities::ModelModality;
use crate::model::info::{MediaWireFormat, ModelTransportProfile};
use crate::provider::ProviderWireProtocol;

/// 模型 profile 校验失败的具体规则。
#[derive(Debug, Clone, PartialEq, Error)]
pub enum ModelProfileError {
    #[error("model {model} pricing: {source}")]
    InvalidPricing {
        model: String,
        source: super::pricing::PricingError,
    },
    #[error("model {model} request options do not match {protocol:?}")]
    ProtocolOptionsMismatch {
        model: String,
        protocol: ProviderWireProtocol,
    },
    /// `supported_connection_modes` 为空。
    #[error("model {model} has no supported connection modes")]
    NoSupportedConnectionModes { model: String },

    /// 默认连接方式不在支持列表内。
    #[error("model {model} default connection mode is not supported")]
    DefaultConnectionModeNotSupported { model: String },

    /// Chat Completions 协议不支持 WebSocket 连接。
    #[error("model {model} chat_completions transport does not support web_socket")]
    ChatCompletionsOverWebSocket { model: String },

    /// 同一模态在能力声明中出现两次。
    #[error("model {model} declares duplicate input modality {modality:?}")]
    DuplicateInputModality {
        model: String,
        modality: ModelModality,
    },

    /// 文本输入不得声明媒体来源。
    #[error("model {model} text input must not declare media sources")]
    TextInputDeclaresMediaSources { model: String },

    /// 声明的模态缺少请求媒体 profile。
    #[error("model {model} input modality {modality:?} has no request media profile")]
    ModalityWithoutMediaProfile {
        model: String,
        modality: ModelModality,
    },

    /// 声明的模态未声明任何允许来源。
    #[error("model {model} input modality {modality:?} has no admitted sources")]
    ModalityWithoutAdmittedSources {
        model: String,
        modality: ModelModality,
    },

    /// 媒体 profile 缺少首发或重放表示。
    #[error(
        "model {model} input modality {modality:?} needs first-send and replay representations"
    )]
    MissingSendOrReplayRepresentations {
        model: String,
        modality: ModelModality,
    },

    /// 媒体 wire 的模态与 profile 模态不一致。
    #[error("model {model} input modality {modality:?} does not match media wire {wire:?}")]
    WireModalityMismatch {
        model: String,
        modality: ModelModality,
        wire: MediaWireFormat,
    },

    /// 媒体 wire 的协议与模型 transport 协议不一致。
    #[error("model {model} media wire {wire:?} does not match transport {protocol:?}")]
    WireProtocolMismatch {
        model: String,
        wire: MediaWireFormat,
        protocol: ProviderWireProtocol,
    },

    /// 首发或重放表示重复。
    #[error("model {model} input modality {modality:?} repeats a media representation")]
    RepeatedMediaRepresentation {
        model: String,
        modality: ModelModality,
    },

    /// 重放不得使用原始远端 URL。
    #[error(
        "model {model} input modality {modality:?} replay must not use the original remote URL"
    )]
    ReplayUsesRemoteUrl {
        model: String,
        modality: ModelModality,
    },

    /// 允许本地来源时首发必须包含可持久化表示。
    #[error("model {model} input modality {modality:?} cannot send an admitted local snapshot")]
    LocalSourceWithoutDurableRepresentation {
        model: String,
        modality: ModelModality,
    },

    /// 首发包含远端 URL 但能力未声明远端来源。
    #[error(
        "model {model} input modality {modality:?} has a remote URL strategy without admitting URLs"
    )]
    RemoteUrlStrategyWithoutAdmission {
        model: String,
        modality: ModelModality,
    },

    /// 重放全部使用远端 URL，无法回放持久快照。
    #[error("model {model} input modality {modality:?} cannot replay a durable snapshot")]
    ReplayNotDurable {
        model: String,
        modality: ModelModality,
    },

    /// 同一模态的媒体 profile 出现两次。
    #[error("model {model} declares duplicate media profile {modality:?}")]
    DuplicateMediaProfile {
        model: String,
        modality: ModelModality,
    },

    /// 媒体 profile 声明了能力中不存在的模态。
    #[error("model {model} has a request media profile for undeclared modality {modality:?}")]
    MediaProfileForUndeclaredModality {
        model: String,
        modality: ModelModality,
    },
}

impl ModelTransportProfile {
    /// 校验 transport 组合的合法性。
    ///
    /// # Errors
    ///
    /// 支持列表为空、默认连接方式不受支持或 Chat Completions 声明 WebSocket 时
    /// 返回对应的 [`ModelProfileError`] 变体。
    pub fn validate(&self, model: &str) -> Result<(), ModelProfileError> {
        if self.supported_connection_modes.is_empty() {
            return Err(ModelProfileError::NoSupportedConnectionModes {
                model: model.to_string(),
            });
        }
        if !self
            .supported_connection_modes
            .contains(&self.default_connection_mode)
        {
            return Err(ModelProfileError::DefaultConnectionModeNotSupported {
                model: model.to_string(),
            });
        }
        if self.protocol == ProviderWireProtocol::ChatCompletions
            && self
                .supported_connection_modes
                .contains(&crate::provider::ProviderConnectionMode::WebSocket)
        {
            return Err(ModelProfileError::ChatCompletionsOverWebSocket {
                model: model.to_string(),
            });
        }
        Ok(())
    }
}

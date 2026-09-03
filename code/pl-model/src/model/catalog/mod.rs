//! 默认模型目录，按 provider 家族文件组织，用 `ModelFamily` 预设复用共享元数据。
//!
//! 参见 design/07-model.md 7.8 / 7.9 节。同 provider 的模型共享 capabilities、
//! truncation_policy、effort 参数声明（`ModelParameter`）和 base body，具体模型
//! 仅以 [`ModelInstanceSpec`] 差异字段从 family 派生。各 provider 的家族预设、
//! 实例数据与能力矩阵放在对应的 `deepseek` / `openai` / `mimo` / `zhipu` 子文件，
//! 本模块只保留目录编排与跨家族共享的 media/wire helper。

mod deepseek;
mod mimo;
mod openai;
mod zhipu;

use serde_json::Value;

use crate::model::capabilities::ModelModality;
use crate::model::info::{
    MediaRepresentation, MediaWireFormat, ModelInfo, ModelMediaInputProfile, ModelRequestProfile,
};
use crate::model::parameter::{ParameterWire, WireAssignment};

pub use deepseek::deepseek_default_model_slugs;
pub use mimo::mimo_default_model_slugs;
pub use openai::openai_default_model_slugs;
pub use zhipu::zhipu_default_model_slugs;

/// 内建默认模型目录；顺序固定为 DeepSeek → OpenAI → MiMo → Zhipu。
pub fn default_models() -> Vec<ModelInfo> {
    let mut models = deepseek::models();
    models.extend(openai::models());
    models.extend(mimo::models());
    models.extend(zhipu::models());
    models
}

impl ModelRequestProfile {
    /// 覆盖 profile 的图片媒体表示顺序。
    fn with_image_media(mut self, wire: MediaWireFormat, send_order: MediaSendOrder) -> Self {
        self.media = image_media_profiles(wire, send_order);
        self
    }
}

/// 图片首发表示的顺序策略；重放固定使用 DataUrl。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum MediaSendOrder {
    /// 优先发送远端 URL，失败或受限时回退 DataUrl。
    RemoteUrlFirst,
    /// 只发送 DataUrl（模型未声明远端来源时使用）。
    DataUrlOnly,
}

/// 构造图片模态的媒体表示 profile。
fn image_media_profiles(
    wire: MediaWireFormat,
    send_order: MediaSendOrder,
) -> Vec<ModelMediaInputProfile> {
    let first_send = match send_order {
        MediaSendOrder::RemoteUrlFirst => {
            vec![MediaRepresentation::RemoteUrl, MediaRepresentation::DataUrl]
        }
        MediaSendOrder::DataUrlOnly => vec![MediaRepresentation::DataUrl],
    };
    vec![ModelMediaInputProfile {
        modality: ModelModality::Image,
        wire,
        first_send,
        replay: vec![MediaRepresentation::DataUrl],
    }]
}

/// 构造「把单个字段设为字符串值」的 wire。
fn wire_set_one(path: &str, value: &str) -> ParameterWire {
    ParameterWire {
        set: vec![WireAssignment {
            path: path.to_string(),
            value: Value::String(value.to_string()),
        }],
        remove: Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;
    use crate::model::info::{MediaWireFormat, ModelTransportProfile};
    use crate::model::profile_error::ModelProfileError;

    #[test]
    fn provider_default_model_slugs_are_backed_by_default_models() {
        let models = default_models();

        for slug in deepseek_default_model_slugs()
            .iter()
            .chain(openai_default_model_slugs())
            .chain(zhipu_default_model_slugs())
            .chain(mimo_default_model_slugs())
        {
            assert!(models.iter().any(|model| model.slug == *slug));
        }
    }

    #[test]
    fn builtin_model_transport_matrix_is_explicit_for_every_supported_slug() {
        let models = default_models();
        for model in &models {
            let expected = if model.slug.starts_with("gpt-") {
                ModelTransportProfile::responses_websocket()
            } else if model.slug.starts_with("deepseek-") {
                ModelTransportProfile::responses_http()
            } else if model.slug.starts_with("glm-") || model.slug.starts_with("mimo-") {
                ModelTransportProfile::chat_completions_http()
            } else {
                continue;
            };
            assert_eq!(
                model.transport, expected,
                "unexpected transport for {}",
                model.slug
            );
        }

        for slug in openai_default_model_slugs()
            .iter()
            .chain(deepseek_default_model_slugs())
            .chain(zhipu_default_model_slugs())
            .chain(mimo_default_model_slugs())
        {
            assert!(
                models.iter().any(|model| model.slug == *slug),
                "transport matrix did not cover {slug}"
            );
        }
    }

    #[test]
    fn bundled_chat_models_opt_in_to_parallel_wire_only_when_supported() {
        let models = default_models();

        // DeepSeek 内建模型全部使用 Responses API，不再参与 Chat 的
        // `parallel_tool_calls` wire 声明。
        for slug in zhipu_default_model_slugs() {
            let model = models.iter().find(|model| model.slug == *slug).unwrap();
            assert!(
                model.request_profile.chat_parallel_tool_calls,
                "{slug} should opt in to the Chat parallel_tool_calls field"
            );
        }

        for slug in mimo_default_model_slugs() {
            let model = models.iter().find(|model| model.slug == *slug).unwrap();
            assert!(
                !model.request_profile.chat_parallel_tool_calls,
                "{slug} should omit the Chat parallel_tool_calls field"
            );
        }
    }

    #[test]
    fn builtin_model_media_contracts_are_complete_and_protocol_specific() {
        for model in default_models() {
            model.validate_media_contract().unwrap_or_else(|error| {
                panic!("invalid media contract for {}: {error}", model.slug)
            });
        }

        let mut invalid = default_models()
            .into_iter()
            .find(|model| model.slug == "glm-5.3-flash")
            .unwrap();
        invalid.request_profile.media[0].wire = MediaWireFormat::ResponsesInputImage;
        let error = invalid.validate_media_contract().unwrap_err();
        assert!(matches!(
            error,
            ModelProfileError::WireProtocolMismatch { .. }
        ));
    }
}

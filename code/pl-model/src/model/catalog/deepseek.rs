//! DeepSeek 内建模型目录：共享 reasoning family，Flash 与 Pro 路由到 Responses HTTP。

use serde_json::{Map, Value};

use crate::model::capabilities::{
    ModelCapabilities, ModelInputCapability, ModelInputLimits, ModelInputSource, ModelModality,
    PromptCacheModelCapabilities, ReasoningInterleaved, ReasoningInterleavedField,
    ToolCapabilities,
};
use crate::model::family::{ModelFamily, ModelInstanceSpec, ModelPricing};
use crate::model::info::{
    MediaWireFormat, ModelInfo, ModelRequestProfile, ModelTransportProfile, TruncationMode,
};
use crate::model::parameter::ModelParameter;

const DEEPSEEK_DEFAULT_MODEL_SLUGS: &[&str] = &[
    "deepseek-v4-flash",
    "deepseek-v4-flash-vision-exp",
    "deepseek-v4-pro",
];

pub fn deepseek_default_model_slugs() -> &'static [&'static str] {
    DEEPSEEK_DEFAULT_MODEL_SLUGS
}

pub(super) fn models() -> Vec<ModelInfo> {
    vec![
        deepseek_family().instantiate(ModelInstanceSpec {
            slug: "deepseek-v4-flash",
            display_name: "DeepSeek V4 Flash",
            description: "DeepSeek fast reasoning model with thinking mode.",
            context_window: 1_000_000,
            max_context_window: 1_000_000,
            max_output_tokens: Some(384_000),
            pricing: ModelPricing {
                currency: Some("CNY".to_string()),
                input_per_mtok: Some(1.0),
                output_per_mtok: Some(2.0),
                cache_read_per_mtok: Some(0.02),
                cache_write_per_mtok: None,
            },
        }),
        deepseek_vision_family().instantiate(ModelInstanceSpec {
            slug: "deepseek-v4-flash-vision-exp",
            display_name: "DeepSeek V4 Flash Vision Exp",
            description:
                "DeepSeek experimental multimodal reasoning model with image understanding.",
            context_window: 1_000_000,
            max_context_window: 1_000_000,
            max_output_tokens: Some(384_000),
            pricing: ModelPricing {
                currency: Some("CNY".to_string()),
                input_per_mtok: Some(1.0),
                output_per_mtok: Some(2.0),
                cache_read_per_mtok: Some(0.02),
                cache_write_per_mtok: None,
            },
        }),
        deepseek_family().instantiate(ModelInstanceSpec {
            slug: "deepseek-v4-pro",
            display_name: "DeepSeek V4 Pro",
            description: "DeepSeek flagship reasoning model with thinking mode.",
            context_window: 1_000_000,
            max_context_window: 1_000_000,
            max_output_tokens: Some(384_000),
            pricing: ModelPricing {
                currency: Some("CNY".to_string()),
                input_per_mtok: Some(3.0),
                output_per_mtok: Some(6.0),
                cache_read_per_mtok: Some(0.025),
                cache_write_per_mtok: None,
            },
        }),
    ]
}

/// DeepSeek 内建模型共享的 family 元数据；当前全部路由到 Responses HTTP。
fn deepseek_family() -> ModelFamily {
    ModelFamily {
        id: "deepseek-reasoning",
        capabilities: deepseek_capabilities(),
        truncation_mode: TruncationMode::Tokens,
        truncation_limit: 10_000,
        parameters: vec![deepseek_effort_parameter()],
        transport: ModelTransportProfile::responses_http(),
        request_profile: deepseek_request_profile(),
        base_instructions: String::new(),
    }
}

fn deepseek_vision_family() -> ModelFamily {
    ModelFamily {
        id: "deepseek-vision-reasoning",
        capabilities: deepseek_vision_capabilities(),
        truncation_mode: TruncationMode::Tokens,
        truncation_limit: 10_000,
        parameters: vec![deepseek_effort_parameter()],
        transport: ModelTransportProfile::responses_http(),
        request_profile: deepseek_request_profile().with_image_media(
            MediaWireFormat::ResponsesInputImage,
            super::MediaSendOrder::RemoteUrlFirst,
        ),
        base_instructions: String::new(),
    }
}

/// DeepSeek 固定 base body：`thinking.type = enabled`（DeepSeek 模型始终开启 thinking）。
fn deepseek_request_profile() -> ModelRequestProfile {
    let mut thinking = Map::new();
    thinking.insert("type".to_string(), Value::String("enabled".to_string()));
    let mut body = Map::new();
    body.insert("thinking".to_string(), Value::Object(thinking));
    ModelRequestProfile {
        body,
        ..ModelRequestProfile::default()
    }
}

/// DeepSeek effort：候选值按弱到强 high/max，透传到 `reasoning_effort`。
fn deepseek_effort_parameter() -> ModelParameter {
    ModelParameter {
        name: "effort".to_string(),
        label: None,
        candidates: vec!["high".to_string(), "max".to_string()],
        wire: ["high", "max"]
            .into_iter()
            .map(|value| {
                (
                    value.to_string(),
                    super::wire_set_one("reasoning_effort", value),
                )
            })
            .collect(),
    }
}

fn deepseek_capabilities() -> ModelCapabilities {
    ModelCapabilities {
        streaming: true,
        temperature: false,
        reasoning: true,
        web_search: true,
        input: vec![ModelInputCapability::text()],
        output: vec![ModelModality::Text],
        tools: ToolCapabilities {
            function_calling: true,
            parallel_tool_calls: true,
            custom_tools: false,
            freeform_tools: false,
            programmatic_tool_calling: false,
        },
        interleaved: Some(ReasoningInterleaved {
            field: ReasoningInterleavedField::ReasoningContent,
        }),
        prompt_cache: PromptCacheModelCapabilities::default(),
    }
}

fn deepseek_vision_capabilities() -> ModelCapabilities {
    let mut capabilities = deepseek_capabilities();
    capabilities.input.push(ModelInputCapability {
        modality: ModelModality::Image,
        sources: vec![ModelInputSource::Local, ModelInputSource::RemoteUrl],
        limits: ModelInputLimits {
            max_count: Some(600),
            max_bytes: Some(32 * 1024 * 1024),
            max_total_bytes: Some(32 * 1024 * 1024),
            max_width: Some(4096),
            max_height: Some(4096),
            media_types: ["image/jpeg", "image/png", "image/gif", "image/webp"]
                .into_iter()
                .map(ToString::to_string)
                .collect(),
        },
    });
    capabilities
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;
    use crate::model::capabilities::{ModelInputSource, ModelModality};
    use crate::model::info::{MediaRepresentation, MediaWireFormat};
    use crate::provider::ProviderWireProtocol;

    #[test]
    fn default_models_include_deepseek_v4_models() {
        let models = super::super::default_models();

        for &slug in deepseek_default_model_slugs() {
            let model = models.iter().find(|model| model.slug == *slug).unwrap();

            assert_eq!(model.context_window, Some(1_000_000));
            assert_eq!(model.max_output_tokens, Some(384_000));
            assert_eq!(model.currency.as_deref(), Some("CNY"));
            assert!(
                model
                    .supported_efforts()
                    .iter()
                    .any(|effort| effort == "max")
            );
        }
    }

    #[test]
    fn deepseek_default_models_declare_native_web_search_capability() {
        let models = super::super::default_models();

        for &slug in deepseek_default_model_slugs() {
            let model = models
                .iter()
                .find(|model| model.slug == slug)
                .unwrap_or_else(|| panic!("missing bundled DeepSeek model: {slug}"));
            assert_eq!(
                model.transport.protocol,
                ProviderWireProtocol::Responses,
                "DeepSeek native web search requires Responses: {slug}"
            );
            assert!(
                model.capabilities.web_search,
                "DeepSeek model must declare native web search: {slug}"
            );
            assert!(model.capabilities.tools.function_calling);
        }
    }

    #[test]
    fn deepseek_default_models_use_china_pricing() {
        let models = super::super::default_models();
        let flash = models
            .iter()
            .find(|model| model.slug == "deepseek-v4-flash")
            .unwrap();
        let vision = models
            .iter()
            .find(|model| model.slug == "deepseek-v4-flash-vision-exp")
            .unwrap();
        let pro = models
            .iter()
            .find(|model| model.slug == "deepseek-v4-pro")
            .unwrap();

        assert_eq!(flash.cache_read_price_per_mtok, Some(0.02));
        assert_eq!(flash.input_price_per_mtok, Some(1.0));
        assert_eq!(flash.output_price_per_mtok, Some(2.0));
        assert_eq!(vision.cache_read_price_per_mtok, Some(0.02));
        assert_eq!(vision.input_price_per_mtok, Some(1.0));
        assert_eq!(vision.output_price_per_mtok, Some(2.0));
        assert_eq!(pro.cache_read_price_per_mtok, Some(0.025));
        assert_eq!(pro.input_price_per_mtok, Some(3.0));
        assert_eq!(pro.output_price_per_mtok, Some(6.0));
    }

    #[test]
    fn deepseek_vision_model_has_complete_responses_image_contract() {
        let models = super::super::default_models();
        let model = models
            .into_iter()
            .find(|model| model.slug == "deepseek-v4-flash-vision-exp")
            .unwrap();

        assert_eq!(
            model
                .capabilities
                .input
                .iter()
                .map(|capability| capability.modality)
                .collect::<Vec<_>>(),
            vec![ModelModality::Text, ModelModality::Image]
        );
        let image = model
            .capabilities
            .input_capability(ModelModality::Image)
            .unwrap();
        assert_eq!(
            image.sources,
            vec![ModelInputSource::Local, ModelInputSource::RemoteUrl]
        );
        assert_eq!(image.limits.max_count, Some(600));
        assert_eq!(image.limits.max_bytes, Some(32 * 1024 * 1024));
        assert_eq!(image.limits.max_total_bytes, Some(32 * 1024 * 1024));
        assert_eq!(image.limits.max_width, Some(4096));
        assert_eq!(image.limits.max_height, Some(4096));
        assert_eq!(
            image.limits.media_types,
            ["image/jpeg", "image/png", "image/gif", "image/webp"]
        );
        let profile = model
            .request_profile
            .media_profile(ModelModality::Image)
            .unwrap();
        assert_eq!(profile.wire, MediaWireFormat::ResponsesInputImage);
        assert_eq!(
            profile.first_send,
            vec![MediaRepresentation::RemoteUrl, MediaRepresentation::DataUrl]
        );
        assert_eq!(profile.replay, vec![MediaRepresentation::DataUrl]);
        assert!(
            model
                .request_profile
                .media_profile(ModelModality::Video)
                .is_none()
        );
        assert!(
            model
                .request_profile
                .media_profile(ModelModality::File)
                .is_none()
        );
    }
}

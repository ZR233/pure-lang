use pretty_assertions::assert_eq;

use super::*;

#[test]
fn openai_default_models_match_codex_metadata() {
    let models = default_models();

    let openai_models = openai_default_model_slugs()
        .iter()
        .map(|slug| models.iter().find(|model| model.slug == *slug).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(
        openai_models
            .iter()
            .map(|model| model.slug.as_str())
            .collect::<Vec<_>>(),
        vec![
            "gpt-5.5",
            "gpt-5.4",
            "gpt-5.4-mini",
            "gpt-5.6-sol",
            "gpt-5.6-terra",
            "gpt-5.6-luna",
        ]
    );

    let gpt_55 = openai_models[0];
    assert_eq!(gpt_55.display_name, "GPT-5.5");
    assert_eq!(gpt_55.context_window, Some(272_000));
    assert_eq!(gpt_55.max_context_window, Some(272_000));
    assert_eq!(gpt_55.max_output_tokens, None);
    assert_eq!(
        gpt_55.supported_efforts(),
        vec!["medium", "low", "high", "xhigh"]
    );
    assert_eq!(gpt_55.truncation_policy.mode, TruncationMode::Tokens);
    assert!(gpt_55.capabilities.web_search);
    assert!(gpt_55.capabilities.tools.freeform_tools);

    let gpt_54 = openai_models[1];
    assert_eq!(gpt_54.display_name, "gpt-5.4");
    assert_eq!(gpt_54.context_window, Some(272_000));
    assert_eq!(gpt_54.max_context_window, Some(1_000_000));

    let gpt_56_sol = openai_models[3];
    assert_eq!(gpt_56_sol.display_name, "GPT-5.6-Sol");
    assert_eq!(gpt_56_sol.context_window, Some(272_000));
    assert_eq!(gpt_56_sol.max_context_window, Some(272_000));
    assert_eq!(gpt_56_sol.resolved_auto_compact_limit(), Some(244_800));
    assert_eq!(gpt_56_sol.max_output_tokens, None);
    assert_eq!(
        gpt_56_sol.supported_efforts(),
        vec!["low", "medium", "high", "xhigh", "max"]
    );
    assert_eq!(gpt_56_sol.default_effort().as_deref(), Some("low"));
    assert_eq!(gpt_56_sol.truncation_policy.mode, TruncationMode::Tokens);
    assert_eq!(gpt_56_sol.truncation_policy.limit, 10_000);
    assert!(gpt_56_sol.capabilities.web_search);
    assert!(gpt_56_sol.capabilities.tools.freeform_tools);

    for (model, display_name) in [
        (openai_models[4], "GPT-5.6-Terra"),
        (openai_models[5], "GPT-5.6-Luna"),
    ] {
        assert_eq!(model.display_name, display_name);
        assert_eq!(model.context_window, Some(272_000));
        assert_eq!(model.max_context_window, Some(272_000));
        assert_eq!(model.resolved_auto_compact_limit(), Some(244_800));
        assert_eq!(model.max_output_tokens, None);
        assert_eq!(
            model.supported_efforts(),
            vec!["medium", "low", "high", "xhigh", "max"]
        );
        assert_eq!(model.default_effort().as_deref(), Some("medium"));
        assert!(model.capabilities.web_search);
        assert!(model.capabilities.tools.freeform_tools);
    }

    assert!(!models.iter().any(|model| model.slug == "gpt-5.4-nano"));
    assert!(!models.iter().any(|model| model.slug == "gpt-5.3-codex"));
    assert!(!models.iter().any(|model| model.slug == "gpt-5.2"));
    assert!(!models.iter().any(|model| model.slug == "gpt-5.6"));
    assert!(!models.iter().any(|model| model.slug == "gpt-5.6-pro"));
}

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
fn default_models_include_deepseek_v4_models() {
    let models = default_models();

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
    let models = default_models();

    for &slug in deepseek_default_model_slugs() {
        let model = models
            .iter()
            .find(|model| model.slug == slug)
            .unwrap_or_else(|| panic!("missing bundled DeepSeek model: {slug}"));
        assert_eq!(
            model.transport.protocol,
            crate::ProviderWireProtocol::Responses,
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
    let models = default_models();
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
    assert_eq!(
        deepseek_default_model_slugs(),
        [
            "deepseek-v4-flash",
            "deepseek-v4-flash-vision-exp",
            "deepseek-v4-pro",
        ]
    );
    let model = default_models()
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

#[test]
fn default_models_include_zhipu_glm_models_from_official_overview() {
    let models = default_models();

    for slug in [
        "glm-5.3",
        "glm-5.3-flash",
        "glm-5.2",
        "glm-5",
        "glm-5-turbo",
        "glm-4.7",
        "glm-4.7-flashx",
        "glm-4.6",
        "glm-4.5-air",
        "glm-4.5-airx",
        "glm-4-long",
        "glm-4-flashx-250414",
        "glm-4.7-flash",
        "glm-4.5-flash",
        "glm-4-flash-250414",
        "glm-5v-turbo",
        "glm-4.6v",
        "glm-4.1v-thinking-flashx",
        "glm-4.6v-flash",
        "glm-4.1v-thinking-flash",
        "glm-4v-flash",
    ] {
        let model = models.iter().find(|model| model.slug == *slug).unwrap();

        assert!(model.context_window.is_some());
        assert!(model.max_output_tokens.is_some());
        assert_eq!(model.currency, None);
        if slug == "glm-5.2" {
            assert_eq!(model.supported_efforts(), vec!["high", "max", "none"]);
        } else if slug == "glm-5.3" || slug == "glm-5.3-flash" {
            assert_eq!(model.supported_efforts(), vec!["high", "low", "max"]);
        } else {
            assert!(
                model
                    .supported_efforts()
                    .iter()
                    .any(|effort| effort == "enabled")
            );
        }
    }

    let glm_52 = models.iter().find(|model| model.slug == "glm-5.2").unwrap();
    assert_eq!(glm_52.display_name, "GLM-5.2");
    assert_eq!(glm_52.context_window, Some(1_000_000));
    assert_eq!(glm_52.max_output_tokens, Some(128_000));

    let glm_53 = models.iter().find(|model| model.slug == "glm-5.3").unwrap();
    assert_eq!(glm_53.display_name, "GLM-5.3");
    assert_eq!(glm_53.context_window, Some(1_000_000));
    assert_eq!(glm_53.max_output_tokens, Some(128_000));
    assert_eq!(glm_53.default_effort().as_deref(), Some("high"));
    assert!(
        glm_53
            .capabilities
            .input
            .iter()
            .all(|capability| capability.modality == ModelModality::Text)
    );

    let glm_53_flash = models
        .iter()
        .find(|model| model.slug == "glm-5.3-flash")
        .unwrap();
    assert_eq!(glm_53_flash.display_name, "GLM-5.3-Flash");
    assert_eq!(glm_53_flash.context_window, Some(1_000_000));
    assert_eq!(glm_53_flash.max_output_tokens, Some(128_000));
    assert_eq!(
        glm_53_flash
            .capabilities
            .input
            .iter()
            .map(|capability| capability.modality)
            .collect::<Vec<_>>(),
        vec![ModelModality::Text, ModelModality::Image]
    );
    assert!(
        glm_53_flash
            .capabilities
            .supports_input_modality(ModelModality::Image)
    );
    let image_capability = glm_53_flash
        .capabilities
        .input_capability(ModelModality::Image)
        .unwrap();
    assert_eq!(
        image_capability.sources,
        vec![ModelInputSource::Local, ModelInputSource::RemoteUrl]
    );
    let image_profile = glm_53_flash
        .request_profile
        .media_profile(ModelModality::Image)
        .unwrap();
    assert_eq!(image_profile.wire, MediaWireFormat::ChatImageUrl);
    assert_eq!(
        image_profile.first_send,
        vec![MediaRepresentation::RemoteUrl, MediaRepresentation::DataUrl]
    );
    assert_eq!(image_profile.replay, vec![MediaRepresentation::DataUrl]);
    assert!(
        glm_53_flash
            .request_profile
            .media_profile(ModelModality::Video)
            .is_none()
    );
    assert!(
        glm_53_flash
            .request_profile
            .media_profile(ModelModality::File)
            .is_none()
    );

    let glm_5v = models
        .iter()
        .find(|model| model.slug == "glm-5v-turbo")
        .unwrap();
    assert_eq!(
        glm_5v
            .capabilities
            .input
            .iter()
            .map(|capability| capability.modality)
            .collect::<Vec<_>>(),
        vec![ModelModality::Text, ModelModality::Image]
    );
    assert!(
        glm_5v
            .capabilities
            .supports_input_modality(ModelModality::Image)
    );
}

#[test]
fn builtin_model_media_contracts_are_complete_and_protocol_specific() {
    for model in default_models() {
        model
            .validate_media_contract()
            .unwrap_or_else(|error| panic!("invalid media contract for {}: {error}", model.slug));
    }

    let mut invalid = default_models()
        .into_iter()
        .find(|model| model.slug == "glm-5.3-flash")
        .unwrap();
    invalid.request_profile.media[0].wire = MediaWireFormat::ResponsesInputImage;
    let error = invalid.validate_media_contract().unwrap_err();
    assert!(error.contains("does not match transport"));
}

#[test]
fn zhipu_default_model_list_excludes_phasing_out_glm_45_flash() {
    assert_eq!(
        zhipu_default_model_slugs(),
        [
            "glm-5.3",
            "glm-5.3-flash",
            "glm-5.2",
            "glm-5",
            "glm-5-turbo",
            "glm-4.7",
            "glm-4.7-flashx",
            "glm-4.7-flash"
        ]
    );
    assert!(!zhipu_default_model_slugs().contains(&"glm-4.5-flash"));
    assert!(!zhipu_default_model_slugs().contains(&"glm-5v-turbo"));
    assert!(
        default_models()
            .iter()
            .any(|model| model.slug == "glm-4.5-flash")
    );
}

#[test]
fn glm52_effort_wire_links_reasoning_effort_and_thinking() {
    let models = default_models();
    let glm52 = models.iter().find(|model| model.slug == "glm-5.2").unwrap();
    let param = glm52.effort_parameter().unwrap();

    // high：联动 reasoning_effort + thinking.type=enabled + clear_thinking=false
    let high = param.wire_for("high").unwrap();
    let mut body = serde_json::Map::new();
    high.apply_to(&mut body);
    assert_eq!(body["reasoning_effort"], serde_json::json!("high"));
    assert_eq!(body["thinking"]["type"], serde_json::json!("enabled"));
    assert_eq!(body["thinking"]["clear_thinking"], serde_json::json!(false));

    // none：移除 reasoning_effort，thinking.type=disabled
    let mut body = serde_json::Map::new();
    body.insert("reasoning_effort".to_string(), serde_json::json!("high"));
    let none = param.wire_for("none").unwrap();
    none.apply_to(&mut body);
    assert!(!body.contains_key("reasoning_effort"));
    assert_eq!(body["thinking"]["type"], serde_json::json!("disabled"));
}

#[test]
fn glm53_effort_wire_always_enables_thinking() {
    let models = default_models();
    for slug in ["glm-5.3", "glm-5.3-flash"] {
        let model = models.iter().find(|model| model.slug == slug).unwrap();
        let param = model.effort_parameter().unwrap();

        assert_eq!(param.candidates, vec!["high", "low", "max"]);
        // GLM-5.3 系列不支持禁用思考：官方文档明确 disabled 会报错
        assert!(param.wire_for("none").is_none());

        for effort in ["high", "low", "max"] {
            let wire = param.wire_for(effort).unwrap();
            let mut body = serde_json::Map::new();
            wire.apply_to(&mut body);
            assert_eq!(body["reasoning_effort"], serde_json::json!(effort));
            assert_eq!(body["thinking"]["type"], serde_json::json!("enabled"));
            assert_eq!(body["thinking"]["clear_thinking"], serde_json::json!(false));
        }
    }
}

#[test]
fn deepseek_request_includes_base_body_thinking() {
    let profile = deepseek_request_profile();
    assert_eq!(
        profile.body["thinking"]["type"],
        serde_json::json!("enabled")
    );
}

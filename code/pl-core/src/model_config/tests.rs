use std::collections::BTreeMap;

use pl_model::{ModelInfo, ProviderConnectionMode, ProviderInfo};
use pretty_assertions::assert_eq;

use super::*;

fn test_config() -> AgentModelConfig {
    let provider_id = ProviderId::new("deepseek").unwrap();
    let role_id = AgentRoleId::new("custom_executor").unwrap();
    let provider = ProviderConfig::deepseek_preset();
    AgentModelConfig {
        providers: BTreeMap::from([(provider_id.clone(), provider)]),
        routes: BTreeMap::from([(
            role_id,
            ModelRouteConfig {
                provider: provider_id,
                model: "deepseek-v4-flash".to_string(),
                reasoning_effort: Some(ReasoningEffort::new("high")),
            },
        )]),
    }
}

#[test]
fn resolve_uses_route_as_the_only_default_model_source() {
    let config = test_config();
    let role = AgentRoleId::new("custom_executor").unwrap();

    let resolved = config.resolve(&role).unwrap();

    assert_eq!(resolved.provider_id, ProviderId::new("deepseek").unwrap());
    assert_eq!(resolved.model.slug, "deepseek-v4-flash");
    assert_eq!(resolved.provider_info.default_model, "deepseek-v4-flash");
    assert_eq!(
        resolved.reasoning_effort,
        Some(ReasoningEffort::new("high"))
    );
}

#[test]
fn empty_ids_are_rejected_during_construction_and_deserialization() {
    assert!(ProviderId::new("  ").is_err());
    assert!(AgentRoleId::new("").is_err());
    assert!(serde_json::from_str::<ProviderId>(r#"""#).is_err());
}

#[test]
fn validate_rejects_missing_provider_model_and_effort() {
    let role = AgentRoleId::new("custom_executor").unwrap();
    let mut config = test_config();
    config.routes.get_mut(&role).unwrap().provider = ProviderId::new("missing").unwrap();
    assert!(
        config
            .validate()
            .unwrap_err()
            .to_string()
            .contains("missing provider")
    );

    let mut config = test_config();
    config.routes.get_mut(&role).unwrap().model = "missing".to_string();
    assert!(
        config
            .validate()
            .unwrap_err()
            .to_string()
            .contains("missing model")
    );

    let mut config = test_config();
    config.routes.get_mut(&role).unwrap().reasoning_effort =
        Some(ReasoningEffort::new("unsupported"));
    assert!(
        config
            .validate()
            .unwrap_err()
            .to_string()
            .contains("unsupported effort")
    );
}

#[test]
fn provider_runtime_info_is_created_only_after_route_selects_model() {
    let provider = ProviderConfig::from_provider_info(
        ProviderInfo::deepseek(None),
        ProviderConfig::deepseek_preset()
            .effective_models()
            .unwrap(),
    );
    let info = provider.to_provider_info("deepseek-v4-pro").unwrap();

    assert_eq!(info.default_model, "deepseek-v4-pro");
}

#[test]
fn bundled_catalog_rejects_additional_model_slug_conflicts() {
    let mut provider = ProviderConfig::deepseek_preset();
    let ProviderModelCatalogConfig::Bundled {
        additional_models, ..
    } = &mut provider.catalog
    else {
        panic!("builtin preset must use a bundled catalog");
    };
    additional_models.push(ModelInfo::fallback("deepseek-v4-flash"));

    assert!(
        provider
            .effective_models()
            .unwrap_err()
            .to_string()
            .contains("additional model conflicts with bundled model")
    );
}

#[test]
fn non_openai_provider_rejects_websocket_connection_mode() {
    let mut config = test_config();
    let provider = config.providers.values_mut().next().unwrap();
    provider.set_connection_mode(ProviderConnectionMode::WebSocket);

    assert!(
        config
            .validate()
            .unwrap_err()
            .to_string()
            .contains("does not support WebSocket")
    );
}

#[test]
fn same_preset_can_back_multiple_independent_provider_instances() {
    let preset = builtin_provider_catalog()
        .presets
        .into_iter()
        .find(|preset| preset.id.as_str() == "openai")
        .unwrap();
    let mut websocket = preset.provider.clone();
    websocket.bearer_token = Some("ws-secret".to_string());
    let mut http = preset
        .provider
        .with_connection_mode(ProviderConnectionMode::Http);
    http.bearer_token = Some("http-secret".to_string());
    let websocket_id = ProviderId::new("openai-primary").unwrap();
    let http_id = ProviderId::new("openai-proxy").unwrap();
    let config = AgentModelConfig {
        providers: BTreeMap::from([(websocket_id.clone(), websocket), (http_id.clone(), http)]),
        routes: BTreeMap::from([
            (
                AgentRoleId::new("executor").unwrap(),
                ModelRouteConfig {
                    provider: websocket_id.clone(),
                    model: "gpt-5.6-sol".to_string(),
                    reasoning_effort: None,
                },
            ),
            (
                AgentRoleId::new("reviewer").unwrap(),
                ModelRouteConfig {
                    provider: http_id.clone(),
                    model: "gpt-5.6-sol".to_string(),
                    reasoning_effort: None,
                },
            ),
        ]),
    };

    config.validate().unwrap();
    assert_eq!(
        config.providers[&websocket_id].connection_mode(),
        ProviderConnectionMode::WebSocket
    );
    assert_eq!(
        config.providers[&http_id].connection_mode(),
        ProviderConnectionMode::Http
    );
    assert_eq!(
        config.providers[&websocket_id].bearer_token.as_deref(),
        Some("ws-secret")
    );
    assert_eq!(
        config.providers[&http_id].bearer_token.as_deref(),
        Some("http-secret")
    );
}

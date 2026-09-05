use std::collections::BTreeMap;

use pretty_assertions::assert_eq;

use pl_core::{
    AgentModelConfig, AgentRoleId, ModelRouteConfig, ProviderCapabilitySelection, ProviderConfig,
    ProviderId, ProviderModelCatalogConfig, ReasoningEffort, builtin_provider_catalog,
};
use pl_model::model::ModelInfo;
use pl_model::provider::{ProviderConnectionMode, ProviderEndpoint};

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
                effort: Some(ReasoningEffort::new("high")),
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
    assert_eq!(resolved.endpoint.name, "DeepSeek");
    assert_eq!(resolved.effort, Some(ReasoningEffort::new("high")));
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
    config.routes.get_mut(&role).unwrap().effort = Some(ReasoningEffort::new("unsupported"));
    assert!(
        config
            .validate()
            .unwrap_err()
            .to_string()
            .contains("unsupported effort")
    );
}

#[test]
fn route_resolution_binds_one_model_to_the_endpoint() {
    let provider = ProviderConfig::from_explicit_models(
        ProviderEndpoint::deepseek(None),
        ProviderConfig::deepseek_preset()
            .effective_models()
            .unwrap(),
    );
    let provider_id = ProviderId::new("deepseek").unwrap();
    let role = AgentRoleId::new("executor").unwrap();
    let config = AgentModelConfig {
        providers: BTreeMap::from([(provider_id.clone(), provider)]),
        routes: BTreeMap::from([(
            role.clone(),
            ModelRouteConfig {
                provider: provider_id,
                model: "deepseek-v4-pro".to_string(),
                effort: Some(ReasoningEffort::new("high")),
            },
        )]),
    };

    let resolved = config.resolve(&role).unwrap();
    assert_eq!(resolved.endpoint.name, "DeepSeek");
    assert_eq!(resolved.model.slug, "deepseek-v4-pro");
    assert_eq!(
        resolved.model.binding.transport.protocol,
        pl_model::provider::ProviderWireProtocol::Responses
    );
    assert_eq!(
        resolved.model.binding.transport.default_connection_mode,
        ProviderConnectionMode::Http
    );
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
    additional_models.push(ModelInfo::compatible("deepseek-v4-flash"));

    assert!(
        provider
            .effective_models()
            .unwrap_err()
            .to_string()
            .contains("additional model conflicts with bundled model")
    );
}

#[test]
fn model_rejects_unsupported_websocket_connection_mode() {
    let mut config = test_config();
    let provider = config.providers.values_mut().next().unwrap();
    assert!(
        provider
            .set_model_connection_mode("deepseek-v4-flash", ProviderConnectionMode::WebSocket,)
            .unwrap_err()
            .to_string()
            .contains("does not support connection mode")
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
    let mut http = preset.provider;
    http.set_model_connection_mode("gpt-5.6-sol", ProviderConnectionMode::Http)
        .unwrap();
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
                    effort: Some(ReasoningEffort::new("low")),
                },
            ),
            (
                AgentRoleId::new("reviewer").unwrap(),
                ModelRouteConfig {
                    provider: http_id.clone(),
                    model: "gpt-5.6-sol".to_string(),
                    effort: Some(ReasoningEffort::new("low")),
                },
            ),
        ]),
    };

    config.validate().unwrap();
    let executor = config
        .resolve(&AgentRoleId::new("executor").unwrap())
        .unwrap();
    let reviewer = config
        .resolve(&AgentRoleId::new("reviewer").unwrap())
        .unwrap();
    assert_eq!(
        executor.model.binding.transport.default_connection_mode,
        ProviderConnectionMode::WebSocket
    );
    assert_eq!(
        reviewer.model.binding.transport.default_connection_mode,
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

#[test]
fn custom_openai_endpoint_requires_explicit_programmatic_tool_capability() {
    let preset = builtin_provider_catalog()
        .presets
        .into_iter()
        .find(|preset| preset.id.as_str() == "openai")
        .unwrap();
    let official_capabilities = preset.service_capabilities;
    let mut provider = preset.provider;
    provider.base_url = "https://responses-proxy.example/v1".to_string();

    let default_capabilities = provider.service_capabilities().unwrap();
    assert!(
        !default_capabilities
            .responses_tools
            .programmatic_tool_calling
    );
    assert!(!default_capabilities.web_search.hosted_responses);

    provider.capabilities = ProviderCapabilitySelection::Explicit(official_capabilities);
    let explicit_capabilities = provider.service_capabilities().unwrap();
    assert!(
        explicit_capabilities
            .responses_tools
            .programmatic_tool_calling
    );
    assert!(explicit_capabilities.web_search.hosted_responses);
}

#[test]
fn custom_deepseek_base_url_disables_inherited_hosted_search_only() {
    let mut provider = ProviderConfig::deepseek_preset();
    let inherited = provider.service_capabilities().unwrap();
    assert!(inherited.web_search.hosted_responses);
    assert_eq!(
        inherited.web_search.hosted_dialect,
        pl_protocol::HostedWebSearchDialect::DeepSeekResponses
    );

    provider.base_url = "https://deepseek-proxy.example/v1".to_string();
    let overridden = provider.service_capabilities().unwrap();

    assert!(!overridden.web_search.hosted_responses);
    assert_eq!(
        overridden.prompt_cache.dialect,
        pl_model::provider::PromptCacheDialect::ImplicitPrefix
    );
}

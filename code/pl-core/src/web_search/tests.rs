use pl_model::{ModelInfo, ProviderInfo, ProviderServiceCapabilities, WebSearchConfig};
use pretty_assertions::assert_eq;

use super::*;
use crate::{
    AgentRoleId, ModelRouteConfig, ProviderCapabilitySelection, ProviderConfig,
    ProviderModelCatalogConfig, ProviderTransportSelection,
};

fn provider_id(value: &str) -> ProviderId {
    ProviderId::new(value).unwrap()
}

fn role_id(value: &str) -> AgentRoleId {
    AgentRoleId::new(value).unwrap()
}

fn models_with_current(provider: ProviderConfig, model: ModelInfo) -> AgentModelConfig {
    let current = provider_id("current");
    AgentModelConfig {
        providers: [(current.clone(), provider)].into_iter().collect(),
        routes: [(
            role_id("executor"),
            ModelRouteConfig {
                provider: current,
                model: model.slug,
                reasoning_effort: None,
            },
        )]
        .into_iter()
        .collect(),
    }
}

fn openai_provider_with_secret() -> ProviderConfig {
    let mut provider = crate::builtin_provider_catalog()
        .presets
        .into_iter()
        .find(|preset| preset.id.as_str() == "openai")
        .unwrap()
        .provider;
    provider.bearer_token = Some("secret".to_string());
    provider.bearer_token_env = None;
    provider
}

fn custom_responses_provider(model: ModelInfo, bearer_token: Option<&str>) -> ProviderConfig {
    let mut info = ProviderInfo::responses_compatible(
        "Responses-compatible",
        "https://responses.example/v1",
        &model.slug,
    );
    info.bearer_token = bearer_token.map(str::to_string);
    info.service_capabilities = ProviderServiceCapabilities::openai_web_search();
    ProviderConfig::from_provider_info(info, vec![model])
}

#[test]
fn preset_capabilities_resolve_without_provider_identity_checks() {
    let mut preset = openai_provider_with_secret();
    preset.name = "Renamed proxy".to_string();
    preset.base_url = "https://proxy.example/v1".to_string();
    let model = preset.effective_models().unwrap().remove(0);
    let models = models_with_current(preset, model);
    let route = models.resolve(&role_id("executor")).unwrap();

    let plan = plan_web_search(&models, &route, &WebSearchConfig::default()).unwrap();

    assert_eq!(plan.resolution.path, Some(WebSearchPath::Standalone));
    assert_eq!(plan.visibility, ToolVisibilityConstraint::Additive);
}

#[test]
fn explicit_override_can_disable_preset_capabilities() {
    let mut provider = openai_provider_with_secret();
    provider.capabilities =
        ProviderCapabilitySelection::Explicit(ProviderServiceCapabilities::default());
    let model = provider.effective_models().unwrap().remove(0);
    let models = models_with_current(provider, model);
    let route = models.resolve(&role_id("executor")).unwrap();

    let plan = plan_web_search(&models, &route, &WebSearchConfig::default()).unwrap();

    assert_eq!(
        plan.resolution.availability,
        WebSearchAvailability::ProviderUnsupported
    );
}

#[test]
fn custom_capability_uses_same_standalone_planner() {
    let mut info = ProviderInfo::responses_compatible(
        "Future provider",
        "https://future.example/v1",
        "future-model",
    );
    info.bearer_token = Some("secret".to_string());
    info.service_capabilities = ProviderServiceCapabilities::openai_web_search();
    let mut model = ModelInfo::fallback("future-model");
    model.capabilities.tools.function_calling = true;
    let provider = ProviderConfig::from_provider_info(info, vec![model.clone()]);
    assert!(matches!(
        provider.transport,
        ProviderTransportSelection::Custom { .. }
    ));
    assert!(matches!(
        provider.catalog,
        ProviderModelCatalogConfig::Explicit { .. }
    ));
    let models = models_with_current(provider, model);
    let route = models.resolve(&role_id("executor")).unwrap();

    let plan = plan_web_search(&models, &route, &WebSearchConfig::default()).unwrap();

    assert_eq!(plan.resolution.path, Some(WebSearchPath::Standalone));
}

#[test]
fn hosted_path_is_exclusive_when_function_tools_are_unavailable() {
    let mut model = ModelInfo::fallback("hosted-only-model");
    model.capabilities.web_search = true;
    model.capabilities.tools.function_calling = false;
    let provider = custom_responses_provider(model.clone(), Some("secret"));
    let models = models_with_current(provider, model);
    let route = models.resolve(&role_id("executor")).unwrap();

    let plan = plan_web_search(&models, &route, &WebSearchConfig::default()).unwrap();

    assert_eq!(plan.resolution.path, Some(WebSearchPath::Hosted));
    assert_eq!(plan.visibility, ToolVisibilityConstraint::Exclusive);
    assert_eq!(
        plan.constrain_visibility(crate::ToolVisibilitySet::from_tool_names([
            "spawn_agent",
            "read_file",
        ]))
        .into_names(),
        std::collections::BTreeSet::from([crate::tool::TOOL_WEB_SEARCH.to_string()])
    );
}

#[test]
fn standalone_backend_can_be_selected_from_another_routed_provider() {
    let mut current_model = ModelInfo::fallback("current-model");
    current_model.capabilities.tools.function_calling = true;
    let current = ProviderConfig::from_provider_info(
        ProviderInfo::openai_compatible_chat(
            "Current",
            "https://current.example/v1",
            &current_model.slug,
        ),
        vec![current_model.clone()],
    );
    let search = openai_provider_with_secret();
    let search_model = search.effective_models().unwrap().remove(0);
    let current_id = provider_id("current");
    let search_id = provider_id("search");
    let models = AgentModelConfig {
        providers: [(current_id.clone(), current), (search_id.clone(), search)]
            .into_iter()
            .collect(),
        routes: [
            (
                role_id("executor"),
                ModelRouteConfig {
                    provider: current_id,
                    model: current_model.slug,
                    reasoning_effort: None,
                },
            ),
            (
                role_id("search"),
                ModelRouteConfig {
                    provider: search_id.clone(),
                    model: search_model.slug.clone(),
                    reasoning_effort: None,
                },
            ),
        ]
        .into_iter()
        .collect(),
    };
    let route = models.resolve(&role_id("executor")).unwrap();

    let plan = plan_web_search(&models, &route, &WebSearchConfig::default()).unwrap();

    assert_eq!(plan.resolution.path, Some(WebSearchPath::Standalone));
    assert_eq!(plan.resolution.provider_id, Some(search_id));
    assert_eq!(plan.resolution.model, Some(search_model.slug));
}

#[test]
fn planner_distinguishes_disabled_missing_credential_and_model_support() {
    let mut provider = openai_provider_with_secret();
    let mut model = provider.effective_models().unwrap().remove(0);
    let models = models_with_current(provider.clone(), model.clone());
    let route = models.resolve(&role_id("executor")).unwrap();
    let disabled = WebSearchConfig {
        mode: pl_model::WebSearchMode::Disabled,
        ..WebSearchConfig::default()
    };
    assert_eq!(
        plan_web_search(&models, &route, &disabled)
            .unwrap()
            .resolution
            .availability,
        WebSearchAvailability::Disabled
    );

    provider.bearer_token = None;
    provider.bearer_token_env = None;
    let models = models_with_current(provider.clone(), model.clone());
    let route = models.resolve(&role_id("executor")).unwrap();
    assert_eq!(
        plan_web_search(&models, &route, &WebSearchConfig::default())
            .unwrap()
            .resolution
            .availability,
        WebSearchAvailability::MissingCredential
    );

    model.capabilities.web_search = false;
    model.capabilities.tools.function_calling = false;
    provider = custom_responses_provider(model.clone(), Some("secret"));
    let models = models_with_current(provider, model);
    let route = models.resolve(&role_id("executor")).unwrap();
    assert_eq!(
        plan_web_search(&models, &route, &WebSearchConfig::default())
            .unwrap()
            .resolution
            .availability,
        WebSearchAvailability::ModelUnsupported
    );
}

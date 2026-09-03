use pretty_assertions::assert_eq;

use super::*;
use crate::{
    AgentRoleId, ModelRouteConfig, ProviderCapabilitySelection, ProviderConfig,
    ProviderModelCatalogConfig, ReasoningEffort,
};
use pl_model::completion::WebSearchConfig;
use pl_model::model::{ModelInfo, ModelTransportProfile};
use pl_model::provider::{
    ProviderEndpoint, ProviderServiceCapabilities, WebSearchProviderCapabilities,
};

fn provider_id(value: &str) -> ProviderId {
    ProviderId::new(value).unwrap()
}

fn role_id(value: &str) -> AgentRoleId {
    AgentRoleId::new(value).unwrap()
}

fn models_with_current(provider: ProviderConfig, model: ModelInfo) -> AgentModelConfig {
    let current = provider_id("current");
    let effort = model.default_effort().map(ReasoningEffort::new);
    AgentModelConfig {
        providers: [(current.clone(), provider)].into_iter().collect(),
        routes: [(
            role_id("executor"),
            ModelRouteConfig {
                provider: current,
                model: model.slug,
                effort,
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
    let mut info =
        ProviderEndpoint::compatible("Responses-compatible", "https://responses.example/v1");
    info.bearer_token = bearer_token.map(str::to_string);
    info.service_capabilities = ProviderServiceCapabilities::openai_web_search();
    ProviderConfig::from_explicit_models(info, vec![model])
}

fn deepseek_provider_with_hosted_search() -> (ProviderConfig, ModelInfo) {
    let mut provider = ProviderConfig::deepseek_preset();
    provider.bearer_token = Some("deepseek-secret".to_string());
    provider.bearer_token_env = None;
    provider.capabilities = ProviderCapabilitySelection::Explicit(ProviderServiceCapabilities {
        web_search: WebSearchProviderCapabilities {
            hosted_responses: true,
            hosted_dialect: pl_protocol::HostedWebSearchDialect::DeepSeekResponses,
            standalone: None,
        },
        ..ProviderServiceCapabilities::default()
    });
    let mut model = provider.effective_models().unwrap().remove(0);
    model.capabilities.web_search = true;
    (provider, model)
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
    let mut info = ProviderEndpoint::compatible("Future provider", "https://future.example/v1");
    info.bearer_token = Some("secret".to_string());
    info.service_capabilities = ProviderServiceCapabilities::openai_web_search();
    let mut model = ModelInfo::fallback("future-model");
    model.capabilities.tools.function_calling = true;
    let provider = ProviderConfig::from_explicit_models(info, vec![model.clone()]);
    assert!(provider.preset.is_none());
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
    model.transport = ModelTransportProfile::responses_http();
    model.capabilities.web_search = true;
    model.capabilities.tools.function_calling = false;
    let provider = custom_responses_provider(model.clone(), Some("secret"));
    let models = models_with_current(provider, model);
    let route = models.resolve(&role_id("executor")).unwrap();

    let plan = plan_web_search(&models, &route, &WebSearchConfig::default()).unwrap();

    assert_eq!(plan.resolution.path, Some(WebSearchPath::Hosted));
    assert_eq!(plan.visibility, ToolVisibilityConstraint::Exclusive);
}

#[test]
fn standalone_backend_can_be_selected_from_another_routed_provider() {
    let mut current_model = ModelInfo::fallback("current-model");
    current_model.capabilities.tools.function_calling = true;
    let current = ProviderConfig::from_explicit_models(
        ProviderEndpoint::compatible("Current", "https://current.example/v1"),
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
                    effort: None,
                },
            ),
            (
                role_id("search"),
                ModelRouteConfig {
                    provider: search_id.clone(),
                    model: search_model.slug.clone(),
                    effort: None,
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
fn deepseek_hosted_search_is_preferred_and_coexists_with_function_tools() {
    let (deepseek, deepseek_model) = deepseek_provider_with_hosted_search();
    let openai = openai_provider_with_secret();
    let openai_model = openai.effective_models().unwrap().remove(0);
    let deepseek_id = provider_id("deepseek");
    let openai_id = provider_id("openai");
    let models = AgentModelConfig {
        providers: [(deepseek_id.clone(), deepseek), (openai_id.clone(), openai)]
            .into_iter()
            .collect(),
        routes: [
            (
                role_id("executor"),
                ModelRouteConfig {
                    provider: deepseek_id.clone(),
                    model: deepseek_model.slug.clone(),
                    effort: deepseek_model.default_effort().map(ReasoningEffort::new),
                },
            ),
            (
                role_id("search"),
                ModelRouteConfig {
                    provider: openai_id,
                    model: openai_model.slug,
                    effort: None,
                },
            ),
        ]
        .into_iter()
        .collect(),
    };
    let route = models.resolve(&role_id("executor")).unwrap();

    let plans = plan_web_searches(&models, &route, &WebSearchConfig::default(), true).unwrap();
    assert_eq!(plans.selected, Some(WebSearchBackendKind::DeepSeek));
    let plan = plans.deepseek;

    assert_eq!(plan.resolution.path, Some(WebSearchPath::Hosted));
    assert_eq!(plan.resolution.provider_id, Some(deepseek_id));
    assert_eq!(plan.visibility, ToolVisibilityConstraint::Additive);
}

#[test]
fn planner_distinguishes_disabled_missing_credential_and_model_support() {
    let mut provider = openai_provider_with_secret();
    let mut model = provider.effective_models().unwrap().remove(0);
    let models = models_with_current(provider.clone(), model.clone());
    let route = models.resolve(&role_id("executor")).unwrap();
    let disabled = WebSearchConfig {
        mode: pl_model::completion::WebSearchMode::Disabled,
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

#[test]
fn disabled_deepseek_search_falls_back_to_openai_search() {
    let (deepseek, deepseek_model) = deepseek_provider_with_hosted_search();
    let openai = openai_provider_with_secret();
    let openai_model = openai.effective_models().unwrap().remove(0);
    let deepseek_id = provider_id("deepseek");
    let openai_id = provider_id("openai");
    let models = AgentModelConfig {
        providers: [(deepseek_id.clone(), deepseek), (openai_id.clone(), openai)]
            .into_iter()
            .collect(),
        routes: [
            (
                role_id("executor"),
                ModelRouteConfig {
                    provider: deepseek_id,
                    model: deepseek_model.slug.clone(),
                    effort: deepseek_model.default_effort().map(ReasoningEffort::new),
                },
            ),
            (
                role_id("search"),
                ModelRouteConfig {
                    provider: openai_id.clone(),
                    model: openai_model.slug,
                    effort: None,
                },
            ),
        ]
        .into_iter()
        .collect(),
    };
    let route = models.resolve(&role_id("executor")).unwrap();

    let plans = plan_web_searches(&models, &route, &WebSearchConfig::default(), false).unwrap();

    assert_eq!(plans.selected, Some(WebSearchBackendKind::OpenAi));
    assert_eq!(
        plans.deepseek.resolution.availability,
        WebSearchAvailability::Disabled
    );
    assert_eq!(plans.openai.resolution.provider_id, Some(openai_id));
}

#[test]
fn deepseek_search_reports_missing_credential_and_unsupported_model() {
    let (mut provider, model) = deepseek_provider_with_hosted_search();
    provider.bearer_token = None;
    let models = models_with_current(provider, model.clone());
    let route = models.resolve(&role_id("executor")).unwrap();
    let missing = plan_web_searches(&models, &route, &WebSearchConfig::default(), true).unwrap();
    assert_eq!(
        missing.deepseek.resolution.availability,
        WebSearchAvailability::MissingCredential
    );
    assert_eq!(missing.selected, None);

    let (provider, mut model) = deepseek_provider_with_hosted_search();
    model.capabilities.web_search = false;
    let provider =
        ProviderConfig::from_explicit_models(provider.to_endpoint().unwrap(), vec![model.clone()]);
    let models = models_with_current(provider, model);
    let route = models.resolve(&role_id("executor")).unwrap();
    let unsupported =
        plan_web_searches(&models, &route, &WebSearchConfig::default(), true).unwrap();
    assert_eq!(
        unsupported.deepseek.resolution.availability,
        WebSearchAvailability::ModelUnsupported
    );
    assert_eq!(unsupported.selected, None);
}

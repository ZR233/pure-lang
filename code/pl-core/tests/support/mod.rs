#![allow(dead_code)]

use pl_core::{
    AgentRoleId, ModelInfo, ProviderEndpoint, ProviderId, ReasoningEffort, ResolvedModelRoute,
    deepseek_default_model_slugs, default_models,
};

pub fn catalog_model(slug: &str) -> ModelInfo {
    default_models()
        .into_iter()
        .find(|model| model.slug == slug)
        .unwrap_or_else(|| ModelInfo::fallback(slug))
}

pub fn route(
    provider_id: &str,
    endpoint: ProviderEndpoint,
    model: ModelInfo,
    effort: Option<&str>,
) -> ResolvedModelRoute {
    ResolvedModelRoute {
        role: AgentRoleId::new("live-test").expect("static role id is valid"),
        provider_id: ProviderId::new(provider_id).expect("static provider id is valid"),
        endpoint,
        model,
        effort: effort.map(ReasoningEffort::new),
    }
}

pub fn deepseek_route(api_key: String) -> ResolvedModelRoute {
    let mut endpoint = ProviderEndpoint::deepseek(None);
    endpoint.bearer_token = Some(api_key);
    let model = catalog_model(deepseek_default_model_slugs()[0]);
    route("deepseek", endpoint, model, Some("high"))
}

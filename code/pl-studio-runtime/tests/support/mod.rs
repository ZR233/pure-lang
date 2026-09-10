//! Shared route construction for explicit provider-backed integration acceptance.

pub fn deepseek_route(api_key: String) -> pl_model::config::ResolvedModelRoute {
    use pl_model::config::{AgentRoleId, ProviderId, ReasoningEffort, ResolvedModelRoute};
    let mut endpoint = pl_model::provider::ProviderEndpoint::deepseek(None);
    endpoint.bearer_token = Some(api_key);
    let slug = pl_model::model::deepseek_default_model_slugs()[0];
    let model = pl_model::model::default_models()
        .into_iter()
        .find(|model| model.slug == slug)
        .expect("published DeepSeek model");
    ResolvedModelRoute {
        pricing_mode: pl_protocol::PricingMode::Catalog,
        role: AgentRoleId::new("live-test").unwrap(),
        provider_id: ProviderId::new("deepseek").unwrap(),
        endpoint,
        model,
        effort: Some(ReasoningEffort::new("high")),
    }
}

pub mod engine;

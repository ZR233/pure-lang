//! 产品无关的模型、Provider 与动态角色路由配置。

mod catalog;
mod id;
mod provider;
mod route;

#[cfg(test)]
mod tests;

pub use catalog::{
    ModelCatalog, ProviderCatalogRegistry, ProviderConnectionPolicy, ProviderPreset,
    builtin_model_catalog, builtin_provider_catalog, provider_connection_mode_descriptors,
    provider_connection_modes, provider_service_capabilities_descriptor,
};
pub use id::{AgentRoleId, ModelCatalogId, ProviderId, ProviderPresetId};
pub use provider::{
    ProviderCapabilitySelection, ProviderConfig, ProviderModelCatalogConfig,
    ProviderTransportSelection,
};
pub use route::{AgentModelConfig, ModelRouteConfig, ReasoningEffort, ResolvedModelRoute};

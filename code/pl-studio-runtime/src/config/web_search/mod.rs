use pl_core::{ProviderId, ResolvedModelRoute};
use pl_model::{ProviderInfo, ProviderWireProtocol, WebSearchMode};
use serde::{Deserialize, Serialize};

use super::{StudioConfig, StudioRole};

/// Studio 设置页和 turn 规划共用的 Web 搜索可用性。
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum StudioWebSearchAvailability {
    Available,
    Disabled,
    MissingCredential,
    UnsupportedModel,
}

/// 当前 turn 实际采用的 Web 搜索路径。
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum StudioWebSearchPath {
    Standalone,
    Hosted,
}

/// 已解析且可安全创建 `/alpha/search` 客户端的 OpenAI backend。
#[derive(Debug, Clone)]
pub struct StudioWebSearchBackend {
    pub provider_id: ProviderId,
    pub provider_info: ProviderInfo,
    pub model: String,
    pub max_output_tokens: Option<u64>,
}

/// Web 搜索 configured/effective 状态和自动路由结果。
#[derive(Debug, Clone)]
pub struct StudioWebSearchResolution {
    pub configured_mode: WebSearchMode,
    pub effective_mode: WebSearchMode,
    pub availability: StudioWebSearchAvailability,
    pub path: Option<StudioWebSearchPath>,
    pub backend: Option<StudioWebSearchBackend>,
}

impl StudioWebSearchResolution {
    fn unavailable(
        configured_mode: WebSearchMode,
        availability: StudioWebSearchAvailability,
    ) -> Self {
        Self {
            configured_mode,
            effective_mode: WebSearchMode::Disabled,
            availability,
            path: None,
            backend: None,
        }
    }
}

/// 每个 turn 重新解析 OpenAI 账户和双路径规划，凭证变化无需重启。
pub fn resolve_web_search(
    config: &StudioConfig,
    current_route: &ResolvedModelRoute,
) -> StudioWebSearchResolution {
    let configured_mode = config.web_search.mode;
    if configured_mode.is_disabled() {
        return StudioWebSearchResolution::unavailable(
            configured_mode,
            StudioWebSearchAvailability::Disabled,
        );
    }

    let Some(backend) = select_backend(config, current_route) else {
        return StudioWebSearchResolution::unavailable(
            configured_mode,
            StudioWebSearchAvailability::MissingCredential,
        );
    };

    let path = if current_route.model.capabilities.supports_function_calling() {
        Some(StudioWebSearchPath::Standalone)
    } else if backend.provider_id == current_route.provider_id
        && current_route.provider_info.protocol == ProviderWireProtocol::Responses
        && current_route.model.capabilities.supports_web_search()
    {
        Some(StudioWebSearchPath::Hosted)
    } else {
        None
    };

    let Some(path) = path else {
        return StudioWebSearchResolution {
            configured_mode,
            effective_mode: WebSearchMode::Disabled,
            availability: StudioWebSearchAvailability::UnsupportedModel,
            path: None,
            backend: Some(backend),
        };
    };

    StudioWebSearchResolution {
        configured_mode,
        effective_mode: configured_mode,
        availability: StudioWebSearchAvailability::Available,
        path: Some(path),
        backend: Some(backend),
    }
}

fn select_backend(
    config: &StudioConfig,
    current_route: &ResolvedModelRoute,
) -> Option<StudioWebSearchBackend> {
    if let Some(provider) = config.models.providers.get(&current_route.provider_id)
        && is_credentialed_openai(provider)
    {
        return Some(StudioWebSearchBackend {
            provider_id: current_route.provider_id.clone(),
            provider_info: provider.to_provider_info(&current_route.model.slug).ok()?,
            model: current_route.model.slug.clone(),
            max_output_tokens: current_route.model.max_output_tokens,
        });
    }

    config
        .models
        .providers
        .iter()
        .find(|(_, provider)| is_credentialed_openai(provider))
        .and_then(|(provider_id, provider)| {
            let model = model_for_provider(config, provider_id, provider)?;
            Some(StudioWebSearchBackend {
                provider_id: provider_id.clone(),
                provider_info: provider.to_provider_info(&model.slug).ok()?,
                model: model.slug,
                max_output_tokens: model.max_output_tokens,
            })
        })
}

fn is_credentialed_openai(provider: &pl_core::ProviderConfig) -> bool {
    provider
        .preset_id()
        .is_some_and(|preset| preset.as_str() == "openai")
        && provider.resolved_bearer_token().is_some()
}

fn model_for_provider(
    config: &StudioConfig,
    provider_id: &ProviderId,
    provider: &pl_core::ProviderConfig,
) -> Option<pl_model::ModelInfo> {
    StudioRole::all()
        .into_iter()
        .filter_map(|role| config.models.resolve(&role.id()).ok())
        .find(|route| &route.provider_id == provider_id)
        .map(|route| route.model)
        .or_else(|| provider.effective_models().ok()?.into_iter().next())
}

#[cfg(test)]
mod tests {
    use pl_core::{
        AgentRoleId, ModelRouteConfig, ProviderConfig, ProviderModelCatalogConfig,
        ProviderTransportSelection, builtin_provider_catalog,
    };
    use pl_model::{ModelInfo, ProviderConnectionMode, ProviderInfo, ToolCapabilities};

    use super::*;

    #[test]
    fn missing_empty_and_unresolved_credentials_disable_search() {
        let config = StudioConfig::default_config();
        assert_missing_credential(&config);

        let mut config = StudioConfig::default_config();
        let mut provider = openai_provider(Some("   "));
        provider.bearer_token_env = Some("PURE_LANG_TEST_MISSING_OPENAI_TOKEN".to_string());
        config
            .models
            .providers
            .insert(provider_id("openai-empty"), provider);
        assert_missing_credential(&config);
    }

    #[test]
    fn custom_responses_provider_does_not_count_as_openai_account() {
        let mut config = StudioConfig::default_config();
        let mut info = ProviderInfo::openai(None);
        info.bearer_token = Some("custom-secret".to_string());
        let provider =
            ProviderConfig::from_provider_info(info, vec![ModelInfo::fallback("custom-model")]);
        assert!(provider.resolved_bearer_token().is_some());
        assert!(matches!(
            provider.transport,
            ProviderTransportSelection::Custom { .. }
        ));
        config
            .models
            .providers
            .insert(provider_id("custom-responses"), provider);

        assert_missing_credential(&config);
    }

    #[test]
    fn current_credentialed_openai_route_wins_and_name_does_not_define_identity() {
        let mut config = StudioConfig::default_config();
        let mut first = openai_provider(Some("first-secret"));
        first.name = "Renamed First".to_string();
        let mut current = openai_provider(Some("current-secret"));
        current.name = "Anything At All".to_string();
        let current_model = current
            .effective_models()
            .expect("models")
            .first()
            .expect("model")
            .slug
            .clone();
        config
            .models
            .providers
            .insert(provider_id("a-openai"), first);
        config
            .models
            .providers
            .insert(provider_id("z-current"), current);
        set_role_route(
            &mut config,
            StudioRole::Executor,
            "z-current",
            &current_model,
        );

        let resolution = resolve_for(&config, StudioRole::Executor);

        assert_eq!(resolution.path, Some(StudioWebSearchPath::Standalone));
        assert_eq!(
            resolution.backend.expect("backend").provider_id,
            provider_id("z-current")
        );
    }

    #[test]
    fn fallback_provider_and_role_model_selection_are_stable() {
        let mut config = StudioConfig::default_config();
        let first = openai_provider(Some("first-secret"));
        let second = openai_provider(Some("second-secret"));
        let first_models = first.effective_models().expect("models");
        let explorer_model = first_models[1].slug.clone();
        config
            .models
            .providers
            .insert(provider_id("a-openai"), first);
        config
            .models
            .providers
            .insert(provider_id("z-openai"), second);
        set_role_route(
            &mut config,
            StudioRole::Explorer,
            "a-openai",
            &explorer_model,
        );
        let current_route = config
            .models
            .resolve(&StudioRole::Executor.id())
            .expect("current route");

        let resolution = resolve_web_search(&config, &current_route);
        let backend = resolution.backend.expect("backend");

        assert_eq!(backend.provider_id, provider_id("a-openai"));
        assert_eq!(backend.model, explorer_model);
        assert_eq!(resolution.path, Some(StudioWebSearchPath::Standalone));
    }

    #[test]
    fn standalone_and_hosted_paths_are_mutually_exclusive() {
        let mut cross_provider = StudioConfig::default_config();
        cross_provider
            .models
            .providers
            .insert(provider_id("openai"), openai_provider(Some("secret")));
        let standalone = resolve_for(&cross_provider, StudioRole::Executor);
        assert_eq!(standalone.path, Some(StudioWebSearchPath::Standalone));
        assert_eq!(
            standalone.availability,
            StudioWebSearchAvailability::Available
        );

        let mut hosted = StudioConfig::default_config();
        let mut openai = openai_provider(Some("secret"));
        let hosted_model = model_with_tool_support("hosted-only", false, true);
        add_model(&mut openai, hosted_model.clone());
        hosted
            .models
            .providers
            .insert(provider_id("openai"), openai);
        set_role_route(
            &mut hosted,
            StudioRole::Executor,
            "openai",
            &hosted_model.slug,
        );
        let hosted_resolution = resolve_for(&hosted, StudioRole::Executor);
        assert_eq!(hosted_resolution.path, Some(StudioWebSearchPath::Hosted));

        let mut unsupported = StudioConfig::default_config();
        let unsupported_model = model_with_tool_support("no-tools", false, true);
        let deepseek_id = provider_id("deepseek");
        add_model(
            unsupported
                .models
                .providers
                .get_mut(&deepseek_id)
                .expect("deepseek"),
            unsupported_model.clone(),
        );
        unsupported
            .models
            .providers
            .insert(provider_id("openai"), openai_provider(Some("secret")));
        set_role_route(
            &mut unsupported,
            StudioRole::Executor,
            "deepseek",
            &unsupported_model.slug,
        );
        let unavailable = resolve_for(&unsupported, StudioRole::Executor);
        assert_eq!(
            unavailable.availability,
            StudioWebSearchAvailability::UnsupportedModel
        );
        assert!(unavailable.path.is_none());
        assert_eq!(
            unavailable.backend.expect("selected backend").provider_id,
            provider_id("openai")
        );
    }

    #[test]
    fn configured_mode_is_preserved_when_effective_mode_is_disabled() {
        let mut config = StudioConfig::default_config();
        config.web_search.mode = WebSearchMode::Live;
        let resolution = resolve_for(&config, StudioRole::Executor);
        assert_eq!(resolution.configured_mode, WebSearchMode::Live);
        assert_eq!(resolution.effective_mode, WebSearchMode::Disabled);

        config.web_search.mode = WebSearchMode::Disabled;
        let disabled = resolve_for(&config, StudioRole::Executor);
        assert_eq!(disabled.availability, StudioWebSearchAvailability::Disabled);
        assert!(disabled.backend.is_none());
    }

    fn assert_missing_credential(config: &StudioConfig) {
        let resolution = resolve_for(config, StudioRole::Executor);
        assert_eq!(
            resolution.availability,
            StudioWebSearchAvailability::MissingCredential
        );
        assert_eq!(resolution.effective_mode, WebSearchMode::Disabled);
        assert!(resolution.path.is_none());
        assert!(resolution.backend.is_none());
    }

    fn resolve_for(config: &StudioConfig, role: StudioRole) -> StudioWebSearchResolution {
        let route = config.models.resolve(&role.id()).expect("route");
        resolve_web_search(config, &route)
    }

    fn openai_provider(token: Option<&str>) -> ProviderConfig {
        let mut provider = builtin_provider_catalog()
            .presets
            .into_iter()
            .find(|preset| preset.id.as_str() == "openai")
            .expect("openai preset")
            .provider
            .with_connection_mode(ProviderConnectionMode::Http);
        provider.bearer_token = token.map(str::to_string);
        provider.bearer_token_env = None;
        provider
    }

    fn model_with_tool_support(slug: &str, function_calling: bool, web_search: bool) -> ModelInfo {
        let mut model = ModelInfo::fallback(slug);
        model.capabilities.web_search = web_search;
        model.capabilities.tools = ToolCapabilities {
            function_calling,
            parallel_tool_calls: function_calling,
            custom_tools: false,
            freeform_tools: false,
        };
        model
    }

    fn add_model(provider: &mut ProviderConfig, model: ModelInfo) {
        let ProviderModelCatalogConfig::Bundled {
            additional_models, ..
        } = &mut provider.catalog
        else {
            panic!("test provider must use bundled catalog");
        };
        additional_models.push(model);
    }

    fn set_role_route(config: &mut StudioConfig, role: StudioRole, provider: &str, model: &str) {
        config.models.routes.insert(
            AgentRoleId::new(role.key()).expect("role"),
            ModelRouteConfig {
                provider: provider_id(provider),
                model: model.to_string(),
                reasoning_effort: None,
            },
        );
    }

    fn provider_id(value: &str) -> ProviderId {
        ProviderId::new(value).expect("provider id")
    }
}

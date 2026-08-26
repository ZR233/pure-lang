//! Provider capability 驱动的 Web Search 规划与安装。

use std::collections::BTreeSet;

use pl_model::{
    ProviderEndpoint, ProviderWireProtocol, StandaloneWebSearchDialect, WebSearchConfig,
    WebSearchMode,
};
use pl_protocol::{PureError, Result, WebSearchResolutionDescriptor};

use crate::TurnEngine;
use crate::model_config::{AgentModelConfig, ProviderConfig, ProviderId, ResolvedModelRoute};
use crate::tool::{HostedWebSearchTool, TOOL_WEB_SEARCH, WebSearchClient, WebSearchTool};

/// Web Search 工具对本轮其他工具的可见性约束。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolVisibilityConstraint {
    Additive,
    Exclusive,
    Unavailable,
}

/// 当前 turn 实际使用的 Web Search 路径。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WebSearchPath {
    Standalone,
    Hosted,
}

/// Web Search 规划结果的可用性。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WebSearchAvailability {
    Available,
    Disabled,
    MissingCredential,
    ProviderUnsupported,
    ModelUnsupported,
}

/// 已解析且可直接创建 standalone 客户端的 backend。
#[derive(Debug, Clone)]
pub struct WebSearchBackend {
    pub provider_id: ProviderId,
    pub endpoint: ProviderEndpoint,
    pub model: String,
    pub max_output_tokens: Option<u64>,
    pub dialect: StandaloneWebSearchDialect,
}

/// 产品和设置页共用的 Web Search 解析结果。
#[derive(Debug, Clone)]
pub struct WebSearchResolution {
    pub configured_mode: WebSearchMode,
    pub effective_mode: WebSearchMode,
    pub availability: WebSearchAvailability,
    pub path: Option<WebSearchPath>,
    pub provider_id: Option<ProviderId>,
    pub model: Option<String>,
}

impl WebSearchResolution {
    /// 生成不包含凭证的公共协议投影。
    pub fn descriptor(&self) -> WebSearchResolutionDescriptor {
        WebSearchResolutionDescriptor {
            configured_mode: mode_label(self.configured_mode).to_string(),
            effective_mode: mode_label(self.effective_mode).to_string(),
            availability: availability_label(self.availability).to_string(),
            path: self.path.map(path_label).map(str::to_string),
            provider_id: self.provider_id.as_ref().map(ToString::to_string),
            model: self.model.clone(),
        }
    }
}

/// 一次 turn 可直接应用的 Web Search 计划。
#[derive(Debug, Clone)]
pub struct WebSearchPlan {
    pub resolution: WebSearchResolution,
    pub visibility: ToolVisibilityConstraint,
    backend: Option<WebSearchBackend>,
}

impl WebSearchPlan {
    /// 把已解析计划安装到 TurnEngine；产品不得绕过该入口自行构造工具。
    ///
    /// web_search 属于 builtin 来源的 eager 工具；安装失败返回错误且不改变现有
    /// 工具集合。
    ///
    /// # Errors
    ///
    /// backend 缺失或 builtin 来源发布校验失败时返回错误。
    pub fn install(&self, core: &mut TurnEngine, config: &WebSearchConfig) -> Result<()> {
        let tool = match self.resolution.path {
            Some(WebSearchPath::Standalone) => {
                let backend = self.backend.as_ref().ok_or_else(|| {
                    PureError::ConfigError(
                        "standalone web search is missing its resolved backend".to_string(),
                    )
                })?;
                match backend.dialect {
                    StandaloneWebSearchDialect::OpenAiSearchApi => {
                        let client = WebSearchClient::new(&backend.endpoint)?;
                        let tool = WebSearchTool::new(
                            client,
                            backend.model.clone(),
                            config,
                            backend.max_output_tokens,
                            core.tool_session_runtime(),
                        );
                        std::sync::Arc::new(tool) as std::sync::Arc<dyn crate::tool::Tool>
                    }
                }
            }
            Some(WebSearchPath::Hosted) => {
                let tool = HostedWebSearchTool::from_config(config).ok_or_else(|| {
                    PureError::ConfigError(
                        "hosted web search requires an enabled effective mode".to_string(),
                    )
                })?;
                std::sync::Arc::new(tool) as std::sync::Arc<dyn crate::tool::Tool>
            }
            None => {
                core.agent_tools()
                    .uninstall(&crate::tool::ToolGroupId::new("web_search"));
                return Ok(());
            }
        };
        core.agent_tools()
            .install(crate::tool::ToolGroupId::new("web_search"), vec![tool])
    }

    /// 返回 exclusive 路径唯一允许的工具名。
    pub fn exclusive_tool_name(&self) -> Option<&'static str> {
        (self.visibility == ToolVisibilityConstraint::Exclusive).then_some(TOOL_WEB_SEARCH)
    }
}

/// 根据 provider 服务能力和模型能力确定性规划 Web Search。
pub fn plan_web_search(
    models: &AgentModelConfig,
    current: &ResolvedModelRoute,
    config: &WebSearchConfig,
) -> Result<WebSearchPlan> {
    let configured_mode = config.mode;
    if configured_mode.is_disabled() {
        return Ok(unavailable_plan(
            configured_mode,
            WebSearchAvailability::Disabled,
        ));
    }

    let current_has_credential = current.endpoint.bearer_token.is_some();
    let hosted_declared = current
        .endpoint
        .service_capabilities
        .web_search
        .hosted_responses;
    let hosted_supported = hosted_declared
        && current.model.transport.protocol == ProviderWireProtocol::Responses
        && current.model.capabilities.supports_web_search()
        && current_has_credential;

    let standalone = standalone_backend(models, current)?;
    if current.model.capabilities.supports_function_calling()
        && let Some(backend) = standalone.backend
    {
        let resolution = available_resolution(
            configured_mode,
            WebSearchPath::Standalone,
            backend.provider_id.clone(),
            backend.model.clone(),
        );
        return Ok(WebSearchPlan {
            resolution,
            visibility: ToolVisibilityConstraint::Additive,
            backend: Some(backend),
        });
    }

    if hosted_supported {
        return Ok(WebSearchPlan {
            resolution: available_resolution(
                configured_mode,
                WebSearchPath::Hosted,
                current.provider_id.clone(),
                current.model.slug.clone(),
            ),
            visibility: ToolVisibilityConstraint::Exclusive,
            backend: None,
        });
    }

    let any_declared = hosted_declared || standalone.any_declared;
    let missing_credential =
        (hosted_declared && !current_has_credential) || standalone.missing_credential;
    let availability = if missing_credential {
        WebSearchAvailability::MissingCredential
    } else if !any_declared {
        WebSearchAvailability::ProviderUnsupported
    } else {
        WebSearchAvailability::ModelUnsupported
    };
    Ok(unavailable_plan(configured_mode, availability))
}

#[derive(Debug, Default)]
struct StandaloneSelection {
    backend: Option<WebSearchBackend>,
    any_declared: bool,
    missing_credential: bool,
}

fn standalone_backend(
    models: &AgentModelConfig,
    current: &ResolvedModelRoute,
) -> Result<StandaloneSelection> {
    let mut provider_ids = Vec::new();
    provider_ids.push(current.provider_id.clone());
    provider_ids.extend(models.routes.values().map(|route| route.provider.clone()));
    provider_ids.extend(models.providers.keys().cloned());

    let mut visited = BTreeSet::new();
    let mut selection = StandaloneSelection::default();
    for provider_id in provider_ids {
        if !visited.insert(provider_id.clone()) {
            continue;
        }
        let Some(provider) = models.providers.get(&provider_id) else {
            continue;
        };
        let capabilities = provider.service_capabilities()?;
        let Some(dialect) = capabilities.web_search.standalone else {
            continue;
        };
        selection.any_declared = true;
        if provider.resolved_bearer_token().is_none() {
            selection.missing_credential = true;
            continue;
        }
        let model = selected_model(models, current, &provider_id, provider)?;
        let endpoint = provider.to_endpoint()?;
        selection.backend = Some(WebSearchBackend {
            provider_id,
            endpoint,
            model: model.slug,
            max_output_tokens: model.max_output_tokens,
            dialect,
        });
        return Ok(selection);
    }
    Ok(selection)
}

fn selected_model(
    models: &AgentModelConfig,
    current: &ResolvedModelRoute,
    provider_id: &ProviderId,
    provider: &ProviderConfig,
) -> Result<pl_model::ModelInfo> {
    if provider_id == &current.provider_id {
        return Ok(current.model.clone());
    }
    if let Some(route) = models
        .routes
        .values()
        .find(|route| &route.provider == provider_id)
    {
        return provider
            .effective_models()?
            .into_iter()
            .find(|model| model.slug == route.model)
            .ok_or_else(|| {
                PureError::ConfigError(format!(
                    "provider {provider_id} route references missing model: {}",
                    route.model
                ))
            });
    }
    provider
        .effective_models()?
        .into_iter()
        .next()
        .ok_or_else(|| PureError::ConfigError(format!("provider {provider_id} has no models")))
}

fn available_resolution(
    mode: WebSearchMode,
    path: WebSearchPath,
    provider_id: ProviderId,
    model: String,
) -> WebSearchResolution {
    WebSearchResolution {
        configured_mode: mode,
        effective_mode: mode,
        availability: WebSearchAvailability::Available,
        path: Some(path),
        provider_id: Some(provider_id),
        model: Some(model),
    }
}

fn unavailable_plan(
    configured_mode: WebSearchMode,
    availability: WebSearchAvailability,
) -> WebSearchPlan {
    WebSearchPlan {
        resolution: WebSearchResolution {
            configured_mode,
            effective_mode: WebSearchMode::Disabled,
            availability,
            path: None,
            provider_id: None,
            model: None,
        },
        visibility: ToolVisibilityConstraint::Unavailable,
        backend: None,
    }
}

fn mode_label(mode: WebSearchMode) -> &'static str {
    match mode {
        WebSearchMode::Disabled => "disabled",
        WebSearchMode::Cached => "cached",
        WebSearchMode::Indexed => "indexed",
        WebSearchMode::Live => "live",
    }
}

fn availability_label(availability: WebSearchAvailability) -> &'static str {
    match availability {
        WebSearchAvailability::Available => "available",
        WebSearchAvailability::Disabled => "disabled",
        WebSearchAvailability::MissingCredential => "missing_credential",
        WebSearchAvailability::ProviderUnsupported => "provider_unsupported",
        WebSearchAvailability::ModelUnsupported => "model_unsupported",
    }
}

fn path_label(path: WebSearchPath) -> &'static str {
    match path {
        WebSearchPath::Standalone => "standalone",
        WebSearchPath::Hosted => "hosted",
    }
}

#[cfg(test)]
mod unit_tests;

//! Web Search 计划投影为独立的工具执行器与模型持有的 hosted 能力。
//!
//! 装配过程中只消费规划结果与共享配置快照；hosted 条目永不在本层获取 core 执行器。
//! OpenAI 与 DeepSeek 两条规划彼此独立，任一可用都只追加自己的工具，不排斥其他工具。

use pl_core::tool::opaque::Registration;
use pl_model::provider::StandaloneWebSearchDialect;
use pl_model::provider::deepseek::search::{SearchClient, SearchError, SearchOptions};
use pl_model::runtime::{HostedTool, thread_tool_declaration};
use pl_protocol::HostedWebSearchDialect;
use pl_protocol::search::WebSearchConfig;

use super::client::{SearchEndpoint, WebSearchClient};
use super::hosted_options;
use super::plan::{WebSearchPath, WebSearchPlans};
use super::thread::{ThreadDeepSeekWebSearchTool, ThreadSearchOptions, ThreadWebSearchTool};

/// 公共搜索装配错误；产品调用方负责映射到自身错误域。
///
/// 该类型刻意不引用任何 Studio 专属错误，以便 Studio 与 mai-team 等调用方共用。
#[derive(Debug, thiserror::Error)]
pub enum SearchBindingError {
    #[error("standalone search is missing its resolved backend")]
    MissingBackend,
    #[error(transparent)]
    Config(#[from] pl_protocol::PureError),
    #[error("web search tool declaration failed")]
    Declaration(#[from] pl_core::model::ModelError),
    #[error("web search tool registration failed")]
    Registry(#[from] pl_core::tool::opaque::RegistryError),
    #[error(transparent)]
    DeepSeek(#[from] SearchError),
}

/// Frozen search assembly; hosted entries never acquire core executors.
#[derive(Debug)]
pub struct ThreadSearchBinding {
    pub tools: Vec<Registration>,
    pub hosted: Vec<HostedTool>,
}

impl WebSearchPlans {
    /// Returns only provider-executed declarations without constructing local tool resources.
    pub fn hosted_tools(&self, config: &WebSearchConfig) -> pl_protocol::Result<Vec<HostedTool>> {
        let mut hosted = Vec::new();
        for plan in self.plans() {
            if plan.resolution.path != Some(WebSearchPath::Hosted) {
                continue;
            }
            match plan.hosted_dialect {
                Some(HostedWebSearchDialect::OpenAiResponses) => {
                    let options = hosted_options::openai_options(config).ok_or_else(|| {
                        pl_protocol::PureError::ConfigError(
                            "hosted search requires an enabled effective mode".into(),
                        )
                    })?;
                    hosted.push(HostedTool::WebSearch(options));
                }
                // Only OpenAI Responses hosted is planned; DeepSeek search is native standalone.
                // Any other dialect on a hosted path is an assembly bug, not a silent skip.
                Some(HostedWebSearchDialect::DeepSeekResponses) | None => {
                    return Err(pl_protocol::PureError::ConfigError(
                        "planned hosted web search has an unsupported dialect".into(),
                    ));
                }
            }
        }
        Ok(hosted)
    }

    /// Projects every available provider capability using the same configuration snapshot as the route.
    ///
    /// # Errors
    /// Returns missing backend, disabled hosted options, declaration or registration errors.
    pub fn build_thread(
        &self,
        config: &WebSearchConfig,
    ) -> std::result::Result<ThreadSearchBinding, SearchBindingError> {
        let mut binding = ThreadSearchBinding {
            tools: Vec::new(),
            hosted: self.hosted_tools(config)?,
        };
        for plan in self.plans() {
            if plan.resolution.path == Some(WebSearchPath::Standalone) {
                let backend = plan
                    .backend
                    .as_ref()
                    .ok_or(SearchBindingError::MissingBackend)?;
                match backend.dialect {
                    StandaloneWebSearchDialect::OpenAiSearchApi => {
                        let client = WebSearchClient::new(&SearchEndpoint {
                            base_url: backend.endpoint.base_url.clone(),
                            bearer_token: backend.endpoint.bearer_token.clone(),
                            http_headers: backend.endpoint.http_headers.clone(),
                        })?;
                        let tool = ThreadWebSearchTool::new(
                            client,
                            ThreadSearchOptions {
                                model: backend.model.clone(),
                                settings: pl_protocol::search::SearchSettings::from_config(config),
                                max_output_tokens: backend.max_output_tokens,
                            },
                        );
                        let declaration =
                            thread_tool_declaration(&ThreadWebSearchTool::declaration())?;
                        binding.tools.push(tool.registration(declaration)?);
                    }
                    StandaloneWebSearchDialect::DeepSeekAnthropicMessages => {
                        let options = SearchOptions {
                            model: backend.model.clone(),
                            ..SearchOptions::default()
                        };
                        let client = SearchClient::new(&backend.endpoint, options)?;
                        let tool = ThreadDeepSeekWebSearchTool::new(client);
                        let declaration =
                            thread_tool_declaration(&ThreadDeepSeekWebSearchTool::declaration())?;
                        binding.tools.push(tool.registration(declaration)?);
                    }
                }
            }
        }
        Ok(binding)
    }
}

//! Search plan projection into independent tool executors and model-owned capabilities.
use super::*;
use crate::thread_assembler::ThreadAssemblyError;
use pl_core::tool::opaque::Registration;
use pl_model::runtime::HostedTool;

/// Frozen search assembly; hosted entries never acquire core executors.
#[derive(Debug)]
pub struct ThreadSearchBinding {
    pub tools: Vec<Registration>,
    pub hosted: Vec<HostedTool>,
    pub visibility: ToolVisibilityConstraint,
}

impl WebSearchPlans {
    /// Returns only provider-executed declarations without constructing local tool resources.
    pub(crate) fn hosted_tools(&self, config: &WebSearchConfig) -> Result<Vec<HostedTool>> {
        let Some(plan) = self.active() else {
            return Ok(Vec::new());
        };
        if plan.resolution.path != Some(WebSearchPath::Hosted) {
            return Ok(Vec::new());
        }
        let options = match plan.hosted_dialect {
            Some(HostedWebSearchDialect::DeepSeekResponses) => {
                pl_protocol::HostedWebSearchOptions::DeepSeek
            }
            Some(HostedWebSearchDialect::OpenAiResponses) | None => {
                super::hosted_options::openai_options(config).ok_or_else(|| {
                    PureError::ConfigError(
                        "hosted search requires an enabled effective mode".into(),
                    )
                })?
            }
        };
        Ok(vec![HostedTool::WebSearch(options)])
    }

    /// Projects the selected provider capability using the same configuration snapshot as the route.
    ///
    /// # Errors
    /// Returns missing backend, disabled hosted options, declaration or registration errors.
    pub fn build_thread(
        &self,
        config: &WebSearchConfig,
    ) -> std::result::Result<ThreadSearchBinding, ThreadAssemblyError> {
        let mut binding = ThreadSearchBinding {
            tools: Vec::new(),
            hosted: self.hosted_tools(config)?,
            visibility: self.visibility(),
        };
        let Some(plan) = self.active() else {
            return Ok(binding);
        };
        match plan.resolution.path {
            Some(WebSearchPath::Standalone) => {
                let backend = plan.backend.as_ref().ok_or_else(|| {
                    PureError::ConfigError(
                        "standalone search is missing its resolved backend".into(),
                    )
                })?;
                match backend.dialect {
                    StandaloneWebSearchDialect::OpenAiSearchApi => {
                        let client = WebSearchClient::new(&pl_tool::search::SearchEndpoint {
                            base_url: backend.endpoint.base_url.clone(),
                            bearer_token: backend.endpoint.bearer_token.clone(),
                            http_headers: backend.endpoint.http_headers.clone(),
                        })?;
                        let tool = pl_tool::search::ThreadWebSearchTool::new(
                            client,
                            pl_tool::search::ThreadSearchOptions {
                                model: backend.model.clone(),
                                settings: pl_protocol::search::SearchSettings::from_config(config),
                                max_output_tokens: backend.max_output_tokens,
                            },
                        );
                        let declaration = pl_model::runtime::thread_tool_declaration(
                            &pl_tool::search::ThreadWebSearchTool::declaration(),
                        )?;
                        binding.tools.push(tool.registration(declaration)?);
                    }
                }
            }
            Some(WebSearchPath::Hosted) => {}
            None => {}
        }
        Ok(binding)
    }
}

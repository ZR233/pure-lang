//! Deterministic product options for model-owned hosted search.
use pl_protocol::search::{WebSearchConfig, WebSearchMode};
use pl_protocol::{HostedWebSearchOptions, WebSearchFilters, WebSearchUserLocation};

pub(super) fn openai_options(config: &WebSearchConfig) -> Option<HostedWebSearchOptions> {
    let (external_web_access, indexed_web_access) = match config.mode {
        WebSearchMode::Cached => (false, None),
        WebSearchMode::Indexed => (true, Some(true)),
        WebSearchMode::Live => (true, None),
        WebSearchMode::Disabled => return None,
    };
    Some(HostedWebSearchOptions::OpenAi {
        external_web_access,
        indexed_web_access,
        filters: (!config.allowed_domains.is_empty()).then(|| WebSearchFilters {
            allowed_domains: config.allowed_domains.clone(),
        }),
        user_location: config
            .location
            .as_ref()
            .filter(|location| !location.is_empty())
            .map(WebSearchUserLocation::from),
        search_context_size: config.context_size,
        search_content_types: None,
    })
}

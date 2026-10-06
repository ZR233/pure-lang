mod binding;
mod client;
mod hosted_options;
mod plan;
mod thread;

pub use binding::{SearchBindingError, ThreadSearchBinding};
pub use client::{SearchEndpoint, WebSearchClient};
pub use plan::{
    WebSearchAvailability, WebSearchBackend, WebSearchPath, WebSearchPlan, WebSearchPlans,
    WebSearchResolution, WebSearchService, WebSearchServices, plan_deepseek_web_search,
    plan_openai_web_search, plan_web_search_services, plan_web_searches,
};
pub use thread::{ThreadDeepSeekWebSearchTool, ThreadSearchOptions, ThreadWebSearchTool};

pub const TOOL_WEB_SEARCH: &str = "web_search";
pub const TOOL_DEEPSEEK_WEB_SEARCH: &str = "deepseek_web_search";
const ASSISTANT_CONTEXT_CHAR_LIMIT: usize = 4_000;
const DEEPSEEK_SEARCH_CONTEXT_CHAR_LIMIT: usize = 12_000;
const WEB_SEARCH_DESCRIPTION: &str = "Search or open web pages, find text in pages, capture PDF pages, and query finance, weather, sports, or time data. Pass commands as arrays of objects, for example {\"search_query\":[{\"q\":\"latest Flutter release\"}]} or {\"open\":[{\"ref_id\":\"turn0search0\"}]}. Multiple commands may be combined in one call.";
const DEEPSEEK_WEB_SEARCH_DESCRIPTION: &str = "Paid fallback web search executed by the DeepSeek search service; every call is billed. Use it only after other web search is not configured, unavailable, or has actually failed. Call discover_tools first to discover available MCP tools before choosing this fallback.";

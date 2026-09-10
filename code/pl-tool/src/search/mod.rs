mod client;
mod thread;
pub use thread::{ThreadSearchOptions, ThreadWebSearchTool};

pub use self::client::{SearchEndpoint, WebSearchClient};

pub const TOOL_WEB_SEARCH: &str = "web_search";
const ASSISTANT_CONTEXT_CHAR_LIMIT: usize = 4_000;
const WEB_SEARCH_DESCRIPTION: &str = "Search or open web pages, find text in pages, capture PDF pages, and query finance, weather, sports, or time data. Pass commands as arrays of objects, for example {\"search_query\":[{\"q\":\"latest Flutter release\"}]} or {\"open\":[{\"ref_id\":\"turn0search0\"}]}. Multiple commands may be combined in one call.";

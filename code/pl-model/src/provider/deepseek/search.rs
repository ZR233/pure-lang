//! DeepSeek 原生独立 Web Search：基于 Anthropic 兼容 Messages API 的 `web_search` server tool。
//!
//! 该模块是 `pl-model` 暴露给上层的公开搜索域：调用方只消费 [`SearchClient`]、
//! [`SearchOptions`]、[`SearchRequest`]、[`SearchResponse`] 与 [`SearchError`]。请求与响应都是
//! DeepSeek provider 私有的 wire 细节，不进入会话 transport，也不携带任何会话历史。
//!
//! 契约要点：
//! - 端点由 `provider.base_url` 自身地址组装：保留自定义 reverse-proxy 的 path 前缀，追加
//!   `/anthropic/v1/messages`，并把 canonical `/v1` 版本别名归一化；默认官方根为
//!   `https://api.deepseek.com`。
//! - 鉴权只使用所选 provider 的 `x-api-key`（附带 `anthropic-version: 2023-06-01` 与 JSON
//!   headers）；显式禁用重定向与 reqwest 默认 protocol 重试。
//! - 超时覆盖整个请求（等待 headers 与读取 body）；取消同样覆盖这两段读取。
//! - 只有 `query`（经固定指令包装）进入请求、不带会话历史；空 query 与非法配置在发出前拒绝。
//! - 解析 `web_search_tool_result` 结果块与 `text.citations`，按 URL 首现去重并合并
//!   `cited_text`；`web_search_tool_result_error` 与缺失结果块都明确失败，合法空数组可成功。
//! - 已完整取得的响应正文通过 [`SearchError::observed_response`] 保留到错误与取消路径，含
//!   `usage`/`model` 事实；合法 JSON 保留解析值，非 JSON（含畸形 JSON 与非 UTF-8 字节）按
//!   base64 约定无损保留原始字节，`usage`/`model` 缺失即未知，不伪造来源。错误信息与 `Debug`
//!   输出都不包含凭据。

use std::collections::{HashMap, HashSet};
use std::time::Duration;

use base64::{Engine, engine::general_purpose::STANDARD};
use reqwest::header::{ACCEPT, CONTENT_TYPE, HeaderMap, HeaderName, HeaderValue};
use serde::{Deserialize, Serialize};

use crate::provider::ProviderEndpoint;

/// 与运行时共享的精确取消类型重导出。
pub use tokio_util::sync::CancellationToken;

const DEFAULT_MODEL: &str = "deepseek-flash";
const DEFAULT_MAX_TOKENS: u32 = 4096;
const DEFAULT_MAX_USES: u32 = 5;
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(60);
const ANTHROPIC_VERSION: &str = "2023-06-01";
const MESSAGES_PATH: &str = "/anthropic/v1/messages";
const WEB_SEARCH_TOOL_TYPE: &str = "web_search_20250305";
const WEB_SEARCH_TOOL_NAME: &str = "web_search";
const MAX_ERROR_BODY_CHARS: usize = 512;
const RAW_BODY_ENCODING: &str = "base64";
const RESERVED_HEADERS: [&str; 6] = [
    "authorization",
    "x-api-key",
    "anthropic-version",
    "content-type",
    "accept",
    "host",
];

/// 独立搜索的请求参数。
///
/// 默认值沿用本项目 harness 的有界参数：模型 `deepseek-flash`、生成上限 4096、单次请求最多
/// 5 次原生搜索、整体超时 60 秒；这些是本项目选择的上界，不声称是官方推荐值。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchOptions {
    /// Anthropic 格式的模型名。
    pub model: String,
    /// `max_tokens`，必须为正整数。
    pub max_tokens: u32,
    /// `web_search` server tool 的单请求使用上限，必须为正整数。
    pub max_uses: u32,
    /// 覆盖整个请求的超时，必须为正。
    pub timeout: Duration,
}

impl Default for SearchOptions {
    fn default() -> Self {
        Self {
            model: DEFAULT_MODEL.to_string(),
            max_tokens: DEFAULT_MAX_TOKENS,
            max_uses: DEFAULT_MAX_USES,
            timeout: DEFAULT_TIMEOUT,
        }
    }
}

/// 一次独立搜索的输入；只有 `query` 会进入请求。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchRequest {
    /// 用户查询文本；会作为唯一的 user message 发送。
    pub query: String,
}

impl SearchRequest {
    /// 构造只包含查询文本的请求。
    pub fn new(query: impl Into<String>) -> Self {
        Self {
            query: query.into(),
        }
    }
}

/// 归一化后的搜索来源。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchSource {
    /// 结果页 URL；同一 URL 只保留首次出现。
    pub url: String,
    /// 结果标题，缺省为未知。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// 引用片段，来自 `text.citations[]` 的 `cited_text`。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub snippet: Option<String>,
    /// 页面发布时间（`page_age`），缺省为未知。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub published_at: Option<String>,
}

/// 归一化后的用量报告；字段缺失保持未知。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchUsage {
    /// 输入 token 数。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_tokens: Option<u64>,
    /// 输出 token 数。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_tokens: Option<u64>,
    /// 命中提示词缓存的输入 token 数。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_read_input_tokens: Option<u64>,
    /// 写入提示词缓存的输入 token 数。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_creation_input_tokens: Option<u64>,
    /// 供应商报告的原生 `web_search` server tool 调用次数。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub server_web_search_requests: Option<u64>,
}

/// 归一化的独立搜索结果；同时保留原始响应事实。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchResponse {
    /// 按 URL 首现去重后的来源。
    pub sources: Vec<SearchSource>,
    /// 供应商原生响应正文。合法 JSON 原样保留；正文已完整收到但不是合法 JSON（非 2xx 文本、
    /// 畸形 JSON、非 UTF-8 字节）时改为 `{"encoding":"base64","data":...}`，逐字节无损保留原始
    /// 正文。
    pub raw_response: serde_json::Value,
    /// 供应商报告的用量；缺失保持未知。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage: Option<SearchUsage>,
    /// 实际提供服务的模型名。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
}

/// 独立搜索的类型化错误；已观察到的响应通过 [`SearchError::observed_response`] 保留。
#[derive(Debug, thiserror::Error)]
pub enum SearchError {
    /// 所选 provider 没有可用凭据。
    #[error("deepseek web search requires a non-empty provider credential")]
    MissingCredential,
    /// 空 query 在发出前被拒绝。
    #[error("deepseek web search requires a non-empty query")]
    EmptyQuery,
    /// 配置或参数在发出前被拒绝。
    #[error("invalid deepseek web search configuration: {message}")]
    Config { message: String },
    /// 构建 HTTP 客户端失败。
    #[error("failed to build deepseek web search client: {source}")]
    Client {
        #[source]
        source: reqwest::Error,
    },
    /// 请求在传输阶段失败。
    #[error("deepseek web search request failed: {source}")]
    Transport {
        #[source]
        source: reqwest::Error,
        observed: Option<Box<SearchResponse>>,
    },
    /// 请求整体超时。
    #[error("deepseek web search timed out after {seconds} seconds")]
    Timeout {
        seconds: u64,
        #[source]
        source: reqwest::Error,
        observed: Option<Box<SearchResponse>>,
    },
    /// 请求被调用方取消。
    #[error("deepseek web search was cancelled")]
    Cancelled {
        observed: Option<Box<SearchResponse>>,
    },
    /// HTTP 状态非 2xx；`message` 只包含状态与受限的响应片段，完整正文经 `observed` 保留。
    #[error("deepseek web search returned HTTP {status}: {message}")]
    HttpStatus {
        status: u16,
        message: String,
        observed: Option<Box<SearchResponse>>,
    },
    /// 2xx 响应体不是合法 JSON；完整正文经 `observed` 无损保留。
    #[error("deepseek web search returned an unreadable response: {source}")]
    InvalidResponse {
        #[source]
        source: serde_json::Error,
        observed: Option<Box<SearchResponse>>,
    },
    /// 响应里没有任何 `web_search_tool_result` 结果块，普通回答不能伪造来源。
    #[error("deepseek returned no web_search_tool_result blocks")]
    NoResults { observed: Box<SearchResponse> },
    /// 原生搜索结果块明确报告了错误。
    #[error("deepseek web search tool reported an error: {message}")]
    ToolResultError {
        message: String,
        observed: Box<SearchResponse>,
    },
}

impl SearchError {
    /// 返回本次调用已取得的原始响应（含 raw/usage/model），未取得时为 `None`。
    ///
    /// 完整收到响应正文时一定返回 `Some`：合法 JSON 保留解析后的 [`SearchResponse::raw_response`]，
    /// 非 JSON 正文（畸形 JSON、非 UTF-8 字节、非 2xx 文本）以 base64 约定无损保留原始字节，
    /// `usage`/`model` 缺失即保持未知。超时或取消等尚未取到完整正文的路径返回 `None`。
    pub fn observed_response(&self) -> Option<&SearchResponse> {
        match self {
            Self::Transport { observed, .. }
            | Self::Timeout { observed, .. }
            | Self::Cancelled { observed }
            | Self::HttpStatus { observed, .. }
            | Self::InvalidResponse { observed, .. } => observed.as_deref(),
            Self::NoResults { observed } | Self::ToolResultError { observed, .. } => {
                Some(observed.as_ref())
            }
            Self::MissingCredential
            | Self::EmptyQuery
            | Self::Config { .. }
            | Self::Client { .. } => None,
        }
    }
}

/// 只负责 DeepSeek 原生独立搜索的 HTTP 客户端。
///
/// 客户端冻结一次解析出的端点、参数与凭据；凭据只作为 sensitive header 保存在内部，
/// 不进入 `Debug` 或错误信息。
#[derive(Clone)]
pub struct SearchClient {
    client: reqwest::Client,
    endpoint: String,
    options: SearchOptions,
}

impl std::fmt::Debug for SearchClient {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SearchClient")
            .field("endpoint", &self.endpoint)
            .field("options", &self.options)
            .finish_non_exhaustive()
    }
}

impl SearchClient {
    /// 从所选 provider endpoint 与参数构造原生搜索客户端。
    ///
    /// # Errors
    /// 缺少非空凭据返回 [`SearchError::MissingCredential`]；非法端点、模型、token 上限或超时
    /// 返回 [`SearchError::Config`]；构建 HTTP 客户端失败返回 [`SearchError::Client`]。这些失败
    /// 都发生在发出任何请求之前。
    pub fn new(provider: &ProviderEndpoint, options: SearchOptions) -> Result<Self, SearchError> {
        let credential = provider
            .bearer_token
            .as_deref()
            .map(str::trim)
            .filter(|token| !token.is_empty())
            .ok_or(SearchError::MissingCredential)?;
        if options.model.trim().is_empty() {
            return Err(SearchError::Config {
                message: "search model must not be empty".to_string(),
            });
        }
        if options.max_tokens == 0 {
            return Err(SearchError::Config {
                message: "max_tokens must be a positive integer".to_string(),
            });
        }
        if options.max_uses == 0 {
            return Err(SearchError::Config {
                message: "max_uses must be a positive integer".to_string(),
            });
        }
        if options.timeout.is_zero() {
            return Err(SearchError::Config {
                message: "timeout must be positive".to_string(),
            });
        }

        let endpoint = messages_endpoint(&provider.base_url)?;
        let headers = request_headers(provider, credential)?;
        let client = reqwest::Client::builder()
            .timeout(options.timeout)
            .redirect(reqwest::redirect::Policy::none())
            .retry(reqwest::retry::never())
            .default_headers(headers)
            .build()
            .map_err(|source| SearchError::Client { source })?;

        Ok(Self {
            client,
            endpoint,
            options,
        })
    }

    /// 返回本次请求实际使用的端点，便于上层无凭据地诊断。
    pub fn endpoint(&self) -> &str {
        &self.endpoint
    }

    /// 执行一次原生搜索。
    ///
    /// # Errors
    /// 空 query 返回 [`SearchError::EmptyQuery`]；其余失败返回类型化的 [`SearchError`]，
    /// 并在取得响应时保留已观察事实。
    pub async fn search(
        &self,
        request: &SearchRequest,
        cancellation: CancellationToken,
    ) -> Result<SearchResponse, SearchError> {
        let query = request.query.trim();
        if query.is_empty() {
            return Err(SearchError::EmptyQuery);
        }

        let body = self.native_body(query);
        let sending = self.client.post(&self.endpoint).json(&body).send();
        let response = tokio::select! {
            biased;
            () = cancellation.cancelled() => {
                return Err(SearchError::Cancelled { observed: None });
            }
            result = sending => {
                result.map_err(|source| transport_error(source, self.options.timeout))?
            }
        };

        let status = response.status();
        let reading = response.bytes();
        let bytes = tokio::select! {
            biased;
            () = cancellation.cancelled() => {
                return Err(SearchError::Cancelled { observed: None });
            }
            result = reading => {
                result.map_err(|source| transport_error(source, self.options.timeout))?
            }
        };

        let parsed = serde_json::from_slice::<serde_json::Value>(&bytes).ok();
        if !status.is_success() {
            let message = http_error_message(status.as_u16(), &bytes, parsed.as_ref());
            let observed = match &parsed {
                Some(value) => raw_observed(value),
                None => raw_body_observed(&bytes),
            };
            return Err(SearchError::HttpStatus {
                status: status.as_u16(),
                message,
                observed: Some(Box::new(observed)),
            });
        }

        let value = match serde_json::from_slice::<serde_json::Value>(&bytes) {
            Ok(value) => value,
            Err(source) => {
                return Err(SearchError::InvalidResponse {
                    source,
                    observed: Some(Box::new(raw_body_observed(&bytes))),
                });
            }
        };
        normalize_response(value)
    }

    fn native_body(&self, query: &str) -> serde_json::Value {
        serde_json::json!({
            "model": self.options.model,
            "max_tokens": self.options.max_tokens,
            "messages": [{
                "role": "user",
                "content": [{ "type": "text", "text": search_prompt(query) }],
            }],
            "tools": [{
                "type": WEB_SEARCH_TOOL_TYPE,
                "name": WEB_SEARCH_TOOL_NAME,
                "max_uses": self.options.max_uses,
            }],
        })
    }
}

/// 固定指令包装：命令模型执行一次原生搜索并返回结构化来源，`query` 仍是唯一输入。
fn search_prompt(query: &str) -> String {
    format!("Perform a web search for the query: {query}\n\nReturn source citations.")
}

fn transport_error(source: reqwest::Error, timeout: Duration) -> SearchError {
    if source.is_timeout() {
        SearchError::Timeout {
            seconds: timeout.as_secs(),
            source,
            observed: None,
        }
    } else {
        SearchError::Transport {
            source,
            observed: None,
        }
    }
}

/// 用 provider 的 base_url 自身地址组装 Messages 端点。
///
/// 保留自定义 reverse-proxy 的 path 前缀并追加 `/anthropic/v1/messages`；canonical `/v1`
/// 版本别名（如 `https://api.deepseek.com/v1`）归一化到同一根；已指向 `/anthropic` 或
/// `/anthropic/v1` 的地址不重复追加。只接受无 userinfo / query / fragment 的 http(s) 地址，
/// 不替换 host，也不回退到官方域名。
fn messages_endpoint(base_url: &str) -> Result<String, SearchError> {
    let trimmed = base_url.trim();
    if trimmed.is_empty() {
        return Err(SearchError::Config {
            message: "deepseek search base url must not be empty".to_string(),
        });
    }
    let mut url = reqwest::Url::parse(trimmed).map_err(|error| SearchError::Config {
        message: format!("invalid deepseek search base url: {error}"),
    })?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err(SearchError::Config {
            message: format!("deepseek search base url must use http or https: {trimmed}"),
        });
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err(SearchError::Config {
            message: "deepseek search base url must not contain credentials".to_string(),
        });
    }
    if url.query().is_some() || url.fragment().is_some() {
        return Err(SearchError::Config {
            message: "deepseek search base url must not contain a query or fragment".to_string(),
        });
    }

    let base_path = url.path().trim_end_matches('/');
    let path = if base_path.ends_with(MESSAGES_PATH) {
        base_path.to_string()
    } else if let Some(root) = base_path.strip_suffix("/anthropic/v1") {
        format!("{root}{MESSAGES_PATH}")
    } else if let Some(root) = base_path.strip_suffix("/anthropic") {
        format!("{root}{MESSAGES_PATH}")
    } else {
        let root = base_path.strip_suffix("/v1").unwrap_or(base_path);
        format!("{root}{MESSAGES_PATH}")
    };
    url.set_path(&path);
    Ok(url.to_string())
}

fn request_headers(
    provider: &ProviderEndpoint,
    credential: &str,
) -> Result<HeaderMap, SearchError> {
    let mut headers = HeaderMap::new();
    for (name, value) in provider.http_headers.iter().flatten() {
        if RESERVED_HEADERS
            .iter()
            .any(|reserved| name.eq_ignore_ascii_case(reserved))
        {
            continue;
        }
        let header_name =
            HeaderName::from_bytes(name.as_bytes()).map_err(|error| SearchError::Config {
                message: format!("invalid provider header name `{name}`: {error}"),
            })?;
        let header_value = HeaderValue::from_str(value).map_err(|error| SearchError::Config {
            message: format!("invalid provider header value for `{name}`: {error}"),
        })?;
        headers.insert(header_name, header_value);
    }

    let mut key = HeaderValue::from_str(credential).map_err(|_| SearchError::Config {
        message: "deepseek search credential is not a valid header value".to_string(),
    })?;
    key.set_sensitive(true);
    headers.insert(HeaderName::from_static("x-api-key"), key);
    headers.insert(
        HeaderName::from_static("anthropic-version"),
        HeaderValue::from_static(ANTHROPIC_VERSION),
    );
    headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
    headers.insert(ACCEPT, HeaderValue::from_static("application/json"));
    Ok(headers)
}

/// 把供应商原生响应正文归一化为 [`SearchResponse`]。
fn normalize_response(value: serde_json::Value) -> Result<SearchResponse, SearchError> {
    let mut citations: HashMap<String, String> = HashMap::new();
    let mut tool_error: Option<String> = None;
    let mut has_result_block = false;
    let mut sources = Vec::new();

    {
        let mut result_blocks: Vec<&Vec<serde_json::Value>> = Vec::new();
        if let Some(blocks) = value.get("content").and_then(|content| content.as_array()) {
            for block in blocks {
                match block.get("type").and_then(|kind| kind.as_str()) {
                    Some("text") => collect_citations(block, &mut citations),
                    Some("web_search_tool_result") => match block.get("content") {
                        Some(serde_json::Value::Array(items)) => {
                            has_result_block = true;
                            result_blocks.push(items);
                        }
                        Some(serde_json::Value::Object(error)) => {
                            let code = error
                                .get("error_code")
                                .and_then(|code| code.as_str())
                                .filter(|code| !code.is_empty())
                                .unwrap_or("unknown");
                            tool_error.get_or_insert_with(|| code.to_string());
                        }
                        _ => {}
                    },
                    _ => {}
                }
            }
        }

        if tool_error.is_none() && has_result_block {
            let mut seen = HashSet::new();
            for items in result_blocks {
                for item in items {
                    collect_source(item, &citations, &mut seen, &mut sources);
                }
            }
        }
    }

    if let Some(code) = tool_error {
        return Err(SearchError::ToolResultError {
            message: format!("deepseek web_search_tool_result_error: {code}"),
            observed: Box::new(raw_observed(&value)),
        });
    }
    if !has_result_block {
        return Err(SearchError::NoResults {
            observed: Box::new(raw_observed(&value)),
        });
    }

    Ok(SearchResponse {
        sources,
        usage: parse_usage(&value),
        model: parse_model(&value),
        raw_response: value,
    })
}

fn collect_source(
    item: &serde_json::Value,
    citations: &HashMap<String, String>,
    seen: &mut HashSet<String>,
    sources: &mut Vec<SearchSource>,
) {
    if item.get("type").and_then(|kind| kind.as_str()) != Some("web_search_result") {
        return;
    }
    let Some(url) = item
        .get("url")
        .and_then(|url| url.as_str())
        .filter(|url| !url.is_empty())
    else {
        return;
    };
    if !seen.insert(url.to_string()) {
        return;
    }
    sources.push(SearchSource {
        url: url.to_string(),
        title: non_empty(item.get("title")),
        snippet: citations.get(url).cloned(),
        published_at: non_empty(item.get("page_age")),
    });
}

fn collect_citations(block: &serde_json::Value, citations: &mut HashMap<String, String>) {
    let Some(entries) = block
        .get("citations")
        .and_then(|citations| citations.as_array())
    else {
        return;
    };
    for citation in entries {
        let url = citation.get("url").and_then(|url| url.as_str());
        let cited = citation.get("cited_text").and_then(|text| text.as_str());
        if let (Some(url), Some(cited)) = (url, cited)
            && !url.is_empty()
            && !cited.is_empty()
        {
            citations
                .entry(url.to_string())
                .or_insert_with(|| cited.to_string());
        }
    }
}

/// 保留原始响应事实的归一化外壳；用于错误状态与取消路径。
fn raw_observed(value: &serde_json::Value) -> SearchResponse {
    SearchResponse {
        sources: Vec::new(),
        usage: parse_usage(value),
        model: parse_model(value),
        raw_response: value.clone(),
    }
}

/// 未解析为 JSON 的完整响应体在 [`SearchResponse::raw_response`] 中的无损表示。
///
/// 完整收到的响应体若不是合法 JSON（非 2xx 文本、畸形 JSON、非 UTF-8 字节），就在这里以
/// `{"encoding":"base64","data":...}` 保存原始字节：与解析成功的 JSON 对象可区分、逐字节无损失，
/// 且只占用既有的不透明历史载荷 [`SearchResponse::raw_response`]，不新增第二份事实源。此时
/// [`SearchResponse::usage`] 与 [`SearchResponse::model`] 保持未知、
/// [`SearchResponse::sources`] 为空，不伪造搜索成功或来源。
fn raw_body_observed(bytes: &[u8]) -> SearchResponse {
    SearchResponse {
        sources: Vec::new(),
        usage: None,
        model: None,
        raw_response: serde_json::json!({
            "encoding": RAW_BODY_ENCODING,
            "data": STANDARD.encode(bytes),
        }),
    }
}

fn parse_usage(value: &serde_json::Value) -> Option<SearchUsage> {
    let usage = value.get("usage")?;
    let input_tokens = usage
        .get("input_tokens")
        .and_then(serde_json::Value::as_u64);
    let output_tokens = usage
        .get("output_tokens")
        .and_then(serde_json::Value::as_u64);
    let cache_read_input_tokens = usage
        .get("cache_read_input_tokens")
        .and_then(serde_json::Value::as_u64);
    let cache_creation_input_tokens = usage
        .get("cache_creation_input_tokens")
        .and_then(serde_json::Value::as_u64);
    let server_web_search_requests = usage
        .get("server_tool_use")
        .and_then(|tool_use| tool_use.get("web_search_requests"))
        .and_then(serde_json::Value::as_u64);
    if input_tokens.is_none()
        && output_tokens.is_none()
        && cache_read_input_tokens.is_none()
        && cache_creation_input_tokens.is_none()
        && server_web_search_requests.is_none()
    {
        return None;
    }
    Some(SearchUsage {
        input_tokens,
        output_tokens,
        cache_read_input_tokens,
        cache_creation_input_tokens,
        server_web_search_requests,
    })
}

fn parse_model(value: &serde_json::Value) -> Option<String> {
    non_empty(value.get("model"))
}

fn non_empty(value: Option<&serde_json::Value>) -> Option<String> {
    value
        .and_then(serde_json::Value::as_str)
        .filter(|text| !text.is_empty())
        .map(str::to_string)
}

fn http_error_message(status: u16, bytes: &[u8], parsed: Option<&serde_json::Value>) -> String {
    let detail = parsed.and_then(|value| {
        value
            .get("error")
            .and_then(|error| error.get("message"))
            .and_then(|message| message.as_str())
            .or_else(|| value.get("error").and_then(|error| error.as_str()))
            .or_else(|| value.get("message").and_then(|message| message.as_str()))
            .filter(|detail| !detail.is_empty())
    });
    let text = detail
        .map(str::to_string)
        .unwrap_or_else(|| String::from_utf8_lossy(bytes).into_owned());
    let snippet: String = text
        .chars()
        .filter(|character| !character.is_control())
        .take(MAX_ERROR_BODY_CHARS)
        .collect();
    if snippet.is_empty() {
        format!("HTTP {status}")
    } else {
        snippet
    }
}

use std::collections::HashMap;
use std::time::Duration;

use pl_protocol::{PureError, Result};
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Serialize};

use crate::ProviderInfo;

const WEB_SEARCH_REQUEST_TIMEOUT: Duration = Duration::from_secs(60);

/// Web 搜索访问模式。
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WebSearchMode {
    Disabled,
    #[default]
    Cached,
    Indexed,
    Live,
}

impl WebSearchMode {
    pub fn is_disabled(self) -> bool {
        self == Self::Disabled
    }
}

/// Hosted 与独立搜索共用的上下文规模。
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum WebSearchContextSize {
    Low,
    Medium,
    High,
}

/// Web 搜索使用的近似位置。
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct WebSearchLocation {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub country: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub region: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub city: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timezone: Option<String>,
}

impl WebSearchLocation {
    pub fn is_empty(&self) -> bool {
        self.country.as_deref().is_none_or(str::is_empty)
            && self.region.as_deref().is_none_or(str::is_empty)
            && self.city.as_deref().is_none_or(str::is_empty)
            && self.timezone.as_deref().is_none_or(str::is_empty)
    }
}

/// Web 搜索的产品无关配置。
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct WebSearchConfig {
    #[serde(default)]
    pub mode: WebSearchMode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_size: Option<WebSearchContextSize>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub allowed_domains: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub location: Option<WebSearchLocation>,
}

impl WebSearchConfig {
    pub fn is_default(&self) -> bool {
        self == &Self::default()
    }
}

/// Responses hosted web search 的域名过滤器。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WebSearchFilters {
    pub allowed_domains: Vec<String>,
}

/// Responses hosted web search 的近似位置 wire 值。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WebSearchUserLocation {
    #[serde(rename = "type")]
    pub kind: WebSearchUserLocationType,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub country: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub region: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub city: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timezone: Option<String>,
}

impl From<&WebSearchLocation> for WebSearchUserLocation {
    fn from(location: &WebSearchLocation) -> Self {
        Self {
            kind: WebSearchUserLocationType::Approximate,
            country: location.country.clone(),
            region: location.region.clone(),
            city: location.city.clone(),
            timezone: location.timezone.clone(),
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum WebSearchUserLocationType {
    Approximate,
}

/// Provider-neutral Web Search 活动。
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum WebSearchAction {
    Search {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        query: Option<String>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        queries: Vec<String>,
    },
    OpenPage {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        url: Option<String>,
    },
    FindInPage {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        url: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pattern: Option<String>,
    },
    #[default]
    Other,
}

/// 独立 `/alpha/search` 请求。
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct SearchRequest {
    pub id: String,
    pub model: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input: Option<Vec<serde_json::Value>>,
    pub commands: SearchCommands,
    pub settings: SearchSettings,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_output_tokens: Option<u64>,
}

/// 独立搜索的完整命令集合。
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct SearchCommands {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub search_query: Option<Vec<SearchQuery>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image_query: Option<Vec<SearchQuery>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub open: Option<Vec<OpenOperation>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub click: Option<Vec<ClickOperation>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub find: Option<Vec<FindOperation>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub screenshot: Option<Vec<ScreenshotOperation>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finance: Option<Vec<FinanceOperation>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub weather: Option<Vec<WeatherOperation>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sports: Option<Vec<SportsOperation>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub time: Option<Vec<TimeOperation>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub response_length: Option<SearchResponseLength>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SearchQuery {
    pub q: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recency: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub domains: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct OpenOperation {
    pub ref_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lineno: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ClickOperation {
    pub ref_id: String,
    pub id: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FindOperation {
    pub ref_id: String,
    pub pattern: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ScreenshotOperation {
    pub ref_id: String,
    pub pageno: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FinanceOperation {
    pub ticker: String,
    #[serde(rename = "type")]
    pub asset_type: FinanceAssetType,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub market: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum FinanceAssetType {
    Equity,
    Fund,
    Crypto,
    Index,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WeatherOperation {
    pub location: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SportsOperation {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool: Option<SportsToolName>,
    #[serde(rename = "fn")]
    pub function: SportsFunction,
    pub league: SportsLeague,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub team: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub opponent: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub date_from: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub date_to: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub num_games: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub locale: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum SportsToolName {
    Sports,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum SportsFunction {
    Schedule,
    Standings,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum SportsLeague {
    Nba,
    Wnba,
    Nfl,
    Nhl,
    Mlb,
    Epl,
    Ncaamb,
    Ncaawb,
    Ipl,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TimeOperation {
    pub utc_offset: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum SearchResponseLength {
    Short,
    Medium,
    Long,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SearchSettings {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user_location: Option<WebSearchUserLocation>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub search_context_size: Option<WebSearchContextSize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub filters: Option<WebSearchFilters>,
    pub allowed_callers: Vec<SearchAllowedCaller>,
    pub external_web_access: ExternalWebAccess,
}

impl SearchSettings {
    pub fn from_config(config: &WebSearchConfig) -> Self {
        Self {
            user_location: config
                .location
                .as_ref()
                .filter(|location| !location.is_empty())
                .map(Into::into),
            search_context_size: config.context_size,
            filters: (!config.allowed_domains.is_empty()).then(|| WebSearchFilters {
                allowed_domains: config.allowed_domains.clone(),
            }),
            allowed_callers: vec![SearchAllowedCaller::Direct],
            external_web_access: ExternalWebAccess::from_mode(config.mode),
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum SearchAllowedCaller {
    Direct,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(untagged)]
pub enum ExternalWebAccess {
    Boolean(bool),
    Mode(ExternalWebAccessMode),
}

impl ExternalWebAccess {
    pub fn from_mode(mode: WebSearchMode) -> Self {
        match mode {
            WebSearchMode::Disabled | WebSearchMode::Cached => Self::Boolean(false),
            WebSearchMode::Indexed => Self::Mode(ExternalWebAccessMode::Indexed),
            WebSearchMode::Live => Self::Boolean(true),
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ExternalWebAccessMode {
    Indexed,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct SearchResponse {
    #[serde(default)]
    pub encrypted_output: Option<String>,
    pub output: String,
    #[serde(default)]
    pub results: Option<Vec<serde_json::Value>>,
}

/// 只负责兼容 `/alpha/search` dialect 的 HTTP 客户端。
#[derive(Debug, Clone)]
pub struct WebSearchClient {
    client: reqwest::Client,
    endpoint: String,
    bearer_token: SecretString,
    headers: HeaderMap,
}

impl WebSearchClient {
    /// 使用已解析的 provider runtime 信息创建客户端。
    ///
    /// 服务能力由上层 planner 校验；此处再次拒绝空凭据，保证无凭据时
    /// 客户端不可构造。
    pub fn new(provider: &ProviderInfo) -> Result<Self> {
        Self::with_timeout(provider, WEB_SEARCH_REQUEST_TIMEOUT)
    }

    fn with_timeout(provider: &ProviderInfo, timeout: Duration) -> Result<Self> {
        let token = provider
            .bearer_token
            .as_deref()
            .filter(|token| !token.trim().is_empty())
            .ok_or_else(|| {
                PureError::ConfigError(
                    "standalone web search requires a non-empty bearer token".to_string(),
                )
            })?;
        let headers = configured_headers(provider.http_headers.as_ref())?;
        let client = reqwest::Client::builder()
            .timeout(timeout)
            .build()
            .map_err(|error| {
                PureError::ConfigError(format!("failed to build web search client: {error}"))
            })?;
        Ok(Self {
            client,
            endpoint: format!("{}/alpha/search", provider.base_url.trim_end_matches('/')),
            bearer_token: SecretString::from(token.to_string()),
            headers,
        })
    }

    pub async fn search(&self, request: &SearchRequest) -> Result<SearchResponse> {
        let response = self
            .client
            .post(&self.endpoint)
            .headers(self.headers.clone())
            .bearer_auth(self.bearer_token.expose_secret())
            .json(request)
            .send()
            .await
            .map_err(|error| PureError::LlmError(format!("web search request failed: {error}")))?;
        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            let message = body.chars().take(1000).collect::<String>();
            return Err(PureError::LlmError(format!(
                "web search request returned {status}: {message}"
            )));
        }
        response
            .json::<SearchResponse>()
            .await
            .map_err(|error| PureError::LlmError(format!("invalid web search response: {error}")))
    }
}

fn configured_headers(headers: Option<&HashMap<String, String>>) -> Result<HeaderMap> {
    let mut result = HeaderMap::new();
    for (name, value) in headers.into_iter().flatten() {
        if name.eq_ignore_ascii_case("authorization") {
            continue;
        }
        let name = HeaderName::from_bytes(name.as_bytes()).map_err(|error| {
            PureError::ConfigError(format!("invalid provider header name `{name}`: {error}"))
        })?;
        let value = HeaderValue::from_str(value).map_err(|error| {
            PureError::ConfigError(format!(
                "invalid provider header value for `{name}`: {error}"
            ))
        })?;
        result.insert(name, value);
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;
    use tokio::sync::oneshot;

    use super::*;

    #[test]
    fn search_modes_map_to_external_access() {
        assert_eq!(
            [
                WebSearchMode::Disabled,
                WebSearchMode::Cached,
                WebSearchMode::Indexed,
                WebSearchMode::Live,
            ]
            .map(ExternalWebAccess::from_mode),
            [
                ExternalWebAccess::Boolean(false),
                ExternalWebAccess::Boolean(false),
                ExternalWebAccess::Mode(ExternalWebAccessMode::Indexed),
                ExternalWebAccess::Boolean(true),
            ]
        );
    }

    #[test]
    fn client_rejects_missing_credentials() {
        let provider = ProviderInfo::openai(None);
        let error = WebSearchClient::new(&provider).unwrap_err().to_string();
        assert!(error.contains("bearer token"));
    }

    #[tokio::test]
    async fn client_posts_full_request_with_auth_headers_and_opaque_results() {
        let (base_url, captured) = mock_search_server(
            "200 OK",
            serde_json::json!({
                "encrypted_output": "ignored-by-caller",
                "output": "model-visible output",
                "results": [{
                    "url": "https://example.com/result",
                    "unknownFutureField": {"rank": 1}
                }]
            })
            .to_string(),
            Duration::ZERO,
        )
        .await;
        let mut provider = ProviderInfo::openai(None);
        provider.bearer_token = Some("test-secret".to_string());
        provider.base_url = format!("{base_url}/v1");
        provider.http_headers = Some(HashMap::from([
            ("x-account".to_string(), "account-a".to_string()),
            ("authorization".to_string(), "Bearer wrong".to_string()),
        ]));
        let client = WebSearchClient::new(&provider).expect("client");
        let request = sample_request(WebSearchMode::Indexed);

        let response = client.search(&request).await.expect("search response");
        let raw_request = captured.await.expect("captured request");

        assert!(raw_request.starts_with("POST /v1/alpha/search HTTP/1.1\r\n"));
        assert!(
            raw_request
                .to_ascii_lowercase()
                .contains("x-account: account-a")
        );
        let normalized_request = raw_request.to_ascii_lowercase();
        assert!(normalized_request.contains("authorization: bearer test-secret"));
        assert!(!normalized_request.contains("authorization: bearer wrong"));
        let body = raw_request
            .split_once("\r\n\r\n")
            .map(|(_, body)| body)
            .expect("request body");
        let body: serde_json::Value = serde_json::from_str(body).expect("request json");
        assert_eq!(body["id"], "session-1:call-1");
        assert_eq!(body["model"], "gpt-search");
        assert_eq!(body["settings"]["external_web_access"], "indexed");
        assert_eq!(
            body["settings"]["filters"]["allowed_domains"][0],
            "example.com"
        );
        assert_eq!(body["commands"]["search_query"][0]["q"], "pure lang");
        assert_eq!(response.output, "model-visible output");
        assert_eq!(
            response.results.expect("results")[0]["unknownFutureField"]["rank"],
            1
        );
    }

    #[tokio::test]
    async fn client_returns_structured_service_error() {
        let (base_url, _captured) = mock_search_server(
            "429 Too Many Requests",
            "rate limited".to_string(),
            Duration::ZERO,
        )
        .await;
        let mut provider = ProviderInfo::openai(None);
        provider.bearer_token = Some("test-secret".to_string());
        provider.base_url = base_url;
        let client = WebSearchClient::new(&provider).expect("client");

        let error = client
            .search(&sample_request(WebSearchMode::Cached))
            .await
            .expect_err("service error")
            .to_string();

        assert!(error.contains("429"));
        assert!(error.contains("rate limited"));
        assert!(!error.contains("test-secret"));
    }

    #[tokio::test]
    async fn client_enforces_request_timeout() {
        let (base_url, _captured) = mock_search_server(
            "200 OK",
            serde_json::json!({"output": "late", "results": []}).to_string(),
            Duration::from_millis(200),
        )
        .await;
        let mut provider = ProviderInfo::openai(None);
        provider.bearer_token = Some("test-secret".to_string());
        provider.base_url = base_url;
        let client =
            WebSearchClient::with_timeout(&provider, Duration::from_millis(20)).expect("client");

        let error = client
            .search(&sample_request(WebSearchMode::Live))
            .await
            .expect_err("timeout")
            .to_string();

        assert!(error.contains("web search request failed"));
        assert!(!error.contains("test-secret"));
    }

    fn sample_request(mode: WebSearchMode) -> SearchRequest {
        SearchRequest {
            id: "session-1:call-1".to_string(),
            model: "gpt-search".to_string(),
            input: Some(vec![serde_json::json!({
                "type": "message",
                "role": "user",
                "content": [{"type": "input_text", "text": "search"}]
            })]),
            commands: SearchCommands {
                search_query: Some(vec![SearchQuery {
                    q: "pure lang".to_string(),
                    recency: Some(7),
                    domains: Some(vec!["example.com".to_string()]),
                }]),
                response_length: Some(SearchResponseLength::Short),
                ..SearchCommands::default()
            },
            settings: SearchSettings::from_config(&WebSearchConfig {
                mode,
                context_size: Some(WebSearchContextSize::High),
                allowed_domains: vec!["example.com".to_string()],
                location: Some(WebSearchLocation {
                    country: Some("US".to_string()),
                    region: Some("CA".to_string()),
                    city: Some("San Francisco".to_string()),
                    timezone: Some("America/Los_Angeles".to_string()),
                }),
            }),
            max_output_tokens: Some(4096),
        }
    }

    async fn mock_search_server(
        status: &'static str,
        body: String,
        response_delay: Duration,
    ) -> (String, oneshot::Receiver<String>) {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind mock server");
        let address = listener.local_addr().expect("mock server address");
        let (request_tx, request_rx) = oneshot::channel();
        tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.expect("accept request");
            let mut request = Vec::new();
            let mut buffer = [0_u8; 4096];
            loop {
                let read = stream.read(&mut buffer).await.expect("read request");
                if read == 0 {
                    break;
                }
                request.extend_from_slice(&buffer[..read]);
                if request_is_complete(&request) {
                    break;
                }
            }
            let _ = request_tx.send(String::from_utf8_lossy(&request).into_owned());
            tokio::time::sleep(response_delay).await;
            let response = format!(
                "HTTP/1.1 {status}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = stream.write_all(response.as_bytes()).await;
        });
        (format!("http://{address}"), request_rx)
    }

    fn request_is_complete(request: &[u8]) -> bool {
        let text = String::from_utf8_lossy(request);
        let Some((headers, body)) = text.split_once("\r\n\r\n") else {
            return false;
        };
        let content_length = headers.lines().find_map(|line| {
            let (name, value) = line.split_once(':')?;
            name.eq_ignore_ascii_case("content-length")
                .then(|| value.trim().parse::<usize>().ok())
                .flatten()
        });
        content_length.is_none_or(|length| body.len() >= length)
    }
}

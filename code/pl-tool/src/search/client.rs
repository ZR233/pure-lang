use std::collections::HashMap;
use std::time::Duration;

use pl_protocol::search::{SearchRequest, SearchResponse};
use pl_protocol::{PureError, Result};
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use secrecy::{ExposeSecret, SecretString};

const WEB_SEARCH_REQUEST_TIMEOUT: Duration = Duration::from_secs(60);

/// Explicit standalone search transport parameters. Credentials are supplied by the host.
#[derive(Default)]
pub struct SearchEndpoint {
    pub base_url: String,
    pub bearer_token: Option<String>,
    pub http_headers: Option<HashMap<String, String>>,
}

impl std::fmt::Debug for SearchEndpoint {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SearchEndpoint").finish_non_exhaustive()
    }
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
    /// Builds the standalone `/alpha/search` client from a provider endpoint.
    pub fn new(provider: &SearchEndpoint) -> Result<Self> {
        Self::with_timeout(provider, WEB_SEARCH_REQUEST_TIMEOUT)
    }

    fn with_timeout(provider: &SearchEndpoint, timeout: Duration) -> Result<Self> {
        let token = provider
            .bearer_token
            .as_deref()
            .filter(|token| !token.trim().is_empty())
            .ok_or_else(|| {
                PureError::ConfigError(
                    "standalone web search requires a non-empty bearer token".to_string(),
                )
            })?;
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
            headers: configured_headers(provider.http_headers.as_ref())?,
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

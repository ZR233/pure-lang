use std::collections::HashMap;
use std::time::Duration;

use pl_model::{ProviderEndpoint, SearchRequest, SearchResponse};
use pl_protocol::{PureError, Result};
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use secrecy::{ExposeSecret, SecretString};

const WEB_SEARCH_REQUEST_TIMEOUT: Duration = Duration::from_secs(60);

/// 只负责兼容 `/alpha/search` dialect 的 HTTP 客户端。
#[derive(Debug, Clone)]
pub(crate) struct WebSearchClient {
    client: reqwest::Client,
    endpoint: String,
    bearer_token: SecretString,
    headers: HeaderMap,
}

impl WebSearchClient {
    pub(crate) fn new(provider: &ProviderEndpoint) -> Result<Self> {
        Self::with_timeout(provider, WEB_SEARCH_REQUEST_TIMEOUT)
    }

    fn with_timeout(provider: &ProviderEndpoint, timeout: Duration) -> Result<Self> {
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

    pub(crate) async fn search(&self, request: &SearchRequest) -> Result<SearchResponse> {
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
    use std::time::Duration;

    use pl_model::{
        ProviderEndpoint, SearchCommands, SearchQuery, SearchResponseLength, SearchSettings,
        WebSearchConfig, WebSearchContextSize, WebSearchLocation, WebSearchMode,
    };
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;
    use tokio::sync::oneshot;

    use super::*;

    #[test]
    fn rejects_missing_credentials() {
        let provider = ProviderEndpoint::openai(None);
        let error = WebSearchClient::new(&provider).unwrap_err().to_string();
        assert!(error.contains("bearer token"));
    }

    #[tokio::test]
    async fn posts_full_request_with_auth_headers_and_opaque_results() {
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
        let mut provider = ProviderEndpoint::openai(None);
        provider.bearer_token = Some("test-secret".to_string());
        provider.base_url = format!("{base_url}/v1");
        provider.http_headers = Some(HashMap::from([
            ("x-account".to_string(), "account-a".to_string()),
            ("authorization".to_string(), "Bearer wrong".to_string()),
        ]));
        let client = WebSearchClient::new(&provider).expect("client");

        let response = client
            .search(&sample_request(WebSearchMode::Indexed))
            .await
            .expect("search response");
        let raw_request = captured.await.expect("captured request");

        assert!(raw_request.starts_with("POST /v1/alpha/search HTTP/1.1\r\n"));
        let normalized_request = raw_request.to_ascii_lowercase();
        assert!(normalized_request.contains("x-account: account-a"));
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
    async fn returns_structured_service_error_without_credentials() {
        let (base_url, _captured) = mock_search_server(
            "429 Too Many Requests",
            "rate limited".to_string(),
            Duration::ZERO,
        )
        .await;
        let mut provider = ProviderEndpoint::openai(None);
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
    async fn enforces_request_timeout_without_credentials() {
        let (base_url, _captured) = mock_search_server(
            "200 OK",
            serde_json::json!({"output": "late", "results": []}).to_string(),
            Duration::from_millis(200),
        )
        .await;
        let mut provider = ProviderEndpoint::openai(None);
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

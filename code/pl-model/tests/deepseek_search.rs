//! 公开 API 集成测试：DeepSeek 原生独立 Web Search 的 wire、解析与失败路径。
//!
//! 全部用例通过真实 loopback HTTP 服务驱动 [`SearchClient`]，只消费公开类型；不访问任何真实
//! 供应商，也不断言线上兼容性。

use std::time::Duration;

use base64::Engine;
use pl_model::config::{
    ModelCatalogId, ProviderCapabilitySelection, ProviderConfig, ProviderPresetId,
    builtin_provider_catalog,
};
use pl_model::provider::deepseek::search::{
    CancellationToken, SearchClient, SearchError, SearchOptions, SearchRequest, SearchResponse,
};
use pl_model::provider::{
    ProviderEndpoint, ProviderServiceCapabilities, StandaloneWebSearchDialect,
    WebSearchProviderCapabilities,
};
use serde_json::{Value, json};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::oneshot;

/// 一次 loopback 响应脚本。
enum Reply {
    Json {
        status: u16,
        body: Value,
    },
    Raw {
        status: u16,
        content_type: &'static str,
        body: Vec<u8>,
    },
    Redirect {
        status: u16,
        location: String,
    },
    /// 只发送响应头与声明的 Content-Length，正文永远不到达。
    HeadersOnly {
        status: u16,
        declared_length: usize,
    },
    /// 接受连接并读取请求，但永不响应。
    Hang,
}

struct MockServer {
    base_url: String,
    request: Option<oneshot::Receiver<String>>,
    task: Option<tokio::task::JoinHandle<()>>,
}

impl MockServer {
    fn base_url(&self) -> String {
        self.base_url.clone()
    }

    async fn request(&mut self) -> String {
        self.request
            .take()
            .expect("server request already consumed")
            .await
            .expect("mock server did not capture a request")
    }

    async fn assert_no_request(&mut self) {
        let receiver = self
            .request
            .take()
            .expect("server request already consumed");
        if tokio::time::timeout(Duration::from_millis(200), receiver)
            .await
            .is_ok()
        {
            panic!("expected no HTTP request to be dispatched");
        }
    }

    async fn finish(mut self) {
        if let Some(task) = self.task.take() {
            task.await.expect("mock server task panicked");
        }
    }

    fn abort(&mut self) {
        if let Some(task) = self.task.take() {
            task.abort();
        }
    }
}

async fn mock_server(reply: Reply) -> MockServer {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base_url = format!("http://{}", listener.local_addr().unwrap());
    let (sender, request) = oneshot::channel();
    let task = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.expect("accept");
        socket.set_nodelay(true).unwrap();
        let captured = read_request(&mut socket).await;
        let _ = sender.send(String::from_utf8_lossy(&captured).into_owned());
        match reply {
            Reply::Json { status, body } => {
                let payload = serde_json::to_vec(&body).unwrap();
                write_response(&mut socket, status, "application/json", &payload).await;
            }
            Reply::Raw {
                status,
                content_type,
                body,
            } => {
                write_response(&mut socket, status, content_type, &body).await;
            }
            Reply::Redirect { status, location } => {
                let head = format!(
                    "HTTP/1.1 {status} {}\r\nLocation: {location}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                    reason(status),
                );
                socket.write_all(head.as_bytes()).await.unwrap();
                socket.flush().await.unwrap();
            }
            Reply::HeadersOnly {
                status,
                declared_length,
            } => {
                let head = format!(
                    "HTTP/1.1 {status} {}\r\nContent-Type: application/json\r\nContent-Length: {declared_length}\r\nConnection: close\r\n\r\n",
                    reason(status),
                );
                socket.write_all(head.as_bytes()).await.unwrap();
                socket.flush().await.unwrap();
                tokio::time::sleep(Duration::from_secs(300)).await;
            }
            Reply::Hang => {
                tokio::time::sleep(Duration::from_secs(300)).await;
            }
        }
    });
    MockServer {
        base_url,
        request: Some(request),
        task: Some(task),
    }
}

async fn read_request(socket: &mut tokio::net::TcpStream) -> Vec<u8> {
    let mut request = Vec::new();
    let mut buffer = [0u8; 4096];
    loop {
        let count = socket.read(&mut buffer).await.unwrap();
        if count == 0 {
            break;
        }
        request.extend_from_slice(&buffer[..count]);
        if let Some(end) = request.windows(4).position(|window| window == b"\r\n\r\n") {
            let headers = String::from_utf8_lossy(&request[..end]).to_ascii_lowercase();
            let length = headers
                .lines()
                .find_map(|line| line.strip_prefix("content-length: "))
                .and_then(|value| value.trim().parse::<usize>().ok())
                .unwrap_or(0);
            if request.len() >= end + 4 + length {
                break;
            }
        }
    }
    request
}

async fn write_response(
    socket: &mut tokio::net::TcpStream,
    status: u16,
    content_type: &str,
    body: &[u8],
) {
    let head = format!(
        "HTTP/1.1 {status} {}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        reason(status),
        body.len(),
    );
    socket.write_all(head.as_bytes()).await.unwrap();
    socket.write_all(body).await.unwrap();
    socket.flush().await.unwrap();
}

fn reason(status: u16) -> &'static str {
    match status {
        200 => "OK",
        302 => "Found",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        429 => "Too Many Requests",
        500 => "Internal Server Error",
        _ => "Status",
    }
}

fn endpoint(base_url: impl Into<String>, credential: Option<&str>) -> ProviderEndpoint {
    let mut endpoint = ProviderEndpoint::deepseek(Some(base_url.into()));
    endpoint.bearer_token = credential.map(str::to_string);
    endpoint
}

fn header(request: &str, name: &str) -> Option<String> {
    request.lines().find_map(|line| {
        let (key, value) = line.split_once(':')?;
        key.eq_ignore_ascii_case(name)
            .then_some(value.trim().to_string())
    })
}

fn body_json(request: &str) -> Value {
    let (_, body) = request.split_once("\r\n\r\n").expect("request body");
    serde_json::from_str(body).expect("request body is JSON")
}

fn sources_response() -> Value {
    json!({
        "id": "msg_1",
        "model": "deepseek-flash",
        "content": [
            { "type": "text", "text": "answer", "citations": [
                { "type": "web_search_result_location", "url": "https://example.com/a", "title": "A", "cited_text": "alpha excerpt" },
                { "type": "web_search_result_location", "url": "https://example.com/b", "cited_text": "beta excerpt" }
            ]},
            { "type": "web_search_tool_result", "tool_use_id": "t1", "content": [
                { "type": "web_search_result", "url": "https://example.com/a", "title": "A", "page_age": "2024-01-01" },
                { "type": "web_search_result", "url": "https://example.com/b", "title": "B" }
            ]},
            { "type": "web_search_tool_result", "tool_use_id": "t2", "content": [
                { "type": "web_search_result", "url": "https://example.com/a", "title": "A duplicate" }
            ]}
        ],
        "usage": {
            "input_tokens": 10,
            "output_tokens": 20,
            "cache_read_input_tokens": 4,
            "cache_creation_input_tokens": 2,
            "server_tool_use": { "web_search_requests": 1 }
        }
    })
}

/// 断言已完整收到的非 JSON 正文以 base64 约定无损留存，并返回解码后的原始字节。
fn observed_raw_body(error: &SearchError) -> Vec<u8> {
    let observed = error.observed_response().expect("observed response");
    assert_eq!(
        observed.raw_response["encoding"],
        Value::String("base64".to_string()),
        "non-JSON bodies are keyed by their encoding"
    );
    assert!(
        observed.sources.is_empty(),
        "an unparsed body must not fabricate sources"
    );
    assert!(observed.usage.is_none(), "usage stays unknown");
    assert!(observed.model.is_none(), "model stays unknown");
    let data = observed.raw_response["data"]
        .as_str()
        .expect("base64 data payload");
    base64::engine::general_purpose::STANDARD
        .decode(data)
        .expect("valid base64 payload")
}

/// `SearchClient` 只从 provider base_url 派生 endpoint：保留自定义 path 前缀、归一化 canonical
/// `/v1` 版本别名，且不在已指向 `/anthropic` / `/anthropic/v1` 时重复追加。
#[test]
fn deepseek_search_derives_endpoint_from_provider_base_url() {
    for (base, expected) in [
        (
            "https://api.deepseek.com",
            "https://api.deepseek.com/anthropic/v1/messages",
        ),
        (
            "https://api.deepseek.com/",
            "https://api.deepseek.com/anthropic/v1/messages",
        ),
        (
            "https://api.deepseek.com/v1",
            "https://api.deepseek.com/anthropic/v1/messages",
        ),
        (
            "https://api.deepseek.com/anthropic",
            "https://api.deepseek.com/anthropic/v1/messages",
        ),
        (
            "https://api.deepseek.com/anthropic/v1",
            "https://api.deepseek.com/anthropic/v1/messages",
        ),
        (
            "https://api.deepseek.com/anthropic/v1/messages",
            "https://api.deepseek.com/anthropic/v1/messages",
        ),
        (
            "https://proxy.example.com/proxy",
            "https://proxy.example.com/proxy/anthropic/v1/messages",
        ),
        (
            "http://127.0.0.1:8080/proxy/anthropic/v1",
            "http://127.0.0.1:8080/proxy/anthropic/v1/messages",
        ),
    ] {
        let mut endpoint = ProviderEndpoint::deepseek(Some(base.to_string()));
        endpoint.bearer_token = Some("k".to_string());
        let client = SearchClient::new(&endpoint, SearchOptions::default()).unwrap();
        assert_eq!(client.endpoint(), expected, "base url {base}");
    }
}

/// 无效地址在发出任何请求前被拒绝，且绝不回退到官方域名。
#[test]
fn deepseek_search_rejects_invalid_base_urls() {
    for base in [
        "ftp://api.deepseek.com",
        "https://user:pass@api.deepseek.com",
        "https://api.deepseek.com?x=1",
        "https://api.deepseek.com#frag",
        "not a url",
    ] {
        let mut endpoint = ProviderEndpoint::deepseek(Some(base.to_string()));
        endpoint.bearer_token = Some("k".to_string());
        let result = SearchClient::new(&endpoint, SearchOptions::default());
        assert!(
            matches!(result, Err(SearchError::Config { .. })),
            "base url {base} should be rejected"
        );
    }
}

/// `Debug` 输出不泄漏凭据，只展示解析出的端点。
#[test]
fn deepseek_search_debug_does_not_leak_the_credential() {
    let mut endpoint = ProviderEndpoint::deepseek(Some("https://api.deepseek.com".to_string()));
    endpoint.bearer_token = Some("super-secret-token".to_string());
    let client = SearchClient::new(&endpoint, SearchOptions::default()).unwrap();
    let rendered = format!("{client:?}");
    assert!(!rendered.contains("super-secret-token"));
    assert!(rendered.contains("/anthropic/v1/messages"));
}

#[tokio::test]
async fn deepseek_search_sends_native_messages_request() {
    let mut server = mock_server(Reply::Json {
        status: 200,
        body: sources_response(),
    })
    .await;
    let options = SearchOptions {
        max_tokens: 2048,
        max_uses: 3,
        ..SearchOptions::default()
    };
    let client = SearchClient::new(
        &endpoint(format!("{}/proxy", server.base_url()), Some("secret-key")),
        options,
    )
    .unwrap();
    let response = client
        .search(&SearchRequest::new("rust async"), CancellationToken::new())
        .await
        .unwrap();

    let request = server.request().await;
    assert_eq!(
        request.lines().next().unwrap(),
        "POST /proxy/anthropic/v1/messages HTTP/1.1"
    );
    assert_eq!(header(&request, "x-api-key").as_deref(), Some("secret-key"));
    assert_eq!(
        header(&request, "anthropic-version").as_deref(),
        Some("2023-06-01")
    );
    assert_eq!(
        header(&request, "content-type").as_deref(),
        Some("application/json")
    );
    assert!(header(&request, "authorization").is_none());

    let body = body_json(&request);
    assert_eq!(body["model"], "deepseek-flash");
    assert_eq!(body["max_tokens"], 2048);
    assert_eq!(body["messages"].as_array().unwrap().len(), 1);
    assert_eq!(body["messages"][0]["role"], "user");
    assert_eq!(body["messages"][0]["content"][0]["type"], "text");
    assert_eq!(
        body["messages"][0]["content"][0]["text"],
        "Perform a web search for the query: rust async\n\nReturn source citations."
    );
    assert_eq!(body["tools"][0]["type"], "web_search_20250305");
    assert_eq!(body["tools"][0]["name"], "web_search");
    assert_eq!(body["tools"][0]["max_uses"], 3);

    assert_eq!(response.sources.len(), 2);
    assert_eq!(response.model.as_deref(), Some("deepseek-flash"));
    assert_eq!(
        response.usage.as_ref().and_then(|usage| usage.input_tokens),
        Some(10)
    );
    assert_eq!(
        response
            .usage
            .as_ref()
            .and_then(|usage| usage.output_tokens),
        Some(20)
    );
    assert_eq!(
        response
            .usage
            .as_ref()
            .and_then(|usage| usage.cache_read_input_tokens),
        Some(4)
    );
    assert_eq!(
        response
            .usage
            .as_ref()
            .and_then(|usage| usage.cache_creation_input_tokens),
        Some(2)
    );
    assert_eq!(
        response
            .usage
            .as_ref()
            .and_then(|usage| usage.server_web_search_requests),
        Some(1)
    );
    assert_eq!(response.raw_response["id"], "msg_1");
    server.finish().await;
}

#[tokio::test]
async fn deepseek_search_merges_and_dedupes_citations_by_first_url() {
    let server = mock_server(Reply::Json {
        status: 200,
        body: sources_response(),
    })
    .await;
    let client = SearchClient::new(
        &endpoint(server.base_url(), Some("k")),
        SearchOptions::default(),
    )
    .unwrap();
    let response = client
        .search(&SearchRequest::new("query"), CancellationToken::new())
        .await
        .unwrap();

    assert_eq!(response.sources.len(), 2);
    assert_eq!(response.sources[0].url, "https://example.com/a");
    assert_eq!(response.sources[0].title.as_deref(), Some("A"));
    assert_eq!(
        response.sources[0].snippet.as_deref(),
        Some("alpha excerpt")
    );
    assert_eq!(
        response.sources[0].published_at.as_deref(),
        Some("2024-01-01")
    );
    assert_eq!(response.sources[1].url, "https://example.com/b");
    assert_eq!(response.sources[1].title.as_deref(), Some("B"));
    assert_eq!(response.sources[1].snippet.as_deref(), Some("beta excerpt"));
    assert!(response.sources[1].published_at.is_none());
    server.finish().await;
}

#[tokio::test]
async fn deepseek_search_accepts_a_legal_empty_result_array() {
    let server = mock_server(Reply::Json {
        status: 200,
        body: json!({
            "content": [
                { "type": "web_search_tool_result", "tool_use_id": "t1", "content": [] }
            ],
            "usage": { "input_tokens": 1, "output_tokens": 2 }
        }),
    })
    .await;
    let client = SearchClient::new(
        &endpoint(server.base_url(), Some("k")),
        SearchOptions::default(),
    )
    .unwrap();
    let response = client
        .search(&SearchRequest::new("query"), CancellationToken::new())
        .await
        .unwrap();
    assert!(response.sources.is_empty());
    assert_eq!(
        response.usage.as_ref().and_then(|usage| usage.input_tokens),
        Some(1)
    );
    server.finish().await;
}

#[tokio::test]
async fn deepseek_search_fails_on_a_tool_result_error_block() {
    let server = mock_server(Reply::Json {
        status: 200,
        body: json!({
            "model": "deepseek-flash",
            "content": [
                { "type": "web_search_tool_result", "tool_use_id": "t1",
                  "content": { "type": "web_search_tool_result_error", "error_code": "max_uses_exceeded" } }
            ]
        }),
    })
    .await;
    let client = SearchClient::new(
        &endpoint(server.base_url(), Some("k")),
        SearchOptions::default(),
    )
    .unwrap();
    let error = client
        .search(&SearchRequest::new("query"), CancellationToken::new())
        .await
        .unwrap_err();
    assert!(matches!(error, SearchError::ToolResultError { .. }));
    let observed = error.observed_response().expect("observed response");
    assert_eq!(
        observed.raw_response["content"][0]["content"]["error_code"],
        "max_uses_exceeded"
    );
    assert_eq!(observed.model.as_deref(), Some("deepseek-flash"));
    server.finish().await;
}

#[tokio::test]
async fn deepseek_search_fails_without_a_result_block() {
    let server = mock_server(Reply::Json {
        status: 200,
        body: json!({
            "model": "deepseek-flash",
            "content": [{ "type": "text", "text": "I could not search." }],
            "usage": { "input_tokens": 3, "output_tokens": 4 }
        }),
    })
    .await;
    let client = SearchClient::new(
        &endpoint(server.base_url(), Some("k")),
        SearchOptions::default(),
    )
    .unwrap();
    let error = client
        .search(&SearchRequest::new("query"), CancellationToken::new())
        .await
        .unwrap_err();
    assert!(matches!(error, SearchError::NoResults { .. }));
    let observed = error.observed_response().expect("observed response");
    assert_eq!(
        observed.usage.as_ref().and_then(|usage| usage.input_tokens),
        Some(3)
    );
    server.finish().await;
}

/// 完整收到但非 JSON 的 HTTP 失败正文必须无损留存：`Display` 错误有界，`observed_response`
/// 保留完整原始字节，且 usage/model 未知、不伪造来源。
#[tokio::test]
async fn deepseek_search_preserves_a_long_non_json_http_failure_body() {
    let body = format!("upstream boom {}", "x".repeat(4096));
    let server = mock_server(Reply::Raw {
        status: 500,
        content_type: "text/plain",
        body: body.clone().into_bytes(),
    })
    .await;
    let client = SearchClient::new(
        &endpoint(server.base_url(), Some("k")),
        SearchOptions::default(),
    )
    .unwrap();
    let error = client
        .search(&SearchRequest::new("query"), CancellationToken::new())
        .await
        .unwrap_err();
    match &error {
        SearchError::HttpStatus {
            status, message, ..
        } => {
            assert_eq!(*status, 500);
            assert!(message.contains("upstream boom"));
            assert!(message.chars().count() <= 512);
        }
        other => panic!("expected HttpStatus, got {other:?}"),
    }
    assert_eq!(observed_raw_body(&error), body.into_bytes());
    server.finish().await;
}

/// 2xx 但正文不是合法 JSON：错误仍然失败，完整原始正文可从 `observed_response` 无损取得，
/// usage/model 未知、不伪造搜索成功。
#[tokio::test]
async fn deepseek_search_preserves_a_malformed_json_success_body() {
    let body = format!("{{ not json {}", "y".repeat(2048));
    let server = mock_server(Reply::Raw {
        status: 200,
        content_type: "application/json",
        body: body.clone().into_bytes(),
    })
    .await;
    let client = SearchClient::new(
        &endpoint(server.base_url(), Some("k")),
        SearchOptions::default(),
    )
    .unwrap();
    let error = client
        .search(&SearchRequest::new("query"), CancellationToken::new())
        .await
        .unwrap_err();
    assert!(matches!(error, SearchError::InvalidResponse { .. }));
    assert_eq!(observed_raw_body(&error), body.into_bytes());
    server.finish().await;
}

/// 非 UTF-8 字节的完整正文同样无损留存：base64 载荷逐字节等于原始正文。
#[tokio::test]
async fn deepseek_search_preserves_non_utf8_response_bytes() {
    let body = vec![0xff, 0xfe, 0x00, 0x80, b'o', b'k', 0xc3, 0x28, 0xa0, 0xa1];
    let server = mock_server(Reply::Raw {
        status: 200,
        content_type: "application/json",
        body: body.clone(),
    })
    .await;
    let client = SearchClient::new(
        &endpoint(server.base_url(), Some("k")),
        SearchOptions::default(),
    )
    .unwrap();
    let error = client
        .search(&SearchRequest::new("query"), CancellationToken::new())
        .await
        .unwrap_err();
    assert!(matches!(error, SearchError::InvalidResponse { .. }));
    assert_eq!(observed_raw_body(&error), body);
    server.finish().await;
}

/// JSON error 详情同样有界，且仍然保留已解析的原始响应。
#[tokio::test]
async fn deepseek_search_bounds_json_error_messages() {
    let server = mock_server(Reply::Json {
        status: 429,
        body: json!({ "error": { "message": "x".repeat(2000) } }),
    })
    .await;
    let client = SearchClient::new(
        &endpoint(server.base_url(), Some("k")),
        SearchOptions::default(),
    )
    .unwrap();
    let error = client
        .search(&SearchRequest::new("query"), CancellationToken::new())
        .await
        .unwrap_err();
    match &error {
        SearchError::HttpStatus {
            status, message, ..
        } => {
            assert_eq!(*status, 429);
            assert!(message.chars().count() <= 512);
        }
        other => panic!("expected HttpStatus, got {other:?}"),
    }
    assert!(error.observed_response().is_some());
    server.finish().await;
}

#[tokio::test]
async fn deepseek_search_does_not_follow_redirects() {
    let mut server = mock_server(Reply::Redirect {
        status: 302,
        location: "https://example.com/elsewhere".to_string(),
    })
    .await;
    let client = SearchClient::new(
        &endpoint(server.base_url(), Some("k")),
        SearchOptions::default(),
    )
    .unwrap();
    let error = client
        .search(&SearchRequest::new("query"), CancellationToken::new())
        .await
        .unwrap_err();
    match &error {
        SearchError::HttpStatus { status, .. } => assert_eq!(*status, 302),
        other => panic!("expected HttpStatus, got {other:?}"),
    }
    let request = server.request().await;
    assert_eq!(
        request.lines().next().unwrap(),
        "POST /anthropic/v1/messages HTTP/1.1"
    );
    server.finish().await;
}

#[tokio::test]
async fn deepseek_search_cancellation_aborts_a_pending_body() {
    let mut server = mock_server(Reply::HeadersOnly {
        status: 200,
        declared_length: 64,
    })
    .await;
    let client = SearchClient::new(
        &endpoint(server.base_url(), Some("k")),
        SearchOptions::default(),
    )
    .unwrap();
    let cancellation = CancellationToken::new();
    let search = tokio::spawn({
        let cancellation = cancellation.clone();
        async move {
            client
                .search(&SearchRequest::new("query"), cancellation)
                .await
        }
    });
    let _ = server.request().await;
    cancellation.cancel();
    let error = search.await.unwrap().unwrap_err();
    assert!(matches!(error, SearchError::Cancelled { .. }));
    assert!(error.observed_response().is_none());
    server.abort();
}

#[tokio::test]
async fn deepseek_search_timeout_covers_the_whole_request() {
    let mut server = mock_server(Reply::Hang).await;
    let options = SearchOptions {
        timeout: Duration::from_millis(300),
        ..SearchOptions::default()
    };
    let client = SearchClient::new(&endpoint(server.base_url(), Some("k")), options).unwrap();
    let error = client
        .search(&SearchRequest::new("query"), CancellationToken::new())
        .await
        .unwrap_err();
    assert!(matches!(error, SearchError::Timeout { .. }));
    assert!(error.observed_response().is_none());
    server.abort();
}

#[tokio::test]
async fn deepseek_search_requires_a_credential_without_dispatching() {
    let mut server = mock_server(Reply::Json {
        status: 200,
        body: sources_response(),
    })
    .await;
    let result = SearchClient::new(&endpoint(server.base_url(), None), SearchOptions::default());
    assert!(matches!(result, Err(SearchError::MissingCredential)));
    server.assert_no_request().await;
    server.abort();
}

#[tokio::test]
async fn deepseek_search_rejects_invalid_parameters_without_dispatching() {
    let mut server = mock_server(Reply::Json {
        status: 200,
        body: sources_response(),
    })
    .await;
    for options in [
        SearchOptions {
            max_tokens: 0,
            ..SearchOptions::default()
        },
        SearchOptions {
            max_uses: 0,
            ..SearchOptions::default()
        },
        SearchOptions {
            model: "  ".to_string(),
            ..SearchOptions::default()
        },
        SearchOptions {
            timeout: Duration::ZERO,
            ..SearchOptions::default()
        },
    ] {
        let result = SearchClient::new(&endpoint(server.base_url(), Some("k")), options);
        assert!(matches!(result, Err(SearchError::Config { .. })));
    }
    server.assert_no_request().await;
    server.abort();
}

#[tokio::test]
async fn deepseek_search_rejects_an_empty_query_without_dispatching() {
    let mut server = mock_server(Reply::Json {
        status: 200,
        body: sources_response(),
    })
    .await;
    let client = SearchClient::new(
        &endpoint(server.base_url(), Some("k")),
        SearchOptions::default(),
    )
    .unwrap();
    let error = client
        .search(&SearchRequest::new("   "), CancellationToken::new())
        .await
        .unwrap_err();
    assert!(matches!(error, SearchError::EmptyQuery));
    server.assert_no_request().await;
    server.abort();
}

/// 公开配置解析的搜索能力门控：canonical preset 继承 DeepSeek 原生 standalone，非 canonical
/// DeepSeek PresetDefaults 仅撤销该 DeepSeek 方言，Explicit 声明仍可 opt-in；不启用旧的
/// Responses hosted `web_search`。该撤销只针对 DeepSeek 原生方言，OpenAI 自定义 endpoint 的
/// `/alpha/search` standalone 继承语义保持不变。
#[test]
fn deepseek_search_capability_gating_follows_canonical_preset_and_explicit_opt_in() {
    let canonical = ProviderConfig::deepseek_preset();
    let capabilities = canonical.service_capabilities().unwrap();
    assert_eq!(
        capabilities.web_search.standalone,
        Some(StandaloneWebSearchDialect::DeepSeekAnthropicMessages)
    );
    assert!(!capabilities.web_search.hosted_responses);

    let mut custom = ProviderConfig::deepseek_preset();
    custom.base_url = "https://proxy.example.com".to_string();
    let capabilities = custom.service_capabilities().unwrap();
    assert_eq!(capabilities.web_search.standalone, None);
    assert!(!capabilities.web_search.hosted_responses);

    custom.capabilities = ProviderCapabilitySelection::Explicit(ProviderServiceCapabilities {
        web_search: WebSearchProviderCapabilities {
            standalone: Some(StandaloneWebSearchDialect::DeepSeekAnthropicMessages),
            ..WebSearchProviderCapabilities::default()
        },
        ..ProviderServiceCapabilities::default()
    });
    let capabilities = custom.service_capabilities().unwrap();
    assert_eq!(
        capabilities.web_search.standalone,
        Some(StandaloneWebSearchDialect::DeepSeekAnthropicMessages)
    );

    // 非 canonical OpenAI endpoint 仍继承既有 `/alpha/search` standalone 能力；此撤销只针对
    // DeepSeek 原生方言，不改变 OpenAI standalone 的继承行为。
    let openai_custom = ProviderConfig::from_bundled_catalog(
        ProviderEndpoint::openai(Some("https://ai.muxai.net".to_string())),
        ModelCatalogId::new("openai").unwrap(),
        Vec::new(),
    )
    .with_preset(ProviderPresetId::new("openai").unwrap());
    let capabilities = openai_custom.service_capabilities().unwrap();
    assert_eq!(
        capabilities.web_search.standalone,
        Some(StandaloneWebSearchDialect::OpenAiSearchApi)
    );
}

/// canonical DeepSeek preset 必须声明 DeepSeek 原生 standalone 能力，并在临时 HTTP 环境上
/// 完成一次 native 请求。
#[tokio::test]
async fn deepseek_search_uses_canonical_provider_and_native_sources() {
    let registry = builtin_provider_catalog().unwrap();
    let preset = registry
        .presets
        .iter()
        .find(|preset| preset.id.as_str() == "deepseek")
        .expect("canonical deepseek preset");
    assert_eq!(
        preset.service_capabilities.web_search.standalone,
        Some(StandaloneWebSearchDialect::DeepSeekAnthropicMessages)
    );

    let mut server = mock_server(Reply::Json {
        status: 200,
        body: sources_response(),
    })
    .await;
    let mut endpoint = ProviderEndpoint::deepseek(Some(server.base_url()));
    endpoint.service_capabilities = preset.service_capabilities.clone();
    endpoint.bearer_token = Some("canonical-key".to_string());
    let client = SearchClient::new(&endpoint, SearchOptions::default()).unwrap();
    let response: SearchResponse = client
        .search(
            &SearchRequest::new("deepseek canonical"),
            CancellationToken::new(),
        )
        .await
        .unwrap();
    assert!(!response.sources.is_empty());

    let request = server.request().await;
    assert_eq!(
        header(&request, "x-api-key").as_deref(),
        Some("canonical-key")
    );
    server.finish().await;
}

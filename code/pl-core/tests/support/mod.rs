#![allow(dead_code)]

use tokio::io::{AsyncReadExt, AsyncWriteExt};

use pl_core::{AgentRoleId, ProviderEndpoint, ProviderId, ReasoningEffort, ResolvedModelRoute};
use pl_model::model::{ModelInfo, deepseek_default_model_slugs, default_models};

pub fn catalog_model(slug: &str) -> ModelInfo {
    default_models()
        .into_iter()
        .find(|model| model.slug == slug)
        .unwrap_or_else(|| ModelInfo::compatible(slug))
}

pub fn route(
    provider_id: &str,
    endpoint: ProviderEndpoint,
    model: ModelInfo,
    effort: Option<&str>,
) -> ResolvedModelRoute {
    ResolvedModelRoute {
        pricing_mode: pl_protocol::PricingMode::Catalog,
        role: AgentRoleId::new("live-test").expect("static role id is valid"),
        provider_id: ProviderId::new(provider_id).expect("static provider id is valid"),
        endpoint,
        model,
        effort: effort.map(ReasoningEffort::new),
    }
}

pub fn deepseek_route(api_key: String) -> ResolvedModelRoute {
    let mut endpoint = ProviderEndpoint::deepseek(None);
    endpoint.bearer_token = Some(api_key);
    let model = catalog_model(deepseek_default_model_slugs()[0]);
    route("deepseek", endpoint, model, Some("high"))
}

pub struct TestHttpResponse {
    pub status: u16,
    pub content_type: &'static str,
    pub body: String,
}

impl TestHttpResponse {
    pub fn sse(body: String) -> Self {
        Self {
            status: 200,
            content_type: "text/event-stream",
            body,
        }
    }
}

pub async fn serve_sse_once(sse_body: String) -> (String, tokio::task::JoinHandle<()>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let handle = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut buffer = Vec::new();
        let mut temp = [0_u8; 1024];
        let (header_end, content_length) = loop {
            let n = socket.read(&mut temp).await.unwrap();
            assert_ne!(n, 0);
            buffer.extend_from_slice(&temp[..n]);
            if let Some(header_end) = buffer.windows(4).position(|window| window == b"\r\n\r\n") {
                let headers = String::from_utf8_lossy(&buffer[..header_end]);
                let content_length = headers
                    .lines()
                    .find_map(|line| {
                        let (name, value) = line.split_once(':')?;
                        name.eq_ignore_ascii_case("content-length")
                            .then(|| value.trim().parse::<usize>().ok())?
                    })
                    .unwrap_or(0);
                break (header_end, content_length);
            }
        };

        while buffer.len() < header_end + 4 + content_length {
            let n = socket.read(&mut temp).await.unwrap();
            assert_ne!(n, 0);
            buffer.extend_from_slice(&temp[..n]);
        }

        let response = format!(
            "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
            sse_body.len(),
            sse_body
        );
        socket.write_all(response.as_bytes()).await.unwrap();
        socket.shutdown().await.unwrap();
    });

    (format!("http://{addr}"), handle)
}

pub async fn serve_sse_sequence(
    sse_bodies: Vec<String>,
) -> (
    String,
    std::sync::Arc<std::sync::Mutex<Vec<serde_json::Value>>>,
    tokio::task::JoinHandle<()>,
) {
    serve_http_sequence(sse_bodies.into_iter().map(TestHttpResponse::sse).collect()).await
}

pub async fn serve_sse_sequence_with_raw_requests(
    sse_bodies: Vec<String>,
) -> (
    String,
    std::sync::Arc<std::sync::Mutex<Vec<Vec<u8>>>>,
    tokio::task::JoinHandle<()>,
) {
    let raw_requests = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let (base_url, _json_bodies, handle) = serve_http_sequence_capturing(
        sse_bodies.into_iter().map(TestHttpResponse::sse).collect(),
        Some(raw_requests.clone()),
        None,
    )
    .await;
    (base_url, raw_requests, handle)
}

pub async fn serve_http_sequence(
    responses: Vec<TestHttpResponse>,
) -> (
    String,
    std::sync::Arc<std::sync::Mutex<Vec<serde_json::Value>>>,
    tokio::task::JoinHandle<()>,
) {
    serve_http_sequence_capturing(responses, None, None).await
}

pub async fn serve_checked_sse_sequence(
    responses: Vec<String>,
    accepts: impl Fn(usize, &serde_json::Value) -> bool + Send + 'static,
) -> (String, tokio::task::JoinHandle<()>) {
    let (url, _, server) = serve_http_sequence_capturing(
        responses.into_iter().map(TestHttpResponse::sse).collect(),
        None,
        Some(Box::new(accepts)),
    )
    .await;
    (url, server)
}

type RequestAcceptance = Box<dyn Fn(usize, &serde_json::Value) -> bool + Send>;

async fn serve_http_sequence_capturing(
    responses: Vec<TestHttpResponse>,
    raw_requests: Option<std::sync::Arc<std::sync::Mutex<Vec<Vec<u8>>>>>,
    accepts: Option<RequestAcceptance>,
) -> (
    String,
    std::sync::Arc<std::sync::Mutex<Vec<serde_json::Value>>>,
    tokio::task::JoinHandle<()>,
) {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let bodies = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let captured = bodies.clone();
    let handle = tokio::spawn(async move {
        for (index, mut response) in responses.into_iter().enumerate() {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut buffer = Vec::new();
            let mut temp = [0_u8; 1024];
            let (header_end, content_length) = loop {
                let n = socket.read(&mut temp).await.unwrap();
                assert_ne!(n, 0);
                buffer.extend_from_slice(&temp[..n]);
                if let Some(header_end) = buffer.windows(4).position(|window| window == b"\r\n\r\n")
                {
                    let headers = String::from_utf8_lossy(&buffer[..header_end]);
                    let content_length = headers
                        .lines()
                        .find_map(|line| {
                            let (name, value) = line.split_once(':')?;
                            name.eq_ignore_ascii_case("content-length")
                                .then(|| value.trim().parse::<usize>().ok())?
                        })
                        .unwrap_or(0);
                    break (header_end, content_length);
                }
            };

            while buffer.len() < header_end + 4 + content_length {
                let n = socket.read(&mut temp).await.unwrap();
                assert_ne!(n, 0);
                buffer.extend_from_slice(&temp[..n]);
            }
            let body = &buffer[header_end + 4..header_end + 4 + content_length];
            if let Some(raw_requests) = &raw_requests {
                raw_requests.lock().unwrap().push(body.to_vec());
            }
            let body: serde_json::Value = serde_json::from_slice(body).unwrap();
            if accepts
                .as_ref()
                .is_some_and(|accepts| !accepts(index, &body))
            {
                response.status = 400;
                response.content_type = "application/json";
                response.body = serde_json::json!({"error":{"message":"tool task history or native options were not accepted"}}).to_string();
            }
            captured.lock().unwrap().push(body);

            let response = format!(
                "HTTP/1.1 {} {}\r\ncontent-type: {}\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
                response.status,
                if response.status >= 400 {
                    "Error"
                } else {
                    "OK"
                },
                response.content_type,
                response.body.len(),
                response.body
            );
            socket.write_all(response.as_bytes()).await.unwrap();
            socket.shutdown().await.unwrap();
        }
    });

    (format!("http://{addr}"), bodies, handle)
}

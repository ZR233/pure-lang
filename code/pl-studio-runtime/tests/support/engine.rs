use pl_model::config::{ProviderId, ReasoningEffort, ResolvedModelRoute};
use pl_model::model::ModelInfo;
use pl_model::provider::ProviderEndpoint;
use pl_protocol::AgentRoleId;

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

pub async fn serve_checked_sse_sequence(
    responses: Vec<String>,
    accepts: impl Fn(usize, &serde_json::Value) -> bool + Send + 'static,
) -> (String, tokio::task::JoinHandle<()>) {
    let (url, _, server) = serve_http_sequence_capturing(
        responses.into_iter().map(TestHttpResponse::sse).collect(),
        Some(Box::new(accepts)),
    )
    .await;
    (url, server)
}

type RequestAcceptance = Box<dyn Fn(usize, &serde_json::Value) -> bool + Send>;

async fn serve_http_sequence_capturing(
    responses: Vec<TestHttpResponse>,
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

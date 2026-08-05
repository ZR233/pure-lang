use std::time::Duration;

use crate::{InteractionKind, InteractionPayload, InteractionScope, InteractionStatus};
use pl_model::{ModelInfo, ProviderConnectionMode, ProviderInfo};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::oneshot;

use super::*;
use crate::config::{ModelRouteConfig, ProviderId, ReasoningEffort, StudioConfig, StudioRole};
use crate::{StudioMode, StudioRuntimeStatus};

const TEST_RUNTIME_TIMEOUT: Duration = Duration::from_secs(20);

struct TestHttpResponse {
    status_line: &'static str,
    content_type: &'static str,
    body: String,
}

impl TestHttpResponse {
    fn sse(body: String) -> Self {
        Self {
            status_line: "200 OK",
            content_type: "text/event-stream",
            body,
        }
    }

    fn service_unavailable(body: String) -> Self {
        Self {
            status_line: "503 Service Unavailable",
            content_type: "application/json",
            body,
        }
    }
}

async fn serve_http_sequence(
    responses: Vec<TestHttpResponse>,
) -> (String, tokio::task::JoinHandle<usize>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let handle = tokio::spawn(async move {
        let response_count = responses.len();
        for response in responses {
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

            let response_bytes = format!(
                "HTTP/1.1 {}\r\ncontent-type: {}\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
                response.status_line,
                response.content_type,
                response.body.len(),
                response.body
            );
            socket.write_all(response_bytes.as_bytes()).await.unwrap();
            socket.shutdown().await.unwrap();
        }
        response_count
    });

    (format!("http://{addr}"), handle)
}

async fn serve_delayed_sse() -> (
    String,
    tokio::task::JoinHandle<()>,
    oneshot::Receiver<()>,
    oneshot::Sender<()>,
) {
    serve_delayed_sse_body("data: [DONE]\n\n".to_string()).await
}

async fn serve_delayed_sse_body(
    sse_body: String,
) -> (
    String,
    tokio::task::JoinHandle<()>,
    oneshot::Receiver<()>,
    oneshot::Sender<()>,
) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let (accepted_tx, accepted_rx) = oneshot::channel();
    let (release_tx, release_rx) = oneshot::channel();
    let handle = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut buffer = Vec::new();
        let mut temp = [0_u8; 1024];
        loop {
            let n = socket.read(&mut temp).await.unwrap_or(0);
            if n == 0 {
                return;
            }
            buffer.extend_from_slice(&temp[..n]);
            if buffer.windows(4).any(|window| window == b"\r\n\r\n") {
                break;
            }
        }
        let _ = accepted_tx.send(());
        let _ = release_rx.await;
        let response = format!(
            "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
            sse_body.len(),
            sse_body
        );
        let _ = socket.write_all(response.as_bytes()).await;
        let _ = socket.shutdown().await;
    });

    (format!("http://{addr}"), handle, accepted_rx, release_tx)
}

fn test_config(base_url: String) -> StudioConfig {
    let mut model = ModelInfo::fallback("local-responses");
    model.parameters = vec![crate::ModelParameter {
        name: "effort".to_string(),
        label: None,
        candidates: vec!["none".to_string()],
        wire: std::collections::BTreeMap::new(),
    }];
    let mut info = ProviderInfo::openai(Some(base_url));
    info.connection_mode = ProviderConnectionMode::Http;
    info.default_model = "local-responses".to_string();
    let provider = crate::ProviderConfig::from_provider_info(info, vec![model]);
    let provider_id = ProviderId::new("local").unwrap();
    let route = ModelRouteConfig {
        provider: provider_id.clone(),
        model: "local-responses".to_string(),
        effort: Some(ReasoningEffort::new("none")),
    };
    test_product_config(provider_id, provider, route)
}

fn test_product_config(
    provider_id: ProviderId,
    provider: crate::ProviderConfig,
    route: ModelRouteConfig,
) -> StudioConfig {
    let mut config = StudioConfig::default_config();
    config.models = crate::AgentModelConfig {
        providers: std::collections::BTreeMap::from([(provider_id, provider)]),
        routes: StudioRole::all()
            .into_iter()
            .map(|role| (role.id(), route.clone()))
            .collect(),
    };
    config
}

fn pending_interaction(
    id: &str,
    session_id: &str,
    kind: InteractionKind,
    payload: InteractionPayload,
) -> InteractionRequest {
    InteractionRequest {
        interaction_id: id.to_string(),
        kind,
        status: InteractionStatus::Pending,
        scope: InteractionScope {
            thread_id: session_id.to_string(),
            turn_id: "turn-recovered".to_string(),
            item_id: Some(id.to_string()),
            tool_id: Some(id.to_string()),
            agent_path: None,
        },
        payload,
        created_at: 1,
        updated_at: 1,
        resolved_at: None,
        resolution: None,
    }
}

async fn wait_for_no_active_turn(runtime: &StudioRuntime) {
    tokio::time::timeout(TEST_RUNTIME_TIMEOUT, async {
        loop {
            if runtime.runtime_snapshot().active_turns.is_empty() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
}

mod config;
mod deepseek_cache;
mod lifecycle;
mod ui_runtime;

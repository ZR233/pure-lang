use std::time::Duration;

use crate::{InteractionKind, InteractionScope, InteractionStatus};
use pl_model::{ModelInfo, ProviderEndpoint};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::oneshot;

use super::*;
use crate::config::{ModelRouteConfig, ProviderId, ReasoningEffort, StudioConfig, StudioRole};
use crate::{ConfigStore, StudioMode, StudioRuntimeStateKind};

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
    let (base_url, handle, _requests) = serve_http_sequence_recording(responses).await;
    (base_url, handle)
}

async fn serve_http_sequence_recording(
    responses: Vec<TestHttpResponse>,
) -> (
    String,
    tokio::task::JoinHandle<usize>,
    tokio::sync::mpsc::UnboundedReceiver<serde_json::Value>,
) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let (requests_tx, requests_rx) = tokio::sync::mpsc::unbounded_channel();
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
            let body_start = header_end + 4;
            let request = serde_json::from_slice(
                &buffer[body_start..body_start.saturating_add(content_length)],
            )
            .unwrap();
            let _ = requests_tx.send(request);

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

    (format!("http://{addr}"), handle, requests_rx)
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
    model.transport = pl_model::ModelTransportProfile::responses_http();
    model.parameters = vec![crate::ModelParameter {
        name: "effort".to_string(),
        label: None,
        candidates: vec!["none".to_string()],
        wire: std::collections::BTreeMap::new(),
    }];
    let info = ProviderEndpoint::openai(Some(base_url));
    let provider = crate::ProviderConfig::from_explicit_models(info, vec![model]);
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
    let scope = InteractionScope {
        thread_id: session_id.to_string(),
        turn_id: "turn-recovered".to_string(),
        item_id: Some(id.to_string()),
        tool_id: Some(id.to_string()),
        agent_path: None,
    };
    match (kind, payload) {
        (InteractionKind::UserInput, InteractionPayload::UserInput { questions }) => {
            InteractionRequest::user_input(id, scope, questions, 1)
        }
        (
            InteractionKind::ToolApproval,
            InteractionPayload::ToolApproval {
                name,
                arguments,
                working_directory,
                parent_agent_id,
            },
        ) => InteractionRequest::tool_approval(
            id,
            scope,
            pl_protocol::ToolApprovalRequest {
                name,
                arguments,
                working_directory,
                parent_agent_id,
            },
            1,
        ),
        (
            InteractionKind::PlanConfirmation,
            InteractionPayload::PlanConfirmation { plan_id, content },
        ) => InteractionRequest::plan_confirmation(id, scope, plan_id, content, 1),
        (kind, _) => panic!("fixture payload does not match interaction kind {kind:?}"),
    }
}

enum InteractionPayload {
    UserInput {
        questions: Vec<crate::UserQuestion>,
    },
    ToolApproval {
        name: String,
        arguments: serde_json::Value,
        working_directory: Option<String>,
        parent_agent_id: Option<String>,
    },
    PlanConfirmation {
        plan_id: String,
        content: String,
    },
}

fn responses_function_tool_sse(id: &str, name: &str, arguments: serde_json::Value) -> String {
    let item_id = format!("fc-{id}");
    let call_id = format!("call-{id}");
    responses_sse(vec![
        serde_json::json!({
            "type": "response.output_item.added",
            "item": {
                "type": "function_call",
                "id": item_id,
                "call_id": call_id,
                "name": name
            }
        }),
        serde_json::json!({
            "type": "response.output_item.done",
            "item": {
                "type": "function_call",
                "id": item_id,
                "call_id": call_id,
                "name": name,
                "arguments": serde_json::to_string(&arguments).unwrap()
            }
        }),
    ])
}

fn responses_final_text_sse(id: &str, text: &str) -> String {
    let item_id = format!("msg-{id}");
    responses_sse(vec![
        serde_json::json!({
            "type": "response.output_item.added",
            "item": {
                "id": item_id,
                "type": "message",
                "role": "assistant",
                "phase": "final_answer"
            }
        }),
        serde_json::json!({
            "type": "response.output_text.delta",
            "item_id": item_id,
            "delta": text
        }),
        serde_json::json!({
            "type": "response.output_item.done",
            "item": {
                "id": item_id,
                "type": "message",
                "role": "assistant",
                "phase": "final_answer",
                "content": [{"type": "output_text", "text": text}]
            }
        }),
    ])
}

fn responses_sse(mut events: Vec<serde_json::Value>) -> String {
    events.push(serde_json::json!({
        "type": "response.completed",
        "response": {
            "id": "response-test",
            "model": "local-responses",
            "usage": {
                "input_tokens": 10,
                "output_tokens": 5,
                "total_tokens": 15,
                "input_tokens_details": {"cached_tokens": 0},
                "output_tokens_details": {"reasoning_tokens": 0}
            }
        }
    }));
    events
        .into_iter()
        .map(|event| format!("data: {}\n\n", serde_json::to_string(&event).unwrap()))
        .chain(std::iter::once("data: [DONE]\n\n".to_string()))
        .collect()
}

async fn wait_for_no_active_turn(runtime: &StudioRuntime) {
    tokio::time::timeout(TEST_RUNTIME_TIMEOUT, async {
        loop {
            if runtime.active_turns_for_test().await.is_empty() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
}

fn response_tool_names(request: &serde_json::Value) -> Vec<&str> {
    request["tools"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|tool| {
            tool.get("name")
                .or_else(|| tool.pointer("/function/name"))
                .and_then(serde_json::Value::as_str)
        })
        .collect()
}

mod deepseek_cache;
mod lifecycle;
mod openai_cache;
mod ui_runtime;

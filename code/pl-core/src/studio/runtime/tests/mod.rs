use std::time::Duration;

use pl_model::{ModelInfo, ProviderInfo};
use pl_protocol::{
    InteractionKind, InteractionPayload, InteractionScope, InteractionStatus, PlanLifecycleState,
    StudioTextChannel, StudioTurnStatus,
};
use pl_trace::{
    TraceEvent, TraceEventKind, TracePart, TracePartKind, TracePartSource, TracePartStatus,
    TraceTextChannel,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::{Mutex, oneshot};

use super::plan_confirmation::plan_confirmation_id;
use super::*;
use crate::config::{ModelRole, ProviderConfig, RoleConfig, RoleConfigs};
use crate::studio::runtime::self_learning::{started_tool_snapshot_count, tool_call_count};
use crate::{CompileMode, StudioRuntimeStatus, TurnResultStatus};

const TEST_RUNTIME_TIMEOUT: Duration = Duration::from_secs(20);

async fn serve_sse_once(sse_body: String) -> (String, tokio::task::JoinHandle<()>) {
    serve_sse_sequence(vec![sse_body]).await
}

async fn serve_sse_sequence(sse_bodies: Vec<String>) -> (String, tokio::task::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let handle = tokio::spawn(async move {
        for sse_body in sse_bodies {
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

            let response = format!(
                "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
                sse_body.len(),
                sse_body
            );
            socket.write_all(response.as_bytes()).await.unwrap();
            socket.shutdown().await.unwrap();
        }
    });

    (format!("http://{addr}"), handle)
}

async fn serve_delayed_sse() -> (
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
        let sse_body = "data: [DONE]\n\n";
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

async fn serve_sse_then_delayed_sse(
    first_sse_body: String,
) -> (
    String,
    tokio::task::JoinHandle<()>,
    oneshot::Receiver<()>,
    oneshot::Sender<()>,
) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let (second_accepted_tx, second_accepted_rx) = oneshot::channel();
    let (second_release_tx, second_release_rx) = oneshot::channel();
    let handle = tokio::spawn(async move {
        let mut second_accepted_tx = Some(second_accepted_tx);
        let mut second_release_rx = Some(second_release_rx);
        for (index, sse_body) in [first_sse_body, "data: [DONE]\n\n".to_string()]
            .into_iter()
            .enumerate()
        {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut buffer = Vec::new();
            let mut temp = [0_u8; 1024];
            loop {
                let n = socket.read(&mut temp).await.unwrap();
                assert_ne!(n, 0);
                buffer.extend_from_slice(&temp[..n]);
                if buffer.windows(4).any(|window| window == b"\r\n\r\n") {
                    break;
                }
            }
            if index == 1 {
                if let Some(sender) = second_accepted_tx.take() {
                    let _ = sender.send(());
                }
                if let Some(receiver) = second_release_rx.take() {
                    let _ = receiver.await;
                }
            }
            let response = format!(
                "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
                sse_body.len(),
                sse_body
            );
            socket.write_all(response.as_bytes()).await.unwrap();
            socket.shutdown().await.unwrap();
        }
    });

    (
        format!("http://{addr}"),
        handle,
        second_accepted_rx,
        second_release_tx,
    )
}

fn test_config(base_url: String) -> crate::config::PureConfig {
    let mut model = ModelInfo::fallback("local-responses");
    model.parameters = vec![crate::ModelParameter {
        name: "effort".to_string(),
        label: None,
        candidates: vec!["none".to_string()],
        wire: std::collections::BTreeMap::new(),
    }];
    let mut info = ProviderInfo::openai(Some(base_url));
    info.default_model = "local-responses".to_string();
    let provider = ProviderConfig::from_provider_info(info, vec![model]);
    let role = RoleConfig {
        provider: "local".to_string(),
        model: "local-responses".to_string(),
        effort: crate::config::ReasoningEffort::new("none"),
    };
    crate::config::PureConfig {
        roles: RoleConfigs::from_default_role(role),
        providers: std::collections::BTreeMap::from([("local".to_string(), provider)]),
        ..crate::config::PureConfig::default_config()
    }
}

fn test_chat_config(base_url: String) -> crate::config::PureConfig {
    let mut model = ModelInfo::fallback("local-chat");
    model.context_window = Some(128_000);
    model.parameters = vec![crate::ModelParameter {
        name: "effort".to_string(),
        label: None,
        candidates: vec!["none".to_string()],
        wire: std::collections::BTreeMap::new(),
    }];
    let mut info = ProviderInfo::deepseek(Some(base_url));
    info.default_model = "local-chat".to_string();
    let provider = ProviderConfig::from_provider_info(info, vec![model]);
    let role = RoleConfig {
        provider: "local".to_string(),
        model: "local-chat".to_string(),
        effort: crate::config::ReasoningEffort::new("none"),
    };
    crate::config::PureConfig {
        roles: RoleConfigs::from_default_role(role),
        providers: std::collections::BTreeMap::from([("local".to_string(), provider)]),
        ..crate::config::PureConfig::default_config()
    }
}

fn emitter(
    events: std::sync::Arc<Mutex<Vec<InteractionRequest>>>,
) -> crate::studio::InteractionEmitter {
    std::sync::Arc::new(move |interaction| {
        let events = events.clone();
        Box::pin(async move {
            events.lock().await.push(interaction);
            Ok(())
        })
    })
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
            session_id: session_id.to_string(),
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
mod continuation;
mod lifecycle;
mod plan_flows;
mod self_learning;
mod stream_projection;
mod ui_runtime;

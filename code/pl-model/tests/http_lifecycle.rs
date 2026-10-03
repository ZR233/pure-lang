use pl_model::{
    completion::{CompletionRequest, Message, MessageContent, MessageRole},
    model::{ModelInfo, ModelTransportProfile},
    provider::ProviderEndpoint,
    runtime::{ModelInvocationContext, ModelRuntime, ModelSession},
};
use pl_protocol::trace::{AgentEvent, InMemoryTraceEventSink, TracePartKind};
use pl_provider_fixture::{FixtureServer, Protocol, Reply, Step, responses_text};
use serde_json::json;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

fn request() -> CompletionRequest {
    CompletionRequest::builder()
        .messages(vec![Message {
            role: MessageRole::User,
            content: MessageContent::text("continue"),
            presentation: Default::default(),
            reasoning_content: None,
            tool_calls: None,
            tool_result: None,
            metadata: Default::default(),
        }])
        .build()
}

fn runtime(base: String, native: bool) -> ModelRuntime {
    let mut model = ModelInfo::compatible("http-model");
    model
        .binding
        .set_transport(ModelTransportProfile::responses_http());
    model.capabilities.web_search = true;
    let mut endpoint = ProviderEndpoint::compatible("fixture", base);
    endpoint.service_capabilities.remote_compaction = native;
    ModelRuntime::new(endpoint, model).unwrap()
}

type Chunk = (Vec<u8>, tokio::sync::oneshot::Sender<()>);

async fn server() -> (
    String,
    tokio::sync::mpsc::Sender<Chunk>,
    tokio::task::JoinHandle<()>,
) {
    server_with_status("200 OK").await
}

async fn server_with_status(
    status: &'static str,
) -> (
    String,
    tokio::sync::mpsc::Sender<Chunk>,
    tokio::task::JoinHandle<()>,
) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}/v1", listener.local_addr().unwrap());
    let (send, mut chunks) = tokio::sync::mpsc::channel::<Chunk>(1);
    let task = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        // Small virtual-time fragments must reach the peer immediately rather
        // than waiting for TCP's wall-clock delayed-ACK/Nagle interval.
        socket.set_nodelay(true).unwrap();
        let mut request = Vec::new();
        loop {
            let mut buffer = [0; 4096];
            let count = socket.read(&mut buffer).await.unwrap();
            assert_ne!(count, 0);
            request.extend_from_slice(&buffer[..count]);
            if let Some(end) = request.windows(4).position(|w| w == b"\r\n\r\n") {
                let headers = String::from_utf8_lossy(&request[..end]).to_ascii_lowercase();
                let length = headers
                    .lines()
                    .find_map(|line| line.strip_prefix("content-length: "))
                    .unwrap()
                    .parse::<usize>()
                    .unwrap();
                if request.len() >= end + 4 + length {
                    break;
                }
            }
        }
        socket.write_all(format!("HTTP/1.1 {status}\r\nContent-Type: text/event-stream\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n").as_bytes()).await.unwrap();
        while let Some((bytes, ack)) = chunks.recv().await {
            socket
                .write_all(format!("{:x}\r\n", bytes.len()).as_bytes())
                .await
                .unwrap();
            socket.write_all(&bytes).await.unwrap();
            socket.write_all(b"\r\n").await.unwrap();
            socket.flush().await.unwrap();
            ack.send(()).unwrap();
        }
        socket.write_all(b"0\r\n\r\n").await.unwrap();
    });
    (base, send, task)
}

async fn send(sender: &tokio::sync::mpsc::Sender<Chunk>, bytes: Vec<u8>) {
    let (ack, received) = tokio::sync::oneshot::channel();
    sender.send((bytes, ack)).await.unwrap();
    received.await.unwrap();
    // Let the runtime poll socket readiness before advancing its virtual clock.
    for _ in 0..64 {
        tokio::task::yield_now().await;
    }
}

#[tokio::test]
async fn fragmented_completion_and_native_compaction_survive_active_reads_beyond_300_seconds() {
    for native in [false, true] {
        let (base, sender, server) = server().await;
        let runtime = runtime(base, native);
        let (events, mut received) = tokio::sync::broadcast::channel(64);
        let context = traced(ModelSession::default(), events);
        let invocation = tokio::spawn(async move {
            if native {
                let response = runtime
                    .compaction()
                    .unwrap()
                    .checkpoint(request(), context)
                    .await
                    .unwrap();
                assert!(matches!(
                    response.item,
                    pl_protocol::ModelContextItem::Compaction { .. }
                ));
            } else {
                assert_eq!(
                    runtime
                        .complete(request(), context)
                        .await
                        .unwrap()
                        .content
                        .as_deref(),
                    Some("warmfragmented")
                );
            }
        });
        let warm = responses_text("warm", "active", "http-model");
        send(
            &sender,
            warm[..3]
                .iter()
                .map(|event| format!("data: {event}\n\n"))
                .collect::<String>()
                .into_bytes(),
        )
        .await;
        wait_text(&mut received).await;
        tokio::time::pause();
        let data = if native {
            json!({"type":"response.output_item.done","item":{"type":"compaction","encrypted_content":"checkpoint"}})
        } else {
            json!({"type":"response.output_text.delta","item_id":"message-1","delta":"fragmented"})
        };
        let data = format!("data: {data}\n\n").into_bytes();
        let width = data.len().div_ceil(4);
        for chunk in data.chunks(width) {
            tokio::time::advance(Duration::from_secs(100)).await;
            assert!(
                !invocation.is_finished(),
                "active body was cut off before the fragmented event completed"
            );
            send(&sender, chunk.to_vec()).await;
        }
        tokio::time::resume();
        let completed = if native {
            json!({"type":"response.completed","response":{"id":"active","usage":{"input_tokens":2,"output_tokens":1}}})
        } else {
            responses_text("warmfragmented", "active", "http-model")
                .pop()
                .unwrap()
        };
        send(&sender, format!("data: {completed}\n\n").into_bytes()).await;
        drop(sender);
        tokio::time::timeout(Duration::from_secs(5), invocation)
            .await
            .unwrap()
            .unwrap();
        server.await.unwrap();
    }
}

#[tokio::test]
async fn session_close_cancels_an_active_http_body_and_seals_admission() {
    let mut events = responses_text("observed", "closing", "http-model");
    events.pop();
    let fixture = FixtureServer::start(vec![Step::prompt(
        Protocol::ResponsesHttp,
        "continue",
        0,
        Reply::HangingSse(events),
    )])
    .await
    .unwrap();
    let runtime = runtime(fixture.base_url(), false);
    let session = ModelSession::default();
    let (events, mut received) = tokio::sync::broadcast::channel(64);
    let context = traced(session.clone(), events);
    let invocation = tokio::spawn({
        let runtime = runtime.clone();
        async move { runtime.complete(request(), context).await }
    });
    wait_text(&mut received).await;
    tokio::time::timeout(Duration::from_secs(2), session.close())
        .await
        .unwrap()
        .unwrap();
    let failure = invocation.await.unwrap().unwrap_err();
    assert!(failure.is_cancelled());
    assert!(
        runtime
            .complete(request(), ModelInvocationContext::new(session))
            .await
            .is_err()
    );
    assert_eq!(fixture.finish().await.unwrap().len(), 1);
}

#[tokio::test]
async fn http_read_idle_is_bounded_and_preserves_an_unsafe_attempt_without_replay() {
    let mut events = responses_text("remote work", "idle", "http-model");
    events.pop();
    let fixture = FixtureServer::start(vec![Step::prompt(
        Protocol::ResponsesHttp,
        "continue",
        0,
        Reply::HangingSse(events),
    )])
    .await
    .unwrap();
    let runtime = runtime(fixture.base_url(), false);
    let mut input = request();
    input.tools.push(pl_protocol::ToolSpec::WebSearch {
        options: pl_protocol::HostedWebSearchOptions::OpenAi {
            external_web_access: false,
            indexed_web_access: None,
            filters: None,
            user_location: None,
            search_context_size: None,
            search_content_types: None,
        },
    });
    let (events, mut received) = tokio::sync::broadcast::channel(64);
    let invocation = tokio::spawn(async move {
        runtime
            .complete(input, traced(ModelSession::default(), events))
            .await
    });
    wait_text(&mut received).await;
    for _ in 0..64 {
        tokio::task::yield_now().await;
    }
    tokio::time::pause();
    tokio::time::advance(Duration::from_secs(179)).await;
    assert!(!invocation.is_finished());
    tokio::time::advance(Duration::from_secs(2)).await;
    tokio::time::resume();
    let failure = tokio::time::timeout(Duration::from_secs(5), invocation)
        .await
        .unwrap()
        .unwrap()
        .unwrap_err();
    assert!(failure.is_transient_model_transport());
    assert!(!failure.is_cancelled());
    assert_eq!(fixture.finish().await.unwrap().len(), 1);
}

#[tokio::test]
async fn oversized_sse_event_fails_before_unbounded_json_accumulation_or_retry() {
    let fixture = FixtureServer::start(vec![Step::prompt(
        Protocol::ResponsesHttp, "continue", 0,
        Reply::Sse(vec![json!({"type":"response.output_text.delta","item_id":"message-1","delta":"X".repeat(64 * 1024 * 1024)})]),
    )]).await.unwrap();
    let failure = runtime(fixture.base_url(), false)
        .complete(request(), ModelInvocationContext::default())
        .await
        .unwrap_err();
    assert!(matches!(
        failure.source.as_ref(),
        pl_protocol::PureError::Protocol(_)
    ));
    assert_eq!(fixture.finish().await.unwrap().len(), 1);
}

#[tokio::test]
async fn stalled_authentication_error_body_keeps_status_and_finishes_within_15_seconds() {
    let (base, sender, server) = server_with_status("401 Unauthorized").await;
    let invocation = tokio::spawn(async move {
        runtime(base, false)
            .complete(request(), ModelInvocationContext::default())
            .await
    });
    send(&sender, b"{\"error\":".to_vec()).await;
    tokio::time::pause();
    tokio::time::advance(Duration::from_secs(16)).await;
    tokio::time::resume();
    let failure = tokio::time::timeout(Duration::from_secs(5), invocation)
        .await
        .unwrap()
        .unwrap()
        .unwrap_err();
    let provider = failure.provider_failure_ref().unwrap();
    assert_eq!(provider.http_status, Some(401));
    assert!(!failure.is_transient_model_transport());
    assert!(
        provider
            .message
            .contains("error response body read deadline exceeded")
    );
    server.abort();
    assert!(server.await.unwrap_err().is_cancelled());
}

fn traced(
    session: ModelSession,
    events: pl_protocol::trace::AgentEventSender,
) -> ModelInvocationContext {
    ModelInvocationContext::new(session)
        .with_events(events)
        .with_trace(
            pl_model::completion::CompletionTraceContext {
                session_id: "http-lifecycle".into(),
                turn_id: "turn".into(),
                inference_id: "call".into(),
            },
            Arc::new(InMemoryTraceEventSink::new("http-lifecycle", 0)),
        )
}

async fn wait_text(received: &mut pl_protocol::trace::AgentEventReceiver) {
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if let AgentEvent::TracePartDelta { event } = received.recv().await.unwrap()
                && event.kind() == TracePartKind::Text
            {
                break;
            }
        }
    })
    .await
    .unwrap();
}

use std::collections::HashMap;
use std::future::pending;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use pl_model::{
    ModelInfo, ModelRuntime, ModelTransportProfile, OpenAiCompactionMode, ProviderEndpoint,
    ProviderWireProtocol,
};
use pl_protocol::{Message, MessageContent, MessageRole, ModelContextItem};
use pl_trace::{AgentEvent, AgentEventSender, TracePartSource};
use pretty_assertions::assert_eq;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

use super::history::{
    has_compactable_history, is_compaction_summary, message_text, recent_user_messages,
};
use super::*;
use crate::core::progress::{ProgressEmitter, ProgressVerbosity};

fn text_message(role: MessageRole, text: &str) -> Message {
    Message {
        role,
        content: MessageContent::Text(text.to_string()),
        reasoning_content: None,
        tool_calls: None,
        tool_result: None,
        metadata: HashMap::new(),
    }
}

#[test]
fn compacted_history_filters_old_summary_and_places_new_summary_last() {
    let mut old_summary = text_message(MessageRole::User, "old summary");
    old_summary.metadata.insert(
        SUMMARY_METADATA_KEY.to_string(),
        SUMMARY_METADATA_VALUE.to_string(),
    );
    let messages = vec![
        old_summary,
        text_message(MessageRole::User, "old request"),
        text_message(MessageRole::Assistant, "old answer"),
        text_message(MessageRole::Tool, "tool output"),
        text_message(MessageRole::User, "latest request"),
    ];

    let compacted = build_compacted_history(
        &messages,
        "new summary",
        &ContextCompactionConfig::default(),
    );

    assert_eq!(compacted.len(), 3);
    assert_eq!(message_text(&compacted[0]), "old request");
    assert_eq!(message_text(&compacted[1]), "latest request");
    assert!(is_compaction_summary(&compacted[2]));
}

#[test]
fn recent_user_boundary_is_truncated_to_token_budget() {
    let messages = vec![
        text_message(MessageRole::User, "first request"),
        text_message(MessageRole::User, "latest request with a long body"),
    ];

    let users = recent_user_messages(&messages, 1);

    assert_eq!(users.len(), 1);
    assert_eq!(message_text(&users[0]), "body");
}

#[test]
fn manual_compaction_accepts_one_real_message_but_auto_does_not() {
    let items = vec![ModelContextItem::from(text_message(
        MessageRole::User,
        "latest request",
    ))];

    assert!(!has_compactable_history(
        &items,
        CompactionTrigger::EstimatedTokens
    ));
    assert!(has_compactable_history(&items, CompactionTrigger::Manual));
    assert!(!has_compactable_history(
        &[ModelContextItem::Compaction {
            encrypted_content: "encrypted".to_string(),
        }],
        CompactionTrigger::Manual,
    ));
}

#[tokio::test]
async fn local_context_pressure_retry_emits_progress_and_preserves_summary_order() {
    let provider =
        FakeCompactionProvider::new(test_model(), FakeCompactionFailure::ContextPressure).await;
    let (event_tx, mut event_rx) = tokio::sync::broadcast::channel(16);
    let mut recorder = TraceRecorder::disabled(event_tx.clone());
    let mut progress = ProgressEmitter::new("turn-compact", ProgressVerbosity::Normal);
    let mut session = test_session();
    let config = ContextCompactionConfig::default().with_openai_mode(OpenAiCompactionMode::Local);

    let outcome = maybe_compact_session(
        &mut session,
        compaction_request(
            &provider,
            &config,
            event_tx,
            &mut recorder,
            Some(&mut progress),
        ),
    )
    .await
    .unwrap();

    assert!(matches!(outcome, CompactionOutcome::Compacted { .. }));
    // Chat wire messages include the compaction instruction as a system message.
    assert_eq!(provider.recorded_wire_item_counts(), vec![5, 4]);
    assert!(session.messages().last().is_some_and(is_compaction_summary));
    assert_eq!(
        runtime_progress_texts(&mut event_rx),
        vec![
            "上下文接近上限，正在压缩历史。".to_string(),
            "上下文压缩请求过大，正在缩小历史后重试。".to_string(),
            "上下文已压缩，继续准备模型调用。".to_string(),
        ]
    );
}

#[tokio::test]
async fn local_retries_without_unsupported_max_output_tokens() {
    let provider = FakeCompactionProvider::new(
        test_model(),
        FakeCompactionFailure::UnsupportedMaxOutputTokens,
    )
    .await;
    let (event_tx, _) = tokio::sync::broadcast::channel(8);
    let mut recorder = TraceRecorder::disabled(event_tx.clone());
    let mut session = test_session();
    let config = ContextCompactionConfig::default().with_openai_mode(OpenAiCompactionMode::Local);

    maybe_compact_session(
        &mut session,
        compaction_request(&provider, &config, event_tx, &mut recorder, None),
    )
    .await
    .unwrap();

    // Chat wire messages include the compaction instruction as a system message.
    assert_eq!(provider.recorded_wire_item_counts(), vec![5, 5]);
    assert_eq!(provider.recorded_max_tokens(), vec![Some(4096), None]);
}

#[tokio::test]
async fn local_empty_summary_preserves_session_history_and_revision() {
    let provider =
        FakeCompactionProvider::new(test_model(), FakeCompactionFailure::EmptySummary).await;
    let (event_tx, _) = tokio::sync::broadcast::channel(8);
    let mut recorder = TraceRecorder::disabled(event_tx.clone());
    let mut session = test_session();
    let original_items = session.items().to_vec();
    let original_revision = session.revision();
    let config = ContextCompactionConfig::default().with_openai_mode(OpenAiCompactionMode::Local);

    let error = maybe_compact_session(
        &mut session,
        compaction_request(&provider, &config, event_tx, &mut recorder, None),
    )
    .await
    .unwrap_err();

    assert!(error.to_string().contains(&config.empty_summary_error));
    assert_eq!(session.items(), original_items.as_slice());
    assert_eq!(session.revision(), original_revision);
}

#[tokio::test]
async fn remote_failure_does_not_replace_session_history_or_revision() {
    let provider =
        FakeCompactionProvider::new(responses_test_model(), FakeCompactionFailure::RemoteFailure)
            .await;
    let mut session = test_session();
    let original_items = session.items().to_vec();
    let original_revision = session.revision();
    let config =
        ContextCompactionConfig::default().with_openai_mode(OpenAiCompactionMode::RemoteV2);
    let (event_tx, _) = tokio::sync::broadcast::channel(8);
    let mut recorder = TraceRecorder::disabled(event_tx.clone());

    let error = maybe_compact_session(
        &mut session,
        compaction_request(&provider, &config, event_tx, &mut recorder, None),
    )
    .await
    .unwrap_err();

    assert!(error.to_string().contains("remote compaction failed"));
    assert_eq!(session.items(), original_items.as_slice());
    assert_eq!(session.revision(), original_revision);
}

#[tokio::test]
async fn chat_completions_provider_always_uses_local_compaction() {
    let provider =
        FakeCompactionProvider::new(test_model(), FakeCompactionFailure::ContextPressure).await;
    let mut session = test_session();
    let config =
        ContextCompactionConfig::default().with_openai_mode(OpenAiCompactionMode::RemoteV2);
    let (event_tx, _) = tokio::sync::broadcast::channel(8);
    let mut recorder = TraceRecorder::disabled(event_tx.clone());

    let outcome = maybe_compact_session(
        &mut session,
        compaction_request(&provider, &config, event_tx, &mut recorder, None),
    )
    .await
    .unwrap();

    let CompactionOutcome::Compacted { snapshot, .. } = outcome else {
        panic!("expected compaction");
    };
    assert_eq!(
        snapshot.implementation,
        ContextCompactionImplementation::Local
    );
    // Chat wire messages include the compaction instruction as a system message.
    assert_eq!(provider.recorded_wire_item_counts(), vec![5, 4]);
}

#[test]
fn encrypted_checkpoint_rejects_chat_completions_provider() {
    let session = AgentSession::from_items(vec![ModelContextItem::Compaction {
        encrypted_content: "encrypted".to_string(),
    }]);

    let error =
        ensure_provider_can_consume_session(ProviderWireProtocol::ChatCompletions, &session)
            .unwrap_err();

    assert!(error.to_string().contains("继续使用 Responses provider"));
}

#[test]
fn responses_native_context_rejects_chat_completions_provider() {
    let session = AgentSession::from_items(vec![ModelContextItem::Responses {
        item: pl_protocol::ResponsesContextItem {
            kind: pl_protocol::ResponsesContextItemKind::Program,
            value: serde_json::json!({"type": "program", "id": "program-1"}),
        },
    }]);

    let error =
        ensure_provider_can_consume_session(ProviderWireProtocol::ChatCompletions, &session)
            .unwrap_err();

    assert!(error.to_string().contains("Responses provider"));
}

fn test_model() -> ModelInfo {
    let mut model = ModelInfo::fallback("compact-test");
    model.context_window = Some(100);
    model.max_context_window = Some(100);
    model.auto_compact_token_limit = Some(1);
    model.max_output_tokens = Some(4096);
    model
}

fn responses_test_model() -> ModelInfo {
    let mut model = test_model();
    model.transport = ModelTransportProfile::responses_http();
    model
}

fn test_session() -> AgentSession {
    let mut session = AgentSession::new();
    session.push_user_prompt("old request ".repeat(20));
    session.push_assistant_response("old answer ".repeat(20), None);
    session.push_user_prompt("latest request ".repeat(20));
    session
}

fn compaction_request<'a>(
    provider: &'a FakeCompactionProvider,
    config: &'a ContextCompactionConfig,
    event_tx: AgentEventSender,
    recorder: &'a mut TraceRecorder,
    progress: Option<&'a mut ProgressEmitter>,
) -> ContextCompactionRequest<'a> {
    ContextCompactionRequest {
        runtime: &provider.runtime,
        config,
        request_instructions: "",
        request_messages: &[],
        working_context_tail: None,
        tools: &[],
        parallel_tool_calls: false,
        reasoning: None,
        prompt_cache_key: None,
        trigger: CompactionTrigger::EstimatedTokens,
        phase: ContextCompactionPhase::PreTurn,
        event_tx,
        recorder,
        progress,
        control: super::ContextCompactionControl::default(),
    }
}

#[tokio::test]
async fn remote_compaction_succeeds_through_the_shared_controller() {
    let provider =
        FakeCompactionProvider::new(responses_test_model(), FakeCompactionFailure::Success).await;
    let mut session = test_session();
    let config =
        ContextCompactionConfig::default().with_openai_mode(OpenAiCompactionMode::RemoteV2);
    let (event_tx, _) = tokio::sync::broadcast::channel(8);
    let mut recorder = TraceRecorder::disabled(event_tx.clone());

    let outcome = maybe_compact_session(
        &mut session,
        compaction_request(&provider, &config, event_tx, &mut recorder, None),
    )
    .await
    .unwrap();

    let CompactionOutcome::Compacted { snapshot, .. } = outcome else {
        panic!("expected compaction");
    };
    assert_eq!(
        snapshot.implementation,
        ContextCompactionImplementation::RemoteV2
    );
    assert!(session.revision() > 0);
}

#[tokio::test]
async fn local_compaction_timeout_preserves_session_atomically() {
    let provider = FakeCompactionProvider::new(test_model(), FakeCompactionFailure::Hang).await;
    let mut session = test_session();
    let original_items = session.items().to_vec();
    let original_revision = session.revision();
    let config = ContextCompactionConfig::default().with_openai_mode(OpenAiCompactionMode::Local);
    let (event_tx, _) = tokio::sync::broadcast::channel(8);
    let mut recorder = TraceRecorder::disabled(event_tx.clone());
    let mut request = compaction_request(&provider, &config, event_tx, &mut recorder, None);
    request.control = ContextCompactionControl::default().with_timeout(Duration::from_millis(20));

    let error = maybe_compact_session(&mut session, request)
        .await
        .unwrap_err();

    assert!(error.to_string().contains("timed out after 20ms"));
    assert_eq!(session.items(), original_items.as_slice());
    assert_eq!(session.revision(), original_revision);
}

#[tokio::test]
async fn remote_compaction_timeout_preserves_session_atomically() {
    let provider =
        FakeCompactionProvider::new(responses_test_model(), FakeCompactionFailure::Hang).await;
    let mut session = test_session();
    let original_items = session.items().to_vec();
    let original_revision = session.revision();
    let config =
        ContextCompactionConfig::default().with_openai_mode(OpenAiCompactionMode::RemoteV2);
    let (event_tx, _) = tokio::sync::broadcast::channel(8);
    let mut recorder = TraceRecorder::disabled(event_tx.clone());
    let mut request = compaction_request(&provider, &config, event_tx, &mut recorder, None);
    request.control = ContextCompactionControl::default().with_timeout(Duration::from_millis(20));

    let error = maybe_compact_session(&mut session, request)
        .await
        .unwrap_err();

    assert!(error.to_string().contains("timed out after 20ms"));
    assert_eq!(session.items(), original_items.as_slice());
    assert_eq!(session.revision(), original_revision);
}

#[tokio::test]
async fn compaction_cancellation_preserves_session_atomically() {
    let provider = FakeCompactionProvider::new(test_model(), FakeCompactionFailure::Hang).await;
    let mut session = test_session();
    let original_items = session.items().to_vec();
    let original_revision = session.revision();
    let config = ContextCompactionConfig::default().with_openai_mode(OpenAiCompactionMode::Local);
    let (event_tx, _) = tokio::sync::broadcast::channel(8);
    let mut recorder = TraceRecorder::disabled(event_tx.clone());
    let cancellation = CancellationToken::new();
    cancellation.cancel();
    let mut request = compaction_request(&provider, &config, event_tx, &mut recorder, None);
    request.control = ContextCompactionControl::default()
        .with_timeout(Duration::from_secs(1))
        .with_cancellation(cancellation);

    let error = maybe_compact_session(&mut session, request)
        .await
        .unwrap_err();

    assert!(
        error
            .to_string()
            .contains("cancelled with the current turn")
    );
    assert_eq!(session.items(), original_items.as_slice());
    assert_eq!(session.revision(), original_revision);
}

fn runtime_progress_texts(
    event_rx: &mut tokio::sync::broadcast::Receiver<AgentEvent>,
) -> Vec<String> {
    let mut texts = Vec::new();
    while let Ok(event) = event_rx.try_recv() {
        if let AgentEvent::TracePartCompleted { item } = event
            && item.source == TracePartSource::Runtime
        {
            texts.push(item.content);
        }
    }
    texts
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FakeCompactionFailure {
    Success,
    ContextPressure,
    UnsupportedMaxOutputTokens,
    EmptySummary,
    RemoteFailure,
    Hang,
}

#[derive(Debug)]
struct FakeCompactionProvider {
    runtime: ModelRuntime,
    requests: Arc<Mutex<Vec<serde_json::Value>>>,
}

impl FakeCompactionProvider {
    async fn new(model: ModelInfo, failure: FakeCompactionFailure) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let requests = Arc::new(Mutex::new(Vec::new()));
        let captured = Arc::clone(&requests);
        let protocol = model.transport.protocol;
        tokio::spawn(async move {
            let response_count = match failure {
                FakeCompactionFailure::Success
                | FakeCompactionFailure::EmptySummary
                | FakeCompactionFailure::RemoteFailure
                | FakeCompactionFailure::Hang => 1,
                FakeCompactionFailure::ContextPressure
                | FakeCompactionFailure::UnsupportedMaxOutputTokens => 2,
            };
            for attempt in 0..response_count {
                let (mut socket, _) = listener.accept().await.unwrap();
                let request = read_http_json_request(&mut socket).await;
                captured.lock().unwrap().push(request);
                if failure == FakeCompactionFailure::Hang {
                    pending::<()>().await;
                }
                let response = match (failure, attempt) {
                    (FakeCompactionFailure::Success, _) => remote_compaction_response(),
                    (FakeCompactionFailure::ContextPressure, 0) => {
                        error_response("context token limit exceeded")
                    }
                    (FakeCompactionFailure::UnsupportedMaxOutputTokens, 0) => {
                        error_response("Unsupported parameter: max_output_tokens")
                    }
                    (FakeCompactionFailure::RemoteFailure, 0) => {
                        error_response("remote compaction failed")
                    }
                    (FakeCompactionFailure::RemoteFailure, _) => unreachable!(),
                    (FakeCompactionFailure::EmptySummary, _) => {
                        completion_response(protocol, "   ")
                    }
                    (FakeCompactionFailure::ContextPressure, _)
                    | (FakeCompactionFailure::UnsupportedMaxOutputTokens, _) => {
                        completion_response(protocol, "summary")
                    }
                    (FakeCompactionFailure::Hang, _) => unreachable!(),
                };
                socket.write_all(response.as_bytes()).await.unwrap();
                socket.shutdown().await.unwrap();
            }
        });
        let endpoint = ProviderEndpoint::openai(Some(format!("http://{address}/v1")));
        Self {
            runtime: ModelRuntime::new(endpoint, model).unwrap(),
            requests,
        }
    }

    fn recorded_wire_item_counts(&self) -> Vec<usize> {
        self.requests
            .lock()
            .unwrap()
            .iter()
            .map(|request| {
                request
                    .get("input")
                    .or_else(|| request.get("messages"))
                    .and_then(serde_json::Value::as_array)
                    .map_or(0, Vec::len)
            })
            .collect()
    }

    fn recorded_max_tokens(&self) -> Vec<Option<u64>> {
        self.requests
            .lock()
            .unwrap()
            .iter()
            .map(|request| {
                request
                    .get("max_output_tokens")
                    .or_else(|| request.get("max_completion_tokens"))
                    .or_else(|| request.get("max_tokens"))
                    .and_then(serde_json::Value::as_u64)
            })
            .collect()
    }
}

async fn read_http_json_request(socket: &mut tokio::net::TcpStream) -> serde_json::Value {
    let mut bytes = Vec::new();
    let mut buffer = [0_u8; 4096];
    loop {
        let read = socket.read(&mut buffer).await.unwrap();
        assert_ne!(read, 0);
        bytes.extend_from_slice(&buffer[..read]);
        let text = String::from_utf8_lossy(&bytes);
        let Some((headers, body)) = text.split_once("\r\n\r\n") else {
            continue;
        };
        let content_length = headers
            .lines()
            .find_map(|line| {
                let (name, value) = line.split_once(':')?;
                name.eq_ignore_ascii_case("content-length")
                    .then(|| value.trim().parse::<usize>().ok())
                    .flatten()
            })
            .unwrap_or_default();
        if body.len() >= content_length {
            return serde_json::from_str(&body[..content_length]).unwrap();
        }
    }
}

fn error_response(message: &str) -> String {
    let body = serde_json::json!({
        "error": {
            "message": message,
            "type": "invalid_request_error",
            "code": "invalid_request"
        }
    })
    .to_string();
    format!(
        "HTTP/1.1 400 Bad Request\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
        body.len()
    )
}

fn completion_response(protocol: ProviderWireProtocol, content: &str) -> String {
    let body = match protocol {
        ProviderWireProtocol::Responses => format!(
            "data: {{\"type\":\"response.output_text.delta\",\"item_id\":\"msg-1\",\"delta\":{}}}\n\ndata: {{\"type\":\"response.completed\",\"response\":{{\"id\":\"resp-1\",\"usage\":{{\"input_tokens\":1,\"output_tokens\":2,\"total_tokens\":3}}}}}}\n\ndata: [DONE]\n\n",
            serde_json::to_string(content).unwrap()
        ),
        ProviderWireProtocol::ChatCompletions => format!(
            "data: {{\"choices\":[{{\"delta\":{{\"content\":{}}},\"finish_reason\":null}}]}}\n\ndata: {{\"choices\":[{{\"delta\":{{}},\"finish_reason\":\"stop\"}}],\"usage\":{{\"prompt_tokens\":1,\"completion_tokens\":2,\"total_tokens\":3}}}}\n\ndata: [DONE]\n\n",
            serde_json::to_string(content).unwrap()
        ),
    };
    format!(
        "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
        body.len()
    )
}

fn remote_compaction_response() -> String {
    let body = concat!(
        "data: {\"type\":\"response.output_item.done\",\"item\":{\"type\":\"compaction\",\"encrypted_content\":\"encrypted-v2\"}}\n\n",
        "data: {\"type\":\"response.completed\",\"response\":{\"usage\":{\"input_tokens\":1,\"output_tokens\":2,\"total_tokens\":3}}}\n\n",
        "data: [DONE]\n\n"
    );
    format!(
        "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
        body.len()
    )
}

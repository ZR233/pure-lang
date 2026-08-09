use super::*;
use crate::default_models;
use crate::request::{
    ModelCompactionRequest, OpenAiCompactionMode, ReasoningConfig, ReasoningSummary, ToolSchema,
};
use futures::{SinkExt, StreamExt};
use pl_protocol::ModelContextItem;
use pretty_assertions::assert_eq;
use tokio::net::TcpListener;
use tokio_tungstenite::tungstenite::Message as WebSocketMessage;
use tokio_tungstenite::tungstenite::handshake::server::{
    ErrorResponse, Request as WebSocketRequest, Response as WebSocketResponse,
};
use tokio_tungstenite::tungstenite::protocol::CloseFrame;
use tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode;
use tokio_tungstenite::{accept_async, accept_hdr_async};

// tungstenite 的 header callback 必须返回其固定 ErrorResponse；该第三方类型较大，
// 测试 helper 无法在不改变 callback 契约的情况下装箱。
#[allow(clippy::result_large_err)]
fn validate_openai_websocket_handshake(
    request: &WebSocketRequest,
    response: WebSocketResponse,
) -> std::result::Result<WebSocketResponse, ErrorResponse> {
    assert_eq!(request.uri().path(), "/v1/responses");
    assert_eq!(
        request.headers()["openai-beta"],
        "responses_websockets=2026-02-06"
    );
    assert_eq!(request.headers()["authorization"], "Bearer test-token");
    Ok(response)
}

async fn capture_http_request(socket: &mut tokio::net::TcpStream) -> CapturedHttpRequest {
    let mut buffer = Vec::new();
    let mut temp = [0_u8; 1024];
    let (header_end, content_length) = loop {
        let read = socket.read(&mut temp).await.unwrap();
        assert_ne!(read, 0);
        buffer.extend_from_slice(&temp[..read]);
        if let Some(header_end) = find_header_end(&buffer) {
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
        let read = socket.read(&mut temp).await.unwrap();
        assert_ne!(read, 0);
        buffer.extend_from_slice(&temp[..read]);
    }

    let request_head = String::from_utf8_lossy(&buffer[..header_end]);
    let mut lines = request_head.lines();
    let request_line = lines.next().unwrap_or_default().to_string();
    let headers = lines
        .filter_map(|line| {
            let (name, value) = line.split_once(':')?;
            Some((name.to_ascii_lowercase(), value.trim().to_string()))
        })
        .collect::<HashMap<_, _>>();
    let body =
        serde_json::from_slice(&buffer[header_end + 4..header_end + 4 + content_length]).unwrap();

    CapturedHttpRequest {
        request_line,
        headers,
        body,
    }
}

async fn send_responses_sse(
    socket: &mut tokio::net::TcpStream,
    response_id: &str,
    message_id: &str,
    text: &str,
) {
    let body = format!(
        "data: {{\"type\":\"response.output_text.delta\",\"item_id\":\"{message_id}\",\"delta\":\"{text}\"}}\n\ndata: {{\"type\":\"response.completed\",\"response\":{{\"id\":\"{response_id}\",\"model\":\"local-responses\",\"output\":[{{\"type\":\"message\",\"role\":\"assistant\",\"content\":[{{\"type\":\"output_text\",\"text\":\"{text}\"}}]}}],\"usage\":{{\"input_tokens\":1,\"output_tokens\":1,\"total_tokens\":2}}}}}}\n\ndata: [DONE]\n\n"
    );
    let response = format!(
        "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
        body.len()
    );
    socket.write_all(response.as_bytes()).await.unwrap();
    socket.shutdown().await.unwrap();
}

fn openai_provider(base_url: String) -> OpenAiProvider {
    let mut model = default_models()
        .into_iter()
        .find(|model| model.slug == "gpt-5.5")
        .expect("bundled reasoning model");
    model.slug = "local-responses".to_string();
    model.context_window = Some(128_000);
    OpenAiProvider::new(
        ProviderInfo {
            protocol: crate::provider_info::ProviderWireProtocol::Responses,
            connection_mode: crate::provider_info::ProviderConnectionMode::Http,
            name: "Local Responses".to_string(),
            base_url,
            bearer_token: Some("test-token".to_string()),
            http_headers: Some(HashMap::from([
                ("x-provider-test".to_string(), "present".to_string()),
                (
                    "x-codex-beta-features".to_string(),
                    "existing_feature".to_string(),
                ),
            ])),
            default_model: "local-responses".to_string(),
            tool_wire_policy: crate::provider_info::ToolWirePolicy::NativeCustomTools,
            apply_patch_tool_type: Some(crate::provider_info::ApplyPatchToolType::Freeform),
            service_capabilities: Default::default(),
        },
        vec![model],
    )
    .unwrap()
}

fn compaction_request(mode: OpenAiCompactionMode) -> ModelCompactionRequest {
    ModelCompactionRequest {
        mode,
        model: "local-responses".to_string(),
        instructions: "canonical instructions".to_string(),
        input: vec![ModelContextItem::from(Message {
            role: MessageRole::User,
            content: MessageContent::Text("hello".to_string()),
            reasoning_content: None,
            metadata: HashMap::new(),
        })],
        tools: vec![ToolSchema::function(
            "read_file",
            "Read a file",
            serde_json::json!({"type": "object", "properties": {}}),
        )],
        parallel_tool_calls: true,
        reasoning: Some(ReasoningConfig {
            effort: Some("medium".to_string()),
            summary: Some(ReasoningSummary::Enabled),
        }),
        prompt_cache_key: Some("cache-key".to_string()),
    }
}

#[tokio::test]
async fn responses_websocket_reuses_the_agent_session_connection() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let websocket = accept_hdr_async(stream, validate_openai_websocket_handshake)
            .await
            .unwrap();
        let (mut writer, mut reader) = websocket.split();
        let mut requests = Vec::new();
        for ordinal in 1..=2 {
            let WebSocketMessage::Text(text) = reader.next().await.unwrap().unwrap() else {
                panic!("expected a text response.create frame");
            };
            requests.push(serde_json::from_str::<serde_json::Value>(text.as_str()).unwrap());
            let response_id = format!("resp-{ordinal}");
            for event in [
                serde_json::json!({
                    "type": "response.created",
                    "response": {"id": response_id.clone(), "model": "local-responses"}
                }),
                serde_json::json!({
                    "type": "response.output_text.delta",
                    "item_id": format!("message-{ordinal}"),
                    "delta": format!("ok-{ordinal}")
                }),
                serde_json::json!({
                    "type": "response.completed",
                    "response": {
                        "id": response_id,
                        "model": "local-responses",
                        "output": [{
                            "type": "message",
                            "id": format!("message-{ordinal}"),
                            "status": "completed",
                            "role": "assistant",
                            "content": [{
                                "type": "output_text",
                                "text": format!("ok-{ordinal}")
                            }]
                        }],
                        "usage": {"input_tokens": 1, "output_tokens": 1, "total_tokens": 2}
                    }
                }),
            ] {
                writer
                    .send(WebSocketMessage::Text(event.to_string().into()))
                    .await
                    .unwrap();
            }
            if ordinal == 1 {
                writer
                    .send(WebSocketMessage::Ping(vec![1, 2, 3].into()))
                    .await
                    .unwrap();
                let pong = tokio::time::timeout(std::time::Duration::from_secs(1), reader.next())
                    .await
                    .expect("client must answer ping while the model stream is idle")
                    .expect("pong frame")
                    .expect("valid pong frame");
                assert_eq!(pong, WebSocketMessage::Pong(vec![1, 2, 3].into()));
            }
        }
        requests
    });

    let mut info = ProviderInfo::openai(Some(format!("http://{address}/v1")));
    info.default_model = "local-responses".to_string();
    info.bearer_token = Some("test-token".to_string());
    let mut model = ModelInfo::fallback("local-responses");
    model.context_window = Some(128_000);
    let provider = OpenAiProvider::new(info, vec![model]).unwrap();
    let transport_session = crate::ModelTransportSession::default();
    let (event_tx, _event_rx) = tokio::sync::broadcast::channel(16);

    let mut first_request = minimal_request("local-responses");
    first_request.store = Some(true);
    first_request.prompt_cache_key = Some("thread-generation-key".to_string());
    first_request.transport_session = transport_session.clone();
    let first = provider
        .stream_complete(first_request, event_tx.clone())
        .await
        .unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    let mut second_request = minimal_request("local-responses");
    second_request.store = Some(true);
    second_request.prompt_cache_key = Some("thread-generation-key".to_string());
    second_request.input.extend([
        Message {
            role: MessageRole::Assistant,
            content: MessageContent::Text("ok-1".to_string()),
            reasoning_content: None,
            metadata: HashMap::new(),
        }
        .into(),
        Message {
            role: MessageRole::User,
            content: MessageContent::Text("again".to_string()),
            reasoning_content: None,
            metadata: HashMap::new(),
        }
        .into(),
    ]);
    second_request.transport_session = transport_session;
    let second = provider
        .stream_complete(second_request, event_tx)
        .await
        .unwrap();
    let requests = server.await.unwrap();

    assert_eq!(first.content.as_deref(), Some("ok-1"));
    assert_eq!(second.content.as_deref(), Some("ok-2"));
    assert_eq!(requests[0]["type"], "response.create");
    assert_eq!(requests[0]["tools"], serde_json::json!([]));
    assert_eq!(requests[0]["store"], false);
    assert_eq!(requests[1]["store"], false);
    assert_eq!(requests[0]["prompt_cache_key"], "thread-generation-key");
    assert_eq!(requests[1]["prompt_cache_key"], "thread-generation-key");
    assert_eq!(requests[1]["previous_response_id"], "resp-1");
    assert_eq!(
        requests[1]["input"],
        serde_json::json!([{
            "type": "message",
            "role": "user",
            "content": [{ "type": "input_text", "text": "again" }]
        }])
    );
}

#[tokio::test]
async fn responses_websocket_does_not_replay_after_the_stream_starts() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let websocket = accept_async(stream).await.unwrap();
        let (mut writer, mut reader) = websocket.split();
        let WebSocketMessage::Text(first) = reader.next().await.unwrap().unwrap() else {
            panic!("expected first response.create frame");
        };
        for event in [
            serde_json::json!({
                "type": "response.created",
                "response": {"id": "partial-response", "model": "local-responses"}
            }),
            serde_json::json!({
                "type": "response.output_text.delta",
                "item_id": "partial-message",
                "delta": "partial"
            }),
        ] {
            writer
                .send(WebSocketMessage::Text(event.to_string().into()))
                .await
                .unwrap();
        }
        writer
            .send(WebSocketMessage::Close(Some(CloseFrame {
                code: CloseCode::Error,
                reason: "upstream websocket proxy failed".into(),
            })))
            .await
            .unwrap();
        drop(writer);

        assert!(
            tokio::time::timeout(Duration::from_millis(750), listener.accept())
                .await
                .is_err(),
            "a request that emitted stream events must not be replayed"
        );
        serde_json::from_str::<serde_json::Value>(first.as_str()).unwrap()
    });

    let mut info = ProviderInfo::openai(Some(format!("http://{address}/v1")));
    info.default_model = "local-responses".to_string();
    info.bearer_token = Some("test-token".to_string());
    let mut model = ModelInfo::fallback("local-responses");
    model.context_window = Some(128_000);
    let provider = OpenAiProvider::new(info, vec![model]).unwrap();
    let (event_tx, mut event_rx) = tokio::sync::broadcast::channel(64);
    let mut request = minimal_request("local-responses");
    request.trace = Some(CompletionTraceContext {
        session_id: "session-1".to_string(),
        turn_id: "turn-1".to_string(),
        inference_id: "turn-1-inf-0".to_string(),
        plan_mode: false,
        trace_sequence_base: 0,
    });

    let error = provider
        .stream_complete(request, event_tx)
        .await
        .expect_err("partial stream failure must be returned without replay");
    let initial = server.await.unwrap();
    let events = std::iter::from_fn(|| event_rx.try_recv().ok()).collect::<Vec<_>>();

    assert!(error.is_transient_model_transport());
    assert_eq!(initial["type"], "response.create");
    assert!(events.iter().any(|event| matches!(
        event,
        AgentEvent::TracePartFailed { item, error }
            if item.item_id == "turn-1-inf-0-text-final-1"
                && error.contains("upstream websocket proxy failed")
    )));
}

#[tokio::test]
async fn responses_websocket_does_not_retry_invalid_request_error() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let websocket = accept_async(stream).await.unwrap();
        let (mut writer, mut reader) = websocket.split();
        let WebSocketMessage::Text(_) = reader.next().await.unwrap().unwrap() else {
            panic!("expected response.create frame");
        };
        writer
            .send(WebSocketMessage::Text(
                serde_json::json!({
                    "type": "error",
                    "status": 400,
                    "error": {
                        "code": "invalid_request_error",
                        "message": "model does not support image inputs"
                    }
                })
                .to_string()
                .into(),
            ))
            .await
            .unwrap();
    });

    let mut info = ProviderInfo::openai(Some(format!("http://{address}/v1")));
    info.default_model = "local-responses".to_string();
    let provider = OpenAiProvider::new(info, vec![ModelInfo::fallback("local-responses")]).unwrap();
    let (event_tx, _event_rx) = tokio::sync::broadcast::channel(16);

    let error = provider
        .stream_complete(minimal_request("local-responses"), event_tx)
        .await
        .expect_err("invalid request must fail without transport retry");
    server.await.unwrap();

    assert!(!error.is_transient_model_transport());
    assert!(error.to_string().contains("invalid_request_error"));
    assert!(error.to_string().contains("HTTP 400"));
}

#[tokio::test]
async fn responses_websocket_does_not_retry_unauthorized_handshake() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut request = [0_u8; 4_096];
        let read = socket.read(&mut request).await.unwrap();
        assert!(String::from_utf8_lossy(&request[..read]).contains("GET /v1/responses"));
        socket
            .write_all(
                b"HTTP/1.1 401 Unauthorized\r\ncontent-length: 0\r\nconnection: close\r\n\r\n",
            )
            .await
            .unwrap();
    });

    let mut info = ProviderInfo::openai(Some(format!("http://{address}/v1")));
    info.default_model = "local-responses".to_string();
    let provider = OpenAiProvider::new(info, vec![ModelInfo::fallback("local-responses")]).unwrap();
    let (event_tx, _event_rx) = tokio::sync::broadcast::channel(16);

    let error = provider
        .stream_complete(minimal_request("local-responses"), event_tx)
        .await
        .expect_err("unauthorized handshake must fail without transport retry");
    server.await.unwrap();

    assert!(!error.is_transient_model_transport());
    assert_eq!(
        error.to_string(),
        "HTTP error: Responses WebSocket handshake failed with HTTP 401"
    );
}

#[tokio::test]
async fn responses_websocket_retries_an_immediate_close_with_full_history() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let websocket = accept_async(stream).await.unwrap();
        let (mut writer, mut reader) = websocket.split();
        let WebSocketMessage::Text(first) = reader.next().await.unwrap().unwrap() else {
            panic!("expected first response.create frame");
        };
        writer.send(WebSocketMessage::Close(None)).await.unwrap();
        drop(writer);

        let (stream, _) = listener.accept().await.unwrap();
        let websocket = accept_async(stream).await.unwrap();
        let (mut writer, mut reader) = websocket.split();
        let WebSocketMessage::Text(retried) = reader.next().await.unwrap().unwrap() else {
            panic!("expected retried response.create frame");
        };
        for event in [
            serde_json::json!({
                "type": "response.output_text.delta",
                "item_id": "message-1",
                "delta": "ok"
            }),
            serde_json::json!({
                "type": "response.completed",
                "response": {
                    "id": "resp-1",
                    "model": "local-responses",
                    "output": [{
                        "type": "message",
                        "role": "assistant",
                        "content": [{ "type": "output_text", "text": "ok" }]
                    }],
                    "usage": {"input_tokens": 1, "output_tokens": 1, "total_tokens": 2}
                }
            }),
        ] {
            writer
                .send(WebSocketMessage::Text(event.to_string().into()))
                .await
                .unwrap();
        }
        [first, retried]
            .map(|frame| serde_json::from_str::<serde_json::Value>(frame.as_str()).unwrap())
    });

    let mut info = ProviderInfo::openai(Some(format!("http://{address}/v1")));
    info.default_model = "local-responses".to_string();
    info.bearer_token = Some("test-token".to_string());
    let mut model = ModelInfo::fallback("local-responses");
    model.context_window = Some(128_000);
    let provider = OpenAiProvider::new(info, vec![model]).unwrap();
    let (event_tx, _event_rx) = tokio::sync::broadcast::channel(16);

    let mut request = minimal_request("local-responses");
    request.prompt_cache_key = Some("thread-generation-key".to_string());
    let response = provider.stream_complete(request, event_tx).await.unwrap();
    let [initial, retried] = server.await.unwrap();

    assert_eq!(response.content.as_deref(), Some("ok"));
    assert_eq!(initial, retried);
    assert_eq!(retried["tools"], serde_json::json!([]));
}

#[tokio::test]
async fn responses_websocket_falls_back_to_http_after_one_full_replay() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        let mut websocket_requests = Vec::new();
        for _ in 0..=RESPONSES_WEBSOCKET_MAX_RETRIES {
            let (stream, _) = listener.accept().await.unwrap();
            let websocket = accept_async(stream).await.unwrap();
            let (mut writer, mut reader) = websocket.split();
            let WebSocketMessage::Text(request) = reader.next().await.unwrap().unwrap() else {
                panic!("expected response.create frame");
            };
            websocket_requests
                .push(serde_json::from_str::<serde_json::Value>(request.as_str()).unwrap());
            writer
                .send(WebSocketMessage::Close(Some(CloseFrame {
                    code: CloseCode::Error,
                    reason: "upstream websocket proxy failed".into(),
                })))
                .await
                .unwrap();
        }

        let mut http_requests = Vec::new();
        for (response_id, message_id, text) in [
            ("http-response-1", "http-message-1", "http-ok-1"),
            ("http-response-2", "http-message-2", "http-ok-2"),
        ] {
            let (mut socket, _) = listener.accept().await.unwrap();
            http_requests.push(capture_http_request(&mut socket).await);
            send_responses_sse(&mut socket, response_id, message_id, text).await;
        }

        (websocket_requests, http_requests)
    });

    let mut info = ProviderInfo::openai(Some(format!("http://{address}/v1")));
    info.default_model = "local-responses".to_string();
    info.bearer_token = Some("test-token".to_string());
    let provider = OpenAiProvider::new(info, vec![ModelInfo::fallback("local-responses")]).unwrap();
    let transport_session = crate::ModelTransportSession::default();
    let (event_tx, _event_rx) = tokio::sync::broadcast::channel(16);

    let mut first_request = minimal_request("local-responses");
    first_request.transport_session = transport_session.clone();
    let first = provider
        .stream_complete(first_request, event_tx.clone())
        .await
        .unwrap();
    assert!(transport_session.uses_responses_http_fallback());

    let mut second_request = minimal_request("local-responses");
    second_request.input.extend([
        Message {
            role: MessageRole::Assistant,
            content: MessageContent::Text("http-ok-1".to_string()),
            reasoning_content: None,
            metadata: HashMap::new(),
        }
        .into(),
        Message {
            role: MessageRole::User,
            content: MessageContent::Text("again".to_string()),
            reasoning_content: None,
            metadata: HashMap::new(),
        }
        .into(),
    ]);
    second_request.transport_session = transport_session;
    let second = provider
        .stream_complete(second_request, event_tx)
        .await
        .unwrap();
    let (websocket_requests, http_requests) = server.await.unwrap();

    assert_eq!(first.content.as_deref(), Some("http-ok-1"));
    assert_eq!(second.content.as_deref(), Some("http-ok-2"));
    assert_eq!(websocket_requests.len(), 2);
    assert_eq!(websocket_requests[0], websocket_requests[1]);
    assert_eq!(http_requests.len(), 2);
    assert_eq!(http_requests[0].request_line, "POST /v1/responses HTTP/1.1");
    assert_eq!(http_requests[1].request_line, "POST /v1/responses HTTP/1.1");
    assert_eq!(http_requests[0].body["input"].as_array().unwrap().len(), 1);
    assert_eq!(http_requests[1].body["input"].as_array().unwrap().len(), 3);
}

#[tokio::test]
async fn responses_websocket_retries_an_invalid_continuation_once_with_full_history() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let websocket = accept_async(stream).await.unwrap();
        let (mut writer, mut reader) = websocket.split();
        let WebSocketMessage::Text(first) = reader.next().await.unwrap().unwrap() else {
            panic!("expected first response.create frame");
        };
        for event in [
            serde_json::json!({
                "type": "response.output_text.delta",
                "item_id": "message-1",
                "delta": "ok-1"
            }),
            serde_json::json!({
                "type": "response.completed",
                "response": {
                    "id": "resp-1",
                    "model": "local-responses",
                    "output": [{
                        "type": "message",
                        "role": "assistant",
                        "content": [{ "type": "output_text", "text": "ok-1" }]
                    }],
                    "usage": {"input_tokens": 1, "output_tokens": 1, "total_tokens": 2}
                }
            }),
        ] {
            writer
                .send(WebSocketMessage::Text(event.to_string().into()))
                .await
                .unwrap();
        }
        let WebSocketMessage::Text(incremental) = reader.next().await.unwrap().unwrap() else {
            panic!("expected incremental response.create frame");
        };
        writer
            .send(WebSocketMessage::Text(
                serde_json::json!({
                    "type": "error",
                    "error": {
                        "code": "invalid_previous_response_id",
                        "message": "previous_response_id is no longer valid"
                    }
                })
                .to_string()
                .into(),
            ))
            .await
            .unwrap();
        drop(writer);

        let (stream, _) = listener.accept().await.unwrap();
        let websocket = accept_async(stream).await.unwrap();
        let (mut writer, mut reader) = websocket.split();
        let WebSocketMessage::Text(retried) = reader.next().await.unwrap().unwrap() else {
            panic!("expected full-history retry frame");
        };
        for event in [
            serde_json::json!({
                "type": "response.output_text.delta",
                "item_id": "message-2",
                "delta": "ok-2"
            }),
            serde_json::json!({
                "type": "response.completed",
                "response": {
                    "id": "resp-2",
                    "model": "local-responses",
                    "output": [{
                        "type": "message",
                        "role": "assistant",
                        "content": [{ "type": "output_text", "text": "ok-2" }]
                    }],
                    "usage": {"input_tokens": 3, "output_tokens": 1, "total_tokens": 4}
                }
            }),
        ] {
            writer
                .send(WebSocketMessage::Text(event.to_string().into()))
                .await
                .unwrap();
        }
        [first, incremental, retried]
            .map(|frame| serde_json::from_str::<serde_json::Value>(frame.as_str()).unwrap())
    });

    let mut info = ProviderInfo::openai(Some(format!("http://{address}/v1")));
    info.default_model = "local-responses".to_string();
    info.bearer_token = Some("test-token".to_string());
    let mut model = ModelInfo::fallback("local-responses");
    model.context_window = Some(128_000);
    let provider = OpenAiProvider::new(info, vec![model]).unwrap();
    let transport_session = crate::ModelTransportSession::default();
    let (event_tx, _event_rx) = tokio::sync::broadcast::channel(16);

    let mut first_request = minimal_request("local-responses");
    first_request.transport_session = transport_session.clone();
    let first = provider
        .stream_complete(first_request, event_tx.clone())
        .await
        .unwrap();
    let mut second_request = minimal_request("local-responses");
    second_request.input.extend([
        Message {
            role: MessageRole::Assistant,
            content: MessageContent::Text("ok-1".to_string()),
            reasoning_content: None,
            metadata: HashMap::new(),
        }
        .into(),
        Message {
            role: MessageRole::User,
            content: MessageContent::Text("again".to_string()),
            reasoning_content: None,
            metadata: HashMap::new(),
        }
        .into(),
    ]);
    second_request.transport_session = transport_session;
    let second = provider
        .stream_complete(second_request, event_tx)
        .await
        .unwrap();
    let [initial, incremental, retried] = server.await.unwrap();

    assert_eq!(first.content.as_deref(), Some("ok-1"));
    assert_eq!(second.content.as_deref(), Some("ok-2"));
    assert!(initial.get("previous_response_id").is_none());
    assert_eq!(incremental["previous_response_id"], "resp-1");
    assert_eq!(incremental["input"].as_array().unwrap().len(), 1);
    assert!(retried.get("previous_response_id").is_none());
    assert_eq!(retried["input"].as_array().unwrap().len(), 3);
}

#[tokio::test]
async fn invalid_continuation_full_replay_consumes_the_websocket_retry_budget() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let websocket = accept_async(stream).await.unwrap();
        let (mut writer, mut reader) = websocket.split();
        let WebSocketMessage::Text(initial) = reader.next().await.unwrap().unwrap() else {
            panic!("expected initial response.create frame");
        };
        for event in [
            serde_json::json!({
                "type": "response.output_text.delta",
                "item_id": "message-1",
                "delta": "ok-1"
            }),
            serde_json::json!({
                "type": "response.completed",
                "response": {
                    "id": "resp-1",
                    "model": "local-responses",
                    "output": [{
                        "type": "message",
                        "role": "assistant",
                        "content": [{ "type": "output_text", "text": "ok-1" }]
                    }],
                    "usage": {"input_tokens": 1, "output_tokens": 1, "total_tokens": 2}
                }
            }),
        ] {
            writer
                .send(WebSocketMessage::Text(event.to_string().into()))
                .await
                .unwrap();
        }

        let WebSocketMessage::Text(incremental) = reader.next().await.unwrap().unwrap() else {
            panic!("expected incremental response.create frame");
        };
        writer
            .send(WebSocketMessage::Text(
                serde_json::json!({
                    "type": "error",
                    "error": {
                        "code": "invalid_previous_response_id",
                        "message": "previous_response_id is no longer valid"
                    }
                })
                .to_string()
                .into(),
            ))
            .await
            .unwrap();
        drop(writer);

        let (stream, _) = listener.accept().await.unwrap();
        let websocket = accept_async(stream).await.unwrap();
        let (mut writer, mut reader) = websocket.split();
        let WebSocketMessage::Text(full_replay) = reader.next().await.unwrap().unwrap() else {
            panic!("expected the only full-history WebSocket replay");
        };
        writer
            .send(WebSocketMessage::Close(Some(CloseFrame {
                code: CloseCode::Error,
                reason: "full replay failed".into(),
            })))
            .await
            .unwrap();
        drop(writer);

        let (mut socket, _) = listener.accept().await.unwrap();
        let http_request = capture_http_request(&mut socket).await;
        send_responses_sse(
            &mut socket,
            "http-response-2",
            "http-message-2",
            "http-ok-2",
        )
        .await;

        (
            [initial, incremental, full_replay]
                .map(|frame| serde_json::from_str::<serde_json::Value>(frame.as_str()).unwrap()),
            http_request,
        )
    });

    let mut info = ProviderInfo::openai(Some(format!("http://{address}/v1")));
    info.default_model = "local-responses".to_string();
    info.bearer_token = Some("test-token".to_string());
    let provider = OpenAiProvider::new(info, vec![ModelInfo::fallback("local-responses")]).unwrap();
    let transport_session = crate::ModelTransportSession::default();
    let (event_tx, _event_rx) = tokio::sync::broadcast::channel(16);

    let mut first_request = minimal_request("local-responses");
    first_request.transport_session = transport_session.clone();
    let first = provider
        .stream_complete(first_request, event_tx.clone())
        .await
        .unwrap();
    let mut second_request = minimal_request("local-responses");
    second_request.input.extend([
        Message {
            role: MessageRole::Assistant,
            content: MessageContent::Text("ok-1".to_string()),
            reasoning_content: None,
            metadata: HashMap::new(),
        }
        .into(),
        Message {
            role: MessageRole::User,
            content: MessageContent::Text("again".to_string()),
            reasoning_content: None,
            metadata: HashMap::new(),
        }
        .into(),
    ]);
    second_request.transport_session = transport_session.clone();
    let second = provider
        .stream_complete(second_request, event_tx)
        .await
        .unwrap();
    let ([initial, incremental, full_replay], http_request) = server.await.unwrap();

    assert_eq!(first.content.as_deref(), Some("ok-1"));
    assert_eq!(second.content.as_deref(), Some("http-ok-2"));
    assert!(transport_session.uses_responses_http_fallback());
    assert!(initial.get("previous_response_id").is_none());
    assert_eq!(incremental["previous_response_id"], "resp-1");
    assert!(full_replay.get("previous_response_id").is_none());
    assert_eq!(full_replay["input"].as_array().unwrap().len(), 3);
    assert_eq!(http_request.request_line, "POST /v1/responses HTTP/1.1");
    assert_eq!(http_request.body["input"].as_array().unwrap().len(), 3);
}

#[tokio::test]
async fn responses_websocket_does_not_commit_unconsumed_completion() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let websocket = accept_async(stream).await.unwrap();
        let (mut writer, mut reader) = websocket.split();
        let WebSocketMessage::Text(first) = reader.next().await.unwrap().unwrap() else {
            panic!("expected first response.create frame");
        };
        writer
            .send(WebSocketMessage::Text(
                serde_json::json!({
                    "type": "response.completed",
                    "response": {
                        "id": "resp-unconsumed",
                        "model": "local-responses",
                        "output": [{
                            "type": "message",
                            "role": "assistant",
                            "content": [{"type": "output_text", "text": "not-consumed"}]
                        }],
                        "usage": {"input_tokens": 1, "output_tokens": 1, "total_tokens": 2}
                    }
                })
                .to_string()
                .into(),
            ))
            .await
            .unwrap();
        drop(writer);

        let (stream, _) = listener.accept().await.unwrap();
        let websocket = accept_async(stream).await.unwrap();
        let (mut writer, mut reader) = websocket.split();
        let WebSocketMessage::Text(second) = reader.next().await.unwrap().unwrap() else {
            panic!("expected second response.create frame");
        };
        for event in [
            serde_json::json!({
                "type": "response.output_text.delta",
                "item_id": "message-2",
                "delta": "ok"
            }),
            serde_json::json!({
                "type": "response.completed",
                "response": {
                    "id": "resp-2",
                    "model": "local-responses",
                    "output": [{
                        "type": "message",
                        "role": "assistant",
                        "content": [{"type": "output_text", "text": "ok"}]
                    }],
                    "usage": {"input_tokens": 3, "output_tokens": 1, "total_tokens": 4}
                }
            }),
        ] {
            writer
                .send(WebSocketMessage::Text(event.to_string().into()))
                .await
                .unwrap();
        }
        [first, second]
            .map(|frame| serde_json::from_str::<serde_json::Value>(frame.as_str()).unwrap())
    });

    let mut info = ProviderInfo::openai(Some(format!("http://{address}/v1")));
    info.default_model = "local-responses".to_string();
    let provider = OpenAiProvider::new(info, vec![ModelInfo::fallback("local-responses")]).unwrap();
    let transport_session = crate::ModelTransportSession::default();
    let mut first_request = minimal_request("local-responses");
    first_request.transport_session = transport_session.clone();
    let mut stream = provider.stream_events(first_request).await.unwrap();
    stream
        .next()
        .await
        .expect("first decoded completion event")
        .unwrap();
    drop(stream);

    let mut second_request = minimal_request("local-responses");
    second_request.input.extend([
        Message {
            role: MessageRole::Assistant,
            content: MessageContent::Text("not-consumed".to_string()),
            reasoning_content: None,
            metadata: HashMap::new(),
        }
        .into(),
        Message {
            role: MessageRole::User,
            content: MessageContent::Text("again".to_string()),
            reasoning_content: None,
            metadata: HashMap::new(),
        }
        .into(),
    ]);
    second_request.transport_session = transport_session;
    let (event_tx, _event_rx) = tokio::sync::broadcast::channel(16);
    let response = provider
        .stream_complete(second_request, event_tx)
        .await
        .unwrap();
    let [first, second] = server.await.unwrap();

    assert_eq!(response.content.as_deref(), Some("ok"));
    assert!(first.get("previous_response_id").is_none());
    assert!(second.get("previous_response_id").is_none());
    assert_eq!(second["input"].as_array().unwrap().len(), 3);
}

#[tokio::test]
async fn v2_compaction_uses_responses_trigger_feature_and_completed_usage() {
    let sse_body = concat!(
        "data: {\"type\":\"response.output_item.done\",\"item\":{\"type\":\"message\",\"role\":\"assistant\",\"content\":[{\"type\":\"output_text\",\"text\":\"ignored\"}]}}\n\n",
        "data: {\"type\":\"response.output_item.done\",\"item\":{\"type\":\"compaction\",\"encrypted_content\":\"encrypted-v2\"}}\n\n",
        "data: {\"type\":\"response.completed\",\"response\":{\"usage\":{\"input_tokens\":12,\"output_tokens\":3,\"total_tokens\":15}}}\n\n",
        "data: [DONE]\n\n"
    )
    .to_string();
    let (base_url, handle) = serve_sse_once(sse_body).await;
    let provider = openai_provider(base_url);

    let response = provider
        .compact_context(compaction_request(OpenAiCompactionMode::RemoteV2))
        .await
        .unwrap();
    let captured = handle.await.unwrap();

    assert_eq!(captured.request_line, "POST /responses HTTP/1.1");
    assert_eq!(
        captured.headers["x-codex-beta-features"],
        "existing_feature,remote_compaction_v2"
    );
    assert_eq!(
        captured.body["input"].as_array().unwrap().last().unwrap(),
        &serde_json::json!({"type": "compaction_trigger"})
    );
    assert_eq!(response.input.len(), 1);
    assert_eq!(response.usage.unwrap().total_tokens, 15);
}

#[tokio::test]
async fn v2_compaction_does_not_replay_after_stream_is_established() {
    let (base_url, handle) = serve_sse_once("data: [DONE]\n\n".to_string()).await;
    let provider = openai_provider(base_url);

    let error = provider
        .compact_context(compaction_request(OpenAiCompactionMode::RemoteV2))
        .await
        .unwrap_err();
    let captured = handle.await.unwrap();

    assert!(matches!(error, PureError::HttpError(_)));
    assert_eq!(captured.request_line, "POST /responses HTTP/1.1");
}

#[tokio::test]
async fn stream_complete_uses_chat_endpoint_without_auth_when_token_missing() {
    let sse_body = concat!(
        "data: {\"choices\":[{\"delta\":{\"content\":\"<final>ok</final>\"},\"finish_reason\":null}]}\n\n",
        "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}],\"usage\":{\"prompt_tokens\":1,\"completion_tokens\":2,\"total_tokens\":3}}\n\n",
        "data: [DONE]\n\n"
    )
    .to_string();
    let (base_url, handle) = serve_sse_once(sse_body).await;
    let mut model = ModelInfo::fallback("local-chat");
    model.context_window = Some(128_000);
    let provider = OpenAiProvider::new(
        ProviderInfo {
            protocol: crate::provider_info::ProviderWireProtocol::ChatCompletions,
            connection_mode: crate::provider_info::ProviderConnectionMode::Http,
            name: "Local Chat".to_string(),
            base_url,
            default_model: "local-chat".to_string(),
            bearer_token: None,
            http_headers: None,
            tool_wire_policy: crate::provider_info::ToolWirePolicy::FunctionFallback,
            apply_patch_tool_type: None,
            service_capabilities: Default::default(),
        },
        vec![model],
    )
    .unwrap();
    let (event_tx, _event_rx) = tokio::sync::broadcast::channel(8);

    let mut request = minimal_request("local-chat");
    request.prompt_cache_key = Some("must-not-cross-chat-wire".to_string());
    let response = provider.stream_complete(request, event_tx).await.unwrap();
    let captured = handle.await.unwrap();

    assert_eq!(response.content.as_deref(), Some("ok"));
    assert_eq!(response.usage.total_tokens, 3);
    assert_eq!(captured.request_line, "POST /chat/completions HTTP/1.1");
    assert!(!captured.headers.contains_key("authorization"));
    assert_eq!(captured.body["stream"], serde_json::json!(true));
    assert!(captured.body.get("prompt_cache_key").is_none());
}

#[tokio::test]
async fn openai_compatible_chat_provider_uses_chat_endpoint() {
    let sse_body = concat!(
        "data: {\"choices\":[{\"delta\":{\"content\":\"<final>mimo ok</final>\"},\"finish_reason\":null}]}\n\n",
        "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}],\"usage\":{\"prompt_tokens\":1,\"completion_tokens\":2,\"total_tokens\":3}}\n\n",
        "data: [DONE]\n\n"
    )
    .to_string();
    let (base_url, handle) = serve_sse_once(sse_body).await;
    let mut model = ModelInfo::fallback("mimo-chat");
    model.context_window = Some(128_000);
    let provider = OpenAiProvider::new(
        ProviderInfo::openai_compatible_chat("MiMo", base_url, "mimo-chat"),
        vec![model],
    )
    .unwrap();
    let (event_tx, _event_rx) = tokio::sync::broadcast::channel(8);

    let response = provider
        .stream_complete(minimal_request("mimo-chat"), event_tx)
        .await
        .unwrap();
    let captured = handle.await.unwrap();

    assert_eq!(response.content.as_deref(), Some("mimo ok"));
    assert_eq!(captured.request_line, "POST /chat/completions HTTP/1.1");
}

#[tokio::test]
async fn stream_complete_chat_tags_project_commentary_and_final_only() {
    let sse_body = concat!(
        "data: {\"choices\":[{\"delta\":{\"content\":\"<commentary>检查配置。</commentary>\"},\"finish_reason\":null}]}\n\n",
        "data: {\"choices\":[{\"delta\":{\"content\":\"<final>Ready</final>\"},\"finish_reason\":null}]}\n\n",
        "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}],\"usage\":{\"prompt_tokens\":1,\"completion_tokens\":2,\"total_tokens\":3}}\n\n",
        "data: [DONE]\n\n"
    )
    .to_string();
    let (base_url, handle) = serve_sse_once(sse_body).await;
    let mut model = ModelInfo::fallback("local-chat");
    model.context_window = Some(128_000);
    let provider = OpenAiProvider::new(
        ProviderInfo {
            protocol: crate::provider_info::ProviderWireProtocol::ChatCompletions,
            connection_mode: crate::provider_info::ProviderConnectionMode::Http,
            name: "Local Chat".to_string(),
            base_url,
            default_model: "local-chat".to_string(),
            bearer_token: None,
            http_headers: None,
            tool_wire_policy: crate::provider_info::ToolWirePolicy::FunctionFallback,
            apply_patch_tool_type: None,
            service_capabilities: Default::default(),
        },
        vec![model],
    )
    .unwrap();
    let request = CompletionRequest {
        trace: Some(CompletionTraceContext {
            session_id: "session-1".to_string(),
            turn_id: "turn-1".to_string(),
            inference_id: "inf-1".to_string(),
            plan_mode: true,
            trace_sequence_base: 0,
        }),
        ..minimal_request("local-chat")
    };
    let (event_tx, _event_rx) = tokio::sync::broadcast::channel(32);

    let response = provider.stream_complete(request, event_tx).await.unwrap();
    let captured = handle.await.unwrap();

    assert_eq!(captured.request_line, "POST /chat/completions HTTP/1.1");
    assert_eq!(response.content.as_deref(), Some("Ready"));
    assert!(response.trace_events.iter().any(|event| matches!(
        &event.kind,
        TraceEventKind::TracePartCompleted { item }
            if item.text_channel == Some(pl_trace::TraceTextChannel::Commentary)
                && item.content == "检查配置。"
    )));
    assert!(!response.trace_events.iter().any(|event| matches!(
        &event.kind,
        TraceEventKind::TracePartCompleted { item } if item.kind == TracePartKind::Plan
    )));
    assert!(response.trace_events.iter().any(|event| matches!(
        &event.kind,
        TraceEventKind::TracePartCompleted { item }
            if item.text_channel == Some(pl_trace::TraceTextChannel::Final)
                && item.content == "Ready"
    )));
}

#[tokio::test]
async fn stream_complete_sends_responses_bearer_and_custom_headers() {
    let sse_body = concat!(
        "data: {\"type\":\"response.output_item.added\",\"item\":{\"id\":\"msg_1\",\"type\":\"message\",\"role\":\"assistant\",\"phase\":\"final_answer\"}}\n\n",
        "data: {\"type\":\"response.output_text.delta\",\"item_id\":\"msg_1\",\"delta\":\"ok\"}\n\n",
        "data: {\"type\":\"response.output_item.done\",\"item\":{\"id\":\"msg_1\",\"type\":\"message\",\"role\":\"assistant\",\"phase\":\"final_answer\",\"content\":[{\"type\":\"output_text\",\"text\":\"ok\"}]}}\n\n",
        "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_1\",\"usage\":{\"input_tokens\":1,\"output_tokens\":2,\"total_tokens\":3}}}\n\n",
        "data: [DONE]\n\n"
    )
    .to_string();
    let (base_url, handle) = serve_sse_once(sse_body).await;
    let mut model = ModelInfo::fallback("local-responses");
    model.context_window = Some(128_000);
    let provider = OpenAiProvider::new(
        ProviderInfo {
            protocol: crate::provider_info::ProviderWireProtocol::Responses,
            connection_mode: crate::provider_info::ProviderConnectionMode::Http,
            name: "Local Responses".to_string(),
            base_url,
            bearer_token: Some("test-token".to_string()),
            http_headers: Some(HashMap::from([(
                "x-provider-test".to_string(),
                "present".to_string(),
            )])),
            default_model: "local-responses".to_string(),
            tool_wire_policy: crate::provider_info::ToolWirePolicy::NativeCustomTools,
            apply_patch_tool_type: Some(crate::provider_info::ApplyPatchToolType::Freeform),
            service_capabilities: Default::default(),
        },
        vec![model],
    )
    .unwrap();
    let (event_tx, _event_rx) = tokio::sync::broadcast::channel(8);

    let response = provider
        .stream_complete(minimal_request("local-responses"), event_tx)
        .await
        .unwrap();
    let captured = handle.await.unwrap();

    assert_eq!(response.content.as_deref(), Some("ok"));
    assert_eq!(response.usage.total_tokens, 3);
    assert_eq!(captured.request_line, "POST /responses HTTP/1.1");
    assert_eq!(
        captured.headers.get("authorization").map(String::as_str),
        Some("Bearer test-token")
    );
    assert_eq!(
        captured.headers.get("x-provider-test").map(String::as_str),
        Some("present")
    );
    assert_eq!(captured.body["stream"], serde_json::json!(true));
}

#[tokio::test]
async fn http_retries_transient_request_failures_before_the_stream_starts() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        let mut requests = Vec::new();
        for attempt in 0..=OPENAI_HTTP_MAX_RETRIES {
            let (mut socket, _) = listener.accept().await.unwrap();
            requests.push(capture_http_request(&mut socket).await);
            if attempt < OPENAI_HTTP_MAX_RETRIES {
                socket.shutdown().await.unwrap();
            } else {
                send_responses_sse(&mut socket, "http-response", "http-message", "http-ok").await;
            }
        }
        requests
    });

    let provider = openai_provider(format!("http://{address}/v1"));
    let (event_tx, _event_rx) = tokio::sync::broadcast::channel(16);
    let response = provider
        .stream_complete(minimal_request("local-responses"), event_tx)
        .await
        .unwrap();
    let requests = server.await.unwrap();

    assert_eq!(response.content.as_deref(), Some("http-ok"));
    assert_eq!(requests.len(), 1 + OPENAI_HTTP_MAX_RETRIES as usize);
    assert!(
        requests
            .iter()
            .all(|request| request.request_line == "POST /v1/responses HTTP/1.1")
    );
}

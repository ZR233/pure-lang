use std::collections::HashMap;

use pl_protocol::{Message, MessageContent, MessageRole};
use pl_trace::{AgentEvent, TracePartKind};
use pretty_assertions::assert_eq;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

use super::*;
use crate::protocol::openai::VisibleOutputProtocol;
use crate::protocol::openai::sse::{StreamEvent, ToolCallDeltaPayload};
use crate::request::{CompletionRequest, CompletionTraceContext, ToolCallPayload};
use crate::stream::event::ModelStreamEvent;
use crate::stream::{StreamCompletionAccumulator, VisibleOutputDecoder};
use pl_trace::TraceEventKind;

fn apply_completed(
    accumulator: &mut StreamCompletionAccumulator,
    event_tx: &pl_trace::AgentEventSender,
) {
    accumulator
        .apply(StreamEvent::Completed { response_id: None }, event_tx)
        .unwrap();
}

fn apply_tagged(
    decoder: &mut VisibleOutputDecoder,
    accumulator: &mut StreamCompletionAccumulator,
    event: ModelStreamEvent,
    event_tx: &pl_trace::AgentEventSender,
) {
    for event in decoder.decode(event) {
        accumulator.apply(event, event_tx).unwrap();
    }
}

fn tagged_decoder() -> VisibleOutputDecoder {
    VisibleOutputDecoder::new(VisibleOutputProtocol::TaggedText)
}

fn final_delta(id: &str, delta: &str) -> StreamEvent {
    StreamEvent::text_delta(
        id.to_string(),
        pl_trace::TraceTextChannel::Final,
        delta.to_string(),
    )
}

fn final_started(id: &str) -> StreamEvent {
    StreamEvent::text_started(id.to_string(), pl_trace::TraceTextChannel::Final)
}

fn commentary_started(id: &str) -> StreamEvent {
    StreamEvent::text_started(id.to_string(), pl_trace::TraceTextChannel::Commentary)
}

fn completed_text(
    id: &str,
    channel: pl_trace::TraceTextChannel,
    authoritative_text: Option<&str>,
) -> StreamEvent {
    StreamEvent::text_completed(
        id.to_string(),
        channel,
        authoritative_text.map(ToOwned::to_owned),
    )
}

fn summary_delta(id: &str, section_index: u32, delta: &str) -> StreamEvent {
    StreamEvent::reasoning_summary_delta(id.to_string(), section_index, delta.to_string())
}

fn summary_started(id: &str) -> StreamEvent {
    StreamEvent::reasoning_summary_started(id.to_string(), None)
}

fn trace_part_text(item: &pl_trace::TracePart) -> String {
    match item.kind {
        TracePartKind::Text | TracePartKind::Plan | TracePartKind::Turn => item.content.clone(),
        TracePartKind::Thinking => item
            .thinking_chunks
            .iter()
            .map(|chunk| chunk.content.as_str())
            .collect::<Vec<_>>()
            .join(""),
        TracePartKind::Tool => item
            .tool
            .as_ref()
            .map(|tool| tool.arguments.clone())
            .unwrap_or_default(),
        TracePartKind::Agent | TracePartKind::Inference => item.content.clone(),
    }
}

fn trace_delta_text(delta: &pl_trace::TraceDelta) -> String {
    match delta {
        pl_trace::TraceDelta::Text { delta, .. }
        | pl_trace::TraceDelta::Thinking { delta, .. }
        | pl_trace::TraceDelta::ToolArguments { delta }
        | pl_trace::TraceDelta::ToolResult { delta }
        | pl_trace::TraceDelta::Plan { delta } => delta.clone(),
    }
}

#[derive(Debug)]
struct CapturedHttpRequest {
    request_line: String,
    headers: HashMap<String, String>,
    body: serde_json::Value,
}

async fn serve_sse_once(
    sse_body: String,
) -> (String, tokio::task::JoinHandle<CapturedHttpRequest>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let handle = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut buffer = Vec::new();
        let mut temp = [0_u8; 1024];
        let (header_end, content_length) = loop {
            let n = socket.read(&mut temp).await.unwrap();
            assert_ne!(n, 0);
            buffer.extend_from_slice(&temp[..n]);
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
            let n = socket.read(&mut temp).await.unwrap();
            assert_ne!(n, 0);
            buffer.extend_from_slice(&temp[..n]);
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
        let body_slice = &buffer[header_end + 4..header_end + 4 + content_length];
        let body = serde_json::from_slice(body_slice).unwrap();
        let response = format!(
            "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
            sse_body.len(),
            sse_body
        );
        socket.write_all(response.as_bytes()).await.unwrap();
        socket.shutdown().await.unwrap();

        CapturedHttpRequest {
            request_line,
            headers,
            body,
        }
    });

    (format!("http://{addr}"), handle)
}

fn find_header_end(buffer: &[u8]) -> Option<usize> {
    buffer.windows(4).position(|window| window == b"\r\n\r\n")
}

fn minimal_request(model: &str) -> CompletionRequest {
    CompletionRequest {
        model: model.to_string(),
        instructions: None,
        messages: vec![Message {
            role: MessageRole::User,
            content: MessageContent::Text("hello".to_string()),
            reasoning_content: None,
            metadata: HashMap::new(),
        }],
        tools: Vec::new(),
        tool_choice: "auto".to_string(),
        parallel_tool_calls: false,
        temperature: None,
        max_tokens: None,
        reasoning: None,
        stream: false,
        trace: None,
    }
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
            provider_kind: crate::provider_info::ProviderKind::DeepSeek,
            name: "Local Chat".to_string(),
            base_url,
            default_model: "local-chat".to_string(),
            bearer_token: None,
            http_headers: None,
            tool_wire_policy: crate::provider_info::ToolWirePolicy::FunctionFallback,
            apply_patch_tool_type: None,
        },
        vec![model],
    )
    .unwrap();
    let (event_tx, _event_rx) = tokio::sync::broadcast::channel(8);

    let response = provider
        .stream_complete(minimal_request("local-chat"), event_tx)
        .await
        .unwrap();
    let captured = handle.await.unwrap();

    assert_eq!(response.content.as_deref(), Some("ok"));
    assert_eq!(response.usage.total_tokens, 3);
    assert_eq!(captured.request_line, "POST /chat/completions HTTP/1.1");
    assert!(!captured.headers.contains_key("authorization"));
    assert_eq!(captured.body["stream"], serde_json::json!(true));
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
            provider_kind: crate::provider_info::ProviderKind::DeepSeek,
            name: "Local Chat".to_string(),
            base_url,
            default_model: "local-chat".to_string(),
            bearer_token: None,
            http_headers: None,
            tool_wire_policy: crate::provider_info::ToolWirePolicy::FunctionFallback,
            apply_patch_tool_type: None,
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
            provider_kind: crate::provider_info::ProviderKind::OpenAi,
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

#[test]
fn stream_accumulator_returns_content_and_reasoning_content() {
    let (event_tx, mut event_rx) = tokio::sync::broadcast::channel(8);
    let mut accumulator = StreamCompletionAccumulator::new(Some(CompletionTraceContext {
        session_id: "session-1".to_string(),
        turn_id: "turn-1".to_string(),
        inference_id: "inf-1".to_string(),
        plan_mode: false,
        trace_sequence_base: 0,
    }));
    let mut decoder = tagged_decoder();

    accumulator
        .apply(summary_started("thinking"), &event_tx)
        .unwrap();
    accumulator
        .apply(summary_delta("thinking", 0, "先比较整数位。"), &event_tx)
        .unwrap();
    apply_tagged(
        &mut decoder,
        &mut accumulator,
        final_delta("final", "<final>9.11 更大。</final>"),
        &event_tx,
    );

    apply_completed(&mut accumulator, &event_tx);
    let response = accumulator.finish(&event_tx).unwrap();

    assert_eq!(response.content.as_deref(), Some("9.11 更大。"));
    assert_eq!(response.raw_content.as_deref(), Some("9.11 更大。"));
    assert_eq!(response.reasoning_content, None);
    assert!(response.trace_events.iter().any(|event| matches!(
        &event.kind,
        TraceEventKind::TracePartCompleted { item }
            if item.kind == TracePartKind::Thinking
                && trace_part_text(item) == "先比较整数位。"
    )));
    assert!(matches!(
        event_rx.try_recv().unwrap(),
        AgentEvent::TracePartStarted { .. }
    ));
    assert!(matches!(
        event_rx.try_recv().unwrap(),
        AgentEvent::TracePartDelta { .. }
    ));
    assert!(matches!(
        event_rx.try_recv().unwrap(),
        AgentEvent::TracePartStarted { .. }
    ));
    assert!(matches!(
        event_rx.try_recv().unwrap(),
        AgentEvent::TracePartDelta { .. }
    ));
}

#[test]
fn stream_accumulator_streams_commentary_without_content() {
    let (event_tx, _event_rx) = tokio::sync::broadcast::channel(8);
    let mut accumulator = StreamCompletionAccumulator::new(Some(CompletionTraceContext {
        session_id: "session-1".to_string(),
        turn_id: "turn-1".to_string(),
        inference_id: "inf-1".to_string(),
        plan_mode: false,
        trace_sequence_base: 0,
    }));
    let mut decoder = tagged_decoder();

    for delta in [
        "<comm",
        "entary>检查配置。</commentary>",
        "<final>完成。</final>",
    ] {
        apply_tagged(
            &mut decoder,
            &mut accumulator,
            final_delta("final", delta),
            &event_tx,
        );
    }

    apply_completed(&mut accumulator, &event_tx);
    let response = accumulator.finish(&event_tx).unwrap();

    assert_eq!(response.content.as_deref(), Some("完成。"));
    assert_eq!(response.raw_content.as_deref(), Some("完成。"));
    assert!(response.trace_events.iter().any(|event| matches!(
        &event.kind,
        TraceEventKind::TracePartCompleted { item }
            if item.text_channel == Some(pl_trace::TraceTextChannel::Commentary)
                && item.content == "检查配置。"
    )));
    assert!(response.trace_events.iter().any(|event| matches!(
        &event.kind,
        TraceEventKind::TracePartCompleted { item }
            if item.text_channel == Some(pl_trace::TraceTextChannel::Final)
        && item.content == "完成。"
    )));
}

#[test]
fn stream_accumulator_keeps_tagged_raw_reasoning_hidden() {
    let (event_tx, _event_rx) = tokio::sync::broadcast::channel(16);
    let mut accumulator = StreamCompletionAccumulator::new(Some(CompletionTraceContext {
        session_id: "session-1".to_string(),
        turn_id: "turn-1".to_string(),
        inference_id: "inf-1".to_string(),
        plan_mode: true,
        trace_sequence_base: 0,
    }));
    let mut decoder = tagged_decoder();

    for delta in [
        "<commentary>正在分析日志。</commentary>",
        "<final>完成。</final>",
    ] {
        apply_tagged(
            &mut decoder,
            &mut accumulator,
            StreamEvent::ReasoningRawDelta {
                id: "thinking".to_string(),
                content_index: 0,
                delta: delta.to_string(),
            },
            &event_tx,
        );
    }

    apply_completed(&mut accumulator, &event_tx);
    let response = accumulator.finish(&event_tx).unwrap();

    assert_eq!(response.content, None);
    assert_eq!(
        response.reasoning_content.as_deref(),
        Some("<commentary>正在分析日志。</commentary><final>完成。</final>")
    );
    assert!(!response.trace_events.iter().any(|event| matches!(
        &event.kind,
        TraceEventKind::TracePartCompleted { item }
            if item.kind == TracePartKind::Text
                || item.kind == TracePartKind::Plan
                || item.kind == TracePartKind::Thinking
    )));
}

#[test]
fn stream_accumulator_splits_repeated_tagged_commentary_and_final_blocks() {
    let (event_tx, _event_rx) = tokio::sync::broadcast::channel(16);
    let mut accumulator = StreamCompletionAccumulator::new(Some(CompletionTraceContext {
        session_id: "session-1".to_string(),
        turn_id: "turn-1".to_string(),
        inference_id: "inf-1".to_string(),
        plan_mode: false,
        trace_sequence_base: 0,
    }));
    let mut decoder = tagged_decoder();

    for delta in [
        "<commentary>A</commentary><final>B</final>",
        "<commentary>C</commentary><final>D</final>",
    ] {
        apply_tagged(
            &mut decoder,
            &mut accumulator,
            final_delta("final", delta),
            &event_tx,
        );
    }

    apply_completed(&mut accumulator, &event_tx);
    let response = accumulator.finish(&event_tx).unwrap();
    let completed_text = response
        .trace_events
        .iter()
        .filter_map(|event| match &event.kind {
            TraceEventKind::TracePartCompleted { item } if item.kind == TracePartKind::Text => {
                Some((
                    item.item_id.as_str(),
                    item.text_channel,
                    item.content.as_str(),
                ))
            }
            TraceEventKind::TracePartStarted { .. }
            | TraceEventKind::TracePartDelta { .. }
            | TraceEventKind::TracePartCompleted { .. }
            | TraceEventKind::TracePartFailed { .. }
            | TraceEventKind::PlanLifecycleChanged { .. }
            | TraceEventKind::InteractionChanged { .. }
            | TraceEventKind::SkillActivated { .. }
            | TraceEventKind::EnabledToolsRecorded { .. } => None,
        })
        .collect::<Vec<_>>();

    assert_eq!(response.content.as_deref(), Some("BD"));
    assert_eq!(response.raw_content.as_deref(), Some("BD"));
    assert_eq!(
        completed_text,
        vec![
            (
                "inf-1-text-commentary-1",
                Some(pl_trace::TraceTextChannel::Commentary),
                "A",
            ),
            (
                "inf-1-text-final-1",
                Some(pl_trace::TraceTextChannel::Final),
                "B",
            ),
            (
                "inf-1-text-commentary-2",
                Some(pl_trace::TraceTextChannel::Commentary),
                "C",
            ),
            (
                "inf-1-text-final-2",
                Some(pl_trace::TraceTextChannel::Final),
                "D",
            ),
        ]
    );
}

#[test]
fn stream_accumulator_keeps_untagged_reasoning_hidden() {
    let (event_tx, _event_rx) = tokio::sync::broadcast::channel(8);
    let mut accumulator = StreamCompletionAccumulator::new(Some(CompletionTraceContext {
        session_id: "session-1".to_string(),
        turn_id: "turn-1".to_string(),
        inference_id: "inf-1".to_string(),
        plan_mode: true,
        trace_sequence_base: 0,
    }));

    accumulator
        .apply(
            StreamEvent::ReasoningRawDelta {
                id: "thinking".to_string(),
                content_index: 0,
                delta: "先比较整数位。".to_string(),
            },
            &event_tx,
        )
        .unwrap();

    apply_completed(&mut accumulator, &event_tx);
    let response = accumulator.finish(&event_tx).unwrap();

    assert_eq!(response.content, None);
    assert_eq!(
        response.reasoning_content.as_deref(),
        Some("先比较整数位。")
    );
    assert!(!response.trace_events.iter().any(|event| matches!(
        &event.kind,
        TraceEventKind::TracePartCompleted { item }
            if item.kind == TracePartKind::Text
                || item.kind == TracePartKind::Plan
                || item.kind == TracePartKind::Thinking
    )));
}

#[test]
fn stream_accumulator_treats_untagged_display_text_as_final() {
    let (event_tx, _event_rx) = tokio::sync::broadcast::channel(8);
    let mut accumulator = StreamCompletionAccumulator::new(Some(CompletionTraceContext {
        session_id: "session-1".to_string(),
        turn_id: "turn-1".to_string(),
        inference_id: "inf-1".to_string(),
        plan_mode: false,
        trace_sequence_base: 0,
    }));

    accumulator
        .apply(final_started("final"), &event_tx)
        .unwrap();
    accumulator
        .apply(final_delta("final", "plain text"), &event_tx)
        .unwrap();
    apply_completed(&mut accumulator, &event_tx);

    let response = accumulator.finish(&event_tx).unwrap();

    assert_eq!(response.content.as_deref(), Some("plain text"));
    assert!(response.trace_events.iter().any(|event| matches!(
        &event.kind,
        TraceEventKind::TracePartCompleted { item }
            if item.text_channel == Some(pl_trace::TraceTextChannel::Final)
                && item.content == "plain text"
    )));
}

#[test]
fn stream_accumulator_uses_authoritative_completed_text_for_response_content() {
    let (event_tx, _event_rx) = tokio::sync::broadcast::channel(8);
    let mut accumulator = StreamCompletionAccumulator::new(Some(CompletionTraceContext {
        session_id: "session-1".to_string(),
        turn_id: "turn-1".to_string(),
        inference_id: "inf-1".to_string(),
        plan_mode: false,
        trace_sequence_base: 0,
    }));

    accumulator
        .apply(final_started("msg_1"), &event_tx)
        .unwrap();
    accumulator
        .apply(final_delta("msg_1", "partial"), &event_tx)
        .unwrap();
    accumulator
        .apply(
            completed_text(
                "msg_1",
                pl_trace::TraceTextChannel::Final,
                Some("final text"),
            ),
            &event_tx,
        )
        .unwrap();
    apply_completed(&mut accumulator, &event_tx);

    let response = accumulator.finish(&event_tx).unwrap();

    assert_eq!(response.content.as_deref(), Some("final text"));
    assert!(response.trace_events.iter().any(|event| matches!(
        &event.kind,
        TraceEventKind::TracePartCompleted { item }
            if item.text_channel == Some(pl_trace::TraceTextChannel::Final)
                && item.content == "final text"
    )));
}

#[test]
fn stream_accumulator_creates_part_for_authoritative_completion_without_delta() {
    let (event_tx, _event_rx) = tokio::sync::broadcast::channel(8);
    let mut accumulator = StreamCompletionAccumulator::new(Some(CompletionTraceContext {
        session_id: "session-1".to_string(),
        turn_id: "turn-1".to_string(),
        inference_id: "inf-1".to_string(),
        plan_mode: false,
        trace_sequence_base: 0,
    }));

    accumulator
        .apply(commentary_started("msg_progress"), &event_tx)
        .unwrap();
    accumulator
        .apply(
            completed_text(
                "msg_progress",
                pl_trace::TraceTextChannel::Commentary,
                Some("已完成检查"),
            ),
            &event_tx,
        )
        .unwrap();
    apply_completed(&mut accumulator, &event_tx);

    let response = accumulator.finish(&event_tx).unwrap();

    assert!(response.trace_events.iter().any(|event| matches!(
        &event.kind,
        TraceEventKind::TracePartStarted { item }
            if item.text_channel == Some(pl_trace::TraceTextChannel::Commentary)
                && item.content.is_empty()
    )));
    assert!(response.trace_events.iter().any(|event| matches!(
        &event.kind,
        TraceEventKind::TracePartCompleted { item }
            if item.text_channel == Some(pl_trace::TraceTextChannel::Commentary)
                && item.content == "已完成检查"
    )));
}

#[test]
fn stream_accumulator_does_not_extract_proposed_plan_item() {
    let (event_tx, _event_rx) = tokio::sync::broadcast::channel(16);
    let mut accumulator = StreamCompletionAccumulator::new(Some(CompletionTraceContext {
        session_id: "session-1".to_string(),
        turn_id: "turn-1".to_string(),
        inference_id: "inf-1".to_string(),
        plan_mode: true,
        trace_sequence_base: 0,
    }));
    let mut decoder = tagged_decoder();

    for delta in [
        "<commentary>Intro</commentary>\n<prop",
        "osed_plan>\n- step\n",
        "</proposed_plan>\n<final>Outro</final>",
    ] {
        apply_tagged(
            &mut decoder,
            &mut accumulator,
            final_delta("final", delta),
            &event_tx,
        );
    }

    apply_completed(&mut accumulator, &event_tx);
    let response = accumulator.finish(&event_tx).unwrap();

    assert_eq!(
        response.content.as_deref(),
        Some("\n<proposed_plan>\n- step\n</proposed_plan>\nOutro")
    );
    assert_eq!(
        response.raw_content.as_deref(),
        Some("\n<proposed_plan>\n- step\n</proposed_plan>\nOutro")
    );
    let completed_plan = response
        .trace_events
        .iter()
        .find_map(|event| match &event.kind {
            TraceEventKind::TracePartCompleted { item } if item.kind == TracePartKind::Plan => {
                Some(item)
            }
            TraceEventKind::TracePartStarted { .. }
            | TraceEventKind::TracePartDelta { .. }
            | TraceEventKind::TracePartCompleted { .. }
            | TraceEventKind::TracePartFailed { .. }
            | TraceEventKind::PlanLifecycleChanged { .. }
            | TraceEventKind::InteractionChanged { .. }
            | TraceEventKind::SkillActivated { .. }
            | TraceEventKind::EnabledToolsRecorded { .. } => None,
        });
    assert!(completed_plan.is_none());
}

#[test]
fn stream_accumulator_merges_chat_tool_call_chunks_by_index() {
    let (event_tx, _event_rx) = tokio::sync::broadcast::channel(8);
    let mut accumulator = StreamCompletionAccumulator::new(None);

    accumulator
        .apply(
            StreamEvent::ToolInputDelta {
                stream_id: Some("chat_tool_call:0".to_string()),
                item_id: "call_1".to_string(),
                call_id: None,
                name: Some("read_file".to_string()),
                payload_delta: ToolCallDeltaPayload::FunctionArguments(String::new()),
            },
            &event_tx,
        )
        .unwrap();
    accumulator
        .apply(
            StreamEvent::ToolInputDelta {
                stream_id: Some("chat_tool_call:0".to_string()),
                item_id: String::new(),
                call_id: None,
                name: None,
                payload_delta: ToolCallDeltaPayload::FunctionArguments(
                    "{\"path\":\"Cargo.toml\"}".to_string(),
                ),
            },
            &event_tx,
        )
        .unwrap();

    apply_completed(&mut accumulator, &event_tx);
    let response = accumulator.finish(&event_tx).unwrap();

    assert_eq!(response.tool_calls.len(), 1);
    assert_eq!(response.tool_calls[0].id, "call_1");
    assert_eq!(response.tool_calls[0].name, "read_file");
    match &response.tool_calls[0].payload {
        ToolCallPayload::Function { arguments } => {
            assert_eq!(arguments, &serde_json::json!({"path": "Cargo.toml"}));
        }
        other => panic!("unexpected payload: {other:?}"),
    }
}

#[test]
fn stream_accumulator_splits_reasoning_and_text_across_tool_boundary() {
    let (event_tx, _event_rx) = tokio::sync::broadcast::channel(32);
    let mut accumulator = StreamCompletionAccumulator::new(Some(CompletionTraceContext {
        session_id: "session-1".to_string(),
        turn_id: "turn-1".to_string(),
        inference_id: "turn-1-inf-0".to_string(),
        plan_mode: false,
        trace_sequence_base: 0,
    }));

    accumulator
        .apply(summary_started("thinking"), &event_tx)
        .unwrap();
    accumulator
        .apply(summary_delta("thinking", 0, "before"), &event_tx)
        .unwrap();
    accumulator
        .apply(final_started("msg_1"), &event_tx)
        .unwrap();
    accumulator
        .apply(final_delta("msg_1", "prelude"), &event_tx)
        .unwrap();
    accumulator
        .apply(
            StreamEvent::ToolInputStarted {
                stream_id: None,
                item_id: "call_1".to_string(),
                call_id: Some("call_1".to_string()),
                name: Some("bash".to_string()),
                payload_kind: crate::stream::event::ToolInputPayloadKind::FunctionArguments,
            },
            &event_tx,
        )
        .unwrap();
    accumulator
        .apply(
            StreamEvent::ToolCallReady {
                stream_id: None,
                item_id: "call_1".to_string(),
                call_id: Some("call_1".to_string()),
                name: Some("bash".to_string()),
                payload: Some(ToolCallDeltaPayload::FunctionArguments(
                    "{\"command\":\"pwd\"}".to_string(),
                )),
            },
            &event_tx,
        )
        .unwrap();
    accumulator
        .apply(summary_started("thinking#2"), &event_tx)
        .unwrap();
    accumulator
        .apply(summary_delta("thinking#2", 0, "after"), &event_tx)
        .unwrap();
    accumulator
        .apply(final_started("msg_1#2"), &event_tx)
        .unwrap();
    accumulator
        .apply(final_delta("msg_1#2", "done"), &event_tx)
        .unwrap();

    apply_completed(&mut accumulator, &event_tx);
    let response = accumulator.finish(&event_tx).unwrap();
    let completed = response
        .trace_events
        .iter()
        .filter_map(|event| match &event.kind {
            TraceEventKind::TracePartCompleted { item } => {
                Some((item.item_id.as_str(), item.kind, item.content.as_str()))
            }
            TraceEventKind::TracePartStarted { .. }
            | TraceEventKind::TracePartDelta { .. }
            | TraceEventKind::TracePartFailed { .. }
            | TraceEventKind::PlanLifecycleChanged { .. }
            | TraceEventKind::InteractionChanged { .. }
            | TraceEventKind::SkillActivated { .. }
            | TraceEventKind::EnabledToolsRecorded { .. } => None,
        })
        .collect::<Vec<_>>();
    let tool_seen = response.trace_events.iter().any(|event| match &event.kind {
        TraceEventKind::TracePartStarted { item }
        | TraceEventKind::TracePartCompleted { item }
        | TraceEventKind::TracePartFailed { item, .. } => {
            item.item_id == "turn-1-call_1" && item.kind == TracePartKind::Tool
        }
        TraceEventKind::TracePartDelta { event } => {
            event.item_id == "turn-1-call_1" && event.kind == TracePartKind::Tool
        }
        TraceEventKind::PlanLifecycleChanged { .. }
        | TraceEventKind::InteractionChanged { .. }
        | TraceEventKind::SkillActivated { .. }
        | TraceEventKind::EnabledToolsRecorded { .. } => false,
    });

    assert!(completed.contains(&("turn-1-inf-0-reasoning-1", TracePartKind::Thinking, "")));
    assert!(completed.contains(&("turn-1-inf-0-text-final-1", TracePartKind::Text, "prelude")));
    assert!(tool_seen);
    assert!(completed.contains(&("turn-1-inf-0-reasoning-2", TracePartKind::Thinking, "")));
    assert!(completed.contains(&("turn-1-inf-0-text-final-2", TracePartKind::Text, "done")));
}

#[test]
fn tagged_stream_flushes_visible_text_before_tool_call() {
    let (event_tx, _event_rx) = tokio::sync::broadcast::channel(32);
    let mut accumulator = StreamCompletionAccumulator::new(Some(CompletionTraceContext {
        session_id: "session-1".to_string(),
        turn_id: "turn-1".to_string(),
        inference_id: "turn-1-inf-0".to_string(),
        plan_mode: false,
        trace_sequence_base: 0,
    }));
    let mut decoder = tagged_decoder();

    apply_tagged(
        &mut decoder,
        &mut accumulator,
        final_delta("chat-final", "我先检查项目结构。"),
        &event_tx,
    );
    apply_tagged(
        &mut decoder,
        &mut accumulator,
        StreamEvent::ToolInputStarted {
            stream_id: Some("chat_tool_call:0".to_string()),
            item_id: "call_1".to_string(),
            call_id: None,
            name: Some("read_file".to_string()),
            payload_kind: crate::stream::event::ToolInputPayloadKind::FunctionArguments,
        },
        &event_tx,
    );
    apply_tagged(
        &mut decoder,
        &mut accumulator,
        StreamEvent::ToolCallReady {
            stream_id: Some("chat_tool_call:0".to_string()),
            item_id: "call_1".to_string(),
            call_id: None,
            name: Some("read_file".to_string()),
            payload: Some(ToolCallDeltaPayload::FunctionArguments(
                r#"{"path":"Cargo.toml"}"#.to_string(),
            )),
        },
        &event_tx,
    );

    apply_completed(&mut accumulator, &event_tx);
    let response = accumulator.finish(&event_tx).unwrap();
    let ordered_trace = response
        .trace_events
        .iter()
        .filter_map(|event| match &event.kind {
            TraceEventKind::TracePartCompleted { item } => {
                Some((item.kind, item.item_id.as_str(), trace_part_text(item)))
            }
            TraceEventKind::TracePartStarted { item } => {
                Some((item.kind, item.item_id.as_str(), trace_part_text(item)))
            }
            TraceEventKind::TracePartDelta { event } => Some((
                event.kind,
                event.item_id.as_str(),
                trace_delta_text(&event.delta),
            )),
            TraceEventKind::TracePartFailed { .. }
            | TraceEventKind::PlanLifecycleChanged { .. }
            | TraceEventKind::InteractionChanged { .. }
            | TraceEventKind::SkillActivated { .. }
            | TraceEventKind::EnabledToolsRecorded { .. } => None,
        })
        .collect::<Vec<_>>();

    assert_eq!(response.content.as_deref(), Some("我先检查项目结构。"));
    assert_eq!(response.tool_calls.len(), 1);
    assert!(ordered_trace.iter().any(|(kind, _, text)| {
        *kind == TracePartKind::Text && text == "我先检查项目结构。"
    }));
    let text_index = ordered_trace
        .iter()
        .position(|(kind, _, text)| *kind == TracePartKind::Text && text == "我先检查项目结构。")
        .expect("text part should complete before tool");
    let tool_index = ordered_trace
        .iter()
        .position(|(kind, _, _)| *kind == TracePartKind::Tool)
        .expect("tool part should start");
    assert!(text_index < tool_index);
}

#[test]
fn stream_accumulator_terminal_snapshots_converge_with_live_deltas() {
    let (event_tx, mut event_rx) = tokio::sync::broadcast::channel(32);
    let mut accumulator = StreamCompletionAccumulator::new(Some(CompletionTraceContext {
        session_id: "session-1".to_string(),
        turn_id: "turn-1".to_string(),
        inference_id: "turn-1-inf-0".to_string(),
        plan_mode: false,
        trace_sequence_base: 0,
    }));

    accumulator
        .apply(summary_started("thinking"), &event_tx)
        .unwrap();
    accumulator
        .apply(summary_delta("thinking", 0, "think"), &event_tx)
        .unwrap();
    accumulator
        .apply(final_started("msg_1"), &event_tx)
        .unwrap();
    accumulator
        .apply(final_delta("msg_1", "hello"), &event_tx)
        .unwrap();
    accumulator
        .apply(
            StreamEvent::ToolInputDelta {
                stream_id: None,
                item_id: "fc_1".to_string(),
                call_id: Some("call_1".to_string()),
                name: Some("bash".to_string()),
                payload_delta: ToolCallDeltaPayload::FunctionArguments(
                    "{\"command\":\"pwd\"}".to_string(),
                ),
            },
            &event_tx,
        )
        .unwrap();
    accumulator
        .apply(
            StreamEvent::ToolCallReady {
                stream_id: None,
                item_id: "fc_1".to_string(),
                call_id: Some("call_1".to_string()),
                name: Some("bash".to_string()),
                payload: Some(ToolCallDeltaPayload::FunctionArguments(
                    "{\"command\":\"pwd\"}".to_string(),
                )),
            },
            &event_tx,
        )
        .unwrap();

    apply_completed(&mut accumulator, &event_tx);
    let response = accumulator.finish(&event_tx).unwrap();
    let live_events = std::iter::from_fn(|| event_rx.try_recv().ok()).collect::<Vec<_>>();

    let started = live_events
        .iter()
        .filter_map(|event| match event {
            AgentEvent::TracePartStarted { item } => {
                Some((item.item_id.as_str(), item.kind, item.content.as_str()))
            }
            AgentEvent::TracePartDelta { .. }
            | AgentEvent::TracePartCompleted { .. }
            | AgentEvent::TracePartFailed { .. }
            | AgentEvent::InteractionChanged { .. }
            | AgentEvent::AgentRuntimeUpdated { .. }
            | AgentEvent::AgentStateChanged { .. }
            | AgentEvent::CollabAgentSpawnBegin { .. }
            | AgentEvent::CollabAgentSpawnEnd { .. }
            | AgentEvent::CollabAgentInteractionBegin { .. }
            | AgentEvent::CollabAgentInteractionEnd { .. }
            | AgentEvent::CollabWaitingBegin { .. }
            | AgentEvent::CollabWaitingEnd { .. }
            | AgentEvent::CollabCloseBegin { .. }
            | AgentEvent::CollabCloseEnd { .. }
            | AgentEvent::TurnInterrupted { .. }
            | AgentEvent::TurnBudgetLimited { .. }
            | AgentEvent::SkillActivated { .. }
            | AgentEvent::Done
            | AgentEvent::Error { .. } => None,
        })
        .collect::<Vec<_>>();
    assert!(started.contains(&("turn-1-inf-0-reasoning-1", TracePartKind::Thinking, "")));
    assert!(started.contains(&("turn-1-inf-0-text-final-1", TracePartKind::Text, "")));
    assert!(started.contains(&("turn-1-fc_1", TracePartKind::Tool, "")));

    let mut live = std::collections::HashMap::new();
    for event in &live_events {
        match event {
            AgentEvent::TracePartStarted { item } | AgentEvent::TracePartCompleted { item } => {
                live.insert(item.item_id.clone(), trace_part_text(item));
            }
            AgentEvent::TracePartDelta { event } => {
                live.entry(event.item_id.clone())
                    .or_insert_with(String::new)
                    .push_str(&trace_delta_text(&event.delta));
            }
            AgentEvent::TracePartFailed { item, .. } => {
                live.insert(item.item_id.clone(), trace_part_text(item));
            }
            AgentEvent::InteractionChanged { .. }
            | AgentEvent::AgentRuntimeUpdated { .. }
            | AgentEvent::AgentStateChanged { .. }
            | AgentEvent::CollabAgentSpawnBegin { .. }
            | AgentEvent::CollabAgentSpawnEnd { .. }
            | AgentEvent::CollabAgentInteractionBegin { .. }
            | AgentEvent::CollabAgentInteractionEnd { .. }
            | AgentEvent::CollabWaitingBegin { .. }
            | AgentEvent::CollabWaitingEnd { .. }
            | AgentEvent::CollabCloseBegin { .. }
            | AgentEvent::CollabCloseEnd { .. }
            | AgentEvent::TurnInterrupted { .. }
            | AgentEvent::TurnBudgetLimited { .. }
            | AgentEvent::SkillActivated { .. }
            | AgentEvent::Done
            | AgentEvent::Error { .. } => {}
        }
    }
    let replay = response
        .trace_events
        .iter()
        .filter_map(|event| match &event.kind {
            TraceEventKind::TracePartCompleted { item }
            | TraceEventKind::TracePartStarted { item }
                if matches!(
                    item.kind,
                    TracePartKind::Text | TracePartKind::Thinking | TracePartKind::Plan
                ) && item.status == pl_trace::TracePartStatus::Completed =>
            {
                Some((item.item_id.clone(), trace_part_text(item)))
            }
            TraceEventKind::TracePartStarted { item }
                if item.kind == TracePartKind::Tool
                    && item.item_id == "turn-1-fc_1"
                    && item
                        .tool
                        .as_ref()
                        .is_some_and(|tool| !tool.arguments.is_empty()) =>
            {
                Some((item.item_id.clone(), trace_part_text(item)))
            }
            TraceEventKind::TracePartStarted { .. }
            | TraceEventKind::TracePartDelta { .. }
            | TraceEventKind::TracePartCompleted { .. }
            | TraceEventKind::TracePartFailed { .. }
            | TraceEventKind::PlanLifecycleChanged { .. }
            | TraceEventKind::InteractionChanged { .. }
            | TraceEventKind::SkillActivated { .. }
            | TraceEventKind::EnabledToolsRecorded { .. } => None,
        })
        .collect::<std::collections::HashMap<_, _>>();

    assert_eq!(
        live.get("turn-1-inf-0-reasoning-1"),
        replay.get("turn-1-inf-0-reasoning-1")
    );
    assert_eq!(
        live.get("turn-1-inf-0-text-final-1"),
        replay.get("turn-1-inf-0-text-final-1")
    );
    assert_eq!(live.get("turn-1-fc_1"), replay.get("turn-1-fc_1"));
}

#[test]
fn stream_accumulator_requires_completed_event() {
    let (event_tx, _event_rx) = tokio::sync::broadcast::channel(8);
    let mut accumulator = StreamCompletionAccumulator::new(None);

    accumulator
        .apply(final_started("final"), &event_tx)
        .unwrap();
    accumulator
        .apply(final_delta("final", "partial"), &event_tx)
        .unwrap();

    let error = accumulator.finish(&event_tx).unwrap_err();

    match error {
        PureError::LlmError(message) => {
            assert_eq!(message, "provider stream ended before completion");
        }
        other => panic!("unexpected error: {other:?}"),
    }
}

#[test]
fn stream_accumulator_rejects_events_after_completed() {
    let (event_tx, _event_rx) = tokio::sync::broadcast::channel(8);
    let mut accumulator = StreamCompletionAccumulator::new(None);

    apply_completed(&mut accumulator, &event_tx);
    let error = accumulator
        .apply(
            StreamEvent::ReasoningRawDelta {
                id: "thinking".to_string(),
                content_index: 0,
                delta: "late".to_string(),
            },
            &event_tx,
        )
        .unwrap_err();

    match error {
        PureError::LlmError(message) => {
            assert_eq!(message, "provider stream emitted event after completion");
        }
        other => panic!("unexpected error: {other:?}"),
    }
}

#[test]
fn stream_accumulator_keeps_raw_reasoning_out_of_trace() {
    let (event_tx, _event_rx) = tokio::sync::broadcast::channel(8);
    let mut accumulator = StreamCompletionAccumulator::new(Some(CompletionTraceContext {
        session_id: "session-1".to_string(),
        turn_id: "turn-1".to_string(),
        inference_id: "turn-1-inf-0".to_string(),
        plan_mode: false,
        trace_sequence_base: 0,
    }));

    accumulator
        .apply(
            StreamEvent::ReasoningRawDelta {
                id: "thinking".to_string(),
                content_index: 0,
                delta: "raw only".to_string(),
            },
            &event_tx,
        )
        .unwrap();
    apply_completed(&mut accumulator, &event_tx);
    let response = accumulator.finish(&event_tx).unwrap();

    assert_eq!(response.reasoning_content.as_deref(), Some("raw only"));
    assert!(response.trace_events.iter().all(|event| !matches!(
        &event.kind,
        TraceEventKind::TracePartStarted { item }
            | TraceEventKind::TracePartCompleted { item }
            | TraceEventKind::TracePartFailed { item, .. }
            if item.kind == TracePartKind::Thinking
    )));
}

#[test]
fn stream_accumulator_rejects_tool_delta_without_name() {
    let (event_tx, _event_rx) = tokio::sync::broadcast::channel(8);
    let mut accumulator = StreamCompletionAccumulator::new(None);

    accumulator
        .apply(
            StreamEvent::ToolInputDelta {
                stream_id: Some("chat_tool_call:0".to_string()),
                item_id: "call_1".to_string(),
                call_id: None,
                name: None,
                payload_delta: ToolCallDeltaPayload::FunctionArguments(
                    "{\"path\":\"Cargo.toml\"}".to_string(),
                ),
            },
            &event_tx,
        )
        .unwrap();
    let error = accumulator
        .apply(StreamEvent::Completed { response_id: None }, &event_tx)
        .unwrap_err();

    match error {
        PureError::LlmError(message) => {
            assert_eq!(message, "provider emitted tool call without name");
        }
        other => panic!("unexpected error: {other:?}"),
    }
}

#[test]
fn stream_trace_part_ids_are_scoped_to_turn() {
    let (event_tx, _event_rx) = tokio::sync::broadcast::channel(8);
    let mut accumulator = StreamCompletionAccumulator::new(Some(CompletionTraceContext {
        session_id: "session-1".to_string(),
        turn_id: "turn-1".to_string(),
        inference_id: "turn-1-inf-0".to_string(),
        plan_mode: false,
        trace_sequence_base: 0,
    }));

    accumulator
        .apply(
            StreamEvent::ToolInputDelta {
                stream_id: None,
                item_id: "call_0".to_string(),
                call_id: Some("call_0".to_string()),
                name: Some("bash".to_string()),
                payload_delta: ToolCallDeltaPayload::FunctionArguments(
                    r#"{"command":"pwd"}"#.to_string(),
                ),
            },
            &event_tx,
        )
        .unwrap();
    accumulator
        .apply(
            StreamEvent::ToolCallReady {
                stream_id: None,
                item_id: "call_0".to_string(),
                call_id: Some("call_0".to_string()),
                name: Some("bash".to_string()),
                payload: Some(ToolCallDeltaPayload::FunctionArguments(
                    "{\"command\":\"pwd\"}".to_string(),
                )),
            },
            &event_tx,
        )
        .unwrap();

    apply_completed(&mut accumulator, &event_tx);
    let response = accumulator.finish(&event_tx).unwrap();

    assert_eq!(response.tool_calls[0].id, "call_0");
    let item_ids = response
        .trace_events
        .iter()
        .map(|event| match &event.kind {
            TraceEventKind::TracePartStarted { item }
            | TraceEventKind::TracePartCompleted { item } => item.item_id.as_str(),
            TraceEventKind::TracePartDelta { event } => event.item_id.as_str(),
            TraceEventKind::TracePartFailed { item, .. } => item.item_id.as_str(),
            TraceEventKind::PlanLifecycleChanged { .. }
            | TraceEventKind::InteractionChanged { .. }
            | TraceEventKind::SkillActivated { .. }
            | TraceEventKind::EnabledToolsRecorded { .. } => "",
        })
        .filter(|item_id| !item_id.is_empty())
        .collect::<Vec<_>>();
    assert_eq!(
        item_ids,
        vec!["turn-1-call_0", "turn-1-call_0", "turn-1-call_0"]
    );
}

#[test]
fn stream_accumulator_merges_tool_call_with_late_call_id() {
    let (event_tx, _event_rx) = tokio::sync::broadcast::channel(16);
    let mut accumulator = StreamCompletionAccumulator::new(Some(CompletionTraceContext {
        session_id: "session-1".to_string(),
        turn_id: "turn-1".to_string(),
        inference_id: "turn-1-inf-0".to_string(),
        plan_mode: false,
        trace_sequence_base: 0,
    }));

    accumulator
        .apply(
            StreamEvent::ToolInputStarted {
                stream_id: None,
                item_id: "fc_1".to_string(),
                call_id: None,
                name: Some("read_file".to_string()),
                payload_kind: crate::stream::event::ToolInputPayloadKind::FunctionArguments,
            },
            &event_tx,
        )
        .unwrap();
    accumulator
        .apply(
            StreamEvent::ToolInputDelta {
                stream_id: None,
                item_id: "fc_1".to_string(),
                call_id: Some("call_1".to_string()),
                name: None,
                payload_delta: ToolCallDeltaPayload::FunctionArguments(
                    r#"{"path":"Cargo.toml"}"#.to_string(),
                ),
            },
            &event_tx,
        )
        .unwrap();
    accumulator
        .apply(
            StreamEvent::ToolCallReady {
                stream_id: None,
                item_id: "fc_1".to_string(),
                call_id: Some("call_1".to_string()),
                name: None,
                payload: None,
            },
            &event_tx,
        )
        .unwrap();

    apply_completed(&mut accumulator, &event_tx);
    let response = accumulator.finish(&event_tx).unwrap();

    assert_eq!(response.tool_calls.len(), 1);
    assert_eq!(response.tool_calls[0].id, "fc_1");
    assert_eq!(response.tool_calls[0].call_id.as_deref(), Some("call_1"));
    assert_eq!(response.tool_calls[0].name, "read_file");
    let item_ids = response
        .trace_events
        .iter()
        .filter_map(|event| match &event.kind {
            TraceEventKind::TracePartStarted { item }
            | TraceEventKind::TracePartCompleted { item }
                if item.kind == TracePartKind::Tool =>
            {
                Some(item.item_id.as_str())
            }
            TraceEventKind::TracePartDelta { event } if event.kind == TracePartKind::Tool => {
                Some(event.item_id.as_str())
            }
            TraceEventKind::TracePartStarted { .. }
            | TraceEventKind::TracePartCompleted { .. }
            | TraceEventKind::TracePartDelta { .. }
            | TraceEventKind::TracePartFailed { .. }
            | TraceEventKind::PlanLifecycleChanged { .. }
            | TraceEventKind::InteractionChanged { .. }
            | TraceEventKind::SkillActivated { .. }
            | TraceEventKind::EnabledToolsRecorded { .. } => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(item_ids, vec!["turn-1-fc_1", "turn-1-fc_1", "turn-1-fc_1"]);
}

#[test]
fn stream_accumulator_keeps_tool_trace_id_when_item_id_arrives_late() {
    let (event_tx, _event_rx) = tokio::sync::broadcast::channel(16);
    let mut accumulator = StreamCompletionAccumulator::new(Some(CompletionTraceContext {
        session_id: "session-1".to_string(),
        turn_id: "turn-1".to_string(),
        inference_id: "turn-1-inf-0".to_string(),
        plan_mode: false,
        trace_sequence_base: 0,
    }));

    accumulator
        .apply(
            StreamEvent::ToolInputDelta {
                stream_id: None,
                item_id: String::new(),
                call_id: Some("call_1".to_string()),
                name: Some("read_file".to_string()),
                payload_delta: ToolCallDeltaPayload::FunctionArguments(
                    r#"{"path":"Car"#.to_string(),
                ),
            },
            &event_tx,
        )
        .unwrap();
    accumulator
        .apply(
            StreamEvent::ToolInputDelta {
                stream_id: None,
                item_id: "fc_1".to_string(),
                call_id: Some("call_1".to_string()),
                name: None,
                payload_delta: ToolCallDeltaPayload::FunctionArguments(r#"go.toml"}"#.to_string()),
            },
            &event_tx,
        )
        .unwrap();
    accumulator
        .apply(
            StreamEvent::ToolCallReady {
                stream_id: None,
                item_id: "fc_1".to_string(),
                call_id: Some("call_1".to_string()),
                name: None,
                payload: None,
            },
            &event_tx,
        )
        .unwrap();

    apply_completed(&mut accumulator, &event_tx);
    let response = accumulator.finish(&event_tx).unwrap();

    assert_eq!(response.tool_calls.len(), 1);
    assert_eq!(response.tool_calls[0].id, "fc_1");
    assert_eq!(response.tool_calls[0].call_id.as_deref(), Some("call_1"));
    assert_eq!(response.tool_calls[0].name, "read_file");
    let item_ids = response
        .trace_events
        .iter()
        .filter_map(|event| match &event.kind {
            TraceEventKind::TracePartStarted { item }
            | TraceEventKind::TracePartCompleted { item }
                if item.kind == TracePartKind::Tool =>
            {
                Some(item.item_id.as_str())
            }
            TraceEventKind::TracePartDelta { event } if event.kind == TracePartKind::Tool => {
                Some(event.item_id.as_str())
            }
            TraceEventKind::TracePartStarted { .. }
            | TraceEventKind::TracePartCompleted { .. }
            | TraceEventKind::TracePartDelta { .. }
            | TraceEventKind::TracePartFailed { .. }
            | TraceEventKind::PlanLifecycleChanged { .. }
            | TraceEventKind::InteractionChanged { .. }
            | TraceEventKind::SkillActivated { .. }
            | TraceEventKind::EnabledToolsRecorded { .. } => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(
        item_ids
            .iter()
            .copied()
            .collect::<std::collections::BTreeSet<_>>(),
        std::collections::BTreeSet::from(["turn-1-call_1"])
    );
    assert_eq!(
        item_ids,
        vec![
            "turn-1-call_1",
            "turn-1-call_1",
            "turn-1-call_1",
            "turn-1-call_1"
        ]
    );
}

#[test]
fn stream_trace_scope_rejects_similar_turn_prefix() {
    let (event_tx, _event_rx) = tokio::sync::broadcast::channel(8);
    let mut accumulator = StreamCompletionAccumulator::new(Some(CompletionTraceContext {
        session_id: "session-1".to_string(),
        turn_id: "turn-1".to_string(),
        inference_id: "turn-1-inf-0".to_string(),
        plan_mode: false,
        trace_sequence_base: 0,
    }));

    accumulator
        .apply(
            StreamEvent::ToolInputDelta {
                stream_id: None,
                item_id: "turn-10-call".to_string(),
                call_id: None,
                name: Some("bash".to_string()),
                payload_delta: ToolCallDeltaPayload::FunctionArguments(
                    r#"{"command":"pwd"}"#.to_string(),
                ),
            },
            &event_tx,
        )
        .unwrap();
    accumulator
        .apply(
            StreamEvent::ToolCallReady {
                stream_id: None,
                item_id: "turn-10-call".to_string(),
                call_id: None,
                name: None,
                payload: None,
            },
            &event_tx,
        )
        .unwrap();

    apply_completed(&mut accumulator, &event_tx);
    let response = accumulator.finish(&event_tx).unwrap();

    assert!(response.trace_events.iter().any(|event| matches!(
        &event.kind,
        TraceEventKind::TracePartStarted { item }
            if item.kind == TracePartKind::Tool
                && item.item_id == "turn-1-turn-10-call"
    )));
}

#[test]
fn stream_accumulator_uses_responses_added_item_name_when_done_omits_name() {
    let (event_tx, _event_rx) = tokio::sync::broadcast::channel(8);
    let mut accumulator = StreamCompletionAccumulator::new(None);

    accumulator
        .apply(
            StreamEvent::ToolInputDelta {
                stream_id: None,
                item_id: "ctc_1".to_string(),
                call_id: Some("call_1".to_string()),
                name: Some("apply_patch".to_string()),
                payload_delta: ToolCallDeltaPayload::CustomInput(String::new()),
            },
            &event_tx,
        )
        .unwrap();
    accumulator
        .apply(
            StreamEvent::ToolInputDelta {
                stream_id: None,
                item_id: "ctc_1".to_string(),
                call_id: Some("call_1".to_string()),
                name: None,
                payload_delta: ToolCallDeltaPayload::CustomInput(
                    "*** Begin Patch\n*** End Patch".to_string(),
                ),
            },
            &event_tx,
        )
        .unwrap();
    accumulator
        .apply(
            StreamEvent::ToolCallReady {
                stream_id: None,
                item_id: "ctc_1".to_string(),
                call_id: Some("call_1".to_string()),
                name: None,
                payload: Some(ToolCallDeltaPayload::CustomInput(
                    "*** Begin Patch\n*** End Patch".to_string(),
                )),
            },
            &event_tx,
        )
        .unwrap();

    apply_completed(&mut accumulator, &event_tx);
    let response = accumulator.finish(&event_tx).unwrap();

    assert_eq!(response.tool_calls.len(), 1);
    assert_eq!(response.tool_calls[0].id, "ctc_1");
    assert_eq!(response.tool_calls[0].name, "apply_patch");
    match &response.tool_calls[0].payload {
        ToolCallPayload::Custom { input } => {
            assert_eq!(input, "*** Begin Patch\n*** End Patch");
        }
        other => panic!("unexpected payload: {other:?}"),
    }
}

#[test]
fn configured_models_override_bundled_models() {
    let mut model = ModelInfo::fallback("deepseek-v4-flash");
    model.display_name = "Custom DeepSeek".to_string();
    let provider = OpenAiProvider::new(ProviderInfo::deepseek(None), vec![model]).unwrap();

    assert_eq!(
        provider.model_info("deepseek-v4-flash").display_name,
        "Custom DeepSeek"
    );
}

#[test]
fn redacts_openai_api_keys_from_error_text() {
    let input = "Incorrect API key provided: sk-abc123*******************************************************xyz.";

    let redacted = redact_secret_like_values(input);

    assert_eq!(redacted, "Incorrect API key provided: [REDACTED_API_KEY].");
    assert!(!redacted.contains("sk-abc123"));
}

use std::collections::HashMap;

use pl_protocol::{Message, MessageContent, MessageRole};
use pl_trace::{AgentEvent, TracePartKind};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

use super::*;
use crate::completion::stream::event::{ModelStreamEvent, ToolInputDeltaPayload};
use crate::completion::stream::{StreamCompletionAccumulator, VisibleOutputDecoder};
use crate::completion::{CompletionRequest, CompletionTraceContext, ToolCallPayload};
use crate::runtime::openai::VisibleOutputProtocol;
use pl_trace::TraceEventKind;

fn apply_completed(
    accumulator: &mut StreamCompletionAccumulator,
    event_tx: &pl_trace::AgentEventSender,
) {
    accumulator
        .apply(ModelStreamEvent::Completed { response_id: None }, event_tx)
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

fn final_delta(id: &str, delta: &str) -> ModelStreamEvent {
    ModelStreamEvent::text_delta(
        id.to_string(),
        pl_trace::TraceTextChannel::Final,
        delta.to_string(),
    )
}

fn final_started(id: &str) -> ModelStreamEvent {
    ModelStreamEvent::text_started(id.to_string(), pl_trace::TraceTextChannel::Final)
}

fn commentary_started(id: &str) -> ModelStreamEvent {
    ModelStreamEvent::text_started(id.to_string(), pl_trace::TraceTextChannel::Commentary)
}

fn completed_text(
    id: &str,
    channel: pl_trace::TraceTextChannel,
    authoritative_text: Option<&str>,
) -> ModelStreamEvent {
    ModelStreamEvent::text_completed(
        id.to_string(),
        channel,
        authoritative_text.map(ToOwned::to_owned),
    )
}

fn summary_delta(id: &str, section_index: u32, delta: &str) -> ModelStreamEvent {
    ModelStreamEvent::reasoning_summary_delta(id.to_string(), section_index, delta.to_string())
}

fn summary_started(id: &str) -> ModelStreamEvent {
    ModelStreamEvent::reasoning_summary_started(id.to_string(), None)
}

fn trace_part_text(item: &pl_trace::TracePart) -> String {
    match item.state() {
        pl_trace::TracePartState::Text(text) => text.content().to_string(),
        pl_trace::TracePartState::Plan(plan) => plan.content().to_string(),
        pl_trace::TracePartState::Turn(_) => String::new(),
        pl_trace::TracePartState::Thinking(thinking) => {
            let summary = thinking
                .summary()
                .iter()
                .map(|chunk| chunk.content.as_str())
                .collect::<Vec<_>>()
                .join("");
            if summary.is_empty() {
                thinking
                    .content()
                    .iter()
                    .map(|chunk| chunk.content.as_str())
                    .collect::<Vec<_>>()
                    .join("")
            } else {
                summary
            }
        }
        pl_trace::TracePartState::Tool(tool) => tool.invocation().arguments().to_string(),
        pl_trace::TracePartState::Agent(_) | pl_trace::TracePartState::Inference(_) => {
            String::new()
        }
    }
}

fn trace_delta_text(delta: &pl_trace::TraceDelta) -> String {
    match delta {
        pl_trace::TraceDelta::Text { delta, .. }
        | pl_trace::TraceDelta::Thinking { delta, .. }
        | pl_trace::TraceDelta::ReasoningContent { delta, .. }
        | pl_trace::TraceDelta::ToolArguments { delta }
        | pl_trace::TraceDelta::ToolResult { delta }
        | pl_trace::TraceDelta::Plan { delta } => delta.clone(),
    }
}

fn trace_text_channel(item: &pl_trace::TracePart) -> Option<pl_trace::TraceTextChannel> {
    item.text().map(pl_trace::TraceTextPart::channel)
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

fn minimal_request(_model: &str) -> CompletionRequest {
    CompletionRequest::builder()
        .input(vec![
            Message {
                role: MessageRole::User,
                content: MessageContent::Text("hello".to_string()),
                reasoning_content: None,
                tool_calls: None,
                tool_result: None,
                metadata: HashMap::new(),
            }
            .into(),
        ])
        .build()
}

fn invocation(event_tx: pl_trace::AgentEventSender) -> ModelInvocationContext {
    ModelInvocationContext::new(ModelSession::default(), event_tx)
}

struct TestCompletionResponse {
    response: CompletionResponse,
    trace_events: Vec<pl_trace::TraceEvent>,
}

impl std::ops::Deref for TestCompletionResponse {
    type Target = CompletionResponse;

    fn deref(&self) -> &Self::Target {
        &self.response
    }
}

fn finish_with_trace(
    accumulator: StreamCompletionAccumulator,
    event_tx: &pl_trace::AgentEventSender,
) -> pl_protocol::Result<TestCompletionResponse> {
    let trace_events = Default::default();
    let response = accumulator.finish_with_trace_events(event_tx, &trace_events)?;
    let trace_events = trace_events
        .lock()
        .map(|events| events.clone())
        .unwrap_or_default();
    Ok(TestCompletionResponse {
        response,
        trace_events,
    })
}

mod config;
mod text_stream;
mod tool_stream;
mod transport;

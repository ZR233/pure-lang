use super::tool_dispatch::ToolExecutionOutcome;
use super::*;
use crate::ContextCompactionTrigger;
use crate::tool::{
    GlobalToolInheritance, StaticTool, ToolCallContext, ToolManager, ToolPolicy, ToolResult,
};
use crate::turn::PermissionMode;
use pl_model::{ModelInfo, OpenAiCompactionMode, ProviderEndpoint, ToolCall};
use pl_protocol::{
    InteractionContent, InteractionResolution, ToolApprovalResolution,
    ToolApprovalResolutionPayload,
};
use pl_trace::{TraceEventKind, TracePartKind, TracePartSource, TraceTextChannel};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

fn test_static_tool_definition(
    name: &'static str,
    description: &'static str,
) -> crate::tool::StaticToolDefinition {
    crate::tool::StaticToolDefinition::new(crate::tool::ToolName::builtin(name), description)
}

fn test_route(endpoint: ProviderEndpoint, model: ModelInfo) -> crate::ResolvedModelRoute {
    crate::ResolvedModelRoute {
        role: crate::AgentRoleId::new("test").unwrap(),
        provider_id: crate::ProviderId::new("test").unwrap(),
        endpoint,
        model,
        effort: None,
    }
}

fn test_turn_engine_builder(endpoint: ProviderEndpoint, model: ModelInfo) -> TurnEngineBuilder {
    TurnEngineBuilder::from_route(&test_route(endpoint, model)).unwrap()
}

fn test_turn_engine() -> TurnEngine {
    test_turn_engine_builder(
        ProviderEndpoint::deepseek(None),
        ModelInfo::fallback("deepseek-v4-flash"),
    )
    .build()
}

fn terminal_tool_event_count(events: &[TraceEvent]) -> usize {
    events
        .iter()
        .filter(|event| match &event.kind {
            TraceEventKind::TracePartCompleted { item } => {
                item.kind() == pl_trace::TracePartKind::Tool && item.is_terminal()
            }
            TraceEventKind::TracePartFailed { item } => {
                item.kind() == pl_trace::TracePartKind::Tool && item.is_terminal()
            }
            TraceEventKind::TracePartStarted { .. }
            | TraceEventKind::TracePartDelta { .. }
            | TraceEventKind::InteractionChanged { .. }
            | TraceEventKind::SkillActivated { .. }
            | TraceEventKind::EnabledToolsRecorded { .. } => false,
        })
        .count()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TestToolPhase {
    Started,
    Streaming,
    AwaitingApproval,
    Approved,
    Running,
    Succeeded,
    Failed,
    Denied,
    Cancelled,
}

impl From<&pl_trace::TraceToolState> for TestToolPhase {
    fn from(state: &pl_trace::TraceToolState) -> Self {
        match state {
            pl_trace::TraceToolState::Started(_) => Self::Started,
            pl_trace::TraceToolState::Streaming(_) => Self::Streaming,
            pl_trace::TraceToolState::AwaitingApproval(_) => Self::AwaitingApproval,
            pl_trace::TraceToolState::Approved(_) => Self::Approved,
            pl_trace::TraceToolState::Running(_) => Self::Running,
            pl_trace::TraceToolState::Succeeded(_) => Self::Succeeded,
            pl_trace::TraceToolState::Failed(_) => Self::Failed,
            pl_trace::TraceToolState::Denied(_) => Self::Denied,
            pl_trace::TraceToolState::Cancelled(_) => Self::Cancelled,
        }
    }
}

fn tool_statuses(events: &[TraceEvent], item_id: &str) -> Vec<TestToolPhase> {
    events
        .iter()
        .filter_map(|event| match &event.kind {
            TraceEventKind::TracePartStarted { item }
            | TraceEventKind::TracePartCompleted { item }
            | TraceEventKind::TracePartFailed { item }
                if item.kind() == TracePartKind::Tool && item.item_id() == item_id =>
            {
                item.tool().map(|tool| TestToolPhase::from(tool.state()))
            }
            TraceEventKind::TracePartDelta { .. }
            | TraceEventKind::InteractionChanged { .. }
            | TraceEventKind::SkillActivated { .. }
            | TraceEventKind::EnabledToolsRecorded { .. }
            | TraceEventKind::TracePartStarted { .. }
            | TraceEventKind::TracePartCompleted { .. }
            | TraceEventKind::TracePartFailed { .. } => None,
        })
        .collect()
}

fn live_tool_result_deltas(events: &[AgentEvent], item_id: &str) -> Vec<String> {
    events
        .iter()
        .filter_map(|event| match event {
            AgentEvent::TracePartDelta { event }
                if event.kind() == TracePartKind::Tool && event.item_id == item_id =>
            {
                match &event.delta {
                    pl_trace::TraceDelta::ToolResult { delta } => Some(delta.clone()),
                    pl_trace::TraceDelta::Text { .. }
                    | pl_trace::TraceDelta::Thinking { .. }
                    | pl_trace::TraceDelta::ReasoningContent { .. }
                    | pl_trace::TraceDelta::ToolArguments { .. } => None,
                }
            }
            AgentEvent::TracePartStarted { .. }
            | AgentEvent::TracePartDelta { .. }
            | AgentEvent::TracePartCompleted { .. }
            | AgentEvent::TracePartFailed { .. }
            | AgentEvent::InteractionChanged { .. }
            | AgentEvent::AgentRuntimeUpdated { .. }
            | AgentEvent::SkillActivated { .. }
            | AgentEvent::TodoListUpdated { .. }
            | AgentEvent::TurnInterrupted { .. }
            | AgentEvent::TurnBudgetLimited { .. }
            | AgentEvent::Error { .. }
            | AgentEvent::Done => None,
        })
        .collect()
}

fn runtime_progress_texts(
    event_rx: &mut tokio::sync::broadcast::Receiver<AgentEvent>,
) -> Vec<String> {
    let mut progress_texts = Vec::new();
    while let Ok(event) = event_rx.try_recv() {
        match event {
            AgentEvent::TracePartCompleted { item }
                if item.source() == TracePartSource::Runtime
                    && item
                        .text()
                        .is_some_and(|text| text.channel() == TraceTextChannel::Commentary) =>
            {
                progress_texts.push(
                    item.text()
                        .expect("runtime commentary text")
                        .content()
                        .to_string(),
                )
            }
            AgentEvent::TracePartStarted { .. }
            | AgentEvent::TracePartDelta { .. }
            | AgentEvent::TracePartCompleted { .. }
            | AgentEvent::TracePartFailed { .. }
            | AgentEvent::InteractionChanged { .. }
            | AgentEvent::AgentRuntimeUpdated { .. }
            | AgentEvent::SkillActivated { .. }
            | AgentEvent::TodoListUpdated { .. }
            | AgentEvent::TurnInterrupted { .. }
            | AgentEvent::TurnBudgetLimited { .. }
            | AgentEvent::Error { .. }
            | AgentEvent::Done => {}
        }
    }
    progress_texts
}

mod approval;
mod default_tools;
mod errors;
mod progress_emitter;
mod run_turn;
mod tool_execution;

async fn serve_sse_once(sse_body: String) -> (String, tokio::task::JoinHandle<()>) {
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

async fn serve_sse_sequence(
    sse_bodies: Vec<String>,
) -> (
    String,
    std::sync::Arc<std::sync::Mutex<Vec<serde_json::Value>>>,
    tokio::task::JoinHandle<()>,
) {
    serve_http_sequence(sse_bodies.into_iter().map(TestHttpResponse::sse).collect()).await
}

async fn serve_sse_sequence_with_raw_requests(
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
    )
    .await;
    (base_url, raw_requests, handle)
}

struct TestHttpResponse {
    status: u16,
    content_type: &'static str,
    body: String,
}

impl TestHttpResponse {
    fn sse(body: String) -> Self {
        Self {
            status: 200,
            content_type: "text/event-stream",
            body,
        }
    }
}

async fn serve_http_sequence(
    responses: Vec<TestHttpResponse>,
) -> (
    String,
    std::sync::Arc<std::sync::Mutex<Vec<serde_json::Value>>>,
    tokio::task::JoinHandle<()>,
) {
    serve_http_sequence_capturing(responses, None).await
}

async fn serve_http_sequence_capturing(
    responses: Vec<TestHttpResponse>,
    raw_requests: Option<std::sync::Arc<std::sync::Mutex<Vec<Vec<u8>>>>>,
) -> (
    String,
    std::sync::Arc<std::sync::Mutex<Vec<serde_json::Value>>>,
    tokio::task::JoinHandle<()>,
) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let bodies = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let captured = bodies.clone();
    let handle = tokio::spawn(async move {
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
            let body = &buffer[header_end + 4..header_end + 4 + content_length];
            if let Some(raw_requests) = &raw_requests {
                raw_requests.lock().unwrap().push(body.to_vec());
            }
            captured
                .lock()
                .unwrap()
                .push(serde_json::from_slice(body).unwrap());

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

fn trace_started_kinds(events: &[TraceEvent]) -> Vec<TracePartKind> {
    events
        .iter()
        .filter_map(|event| match &event.kind {
            TraceEventKind::TracePartStarted { item } => Some(item.kind()),
            TraceEventKind::TracePartDelta { .. }
            | TraceEventKind::TracePartCompleted { .. }
            | TraceEventKind::TracePartFailed { .. }
            | TraceEventKind::InteractionChanged { .. }
            | TraceEventKind::SkillActivated { .. }
            | TraceEventKind::EnabledToolsRecorded { .. } => None,
        })
        .collect()
}

fn record_enabled_tools_for_core(
    core: &TurnEngine,
    session_id: &str,
    turn_id: &str,
) -> Vec<TraceEvent> {
    let tool_plan = core.acquire_tool_plan();
    let (event_tx, _event_rx) = tokio::sync::broadcast::channel(8);
    let mut recorder = TraceRecorder::new(session_id.to_string(), event_tx, 0);

    super::turn_loop::enabled_tools::record_enabled_tools(&mut recorder, turn_id, 0, &tool_plan);

    recorder.drain()
}

fn enabled_tools_event(events: &[TraceEvent]) -> &pl_trace::EnabledToolsEvent {
    events
        .iter()
        .find_map(|event| match &event.kind {
            TraceEventKind::EnabledToolsRecorded { event } => Some(event),
            TraceEventKind::TracePartStarted { .. }
            | TraceEventKind::TracePartDelta { .. }
            | TraceEventKind::TracePartCompleted { .. }
            | TraceEventKind::TracePartFailed { .. }
            | TraceEventKind::InteractionChanged { .. }
            | TraceEventKind::SkillActivated { .. } => None,
        })
        .expect("enabled tools event")
}

#[derive(Debug)]
struct SleepingTool;

impl StaticTool for SleepingTool {
    type Input = serde_json::Value;

    fn definition(&self) -> crate::tool::StaticToolDefinition {
        test_static_tool_definition("sleeping_tool", "Sleeps until the turn is cancelled")
    }

    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {},
            "additionalProperties": false
        })
    }

    fn policy(&self) -> ToolPolicy {
        ToolPolicy::default().with_parallel_tool_calls()
    }

    fn execute(
        &self,
        _input: Self::Input,
        _context: ToolCallContext,
    ) -> impl std::future::Future<Output = crate::Result<ToolResult>> + Send {
        async {
            tokio::time::sleep(std::time::Duration::from_secs(60)).await;
            Ok(ToolResult::success("done"))
        }
    }
}

#[derive(Debug)]
struct DeltaEchoTool;

impl StaticTool for DeltaEchoTool {
    type Input = serde_json::Value;

    fn definition(&self) -> crate::tool::StaticToolDefinition {
        test_static_tool_definition("delta_echo", "Echoes a trace delta before completing")
    }

    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {},
            "additionalProperties": false
        })
    }

    fn policy(&self) -> ToolPolicy {
        ToolPolicy::default()
    }

    fn execute(
        &self,
        _input: Self::Input,
        context: ToolCallContext,
    ) -> impl std::future::Future<Output = crate::Result<ToolResult>> + Send {
        async move {
            let now = crate::time::unix_seconds();
            let event = pl_trace::TracePartDeltaEvent {
                turn_id: context.identity().turn_id.clone(),
                item_id: context.identity().item_id.clone(),
                started_sequence: 0,
                revision: context.identity().revision_base.saturating_add(1),
                created_at: now,
                updated_at: now,
                delta: pl_trace::TraceDelta::ToolResult {
                    delta: "runtime delta".to_string(),
                },
            };
            let _ = context.events().send(AgentEvent::TracePartDelta { event });
            Ok(ToolResult::success("delta complete"))
        }
    }
}

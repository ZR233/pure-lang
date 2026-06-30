use super::*;
use crate::tool::{OutputTruncation, Tool, ToolInput, ToolOutput};
use crate::turn::{CompileMode, PermissionMode, ToolApprovalPolicy};
use crate::{ConfigStore, ModelRole};
use pl_model::ToolCall;
use pl_protocol::{InteractionPayload, InteractionResolution, ToolApprovalResolution};
use pl_trace::{TraceEventKind, TracePartKind, TracePartSource, TraceTextChannel};
use pretty_assertions::assert_eq;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

fn test_tool_context(event_tx: AgentEventSender) -> ToolContext {
    ToolContext {
        event_tx,
        options: TurnOptions::default(),
        workspace_access: WorkspaceAccess::WorkspaceOnly,
        mode: crate::turn::CompileMode::Auto,
        workspace_root: std::env::temp_dir(),
        workspace_instructions: None,
        instruction_snapshot: None,
        active_subagent: None,
        agent_control: crate::AgentControl::default(),
        lsp_runtime: None,
        parent_session: std::sync::Arc::new(CoreSession::new()),
    }
}

fn terminal_tool_event_count(events: &[TraceEvent]) -> usize {
    events
        .iter()
        .filter(|event| match &event.kind {
            TraceEventKind::TracePartCompleted { item } => {
                item.kind == pl_trace::TracePartKind::Tool
                    && item.status == TracePartStatus::Completed
            }
            TraceEventKind::TracePartFailed { item, .. } => {
                item.kind == pl_trace::TracePartKind::Tool
                    && matches!(
                        item.status,
                        TracePartStatus::Denied
                            | TracePartStatus::Failed
                            | TracePartStatus::Interrupted
                            | TracePartStatus::BudgetLimited
                    )
            }
            TraceEventKind::TracePartStarted { .. }
            | TraceEventKind::TracePartDelta { .. }
            | TraceEventKind::PlanLifecycleChanged { .. }
            | TraceEventKind::InteractionChanged { .. }
            | TraceEventKind::SkillActivated { .. }
            | TraceEventKind::EnabledToolsRecorded { .. } => false,
        })
        .count()
}

fn tool_statuses(events: &[TraceEvent], item_id: &str) -> Vec<TracePartStatus> {
    events
        .iter()
        .filter_map(|event| match &event.kind {
            TraceEventKind::TracePartStarted { item }
            | TraceEventKind::TracePartCompleted { item }
            | TraceEventKind::TracePartFailed { item, .. }
                if item.kind == TracePartKind::Tool && item.item_id == item_id =>
            {
                Some(item.status)
            }
            TraceEventKind::TracePartDelta { .. }
            | TraceEventKind::PlanLifecycleChanged { .. }
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
                if event.kind == TracePartKind::Tool && event.item_id == item_id =>
            {
                match &event.delta {
                    pl_trace::TraceDelta::ToolResult { delta } => Some(delta.clone()),
                    pl_trace::TraceDelta::Text { .. }
                    | pl_trace::TraceDelta::Thinking { .. }
                    | pl_trace::TraceDelta::ToolArguments { .. }
                    | pl_trace::TraceDelta::Plan { .. } => None,
                }
            }
            AgentEvent::TracePartStarted { .. }
            | AgentEvent::TracePartDelta { .. }
            | AgentEvent::TracePartCompleted { .. }
            | AgentEvent::TracePartFailed { .. }
            | AgentEvent::InteractionChanged { .. }
            | AgentEvent::AgentStateChanged { .. }
            | AgentEvent::AgentRuntimeUpdated { .. }
            | AgentEvent::SkillActivated { .. }
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
                if item.source == TracePartSource::Runtime
                    && item.text_channel == Some(TraceTextChannel::Commentary) =>
            {
                progress_texts.push(item.content)
            }
            AgentEvent::TracePartStarted { .. }
            | AgentEvent::TracePartDelta { .. }
            | AgentEvent::TracePartCompleted { .. }
            | AgentEvent::TracePartFailed { .. }
            | AgentEvent::InteractionChanged { .. }
            | AgentEvent::AgentStateChanged { .. }
            | AgentEvent::AgentRuntimeUpdated { .. }
            | AgentEvent::SkillActivated { .. }
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
            | AgentEvent::Error { .. }
            | AgentEvent::Done => {}
        }
    }
    progress_texts
}

#[test]
fn progress_emitter_sends_runtime_commentary_by_verbosity() {
    let (event_tx, mut event_rx) = tokio::sync::broadcast::channel(8);
    let mut progress =
        progress::ProgressEmitter::new(event_tx, "turn-1", progress::ProgressVerbosity::Normal);

    progress.milestone("正在准备上下文。");
    progress.heartbeat("等待模型响应。");
    progress.tool_detail("工具 `bash` 已完成。");

    let first = event_rx.try_recv().unwrap();
    assert!(event_rx.try_recv().is_err());

    let AgentEvent::TracePartCompleted { item: first } = first else {
        panic!("expected completed progress part");
    };
    assert_eq!(first.turn_id, "turn-1");
    assert_eq!(first.item_id, "turn-1:progress:1");
    assert_eq!(first.started_sequence, 1);
    assert_eq!(first.source, TracePartSource::Runtime);
    assert_eq!(first.text_channel, Some(TraceTextChannel::Commentary));
    assert_eq!(first.content, "正在准备上下文。");
}

#[test]
fn progress_emitter_sends_tool_detail_only_when_verbose() {
    let (normal_tx, mut normal_rx) = tokio::sync::broadcast::channel(8);
    let mut normal =
        progress::ProgressEmitter::new(normal_tx, "turn-1", progress::ProgressVerbosity::Normal);
    normal.tool_detail("工具 `bash` 已完成。");
    normal.tool_detail("工具结果已写入上下文，准备继续调用模型。");
    normal.tool_detail("模型请求调用 2 个工具。");
    assert!(normal_rx.try_recv().is_err());

    let (verbose_tx, mut verbose_rx) = tokio::sync::broadcast::channel(8);
    let mut verbose =
        progress::ProgressEmitter::new(verbose_tx, "turn-1", progress::ProgressVerbosity::Verbose);
    verbose.tool_detail("工具 `bash` 已完成。");
    verbose.tool_detail("工具结果已写入上下文，准备继续调用模型。");
    verbose.tool_detail("模型请求调用 2 个工具。");

    let AgentEvent::TracePartCompleted { item: first } = verbose_rx.try_recv().unwrap() else {
        panic!("expected completed progress part");
    };
    assert_eq!(first.source, TracePartSource::Runtime);
    assert_eq!(first.text_channel, Some(TraceTextChannel::Commentary));
    assert_eq!(first.content, "工具 `bash` 已完成。");

    let AgentEvent::TracePartCompleted { item: second } = verbose_rx.try_recv().unwrap() else {
        panic!("expected completed progress part");
    };
    assert_eq!(second.source, TracePartSource::Runtime);
    assert_eq!(second.text_channel, Some(TraceTextChannel::Commentary));
    assert_eq!(second.content, "工具结果已写入上下文，准备继续调用模型。");

    let AgentEvent::TracePartCompleted { item: third } = verbose_rx.try_recv().unwrap() else {
        panic!("expected completed progress part");
    };
    assert_eq!(third.source, TracePartSource::Runtime);
    assert_eq!(third.text_channel, Some(TraceTextChannel::Commentary));
    assert_eq!(third.content, "模型请求调用 2 个工具。");
}

#[test]
fn progress_emitter_scopes_item_ids_without_changing_turn_id() {
    let (event_tx, mut event_rx) = tokio::sync::broadcast::channel(8);
    let mut root_progress = progress::ProgressEmitter::new(
        event_tx.clone(),
        "turn-1",
        progress::ProgressVerbosity::Normal,
    );
    let mut tool_progress = progress::ProgressEmitter::new_scoped(
        event_tx,
        "turn-1",
        "turn-1:tool-progress",
        progress::ProgressVerbosity::Normal,
    );

    root_progress.milestone("准备上下文");
    tool_progress.milestone("执行工具");

    let first = event_rx.try_recv().unwrap();
    let second = event_rx.try_recv().unwrap();

    let AgentEvent::TracePartCompleted { item: first } = first else {
        panic!("expected completed progress part");
    };
    let AgentEvent::TracePartCompleted { item: second } = second else {
        panic!("expected completed progress part");
    };
    assert_eq!(first.turn_id, "turn-1");
    assert_eq!(first.item_id, "turn-1:progress:1");
    assert_eq!(second.turn_id, "turn-1");
    assert_eq!(second.item_id, "turn-1:tool-progress:1");
}

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

fn trace_started_kinds(events: &[TraceEvent]) -> Vec<TracePartKind> {
    events
        .iter()
        .filter_map(|event| match &event.kind {
            TraceEventKind::TracePartStarted { item } => Some(item.kind),
            TraceEventKind::TracePartDelta { .. }
            | TraceEventKind::TracePartCompleted { .. }
            | TraceEventKind::TracePartFailed { .. }
            | TraceEventKind::PlanLifecycleChanged { .. }
            | TraceEventKind::InteractionChanged { .. }
            | TraceEventKind::SkillActivated { .. }
            | TraceEventKind::EnabledToolsRecorded { .. } => None,
        })
        .collect()
}

fn record_enabled_tools_for_core(
    core: &PureCore,
    session_id: &str,
    turn_id: &str,
    mode: CompileMode,
) -> Vec<TraceEvent> {
    let tool_schemas = core
        .tools
        .schemas()
        .into_iter()
        .filter(|schema| tool_allowed_in_mode(mode, schema.name()))
        .collect::<Vec<_>>();
    let (event_tx, _event_rx) = tokio::sync::broadcast::channel(8);
    let mut recorder = TraceRecorder::new(session_id.to_string(), event_tx, 0);

    super::turn_loop::record_enabled_tools(&mut recorder, turn_id, mode, &tool_schemas);

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
            | TraceEventKind::PlanLifecycleChanged { .. }
            | TraceEventKind::InteractionChanged { .. }
            | TraceEventKind::SkillActivated { .. } => None,
        })
        .expect("enabled tools event")
}

#[derive(Debug)]
struct SleepingTool;

impl Tool for SleepingTool {
    fn name(&self) -> &str {
        "sleeping_tool"
    }

    fn description(&self) -> &str {
        "Sleeps until the turn is cancelled"
    }

    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {},
            "additionalProperties": false
        })
    }

    fn supports_parallel_tool_calls(&self) -> bool {
        true
    }

    fn execute<'a>(
        &'a self,
        _input: ToolInput,
        _context: ToolContext,
    ) -> std::pin::Pin<
        Box<
            dyn std::future::Future<Output = std::result::Result<ToolOutput, PureError>>
                + Send
                + 'a,
        >,
    > {
        Box::pin(async {
            tokio::time::sleep(std::time::Duration::from_secs(60)).await;
            Ok(ToolOutput {
                description: "done".to_string(),
                truncated: crate::tool::OutputTruncation::empty(),
                output_file: PathBuf::new(),
                exit_code: None,
                timed_out: false,
                runtime_events: Vec::new(),
            })
        })
    }
}

#[derive(Debug)]
struct DeltaEchoTool;

impl Tool for DeltaEchoTool {
    fn name(&self) -> &str {
        "delta_echo"
    }

    fn description(&self) -> &str {
        "Echoes a trace delta before completing"
    }

    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {},
            "additionalProperties": false
        })
    }

    fn execute<'a>(
        &'a self,
        input: ToolInput,
        context: ToolContext,
    ) -> std::pin::Pin<
        Box<
            dyn std::future::Future<Output = std::result::Result<ToolOutput, PureError>>
                + Send
                + 'a,
        >,
    > {
        Box::pin(async move {
            let now = crate::core::turn_result::unix_seconds();
            let event = pl_trace::TracePartDeltaEvent {
                turn_id: input.session_id.clone(),
                item_id: input.tool_id.clone(),
                started_sequence: 0,
                revision: input.revision_base.saturating_add(1),
                kind: TracePartKind::Tool,
                status: TracePartStatus::Running,
                created_at: now,
                updated_at: now,
                delta: pl_trace::TraceDelta::ToolResult {
                    delta: "runtime delta".to_string(),
                },
            };
            let _ = context.event_tx.send(AgentEvent::TracePartDelta { event });
            Ok(ToolOutput {
                description: "delta complete".to_string(),
                truncated: OutputTruncation::empty(),
                output_file: std::path::PathBuf::new(),
                exit_code: Some(0),
                timed_out: false,
                runtime_events: Vec::new(),
            })
        })
    }
}

#[test]
fn config_core_uses_planner_role_model_and_effort() {
    let config = ConfigStore::new(crate::ConfigPaths::from_home("unused"))
        .load_or_default()
        .unwrap();
    let core = PureCore::from_config(&config, ModelRole::Planner).unwrap();

    assert_eq!(core.provider.default_model(), "deepseek-v4-flash");
    assert_eq!(core.reasoning_effort.unwrap().as_str(), "high");
}

#[test]
fn detects_explicit_subagent_partition_requests() {
    assert!(prompt_requires_subagent_dispatch(
        "每个 crate 分一个 subagent 探索，然后介绍整个项目"
    ));
    assert!(prompt_requires_subagent_dispatch(
        "请分别用子代理探索前端和后端"
    ));
    assert!(!prompt_requires_subagent_dispatch("介绍整个项目"));
    assert!(!prompt_requires_subagent_dispatch(
        "用 bash 看一下每个 crate"
    ));
    assert!(!prompt_requires_subagent_dispatch(
        "读取 src/tool/subagent.rs，并总结每个模块的职责"
    ));
}

#[test]
fn subagent_dispatch_instructions_describe_recoverable_429() {
    assert!(SUBAGENT_DISPATCH_CONSTRAINT.contains("429"));
    assert!(SUBAGENT_DISPATCH_CONSTRAINT.contains("recoverableSubagentProvider429"));
    assert!(SUBAGENT_FORCE_DISPATCH_INSTRUCTION.contains("429"));
}

#[test]
fn detects_recoverable_subagent_tool_result_marker() {
    let records = vec![
        ToolExecutionRecord {
            id: "item-1".to_string(),
            call_id: Some("call-1".to_string()),
            name: "spawn_agent".to_string(),
            kind: ToolCallKind::Function,
            arguments: "{}".to_string(),
            result: "recoverableSubagentProvider429: retry locally".to_string(),
            display_result: "recoverableSubagentProvider429: retry locally".to_string(),
            status: TracePartStatus::Completed,
            exit_code: None,
            timed_out: false,
            revision: None,
            runtime_events: Vec::new(),
        },
        ToolExecutionRecord {
            id: "item-2".to_string(),
            call_id: Some("call-2".to_string()),
            name: "bash".to_string(),
            kind: ToolCallKind::Function,
            arguments: "{}".to_string(),
            result: "recoverableSubagentProvider429: unrelated text".to_string(),
            display_result: "recoverableSubagentProvider429: unrelated text".to_string(),
            status: TracePartStatus::Completed,
            exit_code: None,
            timed_out: false,
            revision: None,
            runtime_events: Vec::new(),
        },
    ];

    assert!(tool_results_include_recoverable_subagent_capacity(&records));
    assert!(!tool_results_include_recoverable_subagent_capacity(
        &records[1..]
    ));
}

#[test]
fn plan_mode_tool_allowlist_exposes_only_read_and_agent_tools() {
    let auto = crate::turn::CompileMode::Auto;
    let plan = crate::turn::CompileMode::Plan;

    assert!(tool_allowed_in_mode(auto, "write_file"));
    assert!(tool_allowed_in_mode(plan, "read_file"));
    assert!(tool_allowed_in_mode(plan, "search_files"));
    assert!(tool_allowed_in_mode(plan, "skills_list"));
    assert!(tool_allowed_in_mode(plan, "skill_view"));
    assert!(tool_allowed_in_mode(plan, "spawn_agent"));
    assert!(tool_allowed_in_mode(plan, "followup_task"));
    assert!(tool_allowed_in_mode(plan, "request_user_input"));
    assert!(tool_allowed_in_mode(plan, "bash"));
    assert!(tool_allowed_in_mode(plan, "lsp_query_rust"));
    assert!(tool_allowed_in_mode(plan, "mcp__github__search_issues"));
    assert!(!tool_allowed_in_mode(plan, "subagent"));
    assert!(!tool_allowed_in_mode(plan, "write_file"));
    assert!(!tool_allowed_in_mode(plan, "apply_patch"));
    assert!(!tool_allowed_in_mode(plan, "delete_path"));
    assert!(!tool_allowed_in_mode(plan, "skill_manage"));
}

#[test]
fn tool_trace_part_ids_are_scoped_to_turn() {
    assert_eq!(
        namespaced_tool_trace_part_id("turn-1", "call_0"),
        "turn-1-call_0"
    );
    assert_eq!(
        namespaced_tool_trace_part_id("turn-1", "turn-1-call_0"),
        "turn-1-call_0"
    );
}

#[tokio::test]
async fn tool_execution_reuses_streamed_trace_part() {
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let workspace_root = std::env::temp_dir().join(format!("pure-tool-reuse-{unique}"));
    tokio::fs::create_dir_all(&workspace_root).await.unwrap();
    tokio::fs::write(workspace_root.join("note.txt"), "provider item reuse")
        .await
        .unwrap();
    let mut core = PureCore::default_provider().unwrap();
    core.register_tool(ReadFileTool::new());
    let tool_call = ToolCall::function(
        "provider-item-1",
        "read_file",
        serde_json::json!({"path": "note.txt"}),
        Some("call-1".to_string()),
    );
    let (event_tx, mut event_rx) = tokio::sync::broadcast::channel(16);
    let mut recorder = TraceRecorder::new("session-1".to_string(), event_tx, 0);
    let streamed_item = recorder.tool_item(
        "turn-1",
        "turn-1-provider-item-1",
        "read_file".to_string(),
        "{\"path\":\"note.txt\"}".to_string(),
        Some("call-1".to_string()),
        Some("provider-item-1".to_string()),
    );
    recorder.start_item(streamed_item);
    let mut budget = BudgetTracker::new(crate::turn::TurnBudget::new(60_000));

    let records = execute_tool_calls(
        &[tool_call],
        &mut budget,
        &mut recorder,
        ToolExecutionContext {
            core: &core,
            options: &TurnOptions::default(),
            mode: crate::turn::CompileMode::Auto,
            session_id: "turn-1",
            workspace_root: &workspace_root,
            workspace_instructions: None,
            instruction_snapshot: None,
            active_subagent: None,
            agent_control: crate::AgentControl::default(),
            parent_session: std::sync::Arc::new(CoreSession::new()),
        },
    )
    .await
    .unwrap();
    let events = recorder.drain();
    let terminal_tool = events
        .iter()
        .find_map(|event| match &event.kind {
            TraceEventKind::TracePartCompleted { item }
                if item.kind == TracePartKind::Tool && item.item_id == "turn-1-provider-item-1" =>
            {
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
        })
        .expect("completed tool item");

    assert_eq!(records.len(), 1);
    assert_eq!(records[0].status, TracePartStatus::Completed);
    assert_eq!(terminal_tool.status, TracePartStatus::Completed);
    let tool = terminal_tool.tool.as_ref().expect("tool trace metadata");
    assert_eq!(tool.call_id.as_deref(), Some("call-1"));
    assert_eq!(tool.provider_item_id.as_deref(), Some("provider-item-1"));
    assert_eq!(tool.arguments, "{\"path\":\"note.txt\"}");
    assert_eq!(tool.result.as_deref(), Some("provider item reuse"));
    assert_eq!(
        tool_statuses(&events, "turn-1-provider-item-1"),
        vec![
            TracePartStatus::Started,
            TracePartStatus::Approved,
            TracePartStatus::Running,
            TracePartStatus::Completed,
        ]
    );
    assert!(runtime_progress_texts(&mut event_rx).is_empty());
    let _ = tokio::fs::remove_dir_all(workspace_root).await;
}

#[tokio::test]
async fn tool_execution_reuses_streamed_trace_part_when_provider_id_arrives_late() {
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let workspace_root = std::env::temp_dir().join(format!("pure-tool-late-provider-{unique}"));
    tokio::fs::create_dir_all(&workspace_root).await.unwrap();
    tokio::fs::write(workspace_root.join("note.txt"), "late provider id")
        .await
        .unwrap();
    let mut core = PureCore::default_provider().unwrap();
    core.register_tool(ReadFileTool::new());
    let tool_call = ToolCall::function(
        "provider-item-1",
        "read_file",
        serde_json::json!({"path": "note.txt"}),
        Some("call-1".to_string()),
    );
    let (event_tx, _) = tokio::sync::broadcast::channel(16);
    let mut recorder = TraceRecorder::new("session-1".to_string(), event_tx, 0);
    let streamed_item = recorder.tool_item(
        "turn-1",
        "turn-1-call-1",
        "read_file".to_string(),
        "{\"path\":\"note".to_string(),
        Some("call-1".to_string()),
        None,
    );
    recorder.start_item(streamed_item);
    let mut budget = BudgetTracker::new(crate::turn::TurnBudget::new(60_000));

    let records = execute_tool_calls(
        &[tool_call],
        &mut budget,
        &mut recorder,
        ToolExecutionContext {
            core: &core,
            options: &TurnOptions::default(),
            mode: crate::turn::CompileMode::Auto,
            session_id: "turn-1",
            workspace_root: &workspace_root,
            workspace_instructions: None,
            instruction_snapshot: None,
            active_subagent: None,
            agent_control: crate::AgentControl::default(),
            parent_session: std::sync::Arc::new(CoreSession::new()),
        },
    )
    .await
    .unwrap();
    let events = recorder.drain();
    let completed_tool_ids = events
        .iter()
        .filter_map(|event| match &event.kind {
            TraceEventKind::TracePartCompleted { item } if item.kind == TracePartKind::Tool => {
                Some(item.item_id.as_str())
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

    assert_eq!(records.len(), 1);
    assert_eq!(records[0].status, TracePartStatus::Completed);
    assert_eq!(completed_tool_ids, vec!["turn-1-call-1"]);
    let terminal_tool = events
        .iter()
        .find_map(|event| match &event.kind {
            TraceEventKind::TracePartCompleted { item } if item.item_id == "turn-1-call-1" => {
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
        })
        .expect("completed late-provider tool item");
    let tool = terminal_tool.tool.as_ref().expect("tool trace metadata");
    assert_eq!(tool.call_id.as_deref(), Some("call-1"));
    assert_eq!(tool.provider_item_id.as_deref(), Some("provider-item-1"));
    assert_eq!(tool.result.as_deref(), Some("late provider id"));
    assert_eq!(
        tool_statuses(&events, "turn-1-call-1"),
        vec![
            TracePartStatus::Started,
            TracePartStatus::Approved,
            TracePartStatus::Running,
            TracePartStatus::Completed,
        ]
    );
    assert!(tool_statuses(&events, "turn-1-provider-item-1").is_empty());
    let _ = tokio::fs::remove_dir_all(workspace_root).await;
}

#[tokio::test]
async fn tool_runtime_deltas_use_trace_part_id() {
    let mut core = PureCore::default_provider().unwrap();
    core.register_tool(DeltaEchoTool);
    let tool_call = ToolCall::function(
        "provider-item-1",
        "delta_echo",
        serde_json::json!({}),
        Some("call-1".to_string()),
    );
    let (event_tx, mut event_rx) = tokio::sync::broadcast::channel(16);
    let mut recorder = TraceRecorder::new("session-1".to_string(), event_tx, 0);
    let mut budget = BudgetTracker::new(crate::turn::TurnBudget::new(60_000));

    let records = execute_tool_calls(
        &[tool_call],
        &mut budget,
        &mut recorder,
        ToolExecutionContext {
            core: &core,
            options: &TurnOptions::default(),
            mode: crate::turn::CompileMode::Auto,
            session_id: "turn-1",
            workspace_root: &std::env::temp_dir(),
            workspace_instructions: None,
            instruction_snapshot: None,
            active_subagent: None,
            agent_control: crate::AgentControl::default(),
            parent_session: std::sync::Arc::new(CoreSession::new()),
        },
    )
    .await
    .unwrap();
    let mut live_events = Vec::new();
    while let Ok(event) = event_rx.try_recv() {
        live_events.push(event);
    }
    let events = recorder.drain();

    assert_eq!(records.len(), 1);
    assert_eq!(records[0].status, TracePartStatus::Completed);
    assert_eq!(
        live_tool_result_deltas(&live_events, "turn-1-provider-item-1"),
        vec!["runtime delta".to_string()]
    );
    assert_eq!(
        tool_statuses(&events, "turn-1-provider-item-1"),
        vec![
            TracePartStatus::Started,
            TracePartStatus::Approved,
            TracePartStatus::Running,
            TracePartStatus::Completed,
        ]
    );
}

#[test]
fn root_provider_429_is_transient_but_subagent_provider_429_stays_recoverable() {
    assert!(matches!(
        provider_error_severity(None, "API error 429 Too Many Requests"),
        ErrorSeverity::Transient
    ));

    let subagent = SubagentContext {
        id: "agent-1".to_string(),
        parent_id: None,
        agent_path: Some("/root/worker".to_string()),
        role: "executor".to_string(),
        task: "inspect worker".to_string(),
        depth: 1,
    };
    assert!(matches!(
        provider_error_severity(Some(&subagent), "API error 429 Too Many Requests"),
        ErrorSeverity::Recoverable
    ));
    assert!(matches!(
        provider_error_severity(None, "API error 500"),
        ErrorSeverity::Recoverable
    ));
}

#[test]
fn detects_unexecuted_tool_call_text() {
    assert!(looks_like_unexecuted_tool_call_text(
        "<｜｜DSML｜｜tool_calls>\n<｜｜DSML｜｜invoke name=\"spawn_agent\">"
    ));
    assert!(looks_like_unexecuted_tool_call_text(
        r#"{"tool_calls":[{"name":"spawn_agent"}]}"#
    ));
    assert!(!looks_like_unexecuted_tool_call_text(
        "源码中有 tool_calls 字段、name 字段和 subagent.rs 文件。"
    ));
    assert!(!looks_like_unexecuted_tool_call_text(
        r#"{"summary":"tool_calls and name are discussed in docs"}"#
    ));
    assert!(!looks_like_unexecuted_tool_call_text(
        "已完成探索，没有工具调用文本。"
    ));
}

#[test]
fn failed_turn_result_preserves_error_message() {
    let (event_tx, mut event_rx) = tokio::sync::broadcast::channel(8);
    let mut recorder = TraceRecorder::new("session-1".to_string(), event_tx, 0);

    let result = failed_turn_result(
        &mut recorder,
        "turn-1",
        crate::turn::CompileMode::Auto,
        "partial summary".to_string(),
        None,
        "model-a".to_string(),
        TokenUsage::default(),
        3,
        "provider rejected request".to_string(),
        ErrorSeverity::Transient,
    );

    assert_eq!(result.status, TurnResultStatus::Errored);
    assert_eq!(
        result.abort_reason,
        Some(crate::turn::TurnAbortReason::ProviderError),
    );
    assert_eq!(result.content, "partial summary");
    assert_eq!(result.error.as_deref(), Some("provider rejected request"));
    assert!(matches!(
        event_rx.try_recv().unwrap(),
        AgentEvent::TracePartStarted { item }
            if item.item_id == "turn-1-assistant"
                && item.text_channel == Some(TraceTextChannel::Final)
                && item.content == "partial summary"
    ));
    assert!(matches!(
        event_rx.try_recv().unwrap(),
        AgentEvent::TracePartCompleted { item, .. }
            if item.item_id == "turn-1-assistant"
                && item.text_channel == Some(TraceTextChannel::Final)
                && item.content == "partial summary"
    ));
    assert!(matches!(
        event_rx.try_recv().unwrap(),
        AgentEvent::TracePartFailed { item, .. } if item.item_id == "turn-1-turn"
    ));
    assert!(matches!(
        event_rx.try_recv().unwrap(),
        AgentEvent::Error {
            severity: ErrorSeverity::Transient,
            ..
        }
    ));
    assert!(matches!(event_rx.try_recv().unwrap(), AgentEvent::Done));
}

#[test]
fn default_turn_options_auto_allow_tools() {
    let options = TurnOptions::default();

    assert_eq!(options.tool_approval_policy, ToolApprovalPolicy::AutoAllow);
    assert_eq!(options.permission_mode, PermissionMode::RequestApproval);
    assert!(options.interaction_callback.is_none());
}

#[tokio::test]
async fn manual_tool_approval_can_approve_through_interaction() {
    let seen_interaction = std::sync::Arc::new(std::sync::Mutex::new(None));
    let seen_interaction_for_callback = seen_interaction.clone();
    let options = TurnOptions::new(ToolApprovalPolicy::Manual).with_interaction_callback(
        std::sync::Arc::new(move |interaction| {
            let seen_interaction = seen_interaction_for_callback.clone();
            Box::pin(async move {
                assert_eq!(interaction.kind, pl_protocol::InteractionKind::ToolApproval);
                match &interaction.payload {
                    InteractionPayload::ToolApproval { name, .. } => assert_eq!(name, "bash"),
                    other => panic!("unexpected payload: {other:?}"),
                }
                *seen_interaction.lock().unwrap() = Some(interaction);
                InteractionResolution::ToolApproval {
                    decision: ToolApprovalResolution::Approved,
                    reason: None,
                }
            })
        }),
    );
    let request = ToolApprovalRequest {
        id: "call-1".to_string(),
        name: "bash".to_string(),
        arguments: serde_json::json!({"command": "echo hi"}),
        working_directory: None,
        parent_agent_id: None,
    };
    let (event_tx, mut event_rx) = tokio::sync::broadcast::channel(8);
    let context = test_tool_context(event_tx.clone());

    let decision = approve_tool_call(&options, &request, &context).await;

    assert_eq!(decision, ToolApprovalDecision::Approved);
    assert!(event_rx.try_recv().is_err());
    let interaction = seen_interaction.lock().unwrap().clone().unwrap();
    assert_eq!(interaction.interaction_id, "call-1");
    assert_eq!(interaction.status, pl_protocol::InteractionStatus::Pending);
}

#[tokio::test]
async fn plan_mode_bash_requires_manual_approval_even_when_auto_allowed() {
    let options = TurnOptions::default();
    let request = ToolApprovalRequest {
        id: "call-1".to_string(),
        name: "bash".to_string(),
        arguments: serde_json::json!({"command": "pwd"}),
        working_directory: None,
        parent_agent_id: None,
    };
    let (event_tx, mut event_rx) = tokio::sync::broadcast::channel(8);
    let mut context = test_tool_context(event_tx.clone());
    context.mode = crate::turn::CompileMode::Plan;

    let decision = approve_tool_call(&options, &request, &context).await;

    assert_eq!(
        decision,
        ToolApprovalDecision::Denied {
            reason: "manual approval required but no interaction runtime is configured".to_string()
        }
    );
    assert!(event_rx.try_recv().is_err());
}

#[tokio::test]
async fn full_access_plan_bash_does_not_request_manual_approval() {
    let options = TurnOptions::default().with_permission_mode(PermissionMode::FullAccess);
    let request = ToolApprovalRequest {
        id: "call-1".to_string(),
        name: "bash".to_string(),
        arguments: serde_json::json!({"command": "pwd"}),
        working_directory: None,
        parent_agent_id: None,
    };
    let (event_tx, mut event_rx) = tokio::sync::broadcast::channel(8);
    let mut context = test_tool_context(event_tx.clone());
    context.mode = crate::turn::CompileMode::Plan;

    let decision = approve_tool_call(&options, &request, &context).await;

    assert_eq!(decision, ToolApprovalDecision::Approved);
    assert!(event_rx.try_recv().is_err());
}

#[tokio::test]
async fn plan_mode_read_tool_still_uses_auto_allow() {
    let options = TurnOptions::default();
    let request = ToolApprovalRequest {
        id: "call-1".to_string(),
        name: "read_file".to_string(),
        arguments: serde_json::json!({"path": "Cargo.toml"}),
        working_directory: None,
        parent_agent_id: None,
    };
    let (event_tx, mut event_rx) = tokio::sync::broadcast::channel(8);
    let mut context = test_tool_context(event_tx.clone());
    context.mode = crate::turn::CompileMode::Plan;

    let decision = approve_tool_call(&options, &request, &context).await;

    assert_eq!(decision, ToolApprovalDecision::Approved);
    assert!(event_rx.try_recv().is_err());
}

#[tokio::test]
async fn plan_mode_denies_disallowed_tool_before_execution_even_with_full_access() {
    let core = PureCore::default_provider().unwrap();
    let tool_call = ToolCall::function(
        "call-1",
        "write_file",
        serde_json::json!({"path": "a.txt", "content": "oops"}),
        None,
    );
    let options = TurnOptions::default().with_permission_mode(PermissionMode::FullAccess);
    let (event_tx, _event_rx) = tokio::sync::broadcast::channel(16);
    let mut recorder = TraceRecorder::new("session-1".to_string(), event_tx, 0);
    let mut budget = BudgetTracker::new(crate::turn::TurnBudget::new(60_000));
    let workspace_root = std::env::temp_dir();

    let records = execute_tool_calls(
        &[tool_call],
        &mut budget,
        &mut recorder,
        ToolExecutionContext {
            core: &core,
            options: &options,
            mode: crate::turn::CompileMode::Plan,
            session_id: "turn-1",
            workspace_root: &workspace_root,
            workspace_instructions: None,
            instruction_snapshot: None,
            active_subagent: None,
            agent_control: crate::AgentControl::default(),
            parent_session: std::sync::Arc::new(CoreSession::new()),
        },
    )
    .await
    .unwrap();

    assert_eq!(records.len(), 1);
    assert_eq!(records[0].status, TracePartStatus::Denied);
    assert_eq!(records[0].name, "write_file");
    assert_eq!(records[0].result, "Tool disabled in plan mode: write_file");
}

#[tokio::test]
async fn request_approval_allows_external_path_after_user_approval() {
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let workspace_root = std::env::temp_dir().join(format!("pure-permission-workspace-{unique}"));
    let outside_root = std::env::temp_dir().join(format!("pure-permission-outside-{unique}"));
    tokio::fs::create_dir_all(&workspace_root).await.unwrap();
    tokio::fs::create_dir_all(&outside_root).await.unwrap();
    let outside_file = outside_root.join("note.txt");
    tokio::fs::write(&outside_file, "external ok")
        .await
        .unwrap();
    let mut core = PureCore::default_provider().unwrap();
    core.register_tool(ReadFileTool::new());
    let tool_call = ToolCall::function(
        "call-1",
        "read_file",
        serde_json::json!({"path": outside_file.to_string_lossy()}),
        None,
    );
    let seen_interaction = std::sync::Arc::new(std::sync::Mutex::new(None));
    let seen_interaction_for_callback = seen_interaction.clone();
    let options =
        TurnOptions::default().with_interaction_callback(std::sync::Arc::new(move |interaction| {
            let seen_interaction = seen_interaction_for_callback.clone();
            Box::pin(async move {
                match &interaction.payload {
                    InteractionPayload::ToolApproval { name, .. } => {
                        assert_eq!(name, "read_file")
                    }
                    other => panic!("unexpected payload: {other:?}"),
                }
                *seen_interaction.lock().unwrap() = Some(interaction);
                InteractionResolution::ToolApproval {
                    decision: ToolApprovalResolution::Approved,
                    reason: None,
                }
            })
        }));
    let (event_tx, mut event_rx) = tokio::sync::broadcast::channel(16);
    let mut recorder = TraceRecorder::new("session-1".to_string(), event_tx, 0);
    let mut budget = BudgetTracker::new(crate::turn::TurnBudget::new(60_000));

    let records = execute_tool_calls(
        &[tool_call],
        &mut budget,
        &mut recorder,
        ToolExecutionContext {
            core: &core,
            options: &options,
            mode: crate::turn::CompileMode::Auto,
            session_id: "turn-1",
            workspace_root: &workspace_root,
            workspace_instructions: None,
            instruction_snapshot: None,
            active_subagent: None,
            agent_control: crate::AgentControl::default(),
            parent_session: std::sync::Arc::new(CoreSession::new()),
        },
    )
    .await
    .unwrap();

    assert_eq!(records.len(), 1);
    assert_eq!(records[0].status, TracePartStatus::Completed);
    assert!(records[0].result.contains("external ok"));
    assert!(seen_interaction.lock().unwrap().is_some());
    assert!(runtime_progress_texts(&mut event_rx).is_empty());
    let events = recorder.drain();
    assert_eq!(terminal_tool_event_count(&events), 1);
    assert_eq!(
        tool_statuses(&events, "turn-1-call-1"),
        vec![
            TracePartStatus::Started,
            TracePartStatus::AwaitingApproval,
            TracePartStatus::Approved,
            TracePartStatus::Running,
            TracePartStatus::Completed,
        ]
    );
    let _ = tokio::fs::remove_dir_all(workspace_root).await;
    let _ = tokio::fs::remove_dir_all(outside_root).await;
}

#[tokio::test]
async fn unknown_tool_records_one_terminal_event_and_tool_result() {
    let core = PureCore::default_provider().unwrap();
    let tool_call = ToolCall::function(
        "provider-item-1",
        "missing_tool",
        serde_json::json!({"value": 1}),
        Some("call-1".to_string()),
    );
    let (event_tx, _event_rx) = tokio::sync::broadcast::channel(16);
    let mut recorder = TraceRecorder::new("session-1".to_string(), event_tx, 0);
    let mut budget = BudgetTracker::new(crate::turn::TurnBudget::new(60_000));

    let records = execute_tool_calls(
        &[tool_call],
        &mut budget,
        &mut recorder,
        ToolExecutionContext {
            core: &core,
            options: &TurnOptions::default(),
            mode: crate::turn::CompileMode::Auto,
            session_id: "turn-1",
            workspace_root: &std::env::temp_dir(),
            workspace_instructions: None,
            instruction_snapshot: None,
            active_subagent: None,
            agent_control: crate::AgentControl::default(),
            parent_session: std::sync::Arc::new(CoreSession::new()),
        },
    )
    .await
    .unwrap();
    let events = recorder.drain();

    assert_eq!(records.len(), 1);
    assert_eq!(records[0].status, TracePartStatus::Failed);
    assert_eq!(records[0].id, "provider-item-1");
    assert_eq!(records[0].call_id.as_deref(), Some("call-1"));
    assert!(records[0].result.contains("Unknown tool: missing_tool"));
    assert_eq!(terminal_tool_event_count(&events), 1);
    assert_eq!(
        tool_statuses(&events, "turn-1-provider-item-1"),
        vec![
            TracePartStatus::Started,
            TracePartStatus::Failed,
            TracePartStatus::Failed,
        ]
    );
}

#[tokio::test]
async fn plan_disabled_tool_records_one_terminal_event_and_tool_result() {
    let mut core = PureCore::default_provider().unwrap();
    core.register_tool(WriteFileTool);
    let tool_call = ToolCall::function(
        "provider-item-1",
        "write_file",
        serde_json::json!({"path": "note.txt", "content": "nope"}),
        Some("call-1".to_string()),
    );
    let (event_tx, _event_rx) = tokio::sync::broadcast::channel(16);
    let mut recorder = TraceRecorder::new("session-1".to_string(), event_tx, 0);
    let mut budget = BudgetTracker::new(crate::turn::TurnBudget::new(60_000));

    let records = execute_tool_calls(
        &[tool_call],
        &mut budget,
        &mut recorder,
        ToolExecutionContext {
            core: &core,
            options: &TurnOptions::default(),
            mode: crate::turn::CompileMode::Plan,
            session_id: "turn-1",
            workspace_root: &std::env::temp_dir(),
            workspace_instructions: None,
            instruction_snapshot: None,
            active_subagent: None,
            agent_control: crate::AgentControl::default(),
            parent_session: std::sync::Arc::new(CoreSession::new()),
        },
    )
    .await
    .unwrap();
    let events = recorder.drain();

    assert_eq!(records.len(), 1);
    assert_eq!(records[0].status, TracePartStatus::Denied);
    assert!(
        records[0]
            .result
            .contains("Tool disabled in plan mode: write_file")
    );
    assert_eq!(terminal_tool_event_count(&events), 1);
    assert_eq!(
        tool_statuses(&events, "turn-1-provider-item-1"),
        vec![
            TracePartStatus::Started,
            TracePartStatus::Denied,
            TracePartStatus::Denied,
        ]
    );
    let terminal = events
        .iter()
        .find_map(|event| match &event.kind {
            TraceEventKind::TracePartCompleted { item } => Some(item),
            TraceEventKind::TracePartFailed { item, .. } => Some(item),
            TraceEventKind::TracePartStarted { .. }
            | TraceEventKind::TracePartDelta { .. }
            | TraceEventKind::PlanLifecycleChanged { .. }
            | TraceEventKind::InteractionChanged { .. }
            | TraceEventKind::SkillActivated { .. }
            | TraceEventKind::EnabledToolsRecorded { .. } => None,
        })
        .expect("terminal tool item");
    assert_eq!(
        terminal
            .tool
            .as_ref()
            .and_then(|tool| tool.denial_reason.as_deref()),
        Some("Tool disabled in plan mode: write_file")
    );
}

#[tokio::test]
async fn policy_denied_tool_records_one_terminal_event_and_tool_result() {
    let mut core = PureCore::default_provider().unwrap();
    core.register_tool(ReadFileTool::new());
    let tool_call = ToolCall::function(
        "provider-item-1",
        "read_file",
        serde_json::json!({"path": "note.txt"}),
        Some("call-1".to_string()),
    );
    let (event_tx, _event_rx) = tokio::sync::broadcast::channel(16);
    let mut recorder = TraceRecorder::new("session-1".to_string(), event_tx, 0);
    let mut budget = BudgetTracker::new(crate::turn::TurnBudget::new(60_000));

    let records = execute_tool_calls(
        &[tool_call],
        &mut budget,
        &mut recorder,
        ToolExecutionContext {
            core: &core,
            options: &TurnOptions::deny_all(),
            mode: crate::turn::CompileMode::Auto,
            session_id: "turn-1",
            workspace_root: &std::env::temp_dir(),
            workspace_instructions: None,
            instruction_snapshot: None,
            active_subagent: None,
            agent_control: crate::AgentControl::default(),
            parent_session: std::sync::Arc::new(CoreSession::new()),
        },
    )
    .await
    .unwrap();
    let events = recorder.drain();

    assert_eq!(records.len(), 1);
    assert_eq!(records[0].status, TracePartStatus::Denied);
    assert!(
        records[0]
            .result
            .contains("Tool execution denied: tool execution denied by policy")
    );
    assert_eq!(terminal_tool_event_count(&events), 1);
    assert_eq!(
        tool_statuses(&events, "turn-1-provider-item-1"),
        vec![
            TracePartStatus::Started,
            TracePartStatus::Denied,
            TracePartStatus::Denied,
        ]
    );
}

#[tokio::test]
async fn cancelling_running_tool_records_interrupted_terminal_event() {
    let mut core = PureCore::default_provider().unwrap();
    core.register_tool(SleepingTool);
    let tool_call = ToolCall::function(
        "provider-item-1",
        "sleeping_tool",
        serde_json::json!({}),
        Some("call-1".to_string()),
    );
    let token = tokio_util::sync::CancellationToken::new();
    let options = TurnOptions::default().with_cancellation(token.clone());
    let (event_tx, _event_rx) = tokio::sync::broadcast::channel(16);
    let mut recorder = TraceRecorder::new("session-1".to_string(), event_tx, 0);
    let mut budget = BudgetTracker::new(crate::turn::TurnBudget::new(60_000));
    let cancel_task = tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        token.cancel();
    });

    let records = execute_tool_calls(
        &[tool_call],
        &mut budget,
        &mut recorder,
        ToolExecutionContext {
            core: &core,
            options: &options,
            mode: crate::turn::CompileMode::Auto,
            session_id: "turn-1",
            workspace_root: &std::env::temp_dir(),
            workspace_instructions: None,
            instruction_snapshot: None,
            active_subagent: None,
            agent_control: crate::AgentControl::default(),
            parent_session: std::sync::Arc::new(CoreSession::new()),
        },
    )
    .await
    .unwrap();
    cancel_task.await.unwrap();
    let events = recorder.drain();

    assert_eq!(records.len(), 1);
    assert_eq!(records[0].status, TracePartStatus::Interrupted);
    assert_eq!(records[0].result, "Tool execution interrupted");
    assert_eq!(terminal_tool_event_count(&events), 1);
    assert_eq!(
        tool_statuses(&events, "turn-1-provider-item-1"),
        vec![
            TracePartStatus::Started,
            TracePartStatus::Approved,
            TracePartStatus::Running,
            TracePartStatus::Interrupted,
        ]
    );
    let terminal = events
        .iter()
        .find_map(|event| match &event.kind {
            TraceEventKind::TracePartFailed { item, .. } => Some(item),
            TraceEventKind::TracePartStarted { .. }
            | TraceEventKind::TracePartDelta { .. }
            | TraceEventKind::TracePartCompleted { .. }
            | TraceEventKind::PlanLifecycleChanged { .. }
            | TraceEventKind::InteractionChanged { .. }
            | TraceEventKind::SkillActivated { .. }
            | TraceEventKind::EnabledToolsRecorded { .. } => None,
        })
        .expect("interrupted tool item");
    assert_eq!(terminal.status, TracePartStatus::Interrupted);
}

#[tokio::test]
async fn deny_all_tool_approval_denies_without_request_event() {
    let options = TurnOptions::deny_all();
    let request = ToolApprovalRequest {
        id: "call-1".to_string(),
        name: "bash".to_string(),
        arguments: serde_json::json!({"command": "echo hi"}),
        working_directory: None,
        parent_agent_id: None,
    };
    let (event_tx, mut event_rx) = tokio::sync::broadcast::channel(8);
    let context = test_tool_context(event_tx.clone());

    let decision = approve_tool_call(&options, &request, &context).await;

    assert_eq!(
        decision,
        ToolApprovalDecision::Denied {
            reason: "tool execution denied by policy".to_string()
        }
    );
    assert!(event_rx.try_recv().is_err());
}

#[test]
fn approval_request_extracts_working_directory() {
    let call = ToolCall::function(
        "call-1",
        "bash",
        serde_json::json!({
            "command": "pwd",
            "workingDirectory": "C:/work"
        }),
        None,
    );

    let (event_tx, _event_rx) = tokio::sync::broadcast::channel(8);
    let request = approval_request(&call, &test_tool_context(event_tx));

    assert_eq!(request.working_directory.as_deref(), Some("C:/work"));
}

#[test]
fn approval_request_marks_parent_agent() {
    let call = ToolCall::function(
        "call-1",
        "bash",
        serde_json::json!({"command": "pwd"}),
        None,
    );
    let (event_tx, _event_rx) = tokio::sync::broadcast::channel(8);
    let mut context = test_tool_context(event_tx);
    context.active_subagent = Some(SubagentContext {
        id: "subagent-1".to_string(),
        parent_id: None,
        agent_path: None,
        role: "executor".to_string(),
        task: "inspect".to_string(),
        depth: 1,
    });

    let request = approval_request(&call, &context);

    assert_eq!(request.parent_agent_id.as_deref(), Some("subagent-1"));
}

#[tokio::test]
async fn default_tools_register_bash_and_agent_tools() {
    let mut core = PureCore::default_provider().unwrap();

    core.register_default_tools(std::env::temp_dir(), Some("rules".to_string()))
        .await;

    assert!(core.tools.get("bash").is_some());
    assert!(core.tools.get("write_stdin").is_some());
    assert!(core.tools.get("spawn_agent").is_some());
    assert!(core.tools.get("wait_agent").is_some());
    assert!(core.tools.get("list_agents").is_some());
    assert!(core.tools.get("request_user_input").is_some());
    assert!(core.tools.get("plan_exit").is_some());
    assert!(core.tools.get("subagent").is_none());
    assert!(core.tools.get("read_file").is_some());
    assert!(core.tools.get("apply_patch").is_some());
    assert!(core.tools.get("lsp_query").is_none());
}

#[tokio::test]
async fn default_tools_register_lsp_query_when_runtime_is_shared() {
    let registry = pl_lsp::LspRuntimeRegistry::new();
    let mut core = PureCore::default_provider()
        .unwrap()
        .with_lsp_runtime(registry.clone());

    core.register_default_tools(std::env::temp_dir(), Some("rules".to_string()))
        .await;

    // 空注册表没有可用语言，不应注册任何 LSP 工具。
    assert!(core.tools.get("lsp_query_rust").is_none());
    assert!(
        core.tools
            .names()
            .iter()
            .all(|name| !name.starts_with("lsp_query_"))
    );
}

#[tokio::test]
async fn enabled_tools_snapshot_records_mode_filtered_tools() {
    let mut core = PureCore::default_provider().unwrap();
    core.register_default_tools(std::env::temp_dir(), Some("rules".to_string()))
        .await;

    let events = record_enabled_tools_for_core(&core, "session-1", "turn-1", CompileMode::Plan);
    let event = enabled_tools_event(&events);

    assert_eq!(event.turn_id, "turn-1");
    assert_eq!(event.mode, "plan");
    assert!(event.tools.contains(&"bash".to_string()));
    assert!(event.tools.contains(&"read_file".to_string()));
    assert!(event.tools.contains(&"plan_exit".to_string()));
    assert!(!event.tools.contains(&"write_file".to_string()));
    assert!(!event.tools.contains(&"apply_patch".to_string()));
}

#[tokio::test]
async fn enabled_tools_snapshot_includes_lsp_query_when_runtime_is_shared() {
    let registry = pl_lsp::LspRuntimeRegistry::new();
    let mut core = PureCore::default_provider()
        .unwrap()
        .with_lsp_runtime(registry);
    core.register_default_tools(std::env::temp_dir(), Some("rules".to_string()))
        .await;

    let events = record_enabled_tools_for_core(&core, "session-1", "turn-1", CompileMode::Auto);
    let event = enabled_tools_event(&events);

    // 空注册表没有可用语言，不应出现任何 LSP 工具。
    assert!(event.tools.iter().all(|t| !t.starts_with("lsp_query_")));
    assert!(!event.tools.contains(&"plan_exit".to_string()));
}

#[tokio::test]
async fn run_turn_records_user_trace_part_before_internal_parts() {
    let sse_body = concat!(
        "data: {\"type\":\"response.output_item.added\",\"item\":{\"id\":\"msg_1\",\"type\":\"message\",\"role\":\"assistant\",\"phase\":\"final_answer\"}}\n\n",
        "data: {\"type\":\"response.output_text.delta\",\"item_id\":\"msg_1\",\"delta\":\"ok\"}\n\n",
        "data: {\"type\":\"response.output_item.done\",\"item\":{\"id\":\"msg_1\",\"type\":\"message\",\"role\":\"assistant\",\"phase\":\"final_answer\",\"content\":[{\"type\":\"output_text\",\"text\":\"ok\"}]}}\n\n",
        "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_1\",\"usage\":{\"input_tokens\":1,\"output_tokens\":2,\"total_tokens\":3}}}\n\n",
        "data: [DONE]\n\n"
    )
    .to_string();
    let (base_url, handle) = serve_sse_once(sse_body).await;
    let mut provider = ProviderInfo::openai(Some(base_url));
    provider.bearer_token = Some("test-token".to_string());
    provider.default_model = "local-responses".to_string();
    let core = PureCore::from_provider_info(provider).unwrap();
    let (event_tx, _event_rx) = tokio::sync::broadcast::channel(32);
    let mut recorder = TraceRecorder::new("session-1".to_string(), event_tx, 0);
    let mut session = CoreSession::new();

    let result = core
        .run_turn_with_trace(
            &mut session,
            TurnRequest::new("Build the thing".to_string(), CompileMode::Auto)
                .with_budget(crate::turn::TurnBudget::new(60_000)),
            &mut recorder,
            TurnOptions::default(),
        )
        .await
        .unwrap();
    handle.await.unwrap();

    assert_eq!(result.status, TurnResultStatus::Completed);
    let events = &result.trace_events;
    let started_kinds = trace_started_kinds(events);
    assert_eq!(started_kinds[0], TracePartKind::Text);
    assert_eq!(started_kinds[1], TracePartKind::Turn);
    assert_eq!(started_kinds[2], TracePartKind::Inference);

    let user_item = events
        .iter()
        .find_map(|event| match &event.kind {
            TraceEventKind::TracePartStarted { item }
                if item.kind == TracePartKind::Text
                    && item.text_channel == Some(TraceTextChannel::User) =>
            {
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
        })
        .expect("user trace part");
    assert_eq!(user_item.started_sequence, 0);
    assert_eq!(user_item.content, "Build the thing");
}

#[tokio::test]
async fn run_turn_emits_runtime_progress_commentary() {
    let sse_body = concat!(
        "data: {\"type\":\"response.output_item.added\",\"item\":{\"id\":\"msg_1\",\"type\":\"message\",\"role\":\"assistant\",\"phase\":\"final_answer\"}}\n\n",
        "data: {\"type\":\"response.output_text.delta\",\"item_id\":\"msg_1\",\"delta\":\"ok\"}\n\n",
        "data: {\"type\":\"response.output_item.done\",\"item\":{\"id\":\"msg_1\",\"type\":\"message\",\"role\":\"assistant\",\"phase\":\"final_answer\",\"content\":[{\"type\":\"output_text\",\"text\":\"ok\"}]}}\n\n",
        "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_1\",\"usage\":{\"input_tokens\":1,\"output_tokens\":2,\"total_tokens\":3}}}\n\n",
        "data: [DONE]\n\n"
    )
    .to_string();
    let (base_url, handle) = serve_sse_once(sse_body).await;
    let mut provider = ProviderInfo::openai(Some(base_url));
    provider.bearer_token = Some("test-token".to_string());
    provider.default_model = "local-responses".to_string();
    let core = PureCore::from_provider_info(provider).unwrap();
    let (event_tx, mut event_rx) = tokio::sync::broadcast::channel(64);
    let mut recorder = TraceRecorder::new("session-1".to_string(), event_tx, 0);
    let mut session = CoreSession::new();

    let result = core
        .run_turn_with_trace(
            &mut session,
            TurnRequest::new("Build the thing".to_string(), CompileMode::Auto)
                .with_budget(crate::turn::TurnBudget::new(60_000)),
            &mut recorder,
            TurnOptions::default(),
        )
        .await
        .unwrap();
    handle.await.unwrap();

    assert_eq!(result.status, TurnResultStatus::Completed);
    assert_eq!(
        runtime_progress_texts(&mut event_rx),
        vec![
            "已接收请求，正在准备上下文。".to_string(),
            "上下文已整理，准备调用模型。".to_string(),
            "模型已完成正文生成。".to_string(),
            "本轮已完成。".to_string(),
        ]
    );
}

#[tokio::test]
async fn run_turn_persists_only_final_text_to_session_history() {
    let sse_body = concat!(
        "data: {\"type\":\"response.output_item.added\",\"item\":{\"id\":\"msg_commentary\",\"type\":\"message\",\"role\":\"assistant\",\"phase\":\"commentary\"}}\n\n",
        "data: {\"type\":\"response.output_text.delta\",\"item_id\":\"msg_commentary\",\"delta\":\"正在检查。\"}\n\n",
        "data: {\"type\":\"response.output_item.done\",\"item\":{\"id\":\"msg_commentary\",\"type\":\"message\",\"role\":\"assistant\",\"phase\":\"commentary\",\"content\":[{\"type\":\"output_text\",\"text\":\"正在检查。\"}]}}\n\n",
        "data: {\"type\":\"response.output_item.added\",\"item\":{\"id\":\"msg_final\",\"type\":\"message\",\"role\":\"assistant\",\"phase\":\"final_answer\"}}\n\n",
        "data: {\"type\":\"response.output_text.delta\",\"item_id\":\"msg_final\",\"delta\":\"Done\"}\n\n",
        "data: {\"type\":\"response.output_item.done\",\"item\":{\"id\":\"msg_final\",\"type\":\"message\",\"role\":\"assistant\",\"phase\":\"final_answer\",\"content\":[{\"type\":\"output_text\",\"text\":\"Done\"}]}}\n\n",
        "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_1\",\"usage\":{\"input_tokens\":1,\"output_tokens\":2,\"total_tokens\":3}}}\n\n",
        "data: [DONE]\n\n"
    )
    .to_string();
    let (base_url, handle) = serve_sse_once(sse_body).await;
    let mut provider = ProviderInfo::openai(Some(base_url));
    provider.bearer_token = Some("test-token".to_string());
    provider.default_model = "local-responses".to_string();
    let core = PureCore::from_provider_info(provider).unwrap();
    let (event_tx, _event_rx) = tokio::sync::broadcast::channel(32);
    let mut recorder = TraceRecorder::new("session-1".to_string(), event_tx, 0);
    let mut session = CoreSession::new();

    let result = core
        .run_turn_with_trace(
            &mut session,
            TurnRequest::new("Build the thing".to_string(), CompileMode::Auto)
                .with_budget(crate::turn::TurnBudget::new(60_000)),
            &mut recorder,
            TurnOptions::default(),
        )
        .await
        .unwrap();
    handle.await.unwrap();

    assert_eq!(result.status, TurnResultStatus::Completed);
    assert_eq!(result.content, "Done");
    assert_eq!(session.messages().len(), 2);
    assert_eq!(session.messages()[1].role, MessageRole::Assistant);
    assert_eq!(
        session.messages()[1].content,
        MessageContent::Text("Done".to_string())
    );
}

#[tokio::test]
async fn enabled_tools_snapshot_remains_internal_trace_event() {
    let mut core = PureCore::default_provider().unwrap();
    core.register_default_tools(std::env::temp_dir(), Some("rules".to_string()))
        .await;
    let events = record_enabled_tools_for_core(&core, "session-1", "turn-1", CompileMode::Auto);
    let event = enabled_tools_event(&events);

    assert_eq!(event.turn_id, "turn-1");
    assert!(event.tools.contains(&"read_file".to_string()));
}

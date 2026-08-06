use super::*;
use crate::tool::ToolBudgetTiming;
use pretty_assertions::assert_eq;

#[derive(Debug)]
struct ProviderCallIdEchoTool;

impl Tool for ProviderCallIdEchoTool {
    fn name(&self) -> &str {
        "provider_call_id_echo"
    }

    fn description(&self) -> &str {
        "Returns the stable provider call identity from ToolContext"
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
        _input: ToolInput,
        context: ToolContext,
    ) -> std::pin::Pin<
        Box<
            dyn std::future::Future<Output = std::result::Result<ToolOutput, PureError>>
                + Send
                + 'a,
        >,
    > {
        Box::pin(async move {
            Ok(ToolOutput {
                description: context.provider_call_id.unwrap_or_default(),
                truncated: OutputTruncation::empty(),
                output_file: PathBuf::new(),
                exit_code: None,
                timed_out: false,
                runtime_events: Vec::new(),
            })
        })
    }
}

#[derive(Debug)]
struct BudgetPausedWaitTool;

impl Tool for BudgetPausedWaitTool {
    fn name(&self) -> &str {
        "wait_agents"
    }

    fn description(&self) -> &str {
        "Test-only blocking wait"
    }

    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {},
            "additionalProperties": false
        })
    }

    fn budget_timing(&self) -> ToolBudgetTiming {
        ToolBudgetTiming::PauseWhenOnlyScheduledTool
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
            tokio::time::sleep(std::time::Duration::from_millis(30)).await;
            Ok(ToolOutput {
                description: "progress".to_string(),
                truncated: OutputTruncation::empty(),
                output_file: PathBuf::new(),
                exit_code: Some(0),
                timed_out: false,
                runtime_events: Vec::new(),
            })
        })
    }
}

fn read_file_result_text(result: Option<&str>) -> String {
    serde_json::from_str::<serde_json::Value>(result.expect("tool result"))
        .expect("read_file json")
        .get("text")
        .and_then(serde_json::Value::as_str)
        .expect("text")
        .to_string()
}

#[tokio::test]
async fn invalid_function_arguments_are_returned_to_the_model_without_running_the_tool() {
    let core = TurnEngine::default_provider().unwrap();
    let tool_call = ToolCall::invalid_function(
        "call-1",
        "github_api_request",
        "{\"method\":\"POST\"\n\"path\":\"/repos/o/r/pulls/1/reviews\"}",
        "expected `,` or `}` at line 2 column 1",
        None,
    );
    let (event_tx, _) = tokio::sync::broadcast::channel(8);
    let mut recorder = TraceRecorder::new("session-1".to_string(), event_tx, 0);
    let mut budget = BudgetTracker::new(crate::turn::TurnBudget::new(60_000));

    let records = execute_tool_calls(
        &[tool_call],
        &mut budget,
        &mut recorder,
        ToolExecutionContext {
            core: &core,
            options: &TurnOptions::default(),
            session_id: "turn-1",
            workspace: crate::tool::AgentWorkspace::local(std::env::temp_dir()),
            workspace_instructions: None,
            instruction_snapshot: None,
            active_subagent: None,
            parent_session: std::sync::Arc::new(AgentSession::new()),
            working_set: crate::TurnWorkingSetHandle::default(),
            tool_cache: crate::TurnToolCacheHandle::default(),
        },
    )
    .await
    .unwrap();

    assert_eq!(records.len(), 1);
    assert_eq!(records[0].status, TracePartStatus::Failed);
    assert!(records[0].result.contains("Invalid JSON arguments"));
    assert!(records[0].result.contains("github_api_request"));
}

#[tokio::test]
async fn single_wait_agents_call_pauses_active_wall_clock_budget() {
    let mut core = TurnEngine::default_provider().unwrap();
    core.register_tool(BudgetPausedWaitTool);
    let tool_call = ToolCall::function("wait-1", "wait_agents", serde_json::json!({}), None);
    let (event_tx, _) = tokio::sync::broadcast::channel(8);
    let mut recorder = TraceRecorder::new("session-1".to_string(), event_tx, 0);
    let mut budget = BudgetTracker::new(crate::turn::TurnBudget::new(5));

    execute_tool_calls(
        &[tool_call],
        &mut budget,
        &mut recorder,
        ToolExecutionContext {
            core: &core,
            options: &TurnOptions::default(),
            session_id: "turn-1",
            workspace: crate::tool::AgentWorkspace::local(std::env::temp_dir()),
            workspace_instructions: None,
            instruction_snapshot: None,
            active_subagent: None,
            parent_session: std::sync::Arc::new(AgentSession::new()),
            working_set: crate::TurnWorkingSetHandle::default(),
            tool_cache: crate::TurnToolCacheHandle::default(),
        },
    )
    .await
    .unwrap();

    assert!(budget.check_wall_clock().is_ok());
    assert_eq!(budget.usage().wait_calls, 1);
}

#[tokio::test]
async fn mixed_tool_batch_keeps_wait_agents_time_in_active_budget() {
    let mut core = TurnEngine::default_provider().unwrap();
    core.register_tool(BudgetPausedWaitTool);
    core.register_tool(ProviderCallIdEchoTool);
    let calls = [
        ToolCall::function("wait-1", "wait_agents", serde_json::json!({}), None),
        ToolCall::function(
            "echo-1",
            "provider_call_id_echo",
            serde_json::json!({}),
            None,
        ),
    ];
    let (event_tx, _) = tokio::sync::broadcast::channel(8);
    let mut recorder = TraceRecorder::new("session-1".to_string(), event_tx, 0);
    let mut budget = BudgetTracker::new(crate::turn::TurnBudget::new(5));

    execute_tool_calls(
        &calls,
        &mut budget,
        &mut recorder,
        ToolExecutionContext {
            core: &core,
            options: &TurnOptions::default(),
            session_id: "turn-1",
            workspace: crate::tool::AgentWorkspace::local(std::env::temp_dir()),
            workspace_instructions: None,
            instruction_snapshot: None,
            active_subagent: None,
            parent_session: std::sync::Arc::new(AgentSession::new()),
            working_set: crate::TurnWorkingSetHandle::default(),
            tool_cache: crate::TurnToolCacheHandle::default(),
        },
    )
    .await
    .unwrap();

    assert!(budget.check_wall_clock().is_err());
    assert_eq!(budget.usage().wait_calls, 1);
}

#[tokio::test]
async fn tool_context_uses_item_id_when_provider_call_id_is_missing() {
    let mut core = TurnEngine::default_provider().unwrap();
    core.register_tool(ProviderCallIdEchoTool);
    let tool_call = ToolCall::function(
        "chat-tool-call-1",
        "provider_call_id_echo",
        serde_json::json!({}),
        None,
    );
    let (event_tx, _) = tokio::sync::broadcast::channel(8);
    let mut recorder = TraceRecorder::new("session-1".to_string(), event_tx, 0);
    let mut budget = BudgetTracker::new(crate::turn::TurnBudget::new(60_000));

    let records = execute_tool_calls(
        &[tool_call],
        &mut budget,
        &mut recorder,
        ToolExecutionContext {
            core: &core,
            options: &TurnOptions::default(),
            session_id: "turn-1",
            workspace: crate::tool::AgentWorkspace::local(std::env::temp_dir()),
            workspace_instructions: None,
            instruction_snapshot: None,
            active_subagent: None,
            parent_session: std::sync::Arc::new(AgentSession::new()),
            working_set: crate::TurnWorkingSetHandle::default(),
            tool_cache: crate::TurnToolCacheHandle::default(),
        },
    )
    .await
    .unwrap();

    assert_eq!(records.len(), 1);
    assert_eq!(records[0].result, "chat-tool-call-1");
    assert_eq!(records[0].call_id, None);
    let terminal_tool = recorder
        .drain()
        .into_iter()
        .find_map(|event| match event.kind {
            TraceEventKind::TracePartCompleted { item } => item.tool,
            TraceEventKind::TracePartStarted { .. }
            | TraceEventKind::TracePartDelta { .. }
            | TraceEventKind::TracePartFailed { .. }
            | TraceEventKind::PlanLifecycleChanged { .. }
            | TraceEventKind::InteractionChanged { .. }
            | TraceEventKind::SkillActivated { .. }
            | TraceEventKind::EnabledToolsRecorded { .. } => None,
        })
        .expect("terminal tool trace");
    assert_eq!(terminal_tool.call_id.as_deref(), Some("chat-tool-call-1"));
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
    let mut core = TurnEngine::default_provider().unwrap();
    core.register_tool(LocalWorkspaceFileTool::new(WorkspaceFileToolKind::ReadFile));
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
            session_id: "turn-1",
            workspace: crate::tool::AgentWorkspace::local(workspace_root.clone()),
            workspace_instructions: None,
            instruction_snapshot: None,
            active_subagent: None,
            parent_session: std::sync::Arc::new(AgentSession::new()),
            working_set: crate::TurnWorkingSetHandle::default(),
            tool_cache: crate::TurnToolCacheHandle::default(),
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
    assert_eq!(
        read_file_result_text(tool.result.as_deref()),
        "provider item reuse"
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
    let mut core = TurnEngine::default_provider().unwrap();
    core.register_tool(LocalWorkspaceFileTool::new(WorkspaceFileToolKind::ReadFile));
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
            session_id: "turn-1",
            workspace: crate::tool::AgentWorkspace::local(workspace_root.clone()),
            workspace_instructions: None,
            instruction_snapshot: None,
            active_subagent: None,
            parent_session: std::sync::Arc::new(AgentSession::new()),
            working_set: crate::TurnWorkingSetHandle::default(),
            tool_cache: crate::TurnToolCacheHandle::default(),
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
    assert_eq!(
        read_file_result_text(tool.result.as_deref()),
        "late provider id"
    );
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
    let mut core = TurnEngine::default_provider().unwrap();
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
            session_id: "turn-1",
            workspace: crate::tool::AgentWorkspace::local(std::env::temp_dir()),
            workspace_instructions: None,
            instruction_snapshot: None,
            active_subagent: None,
            parent_session: std::sync::Arc::new(AgentSession::new()),
            working_set: crate::TurnWorkingSetHandle::default(),
            tool_cache: crate::TurnToolCacheHandle::default(),
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

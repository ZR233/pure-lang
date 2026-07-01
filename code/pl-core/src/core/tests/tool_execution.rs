use super::*;
use pretty_assertions::assert_eq;

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
            agent_supervisor: crate::AgentSupervisor::default(),
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
            agent_supervisor: crate::AgentSupervisor::default(),
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
            agent_supervisor: crate::AgentSupervisor::default(),
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

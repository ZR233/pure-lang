use super::*;
use pretty_assertions::assert_eq;

#[test]
fn default_turn_options_request_approval_for_workspace_escape() {
    let options = TurnOptions::default();

    assert_eq!(options.permission_mode, PermissionMode::RequestApproval);
    assert!(options.interaction_callback.is_none());
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
    let mut core = TurnEngine::default_provider().unwrap();
    core.register_tool(LocalWorkspaceFileTool::new(WorkspaceFileToolKind::ReadFile));
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
    let core = TurnEngine::default_provider().unwrap();
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
async fn execution_policy_denied_tool_records_one_terminal_event_and_tool_result() {
    let mut core = TurnEngine::default_provider().unwrap();
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
    let options =
        TurnOptions::default().with_execution_policy(crate::AgentExecutionPolicy::default());

    let records = execute_tool_calls(
        &[tool_call],
        &mut budget,
        &mut recorder,
        ToolExecutionContext {
            core: &core,
            options: &options,
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
    let events = recorder.drain();

    assert_eq!(records.len(), 1);
    assert_eq!(records[0].status, TracePartStatus::Denied);
    assert!(
        records[0]
            .result
            .contains("Tool disabled by execution policy: write_file")
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
        Some("Tool disabled by execution policy: write_file")
    );
}

#[tokio::test]
async fn cancelling_running_tool_records_interrupted_terminal_event() {
    let mut core = TurnEngine::default_provider().unwrap();
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

#[test]
fn approval_request_extracts_working_directory() {
    let call = ToolCall::function(
        "call-1",
        "exec",
        serde_json::json!({
            "command": "pwd",
            "cwd": "C:/work"
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
        "exec",
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

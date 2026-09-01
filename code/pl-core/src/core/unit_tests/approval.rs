use super::*;
use futures::FutureExt;
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
    let mut core = test_turn_engine();
    core.register_test_tool(LocalWorkspaceFileTool::new(
        WorkspaceFileToolKind::ReadFile,
        crate::tool::ToolWorkspace::new(crate::tool::AgentWorkspace::local(workspace_root.clone())),
    ));
    let tool_call = ToolCall::function(
        "call-1",
        "read_file",
        serde_json::json!({"path": outside_file.to_string_lossy()}),
        "call-1",
    );
    let seen_interaction = std::sync::Arc::new(std::sync::Mutex::new(None));
    let seen_interaction_for_callback = seen_interaction.clone();
    let options =
        TurnOptions::default().with_interaction_callback(std::sync::Arc::new(move |interaction| {
            let seen_interaction = seen_interaction_for_callback.clone();
            async move {
                match &interaction.content {
                    InteractionContent::ToolApproval(approval) => {
                        assert_eq!(approval.request().name, "read_file")
                    }
                    other => panic!("unexpected payload: {other:?}"),
                }
                *seen_interaction.lock().unwrap() = Some(interaction);
                InteractionResolution::ToolApproval(ToolApprovalResolutionPayload {
                    decision: ToolApprovalResolution::Approved,
                    reason: None,
                })
            }
            .boxed()
        }));
    let (event_tx, mut event_rx) = tokio::sync::broadcast::channel(16);
    let mut recorder = TraceRecorder::new("session-1".to_string(), event_tx, 0);
    let mut budget = BudgetTracker::new(crate::turn::TurnBudget::new(
        std::time::Duration::from_millis(60_000),
    ));

    let records = execute_tool_calls(
        &[tool_call],
        &mut budget,
        &mut recorder,
        ToolExecutionContext {
            core: &core,
            tool_plan: core.acquire_tool_plan(),
            options: &options,
            session_id: "turn-1",
            turn_id: "turn-1",
            step: 0,
            workspace: crate::tool::AgentWorkspace::local(workspace_root.clone()),
            active_subagent: None,
            tool_cache: crate::tool::cache::TurnToolCacheHandle::default(),
        },
    )
    .await
    .unwrap();

    assert_eq!(records.len(), 1);
    assert_eq!(records[0].outcome, ToolExecutionOutcome::Succeeded);
    assert!(records[0].result.contains("external ok"));
    assert!(seen_interaction.lock().unwrap().is_some());
    assert!(runtime_progress_texts(&mut event_rx).is_empty());
    let events = recorder.drain();
    assert_eq!(terminal_tool_event_count(&events), 1);
    assert_eq!(
        tool_statuses(&events, "turn-1-call-1"),
        vec![
            TestToolPhase::Started,
            TestToolPhase::AwaitingApproval,
            TestToolPhase::Approved,
            TestToolPhase::Running,
            TestToolPhase::Succeeded,
        ]
    );
    let _ = tokio::fs::remove_dir_all(workspace_root).await;
    let _ = tokio::fs::remove_dir_all(outside_root).await;
}

#[tokio::test]
async fn workspace_tool_without_approval_skips_approved_trace_phase() {
    let workspace = tempfile::tempdir().unwrap();
    let mut core = test_turn_engine();
    core.register_test_tool(WriteFileTool::new(crate::tool::ToolWorkspace::new(
        crate::tool::AgentWorkspace::local(workspace.path().to_path_buf()),
    )));
    let tool_call = ToolCall::function(
        "provider-item-1",
        "write_file",
        serde_json::json!({
            "path": "note.txt",
            "content": "direct",
            "mode": "create",
        }),
        "call-1",
    );
    let (event_tx, _event_rx) = tokio::sync::broadcast::channel(16);
    let mut recorder = TraceRecorder::new("session-1".to_string(), event_tx, 0);
    let mut budget = BudgetTracker::new(crate::turn::TurnBudget::new(
        std::time::Duration::from_millis(60_000),
    ));

    let records = execute_tool_calls(
        &[tool_call],
        &mut budget,
        &mut recorder,
        ToolExecutionContext {
            core: &core,
            tool_plan: core.acquire_tool_plan(),
            options: &TurnOptions::default(),
            session_id: "turn-1",
            turn_id: "turn-1",
            step: 0,
            workspace: crate::tool::AgentWorkspace::local(workspace.path().to_path_buf()),
            active_subagent: None,
            tool_cache: crate::tool::cache::TurnToolCacheHandle::default(),
        },
    )
    .await
    .unwrap();
    let events = recorder.drain();

    assert_eq!(records.len(), 1);
    assert_eq!(records[0].outcome, ToolExecutionOutcome::Succeeded);
    assert_eq!(
        tool_statuses(&events, "turn-1-provider-item-1"),
        vec![
            TestToolPhase::Started,
            TestToolPhase::Running,
            TestToolPhase::Succeeded,
        ]
    );
}

#[tokio::test]
async fn unknown_tool_records_one_terminal_event_and_tool_result() {
    let core = test_turn_engine();
    let tool_call = ToolCall::function(
        "provider-item-1",
        "missing_tool",
        serde_json::json!({"value": 1}),
        "call-1",
    );
    let (event_tx, _event_rx) = tokio::sync::broadcast::channel(16);
    let mut recorder = TraceRecorder::new("session-1".to_string(), event_tx, 0);
    let mut budget = BudgetTracker::new(crate::turn::TurnBudget::new(
        std::time::Duration::from_millis(60_000),
    ));

    let records = execute_tool_calls(
        &[tool_call],
        &mut budget,
        &mut recorder,
        ToolExecutionContext {
            core: &core,
            tool_plan: core.acquire_tool_plan(),
            options: &TurnOptions::default(),
            session_id: "turn-1",
            turn_id: "turn-1",
            step: 0,
            workspace: crate::tool::AgentWorkspace::local(std::env::temp_dir()),
            active_subagent: None,
            tool_cache: crate::tool::cache::TurnToolCacheHandle::default(),
        },
    )
    .await
    .unwrap();
    let events = recorder.drain();

    assert_eq!(records.len(), 1);
    assert_eq!(
        records[0].outcome,
        ToolExecutionOutcome::Failed(pl_trace::TraceToolFailureKind::Execution),
    );
    assert_eq!(records[0].id, "provider-item-1");
    assert_eq!(records[0].call_id, "call-1");
    assert!(records[0].result.contains("Unknown tool: missing_tool"));
    assert_eq!(terminal_tool_event_count(&events), 1);
    assert_eq!(
        tool_statuses(&events, "turn-1-provider-item-1"),
        vec![TestToolPhase::Started, TestToolPhase::Failed,]
    );
}

#[tokio::test]
async fn execution_policy_denied_tool_records_one_terminal_event_and_tool_result() {
    let mut core = test_turn_engine();
    let tool_workspace = core.tool_workspace();
    core.register_test_tool(WriteFileTool::new(tool_workspace));
    let tool_call = ToolCall::function(
        "provider-item-1",
        "write_file",
        serde_json::json!({"path": "note.txt", "content": "nope"}),
        "call-1",
    );
    let (event_tx, _event_rx) = tokio::sync::broadcast::channel(16);
    let mut recorder = TraceRecorder::new("session-1".to_string(), event_tx, 0);
    let mut budget = BudgetTracker::new(crate::turn::TurnBudget::new(
        std::time::Duration::from_millis(60_000),
    ));
    let options =
        TurnOptions::default().with_execution_policy(crate::AgentExecutionPolicy::default());

    let records = execute_tool_calls(
        &[tool_call],
        &mut budget,
        &mut recorder,
        ToolExecutionContext {
            core: &core,
            tool_plan: core.acquire_tool_plan(),
            options: &options,
            session_id: "turn-1",
            turn_id: "turn-1",
            step: 0,
            workspace: crate::tool::AgentWorkspace::local(std::env::temp_dir()),
            active_subagent: None,
            tool_cache: crate::tool::cache::TurnToolCacheHandle::default(),
        },
    )
    .await
    .unwrap();
    let events = recorder.drain();

    assert_eq!(records.len(), 1);
    assert_eq!(records[0].outcome, ToolExecutionOutcome::Denied);
    assert!(
        records[0]
            .result
            .contains("Tool disabled by execution policy: write_file")
    );
    assert_eq!(terminal_tool_event_count(&events), 1);
    assert_eq!(
        tool_statuses(&events, "turn-1-provider-item-1"),
        vec![TestToolPhase::Started, TestToolPhase::Denied,]
    );
    let terminal = events
        .iter()
        .find_map(|event| match &event.kind {
            TraceEventKind::TracePartCompleted { item } => Some(item),
            TraceEventKind::TracePartFailed { item } => Some(item),
            TraceEventKind::TracePartStarted { .. }
            | TraceEventKind::TracePartDelta { .. }
            | TraceEventKind::InteractionChanged { .. }
            | TraceEventKind::SkillActivated { .. }
            | TraceEventKind::EnabledToolsRecorded { .. } => None,
        })
        .expect("terminal tool item");
    assert_eq!(
        terminal.tool().and_then(|tool| match tool.state() {
            pl_trace::TraceToolState::Denied(state) => Some(state.reason()),
            pl_trace::TraceToolState::Started(_)
            | pl_trace::TraceToolState::Streaming(_)
            | pl_trace::TraceToolState::AwaitingApproval(_)
            | pl_trace::TraceToolState::Approved(_)
            | pl_trace::TraceToolState::Running(_)
            | pl_trace::TraceToolState::Succeeded(_)
            | pl_trace::TraceToolState::Failed(_)
            | pl_trace::TraceToolState::Cancelled(_) => None,
        }),
        Some("Tool disabled by execution policy: write_file")
    );
}

#[tokio::test]
async fn cancelling_running_tool_records_interrupted_terminal_event() {
    let mut core = test_turn_engine();
    core.register_test_tool(SleepingTool);
    let tool_call = ToolCall::function(
        "provider-item-1",
        "sleeping_tool",
        serde_json::json!({}),
        "call-1",
    );
    let token = tokio_util::sync::CancellationToken::new();
    let options = TurnOptions::default().with_cancellation(token.clone());
    let (event_tx, _event_rx) = tokio::sync::broadcast::channel(16);
    let mut recorder = TraceRecorder::new("session-1".to_string(), event_tx, 0);
    let mut budget = BudgetTracker::new(crate::turn::TurnBudget::new(
        std::time::Duration::from_millis(60_000),
    ));
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
            tool_plan: core.acquire_tool_plan(),
            options: &options,
            session_id: "turn-1",
            turn_id: "turn-1",
            step: 0,
            workspace: crate::tool::AgentWorkspace::local(std::env::temp_dir()),
            active_subagent: None,
            tool_cache: crate::tool::cache::TurnToolCacheHandle::default(),
        },
    )
    .await
    .unwrap();
    cancel_task.await.unwrap();
    let events = recorder.drain();

    assert_eq!(records.len(), 1);
    assert_eq!(records[0].outcome, ToolExecutionOutcome::Cancelled);
    assert_eq!(records[0].result, "Tool execution interrupted");
    assert_eq!(terminal_tool_event_count(&events), 1);
    assert_eq!(
        tool_statuses(&events, "turn-1-provider-item-1"),
        vec![
            TestToolPhase::Started,
            TestToolPhase::Running,
            TestToolPhase::Cancelled,
        ]
    );
    let terminal = events
        .iter()
        .find_map(|event| match &event.kind {
            TraceEventKind::TracePartFailed { item } => Some(item),
            TraceEventKind::TracePartStarted { .. }
            | TraceEventKind::TracePartDelta { .. }
            | TraceEventKind::TracePartCompleted { .. }
            | TraceEventKind::InteractionChanged { .. }
            | TraceEventKind::SkillActivated { .. }
            | TraceEventKind::EnabledToolsRecorded { .. } => None,
        })
        .expect("interrupted tool item");
    assert!(matches!(
        terminal.tool().map(pl_trace::TraceToolPart::state),
        Some(pl_trace::TraceToolState::Cancelled(_)),
    ));
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
        "call-1",
    );

    let request = approval_request(&call, None);

    assert_eq!(request.working_directory.as_deref(), Some("C:/work"));
}

#[test]
fn approval_request_marks_parent_agent() {
    let call = ToolCall::function(
        "call-1",
        "exec",
        serde_json::json!({"command": "pwd"}),
        "call-1",
    );
    let active_subagent = SubagentContext {
        id: "subagent-1".to_string(),
        parent_id: None,
        agent_path: None,
        role: "executor".to_string(),
        task: "inspect".to_string(),
        depth: 1,
    };

    let request = approval_request(&call, Some(&active_subagent));

    assert_eq!(request.parent_agent_id.as_deref(), Some("subagent-1"));
}

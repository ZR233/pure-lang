use std::path::PathBuf;

use pl_protocol::{AgentStatus, PureError};
use tokio::sync::oneshot;
use tokio::time::{Duration, timeout};
use tokio_util::sync::CancellationToken;

use super::state::AgentEntry;
use super::*;
use crate::turn::{CompileMode, TurnBudget, TurnOptions};

fn agent_record(id: &str, status: AgentStatus) -> AgentRecord {
    AgentRecord {
        id: id.to_string(),
        path: format!("/root/{id}"),
        parent_path: Some(AgentPath::ROOT.to_string()),
        role: "executor".to_string(),
        task: format!("inspect {id}"),
        status,
        summary: None,
        error: None,
        reason: None,
        budget_limit_kind: None,
        budget_usage: None,
        depth: 1,
        updated_at: 1,
    }
}

async fn insert_agent(supervisor: &AgentSupervisor, record: AgentRecord) {
    let mut state = supervisor.state.lock().await;
    state
        .path_to_id
        .insert(record.path.clone(), record.id.clone());
    state
        .agents
        .insert(record.id.clone(), AgentEntry::new(record));
}

fn test_run_spec(message: &str) -> AgentRunSpec {
    let mut provider_info = pl_model::ProviderInfo::openai(Some("http://example.invalid".into()));
    provider_info.default_model = "test-model".to_string();
    AgentRunSpec {
        provider: pl_model::create_provider(provider_info).unwrap(),
        reasoning_effort: None,
        config: None,
        mcp_runtime: None,
        lsp_runtime: None,
        workspace_instructions: None,
        instruction_snapshot: None,
        tool_registrar: None,
        workspace_root: PathBuf::from("."),
        options: TurnOptions::default(),
        event_tx: tokio::sync::broadcast::channel(8).0,
        call_id: "call-1".to_string(),
        message: message.to_string(),
        mode: CompileMode::Auto,
        budget: TurnBudget::child_default(),
        initial_session: crate::CoreSession::new(),
    }
}

#[tokio::test]
async fn followup_capacity_failure_does_not_mutate_agent_mailbox_or_status() {
    let supervisor = AgentSupervisor::default();
    supervisor.configure_limits(1, 3).await;
    insert_agent(&supervisor, agent_record("worker", AgentStatus::Waiting)).await;
    let _active_guard = supervisor.reserve_agent_execution().unwrap();
    let (event_tx, _event_rx) = tokio::sync::broadcast::channel(8);

    let result = supervisor
        .send_message(AgentMessageRequest {
            current_path: AgentPath::ROOT,
            target: "/root/worker",
            message: "continue".to_string(),
            mode: AgentMessageMode::TriggerTurn,
            run_spec: Some(test_run_spec("continue")),
            event_tx: &event_tx,
            call_id: "call-1".to_string(),
        })
        .await;

    assert!(matches!(result, Err(PureError::AgentLimitReached { .. })));
    let state = supervisor.state.lock().await;
    let entry = state.agents.get("worker").unwrap();
    assert_eq!(entry.record.status, AgentStatus::Waiting);
    assert!(entry.mailbox.is_empty());
    assert!(entry.task.is_none());
}

#[tokio::test]
async fn close_agent_waits_for_cancelled_task_to_finish() {
    let supervisor = AgentSupervisor::default();
    let token = CancellationToken::new();
    let task_token = token.clone();
    let (done_tx, done_rx) = oneshot::channel();
    let handle = tokio::spawn(async move {
        task_token.cancelled().await;
        tokio::time::sleep(Duration::from_millis(25)).await;
        let _ = done_tx.send(());
    });
    let record = agent_record("worker", AgentStatus::Running);
    {
        let mut state = supervisor.state.lock().await;
        state
            .path_to_id
            .insert(record.path.clone(), record.id.clone());
        let mut entry = AgentEntry::new(record.clone());
        entry.cancellation_token = Some(token);
        entry.task = Some(handle);
        state.agents.insert(record.id.clone(), entry);
    }
    let (event_tx, _event_rx) = tokio::sync::broadcast::channel(8);

    supervisor
        .close_agent(
            AgentPath::ROOT,
            "/root/worker",
            "test close",
            &event_tx,
            "call-1".to_string(),
        )
        .await
        .unwrap();

    assert!(timeout(Duration::from_millis(10), done_rx).await.is_ok());
    let state = supervisor.state.lock().await;
    let entry = state.agents.get("worker").unwrap();
    assert_eq!(entry.record.status, AgentStatus::Shutdown);
    assert!(entry.task.is_none());
}

#[tokio::test]
async fn resume_agent_reactivates_shutdown_agent() {
    let supervisor = AgentSupervisor::default();
    let token = CancellationToken::new();
    let handle = tokio::spawn(async {});
    let mut record = agent_record("worker", AgentStatus::Shutdown);
    record.error = Some("provider returned status 429".to_string());
    record.reason = Some("closed for retry".to_string());
    {
        let mut state = supervisor.state.lock().await;
        state
            .path_to_id
            .insert(record.path.clone(), record.id.clone());
        let mut entry = AgentEntry::new(record.clone());
        entry.cancellation_token = Some(token);
        entry.task = Some(handle);
        state.agents.insert(record.id.clone(), entry);
    }
    let (event_tx, mut event_rx) = tokio::sync::broadcast::channel(8);

    let resumed = supervisor
        .resume_agent(AgentPath::ROOT, "/root/worker", &event_tx)
        .await
        .unwrap();

    assert_eq!(
        resumed,
        AgentRecord {
            status: AgentStatus::Waiting,
            error: None,
            reason: None,
            budget_limit_kind: None,
            budget_usage: None,
            updated_at: resumed.updated_at,
            ..record
        }
    );
    let state = supervisor.state.lock().await;
    let entry = state.agents.get("worker").unwrap();
    assert_eq!(entry.record, resumed);
    assert!(entry.cancellation_token.is_none());
    assert!(entry.task.is_none());
    drop(state);

    let event = event_rx.recv().await.unwrap();
    match event {
        pl_trace::AgentEvent::AgentStateChanged {
            id,
            path,
            parent_path,
            role,
            task,
            status,
            summary,
            depth,
            error,
            reason,
            budget_limit_kind,
            budget_usage,
            updated_at,
        } => {
            assert_eq!(
                AgentRecord {
                    id,
                    path,
                    parent_path,
                    role,
                    task,
                    status,
                    summary,
                    error,
                    reason,
                    budget_limit_kind,
                    budget_usage,
                    depth,
                    updated_at,
                },
                resumed
            );
        }
        _ => panic!("expected agent state change event"),
    }
}

#[tokio::test]
async fn resume_agent_rejects_active_agent() {
    let supervisor = AgentSupervisor::default();
    insert_agent(&supervisor, agent_record("worker", AgentStatus::Waiting)).await;
    let (event_tx, _event_rx) = tokio::sync::broadcast::channel(8);

    let result = supervisor
        .resume_agent(AgentPath::ROOT, "/root/worker", &event_tx)
        .await;

    assert!(matches!(
        result,
        Err(PureError::ToolExecutionFailed { tool, error })
            if tool == "resume_agent" && error == "target agent /root/worker is already waiting"
    ));
}

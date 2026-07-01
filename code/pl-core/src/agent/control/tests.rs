use super::*;
use pretty_assertions::assert_eq;

async fn spawn(control: &AgentControl, task_name: &str) -> AgentHandle {
    control
        .spawn_agent(AgentSpawnInput {
            task_name: task_name.to_string(),
            message: format!("inspect {task_name}"),
            role: "explorer".to_string(),
            parent_path: None,
        })
        .await
        .unwrap()
}

async fn spawn_child(control: &AgentControl, parent_path: &str, task_name: &str) -> AgentHandle {
    control
        .spawn_agent(AgentSpawnInput {
            task_name: task_name.to_string(),
            message: format!("inspect {task_name}"),
            role: "explorer".to_string(),
            parent_path: Some(parent_path.to_string()),
        })
        .await
        .unwrap()
}

async fn update_status(control: &AgentControl, agent_id: &str, status: AgentStatus) -> AgentRecord {
    control
        .update_status(agent_id, status, None, None)
        .await
        .unwrap()
}

async fn append_message(
    control: &AgentControl,
    target: &str,
    message: &str,
    mode: MessageDeliveryMode,
) -> Result<AgentRecord, PureError> {
    control
        .append_message(AgentPath::ROOT, target, message.to_string(), mode)
        .await
}

fn root_message(message: &str, trigger_turn: bool) -> AgentMailboxMessage {
    AgentMailboxMessage {
        sender_path: AgentPath::ROOT.to_string(),
        message: message.to_string(),
        trigger_turn,
    }
}

#[tokio::test]
async fn spawns_lists_and_rejects_duplicate_paths() {
    let control = AgentControl::default();
    let input = AgentSpawnInput {
        task_name: "worker".to_string(),
        message: "inspect".to_string(),
        role: "explorer".to_string(),
        parent_path: None,
    };
    let handle = control.spawn_agent(input.clone()).await.unwrap();
    assert_eq!(handle.path, "/root/worker");
    assert!(control.spawn_agent(input).await.is_err());
    assert_eq!(control.list_agents(None).await.len(), 1);
}

#[tokio::test]
async fn list_agents_path_prefix_matches_subtree_boundaries() {
    let control = AgentControl::default();
    let a = spawn(&control, "a").await;
    spawn_child(&control, &a.path, "child").await;
    spawn(&control, "ab").await;

    let agents = control.list_agents(Some("/root/a")).await;

    assert_eq!(
        agents
            .into_iter()
            .map(|agent| agent.path)
            .collect::<Vec<_>>(),
        vec!["/root/a".to_string(), "/root/a/child".to_string()]
    );
}

#[tokio::test]
async fn sends_and_closes_agent() {
    let control = AgentControl::default();
    let handle = spawn(&control, "worker").await;
    update_status(&control, &handle.id, AgentStatus::Interrupted).await;
    let record = append_message(
        &control,
        "worker",
        "follow up",
        MessageDeliveryMode::QueueOnly,
    )
    .await
    .unwrap();
    assert_eq!(record.status, AgentStatus::Waiting);
    let previous = control
        .close_agent(AgentPath::ROOT, "worker")
        .await
        .unwrap();
    assert_eq!(previous.status, AgentStatus::Waiting);
    assert_eq!(
        control.record(&previous.id).await.unwrap().status,
        AgentStatus::Shutdown
    );
    assert!(
        control
            .close_agent(AgentPath::ROOT, AgentPath::ROOT)
            .await
            .is_err()
    );
}

#[tokio::test]
async fn send_message_preserves_queued_or_running_status() {
    let control = AgentControl::default();
    let handle = spawn(&control, "worker").await;

    let queued = append_message(
        &control,
        "worker",
        "queued context",
        MessageDeliveryMode::QueueOnly,
    )
    .await
    .unwrap();
    update_status(&control, &handle.id, AgentStatus::Running).await;
    let running = append_message(
        &control,
        "worker",
        "running context",
        MessageDeliveryMode::QueueOnly,
    )
    .await
    .unwrap();

    assert_eq!(queued.status, AgentStatus::Queued);
    assert_eq!(running.status, AgentStatus::Running);
}

#[tokio::test]
async fn send_message_preserves_interruption_details_without_triggering_turn() {
    let control = AgentControl::default();
    let handle = spawn(&control, "worker").await;
    control
        .update_status_with(
            &handle.id,
            AgentStatusUpdate {
                status: AgentStatus::Interrupted,
                summary: None,
                error: Some("budget used".to_string()),
                reason: Some("budgetLimited".to_string()),
                budget_limit_kind: Some(BudgetLimitKind::ModelStep),
                budget_usage: Some(BudgetUsage {
                    model_steps: 1,
                    tool_calls: 0,
                    wait_calls: 0,
                    elapsed_ms: 10,
                }),
            },
        )
        .await
        .unwrap();

    let record = append_message(
        &control,
        "worker",
        "context",
        MessageDeliveryMode::QueueOnly,
    )
    .await
    .unwrap();

    assert_eq!(record.status, AgentStatus::Waiting);
    assert_eq!(record.error.as_deref(), Some("budget used"));
    assert_eq!(record.reason.as_deref(), Some("budgetLimited"));
    assert_eq!(record.budget_limit_kind, Some(BudgetLimitKind::ModelStep));
    assert!(record.budget_usage.is_some());
}

#[tokio::test]
async fn append_message_rejects_final_agent_statuses() {
    let control = AgentControl::default();
    let handle = spawn(&control, "worker").await;
    update_status(&control, &handle.id, AgentStatus::Completed).await;

    let error = append_message(
        &control,
        "worker",
        "follow up",
        MessageDeliveryMode::TriggerTurn,
    )
    .await
    .unwrap_err()
    .to_string();

    assert!(error.contains("followup_task"));
    assert!(error.contains("already completed"));
    assert_eq!(
        control.record(&handle.id).await.unwrap().status,
        AgentStatus::Completed
    );
}

#[tokio::test]
async fn followup_task_clears_stale_interruption_details() {
    let control = AgentControl::default();
    let handle = spawn(&control, "worker").await;
    control
        .update_status_with(
            &handle.id,
            AgentStatusUpdate {
                status: AgentStatus::Interrupted,
                summary: None,
                error: Some("budget used".to_string()),
                reason: Some("budgetLimited".to_string()),
                budget_limit_kind: Some(BudgetLimitKind::ModelStep),
                budget_usage: Some(BudgetUsage {
                    model_steps: 1,
                    tool_calls: 0,
                    wait_calls: 0,
                    elapsed_ms: 10,
                }),
            },
        )
        .await
        .unwrap();

    let queued = append_message(
        &control,
        "worker",
        "resume",
        MessageDeliveryMode::TriggerTurn,
    )
    .await
    .unwrap();
    assert_eq!(queued.status, AgentStatus::Queued);
    let running = update_status(&control, &handle.id, AgentStatus::Running).await;
    let completed = control
        .update_status(
            &handle.id,
            AgentStatus::Completed,
            Some("done".to_string()),
            None,
        )
        .await
        .unwrap();

    for record in [queued, running, completed] {
        assert_eq!(record.error, None);
        assert_eq!(record.reason, None);
        assert_eq!(record.budget_limit_kind, None);
        assert_eq!(record.budget_usage, None);
    }
}

#[tokio::test]
async fn take_turn_messages_drains_queued_messages_with_trigger() {
    let control = AgentControl::default();
    let handle = spawn(&control, "worker").await;
    update_status(&control, &handle.id, AgentStatus::Interrupted).await;
    append_message(
        &control,
        "worker",
        "context",
        MessageDeliveryMode::QueueOnly,
    )
    .await
    .unwrap();
    assert!(control.take_turn_messages(&handle.id).await.is_none());
    append_message(
        &control,
        "worker",
        "resume",
        MessageDeliveryMode::TriggerTurn,
    )
    .await
    .unwrap();
    append_message(
        &control,
        "worker",
        "late context",
        MessageDeliveryMode::QueueOnly,
    )
    .await
    .unwrap();

    let messages = control.take_turn_messages(&handle.id).await.unwrap();

    assert_eq!(
        messages,
        vec![
            root_message("context", false),
            root_message("resume", true),
            root_message("late context", false),
        ]
    );
    assert!(control.take_turn_messages(&handle.id).await.is_none());
}

#[tokio::test]
async fn followup_task_rejects_running_or_queued_agent() {
    let control = AgentControl::default();
    let handle = spawn(&control, "worker").await;
    let queued_error = append_message(
        &control,
        "worker",
        "queued follow up",
        MessageDeliveryMode::TriggerTurn,
    )
    .await
    .unwrap_err()
    .to_string();
    update_status(&control, &handle.id, AgentStatus::Running).await;
    let running_error = append_message(
        &control,
        "worker",
        "running follow up",
        MessageDeliveryMode::TriggerTurn,
    )
    .await
    .unwrap_err()
    .to_string();

    assert!(queued_error.contains("already queued"));
    assert!(running_error.contains("already running"));
}

#[tokio::test]
async fn wait_for_activity_returns_completed_snapshot_without_new_notify() {
    let control = AgentControl::default();
    let handle = spawn(&control, "worker").await;
    control
        .update_status(
            &handle.id,
            AgentStatus::Completed,
            Some("done".to_string()),
            None,
        )
        .await
        .unwrap();

    let outcome = control.wait_for_activity(250).await;

    assert!(!outcome.timed_out);
    assert_eq!(outcome.agents.len(), 1);
    assert_eq!(outcome.agents[0].status, AgentStatus::Completed);
    assert_eq!(outcome.agents[0].summary.as_deref(), Some("done"));
}

#[tokio::test]
async fn wait_for_activity_does_not_replay_old_final_agents() {
    let control = AgentControl::default();
    let first = spawn(&control, "first").await;
    let second = spawn(&control, "second").await;
    update_status(&control, &first.id, AgentStatus::Completed).await;
    update_status(&control, &second.id, AgentStatus::Running).await;
    assert!(!control.wait_for_activity(250).await.timed_out);

    let before = tokio::time::Instant::now();
    let outcome = control.wait_for_activity(250).await;

    assert!(outcome.timed_out);
    assert!(before.elapsed() >= std::time::Duration::from_millis(200));
}

#[tokio::test]
async fn spawn_reports_agent_budget_limits() {
    let control = AgentControl::default();
    control.configure_limits(0, 3).await;

    let error = control
        .spawn_agent(AgentSpawnInput {
            task_name: "worker".to_string(),
            message: "inspect".to_string(),
            role: "explorer".to_string(),
            parent_path: None,
        })
        .await
        .unwrap_err()
        .to_string();

    assert!(error.contains("agentCount"));
}

#[tokio::test]
async fn shutdown_descendants_marks_live_children() {
    let control = AgentControl::default();
    let parent = spawn(&control, "worker").await;
    let child = spawn_child(&control, &parent.path, "child").await;
    control
        .update_status_with(
            &child.id,
            AgentStatusUpdate {
                status: AgentStatus::Interrupted,
                summary: None,
                error: Some("needs cleanup".to_string()),
                reason: Some("budgetLimited".to_string()),
                budget_limit_kind: Some(BudgetLimitKind::WallClock),
                budget_usage: Some(BudgetUsage {
                    model_steps: 1,
                    tool_calls: 2,
                    wait_calls: 3,
                    elapsed_ms: 4,
                }),
            },
        )
        .await
        .unwrap();

    let records = control
        .shutdown_descendants(&parent.id, "budgetLimited")
        .await;

    assert_eq!(records.len(), 1);
    assert_eq!(records[0].id, child.id);
    assert_eq!(records[0].status, AgentStatus::Shutdown);
    assert_eq!(records[0].error.as_deref(), Some("needs cleanup"));
    assert_eq!(records[0].reason.as_deref(), Some("budgetLimited"));
    assert_eq!(
        records[0].budget_limit_kind,
        Some(BudgetLimitKind::WallClock)
    );
    assert!(records[0].budget_usage.is_some());
}

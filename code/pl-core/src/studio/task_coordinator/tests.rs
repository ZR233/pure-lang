use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use super::*;
use crate::tool::{SubagentContext, Tool, ToolRegistry};
use crate::{
    AgentKernel, AgentKernelToolRequest, AgentRunSpec, AgentSpawnInput, AgentSupervisor,
    CompileMode, CoreAgentProfile, PureCoreBuilder, StudioStore, ToolEffect, TurnBudget,
    TurnExecutionProfile, TurnOptions,
};

#[tokio::test]
async fn clean_committed_delivery_persists_exact_receipt_and_completes_records() {
    let fixture = DeliveryFixture::new("delivery-success", vec!["src/**"]).await;
    std::fs::create_dir_all(fixture.worktree.join("src")).unwrap();
    std::fs::write(
        fixture.worktree.join("src/lib.rs"),
        "pub fn delivered() {}\n",
    )
    .unwrap();
    git(&fixture.worktree, &["add", "src/lib.rs"]);
    git(&fixture.worktree, &["commit", "-m", "deliver"]);
    let head = git_output(&fixture.worktree, &["rev-parse", "HEAD"]);

    let delivery = fixture
        .coordinator
        .submit_delivery(
            &fixture.subagent,
            &fixture.worktree,
            &head,
            "cargo test passed",
        )
        .await
        .unwrap();

    assert_eq!(
        delivery,
        AgentDelivery {
            worktree: AgentWorktreeDelivery {
                path: std::fs::canonicalize(&fixture.worktree)
                    .unwrap()
                    .to_string_lossy()
                    .to_string(),
                branch: fixture.branch.clone(),
            },
            base_commit: fixture.base_commit.clone(),
            head_commit: head,
            changed_files: vec!["src/lib.rs".to_string()],
            verification_summary: "cargo test passed".to_string(),
        }
    );
    let outcome = fixture.outcome().await;
    let work_unit = fixture.work_unit().await;
    assert_eq!(outcome.status, AgentOutcomeStatus::Completed);
    assert_eq!(outcome.delivery, Some(delivery));
    assert_eq!(work_unit.status, WorkUnitStatus::Delivered);
    fixture.cleanup();
}

#[tokio::test]
async fn repeated_successful_delivery_does_not_reopen_completed_records() {
    let fixture = DeliveryFixture::new("repeat-success", vec!["src/**"]).await;
    fixture.commit_file("src/lib.rs");
    let head = git_output(&fixture.worktree, &["rev-parse", "HEAD"]);
    let delivered = fixture.submit(&head).await.unwrap();

    let error = fixture
        .submit(&head)
        .await
        .expect_err("completed delivery cannot be submitted again");

    assert!(error.to_string().contains("already finalized"));
    let outcome = fixture.outcome().await;
    assert_eq!(outcome.status, AgentOutcomeStatus::Completed);
    assert_eq!(outcome.error, None);
    assert_eq!(outcome.delivery, Some(delivered));
    assert_eq!(fixture.work_unit().await.status, WorkUnitStatus::Delivered);
    fixture.cleanup();
}

#[tokio::test]
async fn invalid_retry_after_success_does_not_downgrade_or_record_error() {
    let fixture = DeliveryFixture::new("invalid-retry-after-success", vec!["src/**"]).await;
    fixture.commit_file("src/lib.rs");
    let head = git_output(&fixture.worktree, &["rev-parse", "HEAD"]);
    let delivered = fixture.submit(&head).await.unwrap();
    std::fs::write(fixture.worktree.join("src/lib.rs"), "dirty retry\n").unwrap();

    let error = fixture
        .submit(&head)
        .await
        .expect_err("completed delivery cannot enter waiting state");

    assert!(error.to_string().contains("already finalized"));
    let outcome = fixture.outcome().await;
    assert_eq!(outcome.status, AgentOutcomeStatus::Completed);
    assert_eq!(outcome.error, None);
    assert_eq!(outcome.delivery, Some(delivered));
    assert_eq!(fixture.work_unit().await.status, WorkUnitStatus::Delivered);
    fixture.cleanup();
}

#[tokio::test]
async fn completed_executor_event_without_delivery_waits_for_delivery() {
    let fixture = DeliveryFixture::new("terminal-without-delivery", vec!["src/**"]).await;
    drain_agent_state(
        &fixture,
        pl_protocol::AgentStatus::Completed,
        Some("implementation finished"),
        None,
    )
    .await;

    fixture.assert_waiting().await;
    drain_agent_state(
        &fixture,
        pl_protocol::AgentStatus::Completed,
        Some("duplicate completion"),
        None,
    )
    .await;
    let outcome = fixture.outcome().await;
    assert_eq!(outcome.summary.as_deref(), Some("implementation finished"));
    assert_eq!(
        outcome.error.as_deref(),
        Some("executor completed without a successful delivery")
    );
    let agent = fixture
        .store
        .list_agents(&fixture.session_id)
        .await
        .unwrap()[0]
        .clone();
    assert_eq!(agent.status, pl_protocol::AgentStatus::Waiting);
    fixture.cleanup();
}

#[tokio::test]
async fn completed_event_preserves_successful_delivery_and_studio_snapshot() {
    let fixture = DeliveryFixture::new("terminal-after-delivery", vec!["src/**"]).await;
    fixture.commit_file("src/lib.rs");
    let head = git_output(&fixture.worktree, &["rev-parse", "HEAD"]);
    let delivery = fixture.submit(&head).await.unwrap();

    drain_agent_state(
        &fixture,
        pl_protocol::AgentStatus::Completed,
        Some("turn completed"),
        None,
    )
    .await;

    assert_eq!(
        fixture.outcome().await.status,
        AgentOutcomeStatus::Completed
    );
    assert_eq!(fixture.outcome().await.delivery, Some(delivery));
    assert_eq!(fixture.work_unit().await.status, WorkUnitStatus::Delivered);
    let agent = fixture
        .store
        .list_agents(&fixture.session_id)
        .await
        .unwrap()[0]
        .clone();
    assert_eq!(agent.status, pl_protocol::AgentStatus::Completed);
    assert_eq!(agent.summary.as_deref(), Some("cargo test passed"));
    fixture.cleanup();
}

#[tokio::test]
async fn successful_delivery_terminal_is_changed_once_then_idempotent() {
    let fixture = DeliveryFixture::new("delivered-terminal-one-shot", vec!["src/**"]).await;
    fixture.commit_file("src/lib.rs");
    let head = git_output(&fixture.worktree, &["rev-parse", "HEAD"]);
    fixture.submit(&head).await.unwrap();
    let change = crate::agent::AgentTerminalStateChange {
        agent_id: fixture.subagent.id.clone(),
        role: "executor".to_string(),
        status: pl_protocol::AgentStatus::Completed,
        summary: Some("turn completed".to_string()),
        error: None,
    };

    let first = fixture
        .coordinator
        .record_terminal_agent_state(&fixture.session_id, &change)
        .await
        .unwrap();
    let duplicate = fixture
        .coordinator
        .record_terminal_agent_state(&fixture.session_id, &change)
        .await
        .unwrap();

    assert!(matches!(
        first,
        TerminalAgentStateRecording::Changed { task_run_id, .. }
            if task_run_id == fixture.task_run_id
    ));
    assert!(matches!(
        duplicate,
        TerminalAgentStateRecording::Projected(_)
    ));
    fixture.cleanup();
}

#[tokio::test]
async fn successful_delivery_reopens_terminal_observation_after_waiting() {
    let fixture = DeliveryFixture::new("delivery-after-waiting-terminal", vec!["src/**"]).await;
    let change = crate::agent::AgentTerminalStateChange {
        agent_id: fixture.subagent.id.clone(),
        role: "executor".to_string(),
        status: pl_protocol::AgentStatus::Completed,
        summary: Some("turn completed".to_string()),
        error: None,
    };
    fixture
        .coordinator
        .record_terminal_agent_state(&fixture.session_id, &change)
        .await
        .unwrap();
    fixture.commit_file("src/lib.rs");
    let head = git_output(&fixture.worktree, &["rev-parse", "HEAD"]);
    fixture.submit(&head).await.unwrap();

    let delivered = fixture
        .coordinator
        .record_terminal_agent_state(&fixture.session_id, &change)
        .await
        .unwrap();
    let duplicate = fixture
        .coordinator
        .record_terminal_agent_state(&fixture.session_id, &change)
        .await
        .unwrap();

    assert!(matches!(
        delivered,
        TerminalAgentStateRecording::Changed { task_run_id, .. }
            if task_run_id == fixture.task_run_id
    ));
    assert!(matches!(
        duplicate,
        TerminalAgentStateRecording::Projected(_)
    ));
    fixture.cleanup();
}

#[tokio::test]
async fn errored_and_interrupted_events_persist_terminal_task_states() {
    for (name, agent_status, outcome_status, work_unit_status) in [
        (
            "terminal-error",
            pl_protocol::AgentStatus::Errored,
            AgentOutcomeStatus::Failed,
            WorkUnitStatus::Failed,
        ),
        (
            "terminal-interrupted",
            pl_protocol::AgentStatus::Interrupted,
            AgentOutcomeStatus::Cancelled,
            WorkUnitStatus::Cancelled,
        ),
    ] {
        let fixture = DeliveryFixture::new(name, vec!["src/**"]).await;
        drain_agent_state(
            &fixture,
            agent_status,
            Some("terminal summary"),
            Some("boom"),
        )
        .await;

        let outcome = fixture.outcome().await;
        assert_eq!(outcome.status, outcome_status);
        assert_eq!(outcome.summary.as_deref(), Some("terminal summary"));
        assert_eq!(outcome.error.as_deref(), Some("boom"));
        assert_eq!(fixture.work_unit().await.status, work_unit_status);
        fixture.cleanup();
    }
}

#[tokio::test]
async fn duplicate_terminal_event_does_not_reopen_or_duplicate_outcome() {
    let fixture = DeliveryFixture::new("duplicate-terminal", vec!["src/**"]).await;
    drain_agent_state(
        &fixture,
        pl_protocol::AgentStatus::Errored,
        Some("first"),
        Some("first error"),
    )
    .await;
    drain_agent_state(
        &fixture,
        pl_protocol::AgentStatus::Completed,
        Some("second"),
        None,
    )
    .await;

    let outcomes = fixture
        .store
        .list_agent_outcomes(&fixture.task_run_id)
        .await
        .unwrap();
    assert_eq!(outcomes.len(), 1);
    assert_eq!(outcomes[0].status, AgentOutcomeStatus::Failed);
    assert_eq!(outcomes[0].summary.as_deref(), Some("first"));
    assert_eq!(outcomes[0].error.as_deref(), Some("first error"));
    assert_eq!(fixture.work_unit().await.status, WorkUnitStatus::Failed);
    fixture.cleanup();
}

#[tokio::test]
async fn terminal_recording_reports_only_committed_durable_changes() {
    let fixture = DeliveryFixture::new("terminal-change-signal", vec!["src/**"]).await;
    let change = crate::agent::AgentTerminalStateChange {
        agent_id: fixture.subagent.id.clone(),
        role: "executor".to_string(),
        status: pl_protocol::AgentStatus::Errored,
        summary: Some("failed".to_string()),
        error: Some("boom".to_string()),
    };

    let changed = fixture
        .coordinator
        .record_terminal_agent_state(&fixture.session_id, &change)
        .await
        .unwrap();
    let duplicate = fixture
        .coordinator
        .record_terminal_agent_state(&fixture.session_id, &change)
        .await
        .unwrap();

    assert!(matches!(
        changed,
        TerminalAgentStateRecording::Changed {
            task_run_id,
            projection: _
        } if task_run_id == fixture.task_run_id
    ));
    assert!(matches!(
        duplicate,
        TerminalAgentStateRecording::Projected(_)
    ));
    fixture.cleanup();
}

#[tokio::test]
async fn delayed_terminal_event_cannot_update_new_task_run_at_reused_path() {
    let repository = init_repository("delayed-old-terminal");
    let store = task_store(&repository).await;
    let session = task_session(&store, &repository).await;
    let coordinator = Arc::new(TaskCoordinator::new(store.clone()));
    let old_run = coordinator
        .start_confirmed_task(&session.id, "old plan", &repository)
        .await
        .unwrap();
    let old_agent_id = spawned_agent_id(&session.id).await;
    let old_outcome = store
        .create_agent_outcome(CreateAgentOutcome {
            task_run_id: old_run.id.clone(),
            work_unit_id: None,
            agent_id: old_agent_id.clone(),
            owner_path: "/root".to_string(),
            initiated_by: "planner".to_string(),
            requested_by_call_id: "call-old".to_string(),
            role: "explorer".to_string(),
            status: AgentOutcomeStatus::Running,
            attempt: 1,
        })
        .await
        .unwrap();
    coordinator
        .finish_task(&old_run.id, TaskRunPhase::Cancelled, None)
        .await
        .unwrap();

    let new_run = coordinator
        .start_confirmed_task(&session.id, "new plan", &repository)
        .await
        .unwrap();
    let new_agent_id = spawned_agent_id(&session.id).await;
    let new_outcome = store
        .create_agent_outcome(CreateAgentOutcome {
            task_run_id: new_run.id.clone(),
            work_unit_id: None,
            agent_id: new_agent_id,
            owner_path: "/root".to_string(),
            initiated_by: "planner".to_string(),
            requested_by_call_id: "call-new".to_string(),
            role: "explorer".to_string(),
            status: AgentOutcomeStatus::Running,
            attempt: 1,
        })
        .await
        .unwrap();

    drain_studio_agent_events(
        store.clone(),
        &session.id,
        vec![
            agent_state_event(&old_agent_id, pl_protocol::AgentStatus::Completed),
            pl_trace::AgentEvent::SubAgentActivity {
                call_id: "call-old-terminal".to_string(),
                occurred_at: 11,
                agent_id: Some(old_agent_id.clone()),
                path: Some("/root/worker".to_string()),
                parent_path: Some("/root".to_string()),
                kind: pl_protocol::SubAgentActivityKind::Closed,
                status: Some(pl_protocol::AgentStatus::Completed),
                message: Some("memory completion".to_string()),
                timed_out: None,
                error: None,
            },
        ],
    )
    .await;

    let new_outcome = store
        .list_agent_outcomes(&new_run.id)
        .await
        .unwrap()
        .into_iter()
        .find(|outcome| outcome.id == new_outcome.id)
        .unwrap();
    let old_outcome = store
        .list_agent_outcomes(&old_run.id)
        .await
        .unwrap()
        .into_iter()
        .find(|outcome| outcome.id == old_outcome.id)
        .unwrap();
    let agent = store
        .list_agents(&session.id)
        .await
        .unwrap()
        .into_iter()
        .find(|agent| agent.id == old_agent_id)
        .unwrap();
    let activity: pl_protocol::StudioAgentTimelineEvent =
        serde_json::from_str(&store.list_agent_events(&session.id).await.unwrap()[0].payload_json)
            .unwrap();
    let pl_protocol::StudioAgentTimelineEventKind::SubAgentActivity {
        status: activity_status,
        ..
    } = activity.kind
    else {
        panic!("expected delayed subagent activity");
    };

    assert_eq!(new_outcome.status, AgentOutcomeStatus::Running);
    assert_eq!(old_outcome.status, AgentOutcomeStatus::Running);
    assert_eq!(agent.status, pl_protocol::AgentStatus::Running);
    assert_eq!(activity_status, Some(pl_protocol::AgentStatus::Running));
    coordinator
        .finish_task(&new_run.id, TaskRunPhase::Cancelled, None)
        .await
        .unwrap();
    drop(coordinator);
    remove_repository(repository);
}

#[tokio::test]
async fn simultaneous_sessions_keep_isolated_studio_agent_snapshots() {
    let repository = init_repository("cross-session-agent-ids");
    let store = task_store(&repository).await;
    let first_session = task_session(&store, &repository).await;
    let second_session = task_session(&store, &repository).await;
    let first_agent_id = spawned_agent_id(&first_session.id).await;
    let second_agent_id = spawned_agent_id(&second_session.id).await;

    drain_studio_agent_event(
        store.clone(),
        &first_session.id,
        agent_state_event(&first_agent_id, pl_protocol::AgentStatus::Running),
    )
    .await;
    drain_studio_agent_event(
        store.clone(),
        &second_session.id,
        agent_state_event(&second_agent_id, pl_protocol::AgentStatus::Running),
    )
    .await;

    let first_agents = store.list_agents(&first_session.id).await.unwrap();
    let second_agents = store.list_agents(&second_session.id).await.unwrap();
    assert_eq!(first_agents.len(), 1);
    assert_eq!(second_agents.len(), 1);
    assert_eq!(first_agents[0].id, first_agent_id);
    assert_eq!(second_agents[0].id, second_agent_id);
    assert_ne!(first_agents[0].id, second_agents[0].id);
    remove_repository(repository);
}

#[tokio::test]
async fn agent_changed_and_activity_share_durable_terminal_status() {
    for (name, outcome_status, memory_status, expected_status) in [
        (
            "activity-delivered",
            AgentOutcomeStatus::Completed,
            pl_protocol::AgentStatus::Errored,
            pl_protocol::AgentStatus::Completed,
        ),
        (
            "activity-waiting",
            AgentOutcomeStatus::WaitingForDelivery,
            pl_protocol::AgentStatus::Completed,
            pl_protocol::AgentStatus::Waiting,
        ),
        (
            "activity-failed",
            AgentOutcomeStatus::Failed,
            pl_protocol::AgentStatus::Completed,
            pl_protocol::AgentStatus::Errored,
        ),
        (
            "activity-cancelled",
            AgentOutcomeStatus::Cancelled,
            pl_protocol::AgentStatus::Completed,
            pl_protocol::AgentStatus::Interrupted,
        ),
    ] {
        let fixture = DeliveryFixture::new(name, vec!["src/**"]).await;
        if outcome_status == AgentOutcomeStatus::Completed {
            fixture.commit_file("src/lib.rs");
            let head = git_output(&fixture.worktree, &["rev-parse", "HEAD"]);
            fixture.submit(&head).await.unwrap();
        } else {
            fixture
                .store
                .update_agent_outcome(
                    &fixture.outcome_id,
                    UpdateAgentOutcome {
                        status: outcome_status,
                        summary: Some("durable summary".to_string()),
                        error: (outcome_status != AgentOutcomeStatus::WaitingForDelivery)
                            .then(|| "durable error".to_string()),
                        delivery: None,
                        review: None,
                    },
                )
                .await
                .unwrap();
        }
        let state_event = pl_trace::AgentEvent::AgentStateChanged {
            id: fixture.subagent.id.clone(),
            path: "/root/executor".to_string(),
            parent_path: Some("/root".to_string()),
            role: "executor".to_string(),
            task: "Implement delivery".to_string(),
            status: memory_status,
            summary: Some("memory summary".to_string()),
            depth: 1,
            error: None,
            reason: None,
            budget_limit_kind: None,
            budget_usage: None,
            updated_at: 10,
        };
        let activity_event = pl_trace::AgentEvent::SubAgentActivity {
            call_id: format!("call-{name}"),
            occurred_at: 11,
            agent_id: Some(fixture.subagent.id.clone()),
            path: Some("/root/executor".to_string()),
            parent_path: Some("/root".to_string()),
            kind: pl_protocol::SubAgentActivityKind::Closed,
            status: Some(memory_status),
            message: Some("memory activity".to_string()),
            timed_out: None,
            error: None,
        };

        drain_studio_agent_events(
            fixture.store.clone(),
            &fixture.session_id,
            vec![state_event, activity_event],
        )
        .await;

        let snapshot_status = fixture
            .store
            .list_agents(&fixture.session_id)
            .await
            .unwrap()[0]
            .status;
        let timeline = fixture
            .store
            .list_agent_events(&fixture.session_id)
            .await
            .unwrap();
        let event: pl_protocol::StudioAgentTimelineEvent =
            serde_json::from_str(&timeline[0].payload_json).unwrap();
        let pl_protocol::StudioAgentTimelineEventKind::SubAgentActivity {
            status: activity_status,
            ..
        } = event.kind
        else {
            panic!("expected subagent activity");
        };
        assert_eq!(snapshot_status, expected_status, "{name} snapshot");
        assert_eq!(activity_status, Some(expected_status), "{name} activity");
        fixture.cleanup();
    }
}

#[tokio::test]
async fn terminal_persistence_failure_blocks_task_and_suppresses_memory_snapshot() {
    let fixture = DeliveryFixture::new("terminal-store-failure", vec!["src/**"]).await;
    fixture
        .store
        .execute_test_sql("ALTER TABLE agent_outcomes RENAME TO unavailable_agent_outcomes")
        .await;
    let config_store = crate::config::ConfigStore::new(crate::config::ConfigPaths::from_home(
        std::env::temp_dir().join("pure-terminal-store-failure"),
    ));
    let runtime = crate::studio::StudioRuntime::new(fixture.store.clone(), config_store);
    let mut studio_events = runtime.events().subscribe();
    let (event_tx, event_rx) = tokio::sync::broadcast::channel(8);
    let drain = {
        let runtime = runtime.clone();
        let session_id = fixture.session_id.clone();
        tokio::spawn(async move {
            runtime.drain_agent_events(session_id, event_rx).await;
        })
    };
    let completed_event = pl_trace::AgentEvent::AgentStateChanged {
        id: fixture.subagent.id.clone(),
        path: "/root/executor".to_string(),
        parent_path: Some("/root".to_string()),
        role: "executor".to_string(),
        task: "Implement delivery".to_string(),
        status: pl_protocol::AgentStatus::Completed,
        summary: Some("memory completion".to_string()),
        depth: 1,
        error: None,
        reason: None,
        budget_limit_kind: None,
        budget_usage: None,
        updated_at: 10,
    };
    event_tx.send(completed_event.clone()).unwrap();
    drop(event_tx);
    drain.await.unwrap();

    let run = fixture
        .store
        .read_task_run(&fixture.task_run_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(run.phase, TaskRunPhase::Blocked);
    assert!(!fixture.coordinator.process_lease_is_held(&run));
    assert!(
        run.status_message
            .as_deref()
            .unwrap_or_default()
            .contains("terminal agent state persistence failed")
    );
    assert!(
        fixture
            .store
            .list_agents(&fixture.session_id)
            .await
            .unwrap()
            .is_empty()
    );
    let diagnostic = tokio::time::timeout(std::time::Duration::from_secs(1), async {
        loop {
            let event = studio_events.recv().await.unwrap();
            if let pl_protocol::StudioEventKind::TurnChanged { turn } = event.kind
                && turn.reason.as_deref().is_some_and(|reason| {
                    reason.contains("terminal agent state persistence failed")
                })
            {
                break;
            }
        }
    })
    .await;
    assert!(diagnostic.is_ok());
    fixture
        .store
        .execute_test_sql("ALTER TABLE unavailable_agent_outcomes RENAME TO agent_outcomes")
        .await;

    drain_studio_agent_event(fixture.store.clone(), &fixture.session_id, completed_event).await;

    assert!(
        fixture
            .store
            .list_agents(&fixture.session_id)
            .await
            .unwrap()
            .is_empty(),
        "duplicate terminal event after a persistence block must stay suppressed"
    );
    assert_eq!(fixture.outcome().await.status, AgentOutcomeStatus::Running);
    fixture.cleanup();
}

#[tokio::test]
async fn dirty_tracked_and_untracked_deliveries_wait_for_retry() {
    for (name, untracked) in [("dirty-tracked", false), ("dirty-untracked", true)] {
        let fixture = DeliveryFixture::new(name, vec!["src/**"]).await;
        std::fs::create_dir_all(fixture.worktree.join("src")).unwrap();
        std::fs::write(fixture.worktree.join("src/lib.rs"), "committed\n").unwrap();
        git(&fixture.worktree, &["add", "src/lib.rs"]);
        git(&fixture.worktree, &["commit", "-m", "deliver"]);
        let head = git_output(&fixture.worktree, &["rev-parse", "HEAD"]);
        let dirty_path = if untracked {
            "src/untracked.rs"
        } else {
            "src/lib.rs"
        };
        std::fs::write(fixture.worktree.join(dirty_path), "dirty\n").unwrap();

        let error = fixture.submit(&head).await.expect_err("dirty delivery");

        assert!(error.to_string().contains("clean working tree"));
        fixture.assert_waiting().await;
        fixture.cleanup();
    }
}

#[tokio::test]
async fn invalid_head_and_scope_deliveries_wait_for_retry() {
    let unchanged = DeliveryFixture::new("unchanged-head", vec!["src/**"]).await;
    let error = unchanged
        .submit(&unchanged.base_commit)
        .await
        .expect_err("unchanged HEAD");
    assert!(error.to_string().contains("must advance beyond base"));
    unchanged.assert_waiting().await;
    unchanged.cleanup();

    let mismatch = DeliveryFixture::new("head-mismatch", vec!["src/**"]).await;
    mismatch.commit_file("src/lib.rs");
    let error = mismatch
        .submit("0000000000000000000000000000000000000000")
        .await
        .expect_err("supplied HEAD mismatch");
    assert!(error.to_string().contains("does not match worktree HEAD"));
    mismatch.assert_waiting().await;
    mismatch.cleanup();

    let out_of_scope = DeliveryFixture::new("out-of-scope", vec!["src/**"]).await;
    out_of_scope.commit_file("design/notes.md");
    let head = git_output(&out_of_scope.worktree, &["rev-parse", "HEAD"]);
    let error = out_of_scope
        .submit(&head)
        .await
        .expect_err("out-of-scope delivery");
    assert!(error.to_string().contains("outside ownedPaths"));
    out_of_scope.assert_waiting().await;
    out_of_scope.cleanup();
}

#[tokio::test]
async fn delivery_head_must_descend_from_the_assigned_base() {
    let fixture = DeliveryFixture::new("diverged-head", vec!["src/**"]).await;
    std::fs::remove_file(fixture.worktree.join("README.md")).unwrap();
    std::fs::create_dir_all(fixture.worktree.join("src")).unwrap();
    std::fs::write(fixture.worktree.join("src/lib.rs"), "diverged\n").unwrap();
    git(&fixture.worktree, &["add", "-A"]);
    let tree = git_output(&fixture.worktree, &["write-tree"]);
    let head = git_output(
        &fixture.worktree,
        &["commit-tree", &tree, "-m", "diverged delivery"],
    );
    git(&fixture.worktree, &["reset", "--hard", &head]);

    let error = fixture
        .submit(&head)
        .await
        .expect_err("delivery must descend from base");

    assert!(error.to_string().contains("descend from base"));
    fixture.assert_waiting().await;
    fixture.cleanup();
}

#[tokio::test]
async fn delivery_uses_work_unit_base_after_task_expected_head_advances() {
    let fixture = DeliveryFixture::new("stable-work-unit-base", vec!["src/**"]).await;
    std::fs::write(fixture.repository.join("other.txt"), "other delivery\n").unwrap();
    git(&fixture.repository, &["add", "other.txt"]);
    git(
        &fixture.repository,
        &["commit", "-m", "merge other delivery"],
    );
    let advanced_head = git_output(&fixture.repository, &["rev-parse", "HEAD"]);
    assert!(
        fixture
            .store
            .compare_and_set_task_head(&fixture.task_run_id, &fixture.base_commit, &advanced_head,)
            .await
            .unwrap()
    );
    fixture.commit_file("src/lib.rs");
    let executor_head = git_output(&fixture.worktree, &["rev-parse", "HEAD"]);

    let delivery = fixture.submit(&executor_head).await.unwrap();

    assert_eq!(delivery.base_commit, fixture.base_commit);
    assert_eq!(delivery.changed_files, vec!["src/lib.rs".to_string()]);
    fixture.cleanup();
}

#[tokio::test]
async fn delivery_rejects_main_workspace_and_other_worktree_from_same_repository() {
    let main_workspace = DeliveryFixture::new("reject-main-workspace", vec!["src/**"]).await;
    let main_head = git_output(&main_workspace.repository, &["rev-parse", "HEAD"]);
    let error = main_workspace
        .coordinator
        .submit_delivery(
            &main_workspace.subagent,
            &main_workspace.repository,
            &main_head,
            "cargo test passed",
        )
        .await
        .expect_err("planner workspace is not the assigned executor worktree");
    assert!(error.to_string().contains("assigned worktree"));
    main_workspace.assert_waiting().await;
    main_workspace.cleanup();

    let other_worktree = DeliveryFixture::new("reject-other-worktree", vec!["src/**"]).await;
    let other_path = other_worktree.repository.with_extension("other-worktree");
    let other_path_text = other_path.to_string_lossy().to_string();
    git(
        &other_worktree.repository,
        &[
            "worktree",
            "add",
            "-b",
            "unassigned-worktree",
            &other_path_text,
            &other_worktree.base_commit,
        ],
    );
    std::fs::create_dir_all(other_path.join("src")).unwrap();
    std::fs::write(other_path.join("src/lib.rs"), "unassigned\n").unwrap();
    git(&other_path, &["add", "src/lib.rs"]);
    git(&other_path, &["commit", "-m", "unassigned delivery"]);
    let other_head = git_output(&other_path, &["rev-parse", "HEAD"]);
    let error = other_worktree
        .coordinator
        .submit_delivery(
            &other_worktree.subagent,
            &other_path,
            &other_head,
            "cargo test passed",
        )
        .await
        .expect_err("other worktree is not assigned");
    assert!(error.to_string().contains("assigned worktree"));
    other_worktree.assert_waiting().await;
    let _ = Command::new("git")
        .arg("-C")
        .arg(&other_worktree.repository)
        .args(["worktree", "remove", "--force", &other_path_text])
        .output();
    remove_repository(other_path);
    other_worktree.cleanup();

    let subdirectory = DeliveryFixture::new("reject-worktree-subdirectory", vec!["src/**"]).await;
    subdirectory.commit_file("src/lib.rs");
    let head = git_output(&subdirectory.worktree, &["rev-parse", "HEAD"]);
    let error = subdirectory
        .coordinator
        .submit_delivery(
            &subdirectory.subagent,
            subdirectory.worktree.join("src"),
            &head,
            "cargo test passed",
        )
        .await
        .expect_err("caller path must be the assigned worktree root");
    assert!(error.to_string().contains("assigned worktree"));
    subdirectory.assert_waiting().await;
    subdirectory.cleanup();
}

#[tokio::test]
async fn rename_from_outside_owned_paths_is_rejected() {
    let fixture = DeliveryFixture::new("rename-outside-owned-paths", vec!["src/**"]).await;
    std::fs::create_dir_all(fixture.worktree.join("src")).unwrap();
    git(&fixture.worktree, &["mv", "README.md", "src/README.md"]);
    git(
        &fixture.worktree,
        &["commit", "-m", "rename into owned path"],
    );
    let head = git_output(&fixture.worktree, &["rev-parse", "HEAD"]);

    let error = fixture
        .submit(&head)
        .await
        .expect_err("rename source is outside owned paths");

    assert!(error.to_string().contains("README.md"));
    fixture.assert_waiting().await;
    fixture.cleanup();
}

#[tokio::test]
async fn invalid_owned_path_wrong_role_and_attempt_four_wait_for_retry() {
    for (name, owned_path) in [
        ("traversal-owned-path", "../escape/**"),
        ("absolute-owned-path", "C:/escape/**"),
    ] {
        let invalid_path = DeliveryFixture::new(name, vec![owned_path]).await;
        invalid_path.commit_file("src/lib.rs");
        let head = git_output(&invalid_path.worktree, &["rev-parse", "HEAD"]);
        let error = invalid_path
            .submit(&head)
            .await
            .expect_err("invalid owned path");
        assert!(error.to_string().contains("invalid owned path"));
        invalid_path.assert_waiting().await;
        invalid_path.cleanup();
    }

    let wrong_role = DeliveryFixture::new("wrong-role", vec!["src/**"]).await;
    wrong_role.commit_file("src/lib.rs");
    let head = git_output(&wrong_role.worktree, &["rev-parse", "HEAD"]);
    let mut explorer = wrong_role.subagent.clone();
    explorer.role = "explorer".to_string();
    let error = wrong_role
        .coordinator
        .submit_delivery(&explorer, &wrong_role.worktree, &head, "cargo test passed")
        .await
        .expect_err("wrong role");
    assert!(error.to_string().contains("executor"));
    wrong_role.assert_waiting().await;
    wrong_role.cleanup();

    let attempt_four = DeliveryFixture::new_with_attempt("attempt-four", vec!["src/**"], 4).await;
    attempt_four.commit_file("src/lib.rs");
    let head = git_output(&attempt_four.worktree, &["rev-parse", "HEAD"]);
    let error = attempt_four.submit(&head).await.expect_err("attempt four");
    assert!(error.to_string().contains("attempt must be within 1..=3"));
    attempt_four.assert_waiting().await;
    attempt_four.cleanup();
}

#[tokio::test]
async fn wrong_owner_missing_work_unit_and_empty_summary_are_actionable() {
    let wrong_owner = DeliveryFixture::new("wrong-owner", vec!["src/**"]).await;
    wrong_owner.commit_file("src/lib.rs");
    let head = git_output(&wrong_owner.worktree, &["rev-parse", "HEAD"]);
    let mut other_owner = wrong_owner.subagent.clone();
    other_owner.parent_id = Some("other-planner".to_string());
    let error = wrong_owner
        .coordinator
        .submit_delivery(
            &other_owner,
            &wrong_owner.worktree,
            &head,
            "cargo test passed",
        )
        .await
        .expect_err("wrong owner");
    assert!(error.to_string().contains("does not own this task outcome"));
    wrong_owner.assert_waiting().await;
    wrong_owner.cleanup();

    let missing = DeliveryFixture::new_without_work_unit("missing-work-unit", vec!["src/**"]).await;
    missing.commit_file("src/lib.rs");
    let head = git_output(&missing.worktree, &["rev-parse", "HEAD"]);
    let error = missing.submit(&head).await.expect_err("missing work unit");
    assert!(error.to_string().contains("no work unit"));
    assert_eq!(
        missing.outcome().await.status,
        AgentOutcomeStatus::WaitingForDelivery
    );
    missing.cleanup();

    let empty_summary = DeliveryFixture::new("empty-summary", vec!["src/**"]).await;
    empty_summary.commit_file("src/lib.rs");
    let head = git_output(&empty_summary.worktree, &["rev-parse", "HEAD"]);
    let error = empty_summary
        .coordinator
        .submit_delivery(
            &empty_summary.subagent,
            &empty_summary.worktree,
            &head,
            "  ",
        )
        .await
        .expect_err("empty verification summary");
    assert!(error.to_string().contains("verificationSummary"));
    empty_summary.assert_waiting().await;
    empty_summary.cleanup();
}

#[tokio::test]
async fn exact_and_backslash_directory_owned_paths_are_normalized() {
    let fixture = DeliveryFixture::new("owned-path-shapes", vec!["README.md", r"src\**"]).await;
    std::fs::write(fixture.worktree.join("README.md"), "updated\n").unwrap();
    std::fs::create_dir_all(fixture.worktree.join("src")).unwrap();
    std::fs::write(fixture.worktree.join("src/lib.rs"), "delivered\n").unwrap();
    git(&fixture.worktree, &["add", "README.md", "src/lib.rs"]);
    git(&fixture.worktree, &["commit", "-m", "deliver"]);
    let head = git_output(&fixture.worktree, &["rev-parse", "HEAD"]);

    let delivery = fixture.submit(&head).await.unwrap();

    assert_eq!(
        delivery.changed_files,
        vec!["README.md".to_string(), "src/lib.rs".to_string()]
    );
    fixture.cleanup();
}

#[tokio::test]
async fn delivery_owned_path_case_matching_follows_platform_semantics() {
    let fixture = DeliveryFixture::new("owned-path-case", vec!["Src/**"]).await;
    fixture.commit_file("src/lib.rs");
    let head = git_output(&fixture.worktree, &["rev-parse", "HEAD"]);

    let result = fixture.submit(&head).await;

    if cfg!(windows) {
        let delivery = result.expect("Windows ownedPaths matching is case-insensitive");
        assert_eq!(delivery.changed_files, vec!["src/lib.rs".to_string()]);
    } else {
        let error = result.expect_err("Unix ownedPaths matching is case-sensitive");
        assert!(error.to_string().contains("outside ownedPaths"));
    }
    fixture.cleanup();
}

#[tokio::test]
async fn submit_delivery_tool_has_typed_schema_branch_effect_and_role_visibility() {
    let coordinator = Arc::new(TaskCoordinator::new(
        StudioStore::open_memory().await.unwrap(),
    ));
    let tool = coordinator.submit_delivery_tool();

    assert_eq!(tool.name(), "submit_delivery");
    assert_eq!(tool.effect(), Some(ToolEffect::BranchControl));
    assert_eq!(
        tool.input_schema(),
        serde_json::json!({
            "type": "object",
            "properties": {
                "headCommit": { "type": "string" },
                "verificationSummary": { "type": "string" }
            },
            "required": ["headCommit", "verificationSummary"],
            "additionalProperties": false
        })
    );

    let mut registry = ToolRegistry::new();
    registry.register(tool);
    let visible = |profile| {
        registry
            .schemas_for_profile(profile)
            .into_iter()
            .map(|schema| schema.name().to_string())
            .collect::<Vec<_>>()
    };
    assert_eq!(
        visible(TurnExecutionProfile::for_subagent(
            CompileMode::Task,
            "executor"
        )),
        vec!["submit_delivery".to_string()]
    );
    assert!(visible(TurnExecutionProfile::root(CompileMode::Task)).is_empty());
    assert!(
        visible(TurnExecutionProfile::for_subagent(
            CompileMode::Task,
            "explorer"
        ))
        .is_empty()
    );
    assert!(
        visible(TurnExecutionProfile::for_subagent(
            CompileMode::Task,
            "reviewer"
        ))
        .is_empty()
    );
}

#[tokio::test]
async fn child_dispatch_resolves_delivery_without_task_session_in_tool_input() {
    let fixture = DeliveryFixture::new("delivery-tool-handler", vec!["src/**"]).await;
    fixture.commit_file("src/lib.rs");
    let head = git_output(&fixture.worktree, &["rev-parse", "HEAD"]);
    let tool = fixture.coordinator.submit_delivery_tool();
    let (event_tx, _) = tokio::sync::broadcast::channel(16);
    let kernel = AgentKernel::builder(
        PureCoreBuilder::from_provider_info(pl_model::ProviderInfo::deepseek(None)).unwrap(),
    )
    .with_profile(CoreAgentProfile::host_provided(fixture.worktree.clone()))
    .with_registered_tool(tool)
    .with_subagent_context(fixture.subagent.clone())
    .build()
    .await;

    let output = kernel
        .execute_tool(
            AgentKernelToolRequest::new(
                "submit_delivery",
                serde_json::json!({
                    "headCommit": head,
                    "verificationSummary": "cargo test passed"
                }),
                "child-turn-not-task-session",
                "call-submit",
                event_tx,
            )
            .with_mode(CompileMode::Task),
        )
        .await
        .unwrap();
    let delivery: AgentDelivery = serde_json::from_str(&output.description).unwrap();

    assert_eq!(delivery, fixture.outcome().await.delivery.unwrap());
    fixture.cleanup();
}

struct DeliveryFixture {
    coordinator: Arc<TaskCoordinator>,
    store: StudioStore,
    session_id: String,
    task_run_id: String,
    work_unit_id: String,
    outcome_id: String,
    repository: PathBuf,
    worktree: PathBuf,
    branch: String,
    base_commit: String,
    subagent: SubagentContext,
}

impl DeliveryFixture {
    async fn new(name: &str, owned_paths: Vec<&str>) -> Self {
        Self::new_configured(name, owned_paths, 1, true).await
    }

    async fn new_with_attempt(name: &str, owned_paths: Vec<&str>, attempt: u32) -> Self {
        Self::new_configured(name, owned_paths, attempt, true).await
    }

    async fn new_without_work_unit(name: &str, owned_paths: Vec<&str>) -> Self {
        Self::new_configured(name, owned_paths, 1, false).await
    }

    async fn new_configured(
        name: &str,
        owned_paths: Vec<&str>,
        attempt: u32,
        link_work_unit: bool,
    ) -> Self {
        let repository = init_repository(name);
        let worktree = repository.with_extension("executor-worktree");
        let store = task_store(&repository).await;
        let session = task_session(&store, &repository).await;
        let coordinator = Arc::new(TaskCoordinator::new(store.clone()));
        let run = coordinator
            .start_confirmed_task(&session.id, "plan", &repository)
            .await
            .unwrap();
        let branch = format!("pure-task-{}-agent-1", run.id);
        let worktree_text = worktree.to_string_lossy().to_string();
        git(
            &repository,
            &[
                "worktree",
                "add",
                "-b",
                &branch,
                &worktree_text,
                &run.base_commit,
            ],
        );
        let work_unit = store
            .create_work_unit(CreateWorkUnit {
                task_run_id: run.id.clone(),
                title: "Implement delivery".to_string(),
                owned_paths: owned_paths.into_iter().map(str::to_string).collect(),
                base_commit: run.expected_head.clone(),
                worktree_path: std::fs::canonicalize(&worktree)
                    .unwrap()
                    .to_string_lossy()
                    .to_string(),
                branch: branch.clone(),
                attempt,
            })
            .await
            .unwrap();
        let work_unit = store
            .update_work_unit(
                &work_unit.id,
                WorkUnitStatus::Running,
                Some("agent-1".to_string()),
            )
            .await
            .unwrap();
        let subagent = SubagentContext {
            id: "agent-1".to_string(),
            parent_id: Some("root".to_string()),
            agent_path: Some("root/agent-1".to_string()),
            role: "executor".to_string(),
            task: "Implement delivery".to_string(),
            depth: 1,
        };
        let task_run_id = run.id.clone();
        let outcome = store
            .create_agent_outcome(CreateAgentOutcome {
                task_run_id: run.id,
                work_unit_id: link_work_unit.then(|| work_unit.id.clone()),
                agent_id: subagent.id.clone(),
                owner_path: "root".to_string(),
                initiated_by: "planner".to_string(),
                requested_by_call_id: "call-spawn".to_string(),
                role: subagent.role.clone(),
                status: AgentOutcomeStatus::Running,
                attempt,
            })
            .await
            .unwrap();
        Self {
            coordinator,
            store,
            session_id: session.id,
            task_run_id,
            work_unit_id: work_unit.id,
            outcome_id: outcome.id,
            repository,
            worktree,
            branch,
            base_commit: run.base_commit,
            subagent,
        }
    }

    fn commit_file(&self, relative_path: &str) {
        let path = self.worktree.join(relative_path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, "delivered\n").unwrap();
        git(&self.worktree, &["add", relative_path]);
        git(&self.worktree, &["commit", "-m", "deliver"]);
    }

    async fn submit(&self, head: &str) -> anyhow::Result<AgentDelivery> {
        self.coordinator
            .submit_delivery(&self.subagent, &self.worktree, head, "cargo test passed")
            .await
    }

    async fn outcome(&self) -> AgentOutcomeRecord {
        self.store
            .list_agent_outcomes(&self.task_run_id)
            .await
            .unwrap()
            .into_iter()
            .find(|outcome| outcome.id == self.outcome_id)
            .unwrap()
    }

    async fn work_unit(&self) -> WorkUnitRecord {
        self.store
            .read_work_unit(&self.work_unit_id)
            .await
            .unwrap()
            .unwrap()
    }

    async fn assert_waiting(&self) {
        assert_eq!(
            self.outcome().await.status,
            AgentOutcomeStatus::WaitingForDelivery
        );
        assert_eq!(
            self.work_unit().await.status,
            WorkUnitStatus::WaitingForDelivery
        );
    }

    fn cleanup(self) {
        drop(self.coordinator);
        let worktree_text = self.worktree.to_string_lossy().to_string();
        let _ = Command::new("git")
            .arg("-C")
            .arg(&self.repository)
            .args(["worktree", "remove", "--force", &worktree_text])
            .output();
        remove_repository(self.worktree);
        remove_repository(self.repository);
    }
}

async fn drain_agent_state(
    fixture: &DeliveryFixture,
    status: pl_protocol::AgentStatus,
    summary: Option<&str>,
    error: Option<&str>,
) {
    let config_store = crate::config::ConfigStore::new(crate::config::ConfigPaths::from_home(
        std::env::temp_dir().join(format!(
            "pure-terminal-outcome-test-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        )),
    ));
    let runtime = crate::studio::StudioRuntime::new(fixture.store.clone(), config_store);
    let (event_tx, event_rx) = tokio::sync::broadcast::channel(8);
    let drain = {
        let runtime = runtime.clone();
        let session_id = fixture.session_id.clone();
        tokio::spawn(async move {
            runtime.drain_agent_events(session_id, event_rx).await;
        })
    };
    event_tx
        .send(pl_trace::AgentEvent::AgentStateChanged {
            id: fixture.subagent.id.clone(),
            path: "/root/executor".to_string(),
            parent_path: Some("/root".to_string()),
            role: "executor".to_string(),
            task: "Implement delivery".to_string(),
            status,
            summary: summary.map(str::to_string),
            depth: 1,
            error: error.map(str::to_string),
            reason: None,
            budget_limit_kind: None,
            budget_usage: None,
            updated_at: 10,
        })
        .unwrap();
    drop(event_tx);
    drain.await.unwrap();
}

async fn spawned_agent_id(session_id: &str) -> String {
    let supervisor = AgentSupervisor::default();
    supervisor
        .spawn_agent(
            AgentSpawnInput {
                task_name: "worker".to_string(),
                message: "inspect".to_string(),
                role: "explorer".to_string(),
                parent_path: Some("/root".to_string()),
                session_id: session_id.to_string(),
                owned_paths: Vec::new(),
            },
            test_agent_run_spec("inspect"),
        )
        .await
        .unwrap()
        .id
}

fn test_agent_run_spec(message: &str) -> AgentRunSpec {
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
        call_id: "call-spawn".to_string(),
        message: message.to_string(),
        mode: CompileMode::Simple,
        budget: TurnBudget::default(),
        initial_session: crate::CoreSession::new(),
    }
}

fn agent_state_event(agent_id: &str, status: pl_protocol::AgentStatus) -> pl_trace::AgentEvent {
    pl_trace::AgentEvent::AgentStateChanged {
        id: agent_id.to_string(),
        path: "/root/worker".to_string(),
        parent_path: Some("/root".to_string()),
        role: "explorer".to_string(),
        task: "inspect".to_string(),
        status,
        summary: Some("old completion".to_string()),
        depth: 1,
        error: None,
        reason: None,
        budget_limit_kind: None,
        budget_usage: None,
        updated_at: 10,
    }
}

async fn drain_studio_agent_event(
    store: StudioStore,
    session_id: &str,
    event: pl_trace::AgentEvent,
) {
    drain_studio_agent_events(store, session_id, vec![event]).await;
}

async fn drain_studio_agent_events(
    store: StudioStore,
    session_id: &str,
    events: Vec<pl_trace::AgentEvent>,
) {
    let config_store = crate::config::ConfigStore::new(crate::config::ConfigPaths::from_home(
        std::env::temp_dir().join(format!(
            "pure-agent-id-test-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        )),
    ));
    let runtime = crate::studio::StudioRuntime::new(store, config_store);
    let (event_tx, event_rx) = tokio::sync::broadcast::channel(8);
    let drain = {
        let runtime = runtime.clone();
        let session_id = session_id.to_string();
        tokio::spawn(async move {
            runtime.drain_agent_events(session_id, event_rx).await;
        })
    };
    for event in events {
        event_tx.send(event).unwrap();
    }
    drop(event_tx);
    drain.await.unwrap();
}

#[tokio::test]
async fn task_start_requires_clean_named_branch() {
    let repository = init_repository("start-guards");
    let store = task_store(&repository).await;
    let session = task_session(&store, &repository).await;
    let coordinator = TaskCoordinator::new(store);

    std::fs::write(repository.join("dirty.txt"), "dirty").unwrap();
    let dirty = coordinator
        .start_confirmed_task(&session.id, "plan", &repository)
        .await
        .expect_err("dirty repository must be rejected");
    assert!(dirty.to_string().contains("clean working tree"));

    std::fs::remove_file(repository.join("dirty.txt")).unwrap();
    git(&repository, &["checkout", "--detach"]);
    let detached = coordinator
        .start_confirmed_task(&session.id, "plan", &repository)
        .await
        .expect_err("detached HEAD must be rejected");
    assert!(detached.to_string().contains("detached HEAD"));

    remove_repository(repository);
}

#[tokio::test]
async fn task_head_drift_blocks_the_persisted_run() {
    let repository = init_repository("head-drift");
    let store = task_store(&repository).await;
    let session = task_session(&store, &repository).await;
    let coordinator = TaskCoordinator::new(store.clone());
    let run = coordinator
        .start_confirmed_task(&session.id, "plan", &repository)
        .await
        .unwrap();

    let competing = TaskCoordinator::new(store.clone());
    let lease_error = competing
        .start_confirmed_task(&session.id, "another plan", &repository)
        .await
        .expect_err("one process may not own the branch twice");
    assert!(lease_error.to_string().contains("already owned"));

    std::fs::write(repository.join("external.txt"), "external").unwrap();
    git(&repository, &["add", "external.txt"]);
    git(&repository, &["commit", "-m", "external change"]);

    assert!(!coordinator.verify_expected_head(&run.id).await.unwrap());
    let blocked = store.read_task_run(&run.id).await.unwrap().unwrap();
    assert_eq!(blocked.phase, TaskRunPhase::Blocked);
    assert!(
        blocked
            .status_message
            .as_deref()
            .unwrap_or_default()
            .contains("HEAD drifted")
    );

    coordinator
        .finish_task(
            &run.id,
            TaskRunPhase::Cancelled,
            Some("stopped".to_string()),
        )
        .await
        .unwrap();
    assert!(store.read_branch_lease(&run.id).await.unwrap().is_none());
    remove_repository(repository);
}

#[tokio::test]
async fn coordinator_recovers_active_task_after_restart() {
    let repository = init_repository("recovery");
    let store = task_store(&repository).await;
    let session = task_session(&store, &repository).await;
    let run = {
        let coordinator = TaskCoordinator::new(store.clone());
        coordinator
            .start_confirmed_task(&session.id, "plan", &repository)
            .await
            .unwrap()
    };

    let recovered_coordinator = TaskCoordinator::new(store.clone());
    let recovered = recovered_coordinator.recover_active_tasks().await.unwrap();

    assert_eq!(recovered, vec![run.clone()]);
    assert!(
        recovered_coordinator
            .verify_expected_head(&run.id)
            .await
            .unwrap()
    );
    recovered_coordinator
        .finish_task(&run.id, TaskRunPhase::Cancelled, None)
        .await
        .unwrap();
    remove_repository(repository);
}

async fn task_store(repository: &Path) -> StudioStore {
    let store = StudioStore::open_memory().await.unwrap();
    store.upsert_project(repository).await.unwrap();
    store
}

async fn task_session(store: &StudioStore, repository: &Path) -> crate::studio::SessionRecord {
    let project = store.upsert_project(repository).await.unwrap();
    store
        .create_session(&project.id, "Task", CompileMode::Task)
        .await
        .unwrap()
}

fn init_repository(name: &str) -> PathBuf {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = std::env::temp_dir().join(format!(
        "pure-task-coordinator-{name}-{}-{stamp}",
        std::process::id()
    ));
    std::fs::create_dir_all(&path).unwrap();
    git(&path, &["init"]);
    git(&path, &["checkout", "-b", "main"]);
    git(&path, &["config", "user.email", "pure@example.invalid"]);
    git(&path, &["config", "user.name", "Pure Test"]);
    std::fs::write(path.join("README.md"), "initial\n").unwrap();
    git(&path, &["add", "README.md"]);
    git(&path, &["commit", "-m", "initial"]);
    path
}

fn git(repository: &Path, args: &[&str]) {
    let output = Command::new("git")
        .arg("-C")
        .arg(repository)
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {} failed: {}",
        args.join(" "),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn git_output(repository: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .arg("-C")
        .arg(repository)
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {} failed: {}",
        args.join(" "),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

fn remove_repository(path: PathBuf) {
    let _ = std::fs::remove_dir_all(path);
}

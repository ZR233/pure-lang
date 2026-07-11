use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::process::Command;
use std::sync::Arc;

use anyhow::{Result, anyhow};
use pl_protocol::{AgentStatus, StudioEventKind, StudioPartType};
use tokio::sync::Mutex;

use super::*;
use crate::studio::active_turns::SessionAlreadyHasActiveTurn;
use crate::studio::runtime::continuation::{
    ContinuationLaunch, ContinuationLauncher, ContinuationReason, ContinuationRequest,
    ContinuationScheduler, ContinuationTestBarrier, PromptCompletionTestBarrier, SessionTurnState,
};
use crate::studio::task_coordinator::{
    AgentOutcomeStatus, CreateAgentOutcome, CreateTaskRun, CreateWorkUnit, TaskRunPhase,
    WorkUnitStatus,
};

#[tokio::test]
async fn active_root_turn_defers_and_coalesces_continuation_until_removal() {
    let scheduler = ContinuationScheduler::new();
    let request = continuation_request("run-1", "session-1", ContinuationReason::AgentTerminal);

    assert_eq!(
        scheduler
            .request(request.clone(), SessionTurnState::Active)
            .await,
        None
    );
    assert_eq!(
        scheduler
            .request(request.clone(), SessionTurnState::Active)
            .await,
        None
    );
    let claim = scheduler
        .turn_removed("session-1", "turn-user")
        .await
        .unwrap();
    assert_eq!(claim.request, request);
    assert_eq!(scheduler.bind_turn(&claim, "turn-continuation").await, None);
    assert_eq!(
        scheduler
            .turn_removed("session-1", "turn-continuation")
            .await,
        None
    );
}

#[tokio::test]
async fn durable_fact_during_active_continuation_launches_one_later_continuation() {
    let scheduler = ContinuationScheduler::new();
    let first = continuation_request("run-1", "session-1", ContinuationReason::AgentTerminal);
    let later = continuation_request("run-1", "session-1", ContinuationReason::ReviewReturned);

    let first_claim = scheduler
        .request(first.clone(), SessionTurnState::Idle)
        .await
        .unwrap();
    assert_eq!(first_claim.request, first);
    assert_eq!(scheduler.bind_turn(&first_claim, "turn-first").await, None);
    assert_eq!(
        scheduler
            .request(later.clone(), SessionTurnState::Active)
            .await,
        None
    );
    assert_eq!(
        scheduler
            .request(later.clone(), SessionTurnState::Active)
            .await,
        None
    );
    let later_claim = scheduler
        .turn_removed("session-1", "turn-first")
        .await
        .unwrap();
    assert_eq!(later_claim.request, later);
    assert_eq!(scheduler.bind_turn(&later_claim, "turn-later").await, None);
    assert_eq!(
        scheduler.turn_removed("session-1", "turn-later").await,
        None
    );
}

#[tokio::test]
async fn sessions_schedule_independently_without_concurrent_same_session_launches() {
    let scheduler = ContinuationScheduler::new();
    let first = continuation_request("run-1", "session-1", ContinuationReason::AgentTerminal);
    let first_later = continuation_request("run-1", "session-1", ContinuationReason::MergeConflict);
    let second = continuation_request("run-2", "session-2", ContinuationReason::Recovery);

    let first_claim = scheduler
        .request(first.clone(), SessionTurnState::Idle)
        .await
        .unwrap();
    assert_eq!(first_claim.request, first);
    assert_eq!(scheduler.bind_turn(&first_claim, "turn-first").await, None);
    assert_eq!(
        scheduler
            .request(first_later.clone(), SessionTurnState::Active)
            .await,
        None
    );
    let second_claim = scheduler
        .request(second.clone(), SessionTurnState::Idle)
        .await
        .unwrap();
    assert_eq!(second_claim.request, second);
    assert_eq!(
        scheduler.bind_turn(&second_claim, "turn-second").await,
        None
    );
    let first_later_claim = scheduler
        .turn_removed("session-1", "turn-first")
        .await
        .unwrap();
    assert_eq!(first_later_claim.request, first_later);
    assert_eq!(
        scheduler
            .bind_turn(&first_later_claim, "turn-first-later")
            .await,
        None
    );
    assert_eq!(
        scheduler.turn_removed("session-2", "turn-second").await,
        None
    );
}

#[tokio::test]
async fn duplicate_recovery_notification_does_not_queue_an_extra_turn() {
    let scheduler = ContinuationScheduler::new();
    let recovery = continuation_request("run-1", "session-1", ContinuationReason::Recovery);

    let recovery_claim = scheduler
        .request(recovery.clone(), SessionTurnState::Idle)
        .await
        .unwrap();
    assert_eq!(recovery_claim.request, recovery.clone());
    assert_eq!(
        scheduler.bind_turn(&recovery_claim, "turn-recovery").await,
        None
    );
    assert_eq!(
        scheduler.request(recovery, SessionTurnState::Active).await,
        None
    );
    assert_eq!(
        scheduler.turn_removed("session-1", "turn-recovery").await,
        None
    );
}

#[tokio::test]
async fn stale_defer_cannot_replace_a_newer_claim() {
    let scheduler = ContinuationScheduler::new();
    let first = continuation_request("run-1", "session-1", ContinuationReason::AgentTerminal);
    let second = continuation_request("run-1", "session-1", ContinuationReason::ReviewReturned);
    let first_claim = scheduler
        .request(first, SessionTurnState::Idle)
        .await
        .unwrap();
    assert_eq!(
        scheduler
            .request(second.clone(), SessionTurnState::Active)
            .await,
        None
    );
    let second_claim = scheduler
        .turn_removed("session-1", "turn-user")
        .await
        .unwrap();
    assert_eq!(second_claim.request, second);
    assert_eq!(
        scheduler.bind_turn(&second_claim, "turn-second").await,
        None
    );

    scheduler.defer(first_claim).await;

    assert_eq!(
        scheduler.turn_removed("session-1", "turn-second").await,
        None
    );
}

#[tokio::test]
async fn stale_claim_cannot_cancel_a_newer_claim() {
    let scheduler = ContinuationScheduler::new();
    let first = continuation_request("run-1", "session-1", ContinuationReason::AgentTerminal);
    let second = continuation_request("run-1", "session-1", ContinuationReason::ReviewReturned);
    let first_claim = scheduler
        .request(first, SessionTurnState::Idle)
        .await
        .unwrap();
    assert_eq!(
        scheduler.request(second, SessionTurnState::Active).await,
        None
    );
    let second_claim = scheduler
        .turn_removed("session-1", "turn-user")
        .await
        .unwrap();

    assert!(!scheduler.cancel_claim(&first_claim).await);
    assert_eq!(
        scheduler.bind_turn(&second_claim, "turn-second").await,
        None
    );
    assert!(scheduler.has_session("session-1").await);
}

#[tokio::test]
async fn exact_claim_cancellation_clears_coalesced_pending_state() {
    let scheduler = ContinuationScheduler::new();
    let first = continuation_request("run-1", "session-1", ContinuationReason::AgentTerminal);
    let second = continuation_request("run-1", "session-1", ContinuationReason::ReviewReturned);
    let first_claim = scheduler
        .request(first, SessionTurnState::Idle)
        .await
        .unwrap();
    assert_eq!(
        scheduler.request(second, SessionTurnState::Active).await,
        None
    );

    assert!(scheduler.cancel_claim(&first_claim).await);
    assert!(!scheduler.has_session("session-1").await);
}

#[tokio::test]
async fn continuation_completion_before_turn_binding_does_not_requeue_claim() {
    let scheduler = ContinuationScheduler::new();
    let request = continuation_request("run-1", "session-1", ContinuationReason::AgentTerminal);
    let claim = scheduler
        .request(request, SessionTurnState::Idle)
        .await
        .unwrap();

    assert_eq!(
        scheduler
            .turn_removed("session-1", "turn-continuation")
            .await,
        None
    );
    assert_eq!(scheduler.bind_turn(&claim, "turn-continuation").await, None);
    assert!(!scheduler.has_session("session-1").await);
}

#[tokio::test]
async fn unbound_claim_retains_all_early_turn_removals_until_binding() {
    let scheduler = ContinuationScheduler::new();
    let first = continuation_request("run-1", "session-1", ContinuationReason::AgentTerminal);
    let claim = scheduler
        .request(first, SessionTurnState::Idle)
        .await
        .unwrap();

    assert_eq!(
        scheduler
            .turn_removed("session-1", "turn-continuation")
            .await,
        None
    );
    assert_eq!(scheduler.turn_removed("session-1", "turn-user").await, None);

    let later = continuation_request("run-1", "session-1", ContinuationReason::ReviewReturned);
    assert_eq!(
        scheduler
            .request(later.clone(), SessionTurnState::Active)
            .await,
        None
    );
    let later_claim = scheduler
        .bind_turn(&claim, "turn-continuation")
        .await
        .unwrap();
    assert_eq!(later_claim.request, later);
}

#[tokio::test]
async fn user_removal_before_busy_defer_requeues_exact_claim() {
    let scheduler = ContinuationScheduler::new();
    let request = continuation_request("run-1", "session-1", ContinuationReason::AgentTerminal);
    let claim = scheduler
        .request(request.clone(), SessionTurnState::Idle)
        .await
        .unwrap();

    assert_eq!(scheduler.turn_removed("session-1", "turn-user").await, None);
    scheduler.defer(claim).await;
    let retried = scheduler.claim_if_idle("session-1").await.unwrap();

    assert_eq!(retried.request, request);
}

#[tokio::test]
async fn removal_between_active_observation_and_enqueue_still_launches_once() {
    let (store, run, _) = continuation_fixture("enqueue-removal-race").await;
    let launcher = RecordingLauncher::successful();
    let mut runtime = test_runtime(store, launcher.clone());
    let barrier = ContinuationTestBarrier::new();
    runtime.continuation_request_barrier = Some(barrier.clone());
    runtime
        .active_turns
        .insert(
            run.session_id.clone(),
            "turn-competing".to_string(),
            tokio_util::sync::CancellationToken::new(),
        )
        .await
        .unwrap();
    let request = {
        let runtime = runtime.clone();
        let run_id = run.id.clone();
        tokio::spawn(async move {
            runtime
                .request_task_continuation(run_id, ContinuationReason::AgentTerminal)
                .await;
        })
    };

    barrier.wait_until_entered().await;
    runtime
        .active_turn_removed(&run.session_id, "turn-competing")
        .await;
    barrier.release().await;
    request.await.unwrap();
    let launches = launcher.wait_for_count(1).await;

    assert_eq!(launches.len(), 1);
    assert_eq!(launches[0].request.task_run_id, run.id);
}

#[tokio::test]
async fn stale_turn_removal_preserves_current_generation_and_pending_continuation() {
    let (store, run, _) = continuation_fixture("stale-turn-removal").await;
    let launcher = RecordingLauncher::successful();
    let runtime = test_runtime(store, launcher.clone());
    runtime
        .active_turns
        .insert(
            run.session_id.clone(),
            "turn-current".to_string(),
            tokio_util::sync::CancellationToken::new(),
        )
        .await
        .unwrap();
    runtime
        .request_task_continuation(run.id, ContinuationReason::AgentTerminal)
        .await;

    runtime
        .active_turn_removed(&run.session_id, "turn-stale")
        .await;

    assert!(runtime.active_turns.contains(&run.session_id).await);
    assert!(
        runtime
            .runtime_snapshot()
            .active_turns
            .iter()
            .any(|turn| turn.session_id == run.session_id && turn.turn_id == "turn-current")
    );
    assert!(
        runtime
            .continuation_scheduler
            .has_session(&run.session_id)
            .await
    );
    assert!(launcher.launches.lock().await.is_empty());
}

#[tokio::test]
async fn production_busy_collision_removed_before_defer_relaunches_once() {
    let repository = init_repository("continuation-submit-defer-race");
    let store = StudioStore::open_memory().await.unwrap();
    let run = persisted_repository_run(&store, &repository, "submit-defer-race").await;
    let home = std::env::temp_dir().join(format!("pure-defer-home-{}", unique_id()));
    let mut runtime = StudioRuntime::new(
        store.clone(),
        ConfigStore::new(crate::config::ConfigPaths::from_home(home)),
    );
    let prepared = ContinuationTestBarrier::new();
    let launch_error = ContinuationTestBarrier::new();
    runtime.continuation_pre_submit_barrier = Some(prepared.clone());
    runtime.continuation_launch_error_barrier = Some(launch_error.clone());

    runtime
        .request_task_continuation(run.id.clone(), ContinuationReason::Recovery)
        .await;
    prepared.wait_until_entered().await;
    runtime
        .active_turns
        .insert(
            run.session_id.clone(),
            "turn-competing".to_string(),
            tokio_util::sync::CancellationToken::new(),
        )
        .await
        .unwrap();
    prepared.release().await;
    launch_error.wait_until_entered().await;
    runtime
        .active_turn_removed(&run.session_id, "turn-competing")
        .await;
    launch_error.release().await;

    let part = tokio::time::timeout(TEST_RUNTIME_TIMEOUT, async {
        loop {
            if let Some(part) = store
                .load_message_parts(&run.session_id)
                .await
                .unwrap()
                .into_iter()
                .find(|record| record.part.text == "继续任务")
            {
                break part.part;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();

    assert!(part.synthetic);
    assert!(part.ignored);
    assert_ne!(
        store.read_task_run(&run.id).await.unwrap().unwrap().phase,
        TaskRunPhase::Blocked
    );
    remove_repository(repository);
}

#[tokio::test]
async fn stale_busy_claim_cannot_overwrite_new_fact_claimed_by_turn_removal() {
    let repository = init_repository("continuation-stale-defer-full-race");
    let store = StudioStore::open_memory().await.unwrap();
    let run = persisted_repository_run(&store, &repository, "stale-defer-full-race").await;
    let home = std::env::temp_dir().join(format!("pure-stale-defer-home-{}", unique_id()));
    let mut runtime = StudioRuntime::new(
        store.clone(),
        ConfigStore::new(crate::config::ConfigPaths::from_home(home)),
    );
    let prepared = ContinuationTestBarrier::new();
    let launch_error = ContinuationTestBarrier::new();
    runtime.continuation_pre_submit_barrier = Some(prepared.clone());
    runtime.continuation_launch_error_barrier = Some(launch_error.clone());

    runtime
        .request_task_continuation(run.id.clone(), ContinuationReason::Recovery)
        .await;
    prepared.wait_until_entered().await;
    runtime
        .active_turns
        .insert(
            run.session_id.clone(),
            "turn-competing".to_string(),
            tokio_util::sync::CancellationToken::new(),
        )
        .await
        .unwrap();
    prepared.release().await;
    launch_error.wait_until_entered().await;
    runtime
        .request_task_continuation(run.id.clone(), ContinuationReason::AgentTerminal)
        .await;
    runtime
        .active_turn_removed(&run.session_id, "turn-competing")
        .await;
    launch_error.release().await;

    tokio::time::timeout(TEST_RUNTIME_TIMEOUT, async {
        loop {
            let continuation_parts = store
                .load_message_parts(&run.session_id)
                .await
                .unwrap()
                .into_iter()
                .filter(|record| record.part.text == "继续任务")
                .count();
            if continuation_parts == 1 {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    tokio::task::yield_now().await;

    assert_eq!(
        store
            .load_message_parts(&run.session_id)
            .await
            .unwrap()
            .into_iter()
            .filter(|record| record.part.text == "继续任务")
            .count(),
        1
    );
    assert_ne!(
        store.read_task_run(&run.id).await.unwrap().unwrap().phase,
        TaskRunPhase::Blocked
    );
    remove_repository(repository);
}

#[tokio::test]
async fn changed_terminal_fact_launches_once_with_current_durable_prompt() {
    let (store, run, outcome) = continuation_fixture("terminal-launch").await;
    let launcher = RecordingLauncher::successful();
    let runtime = test_runtime(store.clone(), launcher.clone());
    let event = terminal_event(&outcome.agent_id);
    let (event_tx, event_rx) = tokio::sync::broadcast::channel(8);
    let drain = {
        let runtime = runtime.clone();
        let session_id = run.session_id.clone();
        tokio::spawn(async move { runtime.drain_agent_events(session_id, event_rx).await })
    };

    event_tx.send(event.clone()).unwrap();
    event_tx.send(event).unwrap();
    drop(event_tx);
    drain.await.unwrap();
    let launches = launcher.wait_for_count(1).await;

    assert_eq!(launches.len(), 1);
    assert_eq!(launches[0].request.task_run_id, run.id);
    assert_eq!(
        launches[0].request.reason,
        ContinuationReason::AgentTerminal
    );
    assert!(launches[0].prompt.contains("implementing"));
    assert!(launches[0].prompt.contains(&run.branch));
    assert!(launches[0].prompt.contains(&run.expected_head));
    assert!(launches[0].prompt.contains("agent failed"));
    assert_eq!(
        store
            .list_agent_outcomes(&run.id)
            .await
            .unwrap()
            .into_iter()
            .find(|record| record.id == outcome.id)
            .unwrap()
            .status,
        AgentOutcomeStatus::Failed
    );
}

#[tokio::test]
async fn launch_failure_blocks_exact_run_and_emits_diagnostic() {
    let (store, run, _) = continuation_fixture("launch-failure").await;
    let launcher = RecordingLauncher::failing("injected launch failure");
    let runtime = test_runtime(store.clone(), launcher);

    runtime
        .request_task_continuation(run.id.clone(), ContinuationReason::MergeConflict)
        .await;
    tokio::time::timeout(TEST_RUNTIME_TIMEOUT, async {
        loop {
            let persisted = store.read_task_run(&run.id).await.unwrap().unwrap();
            if persisted.phase == TaskRunPhase::Blocked {
                break persisted;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();

    let blocked = store.read_task_run(&run.id).await.unwrap().unwrap();
    assert_eq!(blocked.phase, TaskRunPhase::Blocked);
    assert!(
        blocked
            .status_message
            .as_deref()
            .unwrap()
            .contains("injected launch failure")
    );
    let events = store
        .load_studio_events(&run.session_id, None, None)
        .await
        .unwrap();
    assert!(events.iter().any(|event| matches!(
        &event.kind,
        StudioEventKind::TurnChanged { turn }
            if turn.reason.as_deref().is_some_and(|reason| reason.contains("injected launch failure"))
    )));
}

#[tokio::test]
async fn snapshot_failure_blocks_exact_run_and_emits_diagnostic() {
    let (store, run, _) = continuation_fixture("snapshot-failure").await;
    store
        .execute_test_sql(&format!(
            "DELETE FROM branch_leases WHERE task_run_id = '{}'",
            run.id
        ))
        .await;
    let launcher = RecordingLauncher::successful();
    let runtime = test_runtime(store.clone(), launcher.clone());

    runtime
        .request_task_continuation(run.id.clone(), ContinuationReason::ReviewReturned)
        .await;
    let blocked = tokio::time::timeout(TEST_RUNTIME_TIMEOUT, async {
        loop {
            let persisted = store.read_task_run(&run.id).await.unwrap().unwrap();
            if persisted.phase == TaskRunPhase::Blocked {
                break persisted;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();

    assert!(
        blocked
            .status_message
            .as_deref()
            .unwrap()
            .contains("task branch lease not found")
    );
    assert!(launcher.launches.lock().await.is_empty());
    tokio::time::timeout(TEST_RUNTIME_TIMEOUT, async {
        loop {
            let events = store
                .load_studio_events(&run.session_id, None, None)
                .await
                .unwrap();
            if events.iter().any(|event| matches!(
                &event.kind,
                StudioEventKind::TurnChanged { turn }
                    if turn.reason.as_deref().is_some_and(|reason| reason.contains("task branch lease not found"))
            )) {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn session_failure_blocks_exact_run_before_launch() {
    let (store, run, _) = continuation_fixture("session-failure").await;
    store
        .execute_test_sql(&format!(
            "UPDATE sessions SET mode = 'auto' WHERE id = '{}'",
            run.session_id
        ))
        .await;
    let launcher = RecordingLauncher::successful();
    let runtime = test_runtime(store.clone(), launcher.clone());

    runtime
        .request_task_continuation(run.id.clone(), ContinuationReason::ReviewReturned)
        .await;
    let blocked = tokio::time::timeout(TEST_RUNTIME_TIMEOUT, async {
        loop {
            let persisted = store.read_task_run(&run.id).await.unwrap().unwrap();
            if persisted.phase == TaskRunPhase::Blocked {
                break persisted;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();

    assert!(
        blocked
            .status_message
            .as_deref()
            .unwrap()
            .contains("task continuation session not found")
    );
    assert!(launcher.launches.lock().await.is_empty());
}

#[tokio::test]
async fn terminal_run_without_branch_lease_is_discarded_before_snapshot() {
    let (store, run, _) = continuation_fixture("terminal-before-prepare").await;
    store
        .transition_task_run(
            &run.id,
            TaskRunPhase::Blocked,
            Some("terminal before prepare".to_string()),
        )
        .await
        .unwrap();
    store.release_branch_lease(&run.id).await.unwrap();
    let launcher = RecordingLauncher::successful();
    let runtime = test_runtime(store.clone(), launcher.clone());

    runtime
        .request_task_continuation(run.id.clone(), ContinuationReason::Recovery)
        .await;

    assert!(launcher.launches.lock().await.is_empty());
    let persisted = store.read_task_run(&run.id).await.unwrap().unwrap();
    assert_eq!(persisted.phase, TaskRunPhase::Blocked);
    assert_eq!(
        persisted.status_message.as_deref(),
        Some("terminal before prepare")
    );
}

#[tokio::test]
async fn terminal_after_prepare_is_discarded_before_callback() {
    let (store, run, _) = continuation_fixture("terminal-after-prepare").await;
    let launcher = RecordingLauncher::successful();
    let mut runtime = test_runtime(store.clone(), launcher.clone());
    let prepared = ContinuationTestBarrier::new();
    runtime.continuation_pre_submit_barrier = Some(prepared.clone());

    runtime
        .request_task_continuation(run.id.clone(), ContinuationReason::Recovery)
        .await;
    prepared.wait_until_entered().await;
    store
        .transition_task_run(
            &run.id,
            TaskRunPhase::Blocked,
            Some("terminal after prepare".to_string()),
        )
        .await
        .unwrap();
    store.release_branch_lease(&run.id).await.unwrap();
    prepared.release().await;
    tokio::time::timeout(TEST_RUNTIME_TIMEOUT, async {
        while runtime
            .continuation_scheduler
            .has_session(&run.session_id)
            .await
        {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();

    assert!(launcher.launches.lock().await.is_empty());
}

#[tokio::test]
async fn production_continuation_uses_ignored_synthetic_user_label() {
    let repository = init_repository("continuation-production-label");
    let store = StudioStore::open_memory().await.unwrap();
    let run = persisted_repository_run(&store, &repository, "production-label").await;
    let home = std::env::temp_dir().join(format!("pure-label-home-{}", unique_id()));
    let runtime = StudioRuntime::new(
        store.clone(),
        ConfigStore::new(crate::config::ConfigPaths::from_home(home)),
    );

    runtime
        .request_task_continuation(run.id.clone(), ContinuationReason::Recovery)
        .await;
    let part = tokio::time::timeout(TEST_RUNTIME_TIMEOUT, async {
        loop {
            if let Some(part) = store
                .load_message_parts(&run.session_id)
                .await
                .unwrap()
                .into_iter()
                .find(|record| record.part.text == "继续任务")
            {
                break part.part;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();

    assert!(part.synthetic);
    assert!(part.ignored);
    remove_repository(repository);
}

#[tokio::test]
async fn production_continuation_does_not_persist_snapshot_turn_in_core_history() {
    let sse_body = concat!(
        "data: {\"type\":\"response.output_text.delta\",\"item_id\":\"msg_1\",\"delta\":\"continued\"}\n\n",
        "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_1\",\"usage\":{\"input_tokens\":1,\"output_tokens\":1,\"total_tokens\":2}}}\n\n",
        "data: [DONE]\n\n"
    )
    .to_string();
    let (base_url, handle) = serve_sse_once(sse_body).await;
    let repository = init_repository("continuation-ephemeral-history");
    let store = StudioStore::open_memory().await.unwrap();
    let run = persisted_repository_run(&store, &repository, "ephemeral-history").await;
    let prior = crate::user_text_message("prior durable history");
    store.append_message(&run.session_id, &prior).await.unwrap();
    let home = std::env::temp_dir().join(format!("pure-history-home-{}", unique_id()));
    let config_store = ConfigStore::new(crate::config::ConfigPaths::from_home(home));
    config_store.save(&test_config(base_url)).unwrap();
    let runtime = StudioRuntime::new(store.clone(), config_store);

    runtime
        .request_task_continuation(run.id.clone(), ContinuationReason::Recovery)
        .await;
    handle.await.unwrap();
    wait_for_no_active_turn(&runtime).await;

    let history = store.load_core_session(&run.session_id).await.unwrap();
    assert_eq!(history.messages(), &[prior]);
    let parts = store.load_message_parts(&run.session_id).await.unwrap();
    assert!(parts.iter().any(|record| {
        record.part.text == "继续任务" && record.part.synthetic && record.part.ignored
    }));
    remove_repository(repository);
}

#[tokio::test]
async fn ephemeral_continuation_plan_part_does_not_create_plan_confirmation() {
    let tool_sse = concat!(
        "data: {\"type\":\"response.output_item.added\",\"item\":{\"type\":\"function_call\",\"id\":\"fc_ephemeral\",\"call_id\":\"call_ephemeral\",\"name\":\"plan_exit\"}}\n\n",
        "data: {\"type\":\"response.function_call_arguments.delta\",\"item_id\":\"fc_ephemeral\",\"call_id\":\"call_ephemeral\",\"delta\":\"{\\\"content\\\":\\\"# Ephemeral Plan\\\\n\\\\n- Continue task\\\"}\"}\n\n",
        "data: {\"type\":\"response.output_item.done\",\"item\":{\"type\":\"function_call\",\"id\":\"fc_ephemeral\",\"call_id\":\"call_ephemeral\",\"name\":\"plan_exit\",\"arguments\":\"{\\\"content\\\":\\\"# Ephemeral Plan\\\\n\\\\n- Continue task\\\"}\"}}\n\n",
        "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_ephemeral_1\",\"usage\":{\"input_tokens\":1,\"output_tokens\":2,\"total_tokens\":3}}}\n\n",
        "data: [DONE]\n\n"
    )
    .to_string();
    let final_sse = concat!(
        "data: {\"type\":\"response.output_item.added\",\"item\":{\"id\":\"msg_ephemeral\",\"type\":\"message\",\"role\":\"assistant\",\"phase\":\"final_answer\"}}\n\n",
        "data: {\"type\":\"response.output_text.delta\",\"item_id\":\"msg_ephemeral\",\"delta\":\"continued\"}\n\n",
        "data: {\"type\":\"response.output_item.done\",\"item\":{\"id\":\"msg_ephemeral\",\"type\":\"message\",\"role\":\"assistant\",\"phase\":\"final_answer\",\"content\":[{\"type\":\"output_text\",\"text\":\"continued\"}]}}\n\n",
        "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_ephemeral_2\",\"usage\":{\"input_tokens\":2,\"output_tokens\":1,\"total_tokens\":3}}}\n\n",
        "data: [DONE]\n\n"
    )
    .to_string();
    let (base_url, server) = serve_sse_sequence(vec![tool_sse, final_sse]).await;
    let repository = init_repository("continuation-ephemeral-plan");
    let store = StudioStore::open_memory().await.unwrap();
    let run = persisted_repository_run(&store, &repository, "ephemeral-plan").await;
    let home = std::env::temp_dir().join(format!("pure-ephemeral-plan-home-{}", unique_id()));
    let config_store = ConfigStore::new(crate::config::ConfigPaths::from_home(&home));
    config_store.save(&test_config(base_url)).unwrap();
    let runtime = StudioRuntime::new(store.clone(), config_store);

    runtime
        .request_task_continuation(run.id, ContinuationReason::Recovery)
        .await;
    server.await.unwrap();
    wait_for_no_active_turn(&runtime).await;

    let events = store
        .load_studio_events(&run.session_id, None, None)
        .await
        .unwrap();
    assert!(events.iter().any(|event| matches!(
        &event.kind,
        StudioEventKind::MessagePartUpdated { part }
            if part.part_type == StudioPartType::Plan
                && part.text.contains("Ephemeral Plan")
    )));
    assert!(
        !store
            .list_pending_interactions(&run.session_id)
            .await
            .unwrap()
            .iter()
            .any(|interaction| interaction.kind == InteractionKind::PlanConfirmation)
    );
    assert!(!events.iter().any(|event| matches!(
        &event.kind,
        StudioEventKind::PlanLifecycleChanged { event }
            if event.state == PlanLifecycleState::PendingConfirmation
    )));
    remove_repository(repository);
    let _ = tokio::fs::remove_dir_all(home).await;
}

#[tokio::test]
async fn recovery_launches_once_after_ready_and_skips_head_drift() {
    let recovered_repository = init_repository("continuation-recovered");
    let drifted_repository = init_repository("continuation-drifted");
    let store = StudioStore::open_memory().await.unwrap();
    let recovered = persisted_repository_run(&store, &recovered_repository, "recovered").await;
    let drifted = persisted_repository_run(&store, &drifted_repository, "drifted").await;
    std::fs::write(drifted_repository.join("drift.txt"), "drift\n").unwrap();
    git(&drifted_repository, &["add", "drift.txt"]);
    git(&drifted_repository, &["commit", "-m", "drift"]);
    let runtime_state = StudioRuntimeState::new();
    let launcher = RecordingLauncher::successful().observing(runtime_state.clone());
    let home = std::env::temp_dir().join(format!("pure-recovery-home-{}", unique_id()));
    let runtime = StudioRuntime::with_runtime_state_and_continuation_launcher(
        store.clone(),
        ConfigStore::new(crate::config::ConfigPaths::from_home(home)),
        runtime_state,
        Arc::new(launcher.clone()),
    );

    let initialized = runtime.initialize_runtime().await.unwrap();
    let repeated = runtime.initialize_runtime().await.unwrap();
    let launches = launcher.wait_for_count(1).await;
    tokio::task::yield_now().await;

    assert_eq!(initialized.status, StudioRuntimeStatus::Ready);
    assert_eq!(repeated.status, StudioRuntimeStatus::Ready);
    assert_eq!(launches.len(), 1);
    assert_eq!(launches[0].request.task_run_id, recovered.id);
    assert_eq!(launches[0].request.reason, ContinuationReason::Recovery);
    assert_eq!(
        launcher.observed_statuses.lock().await.as_slice(),
        &[StudioRuntimeStatus::Ready]
    );
    assert_eq!(
        store
            .read_task_run(&drifted.id)
            .await
            .unwrap()
            .unwrap()
            .phase,
        TaskRunPhase::Blocked
    );

    drop(runtime);
    remove_repository(recovered_repository);
    remove_repository(drifted_repository);
}

#[tokio::test]
async fn concurrent_initialization_recovers_active_run_once_without_blocking_it() {
    let repository = init_repository("continuation-concurrent-initialize");
    let store = StudioStore::open_memory().await.unwrap();
    let run = persisted_repository_run(&store, &repository, "concurrent-initialize").await;
    let runtime_state = StudioRuntimeState::new();
    let launcher = RecordingLauncher::successful().observing(runtime_state.clone());
    let home = std::env::temp_dir().join(format!("pure-concurrent-home-{}", unique_id()));
    let mut runtime = StudioRuntime::with_runtime_state_and_continuation_launcher(
        store.clone(),
        ConfigStore::new(crate::config::ConfigPaths::from_home(home)),
        runtime_state,
        Arc::new(launcher.clone()),
    );
    runtime.initialization_entry_barrier = Some(Arc::new(tokio::sync::Barrier::new(2)));
    let first = {
        let runtime = runtime.clone();
        tokio::spawn(async move { runtime.initialize_runtime().await })
    };
    let second = {
        let runtime = runtime.clone();
        tokio::spawn(async move { runtime.initialize_runtime().await })
    };

    let (first, second) = tokio::join!(first, second);
    let first = first.unwrap().unwrap();
    let second = second.unwrap().unwrap();
    let launches = launcher.wait_for_count(1).await;

    assert_eq!(first.status, StudioRuntimeStatus::Ready);
    assert_eq!(second.status, StudioRuntimeStatus::Ready);
    assert_eq!(launches.len(), 1);
    assert_eq!(launches[0].request.task_run_id, run.id);
    assert_eq!(
        store.read_task_run(&run.id).await.unwrap().unwrap().phase,
        TaskRunPhase::Implementing
    );
    drop(runtime);
    remove_repository(repository);
}

#[tokio::test]
async fn shutdown_clears_pending_and_same_runtime_recovers_once() {
    let repository = init_repository("continuation-shutdown-pending");
    let store = StudioStore::open_memory().await.unwrap();
    let run = persisted_repository_run(&store, &repository, "shutdown-pending").await;
    let launcher = RecordingLauncher::successful();
    let runtime = test_runtime(store.clone(), launcher.clone());
    let token = tokio_util::sync::CancellationToken::new();
    runtime
        .active_turns
        .insert(
            run.session_id.clone(),
            "turn-competing".to_string(),
            token.clone(),
        )
        .await
        .unwrap();
    runtime
        .request_task_continuation(run.id.clone(), ContinuationReason::AgentTerminal)
        .await;

    let shutdown_runtime = runtime.clone();
    let shutdown = tokio::spawn(async move { shutdown_runtime.shutdown_runtime().await });
    token.cancelled().await;
    runtime
        .active_turn_removed(&run.session_id, "turn-competing")
        .await;
    let stopped = shutdown.await.unwrap().unwrap();
    assert_eq!(stopped.status, StudioRuntimeStatus::Stopped);
    assert!(
        !runtime
            .continuation_scheduler
            .has_session(&run.session_id)
            .await
    );
    assert!(launcher.launches.lock().await.is_empty());

    let ready = runtime.initialize_runtime().await.unwrap();
    let launches = launcher.wait_for_count(1).await;
    assert_eq!(ready.status, StudioRuntimeStatus::Ready);
    assert_eq!(launches.len(), 1);
    assert_eq!(launches[0].request.reason, ContinuationReason::Recovery);
    remove_repository(repository);
}

#[tokio::test]
async fn shutdown_discards_claimed_continuation_before_callback() {
    let (store, run, _) = continuation_fixture("shutdown-claimed").await;
    let launcher = RecordingLauncher::successful();
    let mut runtime = test_runtime(store, launcher.clone());
    let prepared = ContinuationTestBarrier::new();
    runtime.continuation_pre_submit_barrier = Some(prepared.clone());

    runtime
        .request_task_continuation(run.id.clone(), ContinuationReason::Recovery)
        .await;
    prepared.wait_until_entered().await;
    let stopped = runtime.shutdown_runtime().await.unwrap();
    prepared.release().await;
    tokio::time::timeout(TEST_RUNTIME_TIMEOUT, async {
        while runtime
            .continuation_scheduler
            .has_session(&run.session_id)
            .await
        {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();

    assert_eq!(stopped.status, StudioRuntimeStatus::Stopped);
    assert!(launcher.launches.lock().await.is_empty());
}

#[tokio::test]
async fn failed_runtime_shutdown_clears_turns_scheduler_and_recovers_cleanly() {
    let repository = init_repository("continuation-failed-shutdown");
    let store = StudioStore::open_memory().await.unwrap();
    let run = persisted_repository_run(&store, &repository, "failed-shutdown").await;
    let launcher = RecordingLauncher::successful();
    let runtime = test_runtime(store, launcher.clone());
    let token = tokio_util::sync::CancellationToken::new();
    runtime
        .active_turns
        .insert(
            run.session_id.clone(),
            "turn-before-failure".to_string(),
            token.clone(),
        )
        .await
        .unwrap();
    runtime
        .request_task_continuation(run.id, ContinuationReason::AgentTerminal)
        .await;
    runtime
        .runtime_state
        .transition(
            StudioRuntimeStatus::Failed,
            Some("injected runtime failure".to_string()),
        )
        .unwrap();

    let shutdown_runtime = runtime.clone();
    let shutdown = tokio::spawn(async move { shutdown_runtime.shutdown_runtime().await });
    token.cancelled().await;
    runtime
        .active_turn_removed(&run.session_id, "turn-before-failure")
        .await;
    let stopped = shutdown.await.unwrap().unwrap();

    assert_eq!(stopped.status, StudioRuntimeStatus::Stopped);
    assert!(token.is_cancelled());
    assert!(!runtime.active_turns.contains(&run.session_id).await);
    assert!(
        !runtime
            .continuation_scheduler
            .has_session(&run.session_id)
            .await
    );
    let ready = runtime.initialize_runtime().await.unwrap();
    let launches = launcher.wait_for_count(1).await;
    assert_eq!(ready.status, StudioRuntimeStatus::Ready);
    assert_eq!(launches.len(), 1);
    assert_eq!(launches[0].request.reason, ContinuationReason::Recovery);
    remove_repository(repository);
}

#[tokio::test]
async fn shutdown_waits_for_background_turn_before_same_runtime_recovery() {
    let old_sse = concat!(
        "data: {\"type\":\"response.output_text.delta\",\"item_id\":\"msg_old\",\"delta\":\"old done\"}\n\n",
        "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_old\",\"usage\":{\"input_tokens\":1,\"output_tokens\":1,\"total_tokens\":2}}}\n\n",
        "data: [DONE]\n\n"
    )
    .to_string();
    let (base_url, server, recovery_accepted, release_recovery) =
        serve_sse_then_delayed_sse(old_sse).await;
    let repository = init_repository("continuation-stale-background-restart");
    let store = StudioStore::open_memory().await.unwrap();
    let run = persisted_repository_run(&store, &repository, "stale-background-restart").await;
    let home = std::env::temp_dir().join(format!("pure-stale-background-home-{}", unique_id()));
    let config_store = ConfigStore::new(crate::config::ConfigPaths::from_home(&home));
    config_store.save(&test_config(base_url)).unwrap();
    let mut runtime = StudioRuntime::new(store.clone(), config_store);
    let completion = PromptCompletionTestBarrier::new();
    runtime.prompt_completion_barrier = Some(completion.clone());
    let history_before = store
        .load_core_session(&run.session_id)
        .await
        .unwrap()
        .messages()
        .to_vec();
    let skills_before = store
        .list_session_skill_names(&run.session_id)
        .await
        .unwrap();
    let old_turn = runtime
        .submit_prompt(StudioSubmitPromptRequest {
            session_id: run.session_id.clone(),
            prompt: "old background turn".to_string(),
            attachment_ids: Vec::new(),
            options: StudioSubmitPromptOptions {
                user_prompt: StudioUserPromptPresentation::Normal,
                lifecycle: Some(StudioPlanImplementationLifecycle {
                    session_id: run.session_id.clone(),
                    plan_id: "plan-old-generation".to_string(),
                }),
                history_policy: PromptHistoryPolicy::Persist,
            },
        })
        .await
        .unwrap();
    completion.wait_until_entered().await;
    let session_runtime_before = store.load_session_runtime(&run.session_id).await.unwrap();

    let shutdown_runtime = runtime.clone();
    let shutdown = tokio::spawn(async move { shutdown_runtime.shutdown_runtime().await });
    tokio::time::timeout(TEST_RUNTIME_TIMEOUT, async {
        while runtime.runtime_snapshot().status == StudioRuntimeStatus::Ready {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert_eq!(
        runtime.runtime_snapshot().status,
        StudioRuntimeStatus::ShuttingDown
    );
    assert!(!shutdown.is_finished());

    let initialize_runtime = runtime.clone();
    let initialize = tokio::spawn(async move { initialize_runtime.initialize_runtime().await });
    tokio::task::yield_now().await;
    assert!(!initialize.is_finished());

    completion.release().await;
    completion.wait_until_finished().await;
    assert_eq!(
        shutdown.await.unwrap().unwrap().status,
        StudioRuntimeStatus::Stopped
    );
    assert_eq!(
        initialize.await.unwrap().unwrap().status,
        StudioRuntimeStatus::Ready
    );
    tokio::time::timeout(TEST_RUNTIME_TIMEOUT, recovery_accepted)
        .await
        .unwrap()
        .unwrap();
    let recovered_turn = runtime.runtime_snapshot().active_turns[0].clone();
    assert_ne!(recovered_turn.turn_id, old_turn.turn_id);
    let mut current_interaction = pending_interaction(
        "interaction-current-generation",
        &run.session_id,
        InteractionKind::UserInput,
        InteractionPayload::UserInput {
            questions: Vec::new(),
        },
    );
    current_interaction.scope.turn_id = recovered_turn.turn_id.clone();
    store
        .upsert_interaction(&current_interaction)
        .await
        .unwrap();
    let events_before = store
        .load_studio_events(&run.session_id, None, None)
        .await
        .unwrap();
    let old_turn_events_before = events_before
        .iter()
        .filter(|event| event.turn_id.as_deref() == Some(old_turn.turn_id.as_str()))
        .count();
    let old_lifecycle_events_before = events_before
        .iter()
        .filter(|event| {
            matches!(
                &event.kind,
                StudioEventKind::PlanLifecycleChanged { event }
                    if event.plan_id == "plan-old-generation"
            )
        })
        .count();

    assert_eq!(
        runtime.runtime_snapshot().active_turns,
        vec![recovered_turn]
    );
    assert!(
        runtime
            .continuation_scheduler
            .has_session(&run.session_id)
            .await
    );
    assert_eq!(
        store
            .read_interaction("interaction-current-generation")
            .await
            .unwrap()
            .unwrap()
            .status,
        InteractionStatus::Pending
    );
    let events_after = store
        .load_studio_events(&run.session_id, None, None)
        .await
        .unwrap();
    assert_eq!(
        events_after
            .iter()
            .filter(|event| event.turn_id.as_deref() == Some(old_turn.turn_id.as_str()))
            .count(),
        old_turn_events_before
    );
    assert_eq!(
        events_after
            .iter()
            .filter(|event| matches!(
                &event.kind,
                StudioEventKind::PlanLifecycleChanged { event }
                    if event.plan_id == "plan-old-generation"
            ))
            .count(),
        old_lifecycle_events_before
    );
    assert_eq!(
        store
            .load_message_parts(&run.session_id)
            .await
            .unwrap()
            .into_iter()
            .filter(|record| record.part.text == "继续任务")
            .count(),
        1
    );
    assert_eq!(
        store
            .load_core_session(&run.session_id)
            .await
            .unwrap()
            .messages(),
        history_before
    );
    assert_eq!(
        store
            .list_session_skill_names(&run.session_id)
            .await
            .unwrap(),
        skills_before
    );
    assert_eq!(
        store.load_session_runtime(&run.session_id).await.unwrap(),
        session_runtime_before
    );
    assert!(
        !store
            .list_pending_interactions(&run.session_id)
            .await
            .unwrap()
            .iter()
            .any(|interaction| interaction.kind == InteractionKind::PlanConfirmation)
    );

    let _ = release_recovery.send(());
    wait_for_no_active_turn(&runtime).await;
    server.await.unwrap();
    remove_repository(repository);
    let _ = tokio::fs::remove_dir_all(home).await;
}

#[tokio::test]
async fn next_turn_interaction_is_inserted_only_after_previous_cleanup_and_removal() {
    let sse_body = concat!(
        "data: {\"type\":\"response.output_text.delta\",\"item_id\":\"msg_old\",\"delta\":\"done\"}\n\n",
        "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_old\",\"usage\":{\"input_tokens\":1,\"output_tokens\":1,\"total_tokens\":2}}}\n\n",
        "data: [DONE]\n\n"
    )
    .to_string();
    let (base_url, server) = serve_sse_once(sse_body).await;
    let repository = init_repository("continuation-finalization-cleanup");
    let store = StudioStore::open_memory().await.unwrap();
    let run = persisted_repository_run(&store, &repository, "finalization-cleanup").await;
    let home = std::env::temp_dir().join(format!("pure-cleanup-home-{}", unique_id()));
    let config_store = ConfigStore::new(crate::config::ConfigPaths::from_home(&home));
    config_store.save(&test_config(base_url)).unwrap();
    let mut runtime = StudioRuntime::new(store.clone(), config_store);
    let finalization = ContinuationTestBarrier::new();
    runtime.prompt_finalization_barrier = Some(finalization.clone());

    let old_turn = runtime
        .submit_prompt(StudioSubmitPromptRequest {
            session_id: run.session_id.clone(),
            prompt: "old background turn".to_string(),
            attachment_ids: Vec::new(),
            options: StudioSubmitPromptOptions::default(),
        })
        .await
        .unwrap();
    finalization.wait_until_entered().await;

    let contender_runtime = runtime.clone();
    let contender_store = store.clone();
    let contender_session_id = run.session_id.clone();
    let (attempted_tx, attempted_rx) = tokio::sync::oneshot::channel();
    let contender = tokio::spawn(async move {
        let mut attempted_tx = Some(attempted_tx);
        let first_insert = contender_runtime
            .active_turns
            .insert(
                contender_session_id.clone(),
                "turn-next-generation".to_string(),
                tokio_util::sync::CancellationToken::new(),
            )
            .await;
        let inserted_before_release = match first_insert {
            Ok(()) => true,
            Err(error)
                if error
                    .downcast_ref::<SessionAlreadyHasActiveTurn>()
                    .is_some() =>
            {
                false
            }
            Err(error) => panic!("next turn insertion failed: {error:#}"),
        };
        if !inserted_before_release {
            if let Some(sender) = attempted_tx.take() {
                let _ = sender.send(false);
            }
            loop {
                let insert = contender_runtime
                    .active_turns
                    .insert(
                        contender_session_id.clone(),
                        "turn-next-generation".to_string(),
                        tokio_util::sync::CancellationToken::new(),
                    )
                    .await;
                match insert {
                    Ok(()) => break,
                    Err(error)
                        if error
                            .downcast_ref::<SessionAlreadyHasActiveTurn>()
                            .is_some() =>
                    {
                        tokio::task::yield_now().await;
                    }
                    Err(error) => panic!("next turn insertion failed: {error:#}"),
                }
            }
        }
        let mut interaction = pending_interaction(
            "interaction-next-generation",
            &contender_session_id,
            InteractionKind::UserInput,
            InteractionPayload::UserInput {
                questions: Vec::new(),
            },
        );
        interaction.scope.turn_id = "turn-next-generation".to_string();
        contender_store
            .upsert_interaction(&interaction)
            .await
            .unwrap();
        if let Some(sender) = attempted_tx {
            let _ = sender.send(true);
        }
    });

    let _ = attempted_rx.await.unwrap();
    finalization.release().await;
    tokio::time::timeout(TEST_RUNTIME_TIMEOUT, contender)
        .await
        .unwrap()
        .unwrap();
    tokio::time::timeout(TEST_RUNTIME_TIMEOUT, async {
        loop {
            let finalized = store
                .load_studio_events(&run.session_id, None, None)
                .await
                .unwrap()
                .iter()
                .any(|event| {
                    matches!(
                        &event.kind,
                        StudioEventKind::TurnChanged { turn }
                            if turn.turn_id == old_turn.turn_id
                                && turn.status == StudioTurnStatus::Completed
                    )
                });
            if finalized {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert_eq!(
        store
            .read_interaction("interaction-next-generation")
            .await
            .unwrap()
            .unwrap()
            .status,
        InteractionStatus::Pending
    );

    runtime
        .active_turn_removed(&run.session_id, "turn-next-generation")
        .await;
    server.await.unwrap();
    remove_repository(repository);
    let _ = tokio::fs::remove_dir_all(home).await;
}

fn continuation_request(
    task_run_id: &str,
    session_id: &str,
    reason: ContinuationReason,
) -> ContinuationRequest {
    ContinuationRequest {
        task_run_id: task_run_id.to_string(),
        session_id: session_id.to_string(),
        reason,
    }
}

#[derive(Clone)]
struct RecordingLauncher {
    launches: Arc<Mutex<Vec<ContinuationLaunch>>>,
    error: Option<String>,
    runtime_state: Option<StudioRuntimeState>,
    observed_statuses: Arc<Mutex<Vec<StudioRuntimeStatus>>>,
}

impl RecordingLauncher {
    fn successful() -> Self {
        Self {
            launches: Arc::default(),
            error: None,
            runtime_state: None,
            observed_statuses: Arc::default(),
        }
    }

    fn failing(error: &str) -> Self {
        Self {
            launches: Arc::default(),
            error: Some(error.to_string()),
            runtime_state: None,
            observed_statuses: Arc::default(),
        }
    }

    fn observing(mut self, runtime_state: StudioRuntimeState) -> Self {
        self.runtime_state = Some(runtime_state);
        self
    }

    async fn wait_for_count(&self, count: usize) -> Vec<ContinuationLaunch> {
        tokio::time::timeout(TEST_RUNTIME_TIMEOUT, async {
            loop {
                let launches = self.launches.lock().await.clone();
                if launches.len() >= count {
                    break launches;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap()
    }
}

impl ContinuationLauncher for RecordingLauncher {
    fn launch(
        &self,
        launch: ContinuationLaunch,
    ) -> Pin<Box<dyn Future<Output = Result<()>> + Send + 'static>> {
        let launches = self.launches.clone();
        let error = self.error.clone();
        let runtime_state = self.runtime_state.clone();
        let observed_statuses = self.observed_statuses.clone();
        Box::pin(async move {
            if let Some(runtime_state) = runtime_state {
                observed_statuses
                    .lock()
                    .await
                    .push(runtime_state.snapshot().status);
            }
            launches.lock().await.push(launch);
            match error {
                Some(error) => Err(anyhow!(error)),
                None => Ok(()),
            }
        })
    }
}

fn test_runtime(store: StudioStore, launcher: RecordingLauncher) -> StudioRuntime {
    let home = std::env::temp_dir().join(format!(
        "pure-continuation-runtime-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    StudioRuntime::with_runtime_state_and_continuation_launcher(
        store,
        ConfigStore::new(crate::config::ConfigPaths::from_home(home)),
        StudioRuntimeState::ready(),
        Arc::new(launcher),
    )
}

async fn continuation_fixture(
    name: &str,
) -> (
    StudioStore,
    crate::studio::task_coordinator::TaskRunRecord,
    crate::studio::task_coordinator::AgentOutcomeRecord,
) {
    let store = StudioStore::open_memory().await.unwrap();
    let root = format!("C:/work/{name}");
    let project = store.upsert_project(&root).await.unwrap();
    let session = store
        .create_session(&project.id, name, CompileMode::Task)
        .await
        .unwrap();
    let (run, _) = store
        .create_task_run_with_lease(CreateTaskRun {
            session_id: session.id,
            phase: TaskRunPhase::Implementing,
            plan: "Implement continuation".to_string(),
            workspace_root: root.clone(),
            git_common_dir: format!("{root}/.git"),
            branch: format!("branch-{name}"),
            head_commit: "1111111".to_string(),
        })
        .await
        .unwrap();
    let work_unit = store
        .create_work_unit(CreateWorkUnit {
            task_run_id: run.id.clone(),
            title: "Implement".to_string(),
            owned_paths: vec!["code/pl-core/**".to_string()],
            base_commit: run.base_commit.clone(),
            worktree_path: format!("{root}/.pure/worktrees/agent-1"),
            branch: format!("pure-task-{name}-agent-1"),
            attempt: 1,
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
    let outcome = store
        .create_agent_outcome(CreateAgentOutcome {
            task_run_id: run.id.clone(),
            work_unit_id: Some(work_unit.id),
            agent_id: "agent-1".to_string(),
            owner_path: "root".to_string(),
            initiated_by: "planner".to_string(),
            requested_by_call_id: "call-agent".to_string(),
            role: "executor".to_string(),
            status: AgentOutcomeStatus::Running,
            attempt: 1,
        })
        .await
        .unwrap();
    (store, run, outcome)
}

fn terminal_event(agent_id: &str) -> pl_trace::AgentEvent {
    pl_trace::AgentEvent::AgentStateChanged {
        id: agent_id.to_string(),
        path: "/root/agent-1".to_string(),
        parent_path: Some("/root".to_string()),
        role: "executor".to_string(),
        task: "Implement".to_string(),
        status: AgentStatus::Errored,
        summary: Some("agent failed".to_string()),
        depth: 1,
        error: Some("boom".to_string()),
        reason: None,
        budget_limit_kind: None,
        budget_usage: None,
        updated_at: 10,
    }
}

async fn persisted_repository_run(
    store: &StudioStore,
    repository: &Path,
    name: &str,
) -> crate::studio::task_coordinator::TaskRunRecord {
    let project = store
        .upsert_project(repository.to_string_lossy().as_ref())
        .await
        .unwrap();
    let session = store
        .create_session(&project.id, name, CompileMode::Task)
        .await
        .unwrap();
    let head = git_output(repository, &["rev-parse", "HEAD"]);
    let branch = git_output(repository, &["branch", "--show-current"]);
    let common_dir = git_output(repository, &["rev-parse", "--git-common-dir"]);
    let common_dir = std::fs::canonicalize(repository.join(common_dir)).unwrap();
    store
        .create_task_run_with_lease(CreateTaskRun {
            session_id: session.id,
            phase: TaskRunPhase::Implementing,
            plan: "Recover task".to_string(),
            workspace_root: std::fs::canonicalize(repository)
                .unwrap()
                .to_string_lossy()
                .to_string(),
            git_common_dir: common_dir.to_string_lossy().to_string(),
            branch,
            head_commit: head,
        })
        .await
        .unwrap()
        .0
}

fn init_repository(name: &str) -> PathBuf {
    let repository = std::env::temp_dir().join(format!("pure-{name}-{}", unique_id()));
    std::fs::create_dir_all(&repository).unwrap();
    git(&repository, &["init", "-b", "main"]);
    git(
        &repository,
        &["config", "user.email", "tests@example.invalid"],
    );
    git(&repository, &["config", "user.name", "Pure Tests"]);
    std::fs::write(repository.join("README.md"), "initial\n").unwrap();
    git(&repository, &["add", "README.md"]);
    git(&repository, &["commit", "-m", "initial"]);
    repository
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
        "git {args:?} failed: {}",
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
    assert!(output.status.success());
    String::from_utf8(output.stdout).unwrap().trim().to_string()
}

fn remove_repository(repository: PathBuf) {
    let _ = std::fs::remove_dir_all(repository);
}

fn unique_id() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos()
}

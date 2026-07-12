use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::process::Command;
use std::sync::Arc;

use anyhow::{Result, anyhow};
use pl_protocol::{
    AgentStatus, InteractionResolution, StudioEventKind, StudioMessageRole, StudioMessageStatus,
    StudioPartType,
};
use tokio::sync::Mutex;

use super::*;
use crate::studio::active_turns::SessionAlreadyHasActiveTurn;
use crate::studio::runtime::continuation::{
    ContinuationLaunch, ContinuationLauncher, ContinuationReason, ContinuationRequest,
    ContinuationScheduler, ContinuationTestBarrier, SessionTurnState,
};
use crate::studio::task_coordinator::{
    AgentDelivery, AgentOutcomeStatus, AgentWorktreeDelivery, BeginTaskMerge, ConflictEntry,
    ConflictKind, ConflictManifest, ConflictTaskMerge, CreateAgentOutcome, CreateTaskRun,
    CreateWorkUnit, TaskRunPhase, WorkUnitStatus,
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
async fn active_turn_removal_claims_durable_merge_conflict_and_launches_exactly_once() {
    let (store, run) = merge_conflict_continuation_fixture("merge-conflict-removal").await;
    let launcher = RecordingLauncher::successful();
    let runtime = test_runtime(store.clone(), launcher.clone());
    runtime
        .active_turns
        .insert(
            run.session_id.clone(),
            "turn-before-conflict".to_string(),
            tokio_util::sync::CancellationToken::new(),
        )
        .await
        .unwrap();

    assert!(
        runtime
            .active_turn_removed(&run.session_id, "turn-before-conflict")
            .await
    );
    let launches = launcher.wait_for_count(1).await;
    assert_eq!(launches.len(), 1);
    assert_eq!(launches[0].request.task_run_id, run.id);
    assert_eq!(
        launches[0].request.reason,
        ContinuationReason::MergeConflict
    );
    assert_eq!(
        store
            .claim_merge_conflict_continuation(&run.session_id)
            .await
            .unwrap(),
        None
    );
    assert!(
        !runtime
            .active_turn_removed(&run.session_id, "turn-before-conflict")
            .await
    );
    tokio::task::yield_now().await;
    assert_eq!(launcher.launches.lock().await.len(), 1);
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
async fn queued_terminal_event_persists_for_stale_prompt_without_side_effects() {
    let (store, run, outcome) = continuation_fixture("stale-prompt-terminal").await;
    let launcher = RecordingLauncher::successful();
    let runtime = test_runtime(store.clone(), launcher.clone());
    let stale_turn_id = "turn-stale-terminal";
    let current_turn_id = "turn-current";
    runtime
        .active_turns
        .insert(
            run.session_id.clone(),
            stale_turn_id.to_string(),
            tokio_util::sync::CancellationToken::new(),
        )
        .await
        .unwrap();
    let (event_tx, event_rx) = tokio::sync::broadcast::channel(8);
    event_tx.send(terminal_event(&outcome.agent_id)).unwrap();
    drop(event_tx);

    runtime
        .active_turn_removed(&run.session_id, stale_turn_id)
        .await;
    runtime
        .active_turns
        .insert(
            run.session_id.clone(),
            current_turn_id.to_string(),
            tokio_util::sync::CancellationToken::new(),
        )
        .await
        .unwrap();
    let mut studio_events = runtime.events().subscribe();
    runtime
        .drain_prompt_agent_events(run.session_id.clone(), stale_turn_id.to_string(), event_rx)
        .await;

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
    assert_eq!(
        store.list_work_units(&run.id).await.unwrap()[0].status,
        WorkUnitStatus::Failed
    );
    assert!(launcher.launches.lock().await.is_empty());
    assert!(
        runtime
            .continuation_scheduler
            .has_session(&run.session_id)
            .await
    );
    runtime
        .active_turn_removed(&run.session_id, current_turn_id)
        .await;
    let launches = launcher.wait_for_count(1).await;
    assert_eq!(launches.len(), 1);
    assert_eq!(launches[0].request.task_run_id, run.id);
    assert_eq!(
        launches[0].request.reason,
        ContinuationReason::AgentTerminal
    );

    let (duplicate_tx, duplicate_rx) = tokio::sync::broadcast::channel(8);
    duplicate_tx
        .send(terminal_event(&outcome.agent_id))
        .unwrap();
    drop(duplicate_tx);
    runtime
        .drain_prompt_agent_events(
            run.session_id.clone(),
            stale_turn_id.to_string(),
            duplicate_rx,
        )
        .await;
    tokio::task::yield_now().await;
    assert_eq!(launcher.launches.lock().await.len(), 1);
    assert!(
        store
            .load_studio_events(&run.session_id, None, None)
            .await
            .unwrap()
            .is_empty()
    );
    assert!(matches!(
        studio_events.try_recv(),
        Err(tokio::sync::broadcast::error::TryRecvError::Empty)
    ));
}

#[tokio::test]
async fn queued_terminal_event_persists_during_shutdown_without_side_effects() {
    let (store, run, outcome) = continuation_fixture("shutdown-prompt-terminal").await;
    let launcher = RecordingLauncher::successful();
    let mut runtime = test_runtime(store.clone(), launcher.clone());
    let shutdown_cancelled = ContinuationTestBarrier::new();
    runtime.shutdown_after_cancel_barrier = Some(shutdown_cancelled.clone());
    let turn_id = "turn-shutdown-terminal";
    runtime
        .active_turns
        .insert(
            run.session_id.clone(),
            turn_id.to_string(),
            tokio_util::sync::CancellationToken::new(),
        )
        .await
        .unwrap();
    let (event_tx, event_rx) = tokio::sync::broadcast::channel(8);
    event_tx.send(terminal_event(&outcome.agent_id)).unwrap();
    drop(event_tx);
    let mut studio_events = runtime.events().subscribe();
    let stopped = tokio::time::timeout(TEST_RUNTIME_TIMEOUT, async {
        let shutdown_runtime = runtime.clone();
        let shutdown = tokio::spawn(async move { shutdown_runtime.shutdown_runtime().await });
        shutdown_cancelled.wait_until_entered().await;
        shutdown_cancelled.release().await;
        runtime
            .drain_prompt_agent_events(run.session_id.clone(), turn_id.to_string(), event_rx)
            .await;
        runtime.active_turn_removed(&run.session_id, turn_id).await;
        shutdown.await.unwrap().unwrap()
    })
    .await
    .unwrap();
    assert_eq!(stopped.status, StudioRuntimeStatus::Stopped);

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
    assert_eq!(
        store.list_work_units(&run.id).await.unwrap()[0].status,
        WorkUnitStatus::Failed
    );
    assert!(launcher.launches.lock().await.is_empty());
    assert!(
        store
            .load_studio_events(&run.session_id, None, None)
            .await
            .unwrap()
            .is_empty()
    );
    assert!(matches!(
        studio_events.try_recv(),
        Err(tokio::sync::broadcast::error::TryRecvError::Empty)
    ));
}

#[tokio::test]
async fn stale_prompt_terminal_persistence_failure_blocks_run_without_diagnostic() {
    let (store, run, outcome) = continuation_fixture("stale-terminal-failure").await;
    let launcher = RecordingLauncher::successful();
    let runtime = test_runtime(store.clone(), launcher.clone());
    let stale_turn_id = "turn-stale-failure";
    runtime
        .active_turns
        .insert(
            run.session_id.clone(),
            stale_turn_id.to_string(),
            tokio_util::sync::CancellationToken::new(),
        )
        .await
        .unwrap();
    let (event_tx, event_rx) = tokio::sync::broadcast::channel(8);
    event_tx.send(terminal_event(&outcome.agent_id)).unwrap();
    drop(event_tx);
    runtime
        .active_turn_removed(&run.session_id, stale_turn_id)
        .await;
    runtime
        .active_turns
        .insert(
            run.session_id.clone(),
            "turn-current".to_string(),
            tokio_util::sync::CancellationToken::new(),
        )
        .await
        .unwrap();
    store
        .execute_test_sql("ALTER TABLE agent_outcomes RENAME TO unavailable_agent_outcomes")
        .await;
    let mut studio_events = runtime.events().subscribe();

    runtime
        .drain_prompt_agent_events(run.session_id.clone(), stale_turn_id.to_string(), event_rx)
        .await;

    let blocked = store.read_task_run(&run.id).await.unwrap().unwrap();
    assert_eq!(blocked.phase, TaskRunPhase::Blocked);
    assert!(
        blocked
            .status_message
            .as_deref()
            .is_some_and(|message| message.contains("terminal agent state persistence failed"))
    );
    assert!(launcher.launches.lock().await.is_empty());
    assert!(
        store
            .load_studio_events(&run.session_id, None, None)
            .await
            .unwrap()
            .is_empty()
    );
    assert!(matches!(
        studio_events.try_recv(),
        Err(tokio::sync::broadcast::error::TryRecvError::Empty)
    ));
}

#[tokio::test]
async fn lagged_standalone_drain_still_emits_stale() {
    let store = StudioStore::open_memory().await.unwrap();
    let runtime = test_runtime(store, RecordingLauncher::successful());
    let mut studio_events = runtime.events().subscribe();

    runtime
        .drain_agent_events(
            "standalone-lagged".to_string(),
            lagged_agent_event_receiver(),
        )
        .await;

    let mut saw_stale = false;
    while let Ok(event) = studio_events.try_recv() {
        saw_stale |= matches!(event.kind, StudioEventKind::Stale { .. });
    }
    assert!(saw_stale);
}

#[tokio::test]
async fn lagged_stale_and_shutdown_prompt_drains_emit_no_stale() {
    let stale_store = StudioStore::open_memory().await.unwrap();
    let stale_runtime = test_runtime(stale_store, RecordingLauncher::successful());
    let mut stale_events = stale_runtime.events().subscribe();
    stale_runtime
        .drain_prompt_agent_events(
            "stale-lagged".to_string(),
            "turn-stale".to_string(),
            lagged_agent_event_receiver(),
        )
        .await;
    assert!(matches!(
        stale_events.try_recv(),
        Err(tokio::sync::broadcast::error::TryRecvError::Empty)
    ));

    let shutdown_store = StudioStore::open_memory().await.unwrap();
    let shutdown_runtime = test_runtime(shutdown_store, RecordingLauncher::successful());
    shutdown_runtime
        .active_turns
        .insert(
            "shutdown-lagged".to_string(),
            "turn-shutdown".to_string(),
            tokio_util::sync::CancellationToken::new(),
        )
        .await
        .unwrap();
    shutdown_runtime
        .runtime_state
        .transition(StudioRuntimeStatus::ShuttingDown, None)
        .unwrap();
    let mut shutdown_events = shutdown_runtime.events().subscribe();
    shutdown_runtime
        .drain_prompt_agent_events(
            "shutdown-lagged".to_string(),
            "turn-shutdown".to_string(),
            lagged_agent_event_receiver(),
        )
        .await;
    assert!(matches!(
        shutdown_events.try_recv(),
        Err(tokio::sync::broadcast::error::TryRecvError::Empty)
    ));
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
async fn spawned_claim_from_previous_lifecycle_cannot_submit_after_restart() {
    let repository = init_repository("continuation-lifecycle-epoch");
    let store = StudioStore::open_memory().await.unwrap();
    let run = persisted_repository_run(&store, &repository, "lifecycle-epoch").await;
    let launcher = RecordingLauncher::successful();
    let mut runtime = test_runtime(store, launcher.clone());
    let prepared = ContinuationTestBarrier::new();
    let lifecycle_entered = ContinuationTestBarrier::new();
    runtime.continuation_pre_submit_barrier = Some(prepared.clone());
    runtime.continuation_post_lifecycle_barrier = Some(lifecycle_entered.clone());

    runtime
        .request_task_continuation(run.id.clone(), ContinuationReason::AgentTerminal)
        .await;
    prepared.wait_until_entered().await;

    assert_eq!(
        runtime.shutdown_runtime().await.unwrap().status,
        StudioRuntimeStatus::Stopped
    );
    assert_eq!(
        runtime.initialize_runtime().await.unwrap().status,
        StudioRuntimeStatus::Ready
    );
    let recovery_launches = launcher.wait_for_count(1).await;
    assert_eq!(
        recovery_launches[0].request.reason,
        ContinuationReason::Recovery
    );

    prepared.release().await;
    lifecycle_entered.wait_until_entered().await;
    lifecycle_entered.release().await;
    let lifecycle_guard = runtime.lifecycle_lock.lock().await;
    drop(lifecycle_guard);

    assert_eq!(launcher.launches.lock().await.len(), 1);
    remove_repository(repository);
}

#[tokio::test]
async fn old_prompt_agent_event_cannot_cross_shutdown_epoch() {
    tokio::time::timeout(TEST_RUNTIME_TIMEOUT, async {
        let repository = init_repository("continuation-old-agent-event");
        let store = StudioStore::open_memory().await.unwrap();
        let project = store
            .upsert_project(repository.to_string_lossy().as_ref())
            .await
            .unwrap();
        let session = store
            .create_session(&project.id, "old agent event", CompileMode::Task)
            .await
            .unwrap();
        let launcher = RecordingLauncher::successful();
        let runtime = test_runtime(store.clone(), launcher.clone());
        let old_epoch = runtime.lifecycle_epoch();
        let turn_id = "turn-reused-across-epoch";
        let old_turn_token = tokio_util::sync::CancellationToken::new();
        runtime
            .active_turns
            .insert(
                session.id.clone(),
                turn_id.to_string(),
                old_turn_token.clone(),
            )
            .await
            .unwrap();
        let (old_event_tx, old_event_rx) = tokio::sync::broadcast::channel(8);
        let drain_started = Arc::new(tokio::sync::Barrier::new(2));
        let old_drain = {
            let runtime = runtime.clone();
            let session_id = session.id.clone();
            let drain_started = drain_started.clone();
            tokio::spawn(async move {
                drain_started.wait().await;
                runtime
                    .drain_prompt_agent_events_for_epoch(
                        session_id,
                        turn_id.to_string(),
                        old_epoch,
                        old_event_rx,
                    )
                    .await;
            })
        };
        drain_started.wait().await;

        let shutdown_runtime = runtime.clone();
        let shutdown = tokio::spawn(async move { shutdown_runtime.shutdown_runtime().await });
        old_turn_token.cancelled().await;
        runtime.active_turn_removed(&session.id, turn_id).await;
        assert_eq!(
            shutdown.await.unwrap().unwrap().status,
            StudioRuntimeStatus::Stopped
        );
        assert_eq!(
            runtime.initialize_runtime().await.unwrap().status,
            StudioRuntimeStatus::Ready
        );
        assert_ne!(runtime.lifecycle_epoch(), old_epoch);
        assert!(launcher.launches.lock().await.is_empty());

        let head = git_output(&repository, &["rev-parse", "HEAD"]);
        let branch = git_output(&repository, &["branch", "--show-current"]);
        let common_dir = git_output(&repository, &["rev-parse", "--git-common-dir"]);
        let common_dir = std::fs::canonicalize(repository.join(common_dir)).unwrap();
        let (run, _) = store
            .create_task_run_with_lease(CreateTaskRun {
                session_id: session.id.clone(),
                phase: TaskRunPhase::Implementing,
                plan: "Guard old agent events".to_string(),
                workspace_root: std::fs::canonicalize(&repository)
                    .unwrap()
                    .to_string_lossy()
                    .to_string(),
                git_common_dir: common_dir.to_string_lossy().to_string(),
                branch,
                head_commit: head,
            })
            .await
            .unwrap();
        let work_unit = store
            .create_work_unit(CreateWorkUnit {
                task_run_id: run.id.clone(),
                title: "New epoch agent".to_string(),
                owned_paths: vec!["code/pl-core/**".to_string()],
                base_commit: run.base_commit.clone(),
                worktree_path: repository
                    .join(".pure/worktrees/reused-agent")
                    .to_string_lossy()
                    .to_string(),
                branch: "pure-task-reused-agent".to_string(),
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
                requested_by_call_id: "call-new-epoch".to_string(),
                role: "executor".to_string(),
                status: AgentOutcomeStatus::Running,
                attempt: 1,
            })
            .await
            .unwrap();
        runtime
            .active_turns
            .insert(
                session.id.clone(),
                turn_id.to_string(),
                tokio_util::sync::CancellationToken::new(),
            )
            .await
            .unwrap();
        let mut studio_events = runtime.events().subscribe();

        old_event_tx.send(terminal_event("agent-1")).unwrap();
        drop(old_event_tx);
        old_drain.await.unwrap();

        let after_old_event = store
            .list_agent_outcomes(&run.id)
            .await
            .unwrap()
            .into_iter()
            .find(|record| record.id == outcome.id)
            .unwrap();
        assert_eq!(after_old_event, outcome);
        assert!(launcher.launches.lock().await.is_empty());
        assert!(
            !runtime
                .continuation_scheduler
                .has_session(&session.id)
                .await
        );
        assert!(matches!(
            studio_events.try_recv(),
            Err(tokio::sync::broadcast::error::TryRecvError::Empty)
        ));

        runtime.active_turn_removed(&session.id, turn_id).await;
        assert!(launcher.launches.lock().await.is_empty());

        let current_turn_token = tokio_util::sync::CancellationToken::new();
        runtime
            .active_turns
            .insert(session.id.clone(), turn_id.to_string(), current_turn_token)
            .await
            .unwrap();
        let (current_event_tx, current_event_rx) = tokio::sync::broadcast::channel(8);
        current_event_tx
            .send(pl_trace::AgentEvent::AgentStateChanged {
                id: "agent-1".to_string(),
                path: "/root/agent-1".to_string(),
                parent_path: Some("/root".to_string()),
                role: "executor".to_string(),
                task: "New epoch agent".to_string(),
                status: AgentStatus::Completed,
                summary: Some("done".to_string()),
                depth: 1,
                error: None,
                reason: None,
                budget_limit_kind: None,
                budget_usage: None,
                updated_at: 20,
            })
            .unwrap();
        drop(current_event_tx);
        runtime
            .drain_prompt_agent_events_for_epoch(
                session.id.clone(),
                turn_id.to_string(),
                runtime.lifecycle_epoch(),
                current_event_rx,
            )
            .await;
        assert_eq!(
            store
                .list_agent_outcomes(&run.id)
                .await
                .unwrap()
                .into_iter()
                .find(|record| record.id == outcome.id)
                .unwrap()
                .status,
            AgentOutcomeStatus::WaitingForDelivery
        );
        runtime.active_turn_removed(&session.id, turn_id).await;
        let launches = launcher.wait_for_count(1).await;
        assert_eq!(launches.len(), 1);
        assert_eq!(
            launches[0].request.reason,
            ContinuationReason::AgentTerminal
        );
        remove_repository(repository);
    })
    .await
    .unwrap();
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

    let (control_base_url, control_server) = serve_sse_once(old_sse.clone()).await;
    let control_store = StudioStore::open_memory().await.unwrap();
    let control_workspace = std::env::temp_dir().join(format!(
        "pure-response-gate-control-workspace-{}",
        unique_id()
    ));
    tokio::fs::create_dir_all(&control_workspace).await.unwrap();
    let control_project = control_store
        .upsert_project(control_workspace.to_str().unwrap())
        .await
        .unwrap();
    let control_session = control_store
        .create_session(
            &control_project.id,
            "response gate control",
            CompileMode::Simple,
        )
        .await
        .unwrap();
    let control_home =
        std::env::temp_dir().join(format!("pure-response-gate-control-home-{}", unique_id()));
    let control_config = ConfigStore::new(crate::config::ConfigPaths::from_home(&control_home));
    control_config.save(&test_config(control_base_url)).unwrap();
    let control_runtime = StudioRuntime::new(control_store.clone(), control_config);
    control_runtime
        .submit_prompt(StudioSubmitPromptRequest {
            session_id: control_session.id.clone(),
            prompt: "normal response".to_string(),
            attachment_ids: Vec::new(),
            options: StudioSubmitPromptOptions::default(),
        })
        .await
        .unwrap();
    wait_for_no_active_turn(&control_runtime).await;
    control_server.await.unwrap();
    assert!(
        !control_store
            .load_core_session(&control_session.id)
            .await
            .unwrap()
            .messages()
            .is_empty()
    );
    assert!(
        control_store
            .load_session_runtime(&control_session.id)
            .await
            .unwrap()
            .is_some()
    );

    let (base_url, server, old_accepted, release_old, recovery_accepted, release_recovery) =
        serve_two_delayed_sse(old_sse, "data: [DONE]\n\n".to_string()).await;
    let repository = init_repository("continuation-stale-background-restart");
    let store = StudioStore::open_memory().await.unwrap();
    let run = persisted_repository_run(&store, &repository, "stale-background-restart").await;
    let home = std::env::temp_dir().join(format!("pure-stale-background-home-{}", unique_id()));
    let config_store = ConfigStore::new(crate::config::ConfigPaths::from_home(&home));
    config_store.save(&test_config(base_url)).unwrap();
    let runtime = StudioRuntime::new(store.clone(), config_store);
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
    let session_runtime_before = store.load_session_runtime(&run.session_id).await.unwrap();
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
    tokio::time::timeout(TEST_RUNTIME_TIMEOUT, old_accepted)
        .await
        .unwrap()
        .unwrap();
    tokio::time::timeout(TEST_RUNTIME_TIMEOUT, async {
        loop {
            let events = store
                .load_studio_events(&run.session_id, None, None)
                .await
                .unwrap();
            let assistant_started = events.iter().any(|event| {
                matches!(
                    &event.kind,
                    StudioEventKind::MessageUpdated { message }
                        if message.turn_id == old_turn.turn_id
                            && message.role == StudioMessageRole::Assistant
                            && message.status == StudioMessageStatus::Streaming
                )
            });
            let turn_part_started = events.iter().any(|event| {
                matches!(
                    &event.kind,
                    StudioEventKind::MessagePartUpdated { part }
                        if part.turn_id == old_turn.turn_id
                            && part.part_type == StudioPartType::Turn
                )
            });
            let context_loading = events.iter().any(|event| {
                matches!(
                    &event.kind,
                    StudioEventKind::MessagePartUpdated { part }
                        if part.turn_id == old_turn.turn_id
                            && part.text == "已接收请求，正在准备上下文。"
                )
            });
            let model_ready = events.iter().any(|event| {
                matches!(
                    &event.kind,
                    StudioEventKind::MessagePartUpdated { part }
                        if part.turn_id == old_turn.turn_id
                            && part.text == "上下文已整理，准备调用模型。"
                )
            });
            let inference_started = events.iter().any(|event| {
                matches!(
                    &event.kind,
                    StudioEventKind::MessagePartUpdated { part }
                        if part.turn_id == old_turn.turn_id
                            && part.part_type == StudioPartType::Inference
                )
            });
            if assistant_started
                && turn_part_started
                && context_loading
                && model_ready
                && inference_started
            {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
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
    assert!(!initialize.is_finished());

    let _ = release_old.send(());
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
    let events_after = store
        .load_studio_events(&run.session_id, None, None)
        .await
        .unwrap();
    assert_eq!(
        events_after
            .iter()
            .filter(|event| event.turn_id.as_deref() == Some(old_turn.turn_id.as_str()))
            .count(),
        old_turn_events_before,
        "new old-turn events: {:#?}",
        events_after
            .iter()
            .filter(|event| event.turn_id.as_deref() == Some(old_turn.turn_id.as_str()))
            .skip(old_turn_events_before)
            .collect::<Vec<_>>()
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
    let _ = tokio::fs::remove_dir_all(control_home).await;
    let _ = tokio::fs::remove_dir_all(control_workspace).await;
}

#[tokio::test]
async fn delayed_plan_response_after_shutdown_cannot_create_plan_side_effects() {
    let tool_sse = concat!(
        "data: {\"type\":\"response.output_item.added\",\"item\":{\"type\":\"function_call\",\"id\":\"fc_gate\",\"call_id\":\"call_gate\",\"name\":\"plan_exit\"}}\n\n",
        "data: {\"type\":\"response.function_call_arguments.delta\",\"item_id\":\"fc_gate\",\"call_id\":\"call_gate\",\"delta\":\"{\\\"content\\\":\\\"# Gated Plan\\\\n\\\\n- Inspect\\\\n- Implement\\\"}\"}\n\n",
        "data: {\"type\":\"response.output_item.done\",\"item\":{\"type\":\"function_call\",\"id\":\"fc_gate\",\"call_id\":\"call_gate\",\"name\":\"plan_exit\",\"arguments\":\"{\\\"content\\\":\\\"# Gated Plan\\\\n\\\\n- Inspect\\\\n- Implement\\\"}\"}}\n\n",
        "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_gate_1\",\"usage\":{\"input_tokens\":1,\"output_tokens\":2,\"total_tokens\":3}}}\n\n",
        "data: [DONE]\n\n"
    )
    .to_string();
    let final_sse = concat!(
        "data: {\"type\":\"response.output_text.delta\",\"item_id\":\"msg_gate\",\"delta\":\"Plan submitted.\"}\n\n",
        "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_gate_2\",\"usage\":{\"input_tokens\":2,\"output_tokens\":1,\"total_tokens\":3}}}\n\n",
        "data: [DONE]\n\n"
    )
    .to_string();

    let (control_base_url, control_server) =
        serve_sse_sequence(vec![tool_sse.clone(), final_sse]).await;
    let control_workspace =
        std::env::temp_dir().join(format!("pure-plan-gate-control-workspace-{}", unique_id()));
    tokio::fs::create_dir_all(&control_workspace).await.unwrap();
    let control_store = StudioStore::open_memory().await.unwrap();
    let control_project = control_store
        .upsert_project(control_workspace.to_str().unwrap())
        .await
        .unwrap();
    let control_session = control_store
        .create_session(&control_project.id, "plan gate control", CompileMode::Task)
        .await
        .unwrap();
    let control_home =
        std::env::temp_dir().join(format!("pure-plan-gate-control-home-{}", unique_id()));
    let control_config = ConfigStore::new(crate::config::ConfigPaths::from_home(&control_home));
    control_config.save(&test_config(control_base_url)).unwrap();
    let control_runtime = StudioRuntime::new(control_store.clone(), control_config);
    control_runtime
        .submit_prompt(StudioSubmitPromptRequest {
            session_id: control_session.id.clone(),
            prompt: "make a plan".to_string(),
            attachment_ids: Vec::new(),
            options: StudioSubmitPromptOptions::default(),
        })
        .await
        .unwrap();
    wait_for_no_active_turn(&control_runtime).await;
    control_server.await.unwrap();
    assert!(
        control_store
            .list_pending_interactions(&control_session.id)
            .await
            .unwrap()
            .iter()
            .any(|interaction| interaction.kind == InteractionKind::PlanConfirmation)
    );

    let (base_url, server, accepted, release_response) = serve_delayed_sse_body(tool_sse).await;
    let workspace = std::env::temp_dir().join(format!("pure-plan-gate-workspace-{}", unique_id()));
    tokio::fs::create_dir_all(&workspace).await.unwrap();
    let store = StudioStore::open_memory().await.unwrap();
    let project = store
        .upsert_project(workspace.to_str().unwrap())
        .await
        .unwrap();
    let session = store
        .create_session(&project.id, "plan response gate", CompileMode::Task)
        .await
        .unwrap();
    let home = std::env::temp_dir().join(format!("pure-plan-gate-home-{}", unique_id()));
    let config_store = ConfigStore::new(crate::config::ConfigPaths::from_home(&home));
    config_store.save(&test_config(base_url)).unwrap();
    let mut runtime = StudioRuntime::new(store.clone(), config_store);
    let shutdown_cancelled = ContinuationTestBarrier::new();
    runtime.shutdown_after_cancel_barrier = Some(shutdown_cancelled.clone());
    let old_turn = runtime
        .submit_prompt(StudioSubmitPromptRequest {
            session_id: session.id.clone(),
            prompt: "make a cancelled plan".to_string(),
            attachment_ids: Vec::new(),
            options: StudioSubmitPromptOptions::default(),
        })
        .await
        .unwrap();
    tokio::time::timeout(TEST_RUNTIME_TIMEOUT, accepted)
        .await
        .unwrap()
        .unwrap();

    let shutdown_runtime = runtime.clone();
    let shutdown = tokio::spawn(async move { shutdown_runtime.shutdown_runtime().await });
    shutdown_cancelled.wait_until_entered().await;
    assert_eq!(
        runtime.runtime_snapshot().status,
        StudioRuntimeStatus::ShuttingDown
    );
    assert!(!shutdown.is_finished());
    let _ = release_response.send(());
    shutdown_cancelled.release().await;
    assert_eq!(
        shutdown.await.unwrap().unwrap().status,
        StudioRuntimeStatus::Stopped
    );
    server.await.unwrap();

    assert!(
        !store
            .list_pending_interactions(&session.id)
            .await
            .unwrap()
            .iter()
            .any(|interaction| interaction.kind == InteractionKind::PlanConfirmation)
    );
    let events = store
        .load_studio_events(&session.id, None, None)
        .await
        .unwrap();
    assert!(!events.iter().any(|event| matches!(
        &event.kind,
        StudioEventKind::MessagePartUpdated { part }
            if part.turn_id == old_turn.turn_id && part.part_type == StudioPartType::Plan
    )));
    assert!(!events.iter().any(|event| matches!(
        &event.kind,
        StudioEventKind::PlanLifecycleChanged { event }
            if event.turn_id.as_deref() == Some(old_turn.turn_id.as_str())
    )));

    let _ = tokio::fs::remove_dir_all(home).await;
    let _ = tokio::fs::remove_dir_all(workspace).await;
    let _ = tokio::fs::remove_dir_all(control_home).await;
    let _ = tokio::fs::remove_dir_all(control_workspace).await;
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

#[tokio::test]
async fn shutdown_fences_token_removal_through_scheduler_hook_before_restart() {
    let repository = init_repository("continuation-removal-drain-fence");
    let store = StudioStore::open_memory().await.unwrap();
    let run = persisted_repository_run(&store, &repository, "removal-drain-fence").await;
    let launcher = RecordingLauncher::successful();
    let mut runtime = test_runtime(store, launcher.clone());
    let token = tokio_util::sync::CancellationToken::new();
    let removal = ContinuationTestBarrier::new();
    let shutdown_entry = ContinuationTestBarrier::new();
    runtime.active_turn_removal_barrier = Some(removal.clone());
    runtime.shutdown_entry_barrier = Some(shutdown_entry.clone());
    runtime
        .active_turns
        .insert(
            run.session_id.clone(),
            "turn-old-epoch".to_string(),
            token.clone(),
        )
        .await
        .unwrap();
    runtime
        .request_task_continuation(run.id.clone(), ContinuationReason::AgentTerminal)
        .await;

    let shutdown_runtime = runtime.clone();
    let shutdown = tokio::spawn(async move { shutdown_runtime.shutdown_runtime().await });
    shutdown_entry.wait_until_entered().await;
    shutdown_entry.release().await;
    token.cancelled().await;

    let removal_runtime = runtime.clone();
    let removal_task = tokio::spawn(async move {
        removal_runtime
            .active_turn_removed(&run.session_id, "turn-old-epoch")
            .await
    });
    removal.wait_until_entered().await;

    assert!(runtime.post_turn_lock.try_lock().is_err());
    let initialize_runtime = runtime.clone();
    let initialize = tokio::spawn(async move { initialize_runtime.initialize_runtime().await });
    assert!(!shutdown.is_finished());
    assert!(!initialize.is_finished());

    removal.release().await;
    assert!(removal_task.await.unwrap());
    assert_eq!(
        shutdown.await.unwrap().unwrap().status,
        StudioRuntimeStatus::Stopped
    );
    assert_eq!(
        initialize.await.unwrap().unwrap().status,
        StudioRuntimeStatus::Ready
    );
    let launches = launcher.wait_for_count(1).await;
    assert_eq!(launches.len(), 1);
    assert_eq!(launches[0].request.reason, ContinuationReason::Recovery);
    remove_repository(repository);
}

#[tokio::test]
async fn shutting_down_current_turn_cancels_transient_rows_and_waiters_before_drain() {
    let (base_url, server, accepted, release_response) = serve_delayed_sse().await;
    let store = StudioStore::open_memory().await.unwrap();
    let workspace = std::env::temp_dir().join(format!(
        "pure-shutdown-interactions-workspace-{}",
        unique_id()
    ));
    tokio::fs::create_dir_all(&workspace).await.unwrap();
    let project = store
        .upsert_project(workspace.to_str().unwrap())
        .await
        .unwrap();
    let session = store
        .create_session(&project.id, "shutdown interactions", CompileMode::Simple)
        .await
        .unwrap();
    let home =
        std::env::temp_dir().join(format!("pure-shutdown-interactions-home-{}", unique_id()));
    let config_store = ConfigStore::new(crate::config::ConfigPaths::from_home(&home));
    config_store.save(&test_config(base_url)).unwrap();
    let mut runtime = StudioRuntime::new(store.clone(), config_store);
    let removal = ContinuationTestBarrier::new();
    runtime.active_turn_removal_barrier = Some(removal.clone());
    let turn = runtime
        .submit_prompt(StudioSubmitPromptRequest {
            session_id: session.id.clone(),
            prompt: "wait for shutdown".to_string(),
            attachment_ids: Vec::new(),
            options: StudioSubmitPromptOptions::default(),
        })
        .await
        .unwrap();
    tokio::time::timeout(TEST_RUNTIME_TIMEOUT, accepted)
        .await
        .unwrap()
        .unwrap();

    let interaction_events = Arc::new(Mutex::new(Vec::new()));
    let callback = runtime
        .interactions()
        .callback(session.id.clone(), emitter(interaction_events));
    let mut user_input = pending_interaction(
        "shutdown-user-input",
        &session.id,
        InteractionKind::UserInput,
        InteractionPayload::UserInput {
            questions: Vec::new(),
        },
    );
    user_input.scope.turn_id = turn.turn_id.clone();
    let mut tool_approval = pending_interaction(
        "shutdown-tool-approval",
        &session.id,
        InteractionKind::ToolApproval,
        InteractionPayload::ToolApproval {
            name: "bash".to_string(),
            arguments: serde_json::json!({"command": "echo hi"}),
            working_directory: None,
            parent_agent_id: None,
        },
    );
    tool_approval.scope.turn_id = turn.turn_id;
    let user_waiter = tokio::spawn(callback(user_input));
    let approval_waiter = tokio::spawn(callback(tool_approval));
    tokio::time::timeout(TEST_RUNTIME_TIMEOUT, async {
        while store
            .list_pending_interactions(&session.id)
            .await
            .unwrap()
            .len()
            != 2
        {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();

    let shutdown_runtime = runtime.clone();
    let shutdown = tokio::spawn(async move { shutdown_runtime.shutdown_runtime().await });
    tokio::time::timeout(TEST_RUNTIME_TIMEOUT, async {
        while runtime.runtime_snapshot().status != StudioRuntimeStatus::ShuttingDown {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    let _ = release_response.send(());
    removal.wait_until_entered().await;

    assert_eq!(
        store
            .read_interaction("shutdown-user-input")
            .await
            .unwrap()
            .unwrap()
            .status,
        InteractionStatus::Cancelled
    );
    assert_eq!(
        store
            .read_interaction("shutdown-tool-approval")
            .await
            .unwrap()
            .unwrap()
            .status,
        InteractionStatus::Cancelled
    );
    assert_eq!(
        user_waiter.await.unwrap(),
        InteractionResolution::UserInput {
            answers: Default::default(),
        }
    );
    assert_eq!(
        approval_waiter.await.unwrap(),
        InteractionResolution::ToolApproval {
            decision: pl_protocol::ToolApprovalResolution::Denied,
            reason: Some("turn completed".to_string()),
        }
    );
    assert!(!shutdown.is_finished());

    removal.release().await;
    assert_eq!(
        shutdown.await.unwrap().unwrap().status,
        StudioRuntimeStatus::Stopped
    );
    server.await.unwrap();
    let _ = tokio::fs::remove_dir_all(home).await;
    let _ = tokio::fs::remove_dir_all(workspace).await;
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
        lifecycle_epoch: 1,
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

async fn merge_conflict_continuation_fixture(
    name: &str,
) -> (StudioStore, crate::studio::task_coordinator::TaskRunRecord) {
    let store = StudioStore::open_memory().await.unwrap();
    let root = format!("C:/work/{name}");
    let project = store.upsert_project(&root).await.unwrap();
    let session = store
        .create_session(&project.id, name, CompileMode::Task)
        .await
        .unwrap();
    let (run, _) = store
        .create_task_run_with_lease(CreateTaskRun {
            session_id: session.id.clone(),
            phase: TaskRunPhase::Implementing,
            plan: "Resolve a merge conflict".to_string(),
            workspace_root: root.clone(),
            git_common_dir: format!("{root}/.git"),
            branch: format!("branch-{name}"),
            head_commit: "1111111".to_string(),
        })
        .await
        .unwrap();
    let worktree_path = format!("{root}/.pure/worktrees/agent-conflict");
    let branch = format!("pure-task-{name}-agent-conflict");
    let work_unit = store
        .create_work_unit(CreateWorkUnit {
            task_run_id: run.id.clone(),
            title: "Create conflict".to_string(),
            owned_paths: vec!["src/conflict.rs".to_string()],
            base_commit: run.base_commit.clone(),
            worktree_path: worktree_path.clone(),
            branch: branch.clone(),
            attempt: 1,
        })
        .await
        .unwrap();
    let work_unit = store
        .update_work_unit(
            &work_unit.id,
            WorkUnitStatus::Running,
            Some("agent-conflict".to_string()),
        )
        .await
        .unwrap();
    let outcome = store
        .create_agent_outcome(CreateAgentOutcome {
            task_run_id: run.id.clone(),
            work_unit_id: Some(work_unit.id.clone()),
            agent_id: "agent-conflict".to_string(),
            owner_path: "/root".to_string(),
            initiated_by: "planner".to_string(),
            requested_by_call_id: "call-agent-conflict".to_string(),
            role: "executor".to_string(),
            status: AgentOutcomeStatus::Running,
            attempt: 1,
        })
        .await
        .unwrap();
    let delivery_head = "2222222".to_string();
    store
        .complete_agent_delivery(
            &outcome.id,
            &work_unit.id,
            AgentDelivery {
                worktree: AgentWorktreeDelivery {
                    path: worktree_path,
                    branch,
                },
                base_commit: run.base_commit.clone(),
                head_commit: delivery_head.clone(),
                changed_files: vec!["src/conflict.rs".to_string()],
                verification_summary: "focused checks passed".to_string(),
            },
        )
        .await
        .unwrap();
    let scope = store
        .begin_task_merge(BeginTaskMerge {
            session_id: session.id,
            agent_id: "agent-conflict".to_string(),
            expected_head: run.expected_head.clone(),
            pre_index_tree: "tree-before-merge".to_string(),
            changed_files: vec!["src/conflict.rs".to_string()],
        })
        .await
        .unwrap();
    store
        .conflict_task_merge(ConflictTaskMerge {
            merge_id: scope.merge.id,
            manifest: ConflictManifest {
                merge_head: delivery_head,
                merge_base: run.base_commit.clone(),
                pre_index_tree: "tree-before-merge".to_string(),
                status_porcelain_v1_z: Vec::new(),
                index_stage_zero_entries: Vec::new(),
                conflicts: vec![ConflictEntry {
                    path: "src/conflict.rs".to_string(),
                    kind: ConflictKind::Text,
                    stages: Vec::new(),
                    worktree_object_id: None,
                    binary: false,
                    rename_source: None,
                    rename_destination: None,
                }],
                auto_merged_entries: Vec::new(),
            },
        })
        .await
        .unwrap();
    let run = store.read_task_run(&run.id).await.unwrap().unwrap();
    (store, run)
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

fn lagged_agent_event_receiver() -> tokio::sync::broadcast::Receiver<pl_trace::AgentEvent> {
    let (event_tx, event_rx) = tokio::sync::broadcast::channel(1);
    for index in 0..3 {
        event_tx
            .send(pl_trace::AgentEvent::Error {
                message: format!("lagged event {index}"),
                severity: pl_protocol::ErrorSeverity::Recoverable,
            })
            .unwrap();
    }
    drop(event_tx);
    event_rx
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

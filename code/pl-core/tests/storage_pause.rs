mod support;
#[path = "support/tool.rs"]
mod tool_support;

use std::{
    collections::BTreeSet,
    io,
    sync::{Arc, Mutex},
    time::Duration,
};

use pl_core::{
    context::OpaquePayload,
    model::DynModelSession,
    thread::{
        AttemptOutcome, ThreadError, ThreadHandle, TurnOutcome, TurnState,
        cold::{
            ColdStore, ColdStoreError, ColdStoreHandle, StorageExecutionPhase, StoragePressure,
            ThreadWrite,
        },
        input::ThreadInput,
    },
};
use tokio::sync::watch;

use support::{ScriptedModel, text, turn};
use tool_support::tool;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Failure {
    QueueFull,
    Admission,
}

#[derive(Debug)]
struct StoreState {
    armed: bool,
    active: bool,
    accepted: BTreeSet<u64>,
}

#[derive(Debug, Clone)]
struct GatedStore {
    failure: Failure,
    state: Arc<Mutex<StoreState>>,
    changed: watch::Sender<()>,
}

impl GatedStore {
    fn new(failure: Failure) -> Self {
        let (changed, _) = watch::channel(());
        Self {
            failure,
            state: Arc::new(Mutex::new(StoreState {
                armed: true,
                active: false,
                accepted: BTreeSet::new(),
            })),
            changed,
        }
    }

    fn repair(&self) {
        self.state.lock().unwrap().active = false;
        self.changed.send_replace(());
    }
}

fn storage_error() -> ColdStoreError {
    ColdStoreError {
        source: Box::new(io::Error::other("history admission unavailable")),
    }
}

impl ColdStore for GatedStore {
    fn pressure(&self, _thread_id: &str) -> StoragePressure {
        let state = self.state.lock().unwrap();
        if !state.active {
            return StoragePressure::default();
        }
        match self.failure {
            Failure::QueueFull => StoragePressure {
                thread_bytes: 64 * 1024 * 1024,
                ..StoragePressure::default()
            },
            Failure::Admission => StoragePressure {
                error: Some(Arc::new(storage_error())),
                ..StoragePressure::default()
            },
        }
    }

    fn subscribe_pressure(&self, _thread_id: &str) -> Option<watch::Receiver<()>> {
        Some(self.changed.subscribe())
    }

    fn admit(&self, _thread_id: &str, write: ThreadWrite) -> Result<(), ColdStoreError> {
        let mut state = self.state.lock().unwrap();
        if state.armed
            && write.effect.attempt.as_ref().is_some_and(|attempt| {
                matches!(&attempt.outcome, AttemptOutcome::Committed(output) if !output.tool_calls.is_empty())
            })
        {
            state.armed = false;
            state.active = true;
            self.changed.send_replace(());
        }
        if state.active && self.failure == Failure::Admission {
            return Err(storage_error());
        }
        state.accepted.insert(write.effect.sequence);
        Ok(())
    }

    async fn flush(&self, _thread_id: &str, sequence: u64) -> Result<(), ColdStoreError> {
        let state = self.state.lock().unwrap();
        if state.active && self.failure == Failure::Admission {
            return Err(storage_error());
        }
        if (1..=sequence).all(|number| state.accepted.contains(&number)) {
            Ok(())
        } else {
            Err(storage_error())
        }
    }
}

async fn paused(thread: &ThreadHandle) {
    let mut snapshots = thread.subscribe();
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let snapshot = snapshots.next().await.expect("thread remains observable");
            if snapshot.persistence.execution_phase == StorageExecutionPhase::PausedForStorage {
                assert!(
                    snapshot
                        .turns
                        .iter()
                        .any(|turn| turn.state == TurnState::Running)
                );
                break;
            }
        }
    })
    .await
    .expect("storage pause was not observed");
}

async fn paused_turn_resumes_without_replaying_model_or_tool(failure: Failure) {
    let store = GatedStore::new(failure);
    let (model, requests) = ScriptedModel::new(&["write"]);
    let thread = ThreadHandle::start("stored".into(), DynModelSession::new(model)).unwrap();
    let executions = Arc::new(Mutex::new(Vec::new()));
    thread
        .register_tools(vec![tool("write", &executions)])
        .await
        .unwrap();
    thread
        .attach_storage(ColdStoreHandle::new(store.clone()))
        .await
        .unwrap();

    let runner = tokio::spawn({
        let thread = thread.clone();
        async move { thread.run_turn(turn("first")).await }
    });
    paused(&thread).await;
    assert!(!runner.is_finished());
    assert_eq!(requests.lock().unwrap().len(), 1);
    assert!(executions.lock().unwrap().is_empty());
    let snapshot = thread.snapshot();
    match failure {
        Failure::QueueFull => assert!(snapshot.persistence.pressure_paused),
        Failure::Admission => assert!(snapshot.persistence.error.is_some()),
    }
    let fresh = ThreadInput {
        id: "blocked-input".into(),
        payload: OpaquePayload::text("blocked"),
        context: vec![text("blocked")],
    };
    assert!(matches!(
        thread.submit_input(fresh).await,
        Err(ThreadError::StoragePressure | ThreadError::Storage(_))
    ));

    store.repair();
    let completed = tokio::time::timeout(Duration::from_secs(5), runner)
        .await
        .expect("turn did not resume")
        .unwrap()
        .unwrap();
    assert_eq!(completed.outcome, TurnOutcome::Completed);
    assert_eq!(completed.model_steps, 2);
    assert_eq!(requests.lock().unwrap().len(), 2);
    assert_eq!(*executions.lock().unwrap(), ["first-write".to_string()]);
    thread.flush().await.unwrap();
    let snapshot = thread.snapshot();
    assert_eq!(
        snapshot.persistence.execution_phase,
        StorageExecutionPhase::Running
    );
    assert!(snapshot.persistence.durable_sequence >= snapshot.commit_sequence);
    thread.close().await.unwrap();
}

#[tokio::test]
async fn full_history_queue_pauses_at_tool_boundary_and_resumes_without_replaying() {
    paused_turn_resumes_without_replaying_model_or_tool(Failure::QueueFull).await;
}

#[tokio::test]
async fn rejected_history_admission_pauses_at_tool_boundary_and_resumes_without_replaying() {
    paused_turn_resumes_without_replaying_model_or_tool(Failure::Admission).await;
}

async fn interrupt_paused_turn(failure: Failure) {
    let store = GatedStore::new(failure);
    let (model, requests) = ScriptedModel::new(&["write"]);
    let thread = ThreadHandle::start("interruptible".into(), DynModelSession::new(model)).unwrap();
    let executions = Arc::new(Mutex::new(Vec::new()));
    thread
        .register_tools(vec![tool("write", &executions)])
        .await
        .unwrap();
    thread
        .attach_storage(ColdStoreHandle::new(store.clone()))
        .await
        .unwrap();

    let runner = tokio::spawn({
        let thread = thread.clone();
        async move { thread.run_turn(turn("interrupted")).await }
    });
    paused(&thread).await;
    assert!(
        thread
            .interrupt_turn(Some("interrupted".into()))
            .await
            .unwrap()
    );
    assert!(matches!(
        tokio::time::timeout(Duration::from_secs(5), runner)
            .await
            .expect("interruption did not reach the paused turn")
            .unwrap(),
        Err(ThreadError::Cancelled)
    ));
    assert_eq!(requests.lock().unwrap().len(), 1);
    assert!(executions.lock().unwrap().is_empty());
    assert!(thread.effects().await.unwrap().iter().any(|effect| {
        effect.turn.as_ref().is_some_and(|turn| {
            turn.turn_id == "interrupted" && turn.state == TurnState::Interrupted
        })
    }));

    store.repair();
    thread.flush().await.unwrap();
    let completed = thread.run_turn(turn("after-interrupt")).await.unwrap();
    assert_eq!(completed.outcome, TurnOutcome::Completed);
    assert_eq!(requests.lock().unwrap().len(), 3);
    assert_eq!(
        *executions.lock().unwrap(),
        ["after-interrupt-write".to_string()]
    );
    thread.close().await.unwrap();
}

#[tokio::test]
async fn interrupt_remains_reachable_during_full_queue_pause() {
    interrupt_paused_turn(Failure::QueueFull).await;
}

#[tokio::test]
async fn interrupt_remains_reachable_during_admission_failure_pause() {
    interrupt_paused_turn(Failure::Admission).await;
}

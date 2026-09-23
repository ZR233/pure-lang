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
        AttemptOutcome, ModelStepLimit, ThreadError, ThreadHandle, TurnOutcome, TurnState,
        cold::{
            ColdStore, ColdStoreError, ColdStoreHandle, StorageExecutionPhase, StoragePressure,
            ThreadWrite,
        },
        input::ThreadInput,
        task::TaskStatus,
    },
    tool::{
        ToolOutput,
        opaque::{CallContext, Registration, Tool, ToolError},
    },
};
use tokio::sync::{Notify, mpsc, watch};

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

    fn fail_now(&self) {
        let mut state = self.state.lock().unwrap();
        state.armed = false;
        state.active = true;
        drop(state);
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

#[derive(Debug)]
struct HeldTool {
    started: mpsc::UnboundedSender<String>,
    release: Arc<Notify>,
    executions: Arc<Mutex<Vec<String>>>,
}

impl Tool for HeldTool {
    async fn execute(
        &self,
        input: OpaquePayload,
        context: CallContext,
    ) -> Result<ToolOutput, ToolError> {
        let call_id = context.call_id;
        self.executions.lock().unwrap().push(call_id.clone());
        self.started
            .send(call_id)
            .expect("test observes tool start");
        self.release.notified().await;
        Ok(ToolOutput::new(input.clone(), vec![text(input.content())]))
    }
}

#[tokio::test]
async fn concurrent_tool_results_remain_owned_until_storage_recovers() {
    let store = GatedStore::new(Failure::Admission);
    store.state.lock().unwrap().armed = false;
    let (model, requests) = ScriptedModel::new(&["first", "second"]);
    let thread = ThreadHandle::start("concurrent".into(), DynModelSession::new(model)).unwrap();
    let (started, mut starts) = mpsc::unbounded_channel();
    let executions = Arc::new(Mutex::new(Vec::new()));
    let releases: Vec<_> = (0..2).map(|_| Arc::new(Notify::new())).collect();
    thread
        .register_tools(
            ["first", "second"]
                .into_iter()
                .zip(releases.iter())
                .map(|(id, release)| {
                    Registration::new(
                        id.into(),
                        OpaquePayload::text(format!("Tool {id}")),
                        HeldTool {
                            started: started.clone(),
                            release: release.clone(),
                            executions: executions.clone(),
                        },
                    )
                    .unwrap()
                })
                .collect(),
        )
        .await
        .unwrap();
    thread
        .attach_storage(ColdStoreHandle::new(store.clone()))
        .await
        .unwrap();

    let mut input = turn("two-in-flight");
    input.max_model_steps = ModelStepLimit::Limited(1.try_into().unwrap());
    let runner = tokio::spawn({
        let thread = thread.clone();
        async move { thread.run_turn(input).await }
    });
    tokio::time::timeout(Duration::from_secs(10), async {
        assert_eq!(starts.recv().await.as_deref(), Some("two-in-flight-first"));
        assert_eq!(starts.recv().await.as_deref(), Some("two-in-flight-second"));
    })
    .await
    .expect("both tools started before either completed");
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(5), runner)
            .await
            .expect("turn dispatch did not complete")
            .unwrap()
            .unwrap()
            .outcome,
        TurnOutcome::StepLimit
    );

    store.fail_now();
    for release in &releases {
        release.notify_one();
    }
    let mut snapshots = thread.subscribe();
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let snapshot = snapshots.next().await.unwrap();
            if snapshot.terminal_tasks.len()
                + snapshot
                    .tasks
                    .values()
                    .filter(|task| task.status != TaskStatus::Running)
                    .count()
                == 2
            {
                break;
            }
        }
    })
    .await
    .expect("both completed results remain visible after rejected admission");
    assert_eq!(requests.lock().unwrap().len(), 1);
    assert_eq!(executions.lock().unwrap().len(), 2);
    let pending = thread.snapshot();
    assert!(pending.persistence.error.is_some());
    assert!(pending.persistence.admitted_sequence < pending.commit_sequence);
    let deliveries: Vec<_> = thread
        .effects()
        .await
        .unwrap()
        .into_iter()
        .flat_map(|effect| effect.deliveries.to_vec())
        .map(|delivery| delivery.call_id)
        .collect();
    assert_eq!(deliveries.len(), 2);
    assert!(deliveries.contains(&"two-in-flight-first".to_owned()));
    assert!(deliveries.contains(&"two-in-flight-second".to_owned()));

    store.repair();
    thread.flush().await.unwrap();
    assert_eq!(executions.lock().unwrap().len(), 2);
    assert!(thread.snapshot().persistence.durable_sequence >= pending.commit_sequence);
    thread.close().await.unwrap();
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

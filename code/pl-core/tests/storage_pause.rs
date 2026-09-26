mod support;
#[path = "support/tool.rs"]
mod tool_support;

use std::{
    collections::BTreeSet,
    io,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::Duration,
};

use pl_core::{
    context::{ContextContent, ContextSource, OpaquePayload, ResourceReference},
    model::{DynModelSession, ToolProgressUpdate},
    thread::{
        AttemptOutcome, ModelStepLimit, ThreadError, ThreadHandle, ThreadSnapshot, TurnOutcome,
        TurnState,
        cold::{
            ColdStore, ColdStoreError, ColdStoreHandle, OutputRepair, OutputRetryFuture,
            OutputRetryObligation, OutputRetryOutcome, OutputStorageFault, StorageExecutionPhase,
            StorageFaultKind, StoragePressure, ThreadWrite,
        },
        input::ThreadInput,
        task::TaskStatus,
    },
    tool::{
        ToolOutput,
        opaque::{CallContext, Registration, Tool, ToolError},
    },
};
use tokio::sync::{Barrier, Notify, mpsc, watch};

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
    /// Highest effect sequence this fake has written durably.
    ///
    /// `admit` only accepts a batch; durability lands later, exactly like a real writer that
    /// acknowledges its own watermark. Keeping the two apart is what lets a test prove that an
    /// accepted-but-not-yet-durable recovery must not release the pause.
    durable: u64,
    /// Ceiling each in-flight operation may reserve for its live output, when the test sets one.
    ///
    /// `None` grants the caller's request unchanged, so every other test still exercises the plain
    /// "no reliable budget" backend.
    operation_limit: Option<u64>,
    /// Typed fault kind this fake reports while it is failing.
    ///
    /// `None` keeps the plain "error text only, no named generation" backend every other test uses,
    /// so the recovery gate they exercise stays trivial.
    typed_fault: Option<StorageFaultKind>,
    /// Generation this fake's reports belong to, whether or not a fault is active.
    ///
    /// A real backend keeps naming its own generation after the retry lands, which is what lets the
    /// owner compare the continue the caller named against the backend's verdict for that same one.
    reported_generation: u64,
    /// Newest generation this fake has itself verified as recovered, reported as the typed receipt.
    ///
    /// A generation that stopped erroring and one whose retry the backend itself confirmed are
    /// deliberately different facts: only the latter is reported here.
    verified_generation: Option<u64>,
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
                durable: 0,
                operation_limit: None,
                typed_fault: None,
                reported_generation: 0,
                verified_generation: None,
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

    /// Makes every accepted batch durable, but not a batch that was merely presented.
    fn commit_durable(&self) {
        let mut state = self.state.lock().unwrap();
        if let Some(latest) = state.accepted.iter().copied().max() {
            state.durable = state.durable.max(latest);
        }
        drop(state);
        self.changed.send_replace(());
    }

    /// Bounds what one in-flight operation may reserve for its live output.
    fn set_operation_limit(&self, limit: u64) {
        self.state.lock().unwrap().operation_limit = Some(limit);
    }

    /// Names a typed fault this backend reports for `generation` while it is failing.
    fn arm_typed_fault(&self, kind: StorageFaultKind, generation: u64) {
        let mut state = self.state.lock().unwrap();
        state.typed_fault = Some(kind);
        state.reported_generation = generation;
        state.verified_generation = None;
        drop(state);
        self.changed.send_replace(());
    }

    /// Records that this backend's own retry of `generation` reached its durable target.
    fn verify_recovery(&self, generation: u64) {
        let mut state = self.state.lock().unwrap();
        state.reported_generation = generation;
        state.verified_generation = Some(generation);
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
    fn reserve_operation_output(
        &self,
        _thread_id: &str,
        _operation_id: &str,
        max_bytes: u64,
    ) -> Result<u64, ColdStoreError> {
        Ok(self
            .state
            .lock()
            .unwrap()
            .operation_limit
            .unwrap_or(max_bytes))
    }

    fn charge_operation_output(
        &self,
        _thread_id: &str,
        _operation_id: &str,
        accepted_bytes: u64,
    ) -> Result<(), ColdStoreError> {
        let limit = self.state.lock().unwrap().operation_limit;
        if limit.is_some_and(|limit| accepted_bytes > limit) {
            return Err(storage_error());
        }
        Ok(())
    }

    fn release_operation_output(&self, _thread_id: &str, _operation_id: &str) {}

    fn pressure(&self, _thread_id: &str) -> StoragePressure {
        let state = self.state.lock().unwrap();
        let active = state.active;
        StoragePressure {
            thread_bytes: if active && self.failure == Failure::QueueFull {
                64 * 1024 * 1024
            } else {
                0
            },
            // This fake models one Thread at a time, so its process budget is exactly the Thread
            // bytes it reports above — the same body is never counted twice here.
            store_bytes: if active && self.failure == Failure::QueueFull {
                64 * 1024 * 1024
            } else {
                0
            },
            durable_sequence: state.durable,
            // The typed category rides the same report as the text: a failing generation names
            // itself, and a repaired one keeps naming it so a continue for that generation can be
            // matched against this backend's own verdict instead of the mere absence of an error.
            fault: if active { state.typed_fault } else { None },
            fault_generation: state.reported_generation,
            recovered_generation: state.verified_generation,
            error: if active && self.failure == Failure::Admission {
                Some(Arc::new(storage_error()))
            } else {
                None
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

/// Runs one external operation with a deadline.
///
/// Every safety control a caller can issue while a Turn is paused must stay reachable through the
/// owner mailbox: a hang is the exact defect this proves absent, so it fails as a timeout instead of
/// silently stalling the test.
async fn within<T>(operation: impl std::future::Future<Output = T>) -> T {
    tokio::time::timeout(Duration::from_secs(5), operation)
        .await
        .expect("storage control must stay reachable while a Turn is paused")
}

/// Waits for the authoritative snapshot to satisfy `predicate`.
///
/// A readiness the owner never publishes fails as a timeout instead of hanging, so "the single
/// owner published this fact by itself" is the thing under test rather than an implementation
/// detail the assertion could pass on.
async fn await_snapshot(
    thread: &ThreadHandle,
    predicate: impl Fn(&ThreadSnapshot) -> bool,
) -> ThreadSnapshot {
    let mut snapshots = thread.subscribe();
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let snapshot = snapshots.next().await.expect("thread remains observable");
            if predicate(&snapshot) {
                return snapshot;
            }
        }
    })
    .await
    .expect("the owner never published the expected storage fact")
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
        within(thread.submit_input(fresh)).await,
        Err(ThreadError::StoragePressure | ThreadError::Storage(_))
    ));

    store.repair();
    if failure == Failure::Admission {
        // A failed save holds new model/tool work until recovery is explicitly confirmed: a purely
        // transient pressure pause resumes on its own, a hard fault does not. The explicit continue
        // itself must reach the owner that is still inside this paused Turn.
        assert!(!runner.is_finished());
        let stale = within(thread.resume_storage(7)).await;
        assert!(
            matches!(
                stale,
                Err(ThreadError::StaleStorageRecovery {
                    requested: 7,
                    recorded: 0
                })
            ),
            "a generation the owner never recorded must be refused: {stale:?}"
        );
        // The refused batch is admitted again but is not durable yet, so the recovery is not a fact
        // yet: releasing here would start the next model step on a save that never landed.
        match within(thread.resume_storage(0)).await {
            Err(ThreadError::StorageRecoveryPending { durable, required }) => assert!(
                durable < required,
                "the refusal must name the unfinished fence: durable {durable}, required {required}"
            ),
            refused => {
                panic!("a resume before its durable fence must be refused: {refused:?}")
            }
        }
        assert!(!runner.is_finished());
        assert_eq!(requests.lock().unwrap().len(), 1);
        assert!(executions.lock().unwrap().is_empty());
        // The save really becomes durable; only then may the explicit continue release the pause.
        store.commit_durable();
        within(thread.resume_storage(0)).await.unwrap();
    }
    let completed = tokio::time::timeout(Duration::from_secs(5), runner)
        .await
        .expect("turn did not resume")
        .unwrap()
        .unwrap();
    assert_eq!(completed.outcome, TurnOutcome::Completed);
    assert_eq!(completed.model_steps, 2);
    assert_eq!(requests.lock().unwrap().len(), 2);
    assert_eq!(*executions.lock().unwrap(), ["first-write".to_string()]);
    within(thread.flush()).await.unwrap();
    let snapshot = thread.snapshot();
    assert_eq!(
        snapshot.persistence.execution_phase,
        StorageExecutionPhase::Running
    );
    assert!(snapshot.persistence.durable_sequence >= snapshot.commit_sequence);
    within(thread.close()).await.unwrap();
}

#[tokio::test]
async fn full_history_queue_pauses_at_tool_boundary_and_resumes_without_replaying() {
    paused_turn_resumes_without_replaying_model_or_tool(Failure::QueueFull).await;
}

#[tokio::test]
async fn rejected_history_admission_pauses_at_tool_boundary_and_resumes_without_replaying() {
    paused_turn_resumes_without_replaying_model_or_tool(Failure::Admission).await;
}

/// A typed fault is continued only against the backend's own verdict for *that* generation.
///
/// The backend stops reporting an error and takes the refused batch again, yet nothing is proven:
/// the continue the caller was offered stays refused until the backend itself names the same
/// generation recovered, and the durable fence that releases it is the one fixed when the fault was
/// latched — a later, unrelated save cannot move the target the caller was shown.
#[tokio::test]
async fn a_typed_fault_needs_its_own_verified_recovery_before_the_continue_is_offered() {
    let store = GatedStore::new(Failure::Admission);
    store.arm_typed_fault(StorageFaultKind::WriteFailed, 1);
    let (model, requests) = ScriptedModel::new(&["write"]);
    let thread = ThreadHandle::start("verified".into(), DynModelSession::new(model)).unwrap();
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
        async move { thread.run_turn(turn("verified")).await }
    });
    paused(&thread).await;

    let snapshot = thread.snapshot();
    assert_eq!(
        snapshot.persistence.fault,
        Some(StorageFaultKind::WriteFailed)
    );
    assert_eq!(snapshot.persistence.fault_generation, 1);
    assert!(snapshot.persistence.resume_required);
    assert!(!snapshot.persistence.resume_ready);
    // A backend still reporting its fault funds no continue at all.
    assert!(matches!(
        within(thread.resume_storage(1)).await,
        Err(ThreadError::Storage(_))
    ));

    // The backend stopped erroring and took the refused batch again, but its own retry verdict for
    // this generation is still missing: the single owner must keep refusing the continue.
    store.repair();
    assert!(matches!(
        within(thread.resume_storage(1)).await,
        Err(ThreadError::StorageRecoveryUnverified { generation: 1 })
    ));
    assert!(!runner.is_finished());
    assert_eq!(requests.lock().unwrap().len(), 1);
    assert!(executions.lock().unwrap().is_empty());

    // The backend now names generation 1 as recovered, but its durable receipt still lags the fence
    // fixed at the fault: a named recovery alone releases nothing.
    store.verify_recovery(1);
    let required = match within(thread.resume_storage(1)).await {
        Err(ThreadError::StorageRecoveryPending { durable, required }) => {
            assert!(
                durable < required,
                "the refusal must name the unfinished fence: durable {durable}, required {required}"
            );
            required
        }
        refused => panic!("a resume before its durable fence must be refused: {refused:?}"),
    };
    assert!(!thread.snapshot().persistence.resume_ready);

    // Reaching exactly the fixed fence is enough: the target never moves onto newer facts, and the
    // owner publishes the same verdict the continue command behind it applies.
    store.commit_durable();
    let ready = await_snapshot(&thread, |snapshot| snapshot.persistence.resume_ready).await;
    assert!(ready.persistence.durable_sequence >= required);
    assert!(
        ready.persistence.resume_required,
        "readiness is a hint for the caller; it never releases the latch by itself"
    );
    within(thread.resume_storage(1)).await.unwrap();

    let completed = tokio::time::timeout(Duration::from_secs(5), runner)
        .await
        .expect("turn did not resume")
        .unwrap()
        .unwrap();
    assert_eq!(completed.outcome, TurnOutcome::Completed);
    assert_eq!(completed.model_steps, 2);
    assert_eq!(requests.lock().unwrap().len(), 2);
    assert_eq!(*executions.lock().unwrap(), ["verified-write".to_string()]);
    within(thread.flush()).await.unwrap();
    within(thread.close()).await.unwrap();
}

/// A Thread parked between Turns learns about a verified recovery from the backend itself.
///
/// Nothing here issues a command after the Turn ended: the owner's storage watch is the only
/// wake-up, and the readiness it publishes is recomputed from its own typed facts — exactly what the
/// continue command re-applies. The latch itself is never released by that notification.
#[tokio::test]
async fn an_idle_owner_publishes_a_verified_recovery_without_a_command() {
    let store = GatedStore::new(Failure::Admission);
    store.arm_typed_fault(StorageFaultKind::WriteFailed, 1);
    let (model, requests) = ScriptedModel::new(&["write"]);
    let thread = ThreadHandle::start("idle".into(), DynModelSession::new(model)).unwrap();
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
        async move { thread.run_turn(turn("idle")).await }
    });
    paused(&thread).await;
    within(thread.interrupt_turn(Some("idle".into())))
        .await
        .unwrap();
    assert!(matches!(
        tokio::time::timeout(Duration::from_secs(5), runner)
            .await
            .expect("interruption did not reach the paused turn")
            .unwrap(),
        Err(ThreadError::Cancelled)
    ));
    assert_eq!(requests.lock().unwrap().len(), 1);
    assert!(executions.lock().unwrap().is_empty());
    // The Turn is gone, but the hard fault latch is not: it belongs to the storage owner, so a
    // parked Thread still owes the caller an accurate readiness.
    let snapshot = thread.snapshot();
    assert!(snapshot.persistence.resume_required);
    assert!(!snapshot.persistence.resume_ready);

    // The backend verified generation 1 and stopped failing. The owner re-presents the rejected fact
    // by itself, yet the durable receipt has not reached the fence.
    store.verify_recovery(1);
    store.repair();
    await_snapshot(&thread, |snapshot| {
        snapshot.persistence.error.is_none() && snapshot.persistence.resume_required
    })
    .await;
    assert!(!thread.snapshot().persistence.resume_ready);

    // The receipt lands; the only fact that changed is this backend's own report.
    store.commit_durable();
    let ready = await_snapshot(&thread, |snapshot| snapshot.persistence.resume_ready).await;
    assert!(ready.persistence.resume_required);

    // The continue the caller now issues is the same verdict, revalidated at the command.
    within(thread.resume_storage(1)).await.unwrap();
    assert!(!thread.snapshot().persistence.resume_required);
    within(thread.flush()).await.unwrap();
    within(thread.close()).await.unwrap();
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
        within(thread.interrupt_turn(Some("interrupted".into())))
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
    assert!(
        within(thread.effects())
            .await
            .unwrap()
            .iter()
            .any(|effect| {
                effect.turn.as_ref().is_some_and(|turn| {
                    turn.turn_id == "interrupted" && turn.state == TurnState::Interrupted
                })
            })
    );

    store.repair();
    if failure == Failure::Admission {
        // The paused owner holds the next admission until the recovered save is durable *and* the
        // caller explicitly releases it; the resume channel stays reachable after the Turn ended.
        let refused = within(thread.resume_storage(0)).await;
        assert!(
            matches!(refused, Err(ThreadError::StorageRecoveryPending { .. })),
            "a resume before its durable fence must be refused: {refused:?}"
        );
        store.commit_durable();
        within(thread.resume_storage(0)).await.unwrap();
    }
    within(thread.flush()).await.unwrap();
    let completed = within(thread.run_turn(turn("after-interrupt")))
        .await
        .unwrap();
    assert_eq!(completed.outcome, TurnOutcome::Completed);
    assert_eq!(requests.lock().unwrap().len(), 3);
    assert_eq!(
        *executions.lock().unwrap(),
        ["after-interrupt-write".to_string()]
    );
    within(thread.close()).await.unwrap();
}

#[tokio::test]
async fn interrupt_remains_reachable_during_full_queue_pause() {
    interrupt_paused_turn(Failure::QueueFull).await;
}

#[tokio::test]
async fn interrupt_remains_reachable_during_admission_failure_pause() {
    interrupt_paused_turn(Failure::Admission).await;
}

/// A model that streams more live output than its reserved reliable quota can hold.
///
/// It charges the bytes it accepted *before* they become resident, exactly like the real adapter:
/// the first charge fits the reserved ceiling, and the second is refused, so the call is cancelled
/// with the bytes it already had instead of buffering output this process could not retain.
#[derive(Debug)]
struct BudgetModel {
    accepted: u64,
    refused: Arc<Mutex<bool>>,
}

impl pl_core::model::ModelSession for BudgetModel {
    async fn prepare(
        &mut self,
        request: pl_core::model::ModelRequest,
    ) -> Result<pl_core::model::PreparedModelCall, pl_core::model::ModelError> {
        let progress = request.progress.clone();
        let accepted = self.accepted;
        let refused = self.refused.clone();
        Ok(pl_core::model::PreparedModelCall::new(async move {
            if let Some(sender) = &progress {
                if let Err(error) = sender.charge_output(accepted) {
                    return Err(model_failure(error.to_string()));
                }
                if let Err(error) = sender.charge_output(accepted + 4096) {
                    *refused.lock().unwrap() = true;
                    return Err(model_failure(error.to_string()));
                }
            }
            Ok(support::response(&request, Vec::new()))
        }))
    }

    async fn close(&mut self) -> Result<(), pl_core::model::ModelError> {
        Ok(())
    }
}

fn model_failure(message: String) -> pl_core::model::ModelError {
    pl_core::model::ModelError {
        details: None,
        kind: pl_core::model::ModelFailureKind::Unavailable,
        usage: Default::default(),
        source: Some(Box::new(io::Error::other(message))),
    }
}

/// Streaming output past the reserved reliable budget cancels the call with a typed fault.
///
/// The reservation is the admission gate: it is what makes "the next chunk would not fit" a fact
/// the call can act on *before* the bytes become resident. The refusal is latched as the typed
/// storage fault and new model/tool work is held until an explicit resume names the generation.
#[tokio::test]
async fn streaming_output_over_the_reliable_budget_cancels_with_a_typed_fault() {
    let store = GatedStore::new(Failure::Admission);
    // No admission failure here: this test isolates the in-flight output budget.
    store.state.lock().unwrap().armed = false;
    store.set_operation_limit(8);
    let refused = Arc::new(Mutex::new(false));
    let thread = ThreadHandle::start(
        "budget".into(),
        DynModelSession::new(BudgetModel {
            accepted: 8,
            refused: refused.clone(),
        }),
    )
    .unwrap();
    thread
        .attach_storage(ColdStoreHandle::new(store.clone()))
        .await
        .unwrap();

    // The call is cancelled once the second chunk cannot be retained; the refusal reached it.
    let _ = within(thread.run_turn(turn("budget"))).await;
    assert!(*refused.lock().unwrap());
    let snapshot = thread.snapshot();
    assert!(
        snapshot.persistence.resume_required,
        "a truncated operation must owe an explicit resume"
    );
    assert_eq!(
        snapshot.persistence.fault,
        Some(pl_core::thread::cold::StorageFaultKind::QueueFull),
        "the truncation must travel as a typed storage fault, not error text"
    );
    let generation = snapshot.persistence.fault_generation;

    // Only after the accepted partial result is durable may the explicit continue release it.
    store.commit_durable();
    within(thread.resume_storage(generation)).await.unwrap();
    assert!(!thread.snapshot().persistence.resume_required);
    within(thread.close()).await.unwrap();
}

/// One scripted reliable-output obligation: the first re-save is itself not durable yet.
///
/// A real archive retry either re-saves the same capture or fails again, so the test scripts the
/// worse first outcome — the retry cannot store the bytes yet — before the capture finally lands.
#[derive(Debug)]
struct ScriptedObligation {
    attempts: Mutex<usize>,
    stored: AtomicUsize,
}

impl ScriptedObligation {
    fn new() -> Self {
        Self {
            attempts: Mutex::new(0),
            stored: AtomicUsize::new(0),
        }
    }
}

impl OutputRetryObligation for ScriptedObligation {
    fn identity(&self) -> String {
        "capture.part".to_owned()
    }
    fn retry(&self) -> OutputRetryFuture<'_> {
        Box::pin(async move {
            let mut attempts = self.attempts.lock().unwrap();
            *attempts += 1;
            if *attempts == 1 {
                return Err(ColdStoreError {
                    source: Box::new(io::Error::other("archive still unavailable")),
                });
            }
            self.stored.fetch_add(1, Ordering::SeqCst);
            Ok(OutputRetryOutcome::Stored)
        })
    }

    fn received_bytes(&self) -> u64 {
        64
    }

    fn location(&self) -> String {
        "capture.part".to_owned()
    }

    fn kind(&self) -> StorageFaultKind {
        StorageFaultKind::WriteFailed
    }
}

/// A producer that could not store its capture reports the owed re-save through its typed fault.
#[derive(Debug)]
struct ObligationFaultTool {
    obligation: Arc<ScriptedObligation>,
}

impl Tool for ObligationFaultTool {
    async fn execute(
        &self,
        input: OpaquePayload,
        context: CallContext,
    ) -> Result<ToolOutput, ToolError> {
        let tasks = context
            .tasks
            .clone()
            .expect("a running call owns its access");
        tasks
            .report_output(ToolProgressUpdate::append("command-output", "partial"))
            .await
            .expect("the accepted increment is charged before the fault");
        let source = ColdStoreError {
            source: Box::new(io::Error::other("capture write failed")),
        };
        let fault = OutputStorageFault::new(StorageFaultKind::WriteFailed, Arc::new(source))
            .with_obligation(self.obligation.clone());
        Err(ToolError::new(fault)
            .with_output(ToolOutput::new(input.clone(), vec![text("partial")])))
    }
}

/// Accepted output a producer could not store is an obligation, not pressure a healthy save clears.
///
/// A healthy history writer makes the failed call's own result durable, but that says nothing about
/// the archive the producer could not store: the pause stays until the same obligation is retried and
/// its bytes really stored. The retry re-saves the exact capture, so a repeated retry is idempotent,
/// and only a successful re-save (plus the durability fence) releases the continue.
#[tokio::test]
async fn a_healthy_history_write_cannot_stand_in_for_a_failed_archive() {
    let store = GatedStore::new(Failure::Admission);
    // The writer itself never fails: only the producer's archive failed, so the healthy save must not
    // be mistaken for the archive becoming durable.
    store.state.lock().unwrap().armed = false;
    let (model, _requests) = ScriptedModel::new(&["store"]);
    let thread =
        ThreadHandle::start("archive-obligation".into(), DynModelSession::new(model)).unwrap();
    let obligation = Arc::new(ScriptedObligation::new());
    thread
        .register_tools(vec![
            Registration::new(
                "store".into(),
                OpaquePayload::text("Tool store"),
                ObligationFaultTool {
                    obligation: obligation.clone(),
                },
            )
            .unwrap(),
        ])
        .await
        .unwrap();
    thread
        .attach_storage(ColdStoreHandle::new(store.clone()))
        .await
        .unwrap();

    let runner = tokio::spawn({
        let thread = thread.clone();
        async move { thread.run_turn(turn("archive")).await }
    });
    let faulted = await_snapshot(&thread, |snapshot| snapshot.persistence.resume_required).await;
    assert_eq!(
        faulted.persistence.fault,
        Some(StorageFaultKind::WriteFailed)
    );
    let generation = faulted.persistence.fault_generation;
    assert!(
        !faulted.persistence.output_obligations.is_empty(),
        "the owed bytes are published for the retry view instead of being hidden"
    );
    // The Turn parks at its next safety point after committing the failed result; that safety point
    // admitted every published fact, so a durable writer really can cover them below. Stop driving
    // the detached Turn and drive the storage controls.
    drop(runner);
    await_snapshot(&thread, |snapshot| {
        snapshot.persistence.execution_phase == StorageExecutionPhase::PausedForStorage
    })
    .await;

    // A healthy writer durably saves every accepted fact, but that is not proof the archive landed.
    store.commit_durable();
    let refused = within(thread.resume_storage(generation)).await;
    assert!(
        matches!(refused, Err(ThreadError::StorageRecoveryUnverified { .. })),
        "a healthy history write must not stand in for the failed archive: {refused:?}"
    );
    assert!(thread.snapshot().persistence.resume_required);

    // The first re-save is itself not durable yet, so the obligation stays owed.
    assert!(
        within(thread.retry_output_storage()).await.is_err(),
        "a re-save that did not land leaves the obligation owed"
    );
    assert!(!thread.snapshot().persistence.output_obligations.is_empty());

    within(thread.retry_output_storage())
        .await
        .expect("the re-saved capture clears the obligation");
    assert!(thread.snapshot().persistence.output_obligations.is_empty());
    within(thread.retry_output_storage())
        .await
        .expect("retrying an already-stored obligation is a no-op");
    assert_eq!(
        obligation.stored.load(Ordering::SeqCst),
        1,
        "the same capture is stored once, never duplicated or lost"
    );

    // The fence, the backend verdict and the obligation now all hold, so the continue is released.
    within(thread.resume_storage(generation)).await.unwrap();
    assert!(!thread.snapshot().persistence.resume_required);
    within(thread.close()).await.unwrap();
}

/// The durable reference a retried archive stores for the repaired complete output.
fn repaired_reference() -> ResourceReference {
    ResourceReference::new(
        "pl.test.resource:repaired-output".to_owned(),
        format!("sha256:{}", "ab".repeat(32)),
        128,
        "application/octet-stream".to_owned(),
    )
    .expect("a valid resource reference")
}

/// A retriable obligation that stored the reference for the committed result of its own call.
#[derive(Debug)]
struct RepairObligation {
    call_id: String,
    identity: String,
    stored: Arc<AtomicUsize>,
}

impl OutputRetryObligation for RepairObligation {
    fn identity(&self) -> String {
        self.identity.clone()
    }
    fn retry(&self) -> OutputRetryFuture<'_> {
        Box::pin(async move {
            self.stored.fetch_add(1, Ordering::SeqCst);
            Ok(OutputRetryOutcome::StoredWithRepair(OutputRepair {
                call_id: self.call_id.clone(),
                reference: repaired_reference(),
            }))
        })
    }
    fn received_bytes(&self) -> u64 {
        64
    }
    fn location(&self) -> String {
        self.identity.clone()
    }
    fn kind(&self) -> StorageFaultKind {
        StorageFaultKind::WriteFailed
    }
}

/// A producer whose failed archive owes a re-save that stores a durable reference for its result.
#[derive(Debug)]
struct RepairFaultTool {
    stored: Arc<AtomicUsize>,
}

impl Tool for RepairFaultTool {
    async fn execute(
        &self,
        input: OpaquePayload,
        context: CallContext,
    ) -> Result<ToolOutput, ToolError> {
        let obligation: Arc<dyn OutputRetryObligation> = Arc::new(RepairObligation {
            call_id: context.call_id.clone(),
            identity: format!("capture:{}", context.call_id),
            stored: self.stored.clone(),
        });
        let source = ColdStoreError {
            source: Box::new(io::Error::other("capture archive failed")),
        };
        let fault = OutputStorageFault::new(StorageFaultKind::WriteFailed, Arc::new(source))
            .with_obligation(obligation);
        Err(ToolError::new(fault)
            .with_output(ToolOutput::new(input.clone(), vec![text("partial")])))
    }
}

/// A committed tool result carries the reference its retried archive stored, under the same call id.
///
/// The first archive attempt could not produce a reference, so the result commits without one; the
/// retry stores the bytes and names the *same* call identity, so the committed result is supplemented
/// in place — the repaired output keeps one identity instead of the reference becoming an orphan the
/// history and cold recovery cannot locate.
#[tokio::test]
async fn a_retried_archive_attaches_its_reference_to_the_committed_result() {
    let store = GatedStore::new(Failure::Admission);
    store.state.lock().unwrap().armed = false;
    let (model, _requests) = ScriptedModel::new(&["store"]);
    let thread = ThreadHandle::start("archive-repair".into(), DynModelSession::new(model)).unwrap();
    let stored = Arc::new(AtomicUsize::new(0));
    thread
        .register_tools(vec![
            Registration::new(
                "store".into(),
                OpaquePayload::text("Tool store"),
                RepairFaultTool {
                    stored: stored.clone(),
                },
            )
            .unwrap(),
        ])
        .await
        .unwrap();
    thread
        .attach_storage(ColdStoreHandle::new(store.clone()))
        .await
        .unwrap();

    let runner = tokio::spawn({
        let thread = thread.clone();
        async move { thread.run_turn(turn("archive-repair")).await }
    });
    let faulted = await_snapshot(&thread, |snapshot| snapshot.persistence.resume_required).await;
    assert_eq!(
        faulted.persistence.fault,
        Some(StorageFaultKind::WriteFailed)
    );
    let generation = faulted.persistence.fault_generation;
    // The failed archive committed its result *without* the durable reference: the first attempt
    // could not produce one, so nothing names the resource yet.
    assert!(
        !result_has_resource(&faulted, "pl.test.resource:repaired-output"),
        "the failed archive cannot have attached a reference to the committed result"
    );
    // The Turn parks at the storage safety point; stop driving it and let the retry store the bytes.
    drop(runner);
    await_snapshot(&thread, |snapshot| {
        snapshot.persistence.execution_phase == StorageExecutionPhase::PausedForStorage
    })
    .await;

    within(thread.retry_output_storage())
        .await
        .expect("the retried archive stores its reference");
    assert_eq!(
        stored.load(Ordering::SeqCst),
        1,
        "the same obligation is re-stored exactly once"
    );
    // The stored reference reaches the same committed result instead of an orphan blob, and the
    // record identity the call already had is unchanged.
    let repaired = thread.snapshot();
    assert!(
        result_has_resource(&repaired, "pl.test.resource:repaired-output"),
        "the committed tool result must gain the repaired durable reference"
    );
    // A repeated retry of an already-cleared obligation is a no-op and does not duplicate content.
    within(thread.retry_output_storage())
        .await
        .expect("retrying an already-stored obligation is a no-op");

    // The retry only clears the obligation: the explicit continue still proves the fixed durability
    // fence really landed, so the fake backend flushes every accepted effect before it can be asked
    // to release the pause. Releasing without this flush would be the "absence of an error is
    // recovery" the gate exists to refuse.
    store.commit_durable();
    within(thread.resume_storage(generation)).await.unwrap();
    within(thread.close()).await.unwrap();
}

/// A tool that reports a repairable archive fault while it is still running, then blocks.
///
/// The producer reports through the running call's own channel, so the user can retry the obligation
/// before the call has committed any result for the repaired reference to supplement.
#[derive(Debug)]
struct EarlyRepairTool {
    stored: Arc<AtomicUsize>,
    reported: mpsc::UnboundedSender<()>,
    release: Arc<Notify>,
}

impl Tool for EarlyRepairTool {
    async fn execute(
        &self,
        input: OpaquePayload,
        context: CallContext,
    ) -> Result<ToolOutput, ToolError> {
        let tasks = context
            .tasks
            .clone()
            .expect("a running call owns its access");
        let source = Arc::new(ColdStoreError {
            source: Box::new(io::Error::other("capture archive failed")),
        });
        let obligation: Arc<dyn OutputRetryObligation> = Arc::new(RepairObligation {
            call_id: context.call_id.clone(),
            identity: format!("capture:{}", context.call_id),
            stored: self.stored.clone(),
        });
        tasks
            .report_output_storage_fault(Arc::new(
                OutputStorageFault::new(StorageFaultKind::WriteFailed, source.clone())
                    .with_obligation(obligation.clone()),
            ))
            .await
            .expect("the running call reports its own storage fault");
        self.reported
            .send(())
            .expect("the test observes the report");
        // Stay in flight so the test can retry before the result is committed.
        self.release.notified().await;
        // The return path reports the same failure again — now possibly after the retry already
        // stored the bytes — so the repeat must not re-arm an obligation the Thread discharged.
        Err(ToolError::new(
            OutputStorageFault::new(StorageFaultKind::WriteFailed, source)
                .with_obligation(obligation),
        )
        .with_output(ToolOutput::new(input.clone(), vec![text("partial")])))
    }
}

/// A retry that lands *before* the result commits is applied by the commit that finally carries it.
///
/// The producer reports its failed archive while the call is still running, so the user retries
/// before any committed result exists. The reference must not be dropped and the pause must not be
/// released: the repair stays pending until the result is committed, then supplements that one
/// identity exactly once, and a repeat report of the already-stored obligation adds nothing back.
#[tokio::test]
async fn a_retry_before_the_result_commits_still_supplements_it_once() {
    let store = GatedStore::new(Failure::Admission);
    store.state.lock().unwrap().armed = false;
    let (model, _requests) = ScriptedModel::new(&["store"]);
    let thread = ThreadHandle::start("early-repair".into(), DynModelSession::new(model)).unwrap();
    let stored = Arc::new(AtomicUsize::new(0));
    let (reported, mut reports) = mpsc::unbounded_channel();
    let release = Arc::new(Notify::new());
    thread
        .register_tools(vec![
            Registration::new(
                "store".into(),
                OpaquePayload::text("Tool store"),
                EarlyRepairTool {
                    stored: stored.clone(),
                    reported,
                    release: release.clone(),
                },
            )
            .unwrap(),
        ])
        .await
        .unwrap();
    thread
        .attach_storage(ColdStoreHandle::new(store.clone()))
        .await
        .unwrap();

    let runner = tokio::spawn({
        let thread = thread.clone();
        async move { thread.run_turn(turn("early-repair")).await }
    });
    within(reports.recv())
        .await
        .expect("the fault is reported while the call is still running");
    let faulted = await_snapshot(&thread, |snapshot| snapshot.persistence.resume_required).await;
    let generation = faulted.persistence.fault_generation;
    assert!(
        faulted
            .tasks
            .values()
            .any(|task| task.status == TaskStatus::Running),
        "the call that reported the fault has not committed its result yet"
    );

    store.commit_durable();
    within(thread.retry_output_storage())
        .await
        .expect("the retried archive stores its reference");
    assert_eq!(
        stored.load(Ordering::SeqCst),
        1,
        "the accepted bytes are re-stored exactly once"
    );
    let pending = thread.snapshot();
    assert!(pending.persistence.output_obligations.is_empty());
    assert!(
        !result_has_resource(&pending, "pl.test.resource:repaired-output"),
        "a result that is not committed yet cannot carry the reference"
    );
    assert!(
        !pending.persistence.resume_ready,
        "a repair with no committed result yet must not offer a continue"
    );

    // The call finally finishes: the commit that carries its result applies the pending repair, so
    // the reference reaches a result that really exists instead of being dropped with the retry.
    release.notify_one();
    let repaired = await_snapshot(&thread, |snapshot| {
        result_has_resource(snapshot, "pl.test.resource:repaired-output")
    })
    .await;
    assert_eq!(
        repaired_resource_count(&repaired, "pl.test.resource:repaired-output"),
        1,
        "the repaired reference supplements the committed result exactly once"
    );
    // A repeat report of the same, already-stored obligation adds nothing back.
    within(thread.retry_output_storage())
        .await
        .expect("retrying an already-stored obligation is a no-op");
    assert!(
        thread.snapshot().persistence.output_obligations.is_empty(),
        "the return path's repeat must not re-arm an obligation the retry discharged"
    );
    assert_eq!(
        repaired_resource_count(&thread.snapshot(), "pl.test.resource:repaired-output"),
        1
    );

    // The Turn parks at the next storage safety point; only then can the durability fence be proven.
    await_snapshot(&thread, |snapshot| {
        snapshot.persistence.execution_phase == StorageExecutionPhase::PausedForStorage
    })
    .await;
    store.commit_durable();
    drop(runner);
    within(thread.resume_storage(generation)).await.unwrap();
    assert!(!thread.snapshot().persistence.resume_required);
    within(thread.close()).await.unwrap();
}

/// How many committed tool results carry the named durable resource reference.
fn repaired_resource_count(snapshot: &ThreadSnapshot, reference_id: &str) -> usize {
    snapshot
        .context
        .records
        .iter()
        .filter(|record| matches!(&record.source, ContextSource::ToolResult { .. }))
        .flat_map(|record| record.content.iter())
        .filter(|content| {
            matches!(content, ContextContent::Resource { reference } if reference.id() == reference_id)
        })
        .count()
}

/// Whether the committed result of any tool call carries the named durable resource reference.
fn result_has_resource(snapshot: &ThreadSnapshot, reference_id: &str) -> bool {
    snapshot.context.records.iter().any(|record| {
        matches!(&record.source, ContextSource::ToolResult { .. })
            && record.content.iter().any(|content| {
                matches!(content, ContextContent::Resource { reference } if reference.id() == reference_id)
            })
    })
}

/// A reliable-output obligation with a stable identity and an optional first-attempt failure.
#[derive(Debug)]
struct CountingObligation {
    identity: String,
    fail_first: AtomicBool,
    stored: AtomicUsize,
}

impl CountingObligation {
    fn new(identity: &str, fail_first: bool) -> Self {
        Self {
            identity: identity.to_owned(),
            fail_first: AtomicBool::new(fail_first),
            stored: AtomicUsize::new(0),
        }
    }
}

impl OutputRetryObligation for CountingObligation {
    fn identity(&self) -> String {
        self.identity.clone()
    }
    fn retry(&self) -> OutputRetryFuture<'_> {
        Box::pin(async move {
            if self.fail_first.swap(false, Ordering::SeqCst) {
                return Err(ColdStoreError {
                    source: Box::new(io::Error::other("the re-save is not durable yet")),
                });
            }
            self.stored.fetch_add(1, Ordering::SeqCst);
            Ok(OutputRetryOutcome::Stored)
        })
    }
    fn received_bytes(&self) -> u64 {
        8
    }
    fn location(&self) -> String {
        self.identity.clone()
    }
    fn kind(&self) -> StorageFaultKind {
        StorageFaultKind::WriteFailed
    }
}

/// Builds a typed capture/archive fault that carries one retriable obligation.
fn capture_obligation_fault(obligation: &Arc<CountingObligation>) -> OutputStorageFault {
    let source = ColdStoreError {
        source: Box::new(io::Error::other("capture write failed")),
    };
    let obligation: Arc<dyn OutputRetryObligation> = obligation.clone();
    OutputStorageFault::new(StorageFaultKind::WriteFailed, Arc::new(source))
        .with_obligation(obligation)
}

/// A running producer that signals it is in flight, waits for the whole cohort, then faults.
///
/// The barrier is what makes the concurrency real: the tool cannot report its fault until *every*
/// sibling is already running, so the owner's safety gate can never be mistaken for one obligation
/// overwriting another.
#[derive(Debug)]
struct ObligationReporterTool {
    obligation: Arc<CountingObligation>,
    reports: usize,
    started: mpsc::UnboundedSender<()>,
    barrier: Arc<Barrier>,
}

impl Tool for ObligationReporterTool {
    async fn execute(
        &self,
        input: OpaquePayload,
        context: CallContext,
    ) -> Result<ToolOutput, ToolError> {
        let tasks = context
            .tasks
            .clone()
            .expect("a running call owns its access");
        self.started.send(()).expect("the test observes the start");
        self.barrier.wait().await;
        for _ in 0..self.reports {
            tasks
                .report_output_storage_fault(Arc::new(capture_obligation_fault(&self.obligation)))
                .await
                .expect("the running call reports its fault");
        }
        Err(ToolError::new(capture_obligation_fault(&self.obligation))
            .with_output(ToolOutput::new(input.clone(), vec![text("partial")])))
    }
}

/// Two parallel failed archives keep independent obligations, and each is re-stored exactly once.
///
/// The owner keys obligations by stable identity, so B arriving while A is owed never overwrites A;
/// a repeated report of one identity adds nothing; and the pause is only released once *both*
/// obligations report `Stored`. Every identity is re-stored once, never duplicated or lost.
#[tokio::test]
async fn parallel_failed_archives_keep_independent_obligations() {
    let store = GatedStore::new(Failure::Admission);
    store.state.lock().unwrap().armed = false;
    let (model, _requests) = ScriptedModel::new(&["store-a", "store-b"]);
    let thread =
        ThreadHandle::start("parallel-obligations".into(), DynModelSession::new(model)).unwrap();
    let a = Arc::new(CountingObligation::new("capture-a", true));
    let b = Arc::new(CountingObligation::new("capture-b", false));
    // Two tools plus this test: releasing the barrier guarantees both tool calls are already running
    // before either injects its fault, so a missed obligation can only be a real overwrite.
    let barrier = Arc::new(Barrier::new(3));
    let (started, mut starts) = mpsc::unbounded_channel();
    thread
        .register_tools(vec![
            Registration::new(
                "store-a".into(),
                OpaquePayload::text("Tool A"),
                ObligationReporterTool {
                    obligation: a.clone(),
                    // Report the same identity twice: the second report must be idempotent.
                    reports: 2,
                    started: started.clone(),
                    barrier: barrier.clone(),
                },
            )
            .unwrap(),
            Registration::new(
                "store-b".into(),
                OpaquePayload::text("Tool B"),
                ObligationReporterTool {
                    obligation: b.clone(),
                    reports: 1,
                    started: started.clone(),
                    barrier: barrier.clone(),
                },
            )
            .unwrap(),
        ])
        .await
        .unwrap();
    thread
        .attach_storage(ColdStoreHandle::new(store.clone()))
        .await
        .unwrap();

    let runner = tokio::spawn({
        let thread = thread.clone();
        async move { thread.run_turn(turn("parallel")).await }
    });
    // Both actual tool calls must be running before any fault lands.
    within(starts.recv()).await.expect("the first tool starts");
    within(starts.recv()).await.expect("the second tool starts");
    barrier.wait().await;
    // Wait for *both* obligations to be latched, not merely the first fault.
    let faulted = await_snapshot(&thread, |snapshot| {
        snapshot.persistence.output_obligations.len() == 2
    })
    .await;
    assert_eq!(
        faulted.persistence.fault,
        Some(StorageFaultKind::WriteFailed)
    );
    let generation = faulted.persistence.fault_generation;
    assert_eq!(
        faulted.persistence.output_obligations.len(),
        2,
        "two parallel failed archives keep two obligations instead of overwriting one another"
    );
    // The Turn parks at its next safety point after committing the failed results; drive the
    // storage controls against the parked owner.
    drop(runner);
    await_snapshot(&thread, |snapshot| {
        snapshot.persistence.execution_phase == StorageExecutionPhase::PausedForStorage
    })
    .await;

    store.commit_durable();
    // A's first re-save is itself not durable yet: the whole generation stays owed, so the pause is
    // not released and B is not silently cleared with it.
    assert!(within(thread.retry_output_storage()).await.is_err());
    assert_eq!(thread.snapshot().persistence.output_obligations.len(), 2);
    assert!(within(thread.resume_storage(generation)).await.is_err());

    within(thread.retry_output_storage())
        .await
        .expect("both re-saves clear once each succeeds");
    assert!(thread.snapshot().persistence.output_obligations.is_empty());
    assert_eq!(
        a.stored.load(Ordering::SeqCst),
        1,
        "the repeated report did not duplicate the obligation"
    );
    assert_eq!(
        b.stored.load(Ordering::SeqCst),
        1,
        "the independent obligation is re-stored exactly once"
    );
    within(thread.resume_storage(generation)).await.unwrap();
    assert!(!thread.snapshot().persistence.resume_required);
    within(thread.close()).await.unwrap();
}

/// A tool that reports a storage fault while it is still running, then blocks.
#[derive(Debug)]
struct ImmediateFaultTool {
    started: mpsc::UnboundedSender<()>,
    reported: mpsc::UnboundedSender<()>,
    release: Arc<Notify>,
}

impl Tool for ImmediateFaultTool {
    async fn execute(
        &self,
        input: OpaquePayload,
        context: CallContext,
    ) -> Result<ToolOutput, ToolError> {
        self.started
            .send(())
            .expect("the test observes the tool start");
        let tasks = context
            .tasks
            .clone()
            .expect("a running call owns its access");
        let source = ColdStoreError {
            source: Box::new(io::Error::other("capture write failed")),
        };
        let fault = Arc::new(OutputStorageFault::new(
            StorageFaultKind::WriteFailed,
            Arc::new(source),
        ));
        tasks
            .report_output_storage_fault(fault)
            .await
            .expect("the running call reports its own storage fault");
        self.reported.send(()).expect("the test observes the claim");
        // Stay in flight so the test reads the latch while the call is still running.
        self.release.notified().await;
        Ok(ToolOutput::new(input.clone(), vec![text(input.content())]))
    }
}

/// A running call's storage fault blocks admission the moment it is reported, not when it returns.
///
/// The producer reports the typed fault through the running call's own reliable channel, so the
/// Thread latches it and stops funding further model/tool work *while the call is still in flight*,
/// instead of the fault staying invisible until `execute` happens to unwind. A parallel call admitted
/// after the report would otherwise run against a storage fact the Thread already knows is broken.
#[tokio::test]
async fn a_running_calls_storage_fault_blocks_admission_before_it_returns() {
    let store = GatedStore::new(Failure::Admission);
    store.state.lock().unwrap().armed = false;
    let (model, _requests) = ScriptedModel::new(&["store"]);
    let thread =
        ThreadHandle::start("immediate-fault".into(), DynModelSession::new(model)).unwrap();
    let (started, mut starts) = mpsc::unbounded_channel();
    let (reported, mut reports) = mpsc::unbounded_channel();
    let release = Arc::new(Notify::new());
    thread
        .register_tools(vec![
            Registration::new(
                "store".into(),
                OpaquePayload::text("Tool store"),
                ImmediateFaultTool {
                    started,
                    reported,
                    release: release.clone(),
                },
            )
            .unwrap(),
        ])
        .await
        .unwrap();
    thread
        .attach_storage(ColdStoreHandle::new(store.clone()))
        .await
        .unwrap();

    let runner = tokio::spawn({
        let thread = thread.clone();
        async move { thread.run_turn(turn("immediate")).await }
    });
    within(starts.recv())
        .await
        .expect("the tool starts before its fault");
    within(reports.recv())
        .await
        .expect("the fault is reported while the call is still running");
    let faulted = await_snapshot(&thread, |snapshot| snapshot.persistence.resume_required).await;
    assert_eq!(
        faulted.persistence.fault,
        Some(StorageFaultKind::WriteFailed),
        "the running call's typed fault is latched, not guessed from error text"
    );
    assert!(
        faulted
            .tasks
            .values()
            .any(|task| task.status == TaskStatus::Running),
        "the fault must be visible while the call that reported it is still in flight"
    );

    release.notify_one();
    drop(runner);
    // The Turn stays parked at its storage safety point; the harness is dropped with the test.
}

/// An explicit flush stays reachable while a Turn is paused on storage.
///
/// The request travels the mailbox the paused Turn services, so it resolves with the store's own
/// answer instead of waiting for a Turn that cannot end on its own. It must not re-execute anything
/// and must not release the pause by itself.
#[tokio::test]
async fn flush_remains_reachable_during_a_storage_pause() {
    let store = GatedStore::new(Failure::QueueFull);
    let (model, requests) = ScriptedModel::new(&["write"]);
    let thread = ThreadHandle::start("flushed".into(), DynModelSession::new(model)).unwrap();
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
        async move { thread.run_turn(turn("flushed")).await }
    });
    paused(&thread).await;
    assert!(!runner.is_finished());
    // The paused owner still services the flush at its safety point: the admitted watermark is
    // already durable in this fake, so the fixed-target barrier resolves instead of waiting on a
    // Turn that cannot end on its own.
    within(thread.flush()).await.unwrap();
    assert_eq!(requests.lock().unwrap().len(), 1);
    assert!(executions.lock().unwrap().is_empty());
    assert!(!runner.is_finished(), "a flush must not resume the Turn");
    within(thread.interrupt_turn(None)).await.unwrap();
    let _ = tokio::time::timeout(Duration::from_secs(5), runner)
        .await
        .expect("interruption did not reach the paused turn");
    within(thread.close()).await.unwrap();
}

/// A tool that accepts one preview, has a larger one refused, then keeps running past the owner's
/// answer.
#[derive(Debug)]
struct ProgressingTool {
    started: mpsc::UnboundedSender<()>,
    accepted: mpsc::UnboundedSender<()>,
    release: Arc<Notify>,
    preview_rejected: Arc<Mutex<bool>>,
}

impl Tool for ProgressingTool {
    async fn execute(
        &self,
        input: OpaquePayload,
        context: CallContext,
    ) -> Result<ToolOutput, ToolError> {
        self.started
            .send(())
            .expect("the test observes the tool start");
        let tasks = context.tasks.expect("a running call owns its task access");
        // A first increment the quota can hold, so the refused one below has accepted bytes it must
        // keep instead of dropping them with the refused replacement.
        tasks
            .report_output(ToolProgressUpdate::append("command-output", "kept"))
            .await
            .expect("the first preview fits the reserved quota");
        self.accepted
            .send(())
            .expect("the test observes the accepted preview");
        let preview = "p".repeat(4096);
        if tasks
            .report_output(ToolProgressUpdate::replace("command-output", preview))
            .await
            .is_err()
        {
            *self.preview_rejected.lock().unwrap() = true;
        }
        // The refusal must not make the owner wait for this call: the test keeps the tool blocked so
        // it can read the published fault while the preview's producer is still in flight.
        self.release.notified().await;
        Ok(ToolOutput::new(input.clone(), vec![text(input.content())]))
    }
}

/// A refused tool preview publishes its typed fault before the tool returns.
///
/// The preview is charged on the owner while the call is still in flight, so the truncation is a
/// fact the owner already owns. A tool that keeps working after its preview was refused must not
/// hold that fault — nor the admission block that goes with it — invisible until it happens to
/// commit: the fault has to be published the moment the charge is refused. The test reads it off the
/// live snapshot while the tool is still blocked, which a commit-time-only latch could not satisfy.
#[tokio::test]
async fn refused_tool_preview_publishes_the_fault_before_the_tool_returns() {
    let store = GatedStore::new(Failure::Admission);
    // No admission failure: this test isolates the in-flight output budget.
    store.state.lock().unwrap().armed = false;
    store.set_operation_limit(64);
    let (model, _requests) = ScriptedModel::new(&["stream"]);
    let thread = ThreadHandle::start("tool-budget".into(), DynModelSession::new(model)).unwrap();
    let (started, mut starts) = mpsc::unbounded_channel();
    let (accepted, mut acceptances) = mpsc::unbounded_channel();
    let release = Arc::new(Notify::new());
    let preview_rejected = Arc::new(Mutex::new(false));
    thread
        .register_tools(vec![
            Registration::new(
                "stream".into(),
                OpaquePayload::text("Tool stream"),
                ProgressingTool {
                    started,
                    accepted,
                    release: release.clone(),
                    preview_rejected: preview_rejected.clone(),
                },
            )
            .unwrap(),
        ])
        .await
        .unwrap();
    thread
        .attach_storage(ColdStoreHandle::new(store.clone()))
        .await
        .unwrap();

    let mut input = turn("slow-progress");
    input.max_model_steps = ModelStepLimit::Limited(1.try_into().unwrap());
    let runner = tokio::spawn({
        let thread = thread.clone();
        async move { thread.run_turn(input).await }
    });
    tokio::time::timeout(Duration::from_secs(10), starts.recv())
        .await
        .expect("the tool starts before its preview is refused")
        .expect("the tool reported its start");
    tokio::time::timeout(Duration::from_secs(10), acceptances.recv())
        .await
        .expect("the tool reports the preview the owner accepted")
        .expect("the accepted preview reached the owner");
    // The dispatch returns with the call still in flight: the tool stays blocked on its release.
    let _ = tokio::time::timeout(Duration::from_secs(5), runner).await;
    let mut snapshots = thread.subscribe();
    let generation = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let snapshot = snapshots.next().await.expect("thread remains observable");
            if snapshot.persistence.resume_required {
                assert_eq!(
                    snapshot.persistence.fault,
                    Some(pl_core::thread::cold::StorageFaultKind::QueueFull),
                    "a truncated preview travels as a typed storage fault, not error text"
                );
                assert!(
                    snapshot
                        .tasks
                        .values()
                        .any(|task| task.status == TaskStatus::Running),
                    "the fault must be visible while the tool is still running"
                );
                // The refused replacement kept nothing, but the increment accepted before it is
                // still the running call's authoritative live output: a truncation must not drop
                // the bytes this Thread already paid for and already published.
                let running = snapshot
                    .tasks
                    .values()
                    .find(|task| task.status == TaskStatus::Running)
                    .expect("a running task to read the accepted preview of");
                let progress = snapshot
                    .tool_progress
                    .get(&running.id)
                    .expect("the accepted preview stays resident with its running call");
                assert_eq!(
                    progress.content().text(),
                    "kept",
                    "a refused preview must keep the last accepted output"
                );
                return snapshot.persistence.fault_generation;
            }
        }
    })
    .await
    .expect(
        "a refused preview must publish the typed fault without waiting for the call to return",
    );
    // The reply reaches the producer right after the latch, so it is observed a moment later.
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if *preview_rejected.lock().unwrap() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("the producer must see the refusal it was charged for");

    release.notify_one();
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let snapshot = snapshots.next().await.expect("thread remains observable");
            if !snapshot
                .tasks
                .values()
                .any(|task| task.status == TaskStatus::Running)
            {
                break;
            }
        }
    })
    .await
    .expect("the released tool commits its accepted result");

    store.commit_durable();
    within(thread.resume_storage(generation)).await.unwrap();
    assert!(!thread.snapshot().persistence.resume_required);
    within(thread.close()).await.unwrap();
}

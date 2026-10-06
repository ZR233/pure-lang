use crate::api::studio::bridge_runtime::{active_bridge, installed_bridge};
use crate::api::studio::convert::runtime::bridge_update_state;
use crate::api::studio::types::{BridgeError, BridgeShutdownIssue, BridgeUpdaterStateSnapshot};
use crate::frb_generated::StreamSink;
use anyhow::Result;
use flutter_rust_bridge::frb;
use futures::FutureExt;
use futures::future::{BoxFuture, Shared};
use pl_studio_runtime::{StudioUpdateCancellation, StudioUpdateError, StudioUpdateErrorCode};
use std::sync::atomic::{AtomicU8, AtomicU64, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};
use tokio::sync::{Mutex, mpsc};
use tokio::task::JoinHandle;

use super::lifecycle::shutdown_runtime_for_update;

/// Bound for taking the update registry lock during a broadcast so a contended registry can never
/// stall the early exit broadcast (or the native deadline) that owns the single first deadline.
const UPDATE_REGISTRY_LOCK_LIMIT: Duration = Duration::from_millis(100);

/// Owned, multi-observable completion of one update task (install or progress sink).
///
/// A `Shared` future keeps the real result so `cancel()` (Dart) and the exit observation both see
/// the same outcome; dropping a waiter on timeout never cancels the underlying task nor loses the
/// ability to observe it later.
type OperationCompletion = Shared<BoxFuture<'static, Result<(), Arc<String>>>>;

/// Which owned task a completion belongs to.
#[derive(Clone, Copy)]
enum OwnerTask {
    Install,
    Sink,
}

const TASK_PENDING: u8 = 0;
const TASK_SUCCEEDED: u8 = 1;
const TASK_FAILED: u8 = 2;
const SINK_NOT_OPENED: u8 = 0;
const SINK_PENDING: u8 = 1;
const SINK_SUCCEEDED: u8 = 2;
const SINK_FAILED: u8 = 3;

/// Owner-published completion facts for one update operation.
///
/// The values are written by the owned observer task the moment the real task ends, never by a
/// waiter's first poll. That makes a completed handoff observable even when nobody awaits its
/// completion — e.g. the desktop exits right after the installer-launch handoff without calling
/// `finishHandoff`/`cancel`.
///
/// `#[frb(ignore)]` keeps this pure internal ownership state out of the generated API: `cargo-expand`
/// turns `#[derive(Default)]` into an `impl Default`, and the code generator otherwise treats
/// `Default::default` as a public constructor and exports the private type as an opaque handle.
#[frb(ignore)]
#[derive(Default)]
struct OperationCompletionFacts {
    task: AtomicU8,
    sink: AtomicU8,
}

impl OperationCompletionFacts {
    fn task_ended(&self) -> bool {
        self.task.load(Ordering::SeqCst) != TASK_PENDING
    }

    fn sink_ended(&self) -> bool {
        self.sink.load(Ordering::SeqCst) != SINK_PENDING
    }

    /// No owned task is still running, so there is nothing left to cancel. A progress sink that
    /// was never opened counts as ended.
    fn is_quiescent(&self) -> bool {
        self.task_ended() && self.sink_ended()
    }

    /// Only a fully successful operation whose owners all confirmed is prunable. Timeouts, task
    /// panics and an unconfirmed progress sink keep the strong owner registered.
    fn is_settled(&self) -> bool {
        self.task.load(Ordering::SeqCst) == TASK_SUCCEEDED
            && matches!(
                self.sink.load(Ordering::SeqCst),
                SINK_NOT_OPENED | SINK_SUCCEEDED
            )
    }
}

/// Registry of live update operations. Entries are strong owners so a pending operation survives a
/// dropped Dart handle; they are pruned only once the install task has actually settled.
static UPDATE_OPERATIONS: OnceLock<Mutex<Vec<Arc<BridgeStudioUpdateOperationInner>>>> =
    OnceLock::new();
static NEXT_OPERATION_ID: AtomicU64 = AtomicU64::new(1);

pub struct BridgeStudioUpdateOperation {
    inner: Arc<BridgeStudioUpdateOperationInner>,
}

struct BridgeStudioUpdateOperationInner {
    id: u64,
    cancellation: StudioUpdateCancellation,
    /// Owned install task completion; set once at spawn and retained for every later observer.
    task: Mutex<Option<OperationCompletion>>,
    /// Owned progress-sink task completion; set when Dart opens the progress stream.
    sink: Mutex<Option<OperationCompletion>>,
    /// Progress events plus the authoritative terminal failure the Dart stream must observe.
    progress_receiver:
        Mutex<Option<mpsc::Receiver<Result<BridgeUpdaterStateSnapshot, BridgeError>>>>,
    /// Whether the failed-update recovery restart has already been attempted.
    restart_started: Mutex<bool>,
    /// Owner-published completion facts for the install task and the progress sink.
    facts: Arc<OperationCompletionFacts>,
}

impl BridgeStudioUpdateOperationInner {
    fn is_settled(&self) -> bool {
        self.facts.is_settled()
    }

    fn is_quiescent(&self) -> bool {
        self.facts.is_quiescent()
    }
}

/// Wraps a spawned task into a shared completion and publishes its terminal fact from an owned
/// observer task.
///
/// The observer awaits the real `JoinHandle`, so the fact is recorded even when no waiter ever
/// polls the returned shared completion. A task that returns `Err` (its own inner failure, e.g. a
/// progress projection task) or panics is a real stop failure; only an aborted task reports
/// success, because cancellation is the expected terminal state.
fn owned_completion(
    handle: JoinHandle<Result<(), String>>,
    operation_id: u64,
    facts: Arc<OperationCompletionFacts>,
    owner: OwnerTask,
) -> OperationCompletion {
    // The observer owns the real `JoinHandle` and publishes the terminal fact the moment the task
    // ends, so a completed handoff is observable even when no waiter ever polls the returned
    // completion (the desktop may exit right after the installer-launch handoff). A one-shot
    // channel keeps the result buffered until some waiter observes it.
    let (published_tx, published_rx) = tokio::sync::oneshot::channel::<Result<(), Arc<String>>>();
    tokio::spawn(async move {
        let outcome = match handle.await {
            Ok(Ok(())) => Ok(()),
            Ok(Err(error)) => Err(Arc::new(error)),
            Err(error) if error.is_cancelled() => Ok(()),
            // Never propagate the raw panic payload: it can carry arbitrary caller-controlled text.
            // The global panic hook already persists the payload and backtrace to the crash log.
            Err(_) => Err(Arc::new(match owner {
                OwnerTask::Install => {
                    format!("Studio update operation {operation_id} install task panicked")
                }
                OwnerTask::Sink => {
                    format!("Studio update operation {operation_id} progress task panicked")
                }
            })),
        };
        if outcome.is_err() {
            // Record the key safety diagnostic the moment the owner actually fails, instead of
            // waiting for a future exit to observe it. Stable identifiers and a forced backtrace
            // only; the (possibly caller-controlled) error text is never written to the log.
            let failure_code = match owner {
                OwnerTask::Install => "installTaskFailed",
                OwnerTask::Sink => "progressTaskFailed",
            };
            let correlation_id = pl_protocol::studio::StudioError::internal().correlation_id;
            tracing::error!(
                pid = std::process::id(),
                stage = "updateOperations",
                code = failure_code,
                resource = operation_id,
                correlation_id = %correlation_id,
                backtrace = %std::backtrace::Backtrace::force_capture(),
                "Studio update operation owner task failed"
            );
        }
        let status = match (owner, outcome.is_ok()) {
            (OwnerTask::Install, true) => TASK_SUCCEEDED,
            (OwnerTask::Install, false) => TASK_FAILED,
            (OwnerTask::Sink, true) => SINK_SUCCEEDED,
            (OwnerTask::Sink, false) => SINK_FAILED,
        };
        match owner {
            OwnerTask::Install => facts.task.store(status, Ordering::SeqCst),
            OwnerTask::Sink => facts.sink.store(status, Ordering::SeqCst),
        }
        // The receiver may already be dropped; the fact above is still published.
        let _ = published_tx.send(outcome);
    });
    async move {
        published_rx.await.unwrap_or_else(|_| {
            // The observer ended without publishing; never report a clean stop.
            Err(Arc::new(
                "Studio update operation completion was never published".to_string(),
            ))
        })
    }
    .boxed()
    .shared()
}

fn prune_settled(registry: &mut Vec<Arc<BridgeStudioUpdateOperationInner>>) {
    registry.retain(|operation| !operation.is_settled());
}

impl BridgeStudioUpdateOperation {
    pub async fn progress_stream(
        &self,
        sink: StreamSink<BridgeUpdaterStateSnapshot>,
    ) -> Result<(), BridgeError> {
        let mut receiver = self
            .inner
            .progress_receiver
            .lock()
            .await
            .take()
            .ok_or_else(|| {
                BridgeError::invalid_argument("update progress stream can only be opened once")
            })?;
        self.inner.facts.sink.store(SINK_PENDING, Ordering::SeqCst);
        let inner = Arc::clone(&self.inner);
        let task = tokio::spawn(async move {
            while let Some(event) = receiver.recv().await {
                let sent = match event {
                    Ok(state) => sink.add(state),
                    // StreamSink's generated error decoder consumes AnyhowException,
                    // independently of the method's typed opening error.
                    Err(error) => sink.add_error(anyhow::anyhow!(
                        "{} ({})",
                        error.message,
                        error.correlation_id
                    )),
                };
                if sent.is_err() {
                    let _ = inner.cancellation.cancel();
                    break;
                }
            }
            Ok::<(), String>(())
        });
        *self.inner.sink.lock().await = Some(owned_completion(
            task,
            self.inner.id,
            Arc::clone(&self.inner.facts),
            OwnerTask::Sink,
        ));
        Ok(())
    }

    /// Cancels this operation and reports the *real* stop result.
    ///
    /// Both the cancellation request error (`CancellationTooLate` when the installer is already
    /// launching) and any task failure are surfaced instead of being swallowed, so a caller cannot
    /// receive a false "cancelled" acknowledgement while the operation keeps running.
    pub async fn cancel(&self) -> Result<(), BridgeError> {
        self.inner.cancellation.cancel()?;
        self.inner.wait().await.map_err(|error| {
            BridgeError::from(anyhow::anyhow!(
                "Studio update operation {} did not stop cleanly: {error}",
                self.inner.id
            ))
        })
    }

    /// Completes the actual process handoff; true tells the old GUI to exit.
    pub async fn finish_handoff(&self) -> Result<bool, BridgeError> {
        // Wait for the owners to actually stop and keep the real failure: a panicked or stuck
        // projection task must not be reported as a clean handoff.
        let stop_result = self.inner.wait().await;
        // The installer-launch fact wins: once the installer really launched, the old program must
        // exit instead of restarting, even if the durable projection failed afterwards.
        if self.inner.cancellation.installer_launched() {
            return Ok(true);
        }
        let mut started = self.inner.restart_started.lock().await;
        if !*started {
            *started = installed_bridge()?
                .studio
                .restart_after_failed_update()
                .await?;
        }
        if *started {
            return Ok(true);
        }
        // No handoff and no recovery: surface the real task failure instead of pretending clean.
        match stop_result {
            Ok(()) => Ok(false),
            Err(error) => Err(BridgeError::from(anyhow::anyhow!(
                "Studio update operation {} did not stop cleanly: {error}",
                self.inner.id
            ))),
        }
    }
}

impl Drop for BridgeStudioUpdateOperation {
    fn drop(&mut self) {
        let _ = self.inner.cancellation.cancel();
    }
}

impl BridgeStudioUpdateOperationInner {
    /// Observes the operation's owned task completions; returns the first real failure.
    ///
    /// Both completions are shared and retained, so concurrent callers (Dart `cancel()` and the
    /// shutdown aggregation) observe the same outcome and a timeout never loses the owner.
    async fn wait(&self) -> Result<(), String> {
        let mut first_error: Option<String> = None;
        let task = self.task.lock().await.clone();
        let sink = self.sink.lock().await.clone();
        if let Some(completion) = task
            && let Err(error) = completion.await
        {
            first_error = Some((*error).clone());
        }
        if let Some(completion) = sink
            && let Err(error) = completion.await
            && first_error.is_none()
        {
            first_error = Some((*error).clone());
        }
        match first_error {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }
}

pub async fn check_studio_update() -> Result<BridgeUpdaterStateSnapshot, BridgeError> {
    let bridge = active_bridge().await?;
    let state = bridge.studio.check_studio_update().await?;
    Ok(bridge_update_state(state))
}

pub async fn read_studio_update_state() -> Result<BridgeUpdaterStateSnapshot, BridgeError> {
    let bridge = installed_bridge()?;
    Ok(bridge_update_state(bridge.studio.read_update_state().await))
}

pub async fn install_studio_update(
    expected_revision: u64,
    version: String,
) -> Result<BridgeStudioUpdateOperation, BridgeError> {
    let bridge = active_bridge().await?;
    if bridge.studio.is_busy_for_update().await? {
        return Err(StudioUpdateError::new(
            StudioUpdateErrorCode::RuntimeBusy,
            "Studio runtime has an active turn or task",
        )
        .into());
    }
    let update = bridge
        .studio
        .verified_studio_update(expected_revision, &version)
        .await?;
    let cancellation = StudioUpdateCancellation::new();
    let (bridge_progress_tx, bridge_progress_rx) = mpsc::channel(64);
    let operation_id = NEXT_OPERATION_ID.fetch_add(1, Ordering::Relaxed);
    let facts = Arc::new(OperationCompletionFacts::default());
    let inner = Arc::new(BridgeStudioUpdateOperationInner {
        id: operation_id,
        cancellation: cancellation.clone(),
        task: Mutex::new(None),
        sink: Mutex::new(None),
        progress_receiver: Mutex::new(Some(bridge_progress_rx)),
        restart_started: Mutex::new(false),
        facts: Arc::clone(&facts),
    });
    let task_facts = Arc::clone(&facts);
    let task = tokio::spawn(async move {
        let (progress_tx, mut progress_rx) = tokio::sync::mpsc::unbounded_channel();
        let final_progress_tx = bridge_progress_tx.clone();
        let forward_cancellation = cancellation.clone();
        let forward = tokio::spawn(async move {
            while let Some(state) = progress_rx.recv().await {
                if bridge_progress_tx
                    .send(Ok(bridge_update_state(state)))
                    .await
                    .is_err()
                {
                    let _ = forward_cancellation.cancel();
                    break;
                }
            }
        });
        let progress_for_install = progress_tx.clone();
        let result = bridge
            .studio
            .install_studio_update_after(update, progress_for_install, cancellation, || async {
                if !shutdown_runtime_for_update(bridge, operation_id)
                    .await
                    .map_err(runtime_error)?
                {
                    return Err(StudioUpdateError::new(
                        StudioUpdateErrorCode::RuntimeBusy,
                        "Studio runtime became busy before installer launch",
                    ));
                }
                Ok(())
            })
            .await;
        drop(progress_tx);
        match forward.await {
            Ok(()) => {
                if let Err(error) = result {
                    let _ = final_progress_tx.send(Err(error.into())).await;
                }
                Ok(())
            }
            // A nested progress task panic must not be reported as a clean stop; it is a real
            // operation failure that both the Dart terminal stream and the shutdown aggregation
            // observe. The raw panic payload is not propagated; the global panic hook keeps it.
            Err(join) => {
                if join.is_cancelled() {
                    Ok(())
                } else {
                    let message = "Studio update progress projection task failed".to_string();
                    let _ = final_progress_tx
                        .send(Err(BridgeError::from(anyhow::anyhow!("{message}"))))
                        .await;
                    Err(message)
                }
            }
        }
    });
    *inner.task.lock().await = Some(owned_completion(
        task,
        operation_id,
        task_facts,
        OwnerTask::Install,
    ));
    {
        let mut registry = update_operations().lock().await;
        prune_settled(&mut registry);
        registry.push(Arc::clone(&inner));
    }
    Ok(BridgeStudioUpdateOperation { inner })
}

/// Live update operations captured by the broadcast phase and observed (joined) later within the
/// same shared deadline.
pub(crate) struct PendingUpdateOperations {
    owners: Vec<Arc<BridgeStudioUpdateOperationInner>>,
    issues: Vec<BridgeShutdownIssue>,
}

impl PendingUpdateOperations {
    /// Broadcast issues only; the captured owners are dropped without joining.
    pub(crate) fn into_issues(self) -> Vec<BridgeShutdownIssue> {
        self.issues
    }
}

/// Signals every in-flight update operation except `exclude` **without joining** any of them.
///
/// Safe to run from the early exit entry: `cancel()` is idempotent and non-blocking, so a stalled
/// install never blocks the broadcast. Real failures (`CancellationTooLate`) are returned so the
/// caller retains them for the same shutdown collector; one failure never stops the broadcast to
/// the remaining operations. The registry lock is bounded so a contended registry cannot stall the
/// native exit deadline.
///
/// The signal is split from the join so every independent owner is signalled before any bounded
/// join can block: a slow join of one owner must never consume the shared deadline before another
/// owner has been cancelled. The handoff owner (`exclude`) is never cancelled or observed here: it
/// is the caller, holding the install command lock and the install-active flag, so cancelling it
/// would self-cancel the handoff and awaiting it would wait on the very task running the hook. No
/// competing install blocks on those locks (`lock_install` uses a non-blocking `try_lock`,
/// `InstallGuard::acquire` a non-blocking compare-exchange), so a competing operation fails fast
/// instead of queueing and the bounded observation cannot deadlock.
///
/// An operation whose install task and progress sink have both actually ended is no longer an
/// active resource: it is skipped, so a completed handoff (whose `cancel()` would only report
/// `CancellationTooLate`) is not mistaken for a live resource that failed to stop. `include_stopped`
/// additionally captures such already-stopped operations for the desktop exit report, so a real
/// terminal failure still reaches the report instead of being dropped.
pub(crate) async fn broadcast_cancel_update_operations_excluding(
    exclude: Option<u64>,
    include_stopped: bool,
) -> PendingUpdateOperations {
    let operations = {
        match tokio::time::timeout(UPDATE_REGISTRY_LOCK_LIMIT, update_operations().lock()).await {
            Ok(mut registry) => {
                prune_settled(&mut registry);
                registry.iter().cloned().collect::<Vec<_>>()
            }
            Err(_) => {
                return PendingUpdateOperations {
                    owners: Vec::new(),
                    issues: vec![update_issue(
                        "timeout",
                        "the Studio update registry lock was not acquired within the early exit budget",
                    )],
                };
            }
        }
    };
    let mut issues = Vec::new();
    let mut owners = Vec::new();
    for operation in operations {
        if Some(operation.id) == exclude {
            continue;
        }
        if operation.is_quiescent() {
            if include_stopped {
                owners.push(operation);
            }
            continue;
        }
        match operation.cancellation.cancel() {
            Ok(()) => owners.push(operation),
            Err(error) => {
                let code = error.code().as_str().to_string();
                let correlation_id = pl_protocol::studio::StudioError::internal().correlation_id;
                tracing::error!(
                    pid = std::process::id(),
                    stage = "updateOperations",
                    code = %code,
                    resource = operation.id,
                    correlation_id = %correlation_id,
                    backtrace = %std::backtrace::Backtrace::force_capture(),
                    "Studio update operation could not be cancelled"
                );
                issues.push(update_issue_with_correlation(
                    &code,
                    &format!("a Studio update operation could not be cancelled: {error}"),
                    correlation_id,
                ));
                // Still observe its completion so the real terminal state, not the cancel error, is
                // what the report reflects; the owner stays registered on timeout.
                owners.push(operation);
            }
        }
    }
    PendingUpdateOperations { owners, issues }
}

/// Early-exit broadcast: signals every in-flight update operation, discarding the captured owners
/// because that entry point never joins them (the later desktop shutdown pass observes them).
pub(crate) async fn broadcast_cancel_update_operations() -> Vec<BridgeShutdownIssue> {
    broadcast_cancel_update_operations_excluding(None, false)
        .await
        .into_issues()
}

/// Bounded-observes previously broadcast update operations within the shared first deadline.
///
/// A real cancellation failure (`CancellationTooLate`), a task failure, or a timeout is reported
/// instead of swallowed; an operation that does not stop in time keeps its strong owner registered
/// so a later pass can still observe it.
pub(crate) async fn observe_update_operations(
    pending: PendingUpdateOperations,
    deadline: Instant,
) -> Vec<BridgeShutdownIssue> {
    let PendingUpdateOperations { owners, mut issues } = pending;
    // Observe every owner **concurrently** within the same first deadline: a slow operation must not
    // consume the budget before the remaining owners are observed. A timed-out or failed owner keeps
    // its strong registry entry so a later pass can still observe it.
    let outcomes = futures::future::join_all(owners.iter().map(|operation| async move {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return UpdateObservation::TimedOut;
        }
        match tokio::time::timeout(remaining, operation.wait()).await {
            Ok(Ok(())) => UpdateObservation::Stopped,
            Ok(Err(error)) => UpdateObservation::Failed(error),
            Err(_) => UpdateObservation::TimedOut,
        }
    }))
    .await;
    for outcome in outcomes {
        match outcome {
            UpdateObservation::Stopped => {}
            UpdateObservation::Failed(error) => issues.push(update_issue(
                "joinError",
                &format!("a Studio update operation task failed to stop: {error}"),
            )),
            UpdateObservation::TimedOut => issues.push(update_issue(
                "timeout",
                "a Studio update operation did not stop within the exit budget; its owner is retained",
            )),
        }
    }
    issues
}

/// Concurrent per-owner outcome of one bounded update-operation observation.
enum UpdateObservation {
    Stopped,
    Failed(String),
    TimedOut,
}

fn update_issue(code: &str, message: &str) -> BridgeShutdownIssue {
    update_issue_with_correlation(
        code,
        message,
        pl_protocol::studio::StudioError::internal().correlation_id,
    )
}

fn update_issue_with_correlation(
    code: &str,
    message: &str,
    correlation_id: String,
) -> BridgeShutdownIssue {
    BridgeShutdownIssue {
        stage: "updateOperations".to_string(),
        code: code.to_string(),
        message: message.to_string(),
        retryable: code == "timeout",
        correlation_id,
    }
}

fn update_operations() -> &'static Mutex<Vec<Arc<BridgeStudioUpdateOperationInner>>> {
    UPDATE_OPERATIONS.get_or_init(|| Mutex::new(Vec::new()))
}

fn runtime_error(error: BridgeError) -> StudioUpdateError {
    StudioUpdateError::new(
        StudioUpdateErrorCode::RuntimeShutdownFailed,
        format!(
            "failed to stop Studio runtime safely: {} ({})",
            error.message, error.correlation_id
        ),
    )
}

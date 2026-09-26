//! Nonblocking effect admission and explicit durability barriers for Thread persistence.
use super::*;
use futures::future::BoxFuture;
use std::{fmt, future::Future, sync::Arc};

/// One fixed persistence ticket. The checkpoint may only publish after the effect fence is durable.
#[derive(Debug, Clone)]
pub struct ThreadWrite {
    pub effect: Arc<ThreadEffectBatch>,
    pub checkpoint: ThreadCheckpoint,
    /// Operation whose reliable output reservation funds this batch, if any.
    ///
    /// Transient budgeting metadata, not a durable fact: the batch is written exactly as the
    /// projection published it. A backend that reserved a live-output ceiling for `operation_id`
    /// transfers it onto this batch at admission, so the output the operation produced is charged
    /// once — as the reservation while it was in flight, as this batch's own charge afterwards —
    /// instead of needing a second charge the process may not have.
    pub output_claim: Option<String>,
}

/// Storage failure retains its typed source; it never rolls back a Thread commit.
#[derive(Debug, thiserror::Error)]
#[error("cold store failed: {source}")]
pub struct ColdStoreError {
    #[source]
    pub source: Box<dyn std::error::Error + Send + Sync>,
}

/// Typed reliable-output storage failure a producer reports through its own boundary.
///
/// A producer that streams output it may have to keep — a tool backend whose capture write failed, or
/// an archive that could not be made durable — reports this through its typed error instead of a
/// plain tool failure. Core latches exactly the [`StorageFaultKind`] the producer names, so it never
/// parses the diagnostic text to decide which storage fact failed and never mistakes a hard storage
/// fault for a recoverable tool error that merely continues.
///
/// A producer whose accepted bytes are *not yet* stored carries an [`OutputRetryObligation`] with the
/// fault. Unlike a pure reliable-budget truncation, that output is not a fact the reliable writer can
/// save on its own: the obligation has to be retried and its bytes really stored before the Thread can
/// continue, so a healthy history write can never stand in for the archive that failed.
#[derive(Debug, Clone, thiserror::Error)]
#[error("reliable output could not be stored: {source}")]
pub struct OutputStorageFault {
    /// Typed category of the failed storage fact, chosen by the producer that observed it.
    pub kind: StorageFaultKind,
    #[source]
    pub source: Arc<ColdStoreError>,
    /// Accepted bytes that still have to be stored, with the handle that re-saves them.
    ///
    /// `None` for a producer failure whose accepted bytes are already durable (a bounded-capture
    /// truncation): those bytes are retained and the Thread may continue once the fence is durable.
    /// `Some` for a failure that owes a real re-save — an archive or capture write that could not be
    /// made durable — so the Thread stays paused until that same obligation is retried successfully.
    pub obligation: Option<Arc<dyn OutputRetryObligation>>,
}

impl OutputStorageFault {
    /// Wraps a producer's typed storage category and its retained source.
    pub fn new(kind: StorageFaultKind, source: Arc<ColdStoreError>) -> Self {
        Self {
            kind,
            source,
            obligation: None,
        }
    }

    /// Attaches the retriable obligation for accepted output that is not yet stored.
    ///
    /// The owner keeps the obligation under the fault generation it latched and only releases the
    /// pause once a retry of the same obligation reported the bytes stored, so retrying can never be
    /// satisfied by an unrelated healthy save.
    pub fn with_obligation(mut self, obligation: Arc<dyn OutputRetryObligation>) -> Self {
        self.obligation = Some(obligation);
        self
    }
}

/// Durable repair fact a retried producer obligation produces for an already-committed tool result.
///
/// A tool result is committed with the content its producer could project at the time; a failed
/// archive means it was committed *without* the durable resource reference for its complete output.
/// When the retry later stores those bytes, the producer names the already-committed call identity
/// the reference belongs to, so the owner attaches it to that same result instead of creating a
/// second one — the repaired output keeps one identity. Core never inspects a producer payload to
/// build the fact: the producer names the identity and the durable reference, and core only appends
/// the reference to that result's content.
///
/// The same typed fact is also the durable supplement the live projection and history writer apply
/// to the already-projected tool identity, so the reference reaches the product surface (and cold
/// recovery) rather than only the model-visible context record. It carries no producer structure,
/// so core never guesses an executor payload.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OutputRepair {
    /// Stable tool-call identity of the committed result the reference belongs to.
    pub call_id: String,
    /// Durable reference to the complete output the retry stored.
    pub reference: crate::context::ResourceReference,
}

/// Result of re-running one producer's reliable-output obligation.
#[derive(Debug, Clone)]
pub enum OutputRetryOutcome {
    /// The accepted bytes are now durably stored under their stable identity.
    Stored,
    /// The accepted bytes are now durably stored, and the committed tool result still needs the
    /// reference the producer could not project on its first attempt.
    StoredWithRepair(OutputRepair),
    /// The retrying backend does not own this obligation, so it stays owed.
    Unsupported,
}

/// Boxed future an [`OutputRetryObligation`] returns, so the trait stays object safe.
pub type OutputRetryFuture<'a> = BoxFuture<'a, Result<OutputRetryOutcome, ColdStoreError>>;

/// Typed, retriable obligation to store accepted reliable output that could not be made durable.
///
/// It names the exact accepted bytes under their stable identity — a capture fragment on disk, an
/// archive source — rather than a string or a tool rerun: retrying re-saves the same bytes, so a
/// repeated retry is idempotent and never loses or duplicates content. Core keeps the handle under
/// the fault generation it latched and releases the pause only once a retry of that same handle
/// reported [`OutputRetryOutcome::Stored`], so a healthy history write can never prove that the
/// failed archive was stored.
pub trait OutputRetryObligation: fmt::Debug + Send + Sync + 'static {
    /// Stable identity of *this* accepted output, shared by every report of the same obligation.
    ///
    /// The owner keys its outstanding obligations by this value, so a producer that reports the same
    /// failure twice — once through its live channel before the call returns and again on the return
    /// path — attaches one obligation, never a duplicate. It also lets the return path enrich a
    /// fault that was first reported without its bytes, instead of overwriting it.
    fn identity(&self) -> String;
    /// Re-saves the accepted bytes and reports whether they are now durably stored.
    ///
    /// # Errors
    /// Returns the retry's own typed storage failure; the obligation stays owed.
    fn retry(&self) -> OutputRetryFuture<'_>;
    /// Bytes accepted and kept for this obligation.
    fn received_bytes(&self) -> u64;
    /// Stable, readable location of the retained capture, for display and retry.
    fn location(&self) -> String;
    /// The producer's typed category for this obligation.
    fn kind(&self) -> StorageFaultKind;
}

/// Serializable view of an owed reliable-output obligation, for the UI and the explicit retry.
///
/// It is a live diagnostic of what still has to be stored, never durable Thread state: the owner
/// holds the real retriable handle and this only mirrors its typed facts.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OutputObligationState {
    /// Typed category the producer named for the unsaved output.
    pub kind: StorageFaultKind,
    /// Bytes already accepted and kept for this obligation.
    pub received_bytes: u64,
    /// Stable, readable location of the retained capture.
    pub location: String,
}

/// Typed storage report of one backend: retained bytes plus the durability receipt.
///
/// The receipt is what lets the owner release the live effect window without asking for an explicit
/// `flush`: a batch the backend already holds durably is answerable from history, so its resident
/// body is no longer needed. The byte counts are the retained memory that really exists at the
/// report, not just what is still unwritten: a backend also counts the product bodies its live
/// projection keeps after their batch became durable, so a Thread cannot claim to be drained while
/// it still holds its Turn. Completed but unsaved work may exceed the byte limits.
#[derive(Debug, Clone, Default)]
pub struct StoragePressure {
    /// Retained bytes of this one Thread: the effects its writer queue still holds, the batches still
    /// waiting to be written, the bodies its live projection and report accumulator still keep and
    /// the ceilings still reserved for its in-flight model/tool output. This is what pauses new
    /// model/tool admission at a safe gap.
    ///
    /// It is the Thread's own scope, so it covers the product bodies the process-wide budget does not
    /// count twice (a prepared batch and the projection hold the same content the queued effect was
    /// already charged for).
    pub thread_bytes: u64,
    /// Retained bytes of the backend's whole process budget, across every Thread.
    ///
    /// The backend's bytes are exactly the effects its channels hold, each at the size it was
    /// admitted with, plus the in-flight output ceilings still reserved per operation. A fact
    /// produced under a reservation transfers those bytes at admission instead of adding a second
    /// charge for the same output, so one body is counted once in this number.
    pub store_bytes: u64,
    /// Highest effect sequence this backend confirmed durable for the Thread.
    ///
    /// `0` means the backend has nothing of this Thread known durable yet. The owner folds the
    /// receipt at a memory-safe boundary and never waits on the owner command queue for it.
    pub durable_sequence: u64,
    /// Typed category of the backend's current storage fault, if any.
    ///
    /// The category is a value the backend already has (its own typed fault), never a parse of the
    /// error text: the text stays a diagnostic and cannot move the storage latch by itself.
    pub fault: Option<StorageFaultKind>,
    /// Fault generation this report belongs to.
    ///
    /// Manual recovery has to name the generation it is recovering, so a report from an older fault
    /// can never be mistaken for a newer one. Backends without generations report `0`.
    pub fault_generation: u64,
    /// Newest fault generation this backend has itself **verified** as recovered, if any.
    ///
    /// `None` means this backend reports no verified recovery: it either never latched a generation
    /// or re-latched the current one. "No error is being reported right now" is deliberately not the
    /// same fact — the backend names the generation whose retry reached *its own* target, so a
    /// recovering caller can require that verdict to be the generation it is releasing instead of
    /// accepting an older generation's success as proof for a newer fault.
    pub recovered_generation: Option<u64>,
    pub error: Option<Arc<ColdStoreError>>,
}

/// Typed storage fault categories a backend can report to its owner.
///
/// The owner pauses new model/tool admission at a storage safety point for every one of them. The
/// distinction that matters to the GUI is *which* fact failed, not the error text, so the category
/// travels as a value from the backend that produced it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum StorageFaultKind {
    /// The reliable queue is full: a transient admission backpressure, not a failed write.
    QueueFull,
    /// A history/calls/checkpoint write failed.
    WriteFailed,
    /// The writer task is gone (stopped or panicked) and retained facts are no longer being written.
    WriterUnavailable,
    /// The writer made no durable progress inside its own deadline.
    NoProgress,
    /// The checkpoint publication failed.
    CheckpointFailed,
    /// A referenced blob/attachment could not be made durable alongside its fact.
    BlobFailed,
}

/// Ceiling one model/tool operation may reserve from the reliable retention budget.
///
/// The provider's own output limit is not a local budget: a call whose stream would have to retain
/// more than this process can keep is cancelled with the bytes it already accepted instead of
/// letting the resident accumulator grow without bound. The quota itself comes from the attached
/// backend when it has one, so a Thread under pressure is granted less than this ceiling.
pub const MAX_OPERATION_OUTPUT_BYTES: u64 = 16 * 1024 * 1024;

/// One durable tool-task fact read back from the host store.
///
/// It is the authority a repeated read-only task query uses after the owner has released the
/// finished result from its transient window: the task identity/status come from the host's durable
/// call facts and `delivery` carries the full committed result body when the call reached a
/// terminal delivery. It is a read of durable facts, never a resident second history.
#[derive(Debug, Clone)]
pub struct DurableToolTask {
    pub task: super::task::TaskRecord,
    pub delivery: Option<ToolDelivery>,
}

/// Injected asynchronous sink. `admit` only queues immutable effect data and must not block.
pub trait ColdStore: Send + Sync + fmt::Debug + 'static {
    /// Reserves an observed presentation identity synchronously, before a coalescing preview
    /// can discard it. Backends without ordered presentation storage need not reserve anything.
    fn reserve_observed_item(
        &self,
        _thread_id: &str,
        _item_id: &str,
    ) -> Result<(), ColdStoreError> {
        Ok(())
    }
    /// Reserves a bounded reliable quota for one in-flight operation's live output.
    ///
    /// The quota comes from the same budget the reliable save path uses, so a model/tool step is
    /// only admitted when its streamed result could really be retained: `admit` may then reserve the
    /// fact it produces without discovering a shortfall after the bytes are already resident —
    /// a batch offered with its operation named in [`ThreadWrite::output_claim`] takes the reserved
    /// bytes over instead of being charged next to them. The returned value is the *granted* ceiling,
    /// never more than `max_bytes`; the producer charges what it accepted against it.
    ///
    /// A backend without a reliable budget grants the request unchanged, and core still bounds the
    /// operation itself, so the refusal path stays reachable for every backend.
    ///
    /// # Errors
    /// Refuses (typed) when the reliable budget cannot hold the quota at all. The operation must not
    /// start unfunded; nothing was lost yet, so this is backpressure rather than a hard fault.
    fn reserve_operation_output(
        &self,
        _thread_id: &str,
        _operation_id: &str,
        max_bytes: u64,
    ) -> Result<u64, ColdStoreError> {
        Ok(max_bytes)
    }
    /// Charges the bytes one operation really accepted against its reserved quota.
    ///
    /// Repeated charges are monotonic in `accepted_bytes` and idempotent. A charge the quota cannot
    /// hold is refused (typed) and is what makes the producer cancel the operation with the bytes it
    /// already accepted instead of buffering more; the backend reports the matching typed fault. The
    /// ceiling a charge is compared against is what is still reserved, so output that already became
    /// a fact leaves less room for the rest of the call.
    ///
    /// # Errors
    /// Refuses (typed) a body beyond the reserved quota.
    fn charge_operation_output(
        &self,
        _thread_id: &str,
        _operation_id: &str,
        _accepted_bytes: u64,
    ) -> Result<(), ColdStoreError> {
        Ok(())
    }
    /// Releases one operation's remaining quota once the call ended.
    ///
    /// The accepted bytes became the charge of the facts the call produced, so only the part no fact
    /// took over is given back here; releasing the whole original ceiling would drop the charge of
    /// those facts, and keeping it would leave a residue on every call.
    fn release_operation_output(&self, _thread_id: &str, _operation_id: &str) {}
    /// Reports current retained-byte pressure and the durable receipt without blocking on IO.
    ///
    /// The receipt is the newest effect sequence the backend already holds durably for this Thread;
    /// the owner folds it at a storage safety point and releases the covered live batches there.
    fn pressure(&self, _thread_id: &str) -> StoragePressure {
        StoragePressure::default()
    }
    /// Optional notification for a change in durable progress or fault state.
    ///
    /// A Thread waits for this signal at a storage safety point while still servicing its
    /// mailbox. Backends without a notification are polled at a bounded interval.
    fn subscribe_pressure(&self, _thread_id: &str) -> Option<tokio::sync::watch::Receiver<()>> {
        None
    }
    /// Accepts exactly this encoded effect batch; repeated admission is idempotent.
    fn admit(&self, thread_id: &str, write: ThreadWrite) -> Result<(), ColdStoreError>;
    /// Waits for previously admitted records to become durable, retaining failed writes for retry.
    fn flush(
        &self,
        thread_id: &str,
        sequence: u64,
    ) -> impl Future<Output = Result<(), ColdStoreError>> + Send;
    /// Reads one finished tool task by its core task identity from the durable store.
    ///
    /// A backend that keeps no task facts returns `Ok(None)`. Returning a non-terminal record means
    /// the delivery is not durable yet; callers must not present that as a finished result.
    fn read_tool_task(
        &self,
        _thread_id: &str,
        _task_id: &str,
    ) -> impl Future<Output = Result<Option<DurableToolTask>, ColdStoreError>> + Send {
        async { Ok(None) }
    }
}
trait ErasedColdStore: Send + Sync + fmt::Debug {
    fn reserve_observed_item(&self, thread_id: &str, item_id: &str) -> Result<(), ColdStoreError>;
    fn reserve_operation_output(
        &self,
        thread_id: &str,
        operation_id: &str,
        max_bytes: u64,
    ) -> Result<u64, ColdStoreError>;
    fn charge_operation_output(
        &self,
        thread_id: &str,
        operation_id: &str,
        accepted_bytes: u64,
    ) -> Result<(), ColdStoreError>;
    fn release_operation_output(&self, thread_id: &str, operation_id: &str);
    fn pressure(&self, thread_id: &str) -> StoragePressure;
    fn subscribe_pressure(&self, thread_id: &str) -> Option<tokio::sync::watch::Receiver<()>>;
    fn admit(&self, thread_id: &str, write: ThreadWrite) -> Result<(), ColdStoreError>;
    fn flush<'a>(
        &'a self,
        thread_id: &'a str,
        sequence: u64,
    ) -> BoxFuture<'a, Result<(), ColdStoreError>>;
    fn read_tool_task<'a>(
        &'a self,
        thread_id: &'a str,
        task_id: &'a str,
    ) -> BoxFuture<'a, Result<Option<DurableToolTask>, ColdStoreError>>;
}
impl<T: ColdStore> ErasedColdStore for T {
    fn reserve_observed_item(&self, thread_id: &str, item_id: &str) -> Result<(), ColdStoreError> {
        ColdStore::reserve_observed_item(self, thread_id, item_id)
    }
    fn reserve_operation_output(
        &self,
        thread_id: &str,
        operation_id: &str,
        max_bytes: u64,
    ) -> Result<u64, ColdStoreError> {
        ColdStore::reserve_operation_output(self, thread_id, operation_id, max_bytes)
    }
    fn charge_operation_output(
        &self,
        thread_id: &str,
        operation_id: &str,
        accepted_bytes: u64,
    ) -> Result<(), ColdStoreError> {
        ColdStore::charge_operation_output(self, thread_id, operation_id, accepted_bytes)
    }
    fn release_operation_output(&self, thread_id: &str, operation_id: &str) {
        ColdStore::release_operation_output(self, thread_id, operation_id)
    }
    fn pressure(&self, thread_id: &str) -> StoragePressure {
        ColdStore::pressure(self, thread_id)
    }
    fn subscribe_pressure(&self, thread_id: &str) -> Option<tokio::sync::watch::Receiver<()>> {
        ColdStore::subscribe_pressure(self, thread_id)
    }
    fn admit(&self, thread_id: &str, write: ThreadWrite) -> Result<(), ColdStoreError> {
        ColdStore::admit(self, thread_id, write)
    }
    fn flush<'a>(
        &'a self,
        thread_id: &'a str,
        sequence: u64,
    ) -> BoxFuture<'a, Result<(), ColdStoreError>> {
        Box::pin(ColdStore::flush(self, thread_id, sequence))
    }
    fn read_tool_task<'a>(
        &'a self,
        thread_id: &'a str,
        task_id: &'a str,
    ) -> BoxFuture<'a, Result<Option<DurableToolTask>, ColdStoreError>> {
        Box::pin(ColdStore::read_tool_task(self, thread_id, task_id))
    }
}

/// Shared store handle; Thread ownership and mutable model/tool instances remain private.
#[derive(Debug, Clone)]
pub struct ColdStoreHandle(Arc<dyn ErasedColdStore>);
impl ColdStoreHandle {
    /// Erases one backend; independent Threads may share its asynchronous writer.
    pub fn new(store: impl ColdStore) -> Self {
        Self(Arc::new(store))
    }

    pub(crate) fn reserve_observed_item(
        &self,
        thread_id: &str,
        item_id: &str,
    ) -> Result<(), ColdStoreError> {
        self.0.reserve_observed_item(thread_id, item_id)
    }

    /// Reserves one operation's live-output quota from this backend's reliable budget.
    pub(crate) fn reserve_operation_output(
        &self,
        thread_id: &str,
        operation_id: &str,
        max_bytes: u64,
    ) -> Result<u64, ColdStoreError> {
        self.0
            .reserve_operation_output(thread_id, operation_id, max_bytes)
    }

    /// Charges what one operation accepted against its reserved quota.
    pub(crate) fn charge_operation_output(
        &self,
        thread_id: &str,
        operation_id: &str,
        accepted_bytes: u64,
    ) -> Result<(), ColdStoreError> {
        self.0
            .charge_operation_output(thread_id, operation_id, accepted_bytes)
    }

    /// Releases one operation's remaining quota once its call ended.
    pub(crate) fn release_operation_output(&self, thread_id: &str, operation_id: &str) {
        self.0.release_operation_output(thread_id, operation_id)
    }

    pub(crate) fn subscribe_pressure(
        &self,
        thread_id: &str,
    ) -> Option<tokio::sync::watch::Receiver<()>> {
        self.0.subscribe_pressure(thread_id)
    }

    pub(crate) fn pressure(&self, thread_id: &str) -> StoragePressure {
        self.0.pressure(thread_id)
    }

    /// Reads one finished tool task from the durable store, if the backend keeps task facts.
    pub(crate) async fn read_tool_task(
        &self,
        thread_id: &str,
        task_id: &str,
    ) -> Result<Option<DurableToolTask>, ColdStoreError> {
        self.0.read_tool_task(thread_id, task_id).await
    }
}

/// Persistence progress is observable separately from authoritative runtime facts.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PersistenceState {
    pub attached: bool,
    pub pressure_paused: bool,
    pub pressure_warning: bool,
    pub pending_thread_bytes: u64,
    pub pending_store_bytes: u64,
    pub admitted_sequence: u64,
    pub durable_sequence: u64,
    pub error: Option<String>,
    /// Typed storage fault the backend reported, mirrored without parsing the error text.
    ///
    /// It is a live diagnostic of the current storage state, not durable Thread state, so it is
    /// never persisted and its absence is "no fault reported", not "healthy".
    #[serde(skip)]
    pub fault: Option<StorageFaultKind>,
    /// Generation of the reported fault, used by manual recovery to name what it retries.
    #[serde(skip)]
    pub fault_generation: u64,
    /// Whether new model/tool admission is held at a storage safety point until an explicit resume.
    ///
    /// A transient pressure pause clears on its own; a failed save does not, so a recovered Thread can
    /// never quietly start executing again without the recovery being confirmed. Live diagnostic, not
    /// durable Thread state, so it is never persisted.
    #[serde(skip)]
    pub resume_required: bool,
    /// Whether an explicit resume of the current fault would be accepted right now.
    ///
    /// Live diagnostic mirroring exactly what [`Owner::resume_storage`] verifies: the latch is still
    /// owed, the backend reports no hard failure (and, when the backend named the generation, has
    /// verified that **same** generation's recovery), every published fact was handed over, no byte
    /// threshold holds admission back, and the durability fence fixed at the fault really landed.
    /// The one owner computes it, so the entry offered to a caller always agrees with the command
    /// behind it; the resume still re-reads all of it, so this is a hint and never a substitute for
    /// the guard. Never persisted.
    #[serde(skip)]
    pub resume_ready: bool,
    /// Producer output that still has to be stored before this Thread can continue, if any.
    ///
    /// A pure reliable-budget truncation leaves this empty: the accepted bytes are retained and the
    /// reliable writer saves them with the fence. A failed capture write or archive leaves a typed,
    /// retriable obligation here, so the pause stays until *every* obligation of the generation is
    /// retried and its bytes really stored — a healthy history write is not proof the archive landed.
    /// A collection, keyed by stable identity, so two parallel operations that both failed keep
    /// their own obligation instead of one overwriting the other. Live diagnostic, never persisted.
    #[serde(skip)]
    pub output_obligations: Vec<OutputObligationState>,
    #[serde(default)]
    pub execution_phase: StorageExecutionPhase,
}

/// A storage pause is not a failed or cancelled Turn.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum StorageExecutionPhase {
    #[default]
    Running,
    PausingForStorage,
    PausedForStorage,
}

impl Owner {
    /// Wait at an execution safety point without failing the active Turn or starving control.
    pub(super) async fn await_storage_admission(
        &mut self,
        cancellation: &tokio_util::sync::CancellationToken,
    ) -> Result<(), ThreadError> {
        let mut updates = self
            .cold
            .as_ref()
            .and_then(|cold| cold.subscribe_pressure(&self.id));
        loop {
            self.admit_cold();
            self.refresh_storage_pressure();
            if self.cold_error.is_some() {
                // A failed save is not pressure: new model/tool work stops here until an explicit
                // resume. Only the caller that recovered (and verified generation and watermarks)
                // may release it, so recovery never silently continues execution on its own.
                self.state.persistence.resume_required = true;
            }
            // The gate is the byte threshold, not `pressure_paused`: the latter also carries the
            // transient "an in-flight operation could not reserve its output" backpressure, and that
            // is resolved by the reservation attempt itself. Gating the safety point on it would
            // wait for a fact only the code after this gate can clear.
            let healthy = !self.threshold_paused && self.cold_error.is_none();
            if healthy && !self.state.persistence.resume_required {
                if self.state.persistence.execution_phase != StorageExecutionPhase::Running {
                    self.state.persistence.execution_phase = StorageExecutionPhase::Running;
                    self.publish_snapshot();
                }
                return Ok(());
            }
            if self.state.persistence.execution_phase != StorageExecutionPhase::PausedForStorage {
                self.state.persistence.execution_phase = StorageExecutionPhase::PausingForStorage;
                self.publish_snapshot();
                self.state.persistence.execution_phase = StorageExecutionPhase::PausedForStorage;
                self.publish_snapshot();
            }
            let cancelled = self
                .await_with_mailbox(async {
                    tokio::select! {
                        () = cancellation.cancelled() => true,
                        available = async {
                            match updates.as_mut() {
                                Some(update) => update.changed().await.is_ok(),
                                None => std::future::pending::<bool>().await,
                            }
                        } => { if !available { updates = None; } false },
                        () = tokio::time::sleep(std::time::Duration::from_secs(1)) => false,
                    }
                })
                .await;
            if cancelled || self.interrupt.is_closing() {
                return Err(ThreadError::Cancelled);
            }
        }
    }

    /// Continues new model/tool admission after a hard storage fault was recovered.
    ///
    /// The caller names the fault generation it recovered. The owner re-reads the backend's typed
    /// report and only releases the latch when that generation is the current one, the store no
    /// longer reports a fault, the backend that named the generation has verified that generation's
    /// own recovery, every published effect really was admitted and the fence fixed at the fault is
    /// genuinely durable. A stale generation, a still-faulted store, an unverified recovery, an
    /// unhanded-over batch or an unfinished durability fence is refused with its own typed reason,
    /// so releasing the pause is a fact about the recovery — never a button that only looks like
    /// one, and never an unbounded wait for a write that has not happened yet.
    pub(super) fn resume_storage(&mut self, generation: u64) -> Result<(), ThreadError> {
        if !self.state.persistence.resume_required {
            // Pressure-only pauses resume by themselves; an explicit resume is a no-op then.
            return Ok(());
        }
        if generation != self.state.persistence.fault_generation {
            return Err(ThreadError::StaleStorageRecovery {
                requested: generation,
                recorded: self.state.persistence.fault_generation,
            });
        }
        self.admit_cold();
        self.refresh_storage_pressure();
        if let Some(error) = &self.cold_error {
            return Err(ThreadError::Storage(error.clone()));
        }
        let handed_over = self.pending_effects.front().is_none_or(|pending| {
            pending.write.effect.sequence <= self.state.persistence.admitted_sequence
        });
        // The byte threshold is what makes a release premature; an in-flight operation that could
        // not reserve its output is transient backpressure the next reservation clears, so it does
        // not by itself block an explicit continue.
        if self.threshold_paused || !handed_over {
            return Err(ThreadError::StoragePressure);
        }
        // Durability is the whole point of the explicit resume: a batch the backend only accepted,
        // or an error it simply stopped reporting, is not a recovered save. An unfinished fence is
        // refused rather than awaited so the caller can retry the write and ask again.
        if !self.backend_verified_recovery() {
            // The generation the backend named is not recovered until the backend verifies that same
            // generation: a retry that landed for an older one, or a queue that merely stopped
            // reporting an error, is not this generation's recovery.
            return Err(ThreadError::StorageRecoveryUnverified {
                generation: self.state.persistence.fault_generation,
            });
        }
        // Accepted output that could not be stored is a real obligation, not pressure: the pause
        // stays until *every* outstanding obligation has been retried and its bytes really stored. A
        // healthy history write is not proof, so this is verified independently of the durability
        // fence, and independently of any single operation's progress.
        if !self.output_obligations.is_empty() {
            return Err(ThreadError::StorageRecoveryUnverified {
                generation: self.state.persistence.fault_generation,
            });
        }
        // A repair whose committed result is not resident yet is an owed fact of the same kind: the
        // reference the retry stored is not on the result the product projects, so the pause stays
        // until the commit that carries the result applies it.
        if !self.pending_output_repairs.is_empty() {
            return Err(ThreadError::StorageRecoveryUnverified {
                generation: self.state.persistence.fault_generation,
            });
        }
        // Fail closed: no target was captured for this generation, so this owner cannot prove
        // the recovery. The newest published facts are the conservative target.
        let required = self.recovery_target();
        if self.state.persistence.durable_sequence < required {
            return Err(ThreadError::StorageRecoveryPending {
                durable: self.state.persistence.durable_sequence,
                required,
            });
        }
        self.fault_fence = None;
        self.store_fault_generation = None;
        self.verified_recovery_generation = None;
        self.local_fault = None;
        self.output_obligations.clear();
        self.satisfied_output_obligations.clear();
        self.pending_output_repairs.clear();
        self.state.persistence.output_obligations.clear();
        self.state.persistence.resume_required = false;
        // Nothing is owed any more, so the readiness flag that asked for this resume goes with it.
        self.state.persistence.resume_ready = false;
        self.state.persistence.execution_phase = StorageExecutionPhase::Running;
        self.publish_snapshot();
        Ok(())
    }

    /// Re-runs the accepted output the current fault owes and clears the obligation on success.
    ///
    /// This is the explicit retry behind the pause: it re-saves the exact bytes the producer kept —
    /// a capture fragment on disk, an archive source — under their stable identity, so it is not a
    /// tool rerun and a repeated call is idempotent. A successful retry only clears the obligation;
    /// the generation, the backend verdict and the durability fence are still verified by
    /// [`Self::resume_storage`], so the pause is never released by the retry alone. Without an owed
    /// obligation the call is a no-op, so an idempotent caller can always retry safely.
    ///
    /// # Errors
    /// Returns the retry's own typed storage failure (the obligation stays owed), or
    /// [`ThreadError::StorageRecoveryUnverified`] when the attached backend cannot own the
    /// obligation at all, so the caller learns the retry could not have stored those bytes.
    pub(super) async fn retry_output_obligation(&mut self) -> Result<(), ThreadError> {
        if self.output_obligations.is_empty() {
            return Ok(());
        }
        // Every outstanding obligation is retried, not only one: two parallel operations that both
        // failed must both be re-stored before the Thread can continue. A snapshot of the identities
        // is taken first so the retries never borrow the map they mutate, and a stable identity is
        // what makes a repeated call idempotent.
        let owed: Vec<(String, Arc<dyn OutputRetryObligation>)> = self
            .output_obligations
            .iter()
            .map(|(identity, (_, obligation))| (identity.clone(), obligation.clone()))
            .collect();
        let generation = self.state.persistence.fault_generation;
        let mut unsupported = false;
        let mut repairs: Vec<OutputRepair> = Vec::new();
        for (identity, obligation) in owed {
            match obligation.retry().await {
                Ok(OutputRetryOutcome::Stored) => {
                    self.output_obligations.remove(&identity);
                    self.satisfied_output_obligations.insert(identity);
                }
                // The bytes are durable, but the committed tool result still lacks the reference the
                // producer could not project on the first attempt. Collect it and attach it to the
                // same result after every obligation of this generation has been retried.
                Ok(OutputRetryOutcome::StoredWithRepair(repair)) => {
                    self.output_obligations.remove(&identity);
                    self.satisfied_output_obligations.insert(identity);
                    repairs.push(repair);
                }
                // The backend that re-saves this obligation does not own it, so its bytes could not
                // have been stored; keep it owed and tell the caller rather than releasing the pause.
                Ok(OutputRetryOutcome::Unsupported) => unsupported = true,
                Err(error) => {
                    let error = Arc::new(error);
                    self.state.persistence.error = Some(error.to_string());
                    self.cold_error = Some(error.clone());
                    self.state.persistence.resume_required = true;
                    self.state.persistence.resume_ready = false;
                    self.sync_output_obligation_state();
                    self.publish_snapshot();
                    return Err(ThreadError::Storage(error));
                }
            }
        }
        // Attach every stored reference to the already-committed result it belongs to before the
        // readiness predicate is recomputed, so a reader that observes the released pause always sees
        // the repaired output too.
        if !repairs.is_empty() {
            self.apply_output_repairs(repairs);
        }
        self.sync_output_obligation_state();
        if unsupported {
            return Err(ThreadError::StorageRecoveryUnverified { generation });
        }
        // The obligations no longer block, but the fence and the backend verdict still have to hold;
        // re-read them so readiness is published from the same predicate that releases the pause
        // instead of from the retry alone.
        self.admit_cold();
        self.refresh_storage_pressure();
        self.publish_snapshot();
        Ok(())
    }

    /// Reserves the reliable output quota of one operation at a storage safety point.
    ///
    /// A model/tool call is never started with a streamed result this process could not keep: when
    /// the reliable budget cannot fund the quota, admission waits here — the mailbox stays reachable
    /// through the wait, so an interrupt or close control still arrives — and resumes by itself once
    /// the budget frees up. Nothing was lost yet, so this is backpressure, not a hard fault.
    ///
    /// # Errors
    /// Returns cancellation when the wait is interrupted or the owner is closing.
    pub(super) async fn reserve_operation_output(
        &mut self,
        operation_id: &str,
        cancellation: &tokio_util::sync::CancellationToken,
    ) -> Result<Arc<crate::model::OutputBudget>, ThreadError> {
        loop {
            match crate::model::OutputBudget::reserve(
                self.cold.clone(),
                &self.id,
                operation_id,
                MAX_OPERATION_OUTPUT_BYTES,
                Some(crate::model::OutputRefusalNotice::new(
                    self.output_refusals.clone(),
                )),
            ) {
                Ok(budget) => {
                    let budget = Arc::new(budget);
                    self.operation_budgets
                        .insert(operation_id.to_owned(), budget.clone());
                    if self.output_backpressure {
                        self.output_backpressure = false;
                        self.refresh_storage_pressure();
                        self.publish_snapshot();
                    }
                    return Ok(budget);
                }
                Err(_error) => {
                    if !self.output_backpressure {
                        self.output_backpressure = true;
                        self.state.persistence.pressure_warning = true;
                        self.refresh_storage_pressure();
                        self.publish_snapshot();
                    }
                    let cancelled = self
                        .await_with_mailbox(async {
                            tokio::select! {
                                () = cancellation.cancelled() => true,
                                () = tokio::time::sleep(std::time::Duration::from_secs(1)) => false,
                            }
                        })
                        .await;
                    if cancelled || self.interrupt.is_closing() {
                        // The wait is over: this owner owes no further reservation, so the transient
                        // admission backpressure it published goes with the cancelled attempt.
                        if self.output_backpressure {
                            self.output_backpressure = false;
                            self.refresh_storage_pressure();
                            self.publish_snapshot();
                        }
                        return Err(ThreadError::Cancelled);
                    }
                }
            }
        }
    }

    /// Ends one operation's live quota after its call returned and its fact left this owner.
    ///
    /// A refusal is a real truncation: the accepted bytes stay with the operation's partial result,
    /// and the typed fault is latched so no new model/tool work starts until an explicit resume
    /// names this generation. What comes back is whatever the operation's facts did not take over:
    /// a call that produced nothing gives its whole ceiling back, and a call whose result was already
    /// handed over gives back the unused remainder, because the handed-over fact carries those bytes
    /// now. Neither `granted - accepted` (which would leave the accepted bytes charged forever next to
    /// the batch that also charges them) nor the whole original grant (which would drop the fact's
    /// own charge) is the right amount.
    pub(super) fn finish_operation_output(&mut self, operation_id: &str) {
        if let Some(budget) = self.operation_budgets.remove(operation_id) {
            if let Some((accepted, limit)) = budget.refusal() {
                self.latch_output_budget_fault(operation_id, accepted, limit);
            }
            budget.release();
        }
    }

    /// Ends one operation's live quota only once its produced fact was handed to the reliable queue.
    ///
    /// The reservation is a transient ceiling charged to the same budget the ordinary admission
    /// charges; the fact that takes those bytes over is what makes the charge permanent until
    /// durability. Handing the remaining ceiling back *before* the produced fact is enrolled would
    /// make the Thread look drained while the fact is still resident — an uncommitted tool
    /// completion, or a published batch the store has not taken over yet — and would let a new
    /// operation take budget those retained bytes need. So the refusal is latched now (the truncation
    /// is a fact the moment it happens) but the remainder is only released when the hand-over really
    /// completed; otherwise the release is deferred to [`Owner::admit_cold`], which is the boundary
    /// where it does.
    pub(super) fn settle_operation_output(&mut self, operation_id: &str) {
        let refusal = self
            .operation_budgets
            .get(operation_id)
            .and_then(|budget| budget.refusal());
        if let Some((accepted, limit)) = refusal {
            self.latch_output_budget_fault(operation_id, accepted, limit);
        }
        if self.output_handover_complete() {
            self.finish_operation_output(operation_id);
        } else {
            self.deferred_output_releases
                .insert(operation_id.to_owned());
        }
    }

    /// Whether every fact this owner produced is already handed to the reliable queue.
    ///
    /// Without an attached store there is no reliable queue to hand anything to, so nothing is
    /// retained behind a reservation there. With one, the hand-over is complete exactly when the
    /// queue is empty and no synchronous rejection is still standing.
    fn output_handover_complete(&self) -> bool {
        self.cold.is_none() || (self.cold_error.is_none() && self.pending_effects.is_empty())
    }

    /// Latches the typed fault for an operation whose accepted output exceeded the reliable budget.
    ///
    /// Visible to the sibling modules that own the two producers of a refusal — the mailbox wait that
    /// observes the model's notice and the tool-progress handler that refuses an oversized preview —
    /// and to nothing outside this Thread's own machinery.
    pub(super) fn latch_output_budget_fault(
        &mut self,
        operation_id: &str,
        accepted: u64,
        limit: u64,
    ) {
        let error = ColdStoreError {
            source: Box::new(std::io::Error::other(format!(
                "operation {operation_id} exceeded the reliable output budget: accepted {accepted} bytes of {limit}"
            ))),
        };
        self.latch_local_storage_fault(StorageFaultKind::QueueFull, Arc::new(error));
    }

    /// Latches a typed storage fault a producer reported through its own boundary.
    ///
    /// The producer names the exact category — a capture write that failed, an archive that could not
    /// be made durable — so the owner pauses admission on the same fault generation/recovery gate a
    /// backend report uses without reading the diagnostic text back out of the error. When the
    /// producer still owes a re-save, its [`OutputRetryObligation`] is kept under this generation, so
    /// the pause is only released once that same obligation is retried and its bytes really stored —
    /// a healthy history write never stands in for the failed archive.
    pub(super) fn latch_storage_fault(
        &mut self,
        kind: StorageFaultKind,
        source: Arc<ColdStoreError>,
        obligation: Option<Arc<dyn OutputRetryObligation>>,
    ) {
        self.latch_local_storage_fault(kind, source);
        if let Some(obligation) = obligation {
            // Keyed by stable identity, so a producer that reports the same failure again attaches
            // nothing new instead of pushing a duplicate, and two parallel operations keep their own
            // obligation instead of one overwriting the other. A refresh of the backend store fault
            // never removes an entry: the map is only emptied when each identity is retried to
            // `Stored` or the explicit resume releases the whole generation.
            let identity = obligation.identity();
            // A return path that reports the same failure *after* the retry already stored its bytes
            // must not re-arm an obligation the Thread discharged: the reference would be re-owed
            // forever even though it is already durable.
            if !self.satisfied_output_obligations.contains(&identity) {
                self.output_obligations
                    .entry(identity)
                    .or_insert((self.state.persistence.fault_generation, obligation));
                self.sync_output_obligation_state();
                // The obligation is a new fact that keeps the readiness predicate closed, so republish
                // it the moment it is known instead of only at the next safety point.
                self.publish_snapshot();
            }
        }
    }

    /// Mirrors the live outstanding obligations into the serializable diagnostic, in stable order.
    ///
    /// The owner is the only writer, so the published collection always agrees with the handles it
    /// will retry. It is a live view for the retry surface, never durable Thread state.
    fn sync_output_obligation_state(&mut self) {
        let states: Vec<OutputObligationState> = self
            .output_obligations
            .values()
            .map(|(_, obligation)| OutputObligationState {
                kind: obligation.kind(),
                received_bytes: obligation.received_bytes(),
                location: obligation.location(),
            })
            .collect();
        self.state.persistence.output_obligations = states;
    }

    /// Queues each retried obligation's durable reference for the committed result it belongs to.
    ///
    /// The repair is keyed by the same call identity the result was committed under, so the reference
    /// supplements that one result instead of creating a second fact, and the repaired output keeps a
    /// single identity. A producer reports a failed archive the moment it happens, so the user can
    /// retry before the operation has finished: the reference is then kept pending instead of being
    /// dropped, and the commit that carries the result applies it. A repeated repair of the same call
    /// and reference never duplicates, so a retried obligation is idempotent.
    fn apply_output_repairs(&mut self, repairs: Vec<OutputRepair>) {
        for repair in repairs {
            let pending = self
                .pending_output_repairs
                .entry(repair.call_id)
                .or_default();
            if !pending.contains(&repair.reference) {
                pending.push(repair.reference);
            }
        }
        if self.flush_pending_output_repairs() {
            self.publish();
        }
    }

    /// Applies every pending repair whose committed result is now resident.
    ///
    /// Returns whether the owner's state changed, so the caller publishes the supplement. A
    /// reference whose result is still not committed stays pending — releasing the storage pause
    /// requires this map to be empty, so a repair can never be reported as done before the result it
    /// supplements really carries it. The map is bounded by the operations still finishing and is
    /// emptied by the commits that apply it, so it never grows with history.
    pub(super) fn flush_pending_output_repairs(&mut self) -> bool {
        if self.pending_output_repairs.is_empty() {
            return false;
        }
        let pending = std::mem::take(&mut self.pending_output_repairs);
        let mut changed = false;
        for (call_id, references) in pending {
            let Some(index) = self.result_record_index(&call_id) else {
                self.pending_output_repairs.insert(call_id, references);
                continue;
            };
            for reference in references {
                changed |= self.record_output_repair(index, &call_id, reference);
            }
        }
        changed
    }

    /// Position of the committed tool result record `call_id` was delivered under, if resident.
    fn result_record_index(&self, call_id: &str) -> Option<usize> {
        self.state.context.records.iter().position(|record| {
            matches!(
                &record.source,
                ContextSource::ToolResult { call_id: recorded, .. } if recorded.as_str() == call_id
            )
        })
    }

    /// Records one durable resource reference for the committed result at `index`.
    ///
    /// The reference supplements the context record the model reads *and* the durable result fact the
    /// product projection and cold recovery project, leaving every other fact of the result — its
    /// text, position, outcome and identity — untouched. An identical reference already recorded is a
    /// no-op, so a producer that retries more than once never duplicates content.
    fn record_output_repair(
        &mut self,
        index: usize,
        call_id: &str,
        reference: crate::context::ResourceReference,
    ) -> bool {
        let resource = ContextContent::Resource {
            reference: reference.clone(),
        };
        let mut records: Vec<ContextRecord> = self.state.context.records.to_vec();
        let mut changed = false;
        if !records[index].content.contains(&resource) {
            records[index].content.push(resource);
            let revision = self.state.context.revision.saturating_add(1);
            self.state.context = ContextSnapshot {
                revision,
                records: records.into(),
            };
            changed = true;
        }
        let repair = OutputRepair {
            call_id: call_id.to_owned(),
            reference,
        };
        let mut repairs = self.state.delivery_repairs.to_vec();
        if !repairs.iter().any(|existing| {
            existing.call_id == repair.call_id && existing.reference == repair.reference
        }) {
            repairs.push(repair);
            self.state.delivery_repairs = repairs.into();
            changed = true;
        }
        changed
    }

    /// Records a fault this owner latched from its own work rather than from a backend report.
    ///
    /// Such a fault owes an explicit resume and has no backend retry outstanding, so its recovery is
    /// proven by the durability fence instead of a backend verdict.
    fn latch_local_storage_fault(&mut self, kind: StorageFaultKind, error: Arc<ColdStoreError>) {
        self.state.persistence.error = Some(error.to_string());
        self.cold_error = Some(error);
        if self.state.persistence.fault != Some(kind) {
            self.state.persistence.fault_generation =
                self.state.persistence.fault_generation.saturating_add(1);
        }
        self.state.persistence.fault = Some(kind);
        self.local_fault = Some(kind);
        // The truncation owes an explicit resume from the moment it happens, not only from the next
        // time the owner visits a storage safety point: a healthy backend report in between must not
        // let the Thread quietly start more work.
        self.state.persistence.resume_required = true;
        // The fault was just latched, so no recovery can be proven yet.
        self.state.persistence.resume_ready = false;
        self.publish_snapshot();
    }

    pub(super) fn refresh_storage_pressure(&mut self) {
        let pressure = match &self.cold {
            Some(store) => store.0.pressure(&self.id),
            // Without an attached store every published batch stays unadmitted below, so the same
            // byte budget still gates admission. The accepted facts are never dropped to make room
            // for new work; the Thread reports pressure and pauses further admission instead of
            // letting a transient window grow without bound.
            None => StoragePressure::default(),
        };
        self.apply_durable_receipt(&pressure);
        // A synchronous `admit` rejection is a fact about *these* effects: they were published but
        // the store did not take them, so a healthy pressure report from a background writer must
        // not erase it. Otherwise the Thread would keep executing model work whose facts are not
        // handed over — and a rejected attempt could be silently corrected instead of failing closed.
        // Only when every published effect really was handed over does a healthy report release a
        // latch that only reflected a transient background conflict.
        let handed_over = match self.pending_effects.front() {
            Some(pending) => {
                pending.write.effect.sequence <= self.state.persistence.admitted_sequence
            }
            None => true,
        };
        match &pressure.error {
            Some(error) => {
                self.state.persistence.error = Some(error.to_string());
                self.cold_error = Some(error.clone());
            }
            None if handed_over => {
                // A fault this owner latched itself (an operation whose accepted output exceeded the
                // reliable budget) keeps its diagnostic text until an explicit resume releases it: a
                // healthy backend report must not erase the reason the Thread stopped. Only the hard
                // `cold_error` latch is cleared here, because it is what the resume path inspects.
                if self.local_fault.is_none() {
                    self.state.persistence.error = None;
                }
                self.cold_error = None;
            }
            None => {}
        }
        // The typed fault rides the same report as the text and obeys the same hand-over rule: a
        // synchronous rejection is a fact about *these* effects, so a healthy report from a
        // background writer must not clear it while a published batch is still not admitted. The
        // category is copied as a value; nothing here reads the error text to decide what it is.
        match pressure.fault {
            Some(fault) => {
                self.state.persistence.fault = Some(fault);
                self.state.persistence.fault_generation = pressure.fault_generation;
                // This generation's recovery proof belongs to the backend that named it: only its
                // own verdict for *this* generation may release the latch, so an older verified
                // generation can never stand in for a newer fault.
                self.store_fault_generation = Some(pressure.fault_generation);
            }
            None if handed_over => {
                // A fault this owner latched itself (an operation whose accepted output exceeded the
                // reliable budget) is a fact about this Thread's own work: the backend reporting no
                // fault of its own must not clear it before an explicit resume releases it.
                match self.local_fault {
                    Some(fault) => self.state.persistence.fault = Some(fault),
                    None => {
                        self.state.persistence.fault = None;
                        self.state.persistence.fault_generation = pressure.fault_generation;
                    }
                }
            }
            None => {}
        }
        // The backend's own recovery receipt, folded as the typed value it is: the comparison that
        // matters happens in the one readiness predicate below, never in a subscriber.
        self.verified_recovery_generation = pressure.recovered_generation;
        self.latch_recovery_fence();
        // A queue drained by an attached store costs nothing here; only a queue that really holds
        // undurable batches is measured, and each of those batches is serialized at most once.
        let unadmitted = if self.pending_effects.is_empty() {
            0
        } else {
            self.pending_effects
                .iter_mut()
                .fold(0_u64, |total, pending| {
                    let bytes = match pending.encoded_bytes {
                        Some(bytes) => bytes,
                        None => {
                            // An unencodable batch keeps the pessimistic `u64::MAX` weight, exactly
                            // like the previous fold-based accounting did.
                            let bytes = pending
                                .write
                                .effect
                                .encode()
                                .map_or(u64::MAX, |payload| payload.content().len() as u64);
                            pending.encoded_bytes = Some(bytes);
                            bytes
                        }
                    };
                    total.saturating_add(bytes)
                })
        };
        let thread = pressure.thread_bytes.saturating_add(unadmitted);
        let total = pressure.store_bytes.saturating_add(unadmitted);
        let state = &mut self.state.persistence;
        state.pending_thread_bytes = thread;
        state.pending_store_bytes = total;
        let threshold = if thread >= 64 * 1024 * 1024 || total >= 256 * 1024 * 1024 {
            true
        } else if thread <= 32 * 1024 * 1024 && total <= 128 * 1024 * 1024 {
            false
        } else {
            self.threshold_paused
        };
        self.threshold_paused = threshold;
        // Backpressure over an operation the reliable budget cannot fund yet pauses new admission
        // without pretending the Thread's resident bytes crossed a threshold of their own.
        state.pressure_paused = threshold || self.output_backpressure;
        state.pressure_warning = thread >= 32 * 1024 * 1024 || total >= 128 * 1024 * 1024;
        self.refresh_resume_ready(handed_over);
    }

    /// Republishes whether an explicit continue of the current fault is provably safe right now.
    ///
    /// It is the same predicate [`Self::resume_storage`] applies, so a caller that only sees the
    /// authoritative snapshot learns the moment the continue really would be accepted instead of
    /// having to guess from "no error" or "queue empty". A change is published immediately: a
    /// recovered save can become resumable while the Thread is parked at the safety point, with no
    /// other fact moving.
    fn refresh_resume_ready(&mut self, handed_over: bool) {
        let ready = self.storage_recovery_ready(handed_over);
        if ready != self.state.persistence.resume_ready {
            self.state.persistence.resume_ready = ready;
            self.publish_snapshot();
        }
    }

    /// Whether an explicit continue would be accepted right now, from this owner's own typed facts.
    ///
    /// This is the one readiness predicate: [`Self::resume_storage`] applies it before releasing the
    /// latch and [`Self::refresh_resume_ready`] publishes exactly the same answer, so the entry a
    /// caller is offered can never disagree with the command behind it. It requires the latch itself,
    /// no hard error from the backend, every published fact handed over, no byte threshold holding
    /// admission back, the durability fence fixed for *this* generation to be durable, and — when the
    /// backend named the generation — that backend's own verdict for that same generation.
    fn storage_recovery_ready(&self, handed_over: bool) -> bool {
        self.state.persistence.resume_required
            && self.backend_verified_recovery()
            && self.cold_error.is_none()
            && handed_over
            && self.output_obligations.is_empty()
            && self.pending_output_repairs.is_empty()
            && !self.threshold_paused
            && self.state.persistence.durable_sequence >= self.recovery_target()
    }

    /// Whether the backend that named the current fault generation has verified its recovery.
    ///
    /// A generation the backend reported needs the backend's own verdict for that generation. A
    /// generation this owner latched by itself — its own reliable-output truncation, or a backend
    /// error that names no generation — has no backend retry outstanding, so the fixed durability
    /// fence is its proof and this is trivially true.
    fn backend_verified_recovery(&self) -> bool {
        let generation = self.state.persistence.fault_generation;
        match self.store_fault_generation {
            Some(reported) if reported == generation => {
                self.verified_recovery_generation == Some(generation)
            }
            _ => true,
        }
    }

    /// Durability watermark the current generation's recovery must reach.
    ///
    /// The fence captured when the fault was first observed is used while that generation is owed; a
    /// generation with no captured target falls back to the newest published facts, which is the
    /// conservative target rather than an unbounded wait.
    fn recovery_target(&self) -> u64 {
        match self.fault_fence {
            Some((latched, fence)) if latched == self.state.persistence.fault_generation => fence,
            _ => self.recovery_fence(),
        }
    }

    /// Folds one durable receipt into the saved watermark and releases the covered live batches.
    ///
    /// The store reports the newest effect it already holds durably, so replacing the explicit
    /// `flush` round trip with this receipt is what keeps a normally saving Thread from retaining
    /// every un-released batch: the window is bounded by the facts that are still unsaved instead of
    /// growing for the whole session. The release happens here, at a storage safety point the owner
    /// already visits while servicing its mailbox, so it neither blocks on the owner command queue
    /// nor serializes an effect under a lock.
    fn apply_durable_receipt(&mut self, pressure: &StoragePressure) {
        // The store can only be durable through effects this owner already handed over.
        let receipt = pressure
            .durable_sequence
            .min(self.state.persistence.admitted_sequence);
        if receipt > self.state.persistence.durable_sequence {
            self.state.persistence.durable_sequence = receipt;
            self.effect_window.release_through(receipt);
        }
    }

    /// Fixes the durability target a latched storage fault must be recovered to.
    ///
    /// A hard fault owes an explicit resume, and that resume has to verify a *durable* fact — not a
    /// batch the store merely accepted and not the mere absence of a reported error: a save that
    /// failed and then stopped reporting an error would otherwise look recovered while nothing was
    /// written. The target is captured once per fault generation from the facts this owner really
    /// published: every handed-over commit plus the oldest batch the store has not taken over. It
    /// never moves while that generation is owed, so the caller's "the fence is durable now" is
    /// verified against the same fence the caller was shown.
    ///
    /// A fault that heals before any resume is owed leaves nothing behind, so a later, unrelated
    /// fault gets its own target instead of inheriting a stale one.
    fn latch_recovery_fence(&mut self) {
        let generation = self.state.persistence.fault_generation;
        let faulted = self.cold_error.is_some() || self.state.persistence.fault.is_some();
        if faulted {
            match self.fault_fence {
                // The same generation is still owed its original target.
                Some((latched, _)) if latched == generation => {}
                // A new fault generation fixes a new target; an inherited fence would let a resume
                // released for the older, lower target stand in for the newer one.
                _ => {
                    let fence = self.recovery_fence();
                    self.fault_fence = Some((generation, fence));
                }
            }
            return;
        }
        if !self.state.persistence.resume_required {
            self.fault_fence = None;
        }
    }

    /// Durability watermark the currently reported storage facts must reach: every commit already
    /// handed over, plus the oldest batch the store has not taken over yet.
    ///
    /// An accepted-but-not-durable batch and a batch the store rejected both sit above this
    /// watermark, so "durable at least this" is exactly "the recovery really landed on disk".
    fn recovery_fence(&self) -> u64 {
        let pending = self
            .pending_effects
            .front()
            .map(|pending| pending.write.effect.sequence)
            .unwrap_or(0);
        self.state.persistence.admitted_sequence.max(pending)
    }

    pub(super) fn admit_cold(&mut self) {
        let Some(store) = self.cold.clone() else {
            return;
        };
        self.state.persistence.attached = true;
        while let Some(pending) = self.pending_effects.front() {
            let sequence = pending.write.effect.sequence;
            match store.0.admit(&self.id, pending.write.clone()) {
                Ok(()) => {
                    self.state.persistence.admitted_sequence = sequence;
                    self.pending_effects.pop_front();
                }
                Err(error) => {
                    self.state.persistence.error = Some(error.to_string());
                    self.cold_error = Some(Arc::new(error));
                    return;
                }
            }
        }
        self.state.persistence.error = None;
        self.cold_error = None;
        // The queue really drained, so every operation whose release was held back until its fact
        // was handed over can give its transient ceiling back now. This is the same boundary the
        // facts completed at, so the accepted bytes are re-charged by the admitted batch instead of
        // staying on the reservation or leaving a gap in between.
        if !self.deferred_output_releases.is_empty() {
            for operation_id in std::mem::take(&mut self.deferred_output_releases) {
                self.finish_operation_output(&operation_id);
            }
        }
        // Every commit is a memory-safe boundary: fold the receipt the store reports for the batch
        // it just took, so a running Turn releases what it already saved instead of holding the
        // whole session's effects until the next explicit `flush`.
        let pressure = store.0.pressure(&self.id);
        self.apply_durable_receipt(&pressure);
    }

    pub(super) async fn flush_cold(&mut self) -> Result<(), ThreadError> {
        self.admit_cold();
        if let Some(error) = &self.cold_error {
            self.publish_snapshot();
            return Err(ThreadError::Storage(error.clone()));
        }
        let Some(store) = &self.cold else {
            return Ok(());
        };
        let result = store
            .0
            .flush(&self.id, self.state.persistence.admitted_sequence)
            .await;
        match result {
            Ok(()) => {
                self.state.persistence.durable_sequence = self.state.persistence.admitted_sequence;
                // The durable handoff makes every covered commit recoverable from history, so the
                // live window may release those bodies once its byte budget needs the space.
                self.effect_window
                    .release_through(self.state.persistence.durable_sequence);
            }
            Err(error) => {
                // A caller that asked for a fixed durable target and did not get it has a terminal
                // fact: answer with it directly instead of letting the pressure mirror below
                // mistake the store's momentary report for recovery.
                let error = Arc::new(error);
                self.state.persistence.error = Some(error.to_string());
                self.cold_error = Some(error.clone());
                self.publish_snapshot();
                return Err(ThreadError::Storage(error));
            }
        }
        self.refresh_storage_pressure();
        self.publish_snapshot();
        match &self.cold_error {
            Some(error) => Err(ThreadError::Storage(error.clone())),
            None => Ok(()),
        }
    }
}

//! Per-Thread ordered persistence owner: effects are written in order, checkpoints are coalesced.
//!
//! Admission never blocks the executor. Effects are history facts, so every admitted batch is
//! written to `history.sqlite`/`calls.sqlite` in sequence order, each against the effect-matched
//! transfer state it arrived with; the writer never reads back a pruned `state.toml`. Checkpoints
//! by contrast are merged: at most one publication is in flight and at most one newest revision is
//! retained (`design/15` §15.4). A retained revision becomes dirty at the first admission and is
//! published at the end of a fixed coalescing interval — no change, no write; a missed tick writes
//! once instead of catching up — unless the admitting effect ends a Turn, changes lifecycle or the
//! caller is waiting on `flush_through`, which publishes it immediately.
//!
//! Publication order is fixed: the history watermark must already cover the checkpoint's
//! `history_fence`, every blob the checkpoint names must be durable first, and only then is the
//! counter-resolved TOML written atomically. The same file also carries the Thread's cumulative
//! usage summary, which this writer folds from each effect exactly once, so hot reads, reconnects
//! and cold restores observe one cumulative value instead of re-aggregating resident attempts.
//! Each checkpoint publishes the summary of its own `state_revision`, bound when the fold reached
//! that effect, so a candidate never carries a summary the state it saves does not include.

use std::{
    collections::{BTreeMap, VecDeque},
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, AtomicUsize, Ordering},
    },
    time::Duration,
};

use anyhow::{Context, Result, ensure};
use pl_core::thread::{
    ThreadCheckpoint, ThreadSnapshot, UsageSummary,
    cold::{ColdStore, ColdStoreError, StoragePressure, ThreadWrite},
};
use tokio::time::Instant;

use crate::studio::StudioStore;
use crate::studio::storage::coordinator::ThreadPersistenceMetrics;
use crate::studio::storage::history::{
    EffectCommit, InputIdentityWrite, MessageIdentityWrite, is_retryable_write,
};
use crate::studio::thread_projection::PreparedEffect;

/// Fixed coalescing interval for dirty checkpoint revisions.
const SNAPSHOT_COALESCE: Duration = Duration::from_secs(1);

/// How long one writer incarnation may keep retrying a transient storage conflict before it must
/// report it.
///
/// A short `SQLITE_BUSY`/`SQLITE_LOCKED`/`SQLITE_IOERR` is a self-healing conflict: the accepted
/// effects stay queued in order and the same `step` writes them again, so surfacing the error would
/// turn a conflict that resolves on its own into a terminal persistence failure that blocks the
/// next model admission. Only a conflict that outlives this window — or a structural, constraint
/// or corruption error, which is never retried — is reported and keeps failing the drain.
const RETRYABLE_BUSY_WINDOW: Duration = Duration::from_secs(30);
const MAX_HISTORY_BATCHES: usize = 4096;
const MAX_HISTORY_THREAD_BYTES: u64 = 64 * 1024 * 1024;
const MAX_HISTORY_PROCESS_BYTES: u64 = 256 * 1024 * 1024;
const NO_PROGRESS_WINDOW: Duration = Duration::from_secs(60);

/// Identity source for live per-Thread writer incarnations.
///
/// The coordinator keys its per-Thread watermarks by this value so a stopped writer can release
/// only its own state, and a replacement incarnation is never mistaken for the writer it replaced.
static NEXT_WRITER_INCARNATION: AtomicUsize = AtomicUsize::new(1);

/// Releases one writer incarnation's coordinator watermarks when its task ends, however it ends.
///
/// The writer loop owns the only strong reference to `Inner`, so it returns exactly when the sink
/// is gone; nothing it failed to report after that can ever be reported again. Leaving the queue
/// behind would make the process-wide drain barrier wait for a writer that can no longer run, so a
/// drained queue is removed and a queue that still owed unsaved facts becomes an explicit terminal
/// diagnostic instead of an un-drainable barrier.
struct DetachGuard {
    store: StudioStore,
    thread_id: String,
    owner: usize,
}

impl Drop for DetachGuard {
    fn drop(&mut self) {
        self.store
            .thread_persistence()
            .detach(&self.thread_id, self.owner);
    }
}

/// One retained checkpoint awaiting publication.
#[derive(Debug, Clone)]
struct PendingCheckpoint {
    /// Monotonic publication epoch of this sink.
    ///
    /// It orders retentions and lets a completed publication clear only the candidate that is still
    /// in flight, so a cover frozen while an older candidate was being written is never swallowed.
    epoch: u64,
    /// Earliest instant the coalescing interval allows this revision to be published.
    due: Instant,
    /// Whether the admitting effect demanded immediate publication.
    immediate: bool,
    checkpoint: ThreadCheckpoint,
    /// Cumulative usage folded exactly through `checkpoint.state_revision`.
    ///
    /// A published checkpoint must carry the summary of its own revision, never the writer's
    /// running fold, which is allowed to be ahead of a retained candidate. The slot stays `None`
    /// until the writer has folded that exact effect, so at most the in-flight candidate and the
    /// newest pending one ever hold a summary — there is no per-revision map that grows with
    /// history.
    usage: Option<UsageSummary>,
}

/// One admitted effect waiting for its ordered durable write.
#[derive(Debug)]
struct QueuedEffect {
    sequence: u64,
    write: Arc<ThreadWrite>,
    /// Encoded size of `write.effect`, measured once at admission.
    bytes: u64,
    /// The product content the Thread's single live projection owner prepared for this commit.
    ///
    /// `None` until the projection owner took the fact over. The writer never projects an effect
    /// itself, so a fact whose reliable handoff is still unfinished stays queued instead of being
    /// written out of the projection's order.
    prepared: Option<Arc<PreparedEffect>>,
    /// Retained bytes of `prepared`, charged to the reliable budget while this entry holds it.
    prepared_bytes: u64,
}

#[derive(Debug, Default)]
struct Progress {
    /// Highest effect sequence whose history/calls write completed.
    durable: u64,
    /// Highest checkpoint revision already published to `state.toml`.
    published_revision: u64,
    /// Publication epoch already published.
    published_epoch: u64,
    /// Next publication epoch this sink will hand out.
    next_epoch: u64,
    /// Newest admitted checkpoint awaiting selection.
    pending: Option<PendingCheckpoint>,
    /// The checkpoint selected for publication.
    ///
    /// Once a revision is selected it is retained here across later admissions, so a fixed flush
    /// target or an immediate publication is never pushed out of reach by the effects admitted
    /// behind it. At most one selection is in flight, so the sink still holds one in-flight and
    /// one newest pending checkpoint.
    in_flight: Option<PendingCheckpoint>,
    /// Checkpoint revision currently being serialized or synced.
    saving_revision: u64,
    /// Admitted effects awaiting their ordered history/calls write.
    ///
    /// An entry leaves the queue only once the durable store committed it *and* the Thread's single
    /// live projection owner took it over, so this queue is the one reliable handoff between a core
    /// commit and both durable history and the live product.
    effects: VecDeque<QueuedEffect>,
    /// Retained bytes of the prepared batches the queue still holds.
    ///
    /// A prepared batch is the immutable product content the projection owner published, and the
    /// queue holds it until the effect is durable: its text is real resident memory, so it is
    /// charged to the Thread's reliable budget instead of hiding outside it. It is released with the
    /// queue entry and never double-counted against that effect's own encoded size.
    prepared_bytes: u64,
    /// Bytes the Thread's live projection currently retains in its own tables.
    ///
    /// The projection owner publishes this absolute gauge after every projection step. It is what
    /// keeps the Thread's budget honest past durability: the retained bodies and the report
    /// accumulator outlive the prepared batch the writer already committed, so without this the
    /// Thread would claim to be drained while still holding its Turn's process bodies.
    projection_bytes: u64,
    /// Newest commit the Thread's live projection owner has handed over.
    ///
    /// This is the handoff ticket the projection's own durability barrier asks for: effects are
    /// admitted in order and the owner hands its batch over in the same order, so "the writer is
    /// durable through this commit" is exactly "every fact this owner handed over is saved". A
    /// channel that has handed nothing over is at `0`, so a projection installed on a restored
    /// Thread — whose commits were already durable before this process started — never waits for a
    /// watermark that cannot move.
    handoff: u64,
    /// Effect sequence a caller is waiting for; publication may not wait for the coalescing tick.
    flush_target: u64,
    /// Highest effect sequence whose call write was admitted.
    calls_admitted: u64,
    /// Highest effect sequence whose call write completed.
    calls_durable: u64,
    /// Encoded bytes of the effect currently being written.
    in_flight_bytes: u64,
    /// Whether the newest admitted state paused inference admission under storage pressure.
    pressure_paused: bool,
    /// Cumulative usage summary folded through the effects written so far.
    ///
    /// This is the running value hot reads see. A checkpoint publishes its own bound copy instead
    /// of this one, because this fold may already be ahead of a retained candidate.
    usage: UsageSummary,
    error: Option<String>,
    fault: Option<pl_protocol::studio::HistoryFault>,
    fault_generation: u64,
    /// Whether the newest fault generation's retry has been verified as really landed.
    ///
    /// It stays `false` from the moment a fault is latched until a retry made the fixed target
    /// durable and published — the only fact that makes an explicit continue acceptable. A later
    /// fault clears it again, so a stale verification never vouches for a newer failure.
    fault_recovered: bool,
    fault_target: u64,
    retry_requested: bool,
    last_progress_at: Option<Instant>,
    /// Reliable output ceilings currently held by in-flight model/tool operations of this Thread.
    ///
    /// One entry per operation that reserved a live-output ceiling from the process budget. The
    /// ceiling is charged to the budget the moment it is granted (a call only starts when its worst
    /// case could be retained). The value is what is still *reserved* for output that has not become
    /// a fact yet: admitting a fact transfers the bytes it covers out of these ceilings instead of
    /// charging the same output a second time next to them, so the process total keeps exactly one
    /// charge for it — as the in-flight ceiling before the fact exists, as the fact's own charge
    /// afterwards. What is left when the call ends is given back in full.
    operation_output: BTreeMap<String, u64>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct HistoryStatus {
    pub(crate) fault_generation: u64,
    pub(crate) fault: Option<pl_protocol::studio::HistoryFault>,
    pub(crate) error: Option<String>,
    pub(crate) admitted_sequence: u64,
    pub(crate) committed_sequence: u64,
    pub(crate) queued_records: u64,
    pub(crate) queued_bytes: u64,
}

impl Progress {
    /// Retains one checkpoint as the newest publication without moving an anchored deadline.
    fn mark_checkpoint(&mut self, checkpoint: ThreadCheckpoint, immediate: bool) {
        let epoch = self.next_epoch.saturating_add(1);
        self.next_epoch = epoch;
        // 合并窗口锚定在第一次 dirty 的时刻：连续提交只合并，不不断推迟或补写。
        let due = match self.pending.as_ref() {
            Some(pending) if !immediate => pending.due,
            _ if immediate => Instant::now(),
            _ => Instant::now() + SNAPSHOT_COALESCE,
        };
        let immediate = immediate
            || self
                .pending
                .as_ref()
                .is_some_and(|pending| pending.immediate);
        self.pending = Some(PendingCheckpoint {
            epoch,
            due,
            immediate,
            checkpoint,
            usage: None,
        });
        self.bind_usage();
    }

    /// Selects the newest pending checkpoint once it is demanded.
    ///
    /// A revision is demanded when it is immediate, when a caller is waiting on a fixed flush
    /// target that it covers, or when its coalescing window expired. Selection only moves the
    /// newest pending into the in-flight slot; later admissions keep filling `pending`.
    fn select_in_flight(&mut self) {
        if self.in_flight.is_some() {
            return;
        }
        let Some(pending) = self.pending.as_ref() else {
            return;
        };
        if pending.epoch <= self.published_epoch {
            self.pending = None;
            return;
        }
        let demanded = pending.immediate
            || self.flush_target >= pending.checkpoint.state_revision
            // A caller is still waiting on a fixed target: publish the next covering checkpoint as
            // soon as its fence is durable instead of waiting for its coalescing window.
            || self.flush_target > self.published_revision
            || Instant::now() >= pending.due;
        if demanded {
            self.in_flight = self.pending.take();
            self.bind_usage();
        }
    }

    /// Freezes the candidate covering `flush_target` into the in-flight slot, under this lock.
    ///
    /// `flush_target` is already set when this runs, and moving the newest checkpoint admitted so
    /// far into the in-flight slot inside the same critical section is what makes a fixed target
    /// exact: an effect admitted after the call can only fill `pending` again, so it can never push
    /// the target's fence out of reach. A checkpoint is a cumulative snapshot, so replacing an
    /// in-flight candidate that does not cover the target yet coalesces it into the newer revision
    /// instead of publishing both; the sink still retains one in-flight and one newest pending
    /// checkpoint.
    fn freeze_flush_cover(&mut self) {
        let covered = self
            .in_flight
            .as_ref()
            .is_some_and(|candidate| candidate.checkpoint.state_revision >= self.flush_target);
        if covered {
            return;
        }
        let Some(pending) = self.pending.take() else {
            return;
        };
        // The fixed target may still be unadmitted: keep the newest pending and freeze the cover
        // once the admission that reaches it arrives.
        if pending.checkpoint.state_revision < self.flush_target {
            self.pending = Some(pending);
            return;
        }
        self.in_flight = Some(pending);
        self.bind_usage();
    }

    /// Binds the running summary to every retained candidate whose own revision it now equals.
    ///
    /// The fold is absolute and advances one effect at a time, so exactly one moment exists at
    /// which a candidate's revision is the fold point. Binding there — instead of reading the
    /// writer's later fold at publication — is what keeps a published `state.toml` summary at its
    /// own `commit_sequence`, so a cold restore never skips effects a newer fold already applied.
    fn bind_usage(&mut self) {
        let applied = self.usage.applied_sequence;
        if !self
            .pending
            .iter()
            .chain(self.in_flight.iter())
            .any(|candidate| {
                candidate.usage.is_none() && candidate.checkpoint.state_revision == applied
            })
        {
            return;
        }
        let usage = self.usage.clone();
        for candidate in self.pending.iter_mut().chain(self.in_flight.iter_mut()) {
            if candidate.usage.is_none() && candidate.checkpoint.state_revision == applied {
                candidate.usage = Some(usage.clone());
            }
        }
    }
}

/// What the writer loop has to do next.
enum Step {
    /// Progress was made; the loop runs another unit immediately.
    Progressed,
    /// Nothing to do yet; the loop waits for this long, or forever when `None`.
    Idle(Option<Duration>),
}

#[derive(Debug, thiserror::Error)]
#[error("{source}")]
struct ClassifiedWriteError {
    kind: pl_protocol::studio::HistoryFault,
    #[source]
    source: anyhow::Error,
}

fn classified(
    kind: pl_protocol::studio::HistoryFault,
    source: impl Into<anyhow::Error>,
) -> anyhow::Error {
    ClassifiedWriteError {
        kind,
        source: source.into(),
    }
    .into()
}

/// Reliable admission state owned by the session manager, not by a writer incarnation.
pub(crate) struct HistoryChannel {
    progress: Mutex<Progress>,
    changed: Arc<tokio::sync::Notify>,
    step_lock: tokio::sync::Mutex<()>,
    /// The process-wide reliable budget, shared by every Thread's channel.
    ///
    /// It charges exactly two things, each byte once: the effects the channels hold, each at the
    /// encoded size it was admitted with, and the live-output ceilings still reserved by in-flight
    /// model/tool operations. A fact whose operation holds a ceiling takes those bytes over at
    /// admission instead of adding a second charge for the same output, and both charges are
    /// released when the fact becomes durable or the call ends without one.
    ///
    /// The prepared batches and the live projection's retained bodies are deliberately *not* part of
    /// this process counter: they are the same product content the queued effect already charged,
    /// kept alive by a different owner, so charging them here would count one body twice and make the
    /// process limit fire at half the memory it names. They are charged to the owning Thread's
    /// budget instead, which is the enforced per-Thread gate (`StoragePressure::thread_bytes`).
    process_bytes: Arc<AtomicU64>,
    status: tokio::sync::watch::Sender<HistoryStatus>,
    pressure_changed: tokio::sync::watch::Sender<()>,
}

impl HistoryChannel {
    pub(crate) fn new(process_bytes: Arc<AtomicU64>) -> Self {
        let (status, _) = tokio::sync::watch::channel(HistoryStatus::default());
        let (pressure_changed, _) = tokio::sync::watch::channel(());
        Self {
            progress: Mutex::new(Progress::default()),
            changed: Arc::new(tokio::sync::Notify::new()),
            step_lock: tokio::sync::Mutex::new(()),
            process_bytes,
            status,
            pressure_changed,
        }
    }

    fn reserve(&self, bytes: u64) -> bool {
        self.process_bytes
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |current| {
                current
                    .checked_add(bytes)
                    .filter(|total| *total <= MAX_HISTORY_PROCESS_BYTES)
            })
            .is_ok()
    }

    /// Reserves up to `max_bytes` from the process budget, returning what was really granted.
    ///
    /// `None` means there is no headroom at all: the caller must wait for the budget to free up
    /// instead of starting an operation whose output this process could not retain.
    fn reserve_output_up_to(&self, max_bytes: u64) -> Option<u64> {
        let mut granted = 0_u64;
        let reserved = self
            .process_bytes
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |current| {
                let headroom = MAX_HISTORY_PROCESS_BYTES.saturating_sub(current);
                let take = headroom.min(max_bytes);
                if take == 0 {
                    return None;
                }
                granted = take;
                Some(current.saturating_add(take))
            })
            .is_ok();
        reserved.then_some(granted)
    }

    /// Reserves one operation's live-output ceiling from the process budget.
    ///
    /// The ceiling is charged to the budget as soon as it is granted, so a call only starts when its
    /// worst case could be retained. The *whole* ceiling is given back at
    /// [`release_operation_output`](Self::release_operation_output): this is a transient reservation
    /// for output that is still in flight, not a second copy of the produced fact. Once the call
    /// returns, the content it produced is charged once by the ordinary effect admission (`admit`
    /// reserves the encoded batch and releases it when the batch becomes durable), so keeping the
    /// accepted bytes charged here too would double-count them and leak the process budget a little
    /// on every successful call. An already reserved operation keeps its original grant, so a
    /// repeated reservation never charges twice.
    fn reserve_operation_output(&self, operation_id: &str, max_bytes: u64) -> Option<u64> {
        let mut progress = self
            .progress
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(granted) = progress.operation_output.get(operation_id) {
            return Some(*granted);
        }
        let granted = self.reserve_output_up_to(max_bytes)?;
        progress
            .operation_output
            .insert(operation_id.to_owned(), granted);
        Some(granted)
    }

    /// Checks that what the producer accepted still fits the ceiling it reserved.
    ///
    /// The ceiling is already charged to the budget, so there is nothing to add here — this only
    /// guards the invariant that an operation can never accept more than the budget funded. The
    /// compared value is the ceiling that is *still reserved* for this operation's not-yet-committed
    /// output: facts the Thread admitted while the call was in flight already took over the bytes
    /// they covered, and retaining more than what is left would exceed the process budget. A refusal
    /// is typed and is what makes the producer truncate the call with the bytes it already had; the
    /// remaining ceiling is released afterwards, so the refused operation leaves no residue.
    fn charge_operation_output(
        &self,
        operation_id: &str,
        accepted_bytes: u64,
    ) -> Result<(), ColdStoreError> {
        let progress = self
            .progress
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        match progress.operation_output.get(operation_id) {
            Some(granted) if accepted_bytes > *granted => Err(cold_error(&format!(
                "operation {operation_id} accepted {accepted_bytes} bytes over its {granted}-byte reservation"
            ))),
            _ => Ok(()),
        }
    }

    /// Gives back whatever ceiling the operation still holds.
    ///
    /// The reservation is transient: the bytes its produced facts already took over are charged to
    /// those facts now, so only the remainder comes back here. Releasing the whole original ceiling
    /// instead would drop the charge of the facts that consumed it, and releasing nothing would leak
    /// the remainder of every call.
    fn release_operation_output(&self, operation_id: &str) {
        let granted = {
            let mut progress = self
                .progress
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            progress.operation_output.remove(operation_id).unwrap_or(0)
        };
        if granted > 0 {
            self.process_bytes.fetch_sub(granted, Ordering::AcqRel);
        }
    }

    pub(crate) fn subscribe(&self) -> tokio::sync::watch::Receiver<HistoryStatus> {
        self.status.subscribe()
    }

    /// Publishes the bytes the Thread's live projection currently retains in its own tables.
    ///
    /// The projection owner is the only writer of this gauge, and it reports an absolute value, so
    /// the Thread's budget covers the report accumulator and the retained bodies without charging a
    /// cumulative estimate twice. It is reported through the same status the reliable queue uses,
    /// so a Thread whose projection still holds its Turn's bodies reads as pending work — which is
    /// what pauses new model/tool admission at a safe gap instead of growing unseen memory.
    pub(crate) fn set_projection_bytes(&self, bytes: u64) {
        let mut progress = self
            .progress
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if progress.projection_bytes == bytes {
            return;
        }
        progress.projection_bytes = bytes;
        self.report(&progress);
    }

    /// Newest commit the Thread's live projection owner has handed over to this channel.
    ///
    /// The projection owner waits on the writer's watermark only for facts it handed over itself, so
    /// this ticket — not an absolute commit sequence — is what its durability barrier targets. It is
    /// `0` while nothing was handed over, which is already durable.
    pub(crate) fn handoff_ticket(&self) -> u64 {
        self.progress
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .handoff
    }

    /// Hands the Thread's single live projection owner's prepared batch to the reliable handoff.
    ///
    /// The batch is attached to the admitted effect it belongs to, so the writer commits exactly the
    /// content the projection published instead of projecting the effect a second time. Only a fact
    /// this channel already admitted can be handed over — the projection owner reads its work from
    /// this same queue — so an unknown or already durable sequence is refused rather than stored in a
    /// second, unordered slot. Returns whether the batch was attached.
    pub(in crate::studio) fn prepare(&self, prepared: Arc<PreparedEffect>) -> bool {
        // Measure before taking the lock: the batch is immutable, and sizing it walks text lengths
        // instead of encoding a payload inside the critical section.
        let bytes = crate::studio::thread_projection::retained_bytes(&prepared.items);
        let sequence = prepared.sequence;
        let mut progress = self
            .progress
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if sequence <= progress.durable {
            return false;
        }
        // Locate the entry by index so the budget update and the attach happen in one place without
        // holding a borrow of the queue while `prepared_bytes` changes.
        let Some(index) = progress
            .effects
            .iter()
            .position(|queued| queued.sequence == sequence)
        else {
            return false;
        };
        if progress.effects[index].prepared.is_none() {
            progress.prepared_bytes = progress.prepared_bytes.saturating_add(bytes);
            progress.effects[index].prepared_bytes = bytes;
        }
        progress.effects[index].prepared = Some(prepared);
        progress.handoff = progress.handoff.max(sequence);
        drop(progress);
        self.changed.notify_one();
        true
    }

    /// Oldest admitted effect the Thread's live projection owner has not taken over yet.
    ///
    /// The reliable queue is the Thread's one ordered handoff, so the projection owner consumes it in
    /// admission order: this is the next fact it owes a projection, and `None` while every admitted
    /// effect is already prepared. Nothing waits for a GUI — a Thread without subscribers still
    /// drains the queue, because the writer cannot save a fact no projection holds.
    pub(in crate::studio) fn next_unprojected(&self) -> Option<Arc<ThreadWrite>> {
        let progress = self
            .progress
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        progress
            .effects
            .iter()
            .find(|queued| queued.prepared.is_none())
            .map(|queued| queued.write.clone())
    }

    /// Publishes the failure of one admitted fact's projection.
    ///
    /// The writer never projects an effect itself, so a fact the projection owner cannot project is
    /// never handed over and never saved. Reporting it here is what makes the durable barrier fail
    /// closed with the projection's own reason — and what keeps `pressure()` agreeing with it —
    /// instead of waiting forever for a batch that will never arrive.
    ///
    /// An unchanged fault is published once. The projection owner re-observes the same pending fact
    /// on every owner snapshot, so re-publishing the identical status would wake its own status
    /// watcher and turn the failure into a busy loop instead of a paused, fail-closed Thread.
    pub(crate) fn fail_projection(&self, sequence: u64, message: String) {
        let published = {
            let mut progress = self
                .progress
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let published = progress.error.as_deref() != Some(message.as_str());
            if progress.error.is_none() {
                progress.fault_generation = progress.fault_generation.saturating_add(1);
                progress.retry_requested = false;
            }
            progress.fault_target = progress.fault_target.max(sequence);
            progress.fault = Some(pl_protocol::studio::HistoryFault::WriteFailed);
            progress.error = Some(message);
            progress.fault_recovered = false;
            progress.saving_revision = 0;
            if published {
                self.report(&progress);
            }
            published
        };
        if published {
            self.changed.notify_one();
        }
    }

    fn report(&self, progress: &Progress) {
        self.status.send_replace(HistoryStatus {
            fault_generation: progress.fault_generation,
            fault: progress.fault,
            error: progress.error.clone(),
            admitted_sequence: progress
                .effects
                .back()
                .map_or(progress.durable, |queued| queued.sequence),
            committed_sequence: progress.durable,
            queued_records: progress.effects.len() as u64,
            queued_bytes: progress
                .effects
                .iter()
                .map(|queued| queued.bytes)
                .sum::<u64>()
                .saturating_add(progress.prepared_bytes)
                .saturating_add(progress.projection_bytes),
        });
        self.pressure_changed.send_replace(());
    }

    pub(crate) fn retry(&self, generation: u64) -> Result<()> {
        let mut progress = self
            .progress
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        ensure!(
            progress.error.is_some() && progress.fault_generation == generation,
            "history fault generation changed; refresh the session status"
        );
        progress.retry_requested = true;
        self.report(&progress);
        drop(progress);
        self.changed.notify_one();
        Ok(())
    }

    pub(crate) fn mark_unavailable(&self) {
        let mut progress = self
            .progress
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if progress.effects.is_empty() && progress.pending.is_none() && progress.in_flight.is_none()
        {
            return;
        }
        if progress.error.is_none() {
            progress.fault_generation = progress.fault_generation.saturating_add(1);
        }
        progress.fault = Some(pl_protocol::studio::HistoryFault::WriterUnavailable);
        progress.error = Some("history writer unavailable; queued facts retained".to_owned());
        progress.fault_recovered = false;
        progress.retry_requested = false;
        progress.fault_target = progress
            .effects
            .back()
            .map_or(progress.durable, |queued| queued.sequence);
        self.report(&progress);
    }

    pub(crate) fn is_clean(&self) -> bool {
        let progress = self
            .progress
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        progress.effects.is_empty()
            && progress.pending.is_none()
            && progress.in_flight.is_none()
            && progress.error.is_none()
    }

    fn detect_stall(&self) {
        let mut progress = self
            .progress
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if progress.error.is_some()
            || progress.effects.is_empty()
            || !progress
                .last_progress_at
                .is_some_and(|last| last.elapsed() >= NO_PROGRESS_WINDOW)
        {
            return;
        }
        progress.fault_generation = progress.fault_generation.saturating_add(1);
        progress.fault = Some(pl_protocol::studio::HistoryFault::NoProgress);
        progress.fault_target = progress
            .effects
            .back()
            .map_or(progress.durable, |queued| queued.sequence);
        progress.error = Some("history writer made no progress with queued facts".to_owned());
        progress.fault_recovered = false;
        progress.retry_requested = false;
        self.report(&progress);
    }
}

impl std::fmt::Debug for HistoryChannel {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("HistoryChannel")
            .finish_non_exhaustive()
    }
}

struct Inner {
    store: StudioStore,
    thread: pl_protocol::Thread,
    chat: pl_core::chat::Session,
    /// This writer incarnation's coordinator identity; see [`NEXT_WRITER_INCARNATION`].
    owner: usize,
    /// One durable history reader for the life of this writer.
    ///
    /// The history database is the single ordinal/identity allocator, so the writer opens it once
    /// instead of per effect; the pool has one connection, so every batch stays a short
    /// single-writer transaction and two consecutive effects cannot interleave their allocations.
    history: tokio::sync::OnceCell<crate::studio::storage::history::HistoryStore>,
    channel: Arc<HistoryChannel>,
    changed: Arc<tokio::sync::Notify>,
    call_observer_stop: tokio::sync::watch::Sender<bool>,
}

impl Drop for Inner {
    fn drop(&mut self) {
        self.call_observer_stop.send_replace(true);
    }
}

/// Cloneable sink attached to one core owner incarnation.
#[derive(Clone)]
pub(crate) struct ThreadStorageSink(Arc<Inner>);

impl std::fmt::Debug for ThreadStorageSink {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ThreadStorageSink")
            .field("thread_id", &self.0.thread.id)
            .finish_non_exhaustive()
    }
}

fn lock_progress(inner: &Inner) -> std::sync::MutexGuard<'_, Progress> {
    inner
        .channel
        .progress
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Encoded size of one effect, or `u64::MAX` when it cannot be encoded.
fn encoded_bytes(effect: &pl_core::thread::ThreadEffectBatch) -> u64 {
    effect
        .encode()
        .map_or(u64::MAX, |payload| payload.content().len() as u64)
}

/// Bytes of `bytes` that one operation's live-output ceiling can take over.
fn output_ceiling_draw(progress: &Progress, operation: &str, want: u64) -> u64 {
    progress
        .operation_output
        .get(operation)
        .map_or(0, |reserved| (*reserved).min(want))
}

/// Plans the transfer of the reserving operation's live-output ceiling onto the fact it funds.
///
/// An operation's ceiling is the reliable budget's hold on the output that operation will produce.
/// When that output becomes an admitted fact, the hold becomes the fact's own charge instead of the
/// fact being charged a second time next to it. Both are the same bytes, so the transfer needs no
/// extra headroom at all: a Thread whose own reservations fill the process budget can still hand its
/// facts over, which is what keeps a fully reserved process from stalling behind its own queue.
///
/// Only the operation the fact itself names may fund it: `ThreadWrite::output_claim` is the
/// producing operation's own identity, so a fact takes over exactly the ceiling that was reserved
/// for it. A fact that names no operation — an accepted input, a turn/lifecycle commit, any other
/// commit that is not an operation's output — is never charged to somebody else's ceiling.
/// Pooling every in-flight ceiling would let an unfunded fact spend headroom a *different*
/// operation still needs, which is exactly the "unlimited, over-budget admission" this budget
/// exists to prevent: such a fact has to fit the real headroom and is otherwise refused with typed
/// backpressure.
///
/// Returns the `(operation, bytes)` pairs to debit and the total they cover, so the caller can take
/// the decision — and the fail-typed backpressure path — before mutating anything.
fn output_transfer_plan(
    progress: &Progress,
    funding: Option<&str>,
    bytes: u64,
) -> (Vec<(String, u64)>, u64) {
    let mut plan = Vec::new();
    let mut covered = 0_u64;
    if let Some(operation) = funding {
        let take = output_ceiling_draw(progress, operation, bytes);
        if take > 0 {
            covered += take;
            plan.push((operation.to_owned(), take));
        }
    }
    (plan, covered)
}

/// The writer's single durable history reader, opened on first use.
///
/// The handle comes from the per-Thread shared writer registry, so this sink's effect commit and a
/// live subscription's ordinal reservation share one ordered writer identity — one SQLite write
/// connection — instead of two independent writers racing for `history.sqlite`'s write lock. Reads
/// stay on their own read-only connections in WAL, so they are never queued behind this one.
async fn history_store(inner: &Inner) -> Result<crate::studio::storage::history::HistoryStore> {
    let opened = inner
        .history
        .get_or_try_init(|| async { inner.store.history_writer(&inner.thread.id).await })
        .await?;
    Ok(opened.clone())
}

/// 结束 Turn、生命周期变化或显式保存点必须立即发布，不等待合并窗口。
fn demands_immediate_publication(effect: &pl_core::thread::ThreadEffectBatch) -> bool {
    effect.lifecycle.is_some()
        || effect
            .turn
            .as_ref()
            .is_some_and(|turn| turn.state != pl_core::thread::TurnState::Running)
}

/// Publishes the current queue state so `wait_for_drain` and the product snapshot never report a
/// Thread as finished while a checkpoint is still unpublished.
fn report(inner: &Inner, progress: &Progress) {
    inner.channel.report(progress);
    // The call recorder is one global writer shared by every Thread, so any owning writer can
    // publish its queue pressure; the coordinator stores the latest observation.
    let calls = inner.store.calls();
    inner
        .store
        .thread_persistence()
        .report_calls(calls.metrics());
    let pending = progress
        .effects
        .iter()
        .map(|queued| queued.sequence)
        .collect::<Vec<_>>();
    let pending_bytes = progress
        .effects
        .iter()
        .fold(0_u64, |total, queued| total.saturating_add(queued.bytes));
    // 进行中 + 最新待写的未发布 checkpoint 也是待处理操作；只保留 checkpoint dirty 时，最老
    // 待写年龄仍来自它自己的 saved_at，而不是退化成 None。
    let dirty = progress
        .pending
        .iter()
        .chain(progress.in_flight.iter())
        .filter(|checkpoint| checkpoint.epoch > progress.published_epoch)
        .collect::<Vec<_>>();
    let pending_operations = progress.effects.len() as u64 + dirty.len() as u64;
    let now = crate::studio::unix_seconds();
    let oldest_pending_age_millis = progress
        .effects
        .iter()
        .map(|queued| queued.write.checkpoint.saved_at)
        .chain(
            dirty
                .iter()
                .map(|checkpoint| checkpoint.checkpoint.saved_at),
        )
        .map(|saved_at| {
            u64::try_from(now.saturating_sub(saved_at))
                .unwrap_or(0)
                .saturating_mul(1000)
        })
        .reduce(u64::min);
    let max_admitted = progress
        .effects
        .back()
        .map(|queued| queued.sequence)
        .unwrap_or(0);
    let metrics = ThreadPersistenceMetrics {
        fault_generation: progress.fault_generation,
        fault: progress.fault,
        state_dirty_revision: dirty
            .iter()
            .map(|checkpoint| checkpoint.checkpoint.state_revision)
            .max()
            .unwrap_or(progress.published_revision),
        state_saving_revision: progress.saving_revision,
        state_durable_revision: progress.published_revision,
        history_admitted_sequence: max_admitted.max(progress.durable),
        history_durable_sequence: progress.durable,
        calls_admitted_sequence: progress.calls_admitted,
        calls_durable_sequence: progress.calls_durable,
        pending_operations,
        pending_bytes,
        in_flight_bytes: progress.in_flight_bytes,
        oldest_pending_age_millis,
        pressure_paused: progress.pressure_paused,
    };
    inner.store.thread_persistence().update(
        &inner.thread.id,
        inner.owner,
        pending,
        !dirty.is_empty(),
        progress.error.clone(),
        metrics,
    );
}

fn clear_recovered_fault(progress: &mut Progress) {
    if progress.retry_requested
        && progress.durable >= progress.fault_target
        && progress.published_revision >= progress.fault_target
    {
        progress.error = None;
        progress.fault = None;
        progress.retry_requested = false;
        // The retry named this generation and the fixed target really landed, so an explicit
        // continue would be accepted now: that verified fact is what the UI is allowed to show.
        progress.fault_recovered = true;
    }
}

impl ThreadStorageSink {
    pub(crate) async fn new(store: StudioStore, thread: pl_protocol::Thread) -> Result<Self> {
        let chat = store.chat_session(&thread.id).await?;
        chat.initialize_order_allocator().await?;
        let (call_observer_stop, mut stop_observer) = tokio::sync::watch::channel(false);
        let owner = NEXT_WRITER_INCARNATION.fetch_add(1, Ordering::Relaxed);
        let channel = store.thread_persistence().history_channel(&thread.id);
        let inner = Arc::new(Inner {
            store,
            thread,
            chat,
            owner,
            history: tokio::sync::OnceCell::new(),
            changed: channel.changed.clone(),
            channel,
            call_observer_stop,
        });
        // Bind this incarnation before it reports anything: a superseded writer must never clear or
        // fail the watermarks of the writer that replaced it, and this one must be freed from the
        // queue its predecessor left behind.
        inner
            .store
            .thread_persistence()
            .claim(&inner.thread.id, owner);
        {
            let progress = lock_progress(&inner);
            report(&inner, &progress);
        }
        let observer = Arc::downgrade(&inner);
        let calls = inner.store.calls().clone();
        tokio::spawn(async move {
            let mut durable_tickets = calls.subscribe_durable_ticket();
            loop {
                tokio::select! {
                    () = refresh_call_watermark(&calls, &observer) => {},
                    _ = stop_observer.changed() => break,
                }
                tokio::select! {
                    Ok(()) = durable_tickets.changed() => {},
                    _ = stop_observer.changed() => break,
                }
            }
        });
        let detach = DetachGuard {
            store: inner.store.clone(),
            thread_id: inner.thread.id.clone(),
            owner,
        };
        let worker = inner.clone();
        let watchdog = Arc::downgrade(&inner);
        tokio::spawn(async move {
            while let Some(inner) = watchdog.upgrade() {
                inner.channel.detect_stall();
                drop(inner);
                tokio::time::sleep(Duration::from_secs(10)).await;
            }
        });
        tokio::spawn(async move {
            let inner = worker;
            let _detach = detach;
            // 连续可重试失败的起点；任何一次成功的推进都清零。只有持续超过窗口才升级上报。
            let mut retrying_since: Option<Instant> = None;
            loop {
                // The task keeps the persistence owner alive while it owes facts, even when the
                // last Thread or GUI handle is dropped. Once clean it may release the connection.
                if Arc::strong_count(&inner) == 1 {
                    let progress = lock_progress(&inner);
                    if progress.effects.is_empty()
                        && progress.pending.is_none()
                        && progress.in_flight.is_none()
                        && progress.error.is_none()
                    {
                        return;
                    }
                }
                let paused = {
                    let progress = lock_progress(&inner);
                    progress.error.is_some()
                        && !progress.retry_requested
                        && progress.fault != Some(pl_protocol::studio::HistoryFault::QueueFull)
                };
                if paused {
                    // 通道可以把一个本 incarnation 自己没失败过的 Thread 标成失败：投影无法完成的
                    // 事实永远不会被交接，writer 因此既不写也不报错。协调器聚合的错误正是进程级
                    // drain 等待的那个事实，所以这里把通道的故障转成协调器可见的错误；否则暂停的
                    // 队列会把停机变成无界等待，而不是带着真实原因的显式失败。
                    {
                        let progress = lock_progress(&inner);
                        report(&inner, &progress);
                    }
                    inner.changed.notified().await;
                    continue;
                }
                match step(&inner).await {
                    Ok(Step::Progressed) => {
                        retrying_since = None;
                        continue;
                    }
                    Ok(Step::Idle(wait)) => {
                        match wait {
                            // 错过 tick 只写一次：等待到期后直接推进下一步，不补写。
                            Some(wait) => {
                                tokio::select! {
                                    () = tokio::time::sleep(wait) => {}
                                    () = inner.changed.notified() => {}
                                }
                            }
                            None => {
                                tokio::select! {
                                    () = tokio::time::sleep(Duration::from_secs(1)) => {}
                                    () = inner.changed.notified() => {}
                                }
                            }
                        }
                    }
                    Err(error) => {
                        let message = error.to_string();
                        let now = Instant::now();
                        let since = *retrying_since.get_or_insert(now);
                        if !is_retryable_write(&message)
                            || now.saturating_duration_since(since) >= RETRYABLE_BUSY_WINDOW
                        {
                            let kind = error
                                .downcast_ref::<ClassifiedWriteError>()
                                .map_or(pl_protocol::studio::HistoryFault::WriteFailed, |typed| {
                                    typed.kind
                                });
                            record_error(&inner, kind, message);
                            retrying_since = None;
                        } else {
                            note_absorbed_conflict(&inner);
                            tokio::select! {
                                () = tokio::time::sleep(Duration::from_secs(1)) => {},
                                () = inner.store.thread_persistence().retry_notified() => {},
                            }
                        }
                    }
                }
            }
        });
        Ok(Self(inner))
    }
}

async fn refresh_call_watermark(
    calls: &crate::studio::storage::calls::CallsStore,
    observer: &std::sync::Weak<Inner>,
) {
    let Some(inner) = observer.upgrade() else {
        return;
    };
    let id = inner.thread.id.clone();
    let store = inner.store.clone();
    drop(inner);
    match calls.durable_effect_sequence(&id).await {
        Ok(sequence) => {
            let Some(inner) = observer.upgrade() else {
                return;
            };
            let mut progress = lock_progress(&inner);
            if sequence > progress.calls_durable {
                progress.calls_durable = sequence;
                progress.calls_admitted = progress.calls_admitted.max(sequence);
                report(&inner, &progress);
            }
        }
        Err(error) => {
            calls.mark_statistics_gap();
            tracing::warn!(thread_id = id, %error, "call watermark could not be read");
            store.thread_persistence().report_calls(calls.metrics());
        }
    }
}

/// Performs one unit of durable work.
async fn step(inner: &Inner) -> Result<Step> {
    let _single_writer = inner.channel.step_lock.lock().await;
    // A demanded checkpoint publishes as soon as its own fence is durable, even while later effects
    // are still queued: a fixed flush target or an immediate publication must not wait for the
    // whole queue to drain. Selecting it into the in-flight slot also shields it from later
    // admissions that keep updating the newest pending revision.
    {
        let mut progress = lock_progress(inner);
        progress.select_in_flight();
    }
    let publishable = {
        let progress = lock_progress(inner);
        progress
            .in_flight
            .as_ref()
            .is_some_and(|candidate| candidate.checkpoint.history_fence <= progress.durable)
    };
    if publishable {
        return publish_checkpoint(inner).await;
    }
    let next_effect = lock_progress(inner).effects.front().map(|queued| {
        (
            queued.sequence,
            queued.write.clone(),
            queued.prepared.clone(),
        )
    });
    if let Some((sequence, write, prepared)) = next_effect {
        // The Thread's live projection owner consumed this fact before the durable store may write
        // it: the writer commits the content that owner published instead of projecting the effect
        // again, so live and durable history can never disagree and durability never advances past a
        // commit the product has not published.
        let Some(prepared) = prepared else {
            return Ok(Step::Idle(None));
        };
        persist_effect(inner, sequence, &write, &prepared).await?;
        let mut progress = lock_progress(inner);
        if let Some(committed) = progress.effects.pop_front() {
            let bytes = committed.bytes;
            let prepared_bytes = committed.prepared_bytes;
            ensure!(
                committed.sequence == sequence,
                "history queue head changed before acknowledgement"
            );
            progress.prepared_bytes = progress.prepared_bytes.saturating_sub(prepared_bytes);
            inner
                .channel
                .process_bytes
                .fetch_sub(bytes, Ordering::AcqRel);
            progress.durable = progress.durable.max(sequence);
            progress.last_progress_at = (!progress.effects.is_empty()).then(Instant::now);
        }
        clear_recovered_fault(&mut progress);
        report(inner, &progress);
        drop(progress);
        inner.changed.notify_one();
        return Ok(Step::Progressed);
    }
    publish_checkpoint(inner).await
}

/// Writes one effect's history and call records, then folds its cumulative accounting.
///
/// The committed content is the immutable batch the Thread's single live projection owner prepared
/// for exactly this commit. The writer therefore never projects the effect a second time — the whole
/// product body is projected once, published once and written once — and the identity/revision pair
/// it confirms is the pair that owner handed over.
async fn persist_effect(
    inner: &Inner,
    sequence: u64,
    write: &ThreadWrite,
    projected: &PreparedEffect,
) -> Result<()> {
    ensure!(
        projected.sequence == sequence,
        "prepared history batch {sequence} was handed over as {}",
        projected.sequence
    );
    {
        let mut progress = lock_progress(inner);
        progress.in_flight_bytes = encoded_bytes(&write.effect);
    }
    let history = history_store(inner).await?;
    // The durable identity indexes answer a repeated `submitPrompt` and a repeated message delivery
    // after the input/message left core state. Only minimal identities are written, and they share
    // this effect's transaction so an index can never be durable without the effect it describes; a
    // mismatching digest fails the transaction instead of overwriting an accepted identity.
    history
        .commit_effect(
            write.effect.sequence,
            EffectCommit {
                items: &projected.items,
                rolled_back_turns: &rolled_back_turns(&write.checkpoint.state),
                identities: &terminal_input_identities(&write.checkpoint.state),
                messages: &admitted_message_identities(&write.effect),
                receipts: &fact_receipts(&write.effect)?,
                tasks: &write.effect.tasks,
                deliveries: &write.effect.deliveries,
                delivery_repairs: &write.effect.delivery_repairs,
                attempt: write.effect.attempt.as_ref(),
            },
        )
        .await?;
    // The writer confirms exactly the identity and revision this transaction committed, which is the
    // same batch the Thread's live projection published into the shared session before the handoff
    // reached this queue. No body is read back and no second content cache exists, so the saved
    // watermark can never drift from the published content.
    for item in &projected.items {
        inner.chat.confirm_saved(&item.id, item.revision);
    }
    if let Some(attempt) = &write.effect.attempt {
        // Release the attempt's speculative previews, but never an identity this transaction just
        // confirmed: the durable row owns that content and its window position even if the session
        // never saw the terminal publication or a slow producer published one more revision of it.
        inner.chat.drop_previews_with_prefix_except(
            &crate::studio::thread_projection::presentation_preview_prefix(&attempt.attempt_id),
            projected.items.iter().map(|item| item.id.as_str()),
        );
    }
    let statistics_admitted = inner.store.calls().try_admit_effect(&write.effect);
    // 累计摘要按 effect 顺序折叠一次：绝对累计值，重复折叠同一 effect 是 no-op。折叠发生在
    // 本 effect 的 durable 事实之后，因此实时与冷恢复读到的都是同一条已落库事实的结果。
    let summary = {
        let mut progress = lock_progress(inner);
        if statistics_admitted {
            progress.calls_admitted = progress.calls_admitted.max(sequence);
        }
        let mut summary = progress.usage.clone();
        crate::studio::thread_projection::fold_effect_accounting(&mut summary, &write.effect)?;
        progress.usage = summary.clone();
        // 折叠点正好落在某个保留候选的 revision 上时，把候选自己的摘要绑定在这一刻；此后即使
        // 继续折叠更晚的 effect，也不会改变该候选将来发布的累计值。
        progress.bind_usage();
        summary
    };
    inner
        .store
        .thread_persistence()
        .set_usage(&inner.thread.id, summary);
    {
        let mut progress = lock_progress(inner);
        progress.in_flight_bytes = 0;
        progress.pressure_paused = write.checkpoint.state.persistence.pressure_paused;
        report(inner, &progress);
    }
    Ok(())
}

/// Publishes the in-flight checkpoint once its fences are durable.
async fn publish_checkpoint(inner: &Inner) -> Result<Step> {
    let (candidate, epoch, usage) = {
        let progress = lock_progress(inner);
        let Some(candidate) = progress.in_flight.as_ref() else {
            // Nothing selected: the loop waits until the newest dirty revision is demanded.
            return Ok(match progress.pending.as_ref() {
                Some(pending) if pending.epoch > progress.published_epoch => {
                    Step::Idle(Some(pending.due.saturating_duration_since(Instant::now())))
                }
                _ => Step::Idle(None),
            });
        };
        // 历史固定水位：fence 未 durable 之前不发布 TOML。
        if candidate.checkpoint.history_fence > progress.durable {
            return Ok(Step::Idle(None));
        }
        // 该候选自己的累计摘要：绑定在折叠刚好到达它 revision 的那一刻。只有写者已经折叠到该
        // revision 时才有值，绝不读取在此之后继续累加的 fold，否则 `state.commit_sequence=N`
        // 会配上 `applied_sequence=M>N`，冷恢复会跳过 N+1..M。
        let usage = candidate.usage.clone().or_else(|| {
            (progress.usage.applied_sequence == candidate.checkpoint.state_revision)
                .then(|| progress.usage.clone())
        });
        (candidate.checkpoint.clone(), candidate.epoch, usage)
    };
    let usage = usage
        .context("checkpoint candidate has no cumulative summary folded to its own revision")?;
    ensure!(
        usage.applied_sequence == candidate.state_revision,
        "checkpoint summary at {} does not match revision {}",
        usage.applied_sequence,
        candidate.state_revision
    );
    let watermark = history_store(inner).await?.watermark().await?;
    ensure!(
        watermark >= candidate.history_fence,
        "history watermark {watermark} is behind checkpoint fence {}",
        candidate.history_fence
    );
    // 统一 blob fence：checkpoint 引用的附件 blob 必须已经 durable，才允许发布引用它们的 TOML。
    let referenced = crate::studio::thread_projection::referenced_attachment_ids(&candidate.state);
    inner
        .store
        .blob_fence(&inner.thread.id, &referenced)
        .await
        .map_err(|error| classified(pl_protocol::studio::HistoryFault::BlobFailed, error))?;
    let mut pruned = candidate.pruned();
    // 累计摘要随 checkpoint 一起发布，冷恢复因此不需要重新聚合历史集合。
    pruned.state.usage_summary = usage;
    {
        let mut progress = lock_progress(inner);
        progress.saving_revision = pruned.state_revision;
        report(inner, &progress);
    }
    inner
        .store
        .state(&inner.thread.id)
        .publish(&pruned)
        .await
        .map_err(|error| classified(pl_protocol::studio::HistoryFault::CheckpointFailed, error))?;
    {
        let mut progress = lock_progress(inner);
        progress.published_revision = progress.published_revision.max(pruned.state_revision);
        progress.published_epoch = progress.published_epoch.max(epoch);
        // 本候选写盘期间可能有覆盖更大固定目标的候选被冻结进 in-flight：只清除仍在原位的那
        // 一份，绝不吞掉后来冻结的封面候选。
        if progress
            .in_flight
            .as_ref()
            .is_some_and(|candidate| candidate.epoch == epoch)
        {
            progress.in_flight = None;
        }
        progress.saving_revision = 0;
        clear_recovered_fault(&mut progress);
        report(inner, &progress);
    }
    inner.changed.notify_one();
    Ok(Step::Progressed)
}

/// Minimal durable identities of the messages one effect admitted.
///
/// The writer stores them in the same transaction as the effect's history, so a delivery that left
/// core's resident window can still be adjudicated from the per-Thread index. The digest comes from
/// core's own message digest over the frozen source/body/context fields, which is exactly what the
/// host recomputes when it repeats a delivery, so an identical repeat returns the original sequence
/// and a different body is rejected instead of delivered twice.
fn admitted_message_identities(
    effect: &pl_core::thread::ThreadEffectBatch,
) -> Vec<MessageIdentityWrite> {
    effect
        .inbox
        .iter()
        .map(|record| MessageIdentityWrite {
            message_id: record.message.id.clone(),
            item_id: crate::studio::thread_projection::order::message_id(&record.message.id),
            sequence: record.sequence,
            digest: Some(record.message.digest()),
        })
        .collect()
}

/// Minimal identities of every terminal input the effect-matched state still carries.
///
/// Alongside the canonical identity it derives the host submission digest from the saved payload,
/// so the durable index can prove a repeated submission carries the same body after the resident
/// identity window (and the attachment drafts it referenced) are gone.
fn terminal_input_identities(snapshot: &ThreadSnapshot) -> Vec<InputIdentityWrite> {
    use pl_core::thread::input::InputState;
    snapshot
        .inputs
        .iter()
        .filter(|record| record.state != InputState::Pending)
        .map(|record| InputIdentityWrite {
            entry: crate::studio::storage::state::InputIdentityEntry::new(
                pl_core::thread::input::input_identity(record),
                record.accepted_sequence,
            ),
            request_digest: crate::studio::thread_projection::saved_prompt_request_digest(
                &record.input.payload,
            ),
            presentation: crate::studio::thread_projection::input_presentation(record),
        })
        .collect()
}

fn record_error(inner: &Inner, kind: pl_protocol::studio::HistoryFault, message: String) {
    {
        let mut progress = lock_progress(inner);
        progress.fault_generation = progress.fault_generation.saturating_add(1);
        progress.fault_target = progress
            .effects
            .back()
            .map_or(progress.durable, |queued| queued.sequence);
        progress.retry_requested = false;
        progress.fault = Some(kind);
        progress.error = Some(message);
        progress.fault_recovered = false;
        // A failed publication is no longer being serialized; the retained checkpoint stays queued
        // for retry, and the metric must not keep reporting it as actively saving.
        progress.saving_revision = 0;
        report(inner, &progress);
    }
    inner.changed.notify_one();
}

/// Keeps the observable metrics honest while one retryable conflict is absorbed.
///
/// The failed step is no longer writing, so the in-flight byte gauge must not keep claiming a write
/// in progress; the queued effects stay counted, so the process-wide drain still waits and this
/// incarnation never looks settled. No error is published, so `pressure()`/`cold_error` stay clear
/// and the next model admission is not failed by a conflict the writer is still resolving.
fn note_absorbed_conflict(inner: &Inner) {
    let mut progress = lock_progress(inner);
    progress.in_flight_bytes = 0;
    progress.saving_revision = 0;
    report(inner, &progress);
}

impl ColdStore for ThreadStorageSink {
    fn reserve_observed_item(&self, thread_id: &str, item_id: &str) -> Result<(), ColdStoreError> {
        if thread_id != self.0.thread.id {
            return Err(ColdStoreError {
                source: Box::new(std::io::Error::other("Thread persistence owner mismatch")),
            });
        }
        self.0
            .chat
            .reserve_order_in_memory(item_id)
            .map(|_| ())
            .map_err(|source| ColdStoreError {
                source: Box::new(source),
            })
    }

    fn subscribe_pressure(&self, thread_id: &str) -> Option<tokio::sync::watch::Receiver<()>> {
        (thread_id == self.0.thread.id).then(|| self.0.channel.pressure_changed.subscribe())
    }

    fn reserve_operation_output(
        &self,
        thread_id: &str,
        operation_id: &str,
        max_bytes: u64,
    ) -> Result<u64, ColdStoreError> {
        if thread_id != self.0.thread.id {
            return Err(cold_error("Thread persistence owner mismatch"));
        }
        self.0
            .channel
            .reserve_operation_output(operation_id, max_bytes)
            .ok_or_else(|| {
                // No headroom is backpressure, not a failed save: the caller waits at a storage
                // safety point and retries, and the Thread resumes by itself once the budget frees
                // up. Latching a hard fault here would turn a transient wait into a pause that only
                // an explicit resume could release.
                cold_error(&format!(
                    "reliable budget exhausted reserving operation {operation_id}"
                ))
            })
    }

    fn charge_operation_output(
        &self,
        thread_id: &str,
        operation_id: &str,
        accepted_bytes: u64,
    ) -> Result<(), ColdStoreError> {
        if thread_id != self.0.thread.id {
            return Err(cold_error("Thread persistence owner mismatch"));
        }
        // The typed refusal a ceiling cannot hold must reach the caller unchanged: it is what makes
        // the producer cancel the call with the bytes it already has instead of buffering output
        // this process could not retain.
        self.0
            .channel
            .charge_operation_output(operation_id, accepted_bytes)
    }

    fn release_operation_output(&self, thread_id: &str, operation_id: &str) {
        if thread_id == self.0.thread.id {
            self.0.channel.release_operation_output(operation_id);
        }
    }

    fn pressure(&self, thread_id: &str) -> StoragePressure {
        if thread_id != self.0.thread.id {
            return StoragePressure {
                error: Some(storage_error("Thread persistence owner mismatch")),
                ..Default::default()
            };
        }
        let progress = lock_progress(&self.0);
        // The Thread's budget covers every resident body this Thread presents, and one of them only
        // once: the effects still queued, the prepared product batches the reliable handoff still
        // retains (the writer's queue length alone would under-report a Thread whose projection is
        // already done but whose save is not), the bodies its live projection and report accumulator
        // keep after their batch became durable, and the ceilings still reserved for in-flight
        // model/tool output. The in-flight ceilings are what a Thread about to publish its result
        // holds; leaving them out would report a Thread as idle while a call is buffering its answer.
        let reserved = progress
            .operation_output
            .values()
            .fold(0_u64, |total, reserved| total.saturating_add(*reserved));
        let mut bytes = progress
            .effects
            .iter()
            .fold(progress.prepared_bytes, |total, queued| {
                total.saturating_add(queued.bytes)
            })
            .saturating_add(progress.projection_bytes)
            .saturating_add(reserved);
        if progress
            .pending
            .as_ref()
            .is_some_and(|candidate| candidate.epoch > progress.published_epoch)
            || progress
                .in_flight
                .as_ref()
                .is_some_and(|candidate| candidate.epoch > progress.published_epoch)
        {
            bytes = bytes.saturating_add(1);
        }
        StoragePressure {
            thread_bytes: bytes,
            store_bytes: self.0.channel.process_bytes.load(Ordering::Acquire),
            // Typed durability receipt: the owner releases the live effect window from this at a
            // storage safety point instead of waiting for an explicit `flush` command.
            durable_sequence: progress.durable,
            // Typed fault category: the history channel already classifies its own failure, so no
            // consumer has to read the error text to know what failed.
            fault: progress.fault.map(storage_fault_kind),
            fault_generation: progress.fault_generation,
            // The writer's own recovery receipt, named by generation: `fault_recovered` means the
            // retry of *this* generation reached the fixed durable target, so the owner can require
            // that exact verdict before releasing the fault it holds instead of accepting an older
            // generation's success.
            recovered_generation: progress
                .fault_recovered
                .then_some(progress.fault_generation),
            error: progress.error.as_deref().map(storage_error),
        }
    }

    fn admit(&self, thread_id: &str, write: ThreadWrite) -> Result<(), ColdStoreError> {
        if thread_id != self.0.thread.id
            || write.effect.thread_id != thread_id
            || write.checkpoint.thread_id != thread_id
            || write.effect.sequence != write.checkpoint.state_revision
            || write.checkpoint.history_fence != write.effect.sequence
        {
            // A contract failure is terminal for this sink and never a transient conflict: publish it
            // so `pressure().error` — the one report core mirrors into its own storage latch — agrees
            // with the error this call returns instead of letting the latch look recovered.
            record_error(
                &self.0,
                pl_protocol::studio::HistoryFault::WriteFailed,
                "Thread persistence ticket mismatch".to_owned(),
            );
            return Err(ColdStoreError {
                source: Box::new(std::io::Error::other("Thread persistence ticket mismatch")),
            });
        }
        let sequence = write.effect.sequence;
        let bytes = encoded_bytes(&write.effect);
        let mut progress = lock_progress(&self.0);
        if sequence > progress.durable
            && !progress
                .effects
                .iter()
                .any(|queued| queued.sequence == sequence)
        {
            let queued_bytes = progress
                .effects
                .iter()
                .fold(0_u64, |total, queued| total.saturating_add(queued.bytes));
            // The fact takes over the bytes of its own operation's reservation instead of being
            // charged next to them; only what that one ceiling does not cover needs new headroom.
            let (transfer, covered) =
                output_transfer_plan(&progress, write.output_claim.as_deref(), bytes);
            let added = bytes.saturating_sub(covered);
            if progress.effects.len() >= MAX_HISTORY_BATCHES
                || queued_bytes
                    .checked_add(bytes)
                    .is_none_or(|total| total > MAX_HISTORY_THREAD_BYTES)
                || (added > 0 && !self.0.channel.reserve(added))
            {
                let message =
                    format!("history queue full for Thread {thread_id} at write_seq {sequence}");
                if progress.error.is_none() {
                    progress.fault_generation = progress.fault_generation.saturating_add(1);
                    progress.retry_requested = false;
                }
                progress.fault_target = progress.fault_target.max(sequence);
                progress.fault = Some(pl_protocol::studio::HistoryFault::QueueFull);
                progress.error = Some(message.clone());
                progress.fault_recovered = false;
                report(&self.0, &progress);
                return Err(cold_error(&message));
            }
            for (operation, taken) in transfer {
                let emptied = match progress.operation_output.get_mut(&operation) {
                    Some(reserved) => {
                        *reserved = reserved.saturating_sub(taken);
                        *reserved == 0
                    }
                    None => false,
                };
                if emptied {
                    progress.operation_output.remove(&operation);
                }
            }
            progress.effects.push_back(QueuedEffect {
                sequence,
                write: Arc::new(write.clone()),
                bytes,
                // The projection owner takes this entry over from the same queue, in this order.
                prepared: None,
                prepared_bytes: 0,
            });
            progress.last_progress_at.get_or_insert_with(Instant::now);
        }
        // 首次受理从 checkpoint 继承已折叠的累计摘要，然后只折叠本 incarnation 的新 effect。
        if progress.usage.applied_sequence == 0
            && write.checkpoint.state.usage_summary.applied_sequence > 0
        {
            progress.usage = write.checkpoint.state.usage_summary.clone();
        }
        progress.pressure_paused = write.checkpoint.state.persistence.pressure_paused;
        let immediate = demands_immediate_publication(&write.effect);
        progress.mark_checkpoint(write.checkpoint, immediate);
        report(&self.0, &progress);
        drop(progress);
        self.0.changed.notify_one();
        Ok(())
    }

    async fn flush(&self, thread_id: &str, sequence: u64) -> Result<(), ColdStoreError> {
        if thread_id != self.0.thread.id {
            return Err(ColdStoreError {
                source: Box::new(std::io::Error::other("Thread persistence owner mismatch")),
            });
        }
        {
            let mut progress = lock_progress(&self.0);
            progress.flush_target = progress.flush_target.max(sequence);
            // 固定目标与覆盖它的候选在同一个临界区里绑定：调用之后受理的 effect 只能重新填满
            // `pending`，不会把 target 对应的 history fence 推到更晚的 admission 之后。
            progress.freeze_flush_cover();
        }
        // 固定目标立即推动目标 checkpoint；等待只在协调器广播的持久化水位上，不依赖队列时序。
        self.0.changed.notify_one();
        let coordinator = self.0.store.thread_persistence().clone();
        let mut progress = coordinator.subscribe();
        // 第二个失败来源是可靠受理通道自己的状态：一个投影无法完成的事实永远到不了协调器的
        // 水位，只有通道会带着投影的真实原因报告它。只等协调器会把这种失败变成永久挂起，
        // 因此两处都必须唤醒这次屏障。
        let mut status = self.0.channel.subscribe();
        loop {
            {
                let local = lock_progress(&self.0);
                // The barrier covers both the ordered history write and the checkpoint publication,
                // so a released owner never leaves an unpublished `state.toml` behind.
                if local.durable >= sequence && local.published_revision >= sequence {
                    return Ok(());
                }
                if let Some(error) = local.error.clone() {
                    return Err(ColdStoreError {
                        source: Box::new(std::io::Error::other(error)),
                    });
                }
            }
            tokio::select! {
                changed = progress.changed() => {
                    changed.map_err(|_| ColdStoreError {
                        source: Box::new(std::io::Error::other(
                            "Thread persistence progress channel closed",
                        )),
                    })?;
                }
                changed = status.changed() => {
                    changed.map_err(|_| ColdStoreError {
                        source: Box::new(std::io::Error::other(
                            "Thread history status channel closed",
                        )),
                    })?;
                }
            }
        }
    }

    fn read_tool_task(
        &self,
        thread_id: &str,
        task_id: &str,
    ) -> impl std::future::Future<
        Output = Result<Option<pl_core::thread::cold::DurableToolTask>, ColdStoreError>,
    > + Send {
        let store = self.0.store.clone();
        let owner = self.0.thread.id.clone();
        let task_id = task_id.to_owned();
        async move {
            if thread_id != owner {
                return Err(cold_error("Thread persistence owner mismatch"));
            }
            store
                .history(&owner)
                .await
                .map_err(|error| cold_error(&error.to_string()))?
                .tool_task(&task_id)
                .await
                .map_err(|error| cold_error(&error.to_string()))
        }
    }
}

/// Durable receipts for the terminal interaction/permission facts one effect committed.
///
/// The receipt payload is the committed core record itself (its stable serde encoding is the
/// source of the digest) and its revision is the record's own revision, so a repeated terminal
/// command can be answered with the same receipt while a conflicting payload for one revision
/// fails the effect transaction instead of overwriting the accepted fact.
fn fact_receipts(
    effect: &pl_core::thread::ThreadEffectBatch,
) -> Result<Vec<crate::studio::storage::history::FactReceiptWrite>> {
    let mut receipts = Vec::new();
    for record in effect.interactions.iter() {
        receipts.push(crate::studio::storage::history::FactReceiptWrite {
            item_id: crate::studio::thread_projection::order::receipt_id(
                "interaction",
                &record.request.id,
            ),
            revision: record.revision,
            kind: "interaction",
            payload: serde_json::to_string(record)?,
        });
    }
    for record in effect.permissions.iter() {
        receipts.push(crate::studio::storage::history::FactReceiptWrite {
            item_id: crate::studio::thread_projection::order::receipt_id("permission", &record.id),
            revision: record.revision,
            kind: "permission",
            payload: serde_json::to_string(record)?,
        });
    }
    Ok(receipts)
}

fn rolled_back_turns(snapshot: &ThreadSnapshot) -> std::collections::BTreeSet<String> {
    let mut removed = std::collections::BTreeSet::new();
    for replacement in snapshot.context_replacements.iter() {
        if replacement.reason != pl_core::thread::ContextReplacementReason::Rewind {
            continue;
        }
        let retained = replacement
            .current
            .records
            .iter()
            .filter_map(|record| record.turn_id.as_ref())
            .collect::<std::collections::BTreeSet<_>>();
        for record in replacement.previous.records.iter() {
            if let Some(turn) = &record.turn_id
                && !retained.contains(turn)
            {
                removed.insert(turn.clone());
            }
        }
    }
    removed
}

fn storage_error(message: &str) -> Arc<ColdStoreError> {
    Arc::new(ColdStoreError {
        source: Box::new(std::io::Error::other(message.to_owned())),
    })
}

/// The protocol's typed history fault as the core storage fault the owner mirrors.
///
/// Both sides are already typed values: this is a rename, not a classification. Core therefore never
/// sees an error string that it would have to interpret to know what failed.
fn storage_fault_kind(
    fault: pl_protocol::studio::HistoryFault,
) -> pl_core::thread::cold::StorageFaultKind {
    use pl_core::thread::cold::StorageFaultKind;
    use pl_protocol::studio::HistoryFault;
    match fault {
        HistoryFault::QueueFull => StorageFaultKind::QueueFull,
        HistoryFault::WriteFailed => StorageFaultKind::WriteFailed,
        HistoryFault::WriterUnavailable => StorageFaultKind::WriterUnavailable,
        HistoryFault::NoProgress => StorageFaultKind::NoProgress,
        HistoryFault::CheckpointFailed => StorageFaultKind::CheckpointFailed,
        HistoryFault::BlobFailed => StorageFaultKind::BlobFailed,
    }
}

/// One typed cold-store failure without the shared-error wrapper.
fn cold_error(message: &str) -> ColdStoreError {
    ColdStoreError {
        source: Box::new(std::io::Error::other(message.to_owned())),
    }
}

#[cfg(test)]
mod storage_fault_tests {
    use super::*;
    use crate::studio::storage::history::chat_item;
    use pl_core::{
        context::{ContextContent, OpaquePayload},
        model::{
            DynModelSession, ModelError, ModelRequest, ModelSession, ModelStepOutput,
            ModelToolCall, PreparedModelCall,
        },
        thread::{
            ModelStepLimit, ThreadEffectBatch, ThreadHandle, ThreadSnapshot, TurnInput,
            TurnOutcome, TurnRecord, TurnState,
            cold::ColdStoreHandle,
            input::{InputChange, InputRecord, InputState, ThreadInput},
            task::TaskStatus,
        },
        tool::{
            ToolOutput,
            opaque::{CallContext, Registration, Tool, ToolError},
        },
    };
    use sea_orm::{ConnectionTrait, Database};
    use tokio::sync::{Notify, mpsc};
    use tokio_util::sync::CancellationToken;

    #[derive(Debug)]
    struct ConcurrentModel(Arc<AtomicUsize>);

    impl ModelSession for ConcurrentModel {
        async fn prepare(
            &mut self,
            request: ModelRequest,
        ) -> Result<PreparedModelCall, ModelError> {
            self.0.fetch_add(1, Ordering::Relaxed);
            Ok(PreparedModelCall::new(async move {
                Ok(ModelStepOutput {
                    attempt_id: request.attempt_id,
                    base_context_revision: request.context.revision,
                    content: vec![ContextContent::Text {
                        text: Arc::from("running both tools"),
                    }],
                    tool_calls: ["first", "second"]
                        .into_iter()
                        .map(|id| ModelToolCall {
                            call_id: id.to_owned(),
                            tool_id: id.to_owned(),
                            arguments: OpaquePayload::text(id),
                        })
                        .collect(),
                    private_context: None,
                    usage: Default::default(),
                })
            }))
        }

        async fn close(&mut self) -> Result<(), ModelError> {
            Ok(())
        }
    }

    #[derive(Debug)]
    struct ConcurrentTool {
        started: mpsc::UnboundedSender<String>,
        release: Arc<Notify>,
        executions: Arc<Mutex<Vec<String>>>,
    }

    impl Tool for ConcurrentTool {
        async fn execute(
            &self,
            input: OpaquePayload,
            context: CallContext,
        ) -> Result<ToolOutput, ToolError> {
            self.executions
                .lock()
                .unwrap()
                .push(context.call_id.clone());
            self.started.send(context.call_id).unwrap();
            self.release.notified().await;
            Ok(ToolOutput::new(
                input.clone(),
                vec![ContextContent::Text {
                    text: Arc::from(input.content()),
                }],
            ))
        }
    }

    fn ticket(thread_id: &str, sequence: u64) -> ThreadWrite {
        let state = ThreadSnapshot {
            commit_sequence: sequence,
            ..Default::default()
        };
        ThreadWrite {
            effect: Arc::new(ThreadEffectBatch {
                thread_id: thread_id.to_owned(),
                sequence,
                committed_at: 1,
                ..Default::default()
            }),
            checkpoint: ThreadCheckpoint::capture_transfer(thread_id.to_owned(), sequence, state),
            output_claim: None,
        }
    }

    fn ticket_referencing_attachment(
        thread_id: &str,
        attachment: &crate::studio::AttachmentRecord,
    ) -> Result<ThreadWrite> {
        let mut write = ticket(thread_id, 1);
        let input = InputRecord {
            accepted_sequence: 1,
            delivery: Default::default(),
            input: ThreadInput {
                id: "attached-input".to_owned(),
                payload: OpaquePayload::new(
                    "pl.studio.prompt",
                    1,
                    serde_json::json!({
                        "text": "attached",
                        "presentation": "visible",
                        "attachments": [attachment],
                    })
                    .to_string(),
                )?,
                context: Vec::new(),
            },
            ordinal: 1,
            revision: 1,
            state: InputState::Pending,
        };
        write.checkpoint.state.inputs = Arc::from([input.clone()]);
        Arc::make_mut(&mut write.effect).inputs = Arc::from([InputChange::Accepted(input)]);
        Ok(write)
    }

    async fn sink(thread_id: &str) -> Result<(tempfile::TempDir, StudioStore, ThreadStorageSink)> {
        let temp = tempfile::tempdir()?;
        let store = StudioStore::open(temp.path().join("studio/v2/studio.sqlite")).await?;
        let sink =
            ThreadStorageSink::new(store.clone(), pl_protocol::Thread::placeholder(thread_id))
                .await?;
        spawn_live_projection(&sink);
        Ok((temp, store, sink))
    }

    /// The projection an observation worker would own: empty until the test hands it the admitted
    /// facts, which it folds and hands back to the reliable channel exactly like the production owner.
    fn live_projection() -> crate::studio::thread_projection::LiveProjection {
        crate::studio::thread_projection::LiveProjection::new()
    }

    /// Takes the admitted facts over exactly as the Thread's live projection owner does.
    ///
    /// The Studio runtime installs that owner during Thread assembly, and the history writer refuses
    /// to save a fact no projection holds. These tests exercise the writer on its own, so they drive
    /// the same production projection against the same reliable admission channel: one projection per
    /// commit, published shape unchanged, only the caller differs.
    fn spawn_live_projection(sink: &ThreadStorageSink) {
        let inner = sink.0.clone();
        tokio::spawn(async move {
            let mut projection = live_projection();
            let mut failed = std::collections::BTreeSet::new();
            // Wake on the channel's own status watch, never on the channel's `Notify`: the history
            // writer is the one task that waits on that `Notify`, and a second waiter would consume
            // the single wakeup permit a recovery notification hands out.
            let mut updates = inner.channel.subscribe();
            loop {
                if let Some(write) = inner.channel.next_unprojected() {
                    let sequence = write.effect.sequence;
                    if failed.contains(&sequence) {
                        tokio::time::sleep(Duration::from_millis(5)).await;
                        continue;
                    }
                    // A reliable-output repair can name an identity this bounded window released; the
                    // production observation owner reads that committed body back and folds it in, so
                    // the harness drives the same step instead of skipping a fact the writer must save.
                    if !write.effect.delivery_repairs.is_empty() {
                        let seeded = match inner.store.history(&inner.thread.id).await {
                            Ok(history) => crate::studio::thread_projection::seed_repaired_targets(
                                &mut projection,
                                &history,
                                &write.effect,
                            )
                            .await
                            .map_err(|error| error.to_string()),
                            Err(error) => Err(error.to_string()),
                        };
                        if let Err(error) = seeded {
                            failed.insert(sequence);
                            inner.channel.fail_projection(sequence, error);
                            continue;
                        }
                    }
                    match projection.project_committed(
                        &inner.chat,
                        &inner.thread,
                        &write.effect,
                        &write.checkpoint.state,
                    ) {
                        Ok(prepared) => {
                            inner.channel.prepare(prepared);
                        }
                        // The writer never projects, so failing here is what the running Studio sees
                        // from its observation worker: the fact is reported and the barrier fails
                        // closed with the projection's own reason.
                        Err(error) => {
                            failed.insert(sequence);
                            inner.channel.fail_projection(sequence, error.to_string());
                        }
                    }
                    continue;
                }
                if updates.changed().await.is_err() {
                    return;
                }
            }
        });
    }

    /// One ticket whose effect commits a visible prompt item, i.e. the shortest effect that projects
    /// a durable body the writer must confirm.
    fn visible_input_ticket(thread_id: &str, sequence: u64, input_id: &str) -> Result<ThreadWrite> {
        let mut write = ticket(thread_id, sequence);
        let input = InputRecord {
            accepted_sequence: sequence,
            delivery: Default::default(),
            input: ThreadInput {
                id: input_id.to_owned(),
                payload: OpaquePayload::new(
                    "pl.studio.prompt",
                    1,
                    serde_json::json!({"text":"hello","presentation":"visible","attachments":[]})
                        .to_string(),
                )?,
                context: vec![ContextContent::Text {
                    text: "hello".into(),
                }],
            },
            ordinal: sequence,
            revision: sequence,
            state: InputState::Consumed {
                turn_id: "visible-turn".into(),
                attempt_id: "attempt".into(),
            },
        };
        write.checkpoint.state.inputs = Arc::from([input.clone()]);
        Arc::make_mut(&mut write.effect).inputs = Arc::from([InputChange::Accepted(input)]);
        Ok(write)
    }

    /// The projected body of `visible_input_ticket`'s item as the live projection would publish it.
    fn visible_input_item(
        thread_id: &str,
        input_id: &str,
        revision: u64,
    ) -> pl_protocol::ThreadItem {
        pl_protocol::ThreadItem::new(
            input_id.to_owned(),
            thread_id.to_owned(),
            "visible-turn".to_owned(),
            revision,
            revision,
            1,
            1,
            pl_protocol::ThreadItemState::Text(pl_protocol::ThreadTextItem::new(
                pl_protocol::ThreadTextChannel::User,
                "hello".into(),
                Vec::new(),
                pl_protocol::ThreadContentLifecycle::completed(1),
            )),
        )
    }

    /// One streaming body revision of `item_id`, always at the same placement `order`.
    ///
    /// Two calls with the same `order` and increasing `revision` are exactly what a re-projected
    /// identity looks like: the same window slot whose content version advanced, never a new item.
    fn revised_preview(
        thread_id: &str,
        item_id: &str,
        order: u64,
        revision: u64,
        text: &str,
    ) -> Result<pl_core::chat::ChatItem> {
        chat_item(
            pl_protocol::ThreadItem::new(
                item_id.to_owned(),
                thread_id.to_owned(),
                "visible-turn".to_owned(),
                order,
                revision,
                1,
                1,
                pl_protocol::ThreadItemState::Text(pl_protocol::ThreadTextItem::new(
                    pl_protocol::ThreadTextChannel::User,
                    text.to_owned(),
                    Vec::new(),
                    pl_protocol::ThreadContentLifecycle::streaming(),
                )),
            ),
            false,
        )
    }

    /// The item the window currently shows for `item_id`.
    fn snapshot_item(view: &pl_core::chat::ChatView, item_id: &str) -> pl_core::chat::ChatItem {
        view.snapshot()
            .items
            .into_iter()
            .find(|item| item.item_id == item_id)
            .expect("the identity stays inside the window")
    }

    /// The writer never reads a committed body back to publish it.
    ///
    /// The Thread's live projection publishes the body into the shared chat session, and the writer
    /// commits exactly the batch that owner handed over: it confirms that identity and revision
    /// instead of reading a second body copy back from SQLite. The saved watermark therefore advances
    /// on the published content, and no second content cache exists to drift from it.
    #[tokio::test]
    async fn committed_identity_is_confirmed_into_the_shared_session_without_a_body_readback()
    -> Result<()> {
        let id = "writer-confirmation";
        let (_temp, store, sink) = sink(id).await?;
        let chat = store.chat_session(id).await?;
        let view = chat.open_chat(pl_core::chat::ChatFocus::Latest).await?;

        // The live projection publishes the body; the reliable handoff then commits that same batch.
        chat.publish_preview(chat_item(
            visible_input_item(id, "projected-first", 1),
            false,
        )?)?;
        sink.admit(id, visible_input_ticket(id, 1, "projected-first")?)?;
        tokio::time::timeout(Duration::from_secs(5), sink.flush(id, 1)).await??;
        let published = view
            .snapshot()
            .items
            .into_iter()
            .find(|item| item.item_id == "projected-first")
            .expect("the published body stays in the window");
        assert!(
            published.saved,
            "the committed identity confirms the body the projection already published"
        );
        Ok(())
    }

    /// A commit the Thread's projection owner has not handed over is never written.
    ///
    /// This is what makes "the owner released this effect from its window" imply "the projection
    /// published it": the durable watermark cannot move past a fact no projection took over, so a
    /// released commit is always one the product already has. The sink below is built without the
    /// test projection, so nothing takes the fact over until the test hands it in itself.
    #[tokio::test]
    async fn a_commit_without_a_projection_handoff_is_never_saved() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let store = StudioStore::open(temp.path().join("studio/v2/studio.sqlite")).await?;
        let sink = ThreadStorageSink::new(
            store.clone(),
            pl_protocol::Thread::placeholder("no-handoff"),
        )
        .await?;
        let write = ticket("no-handoff", 1);
        sink.admit("no-handoff", write.clone())?;
        assert!(
            tokio::time::timeout(Duration::from_millis(200), sink.flush("no-handoff", 1))
                .await
                .is_err(),
            "an unprojected commit must not reach a durable barrier"
        );
        let unprojected = sink.0.channel.subscribe().borrow().clone();
        assert_eq!(unprojected.committed_sequence, 0);
        assert_eq!(unprojected.queued_records, 1);
        assert_eq!(store.history("no-handoff").await?.watermark().await?, 0);

        let chat = store.chat_session("no-handoff").await?;
        let mut projection = live_projection();
        let prepared = projection.project_committed(
            &chat,
            &sink.0.thread,
            &write.effect,
            &write.checkpoint.state,
        )?;
        sink.0.channel.prepare(prepared);
        tokio::time::timeout(Duration::from_secs(5), sink.flush("no-handoff", 1)).await??;
        assert_eq!(store.history("no-handoff").await?.watermark().await?, 1);
        Ok(())
    }

    #[tokio::test]
    async fn hidden_consumed_input_keeps_turn_history_complete_after_body_pruning() -> Result<()> {
        let (_temp, store, sink) = sink("hidden-input").await?;
        let input_id = "interaction:confirmed:continuation";
        let turn_id = "turn-after-confirmation";
        let mut accepted = ticket("hidden-input", 1);
        let input = InputRecord {
            accepted_sequence: 1,
            delivery: Default::default(),
            input: ThreadInput {
                id: input_id.into(),
                payload: OpaquePayload::new(
                    "pl.studio.interaction-continuation",
                    1,
                    serde_json::json!({"interactionId":"confirmed","presentation":"hidden"})
                        .to_string(),
                )?,
                context: vec![ContextContent::Text {
                    text: "answer".into(),
                }],
            },
            ordinal: 1,
            revision: 2,
            state: InputState::Consumed {
                turn_id: turn_id.into(),
                attempt_id: "attempt-1".into(),
            },
        };
        accepted.checkpoint.state.inputs = Arc::from([input.clone()]);
        Arc::make_mut(&mut accepted.effect).inputs = Arc::from([InputChange::Accepted(input)]);
        sink.admit("hidden-input", accepted)?;
        tokio::time::timeout(Duration::from_secs(5), sink.flush("hidden-input", 1)).await??;
        let history = store.history("hidden-input").await?;
        assert!(history.existing_items([input_id.into()]).await?.is_empty());
        assert!(history.input_identity(input_id).await?.is_some());

        let mut later = ticket("hidden-input", 2);
        let turn = TurnRecord {
            elapsed_ms: None,
            input_id: Some(input_id.into()),
            turn_id: turn_id.into(),
            state: TurnState::Running,
            model_steps: 0,
        };
        later.checkpoint.state.turns = Arc::from([turn.clone()]);
        Arc::make_mut(&mut later.effect).turn = Some(turn);
        sink.admit("hidden-input", later)?;
        tokio::time::timeout(Duration::from_secs(5), sink.flush("hidden-input", 2)).await??;
        assert_eq!(history.watermark().await?, 2);
        assert_eq!(sink.0.channel.subscribe().borrow().fault, None);
        Ok(())
    }

    #[tokio::test]
    async fn missing_visible_input_item_still_blocks_history() -> Result<()> {
        // A Thread re-activated after its durable timeline lost a visible input body. The projection
        // that installs now has never folded that input — it is a fresh owner, exactly like a Thread
        // whose earlier incarnation committed the input — so the effect that still references it must
        // fail closed instead of committing a timeline with a hole.
        let temp = tempfile::tempdir()?;
        let store = StudioStore::open(temp.path().join("studio/v2/studio.sqlite")).await?;
        let sink = ThreadStorageSink::new(
            store.clone(),
            pl_protocol::Thread::placeholder("missing-visible"),
        )
        .await?;
        let mut accepted = ticket("missing-visible", 1);
        let input = InputRecord {
            accepted_sequence: 1,
            delivery: Default::default(),
            input: ThreadInput {
                id: "visible-input".into(),
                payload: OpaquePayload::new(
                    "pl.studio.prompt",
                    1,
                    serde_json::json!({"text":"hello","presentation":"visible","attachments":[]})
                        .to_string(),
                )?,
                context: vec![ContextContent::Text {
                    text: "hello".into(),
                }],
            },
            ordinal: 1,
            revision: 2,
            state: InputState::Consumed {
                turn_id: "visible-turn".into(),
                attempt_id: "attempt".into(),
            },
        };
        accepted.checkpoint.state.inputs = Arc::from([input.clone()]);
        Arc::make_mut(&mut accepted.effect).inputs = Arc::from([InputChange::Accepted(input)]);
        // The incarnation that committed the input projects and hands it over normally.
        let chat = store.chat_session("missing-visible").await?;
        sink.admit("missing-visible", accepted.clone())?;
        let mut owner = live_projection();
        let prepared = owner.project_committed(
            &chat,
            &sink.0.thread,
            &accepted.effect,
            &accepted.checkpoint.state,
        )?;
        sink.0.channel.prepare(prepared);
        tokio::time::timeout(Duration::from_secs(5), sink.flush("missing-visible", 1)).await??;
        let history = store.history("missing-visible").await?;
        let path = store
            .thread_storage_dir("missing-visible")
            .join("history.sqlite");
        let db = Database::connect(crate::studio::paths::sqlite_url(&path)).await?;
        db.execute_unprepared("DELETE FROM history_items WHERE item_id='visible-input'")
            .await?;

        // The owner this Thread gets on re-activation never folded the input, and the durable row
        // the projection would resolve it from is gone.
        spawn_live_projection(&sink);
        let mut later = ticket("missing-visible", 2);
        let turn = TurnRecord {
            elapsed_ms: None,
            input_id: Some("visible-input".into()),
            turn_id: "visible-turn".into(),
            state: TurnState::Running,
            model_steps: 0,
        };
        later.checkpoint.state.turns = Arc::from([turn.clone()]);
        Arc::make_mut(&mut later.effect).turn = Some(turn);
        sink.admit("missing-visible", later)?;
        let error = tokio::time::timeout(Duration::from_secs(5), sink.flush("missing-visible", 2))
            .await
            .context("a torn projection must fail the durable barrier instead of hanging")?
            .expect_err("a visible input without its item must fail closed");
        assert!(
            error
                .to_string()
                .contains("missing the committed input item")
        );
        assert_eq!(history.watermark().await?, 1);
        Ok(())
    }

    /// One visible, already-consumed input record owned by `turn_id`.
    ///
    /// This is exactly what core's consuming commit leaves behind: the input is `Consumed`, so it has
    /// a visible timeline item whose `turn_id` is the Turn it opened.
    fn consumed_visible_input(id: &str, turn_id: &str, sequence: u64) -> Result<InputRecord> {
        Ok(InputRecord {
            accepted_sequence: sequence,
            delivery: Default::default(),
            input: ThreadInput {
                id: id.to_owned(),
                payload: OpaquePayload::new(
                    "pl.studio.prompt",
                    1,
                    serde_json::json!({"text":"hello","presentation":"visible","attachments":[]})
                        .to_string(),
                )?,
                context: vec![ContextContent::Text {
                    text: "hello".into(),
                }],
            },
            ordinal: sequence,
            revision: sequence,
            state: InputState::Consumed {
                turn_id: turn_id.to_owned(),
                attempt_id: "attempt".to_owned(),
            },
        })
    }

    /// One Turn record naming `input_id` as the input that opened it.
    fn turn_record(turn_id: &str, input_id: &str, state: TurnState) -> TurnRecord {
        TurnRecord {
            elapsed_ms: None,
            input_id: Some(input_id.to_owned()),
            turn_id: turn_id.to_owned(),
            state,
            model_steps: 0,
        }
    }

    /// One provider output item with a single text part, as a model receipt reports it.
    ///
    /// The multi-item stress response is exactly a long list of these: one stable identity per
    /// provider item, all delivered inside one model step.
    fn provider_presentation_part(
        index: usize,
    ) -> pl_model::completion::CompletionPresentationItem {
        use pl_model::completion::{
            CompletionPresentationItem, CompletionPresentationItemKind, CompletionPresentationPart,
            CompletionPresentationPartKind,
        };
        CompletionPresentationItem {
            provider_item_id: format!("stress-item-{index}"),
            output_index: Some(index as u32),
            kind: CompletionPresentationItemKind::Text(pl_protocol::trace::TraceTextChannel::Final),
            parts: vec![CompletionPresentationPart {
                content_index: 0,
                provider_part_id: Some(format!("stress-part-{index}")),
                kind: CompletionPresentationPartKind::OutputText,
                text: format!("chunk-{index}"),
            }],
        }
    }

    /// The snapshot attempt and the matching effect update for one committed step of `parts` parts.
    ///
    /// The receipt is the model crate's own `pl.model.assistant` frame — the same payload a real
    /// adapter writes — so the projection itemizes it through its public receipt reader instead of a
    /// test-only shape.
    fn committed_parts_attempt(
        turn_id: &str,
        attempt_id: &str,
        parts: usize,
    ) -> Result<(
        pl_core::thread::RequestAttempt,
        pl_core::thread::journal::AttemptUpdate,
    )> {
        let receipt = pl_model::runtime::ModelResponseReceipt {
            binding: pl_model::runtime::ModelCallBinding {
                provider_instance_id: "fixture".into(),
                requested_model: "fixture-model".into(),
                adapter: pl_model::provider::ProviderAdapterKind::OpenAiCompatible,
                protocol: pl_model::provider::ProviderWireProtocol::ChatCompletions,
                isolation: "test".into(),
                purpose: "test".into(),
                context_window: None,
            },
            response: pl_model::completion::CompletionResponse {
                response_id: None,
                content: None,
                reasoning_content: None,
                tool_calls: Vec::new(),
                responses_context_items: Vec::new(),
                presentation_items: (0..parts).map(provider_presentation_part).collect(),
                orchestration: Default::default(),
                timing: None,
                accounting: Default::default(),
                model: "fixture-model".into(),
                model_observation: None,
            },
        };
        // The durable assistant frame is exactly this pair of receipt and tool bindings.
        #[derive(serde::Serialize)]
        #[serde(rename_all = "camelCase")]
        struct Frame<'a> {
            receipt: &'a pl_model::runtime::ModelResponseReceipt,
            bindings: Vec<ModelToolCall>,
        }
        let frame = OpaquePayload::new(
            "pl.model.assistant",
            2,
            serde_json::to_string(&Frame {
                receipt: &receipt,
                bindings: Vec::new(),
            })?,
        )?;
        let output = ModelStepOutput {
            attempt_id: attempt_id.to_owned(),
            base_context_revision: 0,
            content: vec![ContextContent::Opaque { payload: frame }],
            tool_calls: Vec::new(),
            private_context: None,
            usage: Default::default(),
        };
        let tools: Arc<[pl_core::model::ModelToolDeclaration]> =
            Arc::from(Vec::<pl_core::model::ModelToolDeclaration>::new());
        let attempt = pl_core::thread::RequestAttempt {
            request_metadata: None,
            tool_projection: None,
            turn_id: turn_id.to_owned(),
            attempt_id: attempt_id.to_owned(),
            retry_of: None,
            input: pl_core::context::ContextSnapshot::default(),
            tools: tools.clone(),
            outcome: pl_core::thread::AttemptOutcome::Committed(output.clone()),
            input_estimate: None,
        };
        let update = pl_core::thread::journal::AttemptUpdate {
            request_metadata: None,
            tool_projection: None,
            turn_id: turn_id.to_owned(),
            attempt_id: attempt_id.to_owned(),
            retry_of: None,
            input_revision: 0,
            tools,
            outcome: pl_core::thread::AttemptOutcome::Committed(output),
            input_estimate: None,
        };
        Ok((attempt, update))
    }

    /// Hands one admitted fact to the production live projection and to the writer, exactly as the
    /// Thread's observation worker does: admit, take the owed fact over, project it, hand the batch
    /// back. The writer saves only what this owner handed over, so a `flush` afterwards is the same
    /// durable barrier the product uses.
    fn drive_live_projection(
        projection: &mut crate::studio::thread_projection::LiveProjection,
        chat: &pl_core::chat::Session,
        sink: &ThreadStorageSink,
        thread_id: &str,
        write: ThreadWrite,
    ) -> Result<()> {
        sink.admit(thread_id, write)?;
        let write = sink
            .0
            .channel
            .next_unprojected()
            .context("the admitted fact is owed a projection")?;
        let (_, prepared) =
            projection.advance(chat, &sink.0.thread, &write.effect, &write.checkpoint.state)?;
        assert!(sink.0.channel.prepare(prepared));
        Ok(())
    }

    /// A live Turn survives a retained window smaller than the response it produced.
    ///
    /// The live projection is a bounded observation owner: it releases a Turn's oldest facts by
    /// ordinal once that Turn commits more identities than `LIVE_ITEM_WINDOW`. A Turn's own input is
    /// the identity core keeps referencing from *every* later effect of that Turn — above all its
    /// terminal one, whose checkpoint has already pruned the consumed body and carries only the
    /// minimal `turns[].input_id` — while it is also the oldest fact the Turn ever created. Numbering
    /// the window purely by ordinal therefore evicted exactly that identity, the terminal effect then
    /// resolved the pruned input as a hole and the durable barrier failed closed with
    /// `MissingDurableInput`: the multi-item stress fault. The window must bound only *optional*
    /// retained history; the minimal identity a live Turn still references is not optional.
    ///
    /// Keeping an identity without keeping its content version is not enough: the Turn's own item is
    /// re-projected by the same later effects, so a window that released it made the re-projection
    /// restart at revision 1, and the durable row rejected the new payload as a revision conflict.
    /// Identity and version have to survive together.
    ///
    /// This drives the production `advance` path over the same reliable handoff the observation
    /// worker uses, with no GUI subscriber: the projection publishes into the shared session and
    /// hands the batch to the writer exactly like the owner. The pressure is a real provider receipt
    /// with more presentation parts than the window holds, not a fabricated per-Turn input queue.
    #[tokio::test]
    async fn a_live_turn_input_survives_a_window_smaller_than_its_parts() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let store = StudioStore::open(temp.path().join("studio/v2/studio.sqlite")).await?;
        let sink = ThreadStorageSink::new(
            store.clone(),
            pl_protocol::Thread::placeholder("window-input"),
        )
        .await?;
        let thread_id = "window-input";
        let turn_id = "long-turn";
        let attempt_id = "long-attempt";
        let input_id = "input-under-the-window";
        let chat = store.chat_session(thread_id).await?;
        let mut projection = live_projection();

        // Commit 1: the Turn's own opening input. Every later commit's checkpoint has pruned the
        // consumed body from `state.inputs` and keeps only the minimal `turns[].input_id`.
        let input = consumed_visible_input(input_id, turn_id, 1)?;
        let mut opening = ticket(thread_id, 1);
        opening.checkpoint.state.inputs = Arc::from([input.clone()]);
        opening.checkpoint.state.turns =
            Arc::from([turn_record(turn_id, input_id, TurnState::Running)]);
        Arc::make_mut(&mut opening.effect).inputs = Arc::from([InputChange::Accepted(input)]);
        Arc::make_mut(&mut opening.effect).turn =
            Some(turn_record(turn_id, input_id, TurnState::Running));
        drive_live_projection(&mut projection, &chat, &sink, thread_id, opening)?;

        // Commit 2: one model step whose receipt itemizes more provider parts than the window holds.
        // That pushes the Turn's own oldest facts out of the ordinal window.
        let parts = crate::studio::thread_projection::LIVE_ITEM_WINDOW + 8;
        let (attempt, update) = committed_parts_attempt(turn_id, attempt_id, parts)?;
        let mut step = ticket(thread_id, 2);
        step.checkpoint.state.turns =
            Arc::from([turn_record(turn_id, input_id, TurnState::Running)]);
        step.checkpoint.state.attempts = Arc::from([attempt.clone()]);
        Arc::make_mut(&mut step.effect).attempt = Some(update);
        drive_live_projection(&mut projection, &chat, &sink, thread_id, step)?;

        // Commit 3: the Turn's terminal effect. Its checkpoint names the input without carrying its
        // body, and it re-projects the Turn item, so both the input identity and the Turn's content
        // version have to resolve from the projection's own memory.
        let finished = || {
            turn_record(
                turn_id,
                input_id,
                TurnState::Finished(TurnOutcome::Completed),
            )
        };
        let mut terminal = ticket(thread_id, 3);
        terminal.checkpoint.state.turns = Arc::from([finished()]);
        terminal.checkpoint.state.attempts = Arc::from([attempt]);
        Arc::make_mut(&mut terminal.effect).turn = Some(finished());
        drive_live_projection(&mut projection, &chat, &sink, thread_id, terminal)?;

        // Every fact is handed over, so the durable barrier reaches the terminal commit with no
        // fault, and the rows a long Turn produced are complete instead of a hole or a revision
        // conflict.
        tokio::time::timeout(Duration::from_secs(30), sink.flush(thread_id, 3)).await??;
        let history = store.history(thread_id).await?;
        assert_eq!(history.watermark().await?, 3);
        assert_eq!(sink.0.channel.subscribe().borrow().fault, None);
        assert!(history.input_identity(input_id).await?.is_some());
        let turn_item_id = crate::studio::thread_projection::order::turn_id(turn_id);
        let last_part_id = crate::studio::thread_projection::order::presentation_id(
            attempt_id,
            &format!("stress-item-{}", parts - 1),
            Some(crate::studio::thread_projection::order::PresentationPart::OutputText(0)),
        );
        let mut rows = history
            .existing_items([
                input_id.to_owned(),
                turn_item_id.clone(),
                last_part_id.clone(),
            ])
            .await?;
        let input_row = rows
            .remove(input_id)
            .expect("the live Turn's consumed input stays a complete durable row");
        match input_row.state() {
            pl_protocol::ThreadItemState::Text(text) => assert_eq!(text.text(), "hello"),
            other => panic!("the input row keeps its visible body, got {other:?}"),
        }
        let turn_row = rows
            .remove(&turn_item_id)
            .expect("the Turn row is re-projected onto its own content version");
        match turn_row.state() {
            pl_protocol::ThreadItemState::Turn(turn) => {
                assert!(matches!(turn.state(), pl_protocol::TurnState::Completed(_)))
            }
            other => panic!("the Turn row keeps its Turn state, got {other:?}"),
        }
        let part_row = rows
            .remove(&last_part_id)
            .expect("the receipt's provider parts are durable rows");
        match part_row.state() {
            pl_protocol::ThreadItemState::Text(text) => {
                assert_eq!(text.text(), format!("chunk-{}", parts - 1).as_str())
            }
            other => panic!("the provider part keeps its text, got {other:?}"),
        }
        Ok(())
    }

    #[tokio::test]
    async fn a_cold_owner_seeds_the_same_terminal_turn_the_live_projection_publishes() -> Result<()>
    {
        // A Thread re-activated after its last Turn finished has no live frame left for that fact:
        // the authoritative snapshot carries only the active Turn and `turnCompleted` is broadcast
        // once. The owner therefore seeds its retained last-Turn fact from durable history on the
        // install path. This drives that cold read over real committed rows and proves it agrees with
        // the live projection the frame would have used — same identity, same revision, same state —
        // while a Turn that is still running (or not yet durable) is never reported as finished.
        let temp = tempfile::tempdir()?;
        let store = StudioStore::open(temp.path().join("studio/v2/studio.sqlite")).await?;
        let sink =
            ThreadStorageSink::new(store.clone(), pl_protocol::Thread::placeholder("cold-turn"))
                .await?;
        let thread_id = "cold-turn";
        let turn_id = "terminal-turn";
        let input_id = "cold-input";
        let chat = store.chat_session(thread_id).await?;
        let mut projection = live_projection();

        // A Thread whose history was never written reads as "no finished Turn" instead of creating a
        // database, so this cold read can never make the live path wait on the writer.
        assert!(
            store
                .history("never-written")
                .await?
                .newest_terminal_turn()
                .await?
                .is_none()
        );

        // Commit 1: the Turn opens and is durable, but it is still running — the cold seed must not
        // mistake the newest Turn row for a finished one.
        let input = consumed_visible_input(input_id, turn_id, 1)?;
        let mut opening = ticket(thread_id, 1);
        opening.checkpoint.state.inputs = Arc::from([input.clone()]);
        opening.checkpoint.state.turns =
            Arc::from([turn_record(turn_id, input_id, TurnState::Running)]);
        Arc::make_mut(&mut opening.effect).inputs = Arc::from([InputChange::Accepted(input)]);
        Arc::make_mut(&mut opening.effect).turn =
            Some(turn_record(turn_id, input_id, TurnState::Running));
        drive_live_projection(&mut projection, &chat, &sink, thread_id, opening)?;
        tokio::time::timeout(Duration::from_secs(30), sink.flush(thread_id, 1)).await??;
        assert!(
            store
                .history(thread_id)
                .await?
                .newest_terminal_turn()
                .await?
                .is_none()
        );

        // Commit 2: the Turn finishes. The live projection of that same effect is the fact the frame
        // carried; the durable row the writer commits has to recover the identical Turn.
        let finished = || {
            turn_record(
                turn_id,
                input_id,
                TurnState::Finished(TurnOutcome::Completed),
            )
        };
        let mut terminal = ticket(thread_id, 2);
        terminal.checkpoint.state.turns = Arc::from([finished()]);
        Arc::make_mut(&mut terminal.effect).turn = Some(finished());
        let live = crate::studio::thread_projection::project_effect_terminal_turn(
            thread_id,
            &terminal.checkpoint.state,
            &terminal.effect,
        )
        .context("the live projection must carry the finished Turn")?;
        drive_live_projection(&mut projection, &chat, &sink, thread_id, terminal)?;
        tokio::time::timeout(Duration::from_secs(30), sink.flush(thread_id, 2)).await??;

        let cold = store
            .history(thread_id)
            .await?
            .newest_terminal_turn()
            .await?
            .context("the durable history must carry the finished Turn")?;
        assert_eq!(cold.id, live.id);
        assert_eq!(cold.revision, live.revision);
        assert_eq!(cold.state, live.state);
        Ok(())
    }

    #[tokio::test]
    async fn retained_budget_counts_the_bodies_the_live_projection_holds() {
        // A running Turn retains tool arguments, terminal results, attachment metadata and opaque
        // payloads. Charging only text and reasoning would leave the largest resident bodies outside
        // the Thread's reliable budget, so the estimate has to cover every category payload.
        let arguments = "a".repeat(4096);
        let result = "r".repeat(8192);
        let artifact = "z".repeat(2048);
        let raw_content = "p".repeat(1024);
        let failure = "f".repeat(256);
        let attachment = pl_protocol::ThreadAttachment {
            id: "artifact-attachment".to_owned(),
            modality: pl_protocol::AttachmentModality::File,
            media_type: "application/octet-stream".to_owned(),
            filename: Some("artifact.bin".to_owned()),
            width: None,
            height: None,
            byte_size: 64 * 1024 * 1024,
        };
        let tool = pl_protocol::ThreadItem::new(
            "tool-item".to_owned(),
            "retained-budget".to_owned(),
            "turn".to_owned(),
            1,
            1,
            1,
            1,
            pl_protocol::ThreadItemState::Tool(pl_protocol::ThreadToolItem::new(
                pl_protocol::ThreadToolInvocation::new(
                    "call".to_owned(),
                    "shell".to_owned(),
                    arguments.clone(),
                ),
                pl_protocol::ThreadToolState::Succeeded(pl_protocol::SucceededThreadTool::new(
                    1,
                    pl_protocol::ThreadToolOutput::new(
                        result.clone(),
                        vec![attachment.clone()],
                        vec![serde_json::json!({"stdout": artifact.clone()})],
                        Some(0),
                    ),
                )),
            )),
        );
        let raw = pl_protocol::ThreadItem::new(
            "raw-item".to_owned(),
            "retained-budget".to_owned(),
            "turn".to_owned(),
            2,
            1,
            1,
            1,
            pl_protocol::ThreadItemState::Raw(pl_protocol::ThreadRawItem {
                payloads: vec![pl_protocol::ThreadRawPayload {
                    format: "pl.model.receipt".to_owned(),
                    version: 1,
                    content: raw_content.clone(),
                }],
                notice: String::new(),
                recorded_at: 1,
            }),
        );
        let failed_text = pl_protocol::ThreadItem::new(
            "failed-text".to_owned(),
            "retained-budget".to_owned(),
            "turn".to_owned(),
            3,
            1,
            1,
            1,
            pl_protocol::ThreadItemState::Text(pl_protocol::ThreadTextItem::new(
                pl_protocol::ThreadTextChannel::Commentary,
                "visible".to_owned(),
                vec![attachment],
                pl_protocol::ThreadContentLifecycle::failed(1, failure.clone()),
            )),
        );
        let bytes = crate::studio::thread_projection::retained_bytes(&[tool, raw, failed_text]);
        let bodies =
            (arguments.len() + result.len() + artifact.len() + raw_content.len() + failure.len())
                as u64;
        assert!(
            bytes >= bodies,
            "retained budget {bytes} must cover the {bodies} resident body bytes"
        );
    }

    #[tokio::test]
    async fn projection_gauge_is_part_of_the_thread_budget() -> Result<()> {
        let (_temp, _store, sink) = sink("projection-budget").await?;
        // The projection owner keeps a Turn's retained bodies and its report accumulator resident
        // after their batch became durable. The Thread's budget therefore reads the owner's absolute
        // gauge, and releasing the owner's tables releases the budget with them.
        sink.0.channel.set_projection_bytes(8192);
        assert_eq!(sink.pressure("projection-budget").thread_bytes, 8192);
        assert_eq!(sink.0.channel.subscribe().borrow().queued_bytes, 8192);
        sink.0.channel.set_projection_bytes(0);
        assert_eq!(sink.pressure("projection-budget").thread_bytes, 0);
        assert_eq!(sink.0.channel.subscribe().borrow().queued_bytes, 0);
        Ok(())
    }

    #[tokio::test]
    async fn operation_output_reservation_returns_to_baseline_across_rounds() -> Result<()> {
        // A model/tool call reserves its live-output ceiling from the same process budget the
        // reliable save path uses and gives the *whole* ceiling back when it ends. Returning
        // `granted - accepted` would leave the accepted bytes charged forever while the batch that
        // carries the same fact charges them again, so the Thread's water level would creep up on
        // every successful call. This drives the reservation directly and asserts the gauge is back
        // at its baseline after each round, which a leak of any size would break.
        let (_temp, _store, sink) = sink("operation-budget").await?;
        let channel = &sink.0.channel;
        let baseline = channel.process_bytes.load(Ordering::Acquire);
        for round in 0..4_u64 {
            let operation = format!("task:call-{round}");
            let granted = channel
                .reserve_operation_output(&operation, 4096)
                .expect("the empty budget funds the reservation");
            assert_eq!(granted, 4096);
            assert_eq!(
                channel.process_bytes.load(Ordering::Acquire),
                baseline + 4096,
                "the whole ceiling is charged while the call is in flight"
            );
            channel.charge_operation_output(&operation, 1024)?;
            channel.release_operation_output(&operation);
            assert_eq!(
                channel.process_bytes.load(Ordering::Acquire),
                baseline,
                "a finished call returns its whole ceiling instead of leaking the accepted bytes"
            );
        }
        Ok(())
    }

    /// The whole reserve/complete/refuse/recover cycle of parallel in-flight operations.
    ///
    /// Four operations reserve the process budget down to its last byte, which is exactly the state
    /// where charging an admitted fact *next to* the reservation that produced it would need space
    /// that does not exist: every result would be refused, no result could be handed over, and the
    /// reservations would be held forever waiting for the queue they themselves block. The rule under
    /// test is the transfer instead — the bytes a reservation covers become the fact's own charge, so
    /// a fully reserved process still hands its facts over — together with the boundaries that stay
    /// real: an uncovered fact is refused with a typed fault instead of being admitted over budget,
    /// the refused fact is handed over unchanged once its Thread's ceiling comes back, and the
    /// process gauge returns to the baseline once everything is durable.
    #[tokio::test]
    async fn parallel_reservations_transfer_into_their_facts_without_deadlock() -> Result<()> {
        // No test projection here: nothing takes the admitted facts over until the test hands them
        // in itself, so the durable watermark cannot drain the queue while the reservations are
        // still charged and every step below is deterministic.
        let temp = tempfile::tempdir()?;
        let store = StudioStore::open(temp.path().join("studio/v2/studio.sqlite")).await?;
        let sink = ThreadStorageSink::new(
            store.clone(),
            pl_protocol::Thread::placeholder("output-transfer"),
        )
        .await?;
        let channel = &sink.0.channel;
        let baseline = channel.process_bytes.load(Ordering::Acquire);
        // A quarter of the process budget each: four in-flight calls leave no headroom at all.
        let reservation = MAX_HISTORY_PROCESS_BYTES / 4;
        let operations = (0..4)
            .map(|index| format!("task:parallel-{index}"))
            .collect::<Vec<_>>();
        for operation in &operations {
            let granted = channel
                .reserve_operation_output(operation, reservation)
                .expect("the empty budget funds a quarter of itself");
            assert_eq!(granted, reservation);
            channel.charge_operation_output(operation, 512)?;
        }
        assert_eq!(
            channel.process_bytes.load(Ordering::Acquire),
            MAX_HISTORY_PROCESS_BYTES,
            "the reservations alone fill the process budget"
        );
        assert!(
            channel
                .reserve_operation_output("task:one-too-many", reservation)
                .is_none(),
            "a call that the reliable budget cannot fund is backpressure, never a silent grant"
        );

        // A fact no reservation covers is refused while there is no headroom and no projection holds
        // a queued fact that could free any: typed backpressure, nothing dropped, ticket untouched.
        let uncovered = visible_input_ticket("output-transfer", 5, "input-uncovered")?;
        sink.admit("output-transfer", uncovered.clone())
            .expect_err("an unfunded fact cannot be admitted into a full budget");
        let refused = channel.subscribe().borrow().clone();
        assert_eq!(
            refused.fault,
            Some(pl_protocol::studio::HistoryFault::QueueFull)
        );

        // Every call returns: its result is admitted while the process budget is still full, which
        // only works because the reservation it was made under is transferred onto the fact.
        for (index, operation) in operations.iter().enumerate() {
            let sequence = index as u64 + 1;
            let mut write =
                visible_input_ticket("output-transfer", sequence, &format!("input-{index}"))?;
            write.output_claim = Some(operation.clone());
            sink.admit("output-transfer", write)
                .expect("the reservation funds the fact it was made for");
        }
        assert_eq!(
            channel.process_bytes.load(Ordering::Acquire),
            MAX_HISTORY_PROCESS_BYTES,
            "transferring a reservation onto a fact adds no second charge for the same output"
        );

        // Recovery: the first call ends, so its remaining ceiling comes back and the very same
        // ticket is admitted unchanged; the rest of the calls end too.
        channel.release_operation_output(&operations[0]);
        sink.admit("output-transfer", uncovered)?;
        for operation in &operations[1..] {
            channel.release_operation_output(operation);
        }

        // The Thread's own projection owner hands the admitted facts over in admission order, which
        // is what lets the writer save them; then the fixed fault generation is retried and every
        // fact is durable.
        let mut projection = live_projection();
        while let Some(write) = channel.next_unprojected() {
            let prepared = projection.project_committed(
                &sink.0.chat,
                &sink.0.thread,
                &write.effect,
                &write.checkpoint.state,
            )?;
            assert!(channel.prepare(prepared));
        }
        tokio::time::timeout(
            Duration::from_secs(10),
            store
                .thread_persistence()
                .retry_history("output-transfer", refused.fault_generation),
        )
        .await??;
        assert_eq!(
            store.history("output-transfer").await?.watermark().await?,
            5
        );
        assert_eq!(
            channel.process_bytes.load(Ordering::Acquire),
            baseline,
            "once every fact is durable and every call ended, the process gauge is back to baseline"
        );
        let drained = channel.subscribe().borrow().clone();
        assert!(drained.error.is_none() && drained.fault.is_none());
        Ok(())
    }

    #[tokio::test]
    async fn queue_pressure_keeps_the_original_ticket_until_retry_and_commit() -> Result<()> {
        let (_temp, store, sink) = sink("queue-pressure").await?;
        let write = ticket("queue-pressure", 1);
        sink.0
            .channel
            .process_bytes
            .store(MAX_HISTORY_PROCESS_BYTES, Ordering::Release);
        assert!(sink.admit("queue-pressure", write.clone()).is_err());
        let fault = sink.0.channel.subscribe().borrow().clone();
        assert_eq!(
            fault.fault,
            Some(pl_protocol::studio::HistoryFault::QueueFull)
        );
        assert_eq!(fault.admitted_sequence, 0);
        assert_eq!(fault.queued_records, 0);
        sink.0.channel.process_bytes.store(0, Ordering::Release);
        sink.admit("queue-pressure", write)?;
        assert!(sink.flush("queue-pressure", 1).await.is_err());
        tokio::time::timeout(
            Duration::from_secs(5),
            store
                .thread_persistence()
                .retry_history("queue-pressure", fault.fault_generation),
        )
        .await??;
        assert_eq!(store.history("queue-pressure").await?.watermark().await?, 1);
        let recovered = sink.0.channel.subscribe().borrow().clone();
        assert_eq!(recovered.queued_records, 0);
        assert_eq!(recovered.fault, None);
        Ok(())
    }

    #[tokio::test]
    async fn sqlite_rejection_retains_the_real_writer_queue_until_recovery() -> Result<()> {
        let (_temp, store, sink) = sink("sqlite-rejection").await?;
        sink.admit("sqlite-rejection", ticket("sqlite-rejection", 1))?;
        tokio::time::timeout(Duration::from_secs(5), sink.flush("sqlite-rejection", 1)).await??;
        let path = store
            .thread_storage_dir("sqlite-rejection")
            .join("history.sqlite");
        let db = Database::connect(crate::studio::paths::sqlite_url(&path)).await?;
        db.execute_unprepared(
            "CREATE TRIGGER reject_history BEFORE UPDATE OF applied_write_seq ON history_meta \
             BEGIN SELECT RAISE(ABORT, 'controlled writer rejection'); END",
        )
        .await?;
        let mut status = sink.0.channel.subscribe();
        sink.admit("sqlite-rejection", ticket("sqlite-rejection", 2))?;
        tokio::time::timeout(Duration::from_secs(5), async {
            while status.borrow_and_update().fault.is_none() {
                status.changed().await?;
            }
            Ok::<_, tokio::sync::watch::error::RecvError>(())
        })
        .await??;
        let failed = status.borrow().clone();
        assert_eq!(
            failed.fault,
            Some(pl_protocol::studio::HistoryFault::WriteFailed)
        );
        assert_eq!(failed.queued_records, 1);
        assert_eq!(
            store.history("sqlite-rejection").await?.watermark().await?,
            1
        );
        assert!(sink.flush("sqlite-rejection", 2).await.is_err());
        db.execute_unprepared("DROP TRIGGER reject_history").await?;
        tokio::time::timeout(
            Duration::from_secs(5),
            store
                .thread_persistence()
                .retry_history("sqlite-rejection", failed.fault_generation),
        )
        .await??;
        assert_eq!(
            store.history("sqlite-rejection").await?.watermark().await?,
            2
        );
        let recovered = sink.0.channel.subscribe().borrow().clone();
        assert_eq!(recovered.queued_records, 0);
        assert_eq!(recovered.fault, None);
        Ok(())
    }

    #[tokio::test]
    async fn concurrent_tool_results_survive_a_real_writer_failure_without_reexecution()
    -> Result<()> {
        let (_temp, store, sink) = sink("concurrent-results").await?;
        let requests = Arc::new(AtomicUsize::new(0));
        let thread = ThreadHandle::start(
            "concurrent-results".into(),
            DynModelSession::new(ConcurrentModel(requests.clone())),
        )?;
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
                            ConcurrentTool {
                                started: started.clone(),
                                release: release.clone(),
                                executions: executions.clone(),
                            },
                        )
                    })
                    .collect::<Result<Vec<_>, _>>()?,
            )
            .await?;
        thread
            .attach_storage(ColdStoreHandle::new(sink.clone()))
            .await?;
        let turn = TurnInput {
            turn_id: "both-running".into(),
            attempt_prefix: "both-running".into(),
            content: vec![ContextContent::Text {
                text: Arc::from("start"),
            }],
            max_model_steps: ModelStepLimit::Limited(1.try_into()?),
            cancellation: CancellationToken::new(),
        };
        let runner = tokio::spawn({
            let thread = thread.clone();
            async move { thread.run_turn(turn).await }
        });
        tokio::time::timeout(Duration::from_secs(10), async {
            assert_eq!(starts.recv().await.as_deref(), Some("first"));
            assert_eq!(starts.recv().await.as_deref(), Some("second"));
        })
        .await?;
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(5), runner)
                .await???
                .outcome,
            TurnOutcome::StepLimit
        );
        thread.flush().await?;
        let baseline = store
            .history("concurrent-results")
            .await?
            .watermark()
            .await?;
        let path = store
            .thread_storage_dir("concurrent-results")
            .join("history.sqlite");
        let db = Database::connect(crate::studio::paths::sqlite_url(&path)).await?;
        db.execute_unprepared(
            "CREATE TRIGGER reject_history BEFORE UPDATE OF applied_write_seq ON history_meta \
             BEGIN SELECT RAISE(ABORT, 'controlled concurrent rejection'); END",
        )
        .await?;

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
        .await?;
        let mut status = sink.0.channel.subscribe();
        tokio::time::timeout(Duration::from_secs(5), async {
            while status.borrow_and_update().fault.is_none() {
                status.changed().await?;
            }
            Ok::<_, tokio::sync::watch::error::RecvError>(())
        })
        .await??;
        let failed = status.borrow().clone();
        assert_eq!(
            failed.fault,
            Some(pl_protocol::studio::HistoryFault::WriteFailed)
        );
        assert!(failed.queued_records >= 2);
        assert_eq!(
            store
                .history("concurrent-results")
                .await?
                .watermark()
                .await?,
            baseline
        );
        assert_eq!(requests.load(Ordering::Relaxed), 1);
        assert_eq!(*executions.lock().unwrap(), ["first", "second"]);

        db.execute_unprepared("DROP TRIGGER reject_history").await?;
        tokio::time::timeout(
            Duration::from_secs(5),
            store
                .thread_persistence()
                .retry_history("concurrent-results", failed.fault_generation),
        )
        .await??;
        thread.flush().await?;
        let history = store.history("concurrent-results").await?;
        assert_eq!(
            history.watermark().await?,
            thread.snapshot().commit_sequence
        );
        for id in ["first", "second"] {
            let fact = history.tool_task(&format!("task:{id}")).await?.unwrap();
            assert_eq!(fact.task.call_id, id);
            assert_eq!(fact.delivery.unwrap().call_id, id);
        }
        assert_eq!(requests.load(Ordering::Relaxed), 1);
        assert_eq!(executions.lock().unwrap().len(), 2);
        thread.close().await?;
        Ok(())
    }

    #[tokio::test]
    async fn committed_batch_is_replayed_after_ack_is_lost() -> Result<()> {
        let (_temp, store, sink) = sink("lost-ack").await?;
        // The Thread's projection owner hands the batch over before the writer may write it. The test
        // drives that one write directly, under the step lock, so it can lose the acknowledgement while
        // the queue entry stays — exactly the retry the writer has to make idempotent.
        let write = ticket("lost-ack", 1);
        let chat = store.chat_session("lost-ack").await?;
        let mut projection = live_projection();
        let prepared = projection.project_committed(
            &chat,
            &sink.0.thread,
            &write.effect,
            &write.checkpoint.state,
        )?;
        let locked = sink.0.channel.step_lock.lock().await;
        sink.admit("lost-ack", write.clone())?;
        persist_effect(&sink.0, 1, &write, &prepared).await?;
        assert_eq!(store.history("lost-ack").await?.watermark().await?, 1);
        assert_eq!(sink.0.channel.subscribe().borrow().queued_records, 1);
        sink.0.channel.mark_unavailable();
        let unavailable = sink.0.channel.subscribe().borrow().clone();
        assert_eq!(
            unavailable.fault,
            Some(pl_protocol::studio::HistoryFault::WriterUnavailable)
        );
        assert!(sink.flush("lost-ack", 1).await.is_err());
        drop(locked);
        tokio::time::timeout(
            Duration::from_secs(5),
            store
                .thread_persistence()
                .retry_history("lost-ack", unavailable.fault_generation),
        )
        .await??;
        assert_eq!(store.history("lost-ack").await?.watermark().await?, 1);
        let status = sink.0.channel.subscribe().borrow().clone();
        assert_eq!(status.queued_records, 0);
        assert_eq!(status.fault, None);
        let mut persistence = store.thread_persistence().subscribe();
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let row = store
                    .thread_persistence()
                    .queue_snapshot()
                    .threads
                    .into_iter()
                    .find(|row| row.thread_id == "lost-ack")
                    .context("missing live Thread persistence row")?;
                if row.calls_admitted_sequence == Some(1) && row.calls_durable_sequence == Some(1) {
                    break Ok::<_, anyhow::Error>(());
                }
                persistence.changed().await?;
            }
        })
        .await??;
        Ok(())
    }

    #[tokio::test]
    async fn failed_checkpoint_publication_keeps_the_history_fence() -> Result<()> {
        let (_temp, store, sink) = sink("checkpoint-failure").await?;
        let state_path = store
            .thread_storage_dir("checkpoint-failure")
            .join("state.toml");
        tokio::fs::create_dir_all(&state_path).await?;
        let mut status = sink.0.channel.subscribe();
        sink.admit("checkpoint-failure", ticket("checkpoint-failure", 1))?;
        assert!(
            tokio::time::timeout(Duration::from_secs(5), sink.flush("checkpoint-failure", 1))
                .await?
                .is_err()
        );
        tokio::time::timeout(Duration::from_secs(5), async {
            while status.borrow_and_update().fault.is_none() {
                status.changed().await?;
            }
            Ok::<_, tokio::sync::watch::error::RecvError>(())
        })
        .await??;
        let failed = status.borrow().clone();
        assert_eq!(
            failed.fault,
            Some(pl_protocol::studio::HistoryFault::CheckpointFailed)
        );
        assert_eq!(failed.committed_sequence, 1);
        assert_eq!(
            store
                .history("checkpoint-failure")
                .await?
                .watermark()
                .await?,
            1
        );
        tokio::fs::remove_dir(&state_path).await?;
        tokio::time::timeout(
            Duration::from_secs(5),
            store
                .thread_persistence()
                .retry_history("checkpoint-failure", failed.fault_generation),
        )
        .await??;
        assert!(tokio::fs::metadata(&state_path).await?.is_file());
        assert_eq!(
            store
                .history("checkpoint-failure")
                .await?
                .watermark()
                .await?,
            1
        );
        assert_eq!(sink.0.channel.subscribe().borrow().fault, None);
        Ok(())
    }

    #[tokio::test]
    async fn missing_blob_blocks_checkpoint_until_the_same_attachment_is_durable() -> Result<()> {
        let (_temp, store, sink) = sink("blob-fence").await?;
        let blob = store.thread_blobs_dir("blob-fence").join("restored-blob");
        let attachment = crate::studio::AttachmentRecord {
            id: "blob-1".to_owned(),
            thread_id: "blob-fence".to_owned(),
            modality: pl_protocol::studio::StudioAttachmentModality::File,
            media_type: "text/plain".to_owned(),
            filename: None,
            storage_path: blob.to_string_lossy().into_owned(),
            byte_size: 8,
            content_sha256: "0123456789abcdef".to_owned(),
            width: None,
            height: None,
            created_at: 1,
        };
        store.record_attachments(vec![attachment.clone()]).await?;
        let mut status = sink.0.channel.subscribe();
        sink.admit(
            "blob-fence",
            ticket_referencing_attachment("blob-fence", &attachment)?,
        )?;
        tokio::time::timeout(Duration::from_secs(5), async {
            while status.borrow_and_update().fault.is_none() {
                status.changed().await?;
            }
            Ok::<_, tokio::sync::watch::error::RecvError>(())
        })
        .await??;
        let failed = status.borrow().clone();
        assert_eq!(
            failed.fault,
            Some(pl_protocol::studio::HistoryFault::BlobFailed)
        );
        assert_eq!(failed.committed_sequence, 1);
        assert!(sink.flush("blob-fence", 1).await.is_err());
        tokio::fs::create_dir_all(blob.parent().expect("blob has a parent")).await?;
        tokio::fs::write(&blob, b"restored").await?;
        tokio::time::timeout(
            Duration::from_secs(5),
            store
                .thread_persistence()
                .retry_history("blob-fence", failed.fault_generation),
        )
        .await??;
        assert_eq!(store.history("blob-fence").await?.watermark().await?, 1);
        assert!(
            tokio::fs::try_exists(store.thread_storage_dir("blob-fence").join("state.toml"))
                .await?
        );
        Ok(())
    }

    #[tokio::test]
    async fn closing_the_chat_and_sink_does_not_release_pending_history() -> Result<()> {
        let (_temp, store, sink) = sink("closed-view").await?;
        sink.admit("closed-view", ticket("closed-view", 1))?;
        tokio::time::timeout(Duration::from_secs(5), sink.flush("closed-view", 1)).await??;
        let chat = store.chat_session("closed-view").await?;
        let view = chat.open_chat(pl_core::chat::ChatFocus::Latest).await?;
        let path = store
            .thread_storage_dir("closed-view")
            .join("history.sqlite");
        let db = Database::connect(crate::studio::paths::sqlite_url(&path)).await?;
        db.execute_unprepared(
            "CREATE TRIGGER reject_history BEFORE UPDATE OF applied_write_seq ON history_meta \
             BEGIN SELECT RAISE(ABORT, 'controlled closed-view rejection'); END",
        )
        .await?;
        let channel = store.thread_persistence().history_channel("closed-view");
        let mut updates = channel.subscribe();
        sink.admit("closed-view", ticket("closed-view", 2))?;
        tokio::time::timeout(Duration::from_secs(5), async {
            while updates.borrow_and_update().fault.is_none() {
                updates.changed().await?;
            }
            Ok::<_, tokio::sync::watch::error::RecvError>(())
        })
        .await??;
        let generation = updates.borrow().fault_generation;
        drop(view);
        drop(chat);
        drop(sink);
        assert_eq!(channel.subscribe().borrow().queued_records, 1);
        assert_eq!(store.history("closed-view").await?.watermark().await?, 1);
        db.execute_unprepared("DROP TRIGGER reject_history").await?;
        tokio::time::timeout(
            Duration::from_secs(5),
            store
                .thread_persistence()
                .retry_history("closed-view", generation),
        )
        .await??;
        assert_eq!(store.history("closed-view").await?.watermark().await?, 2);
        assert_eq!(channel.subscribe().borrow().queued_records, 0);
        Ok(())
    }

    #[tokio::test]
    async fn stopped_statistics_consumer_never_blocks_history_or_checkpoint() -> Result<()> {
        let (_temp, store, sink) = sink("statistics-gap").await?;
        store.calls().stop_best_effort();
        assert!(
            !store
                .calls()
                .try_admit_effect(&ticket("statistics-gap", 1).effect)
        );
        store
            .thread_persistence()
            .report_calls(store.calls().metrics());
        assert!(store.thread_persistence().queue_snapshot().statistics_gap);
        sink.admit("statistics-gap", ticket("statistics-gap", 1))?;
        tokio::time::timeout(Duration::from_secs(5), sink.flush("statistics-gap", 1)).await??;
        assert_eq!(store.history("statistics-gap").await?.watermark().await?, 1);
        assert_eq!(sink.0.channel.subscribe().borrow().fault, None);
        assert!(store.thread_persistence().queue_snapshot().statistics_gap);
        Ok(())
    }

    #[tokio::test]
    async fn crashed_statistics_consumer_does_not_block_history_and_restarts() -> Result<()> {
        let (_temp, store, sink) = sink("statistics-crash").await?;
        let calls = store.calls();
        calls.panic_on_next_mutation();
        assert!(calls.try_admit_effect(&ticket("statistics-crash", 1).effect));
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if calls
                    .last_error()
                    .is_some_and(|message| message.contains("terminated unexpectedly"))
                {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .context("call writer did not report the injected worker exit")?;
        assert!(calls.statistics_gap());
        let dropped_ticket = calls.admitted_ticket();
        assert!(calls.try_admit_effect(&ticket("statistics-crash", 2).effect));
        let resumed_ticket = calls.admitted_ticket();
        assert!(resumed_ticket > dropped_ticket);

        sink.admit("statistics-crash", ticket("statistics-crash", 1))?;
        tokio::time::timeout(Duration::from_secs(5), sink.flush("statistics-crash", 1))
            .await
            .context("history flush stalled after the call writer exit")??;
        assert_eq!(
            store.history("statistics-crash").await?.watermark().await?,
            1
        );
        assert_eq!(sink.0.channel.subscribe().borrow().fault, None);
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if calls.durable_ticket() >= resumed_ticket {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .context("call writer did not advance the resumed ticket")?;
        assert!(calls.statistics_gap());
        Ok(())
    }

    #[tokio::test]
    async fn stale_fault_generation_cannot_resume_a_new_queue_failure() -> Result<()> {
        let (_temp, store, sink) = sink("fault-generation").await?;
        for sequence in [1, 2] {
            sink.0
                .channel
                .process_bytes
                .store(MAX_HISTORY_PROCESS_BYTES, Ordering::Release);
            let write = ticket("fault-generation", sequence);
            assert!(sink.admit("fault-generation", write.clone()).is_err());
            let failed = sink.0.channel.subscribe().borrow().clone();
            assert_eq!(failed.fault_generation, sequence);
            if sequence == 2 {
                assert!(sink.0.channel.retry(1).is_err());
                assert_eq!(sink.0.channel.subscribe().borrow().fault_generation, 2);
            }
            sink.0.channel.process_bytes.store(0, Ordering::Release);
            sink.admit("fault-generation", write)?;
            tokio::time::timeout(
                Duration::from_secs(5),
                store
                    .thread_persistence()
                    .retry_history("fault-generation", sequence),
            )
            .await??;
        }
        assert_eq!(
            store.history("fault-generation").await?.watermark().await?,
            2
        );
        Ok(())
    }

    /// The continue entry the UI reads is exactly the Thread owner's readiness, with nothing else
    /// able to light it.
    ///
    /// The writer reports watermarks and the typed fault generation, but no readiness of its own:
    /// the owner can still owe a newer fault while a writer has long since written an older retry, so
    /// the projection takes the owner's single verdict — byte for byte — instead of combining
    /// anything a writer reports. `resume_required` stays a separate fact: readiness never releases
    /// the latch by itself.
    #[test]
    fn can_resume_is_exactly_the_owners_readiness() {
        use crate::studio::thread_projection::storage_state;

        let mut owner = ThreadSnapshot::default();
        owner.persistence.resume_required = true;
        owner.persistence.fault_generation = 2;

        // A writer that is already past this generation's fault — it reports the same generation with
        // no error and fully admitted watermarks — still cannot offer a continue the owner refuses.
        let writer = pl_protocol::ThreadPersistenceSnapshot {
            fault_generation: 2,
            history_admitted_sequence: Some(9),
            history_durable_sequence: Some(9),
            last_error: None,
            ..pl_protocol::ThreadPersistenceSnapshot::default()
        };
        let storage = storage_state(&owner, &writer);
        assert!(storage.resume_required);
        assert!(
            !storage.can_resume,
            "only the owner's own generation-matched readiness may enable the continue"
        );
        assert_eq!(
            storage.accepted_sequence,
            Some(9),
            "the writer's own watermarks still reach the UI as facts"
        );

        // The owner's verified readiness is the fact, whatever else the writer reports.
        owner.persistence.resume_ready = true;
        assert!(storage_state(&owner, &writer).can_resume);
    }

    /// One `(identity, content version)` pair is one canonical payload, even when the same fact is
    /// projected a second time.
    ///
    /// The projection owner folds an admitted fact once, but the same effect can legitimately reach
    /// the projection again — a commit the owner's snapshot already carried, a durable step retried
    /// after a real writer failure, or a re-subscription. Every pass must deliver the *same bytes*
    /// for a `(item_id, revision)` pair: a pass that restamped the projection clock would hand the
    /// durable writer two different payloads under one version, which the store's own revision fence
    /// rejects as a real save fault instead of the idempotent rewrite it is. This pins that a repeat
    /// projection reuses the version, placement and timestamps of the payload it already delivered.
    #[tokio::test]
    async fn reprojecting_an_unchanged_effect_keeps_one_canonical_payload() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let store = StudioStore::open(temp.path().join("studio/v2/studio.sqlite")).await?;
        let sink = ThreadStorageSink::new(
            store.clone(),
            pl_protocol::Thread::placeholder("stable-payload"),
        )
        .await?;
        let chat = store.chat_session("stable-payload").await?;
        // The commit sequence is deliberately far above the item's own content version, so a sequence
        // leaking into the version shows up as a value change instead of a coincidence.
        let write = visible_input_ticket("stable-payload", 9, "stable-input")?;
        let mut projection = live_projection();
        let first = projection.project_committed(
            &chat,
            &sink.0.thread,
            &write.effect,
            &write.checkpoint.state,
        )?;
        let second = projection.project_committed(
            &chat,
            &sink.0.thread,
            &write.effect,
            &write.checkpoint.state,
        )?;
        assert_eq!(
            first.items[0].revision, 1,
            "a first delivery takes the projection's own content version, not the commit sequence"
        );
        assert_eq!(
            serde_json::to_vec(&first.items)?,
            serde_json::to_vec(&second.items)?,
            "a repeat projection of an unchanged fact is byte-identical, so one version never names two payloads"
        );
        Ok(())
    }

    /// The durable row carries the item's own **content version**, never the effect sequence that
    /// committed it, and a cold read of the same history file observes the same value.
    ///
    /// A save receipt is a fact about one exact `(identity, content version)` pair. Substituting the
    /// commit sequence for the version would let an acknowledgement vouch for a payload the store
    /// never wrote, so both ends are pinned here: the shared session confirms exactly the revision
    /// the projection published, and a separate history handle — the cold path, which decodes the
    /// durable row itself instead of reading live projection state — returns that same revision.
    #[tokio::test]
    async fn the_durable_row_keeps_the_items_own_content_version_across_a_cold_read() -> Result<()>
    {
        let temp = tempfile::tempdir()?;
        let store = StudioStore::open(temp.path().join("studio/v2/studio.sqlite")).await?;
        let sink = ThreadStorageSink::new(
            store.clone(),
            pl_protocol::Thread::placeholder("revision-fence"),
        )
        .await?;
        let chat = store.chat_session("revision-fence").await?;
        let view = chat.open_chat(pl_core::chat::ChatFocus::Latest).await?;
        // The commit sequence is deliberately far above the item's first content version: the
        // projection decides the content version, and the writer has to store that exact value.
        let write = visible_input_ticket("revision-fence", 9, "revision-input")?;
        // No test projection task here: the test hands the one fact over itself, exactly like the
        // Thread's observation worker, so the publish/hand-off/confirm order is deterministic.
        let mut projection = live_projection();
        let prepared = projection.project_committed(
            &chat,
            &sink.0.thread,
            &write.effect,
            &write.checkpoint.state,
        )?;
        // The projection publishes before it hands the same immutable batch to the reliable queue.
        for item in &prepared.items {
            chat.publish(chat_item(item.clone(), false)?)?;
        }
        sink.admit("revision-fence", write)?;
        sink.0.channel.prepare(prepared);
        tokio::time::timeout(Duration::from_secs(5), sink.flush("revision-fence", 9)).await??;

        let published = snapshot_item(&view, "revision-input");
        // The real allocation entry point is pinned, not just "different from the commit sequence":
        // a fresh projection's first delivery of a new identity is that identity's content version 1.
        assert_eq!(
            published.revision, 1,
            "a first delivery takes the projection's own content version, not the commit sequence"
        );
        assert_ne!(
            published.revision, 9,
            "a content version is the item's own fact, never the commit sequence"
        );
        assert!(
            published.saved,
            "the writer confirms exactly the revision the projection published"
        );

        let path = store
            .thread_storage_dir("revision-fence")
            .join("history.sqlite");
        let cold =
            crate::studio::storage::history::HistoryStore::open(&path, "revision-fence").await?;
        let durable = cold.items_for_turn("visible-turn").await?;
        assert_eq!(durable.len(), 1);
        assert_eq!(durable[0].id, "revision-input");
        assert_eq!(
            durable[0].revision, published.revision,
            "the persisted row carries the same content version the live projection confirmed"
        );
        Ok(())
    }

    /// An acknowledgement for a revision the identity already left behind confirms nothing.
    ///
    /// The save watermark is a fact about one exact payload. A receipt that names an older content
    /// version must not mark the newer body durable: the newer payload stays unsaved until its own
    /// revision is acknowledged, so a late receipt can never stand in for a write that never
    /// happened.
    #[tokio::test]
    async fn an_acknowledgement_of_an_older_revision_never_confirms_the_newer_payload() -> Result<()>
    {
        let temp = tempfile::tempdir()?;
        let store = StudioStore::open(temp.path().join("studio/v2/studio.sqlite")).await?;
        let chat = store.chat_session("stale-ack").await?;
        let view = chat.open_chat(pl_core::chat::ChatFocus::Latest).await?;

        chat.publish_preview(revised_preview("stale-ack", "revised", 1, 1, "first body")?)?;
        chat.publish_preview(revised_preview(
            "stale-ack",
            "revised",
            1,
            2,
            "second body",
        )?)?;
        assert_eq!(snapshot_item(&view, "revised").revision, 2);

        // The stale receipt names revision 1 while the identity already advanced to revision 2.
        chat.confirm_saved("revised", 1);
        assert!(
            !snapshot_item(&view, "revised").saved,
            "an acknowledgement of a revision the identity left behind confirms nothing"
        );

        chat.confirm_saved("revised", 2);
        assert!(
            snapshot_item(&view, "revised").saved,
            "only the exact published revision becomes the saved watermark"
        );
        Ok(())
    }

    /// The archive duty a real command capture owes after its append failed.
    ///
    /// It re-materializes the accepted chunk through the real local backend and then archives the
    /// fragment with the real content-addressed store, so the reference the retry names is a genuine
    /// durable resource and not a stand-in.
    #[derive(Debug)]
    struct CaptureRepairObligation {
        backend: pl_tool::command::LocalCommandBackend,
        archive: crate::resource_store::FileResourceStore,
        capture_file: std::path::PathBuf,
        call_id: String,
        committed_len: u64,
        pending: Vec<u8>,
        retries: Arc<AtomicUsize>,
    }

    impl pl_core::thread::cold::OutputRetryObligation for CaptureRepairObligation {
        fn identity(&self) -> String {
            self.capture_file.display().to_string()
        }
        fn retry(&self) -> pl_core::thread::cold::OutputRetryFuture<'_> {
            Box::pin(async move {
                self.retries.fetch_add(1, Ordering::SeqCst);
                // The real backend truncates the fragment back to its committed offset and re-appends
                // exactly the accepted chunk with the same framing a successful append would produce,
                // so a repeated retry reproduces the same bytes instead of appending them twice.
                pl_tool::command::CommandBackend::repair_output_chunk(
                    &self.backend,
                    &self.capture_file,
                    pl_tool::command::CommandCaptureStream::Stdout,
                    self.committed_len,
                    &self.pending,
                )
                .await
                .map_err(|error| pl_core::thread::cold::ColdStoreError {
                    source: Box::new(std::io::Error::other(error.to_string())),
                })?;
                let reference = self
                    .archive
                    .retain_command_capture(&self.capture_file)
                    .await
                    .map_err(|error| pl_core::thread::cold::ColdStoreError {
                        source: Box::new(error),
                    })?;
                Ok(pl_core::thread::cold::OutputRetryOutcome::StoredWithRepair(
                    pl_core::thread::cold::OutputRepair {
                        call_id: self.call_id.clone(),
                        reference,
                    },
                ))
            })
        }
        fn received_bytes(&self) -> u64 {
            self.committed_len.saturating_add(self.pending.len() as u64)
        }
        fn location(&self) -> String {
            self.capture_file.display().to_string()
        }
        fn kind(&self) -> pl_core::thread::cold::StorageFaultKind {
            pl_core::thread::cold::StorageFaultKind::WriteFailed
        }
    }

    /// A command whose capture append really failed, so its archive still owes the accepted bytes.
    #[derive(Debug)]
    struct FailedAppendCaptureTool {
        backend: pl_tool::command::LocalCommandBackend,
        archive: crate::resource_store::FileResourceStore,
        capture_file: std::path::PathBuf,
        committed_len: u64,
        pending: Vec<u8>,
        retries: Arc<AtomicUsize>,
        created: Arc<Mutex<Vec<Arc<CaptureRepairObligation>>>>,
    }

    impl Tool for FailedAppendCaptureTool {
        async fn execute(
            &self,
            input: OpaquePayload,
            context: CallContext,
        ) -> Result<ToolOutput, ToolError> {
            let obligation = Arc::new(CaptureRepairObligation {
                backend: self.backend.clone(),
                archive: self.archive.clone(),
                capture_file: self.capture_file.clone(),
                call_id: context.call_id.clone(),
                committed_len: self.committed_len,
                pending: self.pending.clone(),
                retries: self.retries.clone(),
            });
            self.created.lock().unwrap().push(obligation.clone());
            let source = pl_core::thread::cold::ColdStoreError {
                source: Box::new(std::io::Error::other("capture append failed")),
            };
            let fault = pl_core::thread::cold::OutputStorageFault::new(
                pl_core::thread::cold::StorageFaultKind::WriteFailed,
                Arc::new(source),
            )
            .with_obligation(obligation);
            Err(ToolError::new(fault).with_output(ToolOutput::new(
                input.clone(),
                vec![ContextContent::Text {
                    text: Arc::from("accepted so far"),
                }],
            )))
        }
    }

    #[derive(Debug)]
    struct CaptureModel;

    impl ModelSession for CaptureModel {
        async fn prepare(
            &mut self,
            request: ModelRequest,
        ) -> Result<PreparedModelCall, ModelError> {
            Ok(PreparedModelCall::new(async move {
                Ok(ModelStepOutput {
                    attempt_id: request.attempt_id,
                    base_context_revision: request.context.revision,
                    content: vec![ContextContent::Text {
                        text: Arc::from("calling capture"),
                    }],
                    tool_calls: vec![ModelToolCall {
                        call_id: "capture-1".to_owned(),
                        tool_id: "capture".to_owned(),
                        arguments: OpaquePayload::text("capture"),
                    }],
                    private_context: None,
                    usage: Default::default(),
                })
            }))
        }

        async fn close(&mut self) -> Result<(), ModelError> {
            Ok(())
        }
    }

    /// The owner facts one stuck wait is diagnosed from, so a timeout names a stage instead of a bare
    /// deadline and a real owner bug is not masked by a longer one.
    fn stage_facts(snapshot: &ThreadSnapshot) -> String {
        format!(
            "fault={:?} generation={} phase={:?} obligations={} resume_required={} resume_ready={} \
             commit={} durable={}",
            snapshot.persistence.fault,
            snapshot.persistence.fault_generation,
            snapshot.persistence.execution_phase,
            snapshot.persistence.output_obligations.len(),
            snapshot.persistence.resume_required,
            snapshot.persistence.resume_ready,
            snapshot.commit_sequence,
            snapshot.persistence.durable_sequence,
        )
    }

    /// Waits until the authoritative snapshot satisfies `predicate`, tagging `stage` on timeout.
    ///
    /// The subscription yields the current snapshot first, so a boundary that already landed is observed
    /// without waiting for an unrelated new frame — the exact shape of the earlier hang — and the timeout
    /// reports the stage and the last owner facts so the production cause is located, not hidden.
    async fn await_stage(
        thread: &ThreadHandle,
        stage: &str,
        predicate: impl Fn(&ThreadSnapshot) -> bool,
    ) -> Result<ThreadSnapshot> {
        let mut snapshots = thread.subscribe();
        let reached = tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                let snapshot = snapshots.next().await.expect("thread stays observable");
                if predicate(&snapshot) {
                    return snapshot;
                }
            }
        })
        .await;
        reached.map_err(|_| {
            anyhow::anyhow!(
                "stage `{stage}` was not reached: {}",
                stage_facts(&thread.snapshot())
            )
        })
    }

    /// The reference a repaired capture stored reaches the durable tool result, not just memory.
    ///
    /// The append failed after the fragment already held a partial write, so the retry must repair
    /// the fragment through the real local backend before archiving it: the archived bytes are the
    /// accepted content, verbatim and without duplication. The writer must then record that reference
    /// on the tool identity it was committed under — on the projected item *and* on the delivery
    /// history keeps — so the UI and a cold reopen can both locate the complete output, and the pause
    /// must stay closed until the archive really landed.
    #[tokio::test]
    async fn a_repaired_capture_reference_reaches_the_durable_tool_result() -> Result<()> {
        use pl_core::context::ResourceReader;
        use pl_core::thread::cold::OutputRetryObligation as _;

        let (temp, store, sink) = sink("archive-repair").await?;
        let resources_root = temp.path().join("resources");
        std::fs::create_dir_all(&resources_root)?;
        let archive = crate::resource_store::FileResourceStore::new(resources_root);
        let backend = pl_tool::command::LocalCommandBackend::new(temp.path().to_path_buf());

        let capture_file = temp.path().join("capture.fragment");
        let committed = b"accepted line one\n".to_vec();
        let pending = b"accepted line two".to_vec();
        // The failed append left the fragment short of the bytes already accepted and published live:
        // a few bytes of the pending chunk were written, without their framing and without the rest.
        let mut partial = committed.clone();
        partial.extend_from_slice(&pending[..4]);
        std::fs::write(&capture_file, &partial)?;
        let mut expected = committed.clone();
        expected.extend_from_slice(b"=== STDOUT ===\n");
        expected.extend_from_slice(&pending);
        expected.push(b'\n');

        let retries = Arc::new(AtomicUsize::new(0));
        let created: Arc<Mutex<Vec<Arc<CaptureRepairObligation>>>> =
            Arc::new(Mutex::new(Vec::new()));
        let thread =
            ThreadHandle::start("archive-repair".into(), DynModelSession::new(CaptureModel))?;
        thread
            .register_tools(vec![Registration::new(
                "capture".into(),
                OpaquePayload::text("Tool capture"),
                FailedAppendCaptureTool {
                    backend,
                    archive: archive.clone(),
                    capture_file: capture_file.clone(),
                    committed_len: committed.len() as u64,
                    pending: pending.clone(),
                    retries: retries.clone(),
                    created: created.clone(),
                },
            )?])
            .await?;
        thread
            .attach_storage(ColdStoreHandle::new(sink.clone()))
            .await?;

        let runner = tokio::spawn({
            let thread = thread.clone();
            async move {
                thread
                    .run_turn(TurnInput {
                        turn_id: "capture-turn".into(),
                        attempt_prefix: "capture".into(),
                        content: vec![ContextContent::Text {
                            text: Arc::from("run capture"),
                        }],
                        max_model_steps: ModelStepLimit::Limited(
                            std::num::NonZeroU32::new(1).expect("positive model step limit"),
                        ),
                        cancellation: CancellationToken::new(),
                    })
                    .await
            }
        });
        let faulted = await_stage(&thread, "typed storage fault latch", |snapshot| {
            snapshot.persistence.resume_required
        })
        .await?;
        assert_eq!(
            faulted.persistence.fault,
            Some(pl_core::thread::cold::StorageFaultKind::WriteFailed)
        );
        let generation = faulted.persistence.fault_generation;
        assert_eq!(
            faulted.persistence.output_obligations.len(),
            1,
            "the failed capture owes exactly one archive obligation"
        );
        assert!(
            !faulted.persistence.resume_ready,
            "a failed archive must not offer a continue before its bytes are stored"
        );

        // The requirement is that the owed obligation blocks continuation, not that the Turn reaches
        // one particular phase: with a one-step limit the Turn may simply end after the fault, and no
        // later phase frame is guaranteed to be published. Stopping the driver and refusing the
        // explicit continue while the obligation is still owed proves the real boundary.
        drop(runner);
        assert!(
            !thread.snapshot().persistence.output_obligations.is_empty(),
            "the failed capture is still owed before the retry: {}",
            stage_facts(&thread.snapshot())
        );
        assert!(
            thread.resume_storage(generation).await.is_err(),
            "the pause stays closed until the archive obligation is re-stored"
        );

        thread.retry_output_storage().await?;
        thread.flush().await?;
        assert_eq!(
            retries.load(Ordering::SeqCst),
            1,
            "the real repair and archive run exactly once for the failed capture"
        );

        let history = store.history("archive-repair").await?;
        let fact = history
            .tool_task("task:capture-1")
            .await?
            .context("the committed tool task is durable")?;
        let delivery = fact.delivery.context("the committed delivery is durable")?;
        let reference = delivery
            .delivered_context
            .iter()
            .find_map(|content| match content {
                ContextContent::Resource { reference } => Some(reference.clone()),
                _ => None,
            })
            .context("the repaired reference is recorded on the committed delivery")?;
        assert!(
            reference.id().starts_with("pl.studio.resource:"),
            "the delivery records the real content-addressed archive reference"
        );
        let stored_bytes =
            ResourceReader::read(&archive, reference.clone(), CancellationToken::new()).await?;
        assert_eq!(
            stored_bytes.as_ref(),
            expected.as_slice(),
            "the archive stores the accepted bytes verbatim, once, without the partial write"
        );
        let items = history.items_for_turn("capture-turn").await?;
        let output = items
            .iter()
            .find_map(|item| match item.state() {
                pl_protocol::ThreadItemState::Tool(tool) => tool.terminal_output(),
                _ => None,
            })
            .context("the repaired call keeps its durable terminal result")?;
        let referenced = serde_json::to_value(&reference)?;
        assert_eq!(
            output
                .output_artifacts()
                .iter()
                .filter(|artifact| *artifact == &referenced)
                .count(),
            1,
            "the durable item carries the repaired reference exactly once"
        );

        // Off-window stage: the bounded live window is not a save authority. A window that never saw
        // the commit — no GUI open, a slow reader, or a rolled-over window — must still project the
        // repair from the already-committed body, so the canonical item and its content version come
        // from the single projection owner and the durable row is a pure confirmation. A second,
        // already-committed tool identity with no reference yet is written through the writer's own
        // durable commit path, then repaired by a fresh window that has to read it back.
        // The durable identity the single projection owner derives from the call id, kept identical to
        // `order::tool_id` so the owner's own history lookup finds exactly this committed row.
        let off_window_id = format!("tool:{}:capture-2", "capture-2".len());
        let off_window_item = pl_protocol::ThreadItem::new(
            off_window_id.clone(),
            "archive-repair".to_owned(),
            "capture-turn".to_owned(),
            41,
            1,
            1,
            1,
            pl_protocol::ThreadItemState::Tool(pl_protocol::ThreadToolItem::new(
                pl_protocol::ThreadToolInvocation::new(
                    "capture-2".to_owned(),
                    "capture".to_owned(),
                    "{}".to_owned(),
                ),
                pl_protocol::ThreadToolState::Succeeded(pl_protocol::SucceededThreadTool::new(
                    1,
                    pl_protocol::ThreadToolOutput::new(
                        "accepted so far".to_owned(),
                        Vec::new(),
                        Vec::new(),
                        None,
                    ),
                )),
            )),
        );
        history
            .commit_effect(
                1_000_000,
                EffectCommit {
                    items: std::slice::from_ref(&off_window_item),
                    rolled_back_turns: &Default::default(),
                    identities: &[],
                    messages: &[],
                    receipts: &[],
                    tasks: &[],
                    deliveries: &[],
                    delivery_repairs: &[],
                    attempt: None,
                },
            )
            .await?;
        let repair_only = ThreadEffectBatch {
            thread_id: "archive-repair".to_owned(),
            sequence: 1,
            committed_at: 1,
            delivery_repairs: Arc::from([pl_core::thread::cold::OutputRepair {
                call_id: "capture-2".to_owned(),
                reference: reference.clone(),
            }]),
            ..Default::default()
        };
        let mut off_window = crate::studio::thread_projection::LiveProjection::new();
        crate::studio::thread_projection::seed_repaired_targets(
            &mut off_window,
            &history,
            &repair_only,
        )
        .await?;
        assert!(
            off_window.holds_terminal_tool(&off_window_id),
            "the off-window repair reads the committed body back before projecting it"
        );
        let chat = store.chat_session("archive-repair").await?;
        let packaged = off_window.project_committed(
            &chat,
            &pl_protocol::Thread::placeholder("archive-repair"),
            &repair_only,
            &ThreadSnapshot::default(),
        )?;
        let repackaged = packaged
            .items
            .iter()
            .find(|item| item.id == off_window_id)
            .context("the off-window repair is projected at its original identity")?;
        assert_eq!(
            repackaged.ordinal, off_window_item.ordinal,
            "the supplement keeps the committed position instead of taking a new one"
        );
        assert_eq!(
            repackaged.revision,
            off_window_item.revision + 1,
            "the single projection owner assigns the content version; the writer never invents one"
        );
        let repackaged_output = match repackaged.state() {
            pl_protocol::ThreadItemState::Tool(tool) => tool.terminal_output(),
            _ => None,
        }
        .context("the off-window supplement keeps the terminal tool result")?;
        assert_eq!(
            repackaged_output
                .output_artifacts()
                .iter()
                .filter(|artifact| *artifact == &referenced)
                .count(),
            1,
            "the off-window projection appends the reference exactly once"
        );

        // The caller can only continue once the obligation and its repair really landed.
        thread.resume_storage(generation).await?;
        assert!(!thread.snapshot().persistence.resume_required);

        // A repeated retry reproduces the same bytes and the same reference instead of appending twice.
        let obligation = created
            .lock()
            .unwrap()
            .first()
            .cloned()
            .expect("the tool created its obligation");
        let again = obligation.retry().await?;
        let pl_core::thread::cold::OutputRetryOutcome::StoredWithRepair(repaired) = again else {
            anyhow::bail!("the repeated retry must name the same repaired reference");
        };
        assert_eq!(repaired.reference, reference);
        assert_eq!(
            std::fs::read(&capture_file)?,
            expected,
            "repeating the repair reproduces the fragment instead of duplicating the chunk"
        );
        assert_eq!(retries.load(Ordering::SeqCst), 2);
        thread.close().await?;
        Ok(())
    }

    /// Re-materializes a whole multi-chunk capture plan through the real local backend, then archives
    /// the fragment: the same shape core retries for a failed command capture.
    #[derive(Debug)]
    struct CapturePlanObligation {
        backend: pl_tool::command::LocalCommandBackend,
        archive: crate::resource_store::FileResourceStore,
        capture_file: std::path::PathBuf,
        call_id: String,
        plan: pl_tool::command::CaptureRepair,
        retries: Arc<AtomicUsize>,
    }

    impl pl_core::thread::cold::OutputRetryObligation for CapturePlanObligation {
        fn identity(&self) -> String {
            self.capture_file.display().to_string()
        }
        fn retry(&self) -> pl_core::thread::cold::OutputRetryFuture<'_> {
            Box::pin(async move {
                self.retries.fetch_add(1, Ordering::SeqCst);
                // Replay the plan in acceptance order: one truncation back to the backend-confirmed
                // offset, then every accepted chunk re-appended with its own framing.
                let mut committed_len = self.plan.committed_len;
                for chunk in &self.plan.chunks {
                    committed_len = pl_tool::command::CommandBackend::repair_output_chunk(
                        &self.backend,
                        &self.capture_file,
                        chunk.stream,
                        committed_len,
                        &chunk.pending,
                    )
                    .await
                    .map_err(|error| {
                        pl_core::thread::cold::ColdStoreError {
                            source: Box::new(std::io::Error::other(error.to_string())),
                        }
                    })?;
                }
                let reference = self
                    .archive
                    .retain_command_capture(&self.capture_file)
                    .await
                    .map_err(|error| pl_core::thread::cold::ColdStoreError {
                        source: Box::new(error),
                    })?;
                Ok(pl_core::thread::cold::OutputRetryOutcome::StoredWithRepair(
                    pl_core::thread::cold::OutputRepair {
                        call_id: self.call_id.clone(),
                        reference,
                    },
                ))
            })
        }
        fn received_bytes(&self) -> u64 {
            self.plan.committed_len
        }
        fn location(&self) -> String {
            self.capture_file.display().to_string()
        }
        fn kind(&self) -> pl_core::thread::cold::StorageFaultKind {
            pl_core::thread::cold::StorageFaultKind::WriteFailed
        }
    }

    /// Two streams' accepted chunks survive one reader's partial capture write.
    ///
    /// The first stdout append failed after writing part of its chunk, and the stderr reader had
    /// already accepted (and published) a chunk. Both must be re-materialized verbatim, once, in
    /// acceptance order: the second append must not land past the fault, and a repeated retry must not
    /// duplicate or truncate either chunk. The archived reference must read back the whole fragment.
    #[tokio::test]
    async fn a_two_stream_capture_repair_replays_every_accepted_chunk_once() -> Result<()> {
        use pl_core::context::ResourceReader;
        use pl_core::thread::cold::OutputRetryObligation as _;

        let temp = tempfile::tempdir()?;
        let resources_root = temp.path().join("resources");
        std::fs::create_dir_all(&resources_root)?;
        let archive = crate::resource_store::FileResourceStore::new(resources_root);
        let backend = pl_tool::command::LocalCommandBackend::new(temp.path().to_path_buf());

        // The committed offset comes from the real backend's confirmed writes, not a hand-computed or
        // swallowed fragment length: prepare the header, then append the chunk the capture already
        // stored. That returned length is exactly what a repair truncates back to.
        let target = pl_tool::command::CommandBackend::output_target(
            &backend,
            "session",
            "exec",
            "capture-2",
            "capture",
        )
        .await?;
        pl_tool::command::CommandBackend::prepare_output(&backend, &target, "capture", "/tmp")
            .await?;
        let capture_file = target.capture_file().to_path_buf();
        let committed = b"committed line\n".to_vec();
        let committed_len = pl_tool::command::CommandBackend::append_output_chunk(
            &backend,
            &target,
            pl_tool::command::CommandCaptureStream::Stdout,
            &committed,
        )
        .await?;
        let committed_prefix = std::fs::read(&capture_file)?;
        assert_eq!(
            committed_len,
            committed_prefix.len() as u64,
            "the repair offset is the backend-confirmed fragment length"
        );

        let stdout_pending = b"first failed stdout chunk".to_vec();
        let stderr_pending = b"second stream accepted chunk".to_vec();
        // The failed append left its framing and a few bytes of its chunk past the confirmed offset;
        // the stderr chunk was accepted and published before the fault, so it is still owed.
        {
            use std::io::Write as _;
            let mut fragment = std::fs::OpenOptions::new()
                .append(true)
                .open(&capture_file)?;
            fragment.write_all(b"=== STDOUT ===\n")?;
            fragment.write_all(&stdout_pending[..7])?;
        }

        let expected = {
            let mut expected = committed_prefix;
            expected.extend_from_slice(b"=== STDOUT ===\n");
            expected.extend_from_slice(&stdout_pending);
            expected.push(b'\n');
            expected.extend_from_slice(b"=== STDERR ===\n");
            expected.extend_from_slice(&stderr_pending);
            expected.push(b'\n');
            expected
        };

        let retries = Arc::new(AtomicUsize::new(0));
        let obligation = Arc::new(CapturePlanObligation {
            backend,
            archive: archive.clone(),
            capture_file: capture_file.clone(),
            call_id: "capture-2".to_owned(),
            plan: pl_tool::command::CaptureRepair {
                committed_len,
                chunks: vec![
                    pl_tool::command::CaptureRepairChunk {
                        stream: pl_tool::command::CommandCaptureStream::Stdout,
                        pending: Arc::from(stdout_pending.clone()),
                    },
                    pl_tool::command::CaptureRepairChunk {
                        stream: pl_tool::command::CommandCaptureStream::Stderr,
                        pending: Arc::from(stderr_pending.clone()),
                    },
                ],
            },
            retries: retries.clone(),
        });

        let first = obligation.retry().await?;
        let pl_core::thread::cold::OutputRetryOutcome::StoredWithRepair(first) = first else {
            anyhow::bail!("the repair must name the stored reference");
        };
        assert_eq!(
            std::fs::read(&capture_file)?,
            expected,
            "the retry stores both accepted chunks verbatim, in order, without the partial write"
        );
        let stored =
            ResourceReader::read(&archive, first.reference.clone(), CancellationToken::new())
                .await?;
        assert_eq!(
            stored.as_ref(),
            expected.as_slice(),
            "the archived reference reads back the whole accepted fragment"
        );

        let again = obligation.retry().await?;
        let pl_core::thread::cold::OutputRetryOutcome::StoredWithRepair(again) = again else {
            anyhow::bail!("the repeated retry must name the same stored reference");
        };
        assert_eq!(again.reference, first.reference);
        assert_eq!(
            std::fs::read(&capture_file)?,
            expected,
            "the repeated retry reproduces the same fragment instead of duplicating a chunk"
        );
        assert_eq!(retries.load(Ordering::SeqCst), 2);
        Ok(())
    }

    use pl_tool::command::CommandBackend as _;

    /// A command backend whose capture append is held and then failed, over in-memory IO.
    ///
    /// The read/repair and header capabilities delegate to a real [`pl_tool::command::LocalCommandBackend`],
    /// but `spawn` hands back a [`pl_tool::command::ManagedCommand`] over duplex pipes the test feeds
    /// directly, and `append_output_chunk` waits for a gate and then fails like a real disk write error.
    /// That makes the operation's single writer task — not a hand-made plan — the thing that observes the
    /// fault.
    struct ScriptedCaptureBackend {
        inner: pl_tool::command::LocalCommandBackend,
        stdout: std::sync::Mutex<Option<pl_tool::command::CommandReader>>,
        stderr: std::sync::Mutex<Option<pl_tool::command::CommandReader>>,
        exit: Arc<Notify>,
        gate: Arc<Notify>,
    }

    impl std::fmt::Debug for ScriptedCaptureBackend {
        fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            formatter
                .debug_struct("ScriptedCaptureBackend")
                .finish_non_exhaustive()
        }
    }

    impl pl_tool::command::CommandBackend for ScriptedCaptureBackend {
        type Error = pl_protocol::PureError;

        async fn resolve_cwd(
            &self,
            cwd: Option<&std::path::Path>,
            allow_workspace_escape: bool,
        ) -> Result<String, Self::Error> {
            self.inner.resolve_cwd(cwd, allow_workspace_escape).await
        }

        async fn output_target(
            &self,
            session_id: &str,
            tool_id: &str,
            call_id: &str,
            command: &str,
        ) -> Result<pl_tool::command::CommandOutputTarget, Self::Error> {
            self.inner
                .output_target(session_id, tool_id, call_id, command)
                .await
        }

        async fn spawn(
            &self,
            _request: pl_tool::command::CommandSpawnRequest,
        ) -> Result<pl_tool::command::ManagedCommand, Self::Error> {
            let stdout = self.stdout.lock().expect("stdout reader").take();
            let stderr = self.stderr.lock().expect("stderr reader").take();
            let exit = self.exit.clone();
            Ok(pl_tool::command::ManagedCommand::new(
                None,
                pl_tool::command::CommandIo {
                    stdin: None,
                    stdout,
                    stderr,
                },
                move |_cancellation| async move {
                    exit.notified().await;
                    Ok(pl_tool::command::CommandExit { exit_code: Some(0) })
                },
            ))
        }

        async fn prepare_output(
            &self,
            target: &pl_tool::command::CommandOutputTarget,
            command: &str,
            working_directory: &str,
        ) -> Result<u64, Self::Error> {
            self.inner
                .prepare_output(target, command, working_directory)
                .await
        }

        async fn append_output_chunk(
            &self,
            _target: &pl_tool::command::CommandOutputTarget,
            _stream: pl_tool::command::CommandCaptureStream,
            _chunk: &[u8],
        ) -> Result<u64, Self::Error> {
            // Hold the operation's single writer until the test has confirmed both streams' accepted
            // chunks, then fail the append the way a real capture write error would.
            self.gate.notified().await;
            Err(pl_protocol::PureError::ToolExecutionFailed {
                tool: "exec".to_owned(),
                error: "capture append failed".to_owned(),
            })
        }

        async fn repair_output_chunk(
            &self,
            capture_file: &std::path::Path,
            stream: pl_tool::command::CommandCaptureStream,
            committed_len: u64,
            chunk: &[u8],
        ) -> Result<u64, Self::Error> {
            self.inner
                .repair_output_chunk(capture_file, stream, committed_len, chunk)
                .await
        }

        async fn publish_output(
            &self,
            target: &pl_tool::command::CommandOutputTarget,
        ) -> Result<(), Self::Error> {
            self.inner.publish_output(target).await
        }

        async fn collect_output_artifacts(
            &self,
            target: &pl_tool::command::CommandOutputTarget,
            sizes: pl_tool::command::CommandOutputSizes,
        ) -> Result<Vec<serde_json::Value>, Self::Error> {
            self.inner.collect_output_artifacts(target, sizes).await
        }
    }

    /// Signals once both command streams have published an accepted chunk.
    #[derive(Debug, Default)]
    struct BothStreamsSeen {
        stdout: AtomicUsize,
        stderr: AtomicUsize,
        both: Notify,
    }

    impl pl_tool::command::CommandOutputObserver for BothStreamsSeen {
        fn output_chunk(
            &self,
            stream: pl_tool::command::CommandOutputStream,
            chunk: &[u8],
            _revision: u64,
        ) {
            let counter = match stream {
                pl_tool::command::CommandOutputStream::Stdout => &self.stdout,
                pl_tool::command::CommandOutputStream::Stderr => &self.stderr,
            };
            counter.fetch_add(chunk.len().max(1), Ordering::SeqCst);
            if self.stdout.load(Ordering::SeqCst) > 0 && self.stderr.load(Ordering::SeqCst) > 0 {
                self.both.notify_one();
            }
        }
    }

    /// The real command lifecycle retains and replays a two-stream capture fault.
    ///
    /// This drives the operation, not a hand-made plan: both readers accept *and publish* a chunk from
    /// their stream, the operation's single writer is held until then and only then fails its append,
    /// and the scripted process then exits. So the retained repair plan must hold both streams' accepted
    /// chunks, the terminal result must not be a success, and replaying the plan through the real local
    /// backend must archive exactly those bytes under one reference that reads back whole.
    #[tokio::test]
    async fn a_two_stream_capture_fault_is_retained_and_replayed_by_the_lifecycle() -> Result<()> {
        use pl_core::context::ResourceReader;

        let temp = tempfile::tempdir()?;
        let resources_root = temp.path().join("resources");
        std::fs::create_dir_all(&resources_root)?;
        let archive = crate::resource_store::FileResourceStore::new(resources_root);

        let (mut stdout_writer, stdout_reader) = tokio::io::duplex(1024);
        let (mut stderr_writer, stderr_reader) = tokio::io::duplex(1024);
        let gate = Arc::new(Notify::new());
        let exit = Arc::new(Notify::new());
        let backend = ScriptedCaptureBackend {
            inner: pl_tool::command::LocalCommandBackend::new(temp.path().to_path_buf()),
            stdout: std::sync::Mutex::new(Some(
                Box::pin(stdout_reader) as pl_tool::command::CommandReader
            )),
            stderr: std::sync::Mutex::new(Some(
                Box::pin(stderr_reader) as pl_tool::command::CommandReader
            )),
            exit: exit.clone(),
            gate: gate.clone(),
        };
        let manager = Arc::new(pl_tool::command::CommandProcessManager::new(Arc::new(
            backend,
        )));

        let seen = Arc::new(BothStreamsSeen::default());
        let observer: Arc<dyn pl_tool::command::CommandOutputObserver> = seen.clone();
        let request = pl_tool::command::CommandStartRequest {
            command: "scripted".to_owned(),
            cwd: Some(temp.path().to_path_buf()),
            allow_workspace_escape: false,
            timeout: Duration::from_secs(30),
            yield_time: Duration::ZERO,
            max_output_chars: 4096,
            session_id: "two-stream-lifecycle".to_owned(),
            tool_id: "task-two-stream-lifecycle".to_owned(),
            call_id: "task-two-stream-lifecycle".to_owned(),
            cancellation_token: None,
            output_observer: Some(observer),
        };
        let runner = {
            let manager = manager.clone();
            tokio::spawn(
                async move { manager.run_task("task-two-stream-lifecycle", request).await },
            )
        };

        tokio::io::AsyncWriteExt::write_all(&mut stdout_writer, b"OUT-1\n").await?;
        tokio::io::AsyncWriteExt::write_all(&mut stderr_writer, b"ERR-1\n").await?;
        tokio::time::timeout(Duration::from_secs(30), seen.both.notified())
            .await
            .context("both streams should have published an accepted chunk")?;

        // Both accepted chunks are queued in the one plan; now let the held append fail for real, then
        // close the streams and exit the scripted process so the operation settles.
        gate.notify_one();
        drop(stdout_writer);
        drop(stderr_writer);
        exit.notify_one();

        let snapshot = match tokio::time::timeout(Duration::from_secs(30), runner).await {
            Ok(Ok(Ok(snapshot))) => snapshot,
            Ok(Ok(Err(error))) => {
                anyhow::bail!("the operation returned an error instead of a snapshot: {error}")
            }
            Ok(Err(error)) => anyhow::bail!("the operation task failed: {error}"),
            Err(_) => {
                anyhow::bail!("the operation never settled: the capture writer did not drain")
            }
        };

        assert!(
            matches!(
                &snapshot.output_failure,
                Some(pl_tool::command::CommandCaptureFailure::Write { .. })
            ),
            "the real append failure must surface as a typed capture write failure: {:?}",
            snapshot.output_failure
        );
        assert!(
            snapshot.state.final_result().is_some(),
            "the snapshot is only returned once the operation settled"
        );
        assert!(
            !matches!(
                snapshot.state.final_result(),
                Some(pl_tool::command::CommandProcessFinalResult::Succeeded { .. })
            ),
            "a capture write failure must never be reported as a successful command"
        );

        let plan = snapshot
            .capture_repair()
            .cloned()
            .context("both accepted chunks must stay owed after the failed append")?;
        assert_eq!(
            plan.chunks.len(),
            2,
            "both streams' accepted chunks are retained"
        );
        assert!(
            plan.chunks.iter().any(|chunk| {
                chunk.stream == pl_tool::command::CommandCaptureStream::Stdout
                    && &chunk.pending[..] == b"OUT-1\n".as_slice()
            }),
            "the stdout chunk is retained verbatim"
        );
        assert!(
            plan.chunks.iter().any(|chunk| {
                chunk.stream == pl_tool::command::CommandCaptureStream::Stderr
                    && &chunk.pending[..] == b"ERR-1\n".as_slice()
            }),
            "the stderr chunk is retained verbatim"
        );

        // Replay the plan through the real local backend and archive it: the stored reference must read
        // back exactly the accepted bytes, in acceptance order, with the failed append left off.
        let real = pl_tool::command::LocalCommandBackend::new(temp.path().to_path_buf());
        let mut committed_len = plan.committed_len;
        for chunk in &plan.chunks {
            committed_len = real
                .repair_output_chunk(
                    &snapshot.capture_file,
                    chunk.stream,
                    committed_len,
                    &chunk.pending,
                )
                .await?;
        }
        let reference = archive
            .retain_command_capture(&snapshot.capture_file)
            .await?;
        let stored = ResourceReader::read(&archive, reference, CancellationToken::new()).await?;
        let stored = String::from_utf8_lossy(stored.as_ref());
        assert!(
            stored.contains("OUT-1"),
            "the archive keeps the stdout chunk: {stored}"
        );
        assert!(
            stored.contains("ERR-1"),
            "the archive keeps the stderr chunk: {stored}"
        );

        Ok(())
    }
}

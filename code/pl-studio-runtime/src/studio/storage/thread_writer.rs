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
    collections::{BTreeMap, BTreeSet, VecDeque},
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
    EffectCommit, InputIdentityWrite, MessageIdentityWrite, chat_item, is_retryable_write,
};

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
    /// Admitted effects awaiting their ordered history/calls write, with their projection state.
    effects: VecDeque<(u64, Arc<ThreadWrite>, u64)>,
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
    fault_target: u64,
    retry_requested: bool,
    last_progress_at: Option<Instant>,
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

    pub(crate) fn subscribe(&self) -> tokio::sync::watch::Receiver<HistoryStatus> {
        self.status.subscribe()
    }

    fn report(&self, progress: &Progress) {
        self.status.send_replace(HistoryStatus {
            fault_generation: progress.fault_generation,
            fault: progress.fault,
            error: progress.error.clone(),
            admitted_sequence: progress
                .effects
                .back()
                .map_or(progress.durable, |(seq, _, _)| *seq),
            committed_sequence: progress.durable,
            queued_records: progress.effects.len() as u64,
            queued_bytes: progress.effects.iter().map(|(_, _, bytes)| *bytes).sum(),
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
        progress.retry_requested = false;
        progress.fault_target = progress
            .effects
            .back()
            .map_or(progress.durable, |(seq, _, _)| *seq);
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
            .map_or(progress.durable, |(seq, _, _)| *seq);
        progress.error = Some("history writer made no progress with queued facts".to_owned());
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
        .map(|(sequence, _, _)| *sequence)
        .collect::<Vec<_>>();
    let pending_bytes = progress
        .effects
        .iter()
        .fold(0_u64, |total, (_, _, bytes)| total.saturating_add(*bytes));
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
        .map(|(_, write, _)| write.checkpoint.saved_at)
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
        .map(|(sequence, _, _)| *sequence)
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
    }
}

/// The owner still retains the effect if admission rejects it; making its new input
/// visible here never transfers durable responsibility to the presentation cache.
fn publish_accepted_inputs(inner: &Inner, write: &ThreadWrite) -> Result<()> {
    for mut item in crate::studio::thread_projection::project_accepted_inputs(
        &inner.thread.id,
        &write.checkpoint.state,
        &write.effect,
    )? {
        item.ordinal = inner.chat.reserve_order_in_memory(&item.id)?;
        inner.chat.publish(chat_item(item, false)?)?;
    }
    Ok(())
}

impl ThreadStorageSink {
    pub(crate) async fn new(store: StudioStore, thread: pl_protocol::Thread) -> Result<Self> {
        let chat = store.chat_session(&thread.id).await?;
        chat.initialize_order_allocator().await?;
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
    let next_effect = lock_progress(inner)
        .effects
        .front()
        .map(|(sequence, _, _)| *sequence);
    if let Some(sequence) = next_effect {
        persist_effect(inner, sequence).await?;
        let mut progress = lock_progress(inner);
        if let Some((committed, _, bytes)) = progress.effects.pop_front() {
            ensure!(
                committed == sequence,
                "history queue head changed before acknowledgement"
            );
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
/// The projection state is the effect's own transfer state, so projections stay deterministic
/// regardless of newer admitted revisions and resolve facts the commit itself pruned from the
/// resident owner.
async fn persist_effect(inner: &Inner, sequence: u64) -> Result<()> {
    let write = {
        let progress = lock_progress(inner);
        progress
            .effects
            .front()
            .filter(|(head, _, _)| *head == sequence)
            .map(|(_, write, _)| write.clone())
            .with_context(|| format!("missing admitted effect {sequence}"))?
    };
    {
        let mut progress = lock_progress(inner);
        progress.in_flight_bytes = encoded_bytes(&write.effect);
    }
    let thread = &inner.thread;
    let history = history_store(inner).await?;
    let provisional = crate::studio::thread_projection::project_effect_items(
        thread,
        &write.checkpoint.state,
        &write.effect,
        &BTreeMap::new(),
        &BTreeMap::new(),
        &BTreeSet::new(),
    )?;
    let mut ids = provisional
        .items
        .iter()
        .map(|item| item.id.clone())
        .collect::<Vec<_>>();
    // A Turn keeps referencing the input id whose body its consuming commit pruned from the resident
    // state. Nothing is re-projected from that identity, but its durable item is what proves the
    // referenced body is committed, so the identity is part of this effect's lookup phase.
    ids.extend(provisional.unresolved_inputs.iter().cloned());
    ids.extend(provisional.unresolved_calls.iter().cloned());
    // Only a channel with an earlier preview or durable row may be finalized here.
    if let Some(attempt) = &write.effect.attempt {
        for channel in ["reasoning", "text"] {
            ids.push(crate::studio::thread_projection::attempt_channel_id(
                &attempt.attempt_id,
                channel,
            ));
        }
    }
    let existing = history.existing_items(ids.clone()).await?;
    let hidden_inputs = history
        .hidden_input_identities(provisional.unresolved_inputs.iter().cloned())
        .await?;
    let mut reserved = BTreeMap::new();
    for id in provisional.items.iter().map(|item| &item.id) {
        if !existing.contains_key(id) && !reserved.contains_key(id) {
            let order = inner.chat.reserve_order(id).await?;
            reserved.insert(id.clone(), order);
        }
    }
    if let Some(attempt) = &write.effect.attempt {
        for channel in ["reasoning", "text"] {
            let id =
                crate::studio::thread_projection::attempt_channel_id(&attempt.attempt_id, channel);
            if let Some(order) = inner.chat.assigned_order(&id) {
                reserved.entry(id).or_insert(order);
            }
        }
    }
    let projected = crate::studio::thread_projection::project_effect_items(
        thread,
        &write.checkpoint.state,
        &write.effect,
        &existing,
        &reserved,
        &hidden_inputs,
    )?;
    // Only an explicitly durable hidden identity can replace the visible item requirement.
    projected.ensure_complete()?;
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
                attempt: write.effect.attempt.as_ref(),
            },
        )
        .await?;
    // A committed identity replaces its pending revision in the shared session. The
    // queue owner remains responsible until this read and publication have succeeded.
    let committed_ids = projected
        .items
        .iter()
        .map(|item| item.id.clone())
        .collect::<Vec<_>>();
    for item in history.committed_chat_items(committed_ids).await? {
        inner.chat.publish(item)?;
    }
    if let Some(attempt) = &write.effect.attempt {
        inner.chat.drop_previews_with_prefix(
            &crate::studio::thread_projection::presentation_preview_prefix(&attempt.attempt_id),
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
            .map_or(progress.durable, |(seq, _, _)| *seq);
        progress.retry_requested = false;
        progress.fault = Some(kind);
        progress.error = Some(message);
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
    fn pressure(&self, thread_id: &str) -> StoragePressure {
        if thread_id != self.0.thread.id {
            return StoragePressure {
                error: Some(storage_error("Thread persistence owner mismatch")),
                ..Default::default()
            };
        }
        let progress = lock_progress(&self.0);
        let mut bytes = progress
            .effects
            .iter()
            .fold(0_u64, |total, (_, _, bytes)| total.saturating_add(*bytes));
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
        if let Err(error) = publish_accepted_inputs(&self.0, &write) {
            let message = format!("Thread input could not be published in memory: {error}");
            record_error(
                &self.0,
                pl_protocol::studio::HistoryFault::WriteFailed,
                message.clone(),
            );
            return Err(cold_error(&message));
        }
        let sequence = write.effect.sequence;
        let bytes = encoded_bytes(&write.effect);
        let mut progress = lock_progress(&self.0);
        if sequence > progress.durable
            && !progress
                .effects
                .iter()
                .any(|(queued, _, _)| *queued == sequence)
        {
            let queued_bytes = progress
                .effects
                .iter()
                .fold(0_u64, |total, (_, _, size)| total.saturating_add(*size));
            if progress.effects.len() >= MAX_HISTORY_BATCHES
                || queued_bytes
                    .checked_add(bytes)
                    .is_none_or(|total| total > MAX_HISTORY_THREAD_BYTES)
                || !self.0.channel.reserve(bytes)
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
                report(&self.0, &progress);
                return Err(cold_error(&message));
            }
            progress
                .effects
                .push_back((sequence, Arc::new(write.clone()), bytes));
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
            progress.changed().await.map_err(|_| ColdStoreError {
                source: Box::new(std::io::Error::other(
                    "Thread persistence progress channel closed",
                )),
            })?;
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

/// One typed cold-store failure without the shared-error wrapper.
fn cold_error(message: &str) -> ColdStoreError {
    ColdStoreError {
        source: Box::new(std::io::Error::other(message.to_owned())),
    }
}

#[cfg(test)]
mod storage_fault_tests {
    use super::*;
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
        Ok((temp, store, sink))
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
        let (_temp, store, sink) = sink("missing-visible").await?;
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
        sink.admit("missing-visible", accepted)?;
        tokio::time::timeout(Duration::from_secs(5), sink.flush("missing-visible", 1)).await??;
        let history = store.history("missing-visible").await?;
        let path = store
            .thread_storage_dir("missing-visible")
            .join("history.sqlite");
        let db = Database::connect(crate::studio::paths::sqlite_url(&path)).await?;
        db.execute_unprepared("DELETE FROM history_items WHERE item_id='visible-input'")
            .await?;

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
            .await?
            .expect_err("a visible input without its item must fail closed");
        assert!(
            error
                .to_string()
                .contains("missing the committed input item")
        );
        assert_eq!(history.watermark().await?, 1);
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
        let locked = sink.0.channel.step_lock.lock().await;
        sink.admit("lost-ack", ticket("lost-ack", 1))?;
        persist_effect(&sink.0, 1).await?;
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
        .await?;
        assert!(calls.statistics_gap());
        let dropped_ticket = calls.admitted_ticket();
        assert!(calls.try_admit_effect(&ticket("statistics-crash", 2).effect));
        let resumed_ticket = calls.admitted_ticket();
        assert!(resumed_ticket > dropped_ticket);

        sink.admit("statistics-crash", ticket("statistics-crash", 1))?;
        tokio::time::timeout(Duration::from_secs(5), sink.flush("statistics-crash", 1)).await??;
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
        .await?;
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
}

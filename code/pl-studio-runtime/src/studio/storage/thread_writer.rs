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
    collections::BTreeMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
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
use crate::studio::storage::calls::{CallStatus, DurableToolCall};
use crate::studio::storage::coordinator::{CallsQueueMetrics, ThreadPersistenceMetrics};
use crate::studio::storage::history::{
    EffectCommit, InputIdentityWrite, MessageIdentityWrite, is_retryable_write,
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
    effects: BTreeMap<u64, ThreadWrite>,
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

struct Inner {
    store: StudioStore,
    thread: pl_protocol::Thread,
    /// This writer incarnation's coordinator identity; see [`NEXT_WRITER_INCARNATION`].
    owner: usize,
    /// One durable history reader for the life of this writer.
    ///
    /// The history database is the single ordinal/identity allocator, so the writer opens it once
    /// instead of per effect; the pool has one connection, so every batch stays a short
    /// single-writer transaction and two consecutive effects cannot interleave their allocations.
    history: tokio::sync::OnceCell<crate::studio::storage::history::HistoryStore>,
    progress: Mutex<Progress>,
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
    // The call recorder is one global writer shared by every Thread, so any owning writer can
    // publish its queue pressure; the coordinator stores the latest observation.
    let calls = inner.store.calls();
    inner
        .store
        .thread_persistence()
        .report_calls(CallsQueueMetrics {
            admitted_sequence: calls.admitted_ticket(),
            durable_sequence: calls.durable_ticket(),
            pending_operations: u64::try_from(calls.pending_count()).unwrap_or(u64::MAX),
            pending_bytes: u64::try_from(calls.pending_bytes()).unwrap_or(u64::MAX),
            in_flight_bytes: u64::try_from(calls.in_flight_bytes()).unwrap_or(u64::MAX),
            oldest_pending_age_millis: calls
                .oldest_pending_age()
                .map(|age| u64::try_from(age.as_millis()).unwrap_or(u64::MAX)),
            last_error: calls.last_error(),
            pressure_paused: calls.pressure_paused(),
        });
    let pending = progress.effects.keys().copied().collect::<Vec<_>>();
    let pending_bytes = progress.effects.values().fold(0_u64, |total, write| {
        total.saturating_add(encoded_bytes(&write.effect))
    });
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
        .values()
        .map(|write| write.checkpoint.saved_at)
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
    let max_admitted = progress.effects.keys().next_back().copied().unwrap_or(0);
    let metrics = ThreadPersistenceMetrics {
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

impl ThreadStorageSink {
    pub(crate) fn new(store: StudioStore, thread: pl_protocol::Thread) -> Self {
        let owner = NEXT_WRITER_INCARNATION.fetch_add(1, Ordering::Relaxed);
        let inner = Arc::new(Inner {
            store,
            thread,
            owner,
            history: tokio::sync::OnceCell::new(),
            progress: Mutex::new(Progress::default()),
            changed: Arc::new(tokio::sync::Notify::new()),
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
        let weak = Arc::downgrade(&inner);
        let changed = inner.changed.clone();
        let detach = DetachGuard {
            store: inner.store.clone(),
            thread_id: inner.thread.id.clone(),
            owner,
        };
        tokio::spawn(async move {
            let _detach = detach;
            // 连续可重试失败的起点；任何一次成功的推进都清零。只有持续超过窗口才升级上报。
            let mut retrying_since: Option<Instant> = None;
            loop {
                let Some(inner) = weak.upgrade() else {
                    return;
                };
                match step(&inner).await {
                    Ok(Step::Progressed) => {
                        retrying_since = None;
                        continue;
                    }
                    Ok(Step::Idle(wait)) => {
                        drop(inner);
                        match wait {
                            // 错过 tick 只写一次：等待到期后直接推进下一步，不补写。
                            Some(wait) => {
                                tokio::select! {
                                    () = tokio::time::sleep(wait) => {}
                                    () = changed.notified() => {}
                                }
                            }
                            None => changed.notified().await,
                        }
                    }
                    Err(error) => {
                        let message = error.to_string();
                        let now = Instant::now();
                        let since = *retrying_since.get_or_insert(now);
                        if !is_retryable_write(&message)
                            || now.saturating_duration_since(since) >= RETRYABLE_BUSY_WINDOW
                        {
                            record_error(&inner, message);
                        } else {
                            note_absorbed_conflict(&inner);
                        }
                        tokio::select! {
                            () = tokio::time::sleep(Duration::from_secs(1)) => {},
                            () = inner.store.thread_persistence().retry_notified() => {},
                        }
                    }
                }
            }
        });
        Self(inner)
    }
}

/// Performs one unit of durable work.
async fn step(inner: &Inner) -> Result<Step> {
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
    let next_effect = lock_progress(inner).effects.keys().next().copied();
    if let Some(sequence) = next_effect {
        persist_effect(inner, sequence).await?;
        let mut progress = lock_progress(inner);
        if progress.effects.remove(&sequence).is_some() {
            progress.durable = progress.durable.max(sequence);
        }
        progress.error = None;
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
            .get(&sequence)
            .cloned()
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
    // The live projection reserves the ordinal of a streaming channel the first time it previews
    // it. Reading those reservations (never creating one here) is what lets the writer finalize
    // exactly the same channels the client saw, instead of fabricating empty terminal items.
    if let Some(attempt) = &write.effect.attempt {
        for channel in ["reasoning", "text"] {
            ids.push(crate::studio::thread_projection::attempt_channel_id(
                &attempt.attempt_id,
                channel,
            ));
        }
    }
    let existing = history.existing_items(ids.clone()).await?;
    let reserved = history.reserved_ordinals(ids).await?;
    let projected = crate::studio::thread_projection::project_effect_items(
        thread,
        &write.checkpoint.state,
        &write.effect,
        &existing,
        &reserved,
    )?;
    // 被裁剪的 input body 只有在其 durable item 已存在时才能补全；否则这条已受理事实的历史永远
    // 无法完整落库，必须作为真实持久化错误上报，而不是写入带空洞的时间线或继续暴露旧的
    // Running 状态。
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
            },
        )
        .await?;
    {
        let mut progress = lock_progress(inner);
        progress.calls_admitted = progress.calls_admitted.max(sequence);
    }
    inner.store.calls().commit(&write.effect).await?;
    // 累计摘要按 effect 顺序折叠一次：绝对累计值，重复折叠同一 effect 是 no-op。折叠发生在
    // 本 effect 的 durable 事实之后，因此实时与冷恢复读到的都是同一条已落库事实的结果。
    let summary = {
        let mut progress = lock_progress(inner);
        progress.calls_durable = progress.calls_durable.max(sequence);
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
        .await?;
    let mut pruned = candidate.pruned();
    // 累计摘要随 checkpoint 一起发布，冷恢复因此不需要重新聚合历史集合。
    pruned.state.usage_summary = usage;
    {
        let mut progress = lock_progress(inner);
        progress.saving_revision = pruned.state_revision;
        report(inner, &progress);
    }
    inner.store.state(&inner.thread.id).publish(&pruned).await?;
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
        progress.error = None;
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
        })
        .collect()
}

fn record_error(inner: &Inner, message: String) {
    {
        let mut progress = lock_progress(inner);
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
    fn pressure(&self, thread_id: &str) -> StoragePressure {
        if thread_id != self.0.thread.id {
            return StoragePressure {
                error: Some(storage_error("Thread persistence owner mismatch")),
                ..Default::default()
            };
        }
        let progress = lock_progress(&self.0);
        let bytes = progress.effects.values().fold(0_u64, |total, write| {
            total.saturating_add(encoded_bytes(&write.effect))
        });
        StoragePressure {
            thread_bytes: bytes,
            store_bytes: bytes,
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
            record_error(&self.0, "Thread persistence ticket mismatch".to_owned());
            return Err(ColdStoreError {
                source: Box::new(std::io::Error::other("Thread persistence ticket mismatch")),
            });
        }
        let mut progress = lock_progress(&self.0);
        let sequence = write.effect.sequence;
        if sequence > progress.durable {
            progress
                .effects
                .entry(sequence)
                .or_insert_with(|| write.clone());
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
            let calls = store.calls();
            let Some(call) = calls
                .tool_task(&owner, &task_id)
                .await
                .map_err(|error| cold_error(&error.to_string()))?
            else {
                return Ok(None);
            };
            // Only a terminal call has a committed delivery; a non-terminal row is reported as such
            // so the caller never sees a fabricated finished result.
            let delivery = if call.terminal {
                calls
                    .tool_delivery(&owner, &call.call_id)
                    .await
                    .map_err(|error| cold_error(&error.to_string()))?
            } else {
                None
            };
            Ok(Some(pl_core::thread::cold::DurableToolTask {
                task: durable_task_record(&call),
                delivery,
            }))
        }
    }
}

/// Projects one durable call fact onto the core task record a read-only query reports.
///
/// The durable row keeps the call identity, revision and status, but not the live
/// `cancel_requested` flag or a cancellation acknowledgement: those are owner facts. A durable
/// answer therefore reports the committed status/result without inventing a cancellation receipt.
fn durable_task_record(call: &DurableToolCall) -> pl_core::thread::task::TaskRecord {
    use pl_core::thread::task::TaskStatus;

    let status = match call.status {
        CallStatus::Running => TaskStatus::Running,
        CallStatus::Completed | CallStatus::Committed => TaskStatus::Succeeded,
        CallStatus::Cancelled => TaskStatus::Cancelled,
        CallStatus::Interrupted => TaskStatus::Interrupted,
        CallStatus::Failed | CallStatus::Rejected => TaskStatus::Failed,
    };
    pl_core::thread::task::TaskRecord {
        id: format!("task:{}", call.call_id),
        call_id: call.call_id.clone(),
        tool_id: call.tool_id.clone(),
        turn_id: call.turn_id.clone(),
        revision: call.revision,
        status,
        cancel_requested: call.cancel_requested || matches!(status, TaskStatus::Cancelled),
        acknowledgement: None,
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
mod tests {
    use super::*;

    /// 现场回归：`(code: 5) database is locked` 来自历史写事务，属于可自愈冲突，必须被写者内部
    /// 静默退避；结构、约束、损坏与契约错误必须立刻上报，绝不无限重试。
    #[test]
    fn only_transient_storage_conflicts_are_absorbed() {
        for retryable in [
            "history effect 12 commit failed: Execution Error: error returned from database: \
             (code: 5) database is locked",
            "database table is locked",
            "database is busy",
            "disk I/O error",
            "SQLITE_BUSY",
            "SQLITE_LOCKED",
            "SQLITE_IOERR",
        ] {
            assert!(
                is_retryable_write(retryable),
                "{retryable} 必须视为可重试忙"
            );
        }
        for terminal in [
            "Thread persistence ticket mismatch",
            "Thread persistence owner mismatch",
            "UNIQUE constraint failed: input_identities.identity",
            "history item revision conflict",
            "checkpoint candidate has no cumulative summary folded to its own revision",
            "history database could not be opened read-only",
        ] {
            assert!(!is_retryable_write(terminal), "{terminal} 不得被吸收");
        }
    }
}

//! Owned, retryable product projections of the Thread's committed effect stream.
//!
//! Observation consumes the Thread's reliable admission queue and is its **single realtime
//! projection owner**: it projects every admitted fact and the streaming overlay once, publishes the
//! content into the shared session and broadcasts the same typed changes on a bounded feed that
//! subscriptions only read. Billing, directory and terminal facts come from the same in-memory
//! projection, so an ordinary commit neither parks this observer behind the owner's command queue
//! nor waits for the disk. Durable history is read back once when the projection is installed on a
//! Thread whose work started earlier, and by the explicit cold recovery of an unloaded child.
mod billing;
mod directory;
mod reports;

use super::{ModelPerformanceOwner, StudioRuntime};
use crate::studio::{ProductEventBus, StudioStore};
use anyhow::{Context, Result, bail};
use pl_core::chat::Session;
use pl_core::thread::{
    ThreadHandle, ThreadLifecycle, ThreadSnapshot, UsageSummary, cold::ThreadWrite,
};
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};
use tokio::{
    sync::{Notify, broadcast, watch},
    task::JoinHandle,
};

use crate::studio::storage::thread_writer::HistoryChannel;
use crate::studio::thread_projection::{LiveEvent, LiveProjection, storage_state};

/// Fixed bound on the frames one subscriber may fall behind the projection owner.
///
/// The feed is a bounded observation channel, not a log: a subscriber that stops draining observes
/// a lag and resynchronizes from the database window instead of letting this queue grow with history.
const LIVE_FEED_FRAMES: usize = 512;

/// One output of a Thread's single realtime projection owner.
#[derive(Debug, Clone)]
pub(in crate::studio) enum ThreadLiveEvent {
    /// Changes produced by committed effect `sequence`.
    ///
    /// The vocabulary is deliberately content-free: the projection owner writes every body into the
    /// shared session and the GUI reads it from that one content window, so the status feed carries
    /// only Turn lifecycle, interaction and runtime facts. A subscriber therefore never projects or
    /// republishes content.
    Committed {
        sequence: u64,
        changes: Vec<LiveEvent>,
    },
    /// The projection owner could not prove continuity for `dropped` commits and re-seeded; every
    /// subscriber must resynchronize from the database window.
    Rebase { dropped: u64 },
    /// Current execution activity of the Thread; `None` means no activity.
    ///
    /// The activity is an independent typed projection of the same owner state (it never carries a
    /// second copy of the content), so it is published by the same single owner and only when it
    /// really changed; a subscriber only forwards it. It rides as one shared handle rather than an
    /// inline value: the feed is a `broadcast` channel that clones its frame once per subscriber, so
    /// the body must stay cheap to clone and must not bloat the frame every other variant shares.
    Activity {
        activity: Option<Arc<pl_protocol::ThreadActivity>>,
    },
}

/// The realtime projection feed of one Thread.
///
/// The Thread's observation worker is its only producer, and it publishes into the shared chat
/// session at the same time. Subscribers therefore forward typed events and never project, publish
/// or read the effect window themselves.
pub(in crate::studio) struct ThreadLiveFeed {
    events: broadcast::Sender<ThreadLiveEvent>,
    /// The Thread's authoritative typed storage state, published by this owner only.
    ///
    /// The projection owner is the one place that derives it from the owner snapshot and the
    /// coordinator's typed watch, so a subscriber mirrors this value instead of recomputing the
    /// state from its own copies. A storage pause changes neither the content nor the activity, so
    /// the value is watched on its own instead of riding the content feed: a subscriber parked on
    /// the feed still learns about a pause the moment the owner publishes it.
    storage: watch::Sender<Option<pl_protocol::ThreadStorageState>>,
    /// Newest terminal Turn this owner projected, published by this owner only.
    ///
    /// A subscription's authoritative snapshot carries only the *active* Turn, and `turnCompleted`
    /// is broadcast exactly once, so a terminal Turn that committed before a client registered —
    /// a late (re)subscription, or one that resynchronizes after observing a lag — has no other way
    /// to reach it. The owner retains the last terminal Turn here as the same typed fact its frame
    /// carried, so a new subscriber is told the authoritative terminal Turn instead of guessing it
    /// from the timeline window, re-reading SQL, or caching the `running` state it happened to see.
    /// It is a watch rather than a feed frame precisely because it has to outlive the broadcast: a
    /// fact nobody was subscribed for is still the fact the next subscriber must start from.
    last_turn: watch::Sender<Option<Arc<pl_protocol::Turn>>>,
}

impl ThreadLiveFeed {
    fn new(storage: Option<pl_protocol::ThreadStorageState>) -> Self {
        let (events, _) = broadcast::channel(LIVE_FEED_FRAMES);
        let (storage, _) = watch::channel(storage);
        let (last_turn, _) = watch::channel(None);
        Self {
            events,
            storage,
            last_turn,
        }
    }

    pub(in crate::studio) fn subscribe(&self) -> broadcast::Receiver<ThreadLiveEvent> {
        self.events.subscribe()
    }

    /// Watches the newest terminal Turn this owner projected, so a subscriber that registers after
    /// that commit never has to derive the fact from the window or from SQL.
    pub(in crate::studio) fn last_turn(&self) -> watch::Receiver<Option<Arc<pl_protocol::Turn>>> {
        self.last_turn.subscribe()
    }

    /// Publishes the newest terminal Turn the owner projected.
    ///
    /// Only a terminal Turn qualifies: the running Turn is already the authoritative snapshot's own
    /// fact, and replacing the retained terminal Turn with a busy one would let a resubscribing
    /// client mistake an in-flight Turn for the last finished one. A committed effect can also reach
    /// this owner *after* a newer one — a deferred admission that storage pressure released late is
    /// taken over behind the live stream — so the retained fact only ever moves forward and the newest
    /// terminal Turn is never replaced by an older one.
    fn publish_last_turn(&self, turn: Arc<pl_protocol::Turn>) {
        let current = self.last_turn.borrow().clone();
        if let Some(current) = current.as_ref() {
            // Only a strictly newer fact moves the slot. A duplicate of the newest fact — the same
            // effect sequence and identity, e.g. a cold seed the live path then re-derives — would
            // only wake every subscriber to deliver nothing, and a terminal Turn that storage
            // pressure released late must never replace a newer one.
            if current.revision > turn.revision
                || (current.revision == turn.revision && current.id == turn.id)
            {
                return;
            }
        }
        self.last_turn.send_replace(Some(turn));
    }

    /// Watches the owner's typed storage state, so a subscriber only mirrors it.
    pub(in crate::studio) fn storage(
        &self,
    ) -> watch::Receiver<Option<pl_protocol::ThreadStorageState>> {
        self.storage.subscribe()
    }

    /// Publishes the owner's newly derived typed storage state.
    fn publish_storage(&self, storage: Option<pl_protocol::ThreadStorageState>) {
        self.storage.send_replace(storage);
    }

    fn publish(&self, event: ThreadLiveEvent) {
        // A feed with no subscriber is the normal state of a Thread without an open GUI; the
        // projection owner still consumed and published the fact.
        let _ = self.events.send(event);
    }

    /// Tells every subscriber that the owner re-seeded and continuity is no longer provable.
    fn rebase(&self, dropped: u64) {
        self.publish(ThreadLiveEvent::Rebase { dropped });
    }
}

#[derive(Clone, Default)]
struct Progress {
    initialized: bool,
    applied: u64,
    /// Newest commit this projection owner had handed over when it applied `applied`.
    ///
    /// The durable barrier of a settled observation waits for exactly this ticket. It is not the
    /// absolute commit watermark: a Thread re-activated in this process starts from a fresh reliable
    /// channel whose commits are already durable, so waiting for the checkpoint's sequence would wait
    /// for a watermark that can never move.
    handoff: u64,
    error: Option<Arc<anyhow::Error>>,
    finished: bool,
}
struct WorkerState {
    progress: watch::Sender<Progress>,
    retry: Notify,
    /// The Thread's reliable admission channel, shared with its history writer.
    ///
    /// A settled observation reads a finished Turn back from history, and its own barrier has to ask
    /// the writer that took the projection's handoff how far it is durable. The handle is captured
    /// here, at observation time, so the barrier observes the very incarnation the writer admits
    /// into instead of a channel that a cleanly detached predecessor already released.
    channel: Arc<HistoryChannel>,
}
struct Observation {
    thread: ThreadHandle,
    state: Arc<WorkerState>,
    task: Mutex<Option<JoinHandle<()>>>,
}
impl Drop for Observation {
    fn drop(&mut self) {
        if let Some(task) = self
            .task
            .get_mut()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take()
        {
            task.abort();
        }
    }
}
#[derive(Clone)]
pub(super) struct ObservationServices {
    pub(super) store: StudioStore,
    pub(super) events: ProductEventBus,
    pub(super) performance: ModelPerformanceOwner,
    pub(super) threads: crate::thread_assembler::StudioThreadAssembler,
    pub(super) writer: crate::studio::agent_host::ThreadWriteBehindWriter,
}
struct Shared {
    projector: ObservationServices,
    observations: Mutex<BTreeMap<String, Vec<Arc<Observation>>>>,
    /// One feed per currently observed Thread; removed when the Thread is drained.
    live: Mutex<BTreeMap<String, Arc<ThreadLiveFeed>>>,
}
#[derive(Clone)]
pub(super) struct ThreadObservations(Arc<Shared>);

impl ThreadObservations {
    pub(super) fn new(projector: ObservationServices) -> Self {
        Self(Arc::new(Shared {
            projector,
            observations: Mutex::new(BTreeMap::new()),
            live: Mutex::new(BTreeMap::new()),
        }))
    }

    /// The live feed of one observed Thread, if its projection owner was installed.
    pub(super) fn live_feed(&self, id: &str) -> Option<Arc<ThreadLiveFeed>> {
        self.0
            .live
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(id)
            .cloned()
    }

    pub(super) fn install(
        &self,
        thread_factory: crate::studio::thread_factory::StudioThreadFactory,
    ) -> Result<()> {
        let owner = Arc::downgrade(&self.0);
        let draining = owner.clone();
        let message_store = self.0.projector.store.clone();
        self.0.projector.threads.observe_assembly(
            move |id, thread| {
                if let Some(owner) = owner.upgrade() {
                    ThreadObservations(owner).observe(id, thread);
                }
            },
            move |id| {
                let owner = draining.clone();
                let thread_factory = thread_factory.clone();
                Box::pin(async move {
                    let owner = owner
                        .upgrade()
                        .ok_or(crate::thread_assembler::ThreadAssemblyError::Closed)?;
                    let observations = ThreadObservations(owner);
                    observations
                        .synchronize(Some(&id))
                        .await
                        .map_err(|source| {
                            crate::thread_assembler::ThreadAssemblyError::Resource {
                                operation: "drain Thread product observation",
                                source: source.into_boxed_dyn_error(),
                            }
                        })?;
                    thread_factory.forget_tool_binding(&id);
                    observations.drain(&id);
                    Ok(())
                })
            },
            // 一条已受理消息的持久身份：per-Thread 身份索引给出正文摘要与原始受理序号，因此离开
            // 常驻窗口的重复投递会被裁决而不是再送一次。索引行缺少摘要（迁移回填）或时间线已提交
            // 但索引无法证明正文的旧消息都按“不可验证”上报，让上层 fail-closed；查询失败同样上报，
            // 绝不在无法证明身份时把投递当成新消息受理。
            Some(Arc::new(move |thread_id: String, message_id: String| {
                let store = message_store.clone();
                Box::pin(async move {
                    let identity_error = |source: anyhow::Error| {
                        crate::thread_assembler::ThreadAssemblyError::Resource {
                            operation: "read durable message identity",
                            source: source.into_boxed_dyn_error(),
                        }
                    };
                    // 该身份查询是冷读路径：没有活跃 writer 时 `history` 只是一个不做 IO 的句柄，
                    // 不建库也不升级它；有活跃 writer 时它正是这个 Thread 的同一权威句柄，读会等
                    // 本句柄 writer 完成建表与 identity meta，因此不会读到半初始化库。这里刻意不
                    // 走 `history_writer`，避免未启动的冷读凭空登记出第二写者身份。
                    let history = store.history(&thread_id).await.map_err(identity_error)?;
                    if let Some(identity) =
                        history.message_identity(&message_id).await.map_err(identity_error)?
                    {
                        return Ok(Some(match identity.digest {
                            Some(digest) => {
                                crate::thread_assembler::observation::DurableMessageIdentity::Proven {
                                    sequence: identity.sequence,
                                    digest,
                                }
                            }
                            None => crate::thread_assembler::observation::DurableMessageIdentity::Unverifiable,
                        }));
                    }
                    // 时间线已经提交这条消息、但索引没有它的记录：升级前的历史无法证明正文，
                    // 必须 fail-closed，而不是重新投递成新消息。
                    let item = crate::studio::thread_projection::order::message_id(&message_id);
                    let committed = history
                        .existing_items([item])
                        .await
                        .map_err(identity_error)?;
                    Ok((!committed.is_empty()).then_some(
                        crate::thread_assembler::observation::DurableMessageIdentity::Unverifiable,
                    ))
                })
            })),
        )?;
        Ok(())
    }

    /// Releases the retained observation for a drained or evicted Thread.
    ///
    /// The observation worker owns a strong `ThreadHandle`; keeping its entry after the assembly
    /// registry released the owner would keep every session that was ever activated resident. The
    /// entry is removed once its final projection is durable, so a later re-activation installs a
    /// fresh observation instead of reusing the released handle.
    fn drain(&self, id: &str) {
        let mut all = self
            .0
            .observations
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        all.remove(id);
        drop(all);
        self.0
            .live
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(id);
    }

    fn observe(&self, id: String, thread: ThreadHandle) {
        let recovered_through = thread.snapshot().commit_sequence;
        let (progress, _) = watch::channel(Progress {
            applied: recovered_through,
            ..Progress::default()
        });
        // The same channel the writer incarnation admits into: the observation barrier below asks
        // this handle how far the writer is durable, not a freshly created one.
        let channel = self
            .0
            .projector
            .store
            .thread_persistence()
            .history_channel(&id);
        let state = Arc::new(WorkerState {
            progress,
            retry: Notify::new(),
            channel,
        });
        // Seed the feed with the newest typed storage state the coordinator already reports, so a
        // subscriber that opens before this owner's first pass mirrors a real fact instead of an
        // unset placeholder. The owner republishes its own derivation on the first pass.
        let initial_storage = self
            .0
            .projector
            .store
            .thread_persistence()
            .snapshot()
            .threads
            .iter()
            .find(|detail| detail.thread_id == id)
            .map(|detail| storage_state(&thread.snapshot(), detail));
        let feed = Arc::new(ThreadLiveFeed::new(initial_storage));
        self.0
            .live
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(id.clone(), feed.clone());
        let worker_state = state.clone();
        let projector = self.0.projector.clone();
        let worker_thread = thread.clone();
        let worker_id = id.clone();
        let task = tokio::spawn(async move {
            run(
                projector,
                worker_id,
                worker_thread,
                worker_state,
                recovered_through,
                feed,
            )
            .await;
        });
        let observation = Arc::new(Observation {
            thread,
            state,
            task: Mutex::new(Some(task)),
        });
        let mut all = self
            .0
            .observations
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let previous = all.entry(id).or_default();
        previous.retain(|entry| {
            let progress = entry.state.progress.borrow();
            !progress.finished || progress.error.is_some()
        });
        previous.push(observation);
    }

    async fn synchronize(&self, id: Option<&str>) -> Result<()> {
        self.0.projector.writer.retry_now();
        let observations: Vec<_> = self
            .0
            .observations
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .iter()
            .filter(|(key, _)| id.is_none_or(|id| id == key.as_str()))
            .flat_map(|(id, entries)| entries.iter().map(|entry| (id.clone(), entry.clone())))
            .collect();
        for (id, observation) in observations {
            let target_snapshot = observation.thread.snapshot();
            let target = target_snapshot.commit_sequence;
            let mut progress = observation.state.progress.subscribe();
            observation.state.progress.send_modify(|progress| {
                if !progress.finished {
                    progress.error = None;
                }
            });
            observation.state.retry.notify_one();
            let durable_through = loop {
                let state = progress.borrow().clone();
                if let Some(error) = state.error {
                    bail!("Thread {id} product observation failed: {error:#}");
                }
                if state.initialized
                    && state.applied >= target
                    && (target_snapshot.lifecycle != ThreadLifecycle::Closed || state.finished)
                {
                    break state.handoff;
                }
                if state.finished {
                    bail!("Thread {id} product observer ended before commit {target}");
                }
                progress
                    .changed()
                    .await
                    .context("Thread product observer progress closed")?;
            };
            // A settled observation is also durable. Observation no longer flushes history per batch,
            // so this barrier is what keeps "the observer applied commit N" meaning "history holds
            // commit N": cold reads (an unloaded child's terminal repair, a restored Turn) read a
            // Turn's items straight from history after this. The wait is for the handoff ticket the
            // projection had reached when it applied `target` — every commit up to `target` it owns —
            // so it neither waits on the owner's command queue nor on a later, unprojected commit, and
            // it never asks for a watermark a channel created by a re-activation cannot move.
            await_history_durable(&observation.state.channel, &id, durable_through)
                .await
                .with_context(|| format!("Thread {id} product observation barrier"))?;
        }
        // The observation worker no longer waits on disk for each projected batch, so the explicit
        // sync barrier carries the durability: every product write admitted up to the watermark this
        // barrier just waited for is drained before the caller observes a settled terminal state.
        // The target is fixed at call time, so this never waits for unrelated later writes.
        let target = self.0.projector.writer.admitted_ticket();
        self.0.projector.writer.flush_through(target).await?;
        Ok(())
    }

    /// Explicit parent continuation repairs terminal notifications even for unloaded children.
    /// This reads a checkpoint plus durable history only; no child model or workspace is opened.
    pub(super) async fn reconcile_children(&self, parent: &str) -> Result<()> {
        let services = &self.0.projector;
        for child in services.store.list_threads_for_root(parent).await? {
            if child.parent_thread_id.as_deref() != Some(parent) {
                continue;
            }
            if services.threads.thread(&child.id).is_some() {
                self.synchronize(Some(&child.id)).await?;
                continue;
            }
            let child = pl_protocol::Thread::from(child);
            let Some(checkpoint) =
                crate::studio::thread_factory::recovery::load_checkpoint(&services.store, &child)
                    .await?
            else {
                continue;
            };
            if let Some(turn) = checkpoint
                .state
                .turns
                .iter()
                .rev()
                .find(|turn| turn.state != pl_core::thread::TurnState::Running)
            {
                // An unloaded child has no resident projection, so this repair reads the Turn's
                // items from durable history: it is the cold path, and the checkpoint cannot carry
                // the finished Turn's body.
                let items = services
                    .store
                    .history(&child.id)
                    .await?
                    .items_for_turn(&turn.turn_id)
                    .await?;
                reports::publish_terminal(
                    services,
                    &child,
                    &checkpoint.state,
                    turn,
                    checkpoint.state_revision,
                    false,
                    &crate::studio::thread_projection::TurnResult::from_items(&items),
                )
                .await?;
            }
        }
        Ok(())
    }

    pub(super) async fn finish(&self) -> Result<()> {
        self.synchronize(None).await?;
        let observations: Vec<_> = self
            .0
            .observations
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .values()
            .flatten()
            .cloned()
            .collect();
        for observation in observations {
            let mut progress = observation.state.progress.subscribe();
            loop {
                let state = progress.borrow().clone();
                if let Some(error) = state.error {
                    bail!("Thread product observer failed: {error:#}");
                }
                if state.finished {
                    break;
                }
                progress
                    .changed()
                    .await
                    .context("Thread product observer ended without final state")?;
            }
            let task = observation
                .task
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .take();
            if let Some(task) = task {
                task.await.context("Thread product observer join failed")?;
            }
        }
        Ok(())
    }
}

impl StudioRuntime {
    /// Waits for product directories and billing to reach the current committed Thread watermark.
    /// Reattempts a previously failed projection without invoking models or tools.
    pub async fn synchronize_thread_observation(&self, thread_id: &str) -> Result<()> {
        self.thread_observations.synchronize(Some(thread_id)).await
    }
}

/// The single realtime projection owner of one Thread.
///
/// It lives in the Thread's observation worker, so a GUI opening and closing never changes who
/// projects or who publishes content: every committed effect is projected exactly once into the
/// shared chat session, and its typed changes are broadcast to whichever subscribers exist. The same
/// batch is handed to the Thread's reliable admission channel, so the history writer commits the
/// content this owner published instead of projecting the effect a second time.
struct LiveOwner {
    projection: LiveProjection,
    /// The Thread's only activity projection: the same owner derives it from the snapshot it just
    /// published, so activity never becomes a second fact source and never reads SQL.
    activity: crate::studio::thread_projection::ActivityProjection,
    /// Last activity already broadcast, so an unchanged projection does not produce a frame.
    activity_published: Option<pl_protocol::ThreadActivity>,
    /// Last typed storage state this owner published to its feed.
    ///
    /// The owner derives it from the owner snapshot plus the coordinator's typed watch, so a
    /// subscriber mirrors the value instead of recomputing it from its own copies. It is kept as one
    /// value so a change of fault kind, generation, watermark or phase is all one comparison.
    storage_published: Option<pl_protocol::ThreadStorageState>,
    /// The Thread's reliable admission channel, shared with the history writer.
    channel: Arc<HistoryChannel>,
    chat: Session,
    thread: pl_protocol::Thread,
    usage: UsageSummary,
    feed: Arc<ThreadLiveFeed>,
}

impl LiveOwner {
    async fn open(
        projector: &ObservationServices,
        id: &str,
        snapshot: &ThreadSnapshot,
        thread: pl_protocol::Thread,
        feed: Arc<ThreadLiveFeed>,
        channel: Arc<HistoryChannel>,
        persistence: Option<&pl_protocol::ThreadPersistenceSnapshot>,
    ) -> Result<Self> {
        let chat = projector.store.chat_session(id).await?;
        let usage = live_usage(projector, id, snapshot);
        // The one cold read of a projection: the durable facts of work that already started before
        // this owner existed. Everything the commit and preview paths resolve afterwards comes from
        // these tables, so a re-activated Thread never waits on a history reader mid-Turn.
        let mut projection = LiveProjection::new();
        let (seed_items, hidden) = cold_seed(projector, id, snapshot).await?;
        projection.seed(seed_items, hidden);
        // Seed the activity projection from the very snapshot this owner was installed on, so a
        // subscriber that opens mid-Turn immediately sees the authoritative current activity.
        let mut activity = crate::studio::thread_projection::ActivityProjection::default();
        let activity_published = activity.observe(id, snapshot);
        // The owner is the single source of the typed storage state: publish the value it was
        // installed with, so a subscriber that opened before this first pass sees the authoritative
        // fact instead of an unset placeholder.
        let storage_published = persistence.map(|detail| storage_state(snapshot, detail));
        feed.publish_storage(storage_published.clone());
        let owner = Self {
            projection,
            activity,
            activity_published,
            storage_published,
            channel,
            chat,
            thread,
            usage,
            feed,
        };
        // The seeded facts are already resident, so the Thread's budget covers them from the moment
        // the owner is installed instead of only after its first commit.
        owner.publish_retained();
        // A Thread re-activated after its last Turn finished has no live frame left for that fact:
        // the authoritative snapshot carries only the active Turn and `turnCompleted` was broadcast
        // once, to subscribers that may be long gone. This is the very cold-read path the owner is
        // installed on, so the newest terminal Turn is read here once — bounded, without item bodies
        // — and seeded into the same retained fact the live projection publishes. The live path can
        // then only move it forward, so a seed can never hide a later finished Turn, and a late
        // duplicate of a Turn already retained is dropped by the same identity/revision guard.
        if let Some(turn) = projector
            .store
            .history(id)
            .await?
            .newest_terminal_turn()
            .await?
        {
            owner.feed.publish_last_turn(Arc::new(turn));
        }
        Ok(owner)
    }

    /// Publishes how much this projection currently retains in its own tables.
    ///
    /// The projection owns the retained bodies and the Turn report accumulator, and both outlive
    /// the batch the writer already committed. The reliable channel's budget is what pauses new
    /// model/tool admission at a safe gap, so it has to see this number; nothing here waits for a
    /// GUI, because the owner publishes whether or not anyone is subscribed.
    fn publish_retained(&self) {
        // The activity summary and its bounded resident detail are resident facts of the same owner,
        // so they belong to the same Thread budget instead of living outside it.
        let bytes = self
            .projection
            .retained_bytes()
            .saturating_add(self.activity.retained_bytes());
        self.channel.set_projection_bytes(bytes);
    }

    /// Projects one admitted write into the session, broadcasts its live changes and hands the very
    /// same immutable batch to the Thread's reliable save channel.
    ///
    /// The batch is published before it is handed over, so the writer can only ever confirm a body
    /// the shared window already holds: there is no save-before-publish interleaving to lose.
    ///
    /// A reliable-output repair can name an identity the bounded window already released. This owner is
    /// still the single fact source for the canonical item and its content version, so it reads that
    /// already-committed body back once here and folds it in before projecting; the ordinary path reads
    /// nothing, and the writer never invents a version or re-projects a written effect.
    async fn advance(
        &mut self,
        services: &ObservationServices,
        thread_id: &str,
        write: &ThreadWrite,
    ) -> Result<()> {
        if !write.effect.delivery_repairs.is_empty() {
            let history = services.store.history(thread_id).await?;
            crate::studio::thread_projection::seed_repaired_targets(
                &mut self.projection,
                &history,
                &write.effect,
            )
            .await
            .map_err(|error| anyhow::anyhow!("live Thread repair seed failed: {error}"))?;
        }
        self.project_write(write)
    }

    /// Folds one admitted write into the shared session and the reliable save channel.
    fn project_write(&mut self, write: &ThreadWrite) -> Result<()> {
        let effect = &write.effect;
        crate::studio::thread_projection::fold_effect_accounting(&mut self.usage, effect)?;
        let projected = self.projection.advance(
            &self.chat,
            &self.thread,
            &self.usage,
            effect,
            &write.checkpoint.state,
        );
        let (changes, prepared) = match projected {
            Ok(projected) => projected,
            Err(error) => {
                // The writer saves exactly what this owner projects, so a projection this owner cannot
                // complete is a fact that cannot be handed over or saved. Report it so the durable
                // barrier fails closed with the real reason instead of waiting forever.
                self.channel.fail_projection(
                    effect.sequence,
                    format!("live Thread projection failed: {error}"),
                );
                return Err(anyhow::anyhow!("live Thread projection failed: {error}"));
            }
        };
        self.channel.prepare(prepared);
        // Retain the terminal Turn of this commit as the owner's own last-turn fact before the frame
        // is broadcast: `turnCompleted` reaches only the subscribers that exist right now, so the
        // value a later subscriber is told has to be the very frame this commit produced.
        if let Some(turn) = changes.iter().rev().find_map(|change| match change {
            LiveEvent::Turn { turn, .. } if turn.state.is_terminal() => Some(turn),
            _ => None,
        }) {
            self.feed.publish_last_turn(Arc::new(turn.clone()));
        }
        self.feed.publish(ThreadLiveEvent::Committed {
            sequence: effect.sequence,
            changes,
        });
        self.publish_retained();
        Ok(())
    }

    /// Hands one admitted effect this owner installed *behind* to the reliable save channel.
    ///
    /// The Thread's initialization commits before the assembler publishes the observation, so those
    /// effects are already part of the snapshot this projection was seeded from and are never
    /// replayed as live frames. The history writer still owes them a durable row, so the same single
    /// projection folds their content into memory here — once, and never the writer projecting the
    /// effect itself.
    fn prepare_behind(&mut self, write: &ThreadWrite) -> Result<()> {
        let projected = self.projection.project_committed(
            &self.chat,
            &self.thread,
            &write.effect,
            &write.checkpoint.state,
        );
        let prepared = match projected {
            Ok(prepared) => prepared,
            Err(error) => {
                self.channel.fail_projection(
                    write.effect.sequence,
                    format!("live Thread projection failed: {error}"),
                );
                return Err(anyhow::anyhow!("live Thread projection failed: {error}"));
            }
        };
        self.channel.prepare(prepared);
        // The effects this owner installed *behind* never become live frames, but they can still be
        // the newest finished Turn of the Thread (a Thread re-activated from its directory, or a
        // subscription that opens after the terminal commit). The terminal Turn is re-derived with
        // the same projection the live frame uses, so a resubscribing client starts from the
        // authoritative terminal fact instead of an empty last-turn.
        if let Some(turn) = crate::studio::thread_projection::project_effect_terminal_turn(
            &self.thread.id,
            &write.checkpoint.state,
            &write.effect,
        ) {
            self.feed.publish_last_turn(Arc::new(turn));
        }
        self.publish_retained();
        Ok(())
    }

    /// Projects the uncommitted streaming preview of the current owner snapshot.
    ///
    /// The preview is written into the shared session — the one content window — and emits no
    /// content frame; only the activity it implies can produce a frame.
    async fn stream(&mut self, snapshot: &ThreadSnapshot) -> Result<()> {
        self.projection
            .stream(&self.chat, &self.thread, snapshot, snapshot.commit_sequence)
            .await
            .map_err(|error| anyhow::anyhow!("live Thread preview failed: {error}"))?;
        self.observe_activity(snapshot);
        self.publish_retained();
        Ok(())
    }

    /// Re-derives the Thread's current activity from the snapshot this owner just published.
    ///
    /// The activity is an independent projection of the same facts: a phase change, a new foreground
    /// tool or an interaction change reaches it through the owner snapshot, never through `effect`.
    /// Only a real change is broadcast, so a streaming body update does not cost a second status
    /// frame and no subscriber ever has to project the activity itself.
    fn observe_activity(&mut self, snapshot: &ThreadSnapshot) {
        let activity = self.activity.observe(&self.thread.id, snapshot);
        if activity == self.activity_published {
            return;
        }
        self.activity_published = activity.clone();
        // One shared handle per published change: every subscriber clones that handle, never the body.
        self.feed.publish(ThreadLiveEvent::Activity {
            activity: activity.map(Arc::new),
        });
    }

    /// Republishes the typed storage state this owner derives, only when it really changed.
    ///
    /// A storage pause, a pressure warning, the explicit-resume latch or a durable receipt change
    /// nothing about the content or the activity, so they need their own fact: the owner watches the
    /// coordinator's typed status directly and republishes here even when no other frame arrived, so
    /// a parked subscriber learns about a save failure without waiting for an unrelated commit.
    fn observe_storage(
        &mut self,
        snapshot: &ThreadSnapshot,
        persistence: Option<&pl_protocol::ThreadPersistenceSnapshot>,
    ) {
        let storage = persistence.map(|detail| storage_state(snapshot, detail));
        if storage == self.storage_published {
            return;
        }
        self.storage_published = storage.clone();
        self.feed.publish_storage(storage);
    }
}

impl Drop for LiveOwner {
    /// Releases this owner's share of the Thread's budget when the observation ends.
    ///
    /// The retained bodies die with the owner, so leaving the gauge where it was would keep a
    /// released Thread reading as pending work forever. A re-activated Thread installs a new owner
    /// that republishes its own tables on its first step.
    fn drop(&mut self) {
        self.channel.set_projection_bytes(0);
    }
}

/// The one cold read of a live projection: the durable facts of the work it is installed onto.
///
/// The current owner snapshot is deliberately compact — it no longer holds the process bodies a
/// running Turn committed earlier — so a projection installed mid-Turn would be unable to resolve an
/// identity a later effect still references. Reading the running and newest Turn's committed items
/// once, together with the hidden dispositions of the inputs still in play, closes that gap without
/// putting a history read on the commit or preview path.
async fn cold_seed(
    projector: &ObservationServices,
    id: &str,
    snapshot: &ThreadSnapshot,
) -> Result<(
    Vec<pl_protocol::ThreadItem>,
    std::collections::BTreeSet<String>,
)> {
    let mut turn_ids = snapshot
        .turns
        .iter()
        .filter(|turn| turn.state == pl_core::thread::TurnState::Running)
        .map(|turn| turn.turn_id.clone())
        .collect::<Vec<_>>();
    if let Some(newest) = snapshot.turns.last()
        && !turn_ids.contains(&newest.turn_id)
    {
        turn_ids.push(newest.turn_id.clone());
    }
    if turn_ids.is_empty() {
        return Ok((Vec::new(), std::collections::BTreeSet::new()));
    }
    let history = projector.store.history(id).await?;
    let mut items = Vec::new();
    for turn_id in &turn_ids {
        items.extend(history.items_for_turn(turn_id).await?);
    }
    let mut input_ids = snapshot
        .inputs
        .iter()
        .map(|record| record.input.id.clone())
        .collect::<std::collections::BTreeSet<_>>();
    for turn in snapshot.turns.iter() {
        if let Some(input_id) = &turn.input_id {
            input_ids.insert(input_id.clone());
        }
    }
    let hidden = history.hidden_input_identities(input_ids).await?;
    Ok((items, hidden))
}

/// Cumulative usage the live projection folds further effects onto.
///
/// The history writer's reported summary is the value a hot increment, a reconnect and a cold
/// restore agree on; a checkpoint that was loaded before any report seeds the same value.
fn live_usage(
    projector: &ObservationServices,
    id: &str,
    snapshot: &ThreadSnapshot,
) -> UsageSummary {
    let coordinator = projector.store.thread_persistence();
    match coordinator.usage(id) {
        Some(usage) => usage,
        None => {
            coordinator.seed_usage(id, snapshot.usage_summary.clone());
            snapshot.usage_summary.clone()
        }
    }
}

async fn run(
    projector: ObservationServices,
    id: String,
    thread: ThreadHandle,
    state: Arc<WorkerState>,
    recovered_through: u64,
    feed: Arc<ThreadLiveFeed>,
) {
    use futures::FutureExt;
    let mut updates = thread.subscribe();
    // The reliable handoff can gain a commit this projection must take over without any new owner
    // snapshot: an admission that storage pressure deferred lands after the observation was
    // installed, so its fact is older than this projection's seed and is never replayed as a live
    // frame. The channel's own status is the signal for it, and it is what makes "the writer is
    // waiting for a handoff" reach the only task that can hand one over.
    let mut handoff = state.channel.subscribe();
    // The coordinator's typed persistence watch is a first-class wake source: a save failure can
    // change with no owner frame and no handoff, so without this arm the owner would only publish
    // the fault when something unrelated happened to arrive.
    let mut persistence = projector.store.thread_persistence().subscribe();
    let mut current = thread.snapshot();
    let mut worker = ProjectionWorker {
        services: projector,
        id,
        thread,
        state,
        recovered_through,
        feed,
        live: None,
    };
    // Product facts are projected once per committed revision; the live projection additionally
    // follows every owner snapshot so streaming previews reach the same single publisher.
    let mut projected = None;
    loop {
        let products = projected != Some((current.commit_sequence, current.lifecycle));
        // Read the coordinator's newest typed facts before publishing: a storage change is a fact
        // this pass has to carry, exactly like a commit or a preview it also projects.
        let detail = persistence
            .borrow_and_update()
            .threads
            .iter()
            .find(|detail| detail.thread_id == worker.id)
            .cloned();
        // The projection stays isolated behind `catch_unwind`, so a panicking projection is
        // still reported as an error instead of taking down the observation worker.
        let result =
            std::panic::AssertUnwindSafe(worker.project(&current, products, detail.as_ref()))
                .catch_unwind()
                .await
                .unwrap_or_else(|_| Err(anyhow::anyhow!("Thread product projection panicked")));
        match result {
            Ok(()) => {
                if products {
                    projected = Some((current.commit_sequence, current.lifecycle));
                }
                let finished = current.lifecycle == ThreadLifecycle::Closed;
                worker.state.progress.send_replace(Progress {
                    initialized: true,
                    applied: current.commit_sequence,
                    handoff: worker.state.channel.handoff_ticket(),
                    error: None,
                    finished: false,
                });
                if finished {
                    // 固定目标 ticket：只等待本次观察已受理的产品写入，不等待整个系统空闲。
                    let target = worker.services.writer.admitted_ticket();
                    match worker.services.writer.flush_through(target).await {
                        Ok(()) => {
                            worker
                                .state
                                .progress
                                .send_modify(|progress| progress.finished = true);
                            return;
                        }
                        Err(error) => {
                            worker.state.progress.send_modify(|progress| {
                                progress.error = Some(Arc::new(anyhow::Error::new(error)));
                                progress.finished = false;
                            });
                        }
                    }
                }
            }
            Err(error) => {
                // A fact this owner could not project leaves the committed stream discontinuous for
                // every subscriber, so the feed tells them to resynchronize from the durable window
                // instead of letting the client splice a hole; the worker retries the same watermark.
                worker.feed.rebase(0);
                worker
                    .state
                    .progress
                    .send_modify(|progress| progress.error = Some(Arc::new(error)));
            }
        }
        if current.lifecycle == ThreadLifecycle::Closed {
            tokio::select! {
                () = worker.state.retry.notified() => {}
                _ = handoff.changed() => {}
            }
            current = worker.thread.snapshot();
            continue;
        }
        tokio::select! {
            () = worker.state.retry.notified() => current = worker.thread.snapshot(),
            // A handoff the writer is waiting for may be the only reason this worker still has work
            // to do; re-run the projection so a deferred admission is taken over and saved.
            _ = handoff.changed() => current = worker.thread.snapshot(),
            // A typed storage change with no owner frame still has to reach subscribers; re-run the
            // pass so the owner republishes the state it just read.
            _ = persistence.changed() => current = worker.thread.snapshot(),
            update = updates.next() => match update {
                Some(snapshot) => current = snapshot,
                None => {
                    worker.state.progress.send_modify(|progress| progress.error = Some(Arc::new(anyhow::anyhow!("Thread subscription ended before its final projection"))));
                    tokio::select! {
                        () = worker.state.retry.notified() => {}
                        _ = handoff.changed() => {}
                    }
                    current = worker.thread.snapshot();
                }
            }
        }
    }
}

/// Everything one Thread's observation worker carries across a projection: the product services, the
/// owner handle it reads committed effects from, its progress and retry channel, the feed it
/// broadcasts on and the live projection it folds into.
///
/// Keeping this as one domain value is what lets the worker project without threading nine
/// independent arguments through every call; the live projection above stays a pure function of one
/// effect.
struct ProjectionWorker {
    services: ObservationServices,
    id: String,
    thread: ThreadHandle,
    state: Arc<WorkerState>,
    /// Committed watermark the Thread already had when this projection was seeded. Terminal reports
    /// for Turns at or below it are repairs, not fresh continuations, so they do not wake a parent.
    recovered_through: u64,
    feed: Arc<ThreadLiveFeed>,
    live: Option<LiveOwner>,
}

/// The report facts of one finished Turn: the live projection's bounded accumulator when it folded
/// the Turn, otherwise the durable single-Turn slice.
///
/// The hot path takes the accumulator, which the projection maintained from the Turn's first
/// record, so a Turn that committed its visible output over many effects reports all of it without a
/// second copy of every body and without a history read. A Turn the resident projection never saw
/// from its start — a Thread re-activated once its Turn had already run, or a recovered child — has
/// no accumulator, and only then does this wait for the matching durable handoff and read the
/// bounded single-Turn slice from history. That is the cold recovery path, never a routine commit.
async fn turn_result(
    services: &ObservationServices,
    live: &LiveOwner,
    turn_id: &str,
) -> Result<crate::studio::thread_projection::TurnResult> {
    if let Some(result) = live.projection.turn_result(turn_id) {
        return Ok(result.clone());
    }
    await_history_durable(
        &live.channel,
        &live.thread.id,
        live.channel.handoff_ticket(),
    )
    .await?;
    let items = services
        .store
        .history(&live.thread.id)
        .await?
        .items_for_turn(turn_id)
        .await?;
    Ok(crate::studio::thread_projection::TurnResult::from_items(
        &items,
    ))
}

/// Waits until the history writer's durable watermark covers the handoff ticket `sequence`.
///
/// This deliberately does not use `Thread::flush`. That barrier targets *every commit the owner has
/// admitted*, including ones this projection has not handed over yet, and the writer refuses to save
/// a fact no projection holds — so an observer waiting on it would wait for its own next step, and
/// the request itself travels through the owner's command queue, which a running Turn only reaches at
/// its own safety points. Waiting for this projection's own handoff ticket asks for exactly the
/// commits it already prepared, in the order they were prepared, so it always completes, and a
/// channel that never saw a handoff (`0`) is trivially durable instead of waiting for a watermark a
/// re-activated Thread's fresh channel can no longer move.
///
/// The writer publishes its terminal diagnostic on the same channel, so a storage failure ends this
/// barrier with the real reason instead of hanging on a watermark that can no longer advance.
async fn await_history_durable(channel: &HistoryChannel, id: &str, sequence: u64) -> Result<()> {
    let mut status = channel.subscribe();
    loop {
        {
            let status = status.borrow_and_update();
            if status.committed_sequence >= sequence {
                return Ok(());
            }
            if let Some(error) = &status.error {
                bail!("Thread {id} history did not become durable through {sequence}: {error}");
            }
        }
        status
            .changed()
            .await
            .context("Thread history progress channel closed")?;
    }
}

/// The Thread's product association, from the resident directory entry or from durable storage.
///
/// An archived hot entry may disappear before its original registration reaches SQLite, so the cold
/// branch drains the accepted directory writes first. The target is fixed at admission time and later
/// unrelated writes do not extend the wait. Only the first install and a product refresh resolve this
/// DTO: a streaming preview belongs to the owner already installed and never reads the directory.
async fn product_association(
    services: &ObservationServices,
    id: &str,
) -> Result<pl_protocol::Thread> {
    if let Some(thread) = services.events.thread_snapshot(id) {
        return Ok(thread);
    }
    let target = services.writer.admitted_ticket();
    services.writer.flush_through(target).await?;
    let association = services
        .store
        .read_thread_association(id)
        .await?
        .context("observed Thread has no product association")?;
    Ok(pl_protocol::Thread::from(association))
}

// The worker carries the projection owner, its feed and the product watermark across one projection;
// splitting them into a context struct would only hide the same state behind one more indirection.
impl ProjectionWorker {
    /// Takes over every admitted fact this projection owner has not folded yet, projects the current
    /// streaming overlay and, when `products` is set, the product directory and billing facts they
    /// imply.
    ///
    /// The reliable admission queue is the one source of committed work: a fact enters it when core
    /// commits it and leaves it only once this owner folded it *and* the durable store saved it, so
    /// the projection never races the owner's command queue or the disk. Facts the owner had already
    /// folded into the snapshot this projection was seeded from are taken over without being replayed
    /// as live frames.
    ///
    /// # Errors
    /// Returns a projection, storage association or directory failure. The worker reports it through
    /// its progress channel and retries the same watermark instead of skipping a commit.
    async fn project(
        &mut self,
        snapshot: &ThreadSnapshot,
        products: bool,
        persistence: Option<&pl_protocol::ThreadPersistenceSnapshot>,
    ) -> Result<()> {
        let Self {
            services,
            id,
            state,
            recovered_through,
            feed,
            live,
            ..
        } = self;
        let recovered_through = *recovered_through;
        if live.is_none() {
            // The product association is resolved once, when this projection owner is installed.
            // A Thread re-activated after its directory entry left memory reads it from SQLite here.
            let product = product_association(services, id).await?;
            *live = Some(
                LiveOwner::open(
                    services,
                    id,
                    snapshot,
                    product,
                    Arc::clone(feed),
                    Arc::clone(&state.channel),
                    persistence,
                )
                .await?,
            );
        }
        let live = live
            .as_mut()
            .expect("live projection owner was just installed");
        // The projection owner is the single producer of realtime content: it folds every admitted
        // fact exactly once, publishes it into the shared session and broadcasts the typed change,
        // whether or not a GUI is open.
        //
        // The association the owner was installed with is what identifies a parent or a billing
        // root; it never changes for a Thread, so a commit does not re-read the directory DTO for it.
        let owner_product = live.thread.clone();
        let mut newest_commit_at = 0_i64;
        while let Some(write) = live.channel.next_unprojected() {
            let sequence = write.effect.sequence;
            if sequence <= recovered_through {
                // A fact the owner committed before this projection existed: it is already part of
                // the snapshot the projection was seeded from, so it is folded for durability only.
                live.prepare_behind(&write)?;
                continue;
            }
            live.advance(services, id.as_str(), &write).await?;
            billing::record(
                &services.performance,
                &owner_product.root_thread_id,
                &write.effect,
            )?;
            newest_commit_at = newest_commit_at.max(write.effect.committed_at);
            if let Some(turn) = write
                .effect
                .turn
                .as_ref()
                .filter(|turn| turn.state != pl_core::thread::TurnState::Running)
            {
                // The terminal report carries the whole Turn from the reliable in-memory projection:
                // a long Turn commits its text and tool facts over many effects, so reporting only
                // the effect that finished it would drop the earlier visible output.
                let result = turn_result(services, live, &turn.turn_id).await?;
                reports::publish_terminal(
                    services,
                    &owner_product,
                    snapshot,
                    turn,
                    sequence,
                    sequence > recovered_through,
                    &result,
                )
                .await?;
            }
        }
        // Streaming previews change without a commit and belong to the same publisher.
        live.stream(snapshot).await?;
        // The typed storage state is one owner fact: it is republished here whether this pass was
        // triggered by a commit, a preview or the coordinator's own typed status changing.
        live.observe_storage(snapshot, persistence);
        if !products {
            return Ok(());
        }
        // The directory DTO is re-read for a product refresh, because a concurrent title, mode or
        // archive operation owns those fields and a preview-only pass must not pay for the lookup.
        let mut product = product_association(services, id).await?;
        product.status = crate::studio::thread_projection::status(snapshot);
        product.updated_at = product.updated_at.max(newest_commit_at);
        // The directory summary is derived from the same reliable owner as the terminal report: the
        // in-memory projection on the hot path, and durable history only for a Turn it never saw.
        let summary = match snapshot
            .turns
            .last()
            .filter(|turn| turn.state != pl_core::thread::TurnState::Running)
        {
            Some(turn) => turn_result(services, live, &turn.turn_id)
                .await?
                .final_text(),
            None => None,
        };
        directory::publish(services, product, snapshot, summary).await?;
        Ok(())
    }
}

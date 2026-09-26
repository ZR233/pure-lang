//! Product observations own no execution tasks: a live subscription is a pure reader of the
//! Thread's single realtime projection owner.
//!
//! The observation worker owns the only `LiveProjection` of a Thread, so it projects every committed
//! effect and the streaming overlay exactly once — into the shared chat session and into a typed
//! feed — whether or not a GUI is open. A subscription registers on that feed before the
//! open/reconnect persistence barrier, emits one authoritative `snapshot` first, and afterwards
//! forwards the owner's typed frames as continuous `notification` envelopes. It never publishes
//! content, never reads the effect window, never runs a model/tool query and never opens a second
//! content source: the current activity is the owner's own typed projection of the state it
//! published, and content itself is delivered only by the shared ChatView window — this status
//! stream carries no window version, so one body update never costs a second status frame. A frame
//! the owner could not prove continuous becomes `lagged`, and the client resynchronizes from the
//! authoritative window instead of splicing a hole.
use super::StudioRuntime;
use anyhow::{Context, Result};
use pl_core::thread::{ThreadHandle, ThreadSnapshot};
use pl_protocol::{
    ThreadActivity, ThreadActivityDetail, ThreadNotification, ThreadNotificationEnvelope,
    ThreadStorageState, ThreadSubscriptionUpdate,
};
use std::{collections::VecDeque, num::NonZeroUsize, sync::Arc};
use tokio::{
    sync::{broadcast, watch},
    time::{Duration, Instant},
};

use super::thread_observation::ThreadLiveEvent;
use crate::studio::thread_projection::{ActivityDetailRead, LiveEvent, TurnEvent};

/// Fixed upper bound on frames a live subscription may queue for one slow consumer.
///
/// A subscription is a bounded observation channel, not a durable log: when the client stops
/// draining, the queued frames are dropped and one `lagged` frame (with a new epoch) tells it to
/// resynchronize from the database window instead of letting this queue grow with history.
const LIVE_PENDING_FRAMES: usize = 512;
const OBSERVATION_DELIVERY_INTERVAL: Duration = Duration::from_millis(16);

/// Snapshot stream backed by the canonical owner or immutable retired-child history.
/// Dropping it stops only observation.
pub struct StudioThreadSubscription {
    runtime: StudioRuntime,
    source: SubscriptionSource,
    _residency_pin: super::residency::ThreadResidencyPins,
}

enum SubscriptionSource {
    Live(Box<LiveSubscription>),
    Retired(Box<RetiredSubscription>),
}

/// A retired child Thread has no execution owner, so its subscription is a bounded one-shot stream:
/// the authoritative snapshot, then the newest terminal Turn the durable history already holds.
///
/// A planner child is immutable once retired, so there is no live feed to mirror — but it is still a
/// Thread fact the client reads `lastTurn` from, and the same "finished Turn is broadcast once" gap
/// applies. Both facts come from the durable history here, never from the parent's live state and
/// never from a timeline body, so a reopened child reports the same terminal Turn its owner would
/// have.
struct RetiredSubscription {
    snapshot: Option<Box<pl_protocol::ThreadSnapshot>>,
    terminal_turn: Option<pl_protocol::Turn>,
    thread_id: String,
    /// Notification watermark, seeded from the snapshot and advanced by exactly one per delivered
    /// notification, exactly like the live subscription's own counter.
    revision: u64,
}

struct LiveSubscription {
    handle: ThreadHandle,
    thread: pl_protocol::Thread,
    /// Typed frames of the Thread's single realtime projection owner, registered before the baseline
    /// below so nothing the owner publishes after this point can be lost.
    feed: Option<broadcast::Receiver<ThreadLiveEvent>>,
    /// Owner state captured right after receiver registration and before the persistence barrier;
    /// the first frame reports it so effects committed past it are still delivered as notifications.
    baseline: Option<ThreadSnapshot>,
    pending: VecDeque<ThreadSubscriptionUpdate>,
    /// Deadline of the current display-throttle window for a queue of only-coalescable frames.
    observation_due: Option<Instant>,
    /// Highest committed effect sequence already forwarded to this subscriber.
    applied: u64,
    /// Notification watermark: the strict `revision` counter seeded from the first snapshot.
    revision: u64,
    /// Broadcast lifecycle identity; increments when the effect window forces a resync.
    epoch: u64,
    started: bool,
    thread_id: String,
    /// Last activity frame already delivered to this subscriber; `None` means no activity yet.
    activity: Option<ThreadActivity>,
    /// The projection owner's typed storage state, mirrored instead of recomputed per frame.
    ///
    /// The Thread's observation owner is the one place that derives the state from the owner
    /// snapshot and the coordinator's typed watch, so a subscriber only reads the value it
    /// published. A storage change wakes this watch directly, so a parked subscriber learns about a
    /// save failure without waiting for an unrelated content frame.
    storage_feed: Option<watch::Receiver<Option<ThreadStorageState>>>,
    /// Last storage state already delivered; `None` means no reportable storage fact.
    storage: Option<ThreadStorageState>,
    /// Newest terminal Turn the projection owner retained, so a subscription that opens after that
    /// Turn committed still starts from the authoritative terminal fact.
    last_turn: Option<watch::Receiver<Option<Arc<pl_protocol::Turn>>>>,
    /// Identity of the Turn this subscriber currently knows as active, from the snapshot or the
    /// newest lifecycle frame. A retained terminal Turn is never replayed while one is active, so the
    /// last finished Turn can never be mistaken for the current one.
    active_turn: Option<String>,
    /// Newest terminal Turn already delivered to this subscriber: its identity and the committed
    /// revision it was delivered at. The retained fact and the live frame that first carried it are
    /// one fact, so it is delivered exactly once at one revision — a duplicate of the same fact must
    /// not manufacture a second notification — while a genuinely newer revision of the same Turn
    /// (a late metadata update the writer legalised at the same identity) is a new fact and passes.
    delivered_terminal: Option<(String, u64)>,
}

/// One non-blocking step of the projection feed.
enum FeedStep {
    /// At least one frame was applied to the pending queue.
    Frame,
    /// No frame is available right now.
    Idle,
    /// The projection owner released the feed; no further frame can arrive.
    Closed,
}

/// One non-blocking read of the projection feed.
enum FeedRead {
    Event(ThreadLiveEvent),
    Lagged(u64),
    Empty,
    Closed,
}

impl LiveSubscription {
    /// Emits the authoritative first frame and seeds the notification watermark from it.
    async fn open(&mut self, runtime: &StudioRuntime) -> Result<ThreadSubscriptionUpdate> {
        self.started = true;
        self.thread = runtime.read_protocol_thread(&self.thread_id).await?;
        let state = self
            .baseline
            .take()
            .unwrap_or_else(|| self.handle.snapshot());
        self.applied = state.commit_sequence;
        self.revision = state.commit_sequence;
        let usage = runtime
            .usage_through_owner(&self.thread_id, &state, &self.handle)
            .await?;
        let persistence = runtime.thread_persistence_snapshot(&self.thread_id);
        let mut snapshot = crate::studio::thread_projection::project_snapshot(
            self.thread.clone(),
            &state,
            &usage,
            persistence.as_ref(),
        )?;
        runtime.annotate_model_route(&mut snapshot)?;
        // The first frame carries the authoritative activity and the owner's typed storage state;
        // later frames only carry them when they really changed. Reading the owner's own value here
        // (and marking it seen) means the very first frame can never disagree with the owner, and the
        // subscriber never has to recompute the state from its own copies.
        self.activity = snapshot.activity.clone();
        self.storage = match self.storage_feed.as_mut() {
            Some(watch) => {
                let observed = watch.borrow_and_update().clone();
                snapshot.storage = observed.clone();
                observed
            }
            None => snapshot.storage.clone(),
        };
        // The authoritative snapshot carries only the active Turn, and `turnCompleted` is broadcast
        // exactly once, so a Turn that finished before this subscription registered would otherwise
        // never reach the client: the timeline window is not a Turn fact and the client must not
        // infer one from a body it read. The owner retains that terminal Turn as its own typed fact,
        // so it is delivered as the first notification after the snapshot, on the watermark the
        // snapshot just seeded.
        self.active_turn = snapshot.active_turn.as_ref().map(|turn| turn.id.clone());
        self.reflect_retained_turn();
        Ok(ThreadSubscriptionUpdate::Snapshot {
            snapshot: Box::new(snapshot),
        })
    }

    /// Queues the owner's retained terminal Turn while no Turn is active.
    ///
    /// An active Turn is the snapshot's own fact and its lifecycle frames follow it, so nothing is
    /// replayed while one is running. The queued frame uses this subscription's own watermark, exactly
    /// like every other notification, so its `base_revision` is the snapshot's revision (or the
    /// revision of the frames already delivered) and no gap is manufactured.
    fn reflect_retained_turn(&mut self) {
        let Some(watch) = self.last_turn.as_mut() else {
            return;
        };
        // The watch is read and marked seen in one step, so a retained fact this subscriber may not
        // deliver now never leaves an unseen change that would wake the loop again immediately.
        let retained = watch.borrow_and_update().clone();
        let Some(turn) = retained else {
            return;
        };
        if self.active_turn.is_some() || self.terminal_delivered(&turn) {
            return;
        }
        self.delivered_terminal = Some((turn.id.clone(), turn.revision));
        self.enqueue(ThreadNotification::TurnCompleted {
            turn: (*turn).clone(),
        });
    }

    /// Whether this subscriber already delivered exactly this terminal fact.
    ///
    /// The comparison is by identity *and* committed revision: the retained fact and the live frame
    /// that first carried it are the same `(id, revision)`, so the second copy is dropped instead of
    /// spending a notification on a fact the client already holds, while a legitimately newer
    /// revision of the same Turn is a different fact and is delivered.
    fn terminal_delivered(&self, turn: &pl_protocol::Turn) -> bool {
        self.delivered_terminal
            .as_ref()
            .is_some_and(|(id, revision)| id == &turn.id && *revision >= turn.revision)
    }

    /// Mirrors the projection owner's typed storage state and emits a frame when it changed.
    ///
    /// The owner already derived the state from the coordinator's fault kind/generation/watermarks
    /// and the owner snapshot's execution phase, so this only reads the value it published: nothing
    /// here parses an error string, reads content or recomputes a body, and there is exactly one
    /// place that decides what the storage state is.
    ///
    /// The comparison is by value against the last delivered state rather than by “did the watch
    /// advance”: the subscription is woken by several sources (owner frames, the storage watch, a
    /// display-throttle window), and a wake that already marked the watch seen must still be able to
    /// report the change it woke for.
    fn refresh_observations(&mut self) {
        let Some(watch) = self.storage_feed.as_mut() else {
            return;
        };
        let storage = watch.borrow_and_update().clone();
        if storage != self.storage {
            self.storage = storage.clone();
            self.enqueue(ThreadNotification::StorageChanged {
                storage: storage.map(Box::new),
            });
        }
    }

    /// Reads whatever the projection owner has already broadcast, without waiting.
    fn read_feed(&mut self) -> FeedRead {
        let Some(feed) = self.feed.as_mut() else {
            return FeedRead::Closed;
        };
        match feed.try_recv() {
            Ok(event) => FeedRead::Event(event),
            Err(broadcast::error::TryRecvError::Empty) => FeedRead::Empty,
            Err(broadcast::error::TryRecvError::Lagged(dropped)) => FeedRead::Lagged(dropped),
            Err(broadcast::error::TryRecvError::Closed) => FeedRead::Closed,
        }
    }

    /// Moves every frame the projection owner already broadcast into the pending queue.
    fn drain_feed(&mut self) -> FeedStep {
        let mut step = FeedStep::Idle;
        loop {
            match self.read_feed() {
                FeedRead::Event(event) => {
                    step = FeedStep::Frame;
                    self.apply(event);
                }
                FeedRead::Lagged(dropped) => {
                    step = FeedStep::Frame;
                    self.lagged(dropped);
                }
                FeedRead::Empty => return step,
                FeedRead::Closed => return FeedStep::Closed,
            }
        }
    }

    /// Waits for the next owner frame, a typed storage-state change or a terminal-Turn change.
    ///
    /// The owner publishes the storage state on its own watch instead of the content feed, so without
    /// the second arm a save failure could only reach the client while some unrelated frame happened
    /// to arrive. It publishes the newest terminal Turn on a watch for the same reason: a Turn that
    /// finished while this subscriber was parked must not wait for an unrelated frame either.
    /// Awaiting all three is what makes both facts realtime on a parked stream.
    async fn wait_feed_or_storage(&mut self) -> FeedStep {
        enum Woke {
            Feed(Result<ThreadLiveEvent, broadcast::error::RecvError>),
            Storage(bool),
            Turn(bool),
        }
        // Without an owner feed there is no projection to wait on; the stream is over. Waiting on
        // the storage watch alone would park a released subscription instead of ending it.
        if self.feed.is_none() {
            return FeedStep::Closed;
        }
        let woke = {
            let feed = self.feed.as_mut();
            let storage = self.storage_feed.as_mut();
            let last_turn = self.last_turn.as_mut();
            tokio::select! {
                received = async {
                    match feed {
                        Some(receiver) => receiver.recv().await,
                        None => std::future::pending().await,
                    }
                } => Woke::Feed(received),
                changed = async {
                    match storage {
                        Some(receiver) => receiver.changed().await.is_ok(),
                        None => std::future::pending().await,
                    }
                } => Woke::Storage(changed),
                // A finished Turn published while this subscriber is parked is a fact it must not
                // learn only on the next unrelated wake-up.
                changed = async {
                    match last_turn {
                        Some(receiver) => receiver.changed().await.is_ok(),
                        None => std::future::pending().await,
                    }
                } => Woke::Turn(changed),
            }
        };
        match woke {
            Woke::Feed(Ok(event)) => {
                self.apply(event);
                FeedStep::Frame
            }
            Woke::Feed(Err(broadcast::error::RecvError::Lagged(dropped))) => {
                self.lagged(dropped);
                FeedStep::Frame
            }
            Woke::Feed(Err(broadcast::error::RecvError::Closed)) => FeedStep::Closed,
            // The changed value is read by `refresh_observations`, which the caller runs next. A
            // released watch is forgotten so it never wakes this loop in a busy cycle.
            Woke::Storage(open) => {
                if !open {
                    self.storage_feed = None;
                }
                FeedStep::Frame
            }
            // A released watch is forgotten so it never wakes this loop in a busy cycle; the
            // changed value is read by `reflect_retained_turn`, which runs next.
            Woke::Turn(open) => {
                if !open {
                    self.last_turn = None;
                }
                FeedStep::Frame
            }
        }
    }

    /// Applies one projection frame from the Thread's single publisher.
    ///
    /// The receiver is registered *before* the baseline is captured, so effects committed between
    /// the two are not lost; the frames already covered by that baseline arrive first and are dropped
    /// by their commit sequence. The feed carries no content, so a dropped baseline-covered commit
    /// cannot leave a content hole here — the shared window is the only content source. The one fact
    /// the baseline does not restate is a finished Turn, which the snapshot cannot carry; that is why
    /// the owner retains it and [`LiveSubscription::reflect_retained_turn`] delivers it whatever the
    /// watermark covered.
    fn apply(&mut self, event: ThreadLiveEvent) {
        match event {
            ThreadLiveEvent::Committed { sequence, changes } => {
                if sequence <= self.applied {
                    return;
                }
                self.applied = sequence;
                for change in changes {
                    self.push(change);
                }
            }
            ThreadLiveEvent::Runtime { runtime } => {
                self.enqueue(ThreadNotification::ThreadRuntimeUpdated {
                    runtime: Box::new((*runtime).clone()),
                });
            }
            ThreadLiveEvent::Activity { activity } => {
                if activity.as_deref() != self.activity.as_ref() {
                    self.activity = activity.as_deref().cloned();
                    self.enqueue(ThreadNotification::ActivityChanged {
                        activity: activity.map(|activity| Box::new((*activity).clone())),
                    });
                }
            }
            ThreadLiveEvent::Rebase { dropped } => self.lagged(dropped),
        }
    }

    /// Signals a gap: the client resynchronizes from the database window instead of splicing.
    fn lagged(&mut self, dropped: u64) {
        self.pending.clear();
        self.observation_due = None;
        self.epoch = self.epoch.saturating_add(1);
        self.push_frame(ThreadNotification::Lagged { dropped });
    }

    /// The status stream only forwards status frames; content is delivered by the ChatView window.
    ///
    /// The projection owner's feed is already content-free — the same bodies are written once into
    /// the shared session and read from that one window — so no `state_only` end-of-pipe filter is
    /// needed and no content revision hole can be manufactured here. 状态流只保留活动、交互、运行时
    /// 与 lagged 事实：窗口版本不属于状态流，正文更新不会再额外走一遍状态流。
    fn push(&mut self, event: LiveEvent) {
        let notification = match event {
            LiveEvent::Turn { turn, event } => {
                // The retained terminal Turn this subscription already delivered is the frame this
                // one stands in for; the client's last-turn fact already is that Turn, so a second
                // copy would only spend a revision without carrying a new fact. It also keeps a
                // frame the authoritative snapshot had already covered from making the finished Turn
                // look active again.
                if self.terminal_delivered(&turn) {
                    return;
                }
                if matches!(&turn.state, pl_protocol::TurnState::Running(_)) {
                    self.active_turn = Some(turn.id.clone());
                } else if turn.state.is_terminal() {
                    self.delivered_terminal = Some((turn.id.clone(), turn.revision));
                    // Only the Turn this subscriber currently knows as active leaves that state: a
                    // late metadata frame of an *older* finished Turn must not make a newer running
                    // Turn disappear from the authoritative snapshot contract.
                    if self.active_turn.as_deref() == Some(turn.id.as_str()) {
                        self.active_turn = None;
                    }
                }
                match event {
                    TurnEvent::Started => ThreadNotification::TurnStarted { turn },
                    TurnEvent::Updated => ThreadNotification::TurnUpdated { turn },
                    TurnEvent::Completed => ThreadNotification::TurnCompleted { turn },
                }
            }
            LiveEvent::Interaction(interaction) => {
                ThreadNotification::InteractionChanged { interaction }
            }
        };
        self.enqueue(notification);
    }

    /// Wraps one typed change into a continuous envelope: `base_revision` is the previous
    /// watermark and `revision` advances it by exactly one.
    fn enqueue(&mut self, notification: ThreadNotification) {
        // 未交付的观察帧就地合并：活动帧只带小型摘要（同一身份最多留一条，身份变化必须按顺序交付），
        // 内容窗口帧只带版本（始终只留最新一条）。
        if self.coalesce_pending(&notification) {
            return;
        }
        // 有界观察通道：慢消费者不能把订阅内存拉成无界队列。超过固定上限时丢弃已排队帧、
        // 提升 epoch，并用一条 lagged 让客户端从数据库窗口重同步。
        if self.pending.len() >= LIVE_PENDING_FRAMES {
            let dropped = self.pending.len() as u64;
            self.pending.clear();
            self.observation_due = None;
            self.epoch = self.epoch.saturating_add(1);
            self.push_frame(ThreadNotification::Lagged { dropped });
        }
        self.push_frame(notification);
    }

    /// 把 `notification` 合并进队列尾部同类的未交付观察帧；合并成功时返回 `true`。
    fn coalesce_pending(&mut self, notification: &ThreadNotification) -> bool {
        let identity = match notification {
            ThreadNotification::ActivityChanged { activity } => {
                Some(activity.as_ref().map(|activity| activity.identity.as_str()))
            }
            _ => return false,
        };
        // Only the tail frame can be superseded: a later activity change replaces the previous one
        // this stream has not delivered yet, and anything older than that tail is already finalized
        // by an intervening frame. So this looks at exactly that one frame — never a loop.
        let Some(ThreadSubscriptionUpdate::Notification { notification: last }) =
            self.pending.back_mut()
        else {
            return false;
        };
        let mergeable = match (&last.notification, &identity) {
            (ThreadNotification::ActivityChanged { activity: old }, Some(identity)) => {
                old.as_ref().map(|old| old.identity.as_str()) == *identity
            }
            _ => false,
        };
        if mergeable {
            last.notification = notification.clone();
            last.emitted_at = crate::studio::unix_seconds();
            return true;
        }
        false
    }

    fn push_frame(&mut self, notification: ThreadNotification) {
        let base_revision = self.revision;
        self.revision = self.revision.saturating_add(1);
        self.pending
            .push_back(ThreadSubscriptionUpdate::Notification {
                notification: Box::new(ThreadNotificationEnvelope::new(
                    self.thread_id.clone(),
                    self.epoch,
                    base_revision,
                    self.revision,
                    crate::studio::unix_seconds(),
                    notification,
                )),
            });
    }

    /// 未交付帧是否全部是可按显示节流合并的观察帧（活动与存储状态）。
    ///
    /// 节流只推迟“还能被下一条覆盖”的观察帧；Turn、交互、运行时与 lagged 事实立即交付。
    fn has_only_coalescable_frames(&self) -> bool {
        self.pending.iter().all(|frame| {
            matches!(frame, ThreadSubscriptionUpdate::Notification { notification }
            if matches!(
                notification.notification,
                ThreadNotification::ActivityChanged { .. } | ThreadNotification::StorageChanged { .. }
            ))
        })
    }
}

impl StudioThreadSubscription {
    /// Receives the authoritative snapshot and the subsequent typed notifications.
    ///
    /// # Errors
    /// Returns malformed historical content or missing committed facts; no partial snapshot is emitted.
    pub async fn recv(&mut self) -> Result<Option<ThreadSubscriptionUpdate>> {
        let runtime = self.runtime.clone();
        let live = match &mut self.source {
            SubscriptionSource::Live(live) => live,
            SubscriptionSource::Retired(retired) => {
                if let Some(snapshot) = retired.snapshot.take() {
                    retired.revision = snapshot.revision;
                    return Ok(Some(ThreadSubscriptionUpdate::Snapshot { snapshot }));
                }
                // The child's newest finished Turn is the one Turn fact a retired history still owes
                // a subscriber that opened after it committed — the same one-shot `turnCompleted` a
                // live owner replays from its retained fact, so the client reads `lastTurn` from one
                // contract whether or not an owner exists.
                if let Some(turn) = retired.terminal_turn.take() {
                    let base_revision = retired.revision;
                    retired.revision = base_revision.saturating_add(1);
                    return Ok(Some(ThreadSubscriptionUpdate::Notification {
                        notification: Box::new(ThreadNotificationEnvelope::new(
                            retired.thread_id.clone(),
                            1,
                            base_revision,
                            retired.revision,
                            crate::studio::unix_seconds(),
                            ThreadNotification::TurnCompleted { turn },
                        )),
                    }));
                }
                // Immutable history has no producer. The transport cancels/drops this wait
                // when the client leaves; ending it would trigger GUI reconnect loops.
                return std::future::pending().await;
            }
        };
        loop {
            // The authoritative first frame is emitted before any owner frame: a subscriber that
            // registered its receiver before the baseline must not deliver a notification ahead of
            // the snapshot that frames it.
            if !live.started {
                return Ok(Some(live.open(&runtime).await?));
            }
            let step = live.drain_feed();
            // A commit or an owner snapshot may have changed the storage state; refresh it from the
            // typed coordinator facts before deciding whether the queue is only coalescable frames.
            live.refresh_observations();
            // A Turn that finished before this subscriber registered — or while it was parked — is
            // the owner's retained terminal fact, not something the client may infer from content.
            live.reflect_retained_turn();
            if !live.pending.is_empty() {
                // 未交付的一串帧里可能只有可合并的观察帧（活动/存储状态）。按显示节流等待
                // 期间继续消费投影帧，既不让每个 token 都单独成帧，也不把一个已提交的事实留在队列里
                // 等节流窗口。
                if live.has_only_coalescable_frames() {
                    let due = *live
                        .observation_due
                        .get_or_insert_with(|| Instant::now() + OBSERVATION_DELIVERY_INTERVAL);
                    while Instant::now() < due {
                        let remaining = due.saturating_duration_since(Instant::now());
                        match tokio::time::timeout(remaining, live.wait_feed_or_storage()).await {
                            Ok(FeedStep::Closed) | Err(_) => break,
                            Ok(_) => {}
                        }
                    }
                    live.refresh_observations();
                }
                live.observation_due = None;
                return Ok(live.pending.pop_front());
            }
            live.observation_due = None;
            if matches!(step, FeedStep::Closed) {
                return Ok(None);
            }
            // The projection owner released the feed, or the Thread was never observed: end the
            // subscription instead of parking a stream that can no longer receive a fact.
            if matches!(live.wait_feed_or_storage().await, FeedStep::Closed) {
                return Ok(None);
            }
        }
    }
}

impl StudioRuntime {
    /// Observes canonical facts; retired child history never activates an execution owner.
    pub async fn subscribe_thread(
        &self,
        request: pl_protocol::ThreadSubscriptionRequest,
    ) -> Result<StudioThreadSubscription> {
        let _lifecycle_guard = self.lifecycle_lock.lock().await;
        let residency_pin = self.residency.pin_many([request.thread_id.clone()]);
        let thread = self.read_protocol_thread(&request.thread_id).await?;
        let source = if thread.parent_thread_id.is_some() && thread.role == "planner" {
            let snapshot = self.thread_snapshot(&request.thread_id).await?;
            // A retired child has no owner to retain its last Turn and its subscription emits only
            // this snapshot, so the newest finished Turn is read once here — bounded, body-free — and
            // delivered as the same one-shot `turnCompleted` the live contract replays. A child whose
            // snapshot still names an active Turn keeps that fact authoritative, exactly like a live
            // subscription, and no finished Turn is replayed over it.
            let terminal_turn = if snapshot.active_turn.is_none() {
                self.store
                    .history(&request.thread_id)
                    .await?
                    .newest_terminal_turn()
                    .await?
            } else {
                None
            };
            SubscriptionSource::Retired(Box::new(RetiredSubscription {
                snapshot: Some(Box::new(snapshot)),
                terminal_turn,
                thread_id: request.thread_id.clone(),
                revision: 0,
            }))
        } else {
            let handle = self.ensure_thread_owner(&request.thread_id).await?;
            // 先注册投影帧接收端，再捕获基线：观测任务在建线程时就接管了唯一投影，订阅的
            // 首个快照就是注册后可见的状态，注册与快照之间提交的效果仍然会被投递（其中已被
            // 快照覆盖的帧按 effect 序号丢弃）。反过来（先快照再订阅）会留下一个很窄的窗口，
            // 该窗口里提交的效果既不在基线里也不会被投递，只能靠有界 effect 窗口兜底。
            let live_feed = self.thread_observations.live_feed(&request.thread_id);
            let feed = live_feed.as_ref().map(|feed| feed.subscribe());
            // The owner publishes the typed storage state; this subscriber only mirrors it.
            let storage_feed = live_feed.as_ref().map(|feed| feed.storage());
            // The owner also retains its newest terminal Turn, so a subscription that opens after
            // that commit restores the last-turn fact instead of reporting none.
            let last_turn = live_feed.as_ref().map(|feed| feed.last_turn());
            let baseline = handle.snapshot();
            let applied = baseline.commit_sequence;
            SubscriptionSource::Live(Box::new(LiveSubscription {
                handle,
                thread,
                feed,
                baseline: Some(baseline.clone()),
                pending: VecDeque::new(),
                observation_due: None,
                applied,
                revision: applied,
                epoch: 1,
                started: false,
                thread_id: request.thread_id.clone(),
                activity: None,
                storage_feed,
                storage: None,
                last_turn,
                active_turn: None,
                delivered_terminal: None,
            }))
        };
        Ok(StudioThreadSubscription {
            source,
            runtime: self.clone(),
            _residency_pin: residency_pin,
        })
    }

    /// Reads live or cold facts without executing a model, tool or recovery continuation.
    pub async fn thread_snapshot(&self, thread_id: &str) -> Result<pl_protocol::ThreadSnapshot> {
        let thread = self.read_protocol_thread(thread_id).await?;
        let state = self.read_thread_state(thread_id).await?;
        let handle = self
            .threads
            .observed_threads()
            .into_iter()
            .find(|(id, _)| id == thread_id)
            .map(|(_, handle)| handle);
        let usage = match handle {
            Some(handle) => self.usage_through_owner(thread_id, &state, &handle).await?,
            None => self.usage_summary(thread_id, &state),
        };
        let persistence = self.thread_persistence_snapshot(thread_id);
        let mut snapshot = crate::studio::thread_projection::project_snapshot(
            thread,
            &state,
            &usage,
            persistence.as_ref(),
        )?;
        self.annotate_model_route(&mut snapshot)?;
        Ok(snapshot)
    }

    /// 该 Thread 的 typed 持久化快照；没有登记过持久化的 Thread 为 `None`（未知不等于健康）。
    ///
    /// 这是 [`pl_protocol::ThreadStorageState`] 的 typed 来源：故障类别、代数与水位都来自协调器
    /// 的类型化值，不是错误文本的解析结果。
    pub(in crate::studio) fn thread_persistence_snapshot(
        &self,
        thread_id: &str,
    ) -> Option<pl_protocol::ThreadPersistenceSnapshot> {
        self.store
            .thread_persistence()
            .queue_snapshot()
            .threads
            .into_iter()
            .find(|snapshot| snapshot.thread_id == thread_id)
    }

    /// 按活动身份读取当前活动的完整内容事实（reasoning、输出正文、工具参数与流式输出）。
    ///
    /// 只读且**纯内存**：活动与活动的有界详情由 owner 助手
    /// [`crate::studio::thread_projection::ActivityProjection`] 从驻留 owner 已发布的
    /// `ThreadSnapshot` 推导——不读 `Session`、不做 SQL、不整 Turn 回读历史、不读 Thread 目录、
    /// 不激活 owner、不 flush writer、不改变任何 revision。身份不再成立时给出明确的
    /// `Superseded` / `Ended` 生命周期结果，而不是回退到旧活动的正文；未知 Thread 明确报
    /// `NotFound`。
    ///
    /// # Errors
    ///
    /// 未知 Thread。
    pub async fn read_thread_activity_detail(
        &self,
        thread_id: &str,
        activity_id: &str,
    ) -> Result<ThreadActivityDetail> {
        let Some(handle) = self
            .threads
            .observed_threads()
            .into_iter()
            .find(|(id, _)| id == thread_id)
            .map(|(_, handle)| handle)
        else {
            // 没有驻留 owner 就没有“当前活动”：不激活执行，也不读 durable 正文。存在性只按
            // canonical 目录确认（内存优先），未知身份明确 NotFound。
            return match self.read_protocol_thread(thread_id).await {
                Ok(_) => Ok(ThreadActivityDetail::Ended {
                    thread_id: thread_id.to_owned(),
                    activity_id: activity_id.to_owned(),
                }),
                Err(_) => Err(anyhow::Error::new(
                    pl_protocol::studio::StudioError::not_found("Thread"),
                )),
            };
        };
        let state = handle.snapshot();
        // 活动与详情都由同一个纯内存 owner 助手推导：只吃 owner 已发布快照，不读 Session、不做 SQL、
        // 不整 Turn 回读历史。推导结果与观察任务持有的那份一致（同一纯函数、同一快照事实）。
        let mut projection = crate::studio::thread_projection::ActivityProjection::default();
        let _ = projection.observe(thread_id, &state);
        Ok(match projection.detail(activity_id) {
            ActivityDetailRead::Current(detail) => ThreadActivityDetail::Current {
                activity: detail.activity,
                reasoning: detail.content.reasoning,
                response: detail.content.response,
                tools: detail.content.tools,
            },
            ActivityDetailRead::Superseded {
                activity,
                requested_activity_id,
            } => ThreadActivityDetail::Superseded {
                activity,
                requested_activity_id,
            },
            ActivityDetailRead::Ended => ThreadActivityDetail::Ended {
                thread_id: thread_id.to_owned(),
                activity_id: activity_id.to_owned(),
            },
        })
    }

    /// Cumulative usage summary the projection must consume for one Thread.
    ///
    /// The writer is the single folder of committed effects, so its reported summary is the value a
    /// hot increment, a reconnect and a cold restore must agree on. A checkpoint that was loaded
    /// before the writer reported seeds the same value, never a re-aggregation of resident attempts.
    pub(in crate::studio) fn usage_summary(
        &self,
        thread_id: &str,
        state: &ThreadSnapshot,
    ) -> pl_core::thread::UsageSummary {
        let coordinator = self.store.thread_persistence();
        if let Some(summary) = coordinator.usage(thread_id) {
            return summary;
        }
        coordinator.seed_usage(thread_id, state.usage_summary.clone());
        state.usage_summary.clone()
    }

    /// Include committed owner facts that the asynchronous history writer has not folded yet.
    /// This is only a presentation snapshot; it never advances the writer's durable watermark.
    async fn usage_through_owner(
        &self,
        thread_id: &str,
        state: &ThreadSnapshot,
        handle: &ThreadHandle,
    ) -> Result<pl_core::thread::UsageSummary> {
        let mut summary = self.usage_summary(thread_id, state);
        while summary.applied_sequence < state.commit_sequence {
            let page = handle
                .effect_page(summary.applied_sequence, NonZeroUsize::new(128).unwrap())
                .await
                .context("committed usage effect is no longer available")?;
            let mut progressed = false;
            for effect in page {
                if effect.sequence > state.commit_sequence {
                    break;
                }
                anyhow::ensure!(
                    effect.sequence == summary.applied_sequence + 1,
                    "committed usage effect has a gap"
                );
                crate::studio::thread_projection::fold_effect_accounting(&mut summary, &effect)?;
                progressed = true;
            }
            anyhow::ensure!(progressed, "committed usage effect has no progress");
        }
        Ok(summary)
    }

    pub(in crate::studio) async fn read_thread_state(
        &self,
        thread_id: &str,
    ) -> Result<ThreadSnapshot> {
        if let Some((_, handle)) = self
            .threads
            .observed_threads()
            .into_iter()
            .find(|(id, _)| id == thread_id)
        {
            let snapshot = handle.snapshot();
            self.store
                .thread_persistence()
                .seed_usage(thread_id, snapshot.usage_summary.clone());
            return Ok(snapshot);
        }
        let thread = self.read_protocol_thread(thread_id).await?;
        let checkpoint =
            crate::studio::thread_factory::recovery::load_checkpoint(&self.store, &thread)
                .await?
                .ok_or_else(|| anyhow::anyhow!("Thread has no saved checkpoint"))?;
        self.store
            .thread_persistence()
            .seed_usage(thread_id, checkpoint.state.usage_summary.clone());
        Ok(checkpoint.state)
    }

    fn annotate_model_route(&self, snapshot: &mut pl_protocol::ThreadSnapshot) -> Result<()> {
        let Some(route) = snapshot
            .runtime
            .as_mut()
            .and_then(|runtime| runtime.model_route.as_mut())
        else {
            return Ok(());
        };
        let selector = pl_model::config::ModelRouteConfig {
            provider: pl_model::config::ProviderId::new(route.provider_id.clone())?,
            model: route.model.clone(),
            effort: route
                .effort
                .clone()
                .map(pl_model::config::ReasoningEffort::new),
        };
        let config = self.config_runtime.read()?.config;
        let role = if snapshot.thread.parent_thread_id.is_none() {
            crate::config::StudioRole::Planner.id()
        } else {
            pl_protocol::AgentRoleId::new(snapshot.thread.role.clone())?
        };
        let resolved = config
            .models
            .resolve_route(role, &selector)
            .and_then(|resolved| {
                if snapshot.thread.parent_thread_id.is_none() {
                    let mode = self.thread_modes.snapshot().mode(&snapshot.thread.mode);
                    crate::mode::validate_thread_mode_model(
                        mode.as_ref().map(|mode| mode.as_ref()),
                        &resolved.model,
                    )?;
                }
                Ok(resolved)
            });
        match resolved {
            Ok(_) => {
                route.available = true;
                route.unavailable_reason = None;
            }
            Err(error) => {
                route.available = false;
                route.unavailable_reason = Some(error.to_string());
            }
        }
        Ok(())
    }
}

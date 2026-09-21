//! Product observations own no execution tasks and project a single core commit watermark.
//!
//! A live subscription registers itself before the open/reconnect persistence barrier, then emits
//! one authoritative `snapshot` first and typed `notification` frames afterwards. Notifications
//! fold the bounded live effect window into the same canonical projection the history writer uses;
//! a truncated window or a projection hole becomes `lagged`, and the client resynchronizes from
//! the database window instead of splicing a hole.
use super::StudioRuntime;
use anyhow::{Context, Result};
use pl_core::thread::{ThreadHandle, ThreadSnapshot, ThreadSubscription};
use pl_protocol::{ThreadNotification, ThreadNotificationEnvelope, ThreadSubscriptionUpdate};
use std::{collections::VecDeque, num::NonZeroUsize};

use crate::studio::thread_projection::{LiveEvent, LiveProjection, TurnEvent};

/// Fixed upper bound on frames a live subscription may queue for one slow consumer.
///
/// A subscription is a bounded observation channel, not a durable log: when the client stops
/// draining, the queued frames are dropped and one `lagged` frame (with a new epoch) tells it to
/// resynchronize from the database window instead of letting this queue grow with history.
const LIVE_PENDING_FRAMES: usize = 512;

/// Snapshot stream backed by the canonical owner or immutable retired-child history.
/// Dropping it stops only observation.
pub struct StudioThreadSubscription {
    runtime: StudioRuntime,
    source: SubscriptionSource,
    _residency_pin: super::residency::ThreadResidencyPins,
}

enum SubscriptionSource {
    Live(Box<LiveSubscription>),
    Retired(Option<Box<pl_protocol::ThreadSnapshot>>),
}

struct LiveSubscription {
    handle: ThreadHandle,
    observations: ThreadSubscription,
    thread: pl_protocol::Thread,
    /// Durable history reader captured when the subscription opened.
    ///
    /// The durable ordinal phase and the per-effect identity lookups are read through this one
    /// read-only handle, so a streaming subscription never opens a new database connection per frame
    /// and a long write transaction can never queue these reads behind it.
    history: Option<crate::studio::storage::history::HistoryStore>,
    /// The Thread's single ordered history writer, captured when the subscription opened.
    ///
    /// Ordinal *reservation* goes through this handle — the same one the Thread's effect commit
    /// uses — so a live frame and the effect that finalizes it are never two independent SQLite
    /// writers racing for `history.sqlite`'s write lock.
    history_writer: Option<crate::studio::storage::history::HistoryStore>,
    /// Owner state captured right after receiver registration and before the persistence barrier;
    /// the first frame reports it so effects committed past it are still delivered as notifications.
    baseline: Option<ThreadSnapshot>,
    /// Canonical effect projection; seeded by the first frame from the durable ordinal phase.
    projection: Option<LiveProjection>,
    pending: VecDeque<ThreadSubscriptionUpdate>,
    /// Highest committed effect sequence already projected into notifications.
    applied: u64,
    /// Notification watermark: the strict `revision` counter seeded from the first snapshot.
    revision: u64,
    /// Broadcast lifecycle identity; increments when the effect window forces a resync.
    epoch: u64,
    started: bool,
    thread_id: String,
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
        // 历史数据库是 item ordinal 的唯一分配者：订阅只消费它已分配的相位，使实时条目与数据库
        // 分页条目落在同一条 ordinal 序列上。这里不提供任何猜测回退——读不到分配相位就让订阅
        // 失败（客户端会重新订阅），而不是编造数据库之后会分配给别的条目的 ordinal。
        let history = runtime.ensure_timeline_history(&self.thread_id).await?;
        let history_writer = runtime
            .ensure_timeline_history_writer(&self.thread_id)
            .await?;
        self.projection = Some(LiveProjection::seed(state.clone()));
        self.history = Some(history);
        self.history_writer = Some(history_writer);
        self.applied = state.commit_sequence;
        self.revision = state.commit_sequence;
        let usage = runtime.usage_summary(&self.thread_id, &state);
        let mut snapshot = crate::studio::thread_projection::project_snapshot(
            self.thread.clone(),
            &state,
            &usage,
        )?;
        runtime.annotate_model_route(&mut snapshot)?;
        Ok(ThreadSubscriptionUpdate::Snapshot {
            snapshot: Box::new(snapshot),
        })
    }

    /// Projects every effect committed since the last watermark, then the live streaming preview.
    /// A hole in the effect window becomes `lagged` so the client resynchronizes from SQL.
    async fn advance(&mut self, runtime: &StudioRuntime, state: &ThreadSnapshot) -> Result<()> {
        let target = state.commit_sequence;
        // 实时投影按 durable history 已分配的 ordinal 相位对齐 item identity，与 history writer
        // 走同一张表，所以重连重放或窗口淘汰都不会重新编号。
        let history = self
            .history
            .clone()
            .context("Thread live subscription has no opened history reader")?;
        // 预留走的句柄与该 Thread 的 effect commit 是同一个写者；读走上面的只读句柄。
        let history_writer = self
            .history_writer
            .clone()
            .context("Thread live subscription has no shared history writer")?;
        // 累计摘要从写者报告的权威值出发；每个 effect 在本地再折叠一次（幂等），因此实时
        // runtime 帧与落库后的 checkpoint 摘要同值。
        let mut usage = runtime.usage_summary(&self.thread_id, state);
        if target > self.applied {
            self.thread = runtime.read_protocol_thread(&self.thread_id).await?;
            while self.applied < target {
                let limit = NonZeroUsize::new(128).expect("constant is nonzero");
                let page = match self.handle.effect_page(self.applied, limit).await {
                    Ok(page) if !page.is_empty() => page,
                    _ => {
                        self.lagged(runtime, state).await?;
                        return Ok(());
                    }
                };
                let mut progressed = false;
                for effect in page {
                    if effect.sequence > target {
                        break;
                    }
                    if crate::studio::thread_projection::fold_effect_accounting(&mut usage, &effect)
                        .is_err()
                    {
                        self.lagged(runtime, state).await?;
                        return Ok(());
                    }
                    let projected = {
                        let projection = self
                            .projection
                            .as_mut()
                            .context("Thread live projection was never seeded")?;
                        projection
                            .advance(&history, &history_writer, &usage, &self.thread, &effect)
                            .await
                    };
                    match projected {
                        Ok(events) => {
                            self.applied = effect.sequence;
                            progressed = true;
                            for event in events {
                                self.push(event);
                            }
                        }
                        Err(error) => {
                            tracing::warn!(
                                thread_id = self.thread_id,
                                sequence = effect.sequence,
                                %error,
                                "Thread live projection closed over a folded-fact hole"
                            );
                            self.lagged(runtime, state).await?;
                            return Ok(());
                        }
                    }
                }
                if !progressed {
                    self.lagged(runtime, state).await?;
                    return Ok(());
                }
            }
        }
        let streamed = {
            // 预览的 canonical revision 由订阅的已投影水位提供：投影自己不再持有一份可能漂移的
            // 计数器，预览帧因此始终不高于收束同一 identity 的 effect revision。
            let applied = self.applied;
            let projection = self
                .projection
                .as_mut()
                .context("Thread live projection was never seeded")?;
            projection
                .stream(&history, &history_writer, &self.thread, state, applied)
                .await
        };
        for event in streamed? {
            self.push(event);
        }
        Ok(())
    }

    /// Rebases on the current owner state and signals a gap; the client resynchronizes from SQL.
    async fn lagged(&mut self, runtime: &StudioRuntime, state: &ThreadSnapshot) -> Result<()> {
        let dropped = state.commit_sequence.saturating_sub(self.applied);
        self.applied = state.commit_sequence;
        // 重新对齐必须继续消费 history 的分配相位；读不到就向上报告失败，绝不猜 ordinal。
        let history = runtime.ensure_timeline_history(&self.thread_id).await?;
        // 重同步后仍复用该 Thread 的唯一有序写者，绝不在预览路径上另开一条 SQLite writer。
        let history_writer = runtime
            .ensure_timeline_history_writer(&self.thread_id)
            .await?;
        self.projection = Some(LiveProjection::seed(state.clone()));
        self.history = Some(history);
        self.history_writer = Some(history_writer);
        self.epoch = self.epoch.saturating_add(1);
        self.enqueue(ThreadNotification::Lagged { dropped });
        Ok(())
    }

    fn push(&mut self, event: LiveEvent) {
        let notification = match event {
            LiveEvent::Turn { turn, event } => match event {
                TurnEvent::Started => ThreadNotification::TurnStarted { turn },
                TurnEvent::Updated => ThreadNotification::TurnUpdated { turn },
                TurnEvent::Completed => ThreadNotification::TurnCompleted { turn },
            },
            LiveEvent::Item(item) => {
                if item.state().is_terminal() {
                    ThreadNotification::ItemCompleted { item }
                } else {
                    ThreadNotification::ItemStarted { item }
                }
            }
            LiveEvent::Delta(delta) => ThreadNotification::ItemDelta { delta },
            LiveEvent::Interaction(interaction) => {
                ThreadNotification::InteractionChanged { interaction }
            }
            LiveEvent::Runtime(runtime) => ThreadNotification::ThreadRuntimeUpdated { runtime },
        };
        self.enqueue(notification);
    }

    /// Wraps one typed change into a continuous envelope: `base_revision` is the previous
    /// watermark and `revision` advances it by exactly one.
    fn enqueue(&mut self, notification: ThreadNotification) {
        // 有界观察通道：慢消费者不能把订阅内存拉成无界队列。超过固定上限时丢弃已排队帧、
        // 提升 epoch，并用一条 lagged 让客户端从数据库窗口重同步。
        if self.pending.len() >= LIVE_PENDING_FRAMES {
            let dropped = self.pending.len() as u64;
            self.pending.clear();
            self.epoch = self.epoch.saturating_add(1);
            self.push_frame(ThreadNotification::Lagged { dropped });
        }
        self.push_frame(notification);
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
            SubscriptionSource::Retired(snapshot) => {
                if let Some(snapshot) = snapshot.take() {
                    return Ok(Some(ThreadSubscriptionUpdate::Snapshot { snapshot }));
                }
                // Immutable history has no producer. The transport cancels/drops this wait
                // when the client leaves; ending it would trigger GUI reconnect loops.
                return std::future::pending().await;
            }
        };
        loop {
            if let Some(update) = live.pending.pop_front() {
                return Ok(Some(update));
            }
            if !live.started {
                return Ok(Some(live.open(&runtime).await?));
            }
            let Some(state) = live.observations.next().await else {
                return Ok(None);
            };
            live.advance(&runtime, &state).await?;
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
            SubscriptionSource::Retired(Some(Box::new(
                self.thread_snapshot(&request.thread_id).await?,
            )))
        } else {
            let handle = self.ensure_thread_owner(&request.thread_id).await?;
            // 先注册事件接收端，再捕获基线：订阅的首个 `initial` 帧就是注册后可见的状态，
            // 因此基线永不晚于首个被投递的状态。反过来（先快照再订阅）会留下一个很窄的
            // 窗口——快照与接收端注册之间提交的效果既不在基线里，也不会作为“新变化”被投递，
            // 只能靠有界 effect 窗口兜底；按注册优先消除该窗口。
            let observations = handle.subscribe();
            let baseline = handle.snapshot();
            SubscriptionSource::Live(Box::new(LiveSubscription {
                observations,
                handle,
                thread,
                baseline: Some(baseline.clone()),
                // 首个帧在持久化屏障之后才按 history 的分配相位播种投影，这里不预先猜相位。
                projection: None,
                history: None,
                history_writer: None,
                pending: VecDeque::new(),
                applied: baseline.commit_sequence,
                revision: baseline.commit_sequence,
                epoch: 1,
                started: false,
                thread_id: request.thread_id.clone(),
            }))
        };
        // 先订阅（receiver 已注册），再让活跃草稿通过固定持久化屏障；此后 GUI 读取的
        // `listTimelineItems(Latest)` 才是权威窗口，期间到达的实时事件按身份与 revision 合并。
        self.await_timeline_barrier(&request.thread_id).await?;
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
        let usage = self.usage_summary(thread_id, &state);
        let mut snapshot =
            crate::studio::thread_projection::project_snapshot(thread, &state, &usage)?;
        self.annotate_model_route(&mut snapshot)?;
        Ok(snapshot)
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

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU8, AtomicU64, Ordering};
use std::time::Instant;

use flutter_rust_bridge::frb;
use futures::FutureExt;
use futures::future::{BoxFuture, Shared};
use tokio::sync::{Mutex, mpsc};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use crate::api::studio::bridge_runtime::{active_bridge, current_bridge};
use crate::api::studio::convert::event::bridge_product_event;
use crate::api::studio::convert::thread_stream::bridge_thread_update;
use crate::api::studio::types::{
    BridgeError, BridgeProductEventEnvelope, BridgeShutdownIssue, BridgeShutdownProgress,
    BridgeThreadSubscriptionUpdate,
};
use crate::frb_generated::StreamSink;
use pl_studio_runtime::StudioShutdownProgress;

#[derive(Debug, Clone)]
pub enum BridgeThreadStreamEnvelope {
    Data {
        update: Box<BridgeThreadSubscriptionUpdate>,
    },
    Failure {
        error: BridgeError,
    },
    Closed,
}

#[derive(Debug, Clone)]
pub enum BridgeProductStreamEnvelope {
    Data {
        event: Box<BridgeProductEventEnvelope>,
    },
    Failure {
        error: BridgeError,
    },
    Closed,
}

/// Shared completion of one owned subscription task.
///
/// `Ok` means the task really stopped (a cancelled task is the expected terminal state); `Err`
/// carries a stable, non-sensitive summary when it panicked. The result is retained inside the
/// subscription, so repeated and concurrent waiters observe the same outcome: a first waiter can
/// never consume a failure and leave a later waiter with a false success.
type SubscriptionCompletion = Shared<BoxFuture<'static, Result<(), Arc<String>>>>;

/// `SubscriptionCompletionFacts` values. The not-started state is also the `Default`, so a sink
/// that was never opened counts as ended without a separate "opened" flag.
const OWNED_NOT_STARTED: u8 = 0;
const OWNED_PENDING: u8 = 1;
const OWNED_SUCCEEDED: u8 = 2;
const OWNED_FAILED: u8 = 3;

#[derive(Debug, Clone, Copy)]
enum SubscriptionOwner {
    Producer,
    Sink,
}

/// Owner-published completion facts for one subscription.
///
/// The values are written by the owned observer task the moment the real task ends, never by a
/// waiter's first poll, so `is_settled` is a cheap, non-blocking read that reflects the real state
/// even when no waiter ever observes the completion.
///
/// `#[frb(ignore)]` keeps this pure internal ownership state out of the generated API: `cargo-expand`
/// turns `#[derive(Default)]` into an `impl Default`, and the code generator otherwise treats
/// `Default::default` as a public constructor and exports the private type as an opaque handle.
#[frb(ignore)]
#[derive(Default)]
struct SubscriptionCompletionFacts {
    producer: AtomicU8,
    sink: AtomicU8,
}

impl SubscriptionCompletionFacts {
    /// Only a fully successful subscription whose owners all confirmed is prunable. A task panic or
    /// an unconfirmed owner keeps the strong registry owner so the failure is never dropped.
    fn is_settled(&self) -> bool {
        self.producer.load(Ordering::SeqCst) == OWNED_SUCCEEDED
            && matches!(
                self.sink.load(Ordering::SeqCst),
                OWNED_NOT_STARTED | OWNED_SUCCEEDED
            )
    }
}

/// Wraps a spawned subscription task into a shared completion whose terminal fact is published by
/// an owned observer task.
///
/// The observer owns the real `JoinHandle`, so the fact and the retained result are recorded the
/// moment the task ends — even when no waiter ever polls the returned completion, and even when a
/// bounded join times out and drops its clone. A task that panics is a real stop failure; only an
/// aborted task reports success, because cancellation is the expected terminal state. The raw panic
/// payload is never propagated: a stable summary and a correlation id are kept instead, so the
/// failure still reaches the same report without leaking caller-controlled text.
fn owned_completion(
    handle: JoinHandle<()>,
    subscription_id: u64,
    facts: Arc<SubscriptionCompletionFacts>,
    owner: SubscriptionOwner,
) -> SubscriptionCompletion {
    let (published_tx, published_rx) = tokio::sync::oneshot::channel::<Result<(), Arc<String>>>();
    tokio::spawn(async move {
        let stable_message = match owner {
            SubscriptionOwner::Producer => {
                format!("Studio subscription {subscription_id} producer task panicked")
            }
            SubscriptionOwner::Sink => {
                format!("Studio subscription {subscription_id} sink task panicked")
            }
        };
        let outcome = match handle.await {
            Ok(()) => Ok(()),
            Err(error) if error.is_cancelled() => Ok(()),
            Err(_) => Err(Arc::new(stable_message)),
        };
        if outcome.is_err() {
            // Record the key safety diagnostic the moment the owner actually fails, instead of
            // waiting for a future exit to observe it. Stable identifiers and a forced backtrace
            // only; the (possibly caller-controlled) failure text is never written to the log.
            let code = match owner {
                SubscriptionOwner::Producer => "producerTaskFailed",
                SubscriptionOwner::Sink => "sinkTaskFailed",
            };
            let correlation_id = pl_protocol::studio::StudioError::internal().correlation_id;
            tracing::error!(
                pid = std::process::id(),
                stage = "subscriptions",
                code = %code,
                resource = subscription_id,
                correlation_id = %correlation_id,
                backtrace = %std::backtrace::Backtrace::force_capture(),
                "Studio subscription owner task failed"
            );
        }
        let status = if outcome.is_ok() {
            OWNED_SUCCEEDED
        } else {
            OWNED_FAILED
        };
        match owner {
            SubscriptionOwner::Producer => facts.producer.store(status, Ordering::SeqCst),
            SubscriptionOwner::Sink => facts.sink.store(status, Ordering::SeqCst),
        }
        // The receiver may already be dropped; the fact above is still published.
        let _ = published_tx.send(outcome);
    });
    async move {
        published_rx.await.unwrap_or_else(|_| {
            // The observer ended without publishing; never report a clean stop.
            Err(Arc::new(
                "Studio subscription completion was never published".to_string(),
            ))
        })
    }
    .boxed()
    .shared()
}

pub struct BridgeEventSubscription {
    inner: Arc<BridgeSubscriptionInner>,
}

struct BridgeSubscriptionInner {
    id: u64,
    kind: BridgeSubscriptionKind,
    cancel: CancellationToken,
    producer_task: Mutex<Option<SubscriptionCompletion>>,
    sink_task: Mutex<Option<SubscriptionCompletion>>,
    thread_receiver: Mutex<Option<mpsc::Receiver<BridgeThreadStreamEnvelope>>>,
    product_receiver: Mutex<Option<mpsc::Receiver<BridgeProductStreamEnvelope>>>,
    shutdown_receiver: Mutex<Option<tokio::sync::broadcast::Receiver<StudioShutdownProgress>>>,
    /// Owner-published completion facts for the producer/sink tasks; also the non-blocking pruning
    /// predicate for the registry strong owner.
    facts: Arc<SubscriptionCompletionFacts>,
}

#[derive(Debug, Clone)]
enum BridgeSubscriptionKind {
    Thread { thread_id: String },
    Product,
    Shutdown,
}

/// Builds one typed shutdown issue for a subscription-cancellation failure.
///
/// `code` is a stable diagnostic string, not a typed error enum; `correlation_id` links the
/// synchronous diagnostics.
fn shutdown_issue(stage: &str, code: &str, message: &str) -> BridgeShutdownIssue {
    BridgeShutdownIssue {
        stage: stage.to_string(),
        code: code.to_string(),
        message: message.to_string(),
        retryable: true,
        correlation_id: pl_protocol::studio::StudioError::internal().correlation_id,
    }
}

impl BridgeEventSubscription {
    /// Cancels this subscription and reports the *real* stop result.
    ///
    /// A panicked producer/sink task is surfaced as a typed `BridgeError` instead of being hidden
    /// behind a clean `Future<void>`, so the caller's `onError` path observes the same failure the
    /// exit report carries. Only a subscription that really stopped is removed from its registry
    /// strong owner; a failure keeps the owner so a later exit pass still observes it.
    pub async fn cancel(&self) -> Result<(), BridgeError> {
        let result = self.inner.cancel_and_wait().await;
        if result.is_ok()
            && let Some(bridge) = current_bridge()
        {
            bridge.subscriptions.unregister(self.inner.id).await;
        }
        result.map_err(|error| {
            BridgeError::from(pl_protocol::studio::StudioError::new(
                pl_protocol::studio::StudioErrorCode::Internal,
                format!(
                    "Studio subscription {} did not stop cleanly: {error}",
                    self.inner.id
                ),
                false,
            ))
        })
    }

    pub async fn thread_stream(
        &self,
        sink: StreamSink<BridgeThreadStreamEnvelope>,
    ) -> Result<(), BridgeError> {
        if !matches!(self.inner.kind, BridgeSubscriptionKind::Thread { .. }) {
            return Err(BridgeError::invalid_argument(
                "product subscription cannot open a Thread stream",
            ));
        }
        let mut receiver = self
            .inner
            .thread_receiver
            .lock()
            .await
            .take()
            .ok_or_else(|| {
                BridgeError::invalid_argument("Thread stream can only be opened once")
            })?;
        let cancel = self.inner.cancel.clone();
        let id = self.inner.id;
        let facts = Arc::clone(&self.inner.facts);
        self.inner.facts.sink.store(OWNED_PENDING, Ordering::SeqCst);
        let task = tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = cancel.cancelled() => break,
                    envelope = receiver.recv() => {
                        let Some(envelope) = envelope else {
                            break;
                        };
                        if sink.add(envelope).is_err() {
                            cancel.cancel();
                            break;
                        }
                    }
                }
            }
        });
        *self.inner.sink_task.lock().await =
            Some(owned_completion(task, id, facts, SubscriptionOwner::Sink));
        Ok(())
    }

    pub async fn product_stream(
        &self,
        sink: StreamSink<BridgeProductStreamEnvelope>,
    ) -> Result<(), BridgeError> {
        if !matches!(self.inner.kind, BridgeSubscriptionKind::Product) {
            return Err(BridgeError::invalid_argument(
                "Thread subscription cannot open a product stream",
            ));
        }
        let mut receiver = self
            .inner
            .product_receiver
            .lock()
            .await
            .take()
            .ok_or_else(|| {
                BridgeError::invalid_argument("product stream can only be opened once")
            })?;
        let cancel = self.inner.cancel.clone();
        let id = self.inner.id;
        let facts = Arc::clone(&self.inner.facts);
        self.inner.facts.sink.store(OWNED_PENDING, Ordering::SeqCst);
        let task = tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = cancel.cancelled() => break,
                    envelope = receiver.recv() => {
                        let Some(envelope) = envelope else {
                            break;
                        };
                        if sink.add(envelope).is_err() {
                            cancel.cancel();
                            break;
                        }
                    }
                }
            }
        });
        *self.inner.sink_task.lock().await =
            Some(owned_completion(task, id, facts, SubscriptionOwner::Sink));
        Ok(())
    }
}

impl Drop for BridgeEventSubscription {
    fn drop(&mut self) {
        self.inner.cancel.cancel();
    }
}

impl BridgeSubscriptionInner {
    /// Signals cancellation for this subscription without joining any owned task.
    ///
    /// Split from the join so every independent owner can be signalled before any bounded join
    /// blocks: a slow join of one owner must never keep another owner from being cancelled within
    /// the shared deadline.
    fn signal_cancel(&self) {
        match &self.kind {
            BridgeSubscriptionKind::Thread { thread_id } => {
                tracing::trace!(subscription_id = self.id, %thread_id, "cancelling Studio Thread subscription");
            }
            BridgeSubscriptionKind::Shutdown => {
                tracing::trace!("cancelling Studio shutdown progress subscription");
            }
            BridgeSubscriptionKind::Product => {
                tracing::trace!(
                    subscription_id = self.id,
                    "cancelling Studio product subscription"
                );
            }
        }
        self.cancel.cancel();
    }

    /// Cheap, non-blocking read of the owner-published completion facts; used to decide whether the
    /// registry strong owner may be pruned.
    fn is_settled(&self) -> bool {
        self.facts.is_settled()
    }

    /// Observes this subscription's owned producer/sink completions and returns the first real
    /// failure.
    ///
    /// The completions are shared, retained values owned by the observer tasks: repeated and
    /// concurrent callers (Dart `cancel()` and the exit aggregation) observe the same outcome, and
    /// a bounded join that times out keeps the strong registry owner and the real handle so a later
    /// pass still resolves it. Only a stable summary is surfaced, never the raw panic payload.
    async fn wait_stopped(&self) -> Result<(), Arc<String>> {
        let mut first_error: Option<Arc<String>> = None;
        for slot in [&self.producer_task, &self.sink_task] {
            let completion = slot.lock().await.clone();
            if let Some(completion) = completion
                && let Err(error) = completion.await
                && first_error.is_none()
            {
                first_error = Some(error);
            }
        }
        match first_error {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }

    /// Cancels and joins this subscription; used by the single-subscription `cancel` entry.
    async fn cancel_and_wait(&self) -> Result<(), Arc<String>> {
        self.signal_cancel();
        self.wait_stopped().await
    }
}

pub(crate) struct BridgeTaskRegistry {
    next_id: AtomicU64,
    /// Strong owners of live bridge-owned content subscriptions.
    ///
    /// A strong owner (not a `Weak`) is retained until the subscription actually settles, so a
    /// dropped Dart handle never leaves an unobserved producer/sink `JoinHandle` behind: a timeout
    /// or a task panic keeps its owner observable for a later exit pass. Entries are pruned only on
    /// a confirmed clean stop, and never form an `Arc` cycle because the owned tasks capture only
    /// tokens/receivers, not this inner.
    subscriptions: Mutex<HashMap<u64, Arc<BridgeSubscriptionInner>>>,
}

/// Live bridge-owned content subscriptions captured by the broadcast phase and joined later within
/// the same shared deadline.
pub(crate) struct PendingSubscriptionCancellation {
    owners: Vec<Arc<BridgeSubscriptionInner>>,
    issues: Vec<BridgeShutdownIssue>,
}

impl BridgeTaskRegistry {
    pub(crate) fn new() -> Self {
        Self {
            next_id: AtomicU64::new(1),
            subscriptions: Mutex::new(HashMap::new()),
        }
    }

    async fn register(&self, inner: &Arc<BridgeSubscriptionInner>) {
        let mut subscriptions = self.subscriptions.lock().await;
        // Strong owners are retained until the subscription actually settles; a settled entry is
        // pruned here so a normal cancelled subscription never pins residency across resubscribes.
        subscriptions.retain(|_, subscription| !subscription.is_settled());
        subscriptions.insert(inner.id, Arc::clone(inner));
    }

    /// Drops the strong owner of one subscription once it really stopped.
    ///
    /// Called after a single-subscription `cancel` confirms the real ACK; an unconfirmed (timed-out
    /// or panicked) subscription keeps its owner for the exit pass. Removing only a settled entry
    /// keeps the "no strong owner is dropped before a real ACK" invariant.
    async fn unregister(&self, id: u64) {
        let mut subscriptions = self.subscriptions.lock().await;
        if subscriptions
            .get(&id)
            .is_some_and(|inner| inner.is_settled())
        {
            subscriptions.remove(&id);
        }
    }

    /// Signals every registered, bridge-owned content subscription **without joining** any of them.
    ///
    /// The exit contract requires a real ACK for every owner before the runtime finalizes. The
    /// bridge-owned content streams (Thread/product) are signalled here; the caller-owned
    /// shutdown-progress stream is deliberately absent: it uses a self-held token that the early
    /// broadcast must not end, and the caller (Dart) cancels it and reports any unconfirmed ACK
    /// through `external_issues`. Its ACK still gates `Clean` because those external issues are
    /// seeded into the same orchestration.
    ///
    /// Cancellation is split from joining so every independent owner is signalled before any
    /// bounded join can block: a slow join of one owner must never keep another owner from being
    /// cancelled within the same first deadline. The captured owners and any real broadcast failure
    /// are returned for the caller's later join phase, so no second orchestration is created.
    pub(crate) async fn broadcast_cancel(
        &self,
        deadline: Instant,
    ) -> PendingSubscriptionCancellation {
        let mut issues = Vec::new();
        // Bound the registry lock by the shared exit deadline; a lock timeout must not stall exit.
        let subscriptions = {
            let remaining = deadline.saturating_duration_since(Instant::now());
            match tokio::time::timeout(remaining, self.subscriptions.lock()).await {
                Ok(mut registry) => {
                    // Keep live owners registered; drop only the entries that already stopped
                    // cleanly. A pending or failed subscription keeps its strong owner.
                    registry.retain(|_, subscription| !subscription.is_settled());
                    registry.values().cloned().collect::<Vec<_>>()
                }
                Err(_) => {
                    issues.push(shutdown_issue(
                        "subscriptions",
                        "timeout",
                        "the Studio subscription registry lock was not acquired within the exit budget",
                    ));
                    Vec::new()
                }
            }
        };
        for subscription in &subscriptions {
            subscription.signal_cancel();
        }
        PendingSubscriptionCancellation {
            owners: subscriptions,
            issues,
        }
    }

    /// Bounded-joins previously signalled subscriptions within the shared first deadline.
    ///
    /// A subscription that does not finish in time keeps its owner (the registry entry is retained)
    /// and is reported as an issue instead of being awaited without limit.
    pub(crate) async fn join_cancelled(
        &self,
        pending: PendingSubscriptionCancellation,
        deadline: Instant,
    ) -> Vec<BridgeShutdownIssue> {
        let PendingSubscriptionCancellation { owners, mut issues } = pending;
        // Wait on every owner **concurrently** within the same first deadline: a slow owner must not
        // consume the budget before the remaining owners are observed. Each owner keeps its strong
        // registry entry until it really stops, so a timeout only abandons that wait.
        let outcomes = futures::future::join_all(owners.iter().map(|subscription| async move {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return SubscriptionStop::TimedOut;
            }
            match tokio::time::timeout(remaining, subscription.wait_stopped()).await {
                Ok(Ok(())) => SubscriptionStop::Stopped,
                Ok(Err(error)) => SubscriptionStop::Failed(error),
                Err(_) => SubscriptionStop::TimedOut,
            }
        }))
        .await;

        let mut joined: Vec<u64> = Vec::new();
        for (subscription, outcome) in owners.iter().zip(outcomes) {
            match outcome {
                SubscriptionStop::Stopped => joined.push(subscription.id),
                SubscriptionStop::Failed(error) => issues.push(BridgeShutdownIssue {
                    stage: "subscriptions".to_string(),
                    code: "joinError".to_string(),
                    message: format!("a Studio subscription task failed to stop: {error}"),
                    retryable: false,
                    correlation_id: pl_protocol::studio::StudioError::internal().correlation_id,
                }),
                SubscriptionStop::TimedOut => issues.push(shutdown_issue(
                    "subscriptions",
                    "timeout",
                    "a Studio subscription was not cancelled within the exit budget; its owner is retained",
                )),
            }
        }
        // Drop the strong owner of every subscription that really stopped; a timed-out or failed
        // one is retained so a later pass can still observe it.
        if !joined.is_empty() {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if let Ok(mut registry) =
                tokio::time::timeout(remaining, self.subscriptions.lock()).await
            {
                for id in joined {
                    registry.remove(&id);
                }
            }
        }
        issues
    }

    fn next_id(&self) -> u64 {
        self.next_id.fetch_add(1, Ordering::Relaxed)
    }
}

/// Concurrent per-owner outcome of one bounded subscription join.
enum SubscriptionStop {
    Stopped,
    Failed(Arc<String>),
    TimedOut,
}

/// 订阅一个 Thread 的**状态流**：Turn、活动、交互、运行时与 lagged 帧。
///
/// 内容不在状态流里：条目与流式正文只由 ChatView 窗口交付。内容帧的过滤发生在生产端
/// （runtime 的状态流不再转发 `Item*`/`Delta`），所以这里没有“末端丢弃内容帧”的开关，客户端
/// 也不会再因为过滤内容帧而看到 revision 空洞。状态流的 envelope `revision` 只按状态帧递增，
/// 与 ChatView 窗口的内容 revision 相互独立。
pub async fn subscribe_thread(thread_id: String) -> Result<BridgeEventSubscription, BridgeError> {
    let bridge = active_bridge().await?;
    let mut events = bridge
        .studio
        .subscribe_thread(pl_protocol::ThreadSubscriptionRequest {
            thread_id: thread_id.clone(),
        })
        .await?;
    let cancel = bridge.shutdown.child_token();
    let producer_cancel = cancel.clone();
    let (sender, receiver) = mpsc::channel(128);
    // 订阅 = 观察者注册：producer 存活期间钉住驻留，防止 LRU 淘汰正在被
    // 观察的线程（淘汰后该订阅流会永久静默——bus 无事件也无关闭信号）。
    let residency_pin = bridge.studio.pin_thread(&thread_id);
    let producer_task = tokio::spawn(async move {
        let _residency_pin = residency_pin;
        loop {
            tokio::select! {
                _ = producer_cancel.cancelled() => break,
                frame = events.recv() => {
                    let frame = match frame {
                        Ok(Some(frame)) => frame,
                        Ok(None) => break,
                        Err(error) => {
                            let _ = sender.send(BridgeThreadStreamEnvelope::Failure {
                                error: BridgeError::from(error),
                            }).await;
                            break;
                        }
                    };
                    match bridge_thread_update(frame) {
                        Ok(Some(update)) => {
                            if sender
                                .send(BridgeThreadStreamEnvelope::Data {
                                    update: Box::new(update),
                                })
                                .await
                                .is_err()
                            {
                                break;
                            }
                        }
                        Ok(None) => {}
                        Err(error) => {
                            let _ = sender
                                .send(BridgeThreadStreamEnvelope::Failure {
                                    error: BridgeError::from(error),
                                })
                                .await;
                            break;
                        }
                    }
                }
            }
        }
        let _ = sender.send(BridgeThreadStreamEnvelope::Closed).await;
    });
    let id = bridge.subscriptions.next_id();
    let facts = Arc::new(SubscriptionCompletionFacts::default());
    facts.producer.store(OWNED_PENDING, Ordering::SeqCst);
    let inner = Arc::new(BridgeSubscriptionInner {
        id,
        kind: BridgeSubscriptionKind::Thread { thread_id },
        cancel,
        producer_task: Mutex::new(Some(owned_completion(
            producer_task,
            id,
            Arc::clone(&facts),
            SubscriptionOwner::Producer,
        ))),
        sink_task: Mutex::new(None),
        thread_receiver: Mutex::new(Some(receiver)),
        product_receiver: Mutex::new(None),
        shutdown_receiver: Mutex::new(None),
        facts,
    });
    bridge.subscriptions.register(&inner).await;
    Ok(BridgeEventSubscription { inner })
}

pub async fn create_product_subscription() -> Result<BridgeEventSubscription, BridgeError> {
    let bridge = active_bridge().await?;
    let mut events = bridge.studio.subscribe_product();
    let cancel = bridge.shutdown.child_token();
    let producer_cancel = cancel.clone();
    let (sender, receiver) = mpsc::channel(64);
    let producer_task = tokio::spawn(async move {
        loop {
            let envelope = tokio::select! {
                _ = producer_cancel.cancelled() => break,
                event = events.recv() => match event {
                    Ok(event) => match bridge_product_event(event) {
                        Ok(event) => BridgeProductStreamEnvelope::Data {
                            event: Box::new(event),
                        },
                        Err(error) => {
                            tracing::warn!(
                                error_bytes = error.to_string().len(),
                                "failed to convert Studio product event"
                            );
                            continue;
                        }
                    },
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(events)) => {
                        BridgeProductStreamEnvelope::Data {
                            event: Box::new(BridgeProductEventEnvelope::stale(events)),
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                }
            };
            if sender.send(envelope).await.is_err() {
                break;
            }
        }
        let _ = sender.send(BridgeProductStreamEnvelope::Closed).await;
    });
    let id = bridge.subscriptions.next_id();
    let facts = Arc::new(SubscriptionCompletionFacts::default());
    facts.producer.store(OWNED_PENDING, Ordering::SeqCst);
    let inner = Arc::new(BridgeSubscriptionInner {
        id,
        kind: BridgeSubscriptionKind::Product,
        cancel,
        producer_task: Mutex::new(Some(owned_completion(
            producer_task,
            id,
            Arc::clone(&facts),
            SubscriptionOwner::Producer,
        ))),
        sink_task: Mutex::new(None),
        thread_receiver: Mutex::new(None),
        product_receiver: Mutex::new(Some(receiver)),
        shutdown_receiver: Mutex::new(None),
        facts,
    });
    bridge.subscriptions.register(&inner).await;
    Ok(BridgeEventSubscription { inner })
}

/// Creates an independently owned shutdown-progress subscription, including during retry.
///
/// The caller cancels this handle before cancelling its Dart stream, so failed shutdown
/// does not leave stream cancellation waiting for a future progress event.
///
/// When no runtime is installed yet, this returns an immediately-closed (empty) stream without
/// triggering initialization or surfacing a spurious `NotInitialized`. A second instance whose
/// startup failed has no owner and must observe a benign "no progress" stream, not a fake error.
pub async fn subscribe_shutdown_progress() -> Result<BridgeEventSubscription, BridgeError> {
    let (id, events) = match current_bridge() {
        Some(bridge) => (
            bridge.subscriptions.next_id(),
            bridge.studio.subscribe_shutdown_progress().await,
        ),
        None => {
            // `broadcast::channel(1)` with the sender dropped hands back a receiver that is already
            // closed, so the shutdown stream ends immediately without owning any runtime.
            let (sender, receiver) = tokio::sync::broadcast::channel(1);
            drop(sender);
            (0, receiver)
        }
    };
    Ok(BridgeEventSubscription {
        inner: Arc::new(BridgeSubscriptionInner {
            id,
            kind: BridgeSubscriptionKind::Shutdown,
            cancel: CancellationToken::new(),
            producer_task: Mutex::new(None),
            sink_task: Mutex::new(None),
            thread_receiver: Mutex::new(None),
            product_receiver: Mutex::new(None),
            shutdown_receiver: Mutex::new(Some(events)),
            facts: Arc::new(SubscriptionCompletionFacts::default()),
        }),
    })
}

impl BridgeEventSubscription {
    /// Opens this shutdown subscription once. Cancellation closes the native sink.
    ///
    /// # Errors
    /// Rejects another subscription kind or a second stream consumer.
    pub async fn shutdown_stream(
        &self,
        sink: StreamSink<BridgeShutdownProgress>,
    ) -> Result<(), BridgeError> {
        let mut events = self
            .inner
            .shutdown_receiver
            .lock()
            .await
            .take()
            .ok_or_else(|| {
                BridgeError::invalid_argument(
                    "shutdown stream can only be opened once on a shutdown subscription",
                )
            })?;
        let cancel = self.inner.cancel.clone();
        let id = self.inner.id;
        let facts = Arc::clone(&self.inner.facts);
        let mut task = self.inner.sink_task.lock().await;
        if cancel.is_cancelled() {
            return Ok(());
        }
        facts.sink.store(OWNED_PENDING, Ordering::SeqCst);
        let sink_task = tokio::spawn(async move {
            loop {
                tokio::select! {
                    biased;
                    _ = cancel.cancelled() => break,
                    progress = events.recv() => match progress {
                        Ok(progress) => {
                            let stopped = progress.is_stopped();
                            if sink.add(bridge_shutdown_progress(progress)).is_err() || stopped { break; }
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
                        Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                    }
                }
            }
        });
        *task = Some(owned_completion(
            sink_task,
            id,
            facts,
            SubscriptionOwner::Sink,
        ));
        Ok(())
    }
}

fn bridge_shutdown_progress(progress: StudioShutdownProgress) -> BridgeShutdownProgress {
    match progress {
        StudioShutdownProgress::StoppingSubscriptions(_) => {
            BridgeShutdownProgress::StoppingSubscriptions
        }
        StudioShutdownProgress::CancellingTurns(_) => BridgeShutdownProgress::CancellingTurns,
        StudioShutdownProgress::FlushingPersistence(progress) => {
            BridgeShutdownProgress::FlushingPersistence {
                pending_commits: progress.pending_commits(),
            }
        }
        StudioShutdownProgress::StoppingAgents(_) => BridgeShutdownProgress::StoppingAgents,
        StudioShutdownProgress::StoppingMcp(_) => BridgeShutdownProgress::StoppingMcp,
        StudioShutdownProgress::StoppingLsp(_) => BridgeShutdownProgress::StoppingLsp,
        StudioShutdownProgress::Stopped(_) => BridgeShutdownProgress::Stopped,
    }
}

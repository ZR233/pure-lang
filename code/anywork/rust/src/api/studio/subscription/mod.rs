//! Studio 订阅句柄与任务注册表：按订阅种类拆分到同目录子模块。
//!
//! - `thread`：Thread 状态流（Turn/活动/交互/运行时；内容走 ChatView 窗口）。
//! - `product_topic`：typed 产品 topic 订阅（首帧基线 + 该领域事件）。
//! - `shutdown`：关机进度广播。
//! - `startup`：pre-active 启动进度 watch，独立于 installed bridge。
//!
//! 本模块只保留共用的句柄、取消令牌、owner-published completion 机制与注册表；各订阅
//! 的具体 producer/sink 在对应子模块构造。不存在无作用域的 GUI 全事件订阅：GUI transport
//! 必须使用 typed topic 订阅。

pub(crate) mod product_topic;
pub(crate) mod shutdown;
pub(crate) mod startup;
pub(crate) mod thread;

pub use product_topic::create_product_topic_subscription;
pub use shutdown::subscribe_shutdown_progress;
pub use startup::subscribe_startup_progress;
pub use thread::subscribe_thread;

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

use crate::api::studio::bridge_runtime::current_bridge;
use crate::api::studio::handlers::StartupFrame;
use crate::api::studio::types::{
    BridgeError, BridgeProductTopicStreamEnvelope, BridgeShutdownIssue,
    BridgeThreadSubscriptionUpdate,
};
use pl_studio_runtime::StudioShutdownProgress;

/// Thread 状态流的交付信封。
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

/// 一个订阅句柄：取消令牌 + producer/sink 任务与一次性接收端。
pub struct BridgeEventSubscription {
    inner: Arc<BridgeSubscriptionInner>,
}

pub(crate) struct BridgeSubscriptionInner {
    id: u64,
    kind: BridgeSubscriptionKind,
    cancel: CancellationToken,
    producer_task: Mutex<Option<SubscriptionCompletion>>,
    sink_task: Mutex<Option<SubscriptionCompletion>>,
    thread_receiver: Mutex<Option<mpsc::Receiver<BridgeThreadStreamEnvelope>>>,
    product_topic_receiver: Mutex<Option<mpsc::Receiver<BridgeProductTopicStreamEnvelope>>>,
    shutdown_receiver: Mutex<Option<tokio::sync::broadcast::Receiver<StudioShutdownProgress>>>,
    startup_receiver: Mutex<Option<tokio::sync::watch::Receiver<StartupFrame>>>,
    /// Owner-published completion facts for the producer/sink tasks; also the non-blocking pruning
    /// predicate for the registry strong owner.
    facts: Arc<SubscriptionCompletionFacts>,
}

#[derive(Debug, Clone)]
pub(crate) enum BridgeSubscriptionKind {
    Thread {
        thread_id: String,
    },
    ProductTopic {
        topic: crate::api::studio::types::BridgeProductTopic,
    },
    Shutdown,
    Startup {
        attempt: u64,
    },
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
            BridgeSubscriptionKind::ProductTopic { topic } => {
                tracing::trace!(
                    subscription_id = self.id,
                    ?topic,
                    "cancelling Studio product topic subscription"
                );
            }
            BridgeSubscriptionKind::Shutdown => {
                tracing::trace!("cancelling Studio shutdown progress subscription");
            }
            BridgeSubscriptionKind::Startup { attempt } => {
                tracing::trace!(attempt, "cancelling Studio startup progress subscription");
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

/// Lossless 有界发送：取消与接收端关闭都能立即结束等待。
///
/// 返回 `true` 表示已交付；`false` 表示已取消或接收端已关闭。producer 的在途帧与
/// 终态 `Closed` 帧共用本入口，保证 `cancel_and_wait` 不会因为消费者离开后仍满的
/// 通道而永久阻塞。
pub(crate) async fn send_or_cancel<T>(
    cancel: &CancellationToken,
    sender: &mpsc::Sender<T>,
    value: T,
) -> bool {
    tokio::select! {
        _ = cancel.cancelled() => false,
        result = sender.send(value) => result.is_ok(),
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

    pub(crate) async fn register(&self, inner: &Arc<BridgeSubscriptionInner>) {
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
    /// bridge-owned content streams (Thread/product topic) are signalled here; the caller-owned
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

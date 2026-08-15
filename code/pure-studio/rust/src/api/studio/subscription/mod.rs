use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Weak};

use tokio::sync::{Mutex, mpsc};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use crate::api::studio::bridge_runtime::active_bridge;
use crate::api::studio::convert::event::bridge_product_event;
use crate::api::studio::convert::thread_stream::bridge_thread_update;
use crate::api::studio::types::{
    BridgeError, BridgeProductEventEnvelope, BridgeShutdownPhase, BridgeShutdownProgress,
    BridgeThreadSubscriptionUpdate,
};
use crate::frb_generated::StreamSink;
use pl_studio_runtime::StudioShutdownPhase;

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

pub struct BridgeEventSubscription {
    inner: Arc<BridgeSubscriptionInner>,
}

struct BridgeSubscriptionInner {
    id: u64,
    kind: BridgeSubscriptionKind,
    cancel: CancellationToken,
    producer_task: Mutex<Option<JoinHandle<()>>>,
    sink_task: Mutex<Option<JoinHandle<()>>>,
    thread_receiver: Mutex<Option<mpsc::Receiver<BridgeThreadStreamEnvelope>>>,
    product_receiver: Mutex<Option<mpsc::Receiver<BridgeProductStreamEnvelope>>>,
}

#[derive(Debug, Clone)]
enum BridgeSubscriptionKind {
    Thread { thread_id: String },
    Product,
}

impl BridgeEventSubscription {
    pub async fn cancel(&self) {
        self.inner.cancel_and_wait().await;
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
        *self.inner.sink_task.lock().await = Some(task);
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
        *self.inner.sink_task.lock().await = Some(task);
        Ok(())
    }
}

impl Drop for BridgeEventSubscription {
    fn drop(&mut self) {
        self.inner.cancel.cancel();
    }
}

impl BridgeSubscriptionInner {
    async fn cancel_and_wait(&self) {
        match &self.kind {
            BridgeSubscriptionKind::Thread { thread_id } => {
                tracing::trace!(subscription_id = self.id, %thread_id, "cancelling Studio Thread subscription");
            }
            BridgeSubscriptionKind::Product => {
                tracing::trace!(
                    subscription_id = self.id,
                    "cancelling Studio product subscription"
                );
            }
        }
        self.cancel.cancel();
        if let Some(task) = self.producer_task.lock().await.take() {
            let _ = task.await;
        }
        if let Some(task) = self.sink_task.lock().await.take() {
            let _ = task.await;
        }
    }
}

pub(crate) struct BridgeTaskRegistry {
    next_id: AtomicU64,
    subscriptions: Mutex<HashMap<u64, Weak<BridgeSubscriptionInner>>>,
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
        subscriptions.retain(|_, subscription| subscription.strong_count() > 0);
        subscriptions.insert(inner.id, Arc::downgrade(inner));
    }

    pub(crate) async fn cancel_all(&self) {
        let subscriptions = {
            let mut registry = self.subscriptions.lock().await;
            std::mem::take(&mut *registry)
                .into_values()
                .filter_map(|subscription| subscription.upgrade())
                .collect::<Vec<_>>()
        };
        for subscription in subscriptions {
            subscription.cancel_and_wait().await;
        }
    }

    fn next_id(&self) -> u64 {
        self.next_id.fetch_add(1, Ordering::Relaxed)
    }
}

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
    let producer_task = tokio::spawn(async move {
        loop {
            tokio::select! {
                _ = producer_cancel.cancelled() => break,
                frame = events.recv() => {
                    let Some(frame) = frame else {
                        break;
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
    let inner = Arc::new(BridgeSubscriptionInner {
        id: bridge.subscriptions.next_id(),
        kind: BridgeSubscriptionKind::Thread { thread_id },
        cancel,
        producer_task: Mutex::new(Some(producer_task)),
        sink_task: Mutex::new(None),
        thread_receiver: Mutex::new(Some(receiver)),
        product_receiver: Mutex::new(None),
    });
    bridge.subscriptions.register(&inner).await;
    Ok(BridgeEventSubscription { inner })
}

pub async fn create_product_subscription() -> Result<BridgeEventSubscription, BridgeError> {
    let bridge = active_bridge().await?;
    let mut events = bridge.studio.product_events().subscribe();
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
    let inner = Arc::new(BridgeSubscriptionInner {
        id: bridge.subscriptions.next_id(),
        kind: BridgeSubscriptionKind::Product,
        cancel,
        producer_task: Mutex::new(Some(producer_task)),
        sink_task: Mutex::new(None),
        thread_receiver: Mutex::new(None),
        product_receiver: Mutex::new(Some(receiver)),
    });
    bridge.subscriptions.register(&inner).await;
    Ok(BridgeEventSubscription { inner })
}

/// 订阅关机阶段进度流。
///
/// 生命周期独立于 product/thread 订阅：不挂到 bridge shutdown token 下，
/// 从订阅建立一直转发到 `Stopped`（或 Dart 关闭 sink），保证关机期间的
/// 阶段事件与 `FlushingPersistence` 的 pending=0 完成事件可达。
pub async fn subscribe_shutdown_progress(
    sink: StreamSink<BridgeShutdownProgress>,
) -> Result<(), BridgeError> {
    let bridge = active_bridge().await?;
    let mut events = bridge.studio.subscribe_shutdown_progress().await;
    tokio::spawn(async move {
        loop {
            match events.recv().await {
                Ok(progress) => {
                    let stopped = progress.phase == StudioShutdownPhase::Stopped;
                    let event = BridgeShutdownProgress {
                        phase: bridge_shutdown_phase(progress.phase),
                        pending_commits: progress.pending_commits,
                    };
                    if sink.add(event).is_err() {
                        break;
                    }
                    if stopped {
                        break;
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                    // 进度事件不重放；跳过错乱继续等待后续阶段。
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            }
        }
    });
    Ok(())
}

fn bridge_shutdown_phase(phase: StudioShutdownPhase) -> BridgeShutdownPhase {
    match phase {
        StudioShutdownPhase::StoppingSubscriptions => BridgeShutdownPhase::StoppingSubscriptions,
        StudioShutdownPhase::CancellingTurns => BridgeShutdownPhase::CancellingTurns,
        StudioShutdownPhase::FlushingPersistence => BridgeShutdownPhase::FlushingPersistence,
        StudioShutdownPhase::SuspendingTasks => BridgeShutdownPhase::SuspendingTasks,
        StudioShutdownPhase::StoppingMcp => BridgeShutdownPhase::StoppingMcp,
        StudioShutdownPhase::StoppingLsp => BridgeShutdownPhase::StoppingLsp,
        StudioShutdownPhase::Stopped => BridgeShutdownPhase::Stopped,
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::AtomicBool;

    use super::*;

    #[test]
    fn dropping_subscription_requests_cancellation() {
        let cancel = CancellationToken::new();
        let inner = Arc::new(BridgeSubscriptionInner {
            id: 1,
            kind: BridgeSubscriptionKind::Thread {
                thread_id: "thread-1".to_string(),
            },
            cancel: cancel.clone(),
            producer_task: Mutex::new(None),
            sink_task: Mutex::new(None),
            thread_receiver: Mutex::new(None),
            product_receiver: Mutex::new(None),
        });

        drop(BridgeEventSubscription { inner });

        assert!(cancel.is_cancelled());
    }

    #[tokio::test]
    async fn registry_shutdown_cancels_and_joins_registered_tasks() {
        let registry = BridgeTaskRegistry::new();
        let cancel = CancellationToken::new();
        let task_cancel = cancel.clone();
        let task_completed = Arc::new(AtomicBool::new(false));
        let task_completed_after_cancel = Arc::clone(&task_completed);
        let producer_task = tokio::spawn(async move {
            task_cancel.cancelled().await;
            task_completed_after_cancel.store(true, Ordering::SeqCst);
        });
        let inner = Arc::new(BridgeSubscriptionInner {
            id: registry.next_id(),
            kind: BridgeSubscriptionKind::Product,
            cancel: cancel.clone(),
            producer_task: Mutex::new(Some(producer_task)),
            sink_task: Mutex::new(None),
            thread_receiver: Mutex::new(None),
            product_receiver: Mutex::new(None),
        });
        registry.register(&inner).await;

        registry.cancel_all().await;

        assert!(cancel.is_cancelled());
        assert!(task_completed.load(Ordering::SeqCst));
        assert!(inner.producer_task.lock().await.is_none());
        assert!(registry.subscriptions.lock().await.is_empty());
    }
}

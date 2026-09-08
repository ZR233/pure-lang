use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Weak};

use tokio::sync::{Mutex, mpsc};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use crate::api::studio::bridge_runtime::{active_bridge, installed_bridge};
use crate::api::studio::convert::event::bridge_product_event;
use crate::api::studio::convert::thread_stream::bridge_thread_update;
use crate::api::studio::types::{
    BridgeError, BridgeProductEventEnvelope, BridgeShutdownProgress, BridgeThreadSubscriptionUpdate,
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
    shutdown_receiver: Mutex<Option<tokio::sync::broadcast::Receiver<StudioShutdownProgress>>>,
}

#[derive(Debug, Clone)]
enum BridgeSubscriptionKind {
    Thread { thread_id: String },
    Product,
    Shutdown,
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
        let producer = self.producer_task.lock().await.take();
        if let Some(task) = producer {
            let _ = task.await;
        }
        let sink = self.sink_task.lock().await.take();
        if let Some(task) = sink {
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
    // 订阅 = 观察者注册：producer 存活期间钉住驻留，防止 LRU 淘汰正在被
    // 观察的线程（淘汰后该订阅流会永久静默——bus 无事件也无关闭信号）。
    let residency_pin = bridge.studio.pin_thread(&thread_id);
    let producer_task = tokio::spawn(async move {
        let _residency_pin = residency_pin;
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
        shutdown_receiver: Mutex::new(None),
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
    let inner = Arc::new(BridgeSubscriptionInner {
        id: bridge.subscriptions.next_id(),
        kind: BridgeSubscriptionKind::Product,
        cancel,
        producer_task: Mutex::new(Some(producer_task)),
        sink_task: Mutex::new(None),
        thread_receiver: Mutex::new(None),
        product_receiver: Mutex::new(Some(receiver)),
        shutdown_receiver: Mutex::new(None),
    });
    bridge.subscriptions.register(&inner).await;
    Ok(BridgeEventSubscription { inner })
}

/// Creates an independently owned shutdown-progress subscription, including during retry.
///
/// The caller cancels this handle before cancelling its Dart stream, so failed shutdown
/// does not leave stream cancellation waiting for a future progress event.
pub async fn subscribe_shutdown_progress() -> Result<BridgeEventSubscription, BridgeError> {
    let bridge = installed_bridge()?;
    let events = bridge.studio.subscribe_shutdown_progress().await;
    Ok(BridgeEventSubscription {
        inner: Arc::new(BridgeSubscriptionInner {
            id: bridge.subscriptions.next_id(),
            kind: BridgeSubscriptionKind::Shutdown,
            cancel: CancellationToken::new(),
            producer_task: Mutex::new(None),
            sink_task: Mutex::new(None),
            thread_receiver: Mutex::new(None),
            product_receiver: Mutex::new(None),
            shutdown_receiver: Mutex::new(Some(events)),
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
        let mut task = self.inner.sink_task.lock().await;
        if cancel.is_cancelled() {
            return Ok(());
        }
        *task = Some(tokio::spawn(async move {
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
        }));
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
            shutdown_receiver: Mutex::new(None),
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
            shutdown_receiver: Mutex::new(None),
        });
        registry.register(&inner).await;

        registry.cancel_all().await;

        assert!(cancel.is_cancelled());
        assert!(task_completed.load(Ordering::SeqCst));
        assert!(inner.producer_task.lock().await.is_none());
        assert!(registry.subscriptions.lock().await.is_empty());
    }
}

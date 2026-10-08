//! 关机进度订阅：与 runtime 生命周期共享同一次 shutdown 序列。

use std::sync::Arc;
use std::sync::atomic::Ordering;

use super::{
    BridgeEventSubscription, BridgeSubscriptionInner, BridgeSubscriptionKind, OWNED_PENDING,
    SubscriptionCompletionFacts, SubscriptionOwner, owned_completion,
};
use crate::api::studio::bridge_runtime::current_bridge;
use crate::api::studio::types::{BridgeError, BridgeShutdownProgress};
use crate::frb_generated::StreamSink;
use pl_studio_runtime::StudioShutdownProgress;

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
            cancel: tokio_util::sync::CancellationToken::new(),
            producer_task: tokio::sync::Mutex::new(None),
            sink_task: tokio::sync::Mutex::new(None),
            thread_receiver: tokio::sync::Mutex::new(None),
            product_topic_receiver: tokio::sync::Mutex::new(None),
            shutdown_receiver: tokio::sync::Mutex::new(Some(events)),
            startup_receiver: tokio::sync::Mutex::new(None),
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

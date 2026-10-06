//! Retained completion for host background tasks, independent of shutdown waiters.

use std::sync::Arc;
use std::time::Duration;

use futures::FutureExt;
use futures::future::{BoxFuture, Shared};
use tokio::sync::Mutex;
use tokio::task::{AbortHandle, JoinError, JoinHandle};

pub(super) type BackgroundTaskSlot = Arc<Mutex<Option<Arc<BackgroundTask>>>>;

/// Upper bound for taking a slot lock while broadcasting cancellation. The slot is only ever held
/// for a short clone/take, so this is normally instant; the bound exists so a contended slot can
/// never stall the cancel broadcast to the other independent owners.
const CANCEL_SIGNAL_LOCK_LIMIT: Duration = Duration::from_millis(100);

pub(super) struct BackgroundTask {
    abort: AbortHandle,
    completion: Shared<BoxFuture<'static, Result<(), Arc<JoinError>>>>,
}

impl BackgroundTask {
    pub(super) fn new(task: JoinHandle<()>) -> Arc<Self> {
        Arc::new(Self {
            abort: task.abort_handle(),
            completion: async move { task.await.map_err(Arc::new) }.boxed().shared(),
        })
    }

    pub(super) fn is_finished(&self) -> bool {
        self.abort.is_finished()
    }

    /// Signals the task to stop without waiting for its completion.
    pub(super) fn signal(&self) {
        self.abort.abort();
    }
}

impl Drop for BackgroundTask {
    fn drop(&mut self) {
        self.abort.abort();
    }
}

pub(super) async fn stop(slot: &BackgroundTaskSlot) -> Result<(), Arc<JoinError>> {
    let task = slot.lock().await.clone();
    let Some(task) = task else {
        return Ok(());
    };
    task.abort.abort();
    // The slot retains this same future if the caller drops its shutdown future.
    match task.completion.clone().await {
        Ok(()) => Ok(()),
        Err(error) if error.is_cancelled() => Ok(()),
        Err(error) => Err(error),
    }
}

/// Aborts a retained background task without waiting for it to finish.
///
/// Used to broadcast cancellation to every independent background owner before the bounded joins
/// begin, so a later budget shortage cannot leave an owner running un-signalled. The slot lock is
/// bounded: a contended slot must not block the cancel broadcast to the remaining owners.
pub(super) async fn signal(slot: &BackgroundTaskSlot) {
    match tokio::time::timeout(CANCEL_SIGNAL_LOCK_LIMIT, slot.lock()).await {
        Ok(guard) => {
            if let Some(task) = guard.clone() {
                task.signal();
            }
        }
        Err(_) => {
            tracing::warn!(
                "background task slot was contended during cancel broadcast; skipping this owner"
            );
        }
    }
}

/// Joins a cooperatively stopped task without aborting an in-flight resource transfer.
pub(super) async fn finish(slot: &BackgroundTaskSlot) -> Result<(), Arc<JoinError>> {
    let task = slot.lock().await.clone();
    match task {
        Some(task) => task.completion.clone().await,
        None => Ok(()),
    }
}

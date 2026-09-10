//! Retained completion for host background tasks, independent of shutdown waiters.

use std::sync::Arc;

use futures::FutureExt;
use futures::future::{BoxFuture, Shared};
use tokio::sync::Mutex;
use tokio::task::{AbortHandle, JoinError, JoinHandle};

pub(super) type BackgroundTaskSlot = Arc<Mutex<Option<Arc<BackgroundTask>>>>;

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

/// Joins a cooperatively stopped task without aborting an in-flight resource transfer.
pub(super) async fn finish(slot: &BackgroundTaskSlot) -> Result<(), Arc<JoinError>> {
    let task = slot.lock().await.clone();
    match task {
        Some(task) => task.completion.clone().await,
        None => Ok(()),
    }
}

//! Owns failed candidate closures across refresh retries and cancelled shutdown waiters.
use pl_core::{
    thread::ThreadHandle,
    tool::opaque::{RejectedTools, ToolError},
};
use std::sync::{Arc, Mutex};

#[derive(Clone, Default)]
pub(super) struct RejectedToolOwners {
    entries: Arc<Mutex<Vec<Arc<Entry>>>>,
    stop: Arc<Mutex<tokio_util::sync::CancellationToken>>,
}
struct Entry {
    thread: ThreadHandle,
    gate: tokio::sync::Mutex<()>,
    resources: Mutex<Option<Box<RejectedTools>>>,
}
struct Lease {
    entry: Arc<Entry>,
    resources: Option<Box<RejectedTools>>,
}
impl Drop for Lease {
    fn drop(&mut self) {
        if let Some(resources) = self.resources.take() {
            *self
                .entry
                .resources
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(resources);
        }
    }
}
#[derive(Debug, thiserror::Error)]
#[error("rejected tool candidate cleanup failed: {0}")]
struct CleanupFailure(#[source] Arc<ToolError>);

impl RejectedToolOwners {
    pub(super) fn start(&self) -> tokio_util::sync::CancellationToken {
        let token = tokio_util::sync::CancellationToken::new();
        *self
            .stop
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = token.clone();
        token
    }
    pub(super) fn stop(&self) {
        self.stop
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .cancel();
    }
    pub(super) fn retain(&self, thread: ThreadHandle, resources: Box<RejectedTools>) {
        self.entries
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(Arc::new(Entry {
                thread,
                gate: Default::default(),
                resources: Mutex::new(Some(resources)),
            }));
    }
    pub(super) async fn retry(&self, thread: Option<&ThreadHandle>) -> anyhow::Result<()> {
        let entries = self
            .entries
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .iter()
            .filter(|entry| thread.is_none_or(|thread| thread.same_instance(&entry.thread)))
            .cloned()
            .collect::<Vec<_>>();
        for entry in entries {
            // This permit serializes cleanup operations; the resource data lock never spans IO.
            let _permit = entry.gate.lock().await;
            let resources = entry
                .resources
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .take();
            let mut lease = Lease {
                entry: entry.clone(),
                resources,
            };
            if let Some(resources) = &mut lease.resources {
                resources.retry_close().await.map_err(CleanupFailure)?;
                lease.resources = None;
            }
            self.entries
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .retain(|current| !Arc::ptr_eq(current, &entry));
        }
        Ok(())
    }
}

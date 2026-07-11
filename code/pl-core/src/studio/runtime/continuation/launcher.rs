use std::future::Future;
use std::pin::Pin;
#[cfg(test)]
use std::sync::Arc;
#[cfg(test)]
use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::Result;

use super::ContinuationRequest;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ContinuationLaunch {
    pub(crate) request: ContinuationRequest,
    pub(crate) prompt: String,
}

pub(crate) type ContinuationLaunchFuture =
    Pin<Box<dyn Future<Output = Result<()>> + Send + 'static>>;

/// Submits a prepared planner continuation without exposing model execution to the scheduler.
pub(crate) trait ContinuationLauncher: Send + Sync {
    fn launch(&self, launch: ContinuationLaunch) -> ContinuationLaunchFuture;
}

#[cfg(test)]
#[derive(Clone)]
pub(crate) struct ContinuationTestBarrier {
    entered: Arc<tokio::sync::Barrier>,
    release: Arc<tokio::sync::Barrier>,
    used: Arc<AtomicBool>,
}

#[cfg(test)]
impl ContinuationTestBarrier {
    pub(crate) fn new() -> Self {
        Self {
            entered: Arc::new(tokio::sync::Barrier::new(2)),
            release: Arc::new(tokio::sync::Barrier::new(2)),
            used: Arc::new(AtomicBool::new(false)),
        }
    }

    pub(crate) async fn pause_once(&self) {
        if !self.used.swap(true, Ordering::SeqCst) {
            self.entered.wait().await;
            self.release.wait().await;
        }
    }

    pub(crate) async fn wait_until_entered(&self) {
        self.entered.wait().await;
    }

    pub(crate) async fn release(&self) {
        self.release.wait().await;
    }
}

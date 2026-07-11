use std::future::Future;
use std::pin::Pin;

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

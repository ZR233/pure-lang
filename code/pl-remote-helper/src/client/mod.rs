//! Physical process leases shared by local execution and the SSH service.

mod control;
mod managed;
mod spec;
mod transport;

pub use control::{CleanupFailure, WorkerControl};
pub use managed::{ManagedWorker, ProcessTermination};
pub use spec::ProcessCommand;
pub use tokio::process::{ChildStderr, ChildStdin, ChildStdout};
pub use tokio::sync::watch::Receiver as CleanupReceiver;

/// Failures of the supervisor transport, distinct from business process failure.
#[derive(Debug, thiserror::Error)]
pub enum WorkerClientError {
    #[error("worker {executable:?} bootstrap failed: {source}; stderr: {stderr}")]
    Bootstrap {
        executable: std::path::PathBuf,
        stderr: String,
        #[source]
        source: Box<WorkerClientError>,
    },
    #[error("worker protocol version mismatch: expected {expected}, got {actual}")]
    VersionMismatch { expected: u32, actual: u32 },
    #[error("process worker {operation}: {source}")]
    Io {
        operation: &'static str,
        #[source]
        source: std::io::Error,
    },
    #[error("process worker protocol: {0}")]
    Protocol(&'static str),
    #[error("process worker event decoding: {0}")]
    Decode(#[from] serde_json::Error),
    #[error("process worker lease is closed")]
    Closed,
    #[error("process worker readiness timed out")]
    ReadinessTimeout,
    #[error("process worker exited unexpectedly: {0}")]
    WorkerExit(std::process::ExitStatus),
    #[error("process worker cleanup failed: {0:?}")]
    Cleanup(CleanupFailure),
    #[error("business process launch failed (OS error {os_error:?})")]
    StartFailed { os_error: Option<i32> },
}

impl WorkerClientError {
    fn io(operation: &'static str, source: std::io::Error) -> Self {
        Self::Io { operation, source }
    }
}

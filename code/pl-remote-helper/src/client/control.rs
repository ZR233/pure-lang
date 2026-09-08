use std::io;
use std::net::Shutdown;
use std::os::unix::net::UnixStream;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use pl_protocol::process_worker::ProcessWorkerCommand;
use tokio::net::unix::OwnedWriteHalf;
use tokio::sync::watch;

use super::WorkerClientError;

/// A physical cleanup failure; observing it does not release the process lease.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CleanupFailure {
    pub operation: String,
    pub os_error: Option<i32>,
}

struct ControlState {
    writer: OwnedWriteHalf,
    shutdown: UnixStream,
    closed: AtomicBool,
    failure: watch::Sender<Option<CleanupFailure>>,
}

/// Cloneable cancellation and observation capability. Only ManagedWorker owns the lease.
#[derive(Clone)]
pub struct WorkerControl {
    state: Arc<ControlState>,
}

impl std::fmt::Debug for WorkerControl {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("WorkerControl")
            .field("closed", &self.state.closed.load(Ordering::Acquire))
            .finish_non_exhaustive()
    }
}

impl WorkerControl {
    pub(super) fn new(writer: OwnedWriteHalf, shutdown: UnixStream) -> Self {
        let (failure, _) = watch::channel(None);
        Self {
            state: Arc::new(ControlState {
                writer,
                shutdown,
                closed: AtomicBool::new(false),
                failure,
            }),
        }
    }

    /// Requests cancellation without claiming physical cleanup is complete.
    /// # Errors
    /// Fails for a released lease or broken control channel.
    pub async fn cancel(&self) -> Result<(), WorkerClientError> {
        self.send(ProcessWorkerCommand::Cancel).await
    }

    /// Retries cleanup using the original worker and its retained process handles.
    /// # Errors
    /// Fails for a released lease or broken control channel.
    pub async fn retry_cleanup(&self) -> Result<(), WorkerClientError> {
        self.send(ProcessWorkerCommand::RetryCleanup).await
    }

    /// Observes cleanup failure without consuming command output or terminal status.
    pub fn cleanup_failures(&self) -> watch::Receiver<Option<CleanupFailure>> {
        self.state.failure.subscribe()
    }

    /// Seals the lease synchronously, including when an owner is being dropped.
    /// This initiates cleanup, not a confirmation of process exit; retries are then unavailable.
    pub fn close(&self) {
        if !self.state.closed.swap(true, Ordering::AcqRel) {
            let _ = self.state.shutdown.shutdown(Shutdown::Write);
        }
    }

    pub(super) fn report(&self, failure: CleanupFailure) {
        self.state.failure.send_replace(Some(failure));
    }

    pub(super) async fn send(
        &self,
        command: ProcessWorkerCommand,
    ) -> Result<(), WorkerClientError> {
        self.write_bytes(&[command as u8]).await
    }

    // Only bootstrap can send a multi-byte frame, before any control clone is exposed.
    // Cancelling a partial write drops the unstarted worker; it is never retried in place.
    pub(super) async fn configure(&self, configuration: &[u8]) -> Result<(), WorkerClientError> {
        let length = u32::try_from(configuration.len())
            .map_err(|_| WorkerClientError::Protocol("configuration exceeds wire length"))?;
        self.write_bytes(&length.to_be_bytes()).await?;
        self.write_bytes(configuration).await
    }

    async fn write_bytes(&self, mut bytes: &[u8]) -> Result<(), WorkerClientError> {
        while !bytes.is_empty() {
            if self.state.closed.load(Ordering::Acquire) {
                return Err(WorkerClientError::Closed);
            }
            self.state
                .writer
                .writable()
                .await
                .map_err(|source| WorkerClientError::io("waitControl", source))?;
            match self.state.writer.try_write(bytes) {
                Ok(0) => {
                    return Err(WorkerClientError::io(
                        "writeControl",
                        io::ErrorKind::WriteZero.into(),
                    ));
                }
                Ok(count) => bytes = &bytes[count..],
                Err(error)
                    if matches!(
                        error.kind(),
                        io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted
                    ) =>
                {
                    continue;
                }
                Err(source) => return Err(WorkerClientError::io("writeControl", source)),
            }
        }
        Ok(())
    }
}

use std::path::Path;
use std::process::ExitStatus;
use std::time::Duration;

use pl_protocol::process_worker::{
    PROCESS_WORKER_PROTOCOL_VERSION, ProcessWorkerCommand, ProcessWorkerEvent,
};
use tokio::process::Child;

use super::transport::{self, EventReader};
use super::{
    ChildStderr, ChildStdin, ChildStdout, CleanupFailure, ProcessCommand, WorkerClientError,
    WorkerControl,
};

/// Actual business termination, never inferred from the supervisor's success code.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcessTermination {
    ExitCode(i32),
    Signal(i32),
}

#[derive(Debug)]
enum Phase {
    Preparing,
    Starting,
    StartFailed,
    Running,
    Exited(ProcessTermination),
    Delivered,
}

/// Unique physical lease. Drop requests cleanup even if control clones remain alive.
/// Callers must drain both output streams concurrently and await this lease before
/// reporting resource closure. Cancelling wait borrows, but never releases, the lease.
pub struct ManagedWorker {
    child: Child,
    events: EventReader,
    control: WorkerControl,
    phase: Phase,
    channel_open: bool,
    status: Option<ExitStatus>,
    failure: Option<WorkerClientError>,
}

impl std::fmt::Debug for ManagedWorker {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ManagedWorker")
            .field("pid", &self.child.id())
            .field("phase", &self.phase)
            .field("status", &self.status)
            .finish_non_exhaustive()
    }
}

impl ManagedWorker {
    /// Starts a trusted worker executable after validating its readiness version.
    /// Business environment and cwd are not applied to the worker itself.
    /// # Errors
    /// Returns bootstrap/transport errors. Before Start, failure kills and reaps the
    /// worker; after Start, errors retain the lease until physical cleanup completes.
    pub async fn spawn(
        executable: &Path,
        command: ProcessCommand,
    ) -> Result<Self, WorkerClientError> {
        let configuration = command.encode()?;
        let mut worker = Self::ready(executable).await?;
        match tokio::time::timeout(
            Duration::from_secs(10),
            worker.control.configure(&configuration),
        )
        .await
        {
            Ok(Ok(())) => {}
            Ok(Err(error)) => return Err(worker.bootstrap_failure(executable, error).await),
            Err(_) => {
                return Err(worker
                    .bootstrap_failure(executable, WorkerClientError::ReadinessTimeout)
                    .await);
            }
        }
        // Once sending Start is attempted, killing the supervisor could orphan a process.
        worker.phase = Phase::Starting;
        if let Err(error) = worker.control.send(ProcessWorkerCommand::Start).await {
            worker.fail(error);
            worker.wait().await?;
            return Err(WorkerClientError::Protocol(
                "Start failed without a transport error",
            ));
        }
        Ok(worker)
    }

    /// Checks the Ready protocol without starting any business process, then reaps the probe.
    /// # Errors
    /// Reports bootstrap failure or failure to shut down the unstarted worker.
    pub async fn probe(executable: &Path) -> Result<(), WorkerClientError> {
        let mut worker = Self::ready(executable).await?;
        // Complete the existing configuration frame, but never send Start.
        let configuration = ProcessCommand::new(executable).encode()?;
        match tokio::time::timeout(
            Duration::from_secs(10),
            worker.control.configure(&configuration),
        )
        .await
        {
            Ok(Ok(())) => {}
            Ok(Err(error)) => return Err(worker.bootstrap_failure(executable, error).await),
            Err(_) => {
                return Err(worker
                    .bootstrap_failure(executable, WorkerClientError::ReadinessTimeout)
                    .await);
            }
        }
        worker.control.close();
        match tokio::time::timeout(Duration::from_secs(10), worker.child.wait()).await {
            Ok(Ok(status)) if status.success() => Ok(()),
            Ok(Ok(status)) => Err(worker
                .bootstrap_failure(executable, WorkerClientError::WorkerExit(status))
                .await),
            Ok(Err(error)) => Err(worker
                .bootstrap_failure(executable, WorkerClientError::io("probeExit", error))
                .await),
            Err(_) => Err(worker
                .bootstrap_failure(executable, WorkerClientError::ReadinessTimeout)
                .await),
        }
    }

    async fn ready(executable: &Path) -> Result<Self, WorkerClientError> {
        let (child, events, control) =
            transport::spawn(executable).map_err(|source| WorkerClientError::Bootstrap {
                executable: executable.into(),
                stderr: String::new(),
                source: Box::new(source),
            })?;
        let mut worker = Self {
            child,
            events,
            control,
            phase: Phase::Preparing,
            channel_open: true,
            status: None,
            failure: None,
        };
        let ready = tokio::time::timeout(Duration::from_secs(10), async {
            match worker.events.next().await? {
                Some(ProcessWorkerEvent::Ready { protocol_version })
                    if protocol_version == PROCESS_WORKER_PROTOCOL_VERSION =>
                {
                    Ok(())
                }
                Some(ProcessWorkerEvent::Ready { protocol_version }) => {
                    Err(WorkerClientError::VersionMismatch {
                        expected: PROCESS_WORKER_PROTOCOL_VERSION,
                        actual: protocol_version,
                    })
                }
                Some(_) => Err(WorkerClientError::Protocol("expected Ready")),
                None => {
                    let status = worker
                        .child
                        .wait()
                        .await
                        .map_err(|e| WorkerClientError::io("bootstrapExit", e))?;
                    Err(WorkerClientError::WorkerExit(status))
                }
            }
        })
        .await;
        match ready {
            Ok(Ok(())) => Ok(worker),
            Ok(Err(error)) => Err(worker.bootstrap_failure(executable, error).await),
            Err(_) => Err(worker
                .bootstrap_failure(executable, WorkerClientError::ReadinessTimeout)
                .await),
        }
    }

    async fn bootstrap_failure(
        &mut self,
        executable: &Path,
        source: WorkerClientError,
    ) -> WorkerClientError {
        use tokio::io::AsyncReadExt;
        self.control.close();
        // Preparing owns no business process. Kill and await before collecting bounded diagnostics.
        let cleanup = self.child.kill().await;
        let mut stderr = Vec::new();
        if let Some(stream) = self.child.stderr.take() {
            let _ = tokio::time::timeout(
                Duration::from_secs(1),
                stream.take(4096).read_to_end(&mut stderr),
            )
            .await;
        }
        let source = match cleanup {
            Ok(()) => source,
            Err(error) => WorkerClientError::io("reapUnstartedWorker", error),
        };
        WorkerClientError::Bootstrap {
            executable: executable.into(),
            stderr: String::from_utf8_lossy(&stderr).into_owned(),
            source: Box::new(source),
        }
    }

    /// Clones control capability, not ownership of the physical lease.
    pub fn control(&self) -> WorkerControl {
        self.control.clone()
    }

    /// Supervisor PID for diagnostics only; never use it for external termination.
    pub fn supervisor_pid(&self) -> Option<u32> {
        self.child.id()
    }

    /// Transfers business stdin to the session's input owner.
    pub fn take_stdin(&mut self) -> Option<ChildStdin> {
        self.child.stdin.take()
    }
    /// Transfers business stdout for concurrent draining.
    pub fn take_stdout(&mut self) -> Option<ChildStdout> {
        self.child.stdout.take()
    }
    /// Transfers business stderr for concurrent draining.
    pub fn take_stderr(&mut self) -> Option<ChildStderr> {
        self.child.stderr.take()
    }

    /// Waits for both the terminal control stream and actual supervisor exit.
    /// Partial frames and exit observations survive cancellation of this future.
    /// # Errors
    /// Protocol, cleanup and worker-exit failures are returned only after physical exit.
    pub async fn wait(&mut self) -> Result<ProcessTermination, WorkerClientError> {
        while self.channel_open || self.status.is_none() {
            tokio::select! {
                event = self.events.next(), if self.channel_open => {
                    match event {
                        Ok(Some(event)) => self.observe(event),
                        Ok(None) => self.channel_open = false,
                        Err(error) => { self.channel_open = false; self.fail(error); }
                    }
                }
                status = self.child.wait(), if self.status.is_none() => {
                    // A wait syscall failure does not prove exit or release ownership.
                    match status {
                        Ok(status) => self.status = Some(status),
                        Err(source) => {
                            self.fail(WorkerClientError::io("waitWorker", source));
                            tokio::time::sleep(Duration::from_millis(100)).await;
                        }
                    }
                }
            }
            if !self.channel_open && self.status.is_none() {
                self.control.close();
            }
        }
        self.control.close();
        let phase = std::mem::replace(&mut self.phase, Phase::Delivered);
        if let Some(error) = self.failure.take() {
            return Err(error);
        }
        if let Some(status) = self.status
            && !status.success()
        {
            return Err(WorkerClientError::WorkerExit(status));
        }
        match phase {
            Phase::Exited(termination) => Ok(termination),
            Phase::Preparing | Phase::Starting | Phase::StartFailed | Phase::Running => {
                Err(WorkerClientError::Protocol("missing terminal result"))
            }
            Phase::Delivered => Err(WorkerClientError::Protocol(
                "terminal result already delivered",
            )),
        }
    }

    fn fail(&mut self, error: WorkerClientError) {
        self.failure.get_or_insert(error);
        self.control.close();
    }

    fn observe(&mut self, event: ProcessWorkerEvent) {
        match event {
            ProcessWorkerEvent::StartFailed { os_error } => {
                if matches!(self.phase, Phase::Starting) {
                    self.phase = Phase::StartFailed;
                    self.fail(WorkerClientError::StartFailed { os_error });
                } else {
                    self.fail(WorkerClientError::Protocol("unexpected StartFailed"));
                }
            }
            ProcessWorkerEvent::Started { pid } => {
                if matches!(self.phase, Phase::Starting) && pid > 0 {
                    self.phase = Phase::Running;
                } else {
                    self.fail(WorkerClientError::Protocol("unexpected Started"));
                }
            }
            ProcessWorkerEvent::Exited { exit_code, signal } => {
                let termination = match (exit_code, signal) {
                    (Some(code), None) if (0..=255).contains(&code) => {
                        Some(ProcessTermination::ExitCode(code))
                    }
                    (None, Some(signal)) if signal > 0 => Some(ProcessTermination::Signal(signal)),
                    _ => None,
                };
                if let Some(termination) = termination
                    && matches!(self.phase, Phase::Running)
                {
                    self.phase = Phase::Exited(termination);
                } else {
                    self.fail(WorkerClientError::Protocol("invalid terminal result"));
                }
            }
            ProcessWorkerEvent::CleanupFailed {
                operation,
                os_error,
            } => {
                let failure = CleanupFailure {
                    operation,
                    os_error,
                };
                self.control.report(failure.clone());
                // Keep the channel open so the original cleanup context can be retried.
                self.failure
                    .get_or_insert(WorkerClientError::Cleanup(failure));
            }
            ProcessWorkerEvent::Ready { .. } | ProcessWorkerEvent::StoppedBeforeStart => {
                self.fail(WorkerClientError::Protocol("unexpected bootstrap event"));
            }
        }
    }
}

impl Drop for ManagedWorker {
    fn drop(&mut self) {
        self.control.close();
        if matches!(self.phase, Phase::Preparing) {
            // No Start byte has been attempted; no business process can exist yet.
            let _ = self.child.start_kill();
        }
    }
}

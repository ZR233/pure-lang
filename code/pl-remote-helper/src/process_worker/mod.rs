//! One supervisor per command, entered before Tokio so reaping and signal state have one owner.

mod bootstrap;
mod channel;
mod kernel;

use std::collections::BTreeMap;
use std::io;
use std::os::fd::OwnedFd;
use std::os::unix::net::UnixStream;
use std::os::unix::process::ExitStatusExt;
use std::process::ExitStatus;
use std::time::{Duration, Instant};

use pl_protocol::process_worker::{ProcessWorkerCommand, ProcessWorkerEvent};

use channel::Channel;
use kernel::{Kernel, Reaped};

const TERMINATION_GRACE: Duration = Duration::from_secs(2);

#[derive(Debug, thiserror::Error)]
#[error("process worker {operation} failed: {source}")]
pub(super) struct WorkerError {
    operation: &'static str,
    #[source]
    source: io::Error,
}

struct TrackedChild {
    descriptor: OwnedFd,
    last_signal: Option<i32>,
}

struct Worker {
    kernel: Kernel,
    channel: Channel,
    children: BTreeMap<libc::pid_t, TrackedChild>,
    root: libc::pid_t,
    root_status: Option<ExitStatus>,
    closing_since: Option<Instant>,
    escalated: bool,
    failure: Option<WorkerError>,
}

pub(super) fn run(control: UnixStream) -> Result<(), WorkerError> {
    let kernel = Kernel::new().map_err(|source| WorkerError {
        operation: "initialize",
        source,
    })?;
    let mut channel = Channel::new(control).map_err(|source| WorkerError {
        operation: "control",
        source,
    })?;
    channel
        .enqueue(ProcessWorkerEvent::Ready {
            protocol_version: pl_protocol::process_worker::PROCESS_WORKER_PROTOCOL_VERSION,
        })
        .map_err(|source| WorkerError {
            operation: "ready",
            source,
        })?;
    let mut command =
        bootstrap::read_command(&mut channel, &kernel).map_err(|source| WorkerError {
            operation: "configuration",
            source,
        })?;
    if !await_start(&kernel, &mut channel).map_err(|source| WorkerError {
        operation: "handshake",
        source,
    })? {
        return Ok(());
    }
    kernel.prepare_command(&mut command);
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(source) => {
            // No business process exists. Report its launch failure without inventing exit.
            channel.stop_reading();
            if channel
                .enqueue(ProcessWorkerEvent::StartFailed {
                    os_error: source.raw_os_error(),
                })
                .is_ok()
            {
                while matches!(channel.flush(), Ok(false)) {
                    if let Err(error) = kernel.wait(channel.interest(), -1)
                        && error.kind() != io::ErrorKind::Interrupted
                    {
                        break;
                    }
                }
            }
            return Err(WorkerError {
                operation: "spawn",
                source,
            });
        }
    };
    let root = child.id() as libc::pid_t;
    let mut worker = Worker {
        kernel,
        channel,
        children: BTreeMap::new(),
        root,
        root_status: None,
        closing_since: None,
        escalated: false,
        failure: None,
    };
    // After spawn every error enters the drain loop; opening a pidfd cannot abandon a child.
    match kernel::pidfd(root) {
        Ok(descriptor) => {
            worker.children.insert(
                root,
                TrackedChild {
                    descriptor,
                    last_signal: None,
                },
            );
        }
        Err(error) => {
            worker.failed("watchRoot", error);
            // The root has not been reaped; Child still denotes our uniquely owned child.
            if let Err(error) = child.kill() {
                worker.failed("stopUnwatchedRoot", error);
            }
        }
    }
    if let Err(error) = worker
        .channel
        .enqueue(ProcessWorkerEvent::Started { pid: child.id() })
    {
        worker.failed("reportStart", error);
    }
    worker.drain()
}

impl Worker {
    fn begin_close(&mut self) {
        self.closing_since.get_or_insert_with(Instant::now);
    }

    fn failed(&mut self, operation: &'static str, source: io::Error) {
        self.begin_close();
        if self.failure.is_none() {
            let event = ProcessWorkerEvent::CleanupFailed {
                operation: operation.into(),
                os_error: source.raw_os_error(),
            };
            self.failure = Some(WorkerError { operation, source });
            // Only one bounded error event is queued; a broken observer cannot stop draining.
            let _ = self.channel.enqueue(event);
        }
    }

    fn drain(mut self) -> Result<(), WorkerError> {
        let mut terminal_queued = false;
        loop {
            match self.channel.receive() {
                Ok(Some(ProcessWorkerCommand::Start)) => self.failed(
                    "duplicateStart",
                    io::Error::new(io::ErrorKind::InvalidData, "command already started"),
                ),
                Ok(Some(ProcessWorkerCommand::Cancel)) => self.begin_close(),
                Ok(Some(ProcessWorkerCommand::RetryCleanup)) => {
                    self.begin_close();
                    for child in self.children.values_mut() {
                        child.last_signal = None;
                    }
                }
                Ok(None) => {}
                Err(error) => self.failed("readControl", error),
            }
            match self.kernel.cancellation_signal() {
                Ok(true) => self.begin_close(),
                Ok(false) => {}
                Err(error) => self.failed("readSignals", error),
            }
            let empty = self.reap_children();
            if self.root_status.is_some() {
                self.begin_close();
            }
            if !empty {
                self.discover_and_signal();
            }
            if empty && !terminal_queued {
                self.channel.stop_reading();
                if let Some(status) = self.root_status {
                    if let Err(error) = self.channel.enqueue(ProcessWorkerEvent::Exited {
                        exit_code: status.code(),
                        signal: status.signal(),
                    }) {
                        self.failed("reportExit", error);
                    }
                } else {
                    self.failed(
                        "rootStatus",
                        io::Error::new(io::ErrorKind::InvalidData, "root exit was not observed"),
                    );
                }
                terminal_queued = true;
            }
            let flushed = match self.channel.flush() {
                Ok(flushed) => flushed,
                Err(error) => {
                    self.failed("writeControl", error);
                    continue;
                }
            };
            if empty && terminal_queued && flushed {
                return match self.failure {
                    Some(error) => Err(error),
                    None => Ok(()),
                };
            }
            let timeout = if empty { -1 } else { self.timeout() };
            if let Err(error) = self.kernel.wait(self.channel.interest(), timeout)
                && error.kind() != io::ErrorKind::Interrupted
            {
                self.failed("poll", error);
                // Infrastructure failure stays observable and retains ownership; bound retries
                // instead of spinning on a persistent descriptor or kernel allocation error.
                std::thread::sleep(Duration::from_millis(100));
            }
        }
    }

    fn timeout(&self) -> i32 {
        match self.closing_since {
            Some(start) if !self.escalated => TERMINATION_GRACE
                .saturating_sub(start.elapsed())
                .as_nanos()
                .div_ceil(1_000_000) as i32,
            Some(_) | None => -1,
        }
    }

    fn reap_children(&mut self) -> bool {
        loop {
            match kernel::reap() {
                Ok(Reaped::Child(pid, status)) => {
                    self.children.remove(&pid);
                    if pid == self.root && self.root_status.is_none() {
                        self.root_status = Some(status);
                    }
                }
                Ok(Reaped::Empty) => return true,
                Ok(Reaped::StateChanged) => continue,
                Ok(Reaped::Pending) => return false,
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                Err(error) => {
                    self.failed("reap", error);
                    return false;
                }
            }
        }
    }

    fn discover_and_signal(&mut self) {
        match kernel::children() {
            Ok(children) => {
                for pid in children {
                    if let std::collections::btree_map::Entry::Vacant(entry) =
                        self.children.entry(pid)
                    {
                        match kernel::pidfd(pid) {
                            Ok(descriptor) => {
                                entry.insert(TrackedChild {
                                    descriptor,
                                    last_signal: None,
                                });
                            }
                            Err(error) => self.failed("watchChild", error),
                        }
                    }
                }
            }
            Err(error) => self.failed("enumerateChildren", error),
        }
        let Some(start) = self.closing_since else {
            return;
        };
        let requested = if start.elapsed() < TERMINATION_GRACE {
            libc::SIGTERM
        } else {
            libc::SIGKILL
        };
        self.escalated |= requested == libc::SIGKILL;
        let mut failure = None;
        for child in self.children.values_mut() {
            if child.last_signal == Some(requested) {
                continue;
            }
            child.last_signal = Some(requested);
            if let Err(error) = kernel::signal(&child.descriptor, requested)
                && error.raw_os_error() != Some(libc::ESRCH)
            {
                failure.get_or_insert(error);
            }
        }
        if let Some(error) = failure {
            self.failed("signalChild", error);
        }
    }
}

fn await_start(kernel: &Kernel, channel: &mut Channel) -> io::Result<bool> {
    loop {
        channel.flush()?;
        if kernel.cancellation_signal()? {
            break;
        }
        match channel.receive()? {
            Some(ProcessWorkerCommand::Start) => return Ok(true),
            Some(ProcessWorkerCommand::Cancel | ProcessWorkerCommand::RetryCleanup) => break,
            None => {}
        }
        if let Err(error) = kernel.wait(channel.interest(), -1)
            && error.kind() != io::ErrorKind::Interrupted
        {
            return Err(error);
        }
    }
    channel.stop_reading();
    channel.enqueue(ProcessWorkerEvent::StoppedBeforeStart)?;
    while !channel.flush()? {
        if let Err(error) = kernel.wait(channel.interest(), -1)
            && error.kind() != io::ErrorKind::Interrupted
        {
            return Err(error);
        }
    }
    Ok(false)
}

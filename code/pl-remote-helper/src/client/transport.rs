use std::os::fd::AsRawFd;
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::process::Stdio;

use pl_protocol::process_worker::ProcessWorkerEvent;
use tokio::io::AsyncReadExt;
use tokio::net::unix::OwnedReadHalf;
use tokio::process::{Child, Command};

use super::{WorkerClientError, WorkerControl};

const MAX_FRAME_BYTES: usize = 4096;

pub(super) struct EventReader {
    reader: OwnedReadHalf,
    pending: Vec<u8>,
}

impl EventReader {
    // All partial bytes live in self, so cancelling next() never loses a frame prefix.
    pub(super) async fn next(&mut self) -> Result<Option<ProcessWorkerEvent>, WorkerClientError> {
        loop {
            if let Some(end) = self.pending.iter().position(|byte| *byte == b'\n') {
                let event = serde_json::from_slice(&self.pending[..end])?;
                self.pending.drain(..=end);
                return Ok(Some(event));
            }
            let remaining = MAX_FRAME_BYTES.saturating_sub(self.pending.len());
            if remaining == 0 {
                return Err(WorkerClientError::Protocol("control frame too large"));
            }
            let mut chunk = [0; 1024];
            let limit = remaining.min(chunk.len());
            let count = self
                .reader
                .read(&mut chunk[..limit])
                .await
                .map_err(|source| WorkerClientError::io("readControl", source))?;
            if count == 0 {
                return if self.pending.is_empty() {
                    Ok(None)
                } else {
                    Err(WorkerClientError::Protocol("truncated control frame"))
                };
            }
            self.pending.extend_from_slice(&chunk[..count]);
        }
    }
}

pub(super) fn spawn(
    executable: &Path,
) -> Result<(Child, EventReader, WorkerControl), WorkerClientError> {
    let (parent, child_socket) =
        UnixStream::pair().map_err(|source| WorkerClientError::io("createControl", source))?;
    parent
        .set_nonblocking(true)
        .map_err(|source| WorkerClientError::io("configureControl", source))?;
    let shutdown = parent
        .try_clone()
        .map_err(|source| WorkerClientError::io("cloneControl", source))?;
    let socket = tokio::net::UnixStream::from_std(parent)
        .map_err(|source| WorkerClientError::io("registerControl", source))?;
    let (reader, writer) = socket.into_split();
    let fd = child_socket.as_raw_fd();
    let mut command = Command::new(executable);
    command.arg("--process-worker").arg(fd.to_string());
    command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    command.kill_on_drop(false).process_group(0);
    // SAFETY: child_socket owns fd until spawn returns and command is dropped. The hook
    // only invokes async-signal-safe fcntl on the child's copied descriptor table. It
    // neither allocates nor locks, and leaves the parent's CLOEXEC flag unchanged.
    unsafe {
        command.pre_exec(move || {
            let flags = libc::fcntl(fd, libc::F_GETFD);
            if flags < 0 || libc::fcntl(fd, libc::F_SETFD, flags & !libc::FD_CLOEXEC) < 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    let result = command.spawn();
    drop(command);
    drop(child_socket);
    let child = result.map_err(|source| WorkerClientError::io("spawnWorker", source))?;
    Ok((
        child,
        EventReader {
            reader,
            pending: Vec::new(),
        },
        WorkerControl::new(writer, shutdown),
    ))
}

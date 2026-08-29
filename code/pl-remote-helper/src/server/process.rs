use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use pl_protocol::remote::{
    RemoteError, RemoteErrorCode, RemoteEvent, RemoteMessage, RemoteOutputStream,
    RemoteProcessExit, RemoteProcessOutput, RemoteSpawnRequest,
};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::process::{ChildStdin, Command};
use tokio::sync::{Mutex, mpsc};

use crate::codec::write_frame;
use crate::path::{io_error, remote_error};

type SharedWriter = Arc<Mutex<Box<dyn AsyncWrite + Send + Unpin>>>;

#[derive(Debug)]
struct ProcessHandle {
    process_group: i32,
    stdin: Option<Arc<Mutex<ChildStdin>>>,
}

#[derive(Clone)]
pub(super) struct ProcessRegistry {
    processes: Arc<Mutex<HashMap<String, ProcessHandle>>>,
    writer: SharedWriter,
    output_sequence: Arc<AtomicU64>,
}

impl std::fmt::Debug for ProcessRegistry {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ProcessRegistry")
            .field("processes", &self.processes)
            .field("output_sequence", &self.output_sequence)
            .finish_non_exhaustive()
    }
}

impl ProcessRegistry {
    pub(super) fn new(writer: SharedWriter) -> Self {
        Self {
            processes: Arc::new(Mutex::new(HashMap::new())),
            writer,
            output_sequence: Arc::new(AtomicU64::new(0)),
        }
    }

    pub(super) async fn spawn(
        &self,
        request: RemoteSpawnRequest,
        cwd: PathBuf,
        capture_path: PathBuf,
    ) -> Result<(), RemoteError> {
        if self
            .processes
            .lock()
            .await
            .contains_key(&request.process_id)
        {
            return Err(remote_error(
                RemoteErrorCode::InvalidRequest,
                format!("process '{}' already exists", request.process_id),
            ));
        }
        if let Some(parent) = capture_path.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(|error| io_error("failed to create capture directory", error))?;
        }
        let header = format!(
            "=== COMMAND ===\n{}\n\n=== CWD ===\n{}\n\n",
            request.command,
            cwd.display()
        );
        let mut capture = tokio::fs::File::create(&capture_path)
            .await
            .map_err(|error| io_error("failed to create capture file", error))?;
        capture
            .write_all(header.as_bytes())
            .await
            .map_err(|error| io_error("failed to write capture header", error))?;

        let mut command = Command::new("/bin/sh");
        command
            .arg("-c")
            .arg(&request.command)
            .current_dir(cwd)
            .envs(&request.environment)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        #[cfg(unix)]
        {
            command.process_group(0);
        }
        let mut child = command
            .spawn()
            .map_err(|error| io_error("failed to spawn remote process", error))?;
        let process_group = child.id().ok_or_else(|| {
            remote_error(RemoteErrorCode::Io, "spawned process did not expose an id")
        })? as i32;
        let stdin = child.stdin.take();
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| remote_error(RemoteErrorCode::Io, "spawned process has no stdout"))?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| remote_error(RemoteErrorCode::Io, "spawned process has no stderr"))?;
        self.processes.lock().await.insert(
            request.process_id.clone(),
            ProcessHandle {
                process_group,
                stdin: stdin.map(|stdin| Arc::new(Mutex::new(stdin))),
            },
        );

        let (output_sender, output_receiver) = mpsc::channel(256);
        let stdout_task =
            spawn_output_reader(stdout, RemoteOutputStream::Stdout, output_sender.clone());
        let stderr_task =
            spawn_output_reader(stderr, RemoteOutputStream::Stderr, output_sender.clone());
        let output_task = spawn_output_writer(
            request.process_id.clone(),
            output_receiver,
            capture,
            self.writer.clone(),
            self.output_sequence.clone(),
        );
        drop(output_sender);
        let processes = self.processes.clone();
        let writer = self.writer.clone();
        let process_id = request.process_id;
        tokio::spawn(async move {
            let result = child.wait().await;
            let _ = tokio::join!(stdout_task, stderr_task);
            let _ = output_task.await;
            processes.lock().await.remove(&process_id);
            let (exit_code, signal) = match result {
                Ok(status) => {
                    #[cfg(unix)]
                    {
                        use std::os::unix::process::ExitStatusExt;
                        (status.code(), status.signal())
                    }
                    #[cfg(not(unix))]
                    {
                        (status.code(), None)
                    }
                }
                Err(_) => (None, None),
            };
            let _ = write_frame(
                &mut *writer.lock().await,
                None,
                RemoteMessage::Event(RemoteEvent::ProcessExit(RemoteProcessExit {
                    process_id,
                    exit_code,
                    signal,
                })),
                &[],
            )
            .await;
        });
        Ok(())
    }

    pub(super) async fn write_stdin(
        &self,
        process_id: &str,
        body: &[u8],
    ) -> Result<(), RemoteError> {
        let stdin = self
            .processes
            .lock()
            .await
            .get(process_id)
            .ok_or_else(|| {
                remote_error(
                    RemoteErrorCode::ProcessNotFound,
                    format!("unknown process id '{process_id}'"),
                )
            })?
            .stdin
            .clone()
            .ok_or_else(|| {
                remote_error(
                    RemoteErrorCode::ProcessNotFound,
                    format!("process '{process_id}' does not accept stdin"),
                )
            })?;
        let mut stdin = stdin.lock().await;
        stdin
            .write_all(body)
            .await
            .map_err(|error| io_error("failed to write process stdin", error))?;
        stdin
            .flush()
            .await
            .map_err(|error| io_error("failed to flush process stdin", error))
    }

    pub(super) async fn close_stdin(&self, process_id: &str) -> Result<(), RemoteError> {
        let mut processes = self.processes.lock().await;
        let handle = processes.get_mut(process_id).ok_or_else(|| {
            remote_error(
                RemoteErrorCode::ProcessNotFound,
                format!("unknown process id '{process_id}'"),
            )
        })?;
        handle.stdin.take();
        Ok(())
    }

    pub(super) async fn terminate(&self, process_id: &str) -> Result<(), RemoteError> {
        let group = self
            .processes
            .lock()
            .await
            .get(process_id)
            .map(|handle| handle.process_group)
            .ok_or_else(|| {
                remote_error(
                    RemoteErrorCode::ProcessNotFound,
                    format!("unknown process id '{process_id}'"),
                )
            })?;
        terminate_process_group(group);
        Ok(())
    }

    pub(super) async fn terminate_all(&self) {
        let groups = self
            .processes
            .lock()
            .await
            .values()
            .map(|handle| handle.process_group)
            .collect::<Vec<_>>();
        for group in groups {
            terminate_process_group(group);
        }
    }
}

fn spawn_output_reader<R>(
    mut reader: R,
    stream: RemoteOutputStream,
    sender: mpsc::Sender<(RemoteOutputStream, Vec<u8>)>,
) -> tokio::task::JoinHandle<()>
where
    R: AsyncRead + Send + Unpin + 'static,
{
    tokio::spawn(async move {
        let mut buffer = [0_u8; 8192];
        loop {
            let count = match reader.read(&mut buffer).await {
                Ok(0) | Err(_) => break,
                Ok(count) => count,
            };
            if sender
                .send((stream, buffer[..count].to_vec()))
                .await
                .is_err()
            {
                break;
            }
        }
    })
}

fn spawn_output_writer(
    process_id: String,
    mut receiver: mpsc::Receiver<(RemoteOutputStream, Vec<u8>)>,
    mut capture: tokio::fs::File,
    writer: SharedWriter,
    sequence: Arc<AtomicU64>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        while let Some((stream, body)) = receiver.recv().await {
            let label = match stream {
                RemoteOutputStream::Stdout => "STDOUT",
                RemoteOutputStream::Stderr => "STDERR",
            };
            if capture
                .write_all(format!("=== {label} ===\n").as_bytes())
                .await
                .is_err()
            {
                break;
            }
            if capture.write_all(&body).await.is_err() {
                break;
            }
            if !body.ends_with(b"\n") && capture.write_all(b"\n").await.is_err() {
                break;
            }
            let mut writer = writer.lock().await;
            let number = sequence.fetch_add(1, Ordering::Relaxed).saturating_add(1);
            if write_frame(
                &mut *writer,
                None,
                RemoteMessage::Event(RemoteEvent::ProcessOutput(RemoteProcessOutput {
                    process_id: process_id.clone(),
                    sequence: number,
                    stream,
                })),
                &body,
            )
            .await
            .is_err()
            {
                break;
            }
        }
    })
}

#[cfg(unix)]
fn terminate_process_group(group: i32) {
    // SAFETY: `group` is a positive pid returned by the child spawn. Negating it targets only the
    // child process group created with `process_group(0)`.
    unsafe {
        libc::kill(-group, libc::SIGTERM);
        libc::kill(-group, libc::SIGKILL);
    }
}

#[cfg(not(unix))]
fn terminate_process_group(_: i32) {}

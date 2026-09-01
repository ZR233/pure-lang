use std::collections::HashMap;
use std::collections::hash_map::Entry;
use std::ffi::{OsStr, OsString};
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use pl_protocol::remote::{
    RemoteError, RemoteErrorCode, RemoteEvent, RemoteMessage, RemoteOutputStream,
    RemoteProcessExit, RemoteProcessOutput, RemoteShellDescriptor, RemoteShellDialect,
    RemoteSpawnRequest,
};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::process::{Child, ChildStdin, Command};
use tokio::sync::{Mutex, mpsc};

use crate::codec::write_frame;
use crate::path::{io_error, remote_error};

type SharedWriter = Arc<Mutex<Box<dyn AsyncWrite + Send + Unpin>>>;

#[derive(Debug)]
struct ProcessHandle {
    process_group: i32,
    stdin: Option<Arc<Mutex<ChildStdin>>>,
}

#[derive(Debug)]
enum ProcessEntry {
    Starting,
    Running(ProcessHandle),
    Finished,
}

#[derive(Clone)]
pub(super) struct ProcessRegistry {
    processes: Arc<Mutex<HashMap<String, ProcessEntry>>>,
    writer: SharedWriter,
    output_sequence: Arc<AtomicU64>,
    shell: RemoteShellDescriptor,
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
    pub(super) fn new(writer: SharedWriter, shell: RemoteShellDescriptor) -> Self {
        Self {
            processes: Arc::new(Mutex::new(HashMap::new())),
            writer,
            output_sequence: Arc::new(AtomicU64::new(0)),
            shell,
        }
    }

    pub(super) async fn spawn(
        &self,
        request: RemoteSpawnRequest,
        cwd: PathBuf,
        capture_path: PathBuf,
    ) -> Result<(), RemoteError> {
        self.reserve_starting(&request.process_id).await?;
        if let Some(parent) = capture_path.parent()
            && let Err(error) = tokio::fs::create_dir_all(parent).await
        {
            self.release_starting(&request.process_id).await;
            return Err(io_error("failed to create capture directory", error));
        }
        let header = format!(
            "=== COMMAND ===\n{}\n\n=== CWD ===\n{}\n\n",
            request.command,
            cwd.display()
        );
        let mut capture = match tokio::fs::File::create(&capture_path).await {
            Ok(capture) => capture,
            Err(error) => {
                self.release_starting(&request.process_id).await;
                return Err(io_error("failed to create capture file", error));
            }
        };
        if let Err(error) = capture.write_all(header.as_bytes()).await {
            self.release_starting(&request.process_id).await;
            return Err(io_error("failed to write capture header", error));
        }

        let mut command = Command::new(&self.shell.path);
        command
            .arg(match self.shell.dialect {
                RemoteShellDialect::Bash | RemoteShellDialect::Sh => "-c",
                RemoteShellDialect::Pwsh | RemoteShellDialect::PowerShell => "-Command",
                RemoteShellDialect::Cmd => "/C",
            })
            .arg(&request.command)
            .current_dir(cwd)
            .envs(&request.environment)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if !request.environment.contains_key("PATH")
            && let Some(path) = user_tool_path(
                std::env::var_os("HOME").as_deref(),
                std::env::var_os("PATH").as_deref(),
            )?
        {
            command.env("PATH", path);
        }
        command.kill_on_drop(true);
        #[cfg(unix)]
        {
            command.process_group(0);
        }
        let mut child = match command.spawn() {
            Ok(child) => child,
            Err(error) => {
                self.release_starting(&request.process_id).await;
                return Err(io_error("failed to spawn remote process", error));
            }
        };
        let process_group = match child.id() {
            Some(id) => id as i32,
            None => {
                stop_failed_child(child, None).await;
                self.release_starting(&request.process_id).await;
                return Err(remote_error(
                    RemoteErrorCode::Io,
                    "spawned process did not expose an id",
                ));
            }
        };
        let stdin = child.stdin.take();
        let stdout = match child.stdout.take() {
            Some(stdout) => stdout,
            None => {
                stop_failed_child(child, Some(process_group)).await;
                self.release_starting(&request.process_id).await;
                return Err(remote_error(
                    RemoteErrorCode::Io,
                    "spawned process has no stdout",
                ));
            }
        };
        let stderr = match child.stderr.take() {
            Some(stderr) => stderr,
            None => {
                stop_failed_child(child, Some(process_group)).await;
                self.release_starting(&request.process_id).await;
                return Err(remote_error(
                    RemoteErrorCode::Io,
                    "spawned process has no stderr",
                ));
            }
        };
        let handle = ProcessHandle {
            process_group,
            stdin: stdin.map(|stdin| Arc::new(Mutex::new(stdin))),
        };
        let activated = {
            let mut processes = self.processes.lock().await;
            if matches!(
                processes.get(&request.process_id),
                Some(ProcessEntry::Starting)
            ) {
                processes.insert(request.process_id.clone(), ProcessEntry::Running(handle));
                true
            } else {
                false
            }
        };
        if !activated {
            stop_failed_child(child, Some(process_group)).await;
            return Err(remote_error(
                RemoteErrorCode::InvalidRequest,
                format!(
                    "process '{}' reservation was cancelled before activation",
                    request.process_id
                ),
            ));
        }

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
            let mut processes = processes.lock().await;
            if matches!(processes.get(&process_id), Some(ProcessEntry::Running(_))) {
                processes.insert(process_id.clone(), ProcessEntry::Finished);
            }
            drop(processes);
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
        let stdin = match self.processes.lock().await.get(process_id) {
            Some(ProcessEntry::Running(handle)) => handle.stdin.clone(),
            Some(ProcessEntry::Starting | ProcessEntry::Finished) | None => {
                return Err(remote_error(
                    RemoteErrorCode::ProcessNotFound,
                    format!("unknown process id '{process_id}'"),
                ));
            }
        }
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
        let Some(ProcessEntry::Running(handle)) = processes.get_mut(process_id) else {
            return Err(remote_error(
                RemoteErrorCode::ProcessNotFound,
                format!("unknown process id '{process_id}'"),
            ));
        };
        handle.stdin.take();
        Ok(())
    }

    pub(super) async fn terminate(&self, process_id: &str) -> Result<(), RemoteError> {
        let group = match self.processes.lock().await.get(process_id) {
            Some(ProcessEntry::Running(handle)) => handle.process_group,
            Some(ProcessEntry::Starting | ProcessEntry::Finished) | None => {
                return Err(remote_error(
                    RemoteErrorCode::ProcessNotFound,
                    format!("unknown process id '{process_id}'"),
                ));
            }
        };
        terminate_process_group(group);
        Ok(())
    }

    pub(super) async fn terminate_all(&self) {
        let groups = {
            let mut processes = self.processes.lock().await;
            let groups = processes
                .values()
                .filter_map(|entry| match entry {
                    ProcessEntry::Starting | ProcessEntry::Finished => None,
                    ProcessEntry::Running(handle) => Some(handle.process_group),
                })
                .collect::<Vec<_>>();
            processes.clear();
            groups
        };
        for group in groups {
            terminate_process_group(group);
        }
    }

    async fn reserve_starting(&self, process_id: &str) -> Result<(), RemoteError> {
        match self.processes.lock().await.entry(process_id.to_string()) {
            Entry::Vacant(entry) => {
                entry.insert(ProcessEntry::Starting);
                Ok(())
            }
            Entry::Occupied(_) => Err(remote_error(
                RemoteErrorCode::InvalidRequest,
                format!("process '{process_id}' already exists"),
            )),
        }
    }

    async fn release_starting(&self, process_id: &str) {
        let mut processes = self.processes.lock().await;
        if matches!(processes.get(process_id), Some(ProcessEntry::Starting)) {
            processes.remove(process_id);
        }
    }
}

fn user_tool_path(
    home: Option<&OsStr>,
    inherited: Option<&OsStr>,
) -> Result<Option<OsString>, RemoteError> {
    let Some(home) = home else {
        return Ok(inherited.map(OsStr::to_os_string));
    };
    let home = PathBuf::from(home);
    let mut paths = vec![home.join(".cargo/bin"), home.join(".local/bin")];
    if let Some(inherited) = inherited {
        paths.extend(std::env::split_paths(inherited));
    }
    std::env::join_paths(paths).map(Some).map_err(|error| {
        remote_error(
            RemoteErrorCode::InvalidRequest,
            format!("failed to assemble remote process PATH: {error}"),
        )
    })
}

async fn stop_failed_child(mut child: Child, process_group: Option<i32>) {
    if let Some(process_group) = process_group {
        terminate_process_group(process_group);
    } else {
        let _ = child.kill().await;
    }
    let _ = child.wait().await;
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

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;

    fn registry(shell_path: &str) -> ProcessRegistry {
        let writer: SharedWriter = Arc::new(Mutex::new(Box::new(tokio::io::sink())));
        ProcessRegistry::new(
            writer,
            RemoteShellDescriptor {
                dialect: RemoteShellDialect::Sh,
                path: shell_path.to_string(),
            },
        )
    }

    fn request(process_id: &str, command: &str) -> RemoteSpawnRequest {
        RemoteSpawnRequest {
            process_id: process_id.to_string(),
            workspace_id: "workspace".to_string(),
            command: command.to_string(),
            cwd: ".".to_string(),
            environment: BTreeMap::new(),
            capture_path: format!("target/{process_id}.log"),
        }
    }

    #[test]
    fn non_login_process_path_includes_common_user_tool_directories() {
        let path = user_tool_path(
            Some(OsStr::new("/home/runner")),
            Some(OsStr::new("/usr/local/bin:/usr/bin")),
        )
        .unwrap()
        .unwrap();
        let entries = std::env::split_paths(&path).collect::<Vec<_>>();

        assert_eq!(
            entries,
            vec![
                PathBuf::from("/home/runner/.cargo/bin"),
                PathBuf::from("/home/runner/.local/bin"),
                PathBuf::from("/usr/local/bin"),
                PathBuf::from("/usr/bin"),
            ]
        );
    }

    async fn wait_until_idle(registry: &ProcessRegistry) {
        for _ in 0..100 {
            if registry
                .processes
                .lock()
                .await
                .values()
                .all(|entry| matches!(entry, ProcessEntry::Finished))
            {
                return;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        panic!("process registry did not become idle");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn concurrent_spawn_with_the_same_id_allows_only_one_process() {
        let temp = tempfile::tempdir().unwrap();
        let registry = registry("/bin/sh");
        let request = request("proc-shared", "sleep 1");
        let first_capture = temp.path().join("first.log");
        let second_capture = temp.path().join("second.log");

        let (first, second) = tokio::join!(
            registry.spawn(request.clone(), temp.path().to_path_buf(), first_capture),
            registry.spawn(request, temp.path().to_path_buf(), second_capture),
        );

        assert_ne!(first.is_ok(), second.is_ok());
        let error = first.err().or_else(|| second.err()).unwrap();
        assert_eq!(error.code, RemoteErrorCode::InvalidRequest);
        assert!(error.message.contains("already exists"));
        assert_eq!(
            registry
                .processes
                .lock()
                .await
                .values()
                .filter(|entry| matches!(entry, ProcessEntry::Running(_)))
                .count(),
            1
        );

        registry.terminate_all().await;
        assert!(registry.processes.lock().await.is_empty());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn different_process_ids_run_in_parallel_and_cleanup() {
        let temp = tempfile::tempdir().unwrap();
        let registry = registry("/bin/sh");

        let (first, second) = tokio::join!(
            registry.spawn(
                request("proc-a", "printf a"),
                temp.path().to_path_buf(),
                temp.path().join("a.log"),
            ),
            registry.spawn(
                request("proc-b", "printf b"),
                temp.path().to_path_buf(),
                temp.path().join("b.log"),
            ),
        );
        first.unwrap();
        second.unwrap();
        wait_until_idle(&registry).await;
        assert!(
            tokio::fs::read_to_string(temp.path().join("a.log"))
                .await
                .unwrap()
                .contains('a')
        );
        assert!(
            tokio::fs::read_to_string(temp.path().join("b.log"))
                .await
                .unwrap()
                .contains('b')
        );

        let reused = registry
            .spawn(
                request("proc-a", "true"),
                temp.path().to_path_buf(),
                temp.path().join("reused.log"),
            )
            .await
            .unwrap_err();
        assert_eq!(reused.code, RemoteErrorCode::InvalidRequest);
        assert!(reused.message.contains("already exists"));
    }

    #[tokio::test]
    async fn failed_spawn_releases_starting_reservation() {
        let temp = tempfile::tempdir().unwrap();
        let registry = registry("/definitely/missing/pure-shell");
        let request = request("proc-retry", "true");

        for capture in ["first.log", "second.log"] {
            let error = registry
                .spawn(
                    request.clone(),
                    temp.path().to_path_buf(),
                    temp.path().join(capture),
                )
                .await
                .unwrap_err();
            assert_eq!(error.code, RemoteErrorCode::PathNotFound);
            assert!(error.message.contains("failed to spawn remote process"));
            assert!(!error.message.contains("already exists"));
            assert!(registry.processes.lock().await.is_empty());
        }
    }
}

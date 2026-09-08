use std::io;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use pl_protocol::remote::{
    REMOTE_PROTOCOL_VERSION, RemoteCapability, RemoteCopyRequest, RemoteDirectoryEntry,
    RemoteDirectoryListing, RemoteError, RemoteErrorCode, RemoteFileStat, RemoteHello,
    RemoteMessage, RemotePathRequest, RemoteReadRequest, RemoteRemoveRequest, RemoteRenameRequest,
    RemoteRequest, RemoteResponse, RemoteShellDescriptor, RemoteShellDialect,
    RemoteWorkspaceOpened,
};
use tokio::io::AsyncWrite;
use tokio::sync::Mutex;

use crate::ServerError;
use crate::codec::read_frame;
use crate::path::{WorkspaceRegistry, io_error, remote_error};

mod outbound;
mod process;

use outbound::Outbound;
use process::ProcessRegistry;

static NEXT_ATOMIC_WRITE_ID: AtomicU64 = AtomicU64::new(0);

#[derive(Debug)]
struct ServerState {
    workspaces: WorkspaceRegistry,
    shell: RemoteShellDescriptor,
}

pub async fn run_stdio() -> Result<(), ServerError> {
    let stdin = tokio::io::stdin();
    let stdout: Box<dyn AsyncWrite + Send + Unpin> = Box::new(tokio::io::stdout());
    run(stdin, stdout).await
}

async fn run<R>(reader: R, writer: Box<dyn AsyncWrite + Send + Unpin>) -> Result<(), ServerError>
where
    R: tokio::io::AsyncRead + Unpin,
{
    let shell =
        detect_shell().map_err(|_| io::Error::other("remote helper could not find a shell"))?;
    run_with_shell(reader, writer, shell).await
}

async fn run_with_shell<R>(
    reader: R,
    writer: Box<dyn AsyncWrite + Send + Unpin>,
    shell: RemoteShellDescriptor,
) -> Result<(), ServerError>
where
    R: tokio::io::AsyncRead + Unpin,
{
    let (writer, mut writer_task) = Outbound::new(writer);
    let state = Arc::new(Mutex::new(ServerState {
        workspaces: WorkspaceRegistry::default(),
        shell: shell.clone(),
    }));
    let (processes, mut registry_task) = ProcessRegistry::new(writer.clone(), shell);
    let outcome = serve_requests(reader, &writer, &state, &processes).await;
    // EOF, malformed input and write failures all seal the byte stream before cleanup.
    writer.close();
    let cleanup = processes.terminate_all().await;
    let registry_outcome = registry_task.wait().await;
    let write_outcome = writer_task.wait().await.map_err(ServerError::Io);
    cleanup.map_err(ServerError::Cleanup)?;
    registry_outcome.map_err(ServerError::Cleanup)?;
    outcome.and(write_outcome)
}

async fn serve_requests<R>(
    reader: R,
    writer: &Outbound,
    state: &Arc<Mutex<ServerState>>,
    processes: &ProcessRegistry,
) -> Result<(), ServerError>
where
    R: tokio::io::AsyncRead + Unpin,
{
    let mut handlers = tokio::task::JoinSet::new();
    let outcome = receive_requests(reader, writer, state, processes, &mut handlers).await;
    if !matches!(outcome, Ok(RequestEnd::Shutdown { .. })) {
        writer.close();
    }
    let cleanup = processes.terminate_all().await;
    let mut handler_failure = None;
    while let Some(result) = handlers.join_next().await {
        match result {
            Ok(Ok(())) => {}
            Ok(Err(error)) => {
                handler_failure.get_or_insert(error);
            }
            Err(error) => {
                handler_failure.get_or_insert(ServerError::Io(io::Error::other(error)));
            }
        }
    }
    cleanup.map_err(ServerError::Cleanup)?;
    let end = outcome?;
    match end {
        // EOF has deliberately closed the writer. In-flight replies are abandoned,
        // while cleanup and genuine writer failures still propagate independently.
        RequestEnd::Disconnected => Ok(()),
        RequestEnd::Shutdown { request_id } => {
            if let Some(error) = handler_failure {
                return Err(error);
            }
            write_response(writer, request_id, RemoteResponse::Ack, &[]).await?;
            Ok(())
        }
    }
}

enum RequestEnd {
    Disconnected,
    Shutdown { request_id: Option<u64> },
}

async fn receive_requests<R>(
    mut reader: R,
    writer: &Outbound,
    state: &Arc<Mutex<ServerState>>,
    processes: &ProcessRegistry,
    handlers: &mut tokio::task::JoinSet<Result<(), ServerError>>,
) -> Result<RequestEnd, ServerError>
where
    R: tokio::io::AsyncRead + Unpin,
{
    loop {
        // read_frame has partial IO state. Keep the same future when a handler completes.
        let incoming = read_frame(&mut reader);
        tokio::pin!(incoming);
        let frame = loop {
            tokio::select! {
                frame = &mut incoming => break frame?,
                error = writer.failed() => return Err(error.into()),
                result = handlers.join_next(), if !handlers.is_empty() => {
                    match result {
                        Some(Ok(result)) => result?,
                        Some(Err(error)) => return Err(io::Error::other(error).into()),
                        None => {},
                    }
                }
            }
        };
        let Some(frame) = frame else {
            return Ok(RequestEnd::Disconnected);
        };
        let request_id = frame.request_id;
        let request = match frame.message {
            RemoteMessage::Request(request) => request,
            RemoteMessage::Response(_) | RemoteMessage::Event(_) => {
                write_response(
                    writer,
                    request_id,
                    RemoteResponse::Error(remote_error(
                        RemoteErrorCode::InvalidRequest,
                        "helper accepts request messages only",
                    )),
                    &[],
                )
                .await?;
                continue;
            }
        };
        if matches!(request, RemoteRequest::Shutdown) {
            return Ok(RequestEnd::Shutdown { request_id });
        }
        // Control remains reachable even when all ordinary request slots are occupied.
        if let RemoteRequest::Terminate { process_id } = request {
            let response = match processes.terminate(&process_id).await {
                Ok(()) => RemoteResponse::Ack,
                Err(error) => RemoteResponse::Error(error),
            };
            write_response(writer, request_id, response, &[]).await?;
            continue;
        }
        if handlers.len() >= 32 {
            write_response(
                writer,
                request_id,
                RemoteResponse::Error(remote_error(
                    RemoteErrorCode::InvalidRequest,
                    "remote request capacity is exhausted",
                )),
                &[],
            )
            .await?;
            continue;
        }
        let state = state.clone();
        let processes = processes.clone();
        let writer = writer.clone();
        handlers.spawn(async move {
            let outcome = handle_request(&state, &processes, request, frame.body).await;
            let (response, body) = match outcome {
                Ok(outcome) => outcome,
                Err(error) => (RemoteResponse::Error(error), Vec::new()),
            };
            write_response(&writer, request_id, response, &body).await?;
            Ok(())
        });
    }
}

async fn write_response(
    writer: &Outbound,
    request_id: Option<u64>,
    response: RemoteResponse,
    body: &[u8],
) -> io::Result<()> {
    writer
        .write(request_id, RemoteMessage::Response(response), body)
        .await
}

async fn handle_request(
    state: &Arc<Mutex<ServerState>>,
    processes: &ProcessRegistry,
    request: RemoteRequest,
    body: Vec<u8>,
) -> Result<(RemoteResponse, Vec<u8>), RemoteError> {
    match request {
        RemoteRequest::Hello { protocol_version } => {
            let shell = state.lock().await.shell.clone();
            hello(protocol_version, shell)
        }
        RemoteRequest::BrowseDirectories { path } => browse_directories(path).await,
        RemoteRequest::OpenWorkspace { path } => open_workspace(state, &path).await,
        RemoteRequest::CloseWorkspace { workspace_id } => {
            state.lock().await.workspaces.close(&workspace_id)?;
            Ok(ack())
        }
        RemoteRequest::Stat(request) => stat(state, request).await,
        RemoteRequest::ReadBytes(request) => read_bytes(state, request).await,
        RemoteRequest::WriteAtomic(request) => write_atomic(state, request, body).await,
        RemoteRequest::ListDirectory(request) => list_directory(state, request).await,
        RemoteRequest::CreateDirectory(request) => create_directory(state, request).await,
        RemoteRequest::RemovePath(request) => remove_path(state, request).await,
        RemoteRequest::RenamePath(request) => rename_path(state, request).await,
        RemoteRequest::CopyPath(request) => copy_path(state, request).await,
        RemoteRequest::Spawn(request) => {
            let (cwd, capture_path) = {
                let workspaces = state.lock().await.workspaces.clone();
                let cwd = workspaces
                    .resolve_existing(&request.workspace_id, &request.cwd)
                    .await?;
                let capture = workspaces
                    .resolve_for_write(&request.workspace_id, &request.capture_path)
                    .await?;
                (cwd, capture)
            };
            let process_id = request.process_id.clone();
            processes.spawn(request, cwd, capture_path).await?;
            Ok((RemoteResponse::ProcessSpawned { process_id }, Vec::new()))
        }
        RemoteRequest::WriteStdin { process_id } => {
            processes.write_stdin(&process_id, &body).await?;
            Ok(ack())
        }
        RemoteRequest::CloseStdin { process_id } => {
            processes.close_stdin(&process_id).await?;
            Ok(ack())
        }
        RemoteRequest::Terminate { process_id } => {
            processes.terminate(&process_id).await?;
            Ok(ack())
        }
        RemoteRequest::Shutdown => {
            processes.terminate_all().await?;
            Ok(ack())
        }
    }
}

fn hello(
    protocol_version: u32,
    shell: RemoteShellDescriptor,
) -> Result<(RemoteResponse, Vec<u8>), RemoteError> {
    if protocol_version != REMOTE_PROTOCOL_VERSION {
        return Err(remote_error(
            RemoteErrorCode::ProtocolMismatch,
            format!(
                "protocol version {protocol_version} is incompatible with helper version {REMOTE_PROTOCOL_VERSION}"
            ),
        ));
    }
    Ok((
        RemoteResponse::Hello(RemoteHello {
            protocol_version: REMOTE_PROTOCOL_VERSION,
            helper_version: env!("CARGO_PKG_VERSION").to_string(),
            os: std::env::consts::OS.to_string(),
            architecture: std::env::consts::ARCH.to_string(),
            capabilities: vec![
                RemoteCapability::DirectoryBrowse,
                RemoteCapability::WorkspaceFiles,
                RemoteCapability::ObservableExec,
            ],
            shell,
        }),
        Vec::new(),
    ))
}

fn detect_shell() -> Result<RemoteShellDescriptor, RemoteError> {
    for (dialect, path) in [
        (RemoteShellDialect::Bash, "/bin/bash"),
        (RemoteShellDialect::Sh, "/bin/sh"),
    ] {
        if is_executable(Path::new(path)) {
            return Ok(RemoteShellDescriptor {
                dialect,
                path: path.to_string(),
            });
        }
    }
    Err(remote_error(
        RemoteErrorCode::Io,
        "remote helper could not find /bin/bash or /bin/sh",
    ))
}

fn is_executable(path: &Path) -> bool {
    let Ok(metadata) = std::fs::metadata(path) else {
        return false;
    };
    if !metadata.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        metadata.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        true
    }
}

async fn browse_directories(
    path: Option<String>,
) -> Result<(RemoteResponse, Vec<u8>), RemoteError> {
    let path = path.unwrap_or_else(|| std::env::var("HOME").unwrap_or_else(|_| "/".to_string()));
    let canonical = tokio::fs::canonicalize(&path)
        .await
        .map_err(|error| io_error("failed to browse directory", error))?;
    let listing = directory_listing(&canonical, None, true).await?;
    Ok((RemoteResponse::Directories(listing), Vec::new()))
}

async fn open_workspace(
    state: &Arc<Mutex<ServerState>>,
    path: &str,
) -> Result<(RemoteResponse, Vec<u8>), RemoteError> {
    let canonical_path = WorkspaceRegistry::resolve_workspace_root(path).await?;
    let (workspace_id, canonical_path) =
        state.lock().await.workspaces.open_resolved(canonical_path);
    Ok((
        RemoteResponse::WorkspaceOpened(RemoteWorkspaceOpened {
            workspace_id,
            canonical_path: canonical_path.to_string_lossy().into_owned(),
        }),
        Vec::new(),
    ))
}

async fn stat(
    state: &Arc<Mutex<ServerState>>,
    request: RemotePathRequest,
) -> Result<(RemoteResponse, Vec<u8>), RemoteError> {
    let workspaces = state.lock().await.workspaces.clone();
    let path = workspaces
        .resolve_existing(&request.workspace_id, &request.path)
        .await?;
    let metadata = tokio::fs::metadata(&path)
        .await
        .map_err(|error| io_error("failed to stat path", error))?;
    Ok((
        RemoteResponse::Stat(RemoteFileStat {
            path: request.path,
            is_file: metadata.is_file(),
            is_directory: metadata.is_dir(),
            len: metadata.is_file().then_some(metadata.len()),
        }),
        Vec::new(),
    ))
}

async fn read_bytes(
    state: &Arc<Mutex<ServerState>>,
    request: RemoteReadRequest,
) -> Result<(RemoteResponse, Vec<u8>), RemoteError> {
    let workspaces = state.lock().await.workspaces.clone();
    let path = workspaces
        .resolve_existing(&request.workspace_id, &request.path)
        .await?;
    let metadata = tokio::fs::metadata(&path)
        .await
        .map_err(|error| io_error("failed to inspect file", error))?;
    if !metadata.is_file() {
        return Err(remote_error(
            RemoteErrorCode::InvalidRequest,
            "read path is not a regular file",
        ));
    }
    if metadata.len() > request.max_bytes as u64 {
        return Err(remote_error(
            RemoteErrorCode::InvalidRequest,
            format!("file exceeds {} byte limit", request.max_bytes),
        ));
    }
    let bytes = tokio::fs::read(path)
        .await
        .map_err(|error| io_error("failed to read file", error))?;
    Ok((RemoteResponse::Bytes, bytes))
}

async fn write_atomic(
    state: &Arc<Mutex<ServerState>>,
    request: RemotePathRequest,
    body: Vec<u8>,
) -> Result<(RemoteResponse, Vec<u8>), RemoteError> {
    let workspaces = state.lock().await.workspaces.clone();
    let path = workspaces
        .resolve_for_write(&request.workspace_id, &request.path)
        .await?;
    let parent = path.parent().ok_or_else(|| {
        remote_error(
            RemoteErrorCode::InvalidRequest,
            "write path has no parent directory",
        )
    })?;
    tokio::fs::create_dir_all(parent)
        .await
        .map_err(|error| io_error("failed to create write directory", error))?;
    let sequence = NEXT_ATOMIC_WRITE_ID
        .fetch_add(1, Ordering::Relaxed)
        .saturating_add(1);
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("file");
    let temporary = parent.join(format!(
        ".{file_name}.pure-tmp-{}-{sequence}",
        std::process::id()
    ));
    tokio::fs::write(&temporary, body)
        .await
        .map_err(|error| io_error("failed to write temporary file", error))?;
    if let Err(error) = tokio::fs::rename(&temporary, &path).await {
        let _ = tokio::fs::remove_file(&temporary).await;
        return Err(io_error("failed to publish file", error));
    }
    Ok(ack())
}

async fn list_directory(
    state: &Arc<Mutex<ServerState>>,
    request: RemotePathRequest,
) -> Result<(RemoteResponse, Vec<u8>), RemoteError> {
    let workspaces = state.lock().await.workspaces.clone();
    let path = workspaces
        .resolve_existing(&request.workspace_id, &request.path)
        .await?;
    let listing = directory_listing(&path, Some(request.path), false).await?;
    Ok((RemoteResponse::Directory(listing), Vec::new()))
}

async fn create_directory(
    state: &Arc<Mutex<ServerState>>,
    request: RemotePathRequest,
) -> Result<(RemoteResponse, Vec<u8>), RemoteError> {
    let workspaces = state.lock().await.workspaces.clone();
    let path = workspaces
        .resolve_for_write(&request.workspace_id, &request.path)
        .await?;
    tokio::fs::create_dir_all(path)
        .await
        .map_err(|error| io_error("failed to create directory", error))?;
    Ok(ack())
}

async fn remove_path(
    state: &Arc<Mutex<ServerState>>,
    request: RemoteRemoveRequest,
) -> Result<(RemoteResponse, Vec<u8>), RemoteError> {
    let workspaces = state.lock().await.workspaces.clone();
    let path = workspaces
        .resolve_existing(&request.workspace_id, &request.path)
        .await?;
    let metadata = tokio::fs::metadata(&path)
        .await
        .map_err(|error| io_error("failed to inspect removal target", error))?;
    if metadata.is_dir() {
        if request.recursive {
            tokio::fs::remove_dir_all(path)
                .await
                .map_err(|error| io_error("failed to remove directory", error))?;
        } else {
            tokio::fs::remove_dir(path)
                .await
                .map_err(|error| io_error("failed to remove empty directory", error))?;
        }
    } else {
        tokio::fs::remove_file(path)
            .await
            .map_err(|error| io_error("failed to remove file", error))?;
    }
    Ok(ack())
}

async fn rename_path(
    state: &Arc<Mutex<ServerState>>,
    request: RemoteRenameRequest,
) -> Result<(RemoteResponse, Vec<u8>), RemoteError> {
    let workspaces = state.lock().await.workspaces.clone();
    let source = workspaces
        .resolve_existing(&request.workspace_id, &request.source)
        .await?;
    let target = workspaces
        .resolve_for_write(&request.workspace_id, &request.target)
        .await?;
    if let Some(parent) = target.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|error| io_error("failed to create rename directory", error))?;
    }
    tokio::fs::rename(source, target)
        .await
        .map_err(|error| io_error("failed to rename path", error))?;
    Ok(ack())
}

async fn copy_path(
    state: &Arc<Mutex<ServerState>>,
    request: RemoteCopyRequest,
) -> Result<(RemoteResponse, Vec<u8>), RemoteError> {
    let workspaces = state.lock().await.workspaces.clone();
    let source = workspaces
        .resolve_existing(&request.workspace_id, &request.source)
        .await?;
    let target = workspaces
        .resolve_for_write(&request.workspace_id, &request.target)
        .await?;
    let metadata = tokio::fs::metadata(&source)
        .await
        .map_err(|error| io_error("failed to inspect copy source", error))?;
    if metadata.is_dir() {
        if !request.recursive {
            return Err(remote_error(
                RemoteErrorCode::InvalidRequest,
                "directory copy requires recursive=true",
            ));
        }
        copy_directory(&source, &target).await?;
    } else {
        if let Some(parent) = target.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(|error| io_error("failed to create copy directory", error))?;
        }
        tokio::fs::copy(source, target)
            .await
            .map_err(|error| io_error("failed to copy file", error))?;
    }
    Ok(ack())
}

async fn directory_listing(
    path: &Path,
    display_path: Option<String>,
    directories_only: bool,
) -> Result<RemoteDirectoryListing, RemoteError> {
    let mut reader = tokio::fs::read_dir(path)
        .await
        .map_err(|error| io_error("failed to read directory", error))?;
    let mut entries = Vec::new();
    while let Some(entry) = reader
        .next_entry()
        .await
        .map_err(|error| io_error("failed to read directory entry", error))?
    {
        let metadata = tokio::fs::symlink_metadata(entry.path())
            .await
            .map_err(|error| io_error("failed to inspect directory entry", error))?;
        let is_symlink = metadata.file_type().is_symlink();
        let is_directory = if is_symlink {
            tokio::fs::metadata(entry.path())
                .await
                .is_ok_and(|target| target.is_dir())
        } else {
            metadata.is_dir()
        };
        if directories_only && !is_directory {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        let entry_path = display_path
            .as_deref()
            .map(|display| {
                if display.is_empty() || display == "." {
                    name.clone()
                } else {
                    format!("{}/{name}", display.trim_end_matches('/'))
                }
            })
            .unwrap_or_else(|| entry.path().to_string_lossy().into_owned());
        entries.push(RemoteDirectoryEntry {
            name,
            path: entry_path,
            is_directory,
            is_symlink,
        });
    }
    entries.sort_by(|left, right| left.name.cmp(&right.name));
    Ok(RemoteDirectoryListing {
        path: display_path.unwrap_or_else(|| path.to_string_lossy().into_owned()),
        parent: path
            .parent()
            .map(|parent| parent.to_string_lossy().into_owned()),
        entries,
    })
}

async fn copy_directory(source: &Path, target: &Path) -> Result<(), RemoteError> {
    let mut stack = vec![(source.to_path_buf(), target.to_path_buf())];
    while let Some((source_dir, target_dir)) = stack.pop() {
        tokio::fs::create_dir_all(&target_dir)
            .await
            .map_err(|error| io_error("failed to create copied directory", error))?;
        let mut reader = tokio::fs::read_dir(&source_dir)
            .await
            .map_err(|error| io_error("failed to read copied directory", error))?;
        while let Some(entry) = reader
            .next_entry()
            .await
            .map_err(|error| io_error("failed to read copied entry", error))?
        {
            let source_path = entry.path();
            let target_path = target_dir.join(entry.file_name());
            let metadata = tokio::fs::symlink_metadata(&source_path)
                .await
                .map_err(|error| io_error("failed to inspect copied entry", error))?;
            if metadata.file_type().is_symlink() {
                return Err(remote_error(
                    RemoteErrorCode::WorkspaceEscape,
                    "copying symbolic links is not allowed",
                ));
            }
            if metadata.is_dir() {
                stack.push((source_path, target_path));
            } else if metadata.is_file() {
                tokio::fs::copy(source_path, target_path)
                    .await
                    .map_err(|error| io_error("failed to copy entry", error))?;
            }
        }
    }
    Ok(())
}

fn ack() -> (RemoteResponse, Vec<u8>) {
    (RemoteResponse::Ack, Vec::new())
}

#[cfg(test)]
mod tests {
    use pl_protocol::remote::{
        REMOTE_PROTOCOL_VERSION, RemoteFrameHeader, RemoteRequest, RemoteResponse,
    };
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    use super::*;

    fn test_shell() -> RemoteShellDescriptor {
        RemoteShellDescriptor {
            dialect: RemoteShellDialect::Sh,
            path: "/test/sh".to_string(),
        }
    }

    #[tokio::test]
    async fn direct_stdio_supports_workspace_file_round_trip() {
        let temp = tempfile::tempdir().expect("tempdir");
        let (mut client, server) = tokio::io::duplex(64 * 1024);
        let server_task = tokio::spawn(async move {
            run_with_shell(server, Box::new(tokio::io::sink()), test_shell()).await
        });

        let request = RemoteFrameHeader {
            request_id: Some(1),
            message: RemoteMessage::Request(RemoteRequest::OpenWorkspace {
                path: temp.path().to_string_lossy().into_owned(),
            }),
            body_len: 0,
        };
        let bytes = serde_json::to_vec(&request).expect("header");
        client.write_u32(bytes.len() as u32).await.expect("length");
        client.write_all(&bytes).await.expect("header bytes");
        client.shutdown().await.expect("shutdown client");
        let mut discard = Vec::new();
        client.read_to_end(&mut discard).await.expect("read end");
        server_task.await.expect("join server").expect("server");
    }

    #[test]
    fn hello_reports_the_shell_used_by_process_registry() {
        let shell = test_shell();
        let (response, _) = hello(REMOTE_PROTOCOL_VERSION, shell.clone()).expect("hello");
        let RemoteResponse::Hello(hello) = response else {
            panic!("expected hello response");
        };
        assert_eq!(hello.shell, shell);
        assert!(!hello.shell.path.is_empty());
    }

    #[test]
    fn hello_rejects_unknown_protocol_version() {
        let shell = test_shell();
        let error = hello(REMOTE_PROTOCOL_VERSION + 1, shell).expect_err("mismatch");
        assert_eq!(
            error.code,
            pl_protocol::remote::RemoteErrorCode::ProtocolMismatch
        );
    }
}

use std::io;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use pl_protocol::remote::{
    REMOTE_PROTOCOL_VERSION, RemoteCapability, RemoteCopyRequest, RemoteDirectoryEntry,
    RemoteDirectoryListing, RemoteError, RemoteErrorCode, RemoteFileStat, RemoteHello,
    RemoteMessage, RemotePathRequest, RemoteReadRequest, RemoteRemoveRequest, RemoteRenameRequest,
    RemoteRequest, RemoteResponse, RemoteWorkspaceOpened,
};
use tokio::io::AsyncWrite;
use tokio::sync::Mutex;

use crate::codec::{read_frame, write_frame};
use crate::path::{WorkspaceRegistry, io_error, remote_error};

mod process;

use process::ProcessRegistry;

static NEXT_ATOMIC_WRITE_ID: AtomicU64 = AtomicU64::new(0);

type SharedWriter = Arc<Mutex<Box<dyn AsyncWrite + Send + Unpin>>>;

#[derive(Debug, thiserror::Error)]
pub enum ServerError {
    #[error("remote helper stdio failed: {0}")]
    Io(#[from] io::Error),
}

#[derive(Debug, Default)]
struct ServerState {
    workspaces: WorkspaceRegistry,
}

pub async fn run_stdio() -> Result<(), ServerError> {
    let stdin = tokio::io::stdin();
    let stdout: Box<dyn AsyncWrite + Send + Unpin> = Box::new(tokio::io::stdout());
    run(stdin, stdout).await
}

async fn run<R>(
    mut reader: R,
    writer: Box<dyn AsyncWrite + Send + Unpin>,
) -> Result<(), ServerError>
where
    R: tokio::io::AsyncRead + Unpin,
{
    let writer = Arc::new(Mutex::new(writer));
    let state = Arc::new(Mutex::new(ServerState::default()));
    let processes = ProcessRegistry::new(writer.clone());
    while let Some(frame) = read_frame(&mut reader).await? {
        let request_id = frame.request_id;
        let request = match frame.message {
            RemoteMessage::Request(request) => request,
            RemoteMessage::Response(_) | RemoteMessage::Event(_) => {
                write_response(
                    &writer,
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
        let shutdown = matches!(request, RemoteRequest::Shutdown);
        let outcome = handle_request(&state, &processes, request, frame.body).await;
        match outcome {
            Ok((response, body)) => {
                write_response(&writer, request_id, response, &body).await?;
            }
            Err(error) => {
                write_response(&writer, request_id, RemoteResponse::Error(error), &[]).await?;
            }
        }
        if shutdown {
            break;
        }
    }
    processes.terminate_all().await;
    Ok(())
}

async fn write_response(
    writer: &SharedWriter,
    request_id: Option<u64>,
    response: RemoteResponse,
    body: &[u8],
) -> io::Result<()> {
    write_frame(
        &mut *writer.lock().await,
        request_id,
        RemoteMessage::Response(response),
        body,
    )
    .await
}

async fn handle_request(
    state: &Arc<Mutex<ServerState>>,
    processes: &ProcessRegistry,
    request: RemoteRequest,
    body: Vec<u8>,
) -> Result<(RemoteResponse, Vec<u8>), RemoteError> {
    match request {
        RemoteRequest::Hello { protocol_version } => hello(protocol_version),
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
            processes.terminate_all().await;
            Ok(ack())
        }
    }
}

fn hello(protocol_version: u32) -> Result<(RemoteResponse, Vec<u8>), RemoteError> {
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
        }),
        Vec::new(),
    ))
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
    use pl_protocol::remote::{RemoteFrameHeader, RemoteRequest};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    use super::*;

    #[tokio::test]
    async fn direct_stdio_supports_workspace_file_round_trip() {
        let temp = tempfile::tempdir().expect("tempdir");
        let (mut client, server) = tokio::io::duplex(64 * 1024);
        let server_task =
            tokio::spawn(async move { run(server, Box::new(tokio::io::sink())).await });

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
}

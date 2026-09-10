//! Filesystem write modes run on the worker, after workspace resolution.
use pl_protocol::remote::{RemoteError, RemoteResponse, RemoteWriteMode, RemoteWriteRequest};
use std::{
    io::{self, Write},
    path::Path,
    sync::Arc,
};
use tokio::sync::Mutex;

use super::{ServerState, ack};
use crate::path::io_error;

pub(super) async fn write(
    state: &Arc<Mutex<ServerState>>,
    request: RemoteWriteRequest,
    body: Vec<u8>,
) -> Result<(RemoteResponse, Vec<u8>), RemoteError> {
    let workspaces = state.lock().await.workspaces.clone();
    let path = workspaces
        .resolve_for_write(&request.target.workspace_id, &request.target.path)
        .await?;
    tokio::task::spawn_blocking(move || write_file(&path, &body, request.mode))
        .await
        .map_err(|error| io_error("file writer task failed", io::Error::other(error)))?
        .map_err(|error| io_error("failed to write workspace file", error))?;
    Ok(ack())
}

fn write_file(path: &Path, content: &[u8], mode: RemoteWriteMode) -> io::Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::other("write path has no parent"))?;
    std::fs::create_dir_all(parent)?;
    match mode {
        RemoteWriteMode::Create => {
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(path)?;
            file.write_all(content)?;
            file.sync_all()?;
        }
        RemoteWriteMode::Append => {
            let mut file = std::fs::OpenOptions::new()
                .append(true)
                .create(true)
                .open(path)?;
            file.write_all(content)?;
            file.sync_all()?;
        }
        RemoteWriteMode::Overwrite => {
            let permissions = match std::fs::metadata(path) {
                Ok(metadata) => Some(metadata.permissions()),
                Err(error) if error.kind() == io::ErrorKind::NotFound => None,
                Err(error) => return Err(error),
            };
            let mut builder = tempfile::Builder::new();
            builder.prefix(".pure-write-");
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                builder.permissions(std::fs::Permissions::from_mode(0o666));
            }
            let mut temporary = builder.tempfile_in(parent)?;
            temporary.write_all(content)?;
            if let Some(permissions) = permissions {
                temporary.as_file().set_permissions(permissions)?;
            }
            temporary.as_file().sync_all()?;
            temporary.persist(path).map_err(|error| error.error)?;
        }
    }
    #[cfg(unix)]
    std::fs::File::open(parent)?.sync_all()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn create_is_exclusive_and_append_preserves_existing_binary_bytes() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("data");
        write_file(&path, &[0, 255, 1], RemoteWriteMode::Create).unwrap();
        assert_eq!(
            write_file(&path, b"replace", RemoteWriteMode::Create)
                .unwrap_err()
                .kind(),
            io::ErrorKind::AlreadyExists
        );
        write_file(&path, &[2, 0, 3], RemoteWriteMode::Append).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), vec![0, 255, 1, 2, 0, 3]);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        write_file(&path, b"new", RemoteWriteMode::Overwrite).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o755
            );
        }
        assert_eq!(std::fs::read(&path).unwrap(), b"new");
        assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 1);
    }
}

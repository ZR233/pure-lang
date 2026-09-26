//! Archive remote output only after a complete remote transfer into local staging.
use std::sync::Arc;

use pl_core::{context::ResourceReference, tool::opaque::ToolError};
use pl_tool::{exec::CommandOutputArchive, remote::RemoteWorkspaceFileBackend};

use super::MAX_COMMAND_ARCHIVE_BYTES;
use super::{FileResourceStore, ResourceStoreError, retention_failure, retention_io_fault};

#[derive(Debug, Clone)]
pub struct RemoteCommandOutputArchive {
    store: FileResourceStore,
    files: Arc<RemoteWorkspaceFileBackend>,
}

impl FileResourceStore {
    /// Binds the exact remote workspace lease that produced the command capture.
    pub fn remote_command_archive(
        &self,
        files: Arc<RemoteWorkspaceFileBackend>,
    ) -> RemoteCommandOutputArchive {
        RemoteCommandOutputArchive {
            store: self.clone(),
            files,
        }
    }
}

impl CommandOutputArchive for RemoteCommandOutputArchive {
    async fn retain(
        &self,
        _: &str,
        snapshot: &pl_tool::command::CommandOutputSnapshot,
    ) -> Result<ResourceReference, ToolError> {
        let path = snapshot
            .capture_file
            .to_str()
            .ok_or_else(|| ToolError::new(ResourceStoreError::Identity))?
            .replace('\\', "/");
        let root = self.store.root.clone();
        let staging = tokio::task::spawn_blocking(move || {
            std::fs::create_dir_all(&*root)
                .and_then(|()| tempfile::NamedTempFile::new_in(&*root))
                .map_err(|source| ResourceStoreError::Io {
                    path: (*root).clone(),
                    source,
                })
        })
        .await
        .map_err(ToolError::new)?
        .map_err(retention_failure)?;
        let local = staging.as_file().try_clone().map_err(retention_io_fault)?;
        let mut writer = tokio::fs::File::from_std(local);
        // Bound the transfer by the authoritative remote length before staging any byte, so a runaway
        // capture is refused without a full download. A local staging write failure is a storage
        // obligation; a transport/oversize failure is not, so it stays a plain tool error.
        self.files
            .copy_capture_bounded(&path, &mut writer, MAX_COMMAND_ARCHIVE_BYTES)
            .await
            .map_err(remote_download_failure)?;
        writer.sync_all().await.map_err(retention_io_fault)?;
        drop(writer);
        self.store
            .retain_command_capture(staging.path())
            .await
            .map_err(retention_failure)
    }
}

/// Maps a bounded remote-capture transfer failure to the tool boundary.
///
/// A local staging write failure is the same typed storage fact a local capture failure is, so it is
/// reported through the obligation boundary. A path, framing, transport or oversize failure is not a
/// storage fault: it stays a plain tool error, and the category is taken from the typed variant
/// rather than parsed out of the text.
fn remote_download_failure(error: pl_tool::remote::RemoteDownloadError) -> ToolError {
    use pl_tool::remote::RemoteDownloadError;
    match error {
        RemoteDownloadError::Write { source, .. } => retention_io_fault(source),
        other => ToolError::new(other),
    }
}

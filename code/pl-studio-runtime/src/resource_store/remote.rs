//! Archive remote output only after a complete remote transfer into local staging.
use std::sync::Arc;

use pl_core::{context::ResourceReference, tool::opaque::ToolError};
use pl_tool::{exec::CommandOutputArchive, remote::RemoteWorkspaceFileBackend};

use super::{FileResourceStore, ResourceStoreError};

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
        .map_err(ToolError::new)?;
        let local = staging.as_file().try_clone().map_err(ToolError::new)?;
        let mut writer = tokio::fs::File::from_std(local);
        self.files
            .copy_file_to(&path, &mut writer)
            .await
            .map_err(ToolError::new)?;
        writer.sync_all().await.map_err(ToolError::new)?;
        drop(writer);
        self.store
            .retain_file(staging.path(), "application/octet-stream")
            .await
            .map_err(ToolError::new)
    }
}

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use pl_patch::{PatchBackend, PatchError, PatchFileChange, PatchPathDisplay, PatchResult};
use pl_protocol::{PureError, Result};
use serde::Serialize;

use super::backend::*;
use super::schema::TOOL_APPLY_PATCH;
use crate::tool::tool_error;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspacePatchOutput {
    pub cwd: String,
    pub added: Vec<String>,
    pub updated: Vec<String>,
    pub deleted: Vec<String>,
    pub moved: Vec<WorkspacePatchMove>,
    pub changed_files: Vec<String>,
    pub stdout: String,
    pub stderr: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub summary: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspacePatchMove {
    pub from: String,
    pub to: String,
}

pub async fn apply_patch_to_backend<B>(
    backend: &B,
    cwd: String,
    patch: &str,
) -> Result<WorkspacePatchOutput>
where
    B: WorkspaceFileBackend,
{
    let patch_backend = WorkspacePatchBackend { backend, cwd: &cwd };
    let outcome = pl_patch::apply_patch(patch, &patch_backend)
        .await
        .map_err(|error| tool_error(TOOL_APPLY_PATCH, error.into_message()))?;
    let summary = outcome.summary(&patch_backend);
    let mut added = Vec::new();
    let mut updated = Vec::new();
    let mut deleted = Vec::new();
    let mut moved = Vec::new();
    let mut changed_files = BTreeSet::new();

    for change in outcome.file_changes() {
        match change {
            PatchFileChange::Add { path } => {
                let path = patch_backend.display_path(&path);
                changed_files.insert(path.clone());
                added.push(path);
            }
            PatchFileChange::Update { path } => {
                let path = patch_backend.display_path(&path);
                changed_files.insert(path.clone());
                updated.push(path);
            }
            PatchFileChange::Delete { path } => {
                let path = patch_backend.display_path(&path);
                changed_files.insert(path.clone());
                deleted.push(path);
            }
            PatchFileChange::Move { source, target } => {
                let from = patch_backend.display_path(&source);
                let to = patch_backend.display_path(&target);
                changed_files.insert(from.clone());
                changed_files.insert(to.clone());
                moved.push(WorkspacePatchMove { from, to });
            }
        }
    }

    Ok(WorkspacePatchOutput {
        cwd,
        added,
        updated,
        deleted,
        moved,
        changed_files: changed_files.into_iter().collect(),
        stdout: "apply_patch completed".to_string(),
        stderr: String::new(),
        summary,
    })
}

#[derive(Debug)]
struct WorkspacePatchBackend<'a, B> {
    backend: &'a B,
    cwd: &'a str,
}

impl<B> PatchPathDisplay for WorkspacePatchBackend<'_, B> {
    fn display_path(&self, path: &Path) -> String {
        path.to_string_lossy().replace('\\', "/")
    }
}

impl<B> PatchBackend for WorkspacePatchBackend<'_, B>
where
    B: WorkspaceFileBackend,
{
    async fn resolve_existing<'a>(&'a self, path: &'a str) -> PatchResult<PathBuf> {
        self.backend
            .stat(WorkspaceFileStatRequest {
                path: path.to_string(),
                cwd: Some(self.cwd.to_string()),
            })
            .await
            .map_err(patch_error)?;
        Ok(PathBuf::from(path))
    }

    async fn resolve_for_write<'a>(&'a self, path: &'a str) -> PatchResult<PathBuf> {
        Ok(PathBuf::from(path))
    }

    async fn reject_symlink_write<'a>(&'a self, _path: &'a Path) -> PatchResult<()> {
        Ok(())
    }

    async fn ensure_file<'a>(&'a self, path: &'a Path) -> PatchResult<()> {
        let stat = self
            .backend
            .stat(WorkspaceFileStatRequest {
                path: self.display_path(path),
                cwd: Some(self.cwd.to_string()),
            })
            .await
            .map_err(patch_error)?;
        if stat.is_file {
            Ok(())
        } else {
            Err(PatchError::new(format!(
                "cannot delete '{}': path is not a file",
                self.display_path(path)
            )))
        }
    }

    async fn read_to_string<'a>(&'a self, path: &'a Path) -> PatchResult<String> {
        self.backend
            .read_text(WorkspaceFileReadRequest {
                path: self.display_path(path),
                cwd: Some(self.cwd.to_string()),
            })
            .await
            .map_err(patch_error)
    }

    async fn read_optional_text<'a>(&'a self, path: &'a Path) -> PatchResult<Option<String>> {
        let path = self.display_path(path);
        let stat = self
            .backend
            .stat(WorkspaceFileStatRequest {
                path: path.clone(),
                cwd: Some(self.cwd.to_string()),
            })
            .await;
        if !stat.is_ok_and(|stat| stat.is_file) {
            return Ok(None);
        }
        self.backend
            .read_text(WorkspaceFileReadRequest {
                path,
                cwd: Some(self.cwd.to_string()),
            })
            .await
            .map(Some)
            .map_err(patch_error)
    }

    async fn create_parent_dirs<'a>(&'a self, _path: &'a Path) -> PatchResult<()> {
        Ok(())
    }

    async fn write_text<'a>(&'a self, path: &'a Path, content: &'a str) -> PatchResult<()> {
        self.backend
            .write_text(WorkspaceFileWriteRequest {
                path: self.display_path(path),
                cwd: Some(self.cwd.to_string()),
                content: content.to_string(),
            })
            .await
            .map_err(patch_error)
    }

    async fn remove_file<'a>(&'a self, path: &'a Path) -> PatchResult<()> {
        self.backend
            .remove_file(WorkspaceFileRemoveRequest {
                path: self.display_path(path),
                cwd: Some(self.cwd.to_string()),
            })
            .await
            .map_err(patch_error)
    }
}

fn patch_error(error: PureError) -> PatchError {
    let message = match error {
        PureError::ToolExecutionFailed { error, .. } => error,
        error => error.to_string(),
    };
    PatchError::new(message)
}

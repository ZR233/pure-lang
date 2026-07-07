use std::collections::BTreeSet;
use std::path::Path;

use pl_protocol::{PureError, Result};
use serde::Serialize;

use crate::tool::file::apply_patch::{apply_chunks, parse_patch};

use super::backend::{
    WorkspaceFileBackend, WorkspaceFileReadRequest, WorkspaceFileRemoveRequest,
    WorkspaceFileWriteRequest,
};
use super::ops::tool_error;
use super::schema::TOOL_APPLY_PATCH;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
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
pub struct WorkspacePatchMove {
    pub from: String,
    pub to: String,
}

#[derive(Debug, Default)]
struct PatchProgress {
    added: Vec<String>,
    updated: Vec<String>,
    deleted: Vec<String>,
    moved: Vec<WorkspacePatchMove>,
}

impl PatchProgress {
    fn summary(&self) -> String {
        let mut output = String::from("Success. Updated the following files:\n");
        for path in &self.added {
            output.push_str(&format!("A {path}\n"));
        }
        for path in &self.updated {
            output.push_str(&format!("M {path}\n"));
        }
        for path in &self.deleted {
            output.push_str(&format!("D {path}\n"));
        }
        for item in &self.moved {
            output.push_str(&format!("M {} -> {}\n", item.from, item.to));
        }
        output
    }

    fn failure_suffix(&self) -> String {
        if self.added.is_empty()
            && self.updated.is_empty()
            && self.deleted.is_empty()
            && self.moved.is_empty()
        {
            return "\nNo files were modified before failure.".to_string();
        }

        let mut output = String::from("\nCommitted changes before failure:\n");
        for path in &self.added {
            output.push_str(&format!("A {path}\n"));
        }
        for path in &self.updated {
            output.push_str(&format!("M {path}\n"));
        }
        for path in &self.deleted {
            output.push_str(&format!("D {path}\n"));
        }
        for item in &self.moved {
            output.push_str(&format!("M {} -> {}\n", item.from, item.to));
        }
        output
    }

    fn output(self, cwd: String) -> WorkspacePatchOutput {
        let summary = self.summary();
        let mut changed_files = BTreeSet::new();
        changed_files.extend(self.added.iter().cloned());
        changed_files.extend(self.updated.iter().cloned());
        changed_files.extend(self.deleted.iter().cloned());
        for item in &self.moved {
            changed_files.insert(item.to.clone());
        }
        WorkspacePatchOutput {
            cwd,
            added: self.added,
            updated: self.updated,
            deleted: self.deleted,
            moved: self.moved,
            changed_files: changed_files.into_iter().collect(),
            stdout: "apply_patch completed".to_string(),
            stderr: String::new(),
            summary,
        }
    }
}

pub async fn apply_patch_to_backend<B>(
    backend: &B,
    cwd: String,
    patch: &str,
) -> Result<WorkspacePatchOutput>
where
    B: WorkspaceFileBackend,
{
    let hunks = parse_patch(patch)?;
    let mut progress = PatchProgress::default();
    for hunk in hunks {
        if let Err(error) = apply_hunk(backend, &cwd, hunk, &mut progress).await {
            let error = error_message(error);
            return Err(tool_error(
                TOOL_APPLY_PATCH,
                format!("{error}{}", progress.failure_suffix()),
            ));
        }
    }
    Ok(progress.output(cwd))
}

async fn apply_hunk<B>(
    backend: &B,
    cwd: &str,
    hunk: crate::tool::file::apply_patch::Hunk,
    progress: &mut PatchProgress,
) -> Result<()>
where
    B: WorkspaceFileBackend,
{
    match hunk {
        crate::tool::file::apply_patch::Hunk::Add { path, content } => {
            backend
                .write_text(WorkspaceFileWriteRequest {
                    path: path.clone(),
                    cwd: Some(cwd.to_string()),
                    content,
                })
                .await?;
            progress.added.push(path);
        }
        crate::tool::file::apply_patch::Hunk::Delete { path } => {
            backend
                .read_text(WorkspaceFileReadRequest {
                    path: path.clone(),
                    cwd: Some(cwd.to_string()),
                })
                .await?;
            backend
                .remove_file(WorkspaceFileRemoveRequest {
                    path: path.clone(),
                    cwd: Some(cwd.to_string()),
                })
                .await?;
            progress.deleted.push(path);
        }
        crate::tool::file::apply_patch::Hunk::Update {
            path,
            move_path,
            chunks,
        } => {
            let old_content = backend
                .read_text(WorkspaceFileReadRequest {
                    path: path.clone(),
                    cwd: Some(cwd.to_string()),
                })
                .await?;
            let new_content = if chunks.is_empty() {
                old_content
            } else {
                apply_chunks(&old_content, Path::new(&path), &chunks)?
            };
            let target = move_path.unwrap_or_else(|| path.clone());
            backend
                .write_text(WorkspaceFileWriteRequest {
                    path: target.clone(),
                    cwd: Some(cwd.to_string()),
                    content: new_content,
                })
                .await?;
            if target != path {
                backend
                    .remove_file(WorkspaceFileRemoveRequest {
                        path: path.clone(),
                        cwd: Some(cwd.to_string()),
                    })
                    .await?;
                progress.moved.push(WorkspacePatchMove {
                    from: path.clone(),
                    to: target,
                });
            }
            progress.updated.push(path);
        }
    }
    Ok(())
}

fn error_message(error: PureError) -> String {
    match error {
        PureError::ToolExecutionFailed { error, .. } => error,
        error => error.to_string(),
    }
}

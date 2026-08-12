use std::collections::BTreeSet;
use std::path::Path;

use pl_protocol::{PureError, Result};
use serde::Serialize;

use crate::tool::file::apply_patch::{CodexPatchHunk, apply_chunks, parse_codex_patch};

use super::backend::*;
use super::ops::tool_error;
use super::schema::TOOL_APPLY_PATCH;

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

#[derive(Debug, Default)]
struct PatchProgress {
    added: Vec<String>,
    updated: Vec<String>,
    deleted: Vec<String>,
    moved: Vec<WorkspacePatchMove>,
    in_flight: Option<String>,
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
        let completed_changes = !self.added.is_empty()
            || !self.updated.is_empty()
            || !self.deleted.is_empty()
            || !self.moved.is_empty();
        if !completed_changes && self.in_flight.is_none() {
            return "\nNo files were modified before failure.".to_string();
        }

        let mut output = if completed_changes {
            let mut output = String::from("\nChanges applied before failure:\n");
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
        } else {
            "\nNo changes completed before failure.\n".to_string()
        };
        if let Some(change) = &self.in_flight {
            output.push_str(
                "An in-flight operation may have partially applied this additional change:\n",
            );
            output.push_str(&format!("? {change}\n"));
        }
        output
    }

    fn begin_change(&mut self, change: String) {
        debug_assert!(self.in_flight.is_none());
        self.in_flight = Some(change);
    }

    fn complete_change(&mut self) {
        debug_assert!(self.in_flight.is_some());
        self.in_flight = None;
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
    let hunks = parse_codex_patch(patch)?;
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
    hunk: CodexPatchHunk,
    progress: &mut PatchProgress,
) -> Result<()>
where
    B: WorkspaceFileBackend,
{
    match hunk {
        CodexPatchHunk::Add { path, content } => {
            progress.begin_change(format!("A {path}"));
            backend
                .write_text(WorkspaceFileWriteRequest {
                    path: path.clone(),
                    cwd: Some(cwd.to_string()),
                    content,
                })
                .await?;
            progress.complete_change();
            progress.added.push(path);
        }
        CodexPatchHunk::Delete { path } => {
            backend
                .read_text(WorkspaceFileReadRequest {
                    path: path.clone(),
                    cwd: Some(cwd.to_string()),
                })
                .await?;
            progress.begin_change(format!("D {path}"));
            backend
                .remove_file(WorkspaceFileRemoveRequest {
                    path: path.clone(),
                    cwd: Some(cwd.to_string()),
                })
                .await?;
            progress.complete_change();
            progress.deleted.push(path);
        }
        CodexPatchHunk::Update {
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
            let change = if target == path {
                format!("M {path}")
            } else {
                format!("M {path} -> {target}")
            };
            progress.begin_change(change);
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
            progress.complete_change();
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

#[cfg(test)]
mod tests {
    use super::PatchProgress;

    #[test]
    fn failure_suffix_reports_completed_and_in_flight_changes_separately() {
        let progress = PatchProgress {
            added: vec!["created.txt".to_string()],
            in_flight: Some("M source.txt -> target.txt".to_string()),
            ..PatchProgress::default()
        };

        let suffix = progress.failure_suffix();

        assert!(suffix.contains("Changes applied before failure:\nA created.txt"));
        assert!(suffix.contains("in-flight operation may have partially applied"));
        assert!(suffix.contains("? M source.txt -> target.txt"));
        assert!(!suffix.contains("Committed changes"));
    }
}

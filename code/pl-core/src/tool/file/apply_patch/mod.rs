use std::path::{Path, PathBuf};

use pl_protocol::PureError;

use super::path::WorkspacePaths;

mod matcher;
mod parser;

use matcher::apply_chunks;
use parser::{Hunk, parse_patch};

pub const APPLY_PATCH_LARK_GRAMMAR: &str = r#"start: begin_patch environment_id? hunk+ end_patch
begin_patch: "*** Begin Patch" LF
environment_id: "*** Environment ID: " filename LF
end_patch: "*** End Patch" LF?

hunk: add_hunk | delete_hunk | update_hunk
add_hunk: "*** Add File: " filename LF add_line+
delete_hunk: "*** Delete File: " filename LF
update_hunk: "*** Update File: " filename LF change_move? change?

filename: /(.+)/
add_line: "+" /(.*)/ LF -> line

change_move: "*** Move to: " filename LF
change: (change_context | change_line)+ eof_line?
change_context: ("@@" | "@@ " /(.+)/) LF
change_line: ("+" | "-" | " ")? /(.*)/ LF
eof_line: "*** End of File" LF

%import common.LF
"#;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PatchOutcome {
    committed: Vec<CommittedChange>,
    exact: bool,
}

impl Default for PatchOutcome {
    fn default() -> Self {
        Self {
            committed: Vec::new(),
            exact: true,
        }
    }
}

impl PatchOutcome {
    pub fn summary(&self, paths: &WorkspacePaths) -> String {
        let mut output = String::from("Success. Updated the following files:\n");
        for change in &self.committed {
            output.push_str(&change.summary_line(paths));
        }
        output
    }

    pub fn changed_paths(&self) -> Vec<PathBuf> {
        self.committed
            .iter()
            .filter_map(|change| match change {
                CommittedChange::Add { path, .. } | CommittedChange::Update { path, .. } => {
                    Some(path.clone())
                }
                CommittedChange::Move { target, .. } => Some(target.clone()),
                CommittedChange::Delete { .. } => None,
            })
            .collect()
    }

    pub fn deleted_paths(&self) -> Vec<PathBuf> {
        self.committed
            .iter()
            .filter_map(|change| match change {
                CommittedChange::Delete { path, .. } => Some(path.clone()),
                CommittedChange::Move { source, .. } => Some(source.clone()),
                CommittedChange::Add { .. } | CommittedChange::Update { .. } => None,
            })
            .collect()
    }

    fn failure_suffix(&self, paths: &WorkspacePaths) -> String {
        if self.committed.is_empty() {
            let mut output = "\nNo files were modified before failure.".to_string();
            if !self.exact {
                output.push_str("\nA write may have partially modified a file before failure.");
            }
            return output;
        }

        let mut output = String::from("\nCommitted changes before failure:\n");
        for change in &self.committed {
            output.push_str(&change.failure_line(paths));
        }
        if !self.exact {
            output.push_str("Committed changes may be incomplete because a write failed.\n");
        }
        output
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum CommittedChange {
    Add {
        path: PathBuf,
        content: String,
        overwritten_content: Option<String>,
    },
    Update {
        path: PathBuf,
        old_content: String,
        new_content: String,
    },
    Delete {
        path: PathBuf,
        content: String,
    },
    Move {
        source: PathBuf,
        target: PathBuf,
        old_content: String,
        new_content: String,
        overwritten_target_content: Option<String>,
    },
}

impl CommittedChange {
    fn summary_line(&self, paths: &WorkspacePaths) -> String {
        match self {
            Self::Add { path, .. } => format!("A {}\n", paths.display_relative(path)),
            Self::Update { path, .. } => format!("M {}\n", paths.display_relative(path)),
            Self::Delete { path, .. } => format!("D {}\n", paths.display_relative(path)),
            Self::Move { target, .. } => format!("M {}\n", paths.display_relative(target)),
        }
    }

    fn failure_line(&self, paths: &WorkspacePaths) -> String {
        match self {
            Self::Add {
                path,
                content,
                overwritten_content,
            } => {
                let overwritten = overwritten_content
                    .as_ref()
                    .map(|content| format!(", overwrote {} bytes", content.len()))
                    .unwrap_or_default();
                format!(
                    "A {} ({} bytes{})\n",
                    paths.display_relative(path),
                    content.len(),
                    overwritten
                )
            }
            Self::Update {
                path,
                old_content,
                new_content,
            } => format!(
                "M {} ({} -> {} bytes)\n",
                paths.display_relative(path),
                old_content.len(),
                new_content.len()
            ),
            Self::Delete { path, content } => format!(
                "D {} ({} bytes)\n",
                paths.display_relative(path),
                content.len()
            ),
            Self::Move {
                source,
                target,
                old_content,
                new_content,
                overwritten_target_content,
            } => {
                let overwritten = overwritten_target_content
                    .as_ref()
                    .map(|content| format!(", overwrote {} bytes", content.len()))
                    .unwrap_or_default();
                format!(
                    "M {} -> {} ({} -> {} bytes{})\n",
                    paths.display_relative(source),
                    paths.display_relative(target),
                    old_content.len(),
                    new_content.len(),
                    overwritten
                )
            }
        }
    }
}

pub async fn apply_patch(patch: &str, paths: &WorkspacePaths) -> Result<PatchOutcome, PureError> {
    let hunks = parse_patch(patch)?;
    let mut outcome = PatchOutcome::default();

    for hunk in hunks {
        if let Err(error) = apply_hunk(hunk, paths, &mut outcome).await {
            let error = error_message(error);
            return Err(tool_error(format!(
                "{error}{}",
                outcome.failure_suffix(paths)
            )));
        }
    }

    Ok(outcome)
}

async fn apply_hunk(
    hunk: Hunk,
    paths: &WorkspacePaths,
    outcome: &mut PatchOutcome,
) -> Result<(), PureError> {
    match hunk {
        Hunk::Add { path, content } => {
            let target = paths.resolve_for_write(&path).await?;
            paths.reject_symlink_write(&target).await?;
            let overwritten_content = read_optional_text(&target).await?;
            if let Some(parent) = target.parent() {
                tokio::fs::create_dir_all(parent).await?;
            }
            write_text(&target, &content, outcome).await?;
            outcome.committed.push(CommittedChange::Add {
                path: target,
                content,
                overwritten_content,
            });
        }
        Hunk::Delete { path } => {
            let target = paths.resolve_existing(&path).await?;
            paths.reject_symlink_write(&target).await?;
            let metadata = tokio::fs::metadata(&target).await?;
            if !metadata.is_file() {
                return Err(tool_error(format!(
                    "cannot delete '{}': path is not a file",
                    target.display()
                )));
            }
            let content = tokio::fs::read_to_string(&target).await.map_err(|error| {
                tool_error(format!("failed to read '{}': {error}", target.display()))
            })?;
            tokio::fs::remove_file(&target).await.map_err(|error| {
                tool_error(format!("failed to delete '{}': {error}", target.display()))
            })?;
            outcome.committed.push(CommittedChange::Delete {
                path: target,
                content,
            });
        }
        Hunk::Update {
            path,
            move_path,
            chunks,
        } => {
            let source = paths.resolve_existing(&path).await?;
            paths.reject_symlink_write(&source).await?;
            let old_content = tokio::fs::read_to_string(&source).await.map_err(|error| {
                tool_error(format!("failed to read '{}': {error}", source.display()))
            })?;
            let new_content = if chunks.is_empty() {
                old_content.clone()
            } else {
                apply_chunks(&old_content, &source, &chunks)?
            };

            if let Some(move_path) = move_path {
                let target = paths.resolve_for_write(&move_path).await?;
                paths.reject_symlink_write(&target).await?;
                if target == source {
                    write_text(&source, &new_content, outcome).await?;
                    outcome.committed.push(CommittedChange::Update {
                        path: source,
                        old_content,
                        new_content,
                    });
                    return Ok(());
                }
                let overwritten_target_content = read_optional_text(&target).await?;
                if let Some(parent) = target.parent() {
                    tokio::fs::create_dir_all(parent).await?;
                }
                write_text(&target, &new_content, outcome).await?;
                let target_commit_index = outcome.committed.len();
                outcome.committed.push(CommittedChange::Add {
                    path: target.clone(),
                    content: new_content.clone(),
                    overwritten_content: overwritten_target_content.clone(),
                });
                tokio::fs::remove_file(&source).await.map_err(|error| {
                    tool_error(format!(
                        "failed to remove original '{}': {error}",
                        source.display()
                    ))
                })?;
                outcome.committed[target_commit_index] = CommittedChange::Move {
                    source,
                    target,
                    old_content,
                    new_content,
                    overwritten_target_content,
                };
            } else {
                write_text(&source, &new_content, outcome).await?;
                outcome.committed.push(CommittedChange::Update {
                    path: source,
                    old_content,
                    new_content,
                });
            }
        }
    }
    Ok(())
}

async fn write_text(
    path: &Path,
    content: &str,
    outcome: &mut PatchOutcome,
) -> Result<(), PureError> {
    tokio::fs::write(path, content).await.map_err(|error| {
        outcome.exact = false;
        tool_error(format!("failed to write '{}': {error}", path.display()))
    })
}

async fn read_optional_text(path: &Path) -> Result<Option<String>, PureError> {
    match tokio::fs::read_to_string(path).await {
        Ok(content) => Ok(Some(content)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(tool_error(format!(
            "failed to read '{}': {error}",
            path.display()
        ))),
    }
}

fn error_message(error: PureError) -> String {
    match error {
        PureError::ToolExecutionFailed { error, .. } => error,
        error => error.to_string(),
    }
}

fn tool_error(error: impl std::fmt::Display) -> PureError {
    PureError::ToolExecutionFailed {
        tool: "apply_patch".to_string(),
        error: error.to_string(),
    }
}

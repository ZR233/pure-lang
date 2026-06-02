use std::borrow::Cow;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

use pl_protocol::PureError;

use super::path::WorkspacePaths;

const BEGIN_PATCH: &str = "*** Begin Patch";
const END_PATCH: &str = "*** End Patch";
const ADD_FILE: &str = "*** Add File: ";
const UPDATE_FILE: &str = "*** Update File: ";
const DELETE_FILE: &str = "*** Delete File: ";
const MOVE_TO: &str = "*** Move to: ";
const EOF_MARKER: &str = "*** End of File";
const VALID_HUNK_HEADERS: &str = "valid hunk headers are '*** Add File: {path}', '*** Delete File: {path}', '*** Update File: {path}'";

pub const APPLY_PATCH_LARK_GRAMMAR: &str = r#"start: begin_patch hunk+ end_patch
begin_patch: "*** Begin Patch" LF
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
change_line: ("+" | "-" | " ") /(.*)/ LF
eof_line: "*** End of File" LF

%import common.LF
"#;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PatchPlan {
    changes: Vec<PlannedChange>,
}

impl PatchPlan {
    pub fn summary(&self, paths: &WorkspacePaths) -> String {
        let mut output = String::from("Success. Updated the following files:\n");
        for change in &self.changes {
            match change {
                PlannedChange::Add { path, .. } => {
                    output.push_str(&format!("A {}\n", paths.display_relative(path)));
                }
                PlannedChange::Update { path, .. } => {
                    output.push_str(&format!("M {}\n", paths.display_relative(path)));
                }
                PlannedChange::Delete { path, .. } => {
                    output.push_str(&format!("D {}\n", paths.display_relative(path)));
                }
            }
        }
        output
    }

    pub async fn apply(&self) -> Result<(), PureError> {
        for change in &self.changes {
            match change {
                PlannedChange::Add { path, content } | PlannedChange::Update { path, content } => {
                    if let Some(parent) = path.parent() {
                        tokio::fs::create_dir_all(parent).await?;
                    }
                    tokio::fs::write(path, content).await?;
                }
                PlannedChange::Delete { path } => {
                    tokio::fs::remove_file(path).await?;
                }
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum PlannedChange {
    Add { path: PathBuf, content: String },
    Update { path: PathBuf, content: String },
    Delete { path: PathBuf },
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Hunk {
    Add {
        path: String,
        content: String,
    },
    Delete {
        path: String,
    },
    Update {
        path: String,
        move_path: Option<String>,
        chunks: Vec<UpdateChunk>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct UpdateChunk {
    context: Option<String>,
    old_lines: Vec<String>,
    new_lines: Vec<String>,
    eof: bool,
}

pub async fn plan_patch(patch: &str, paths: &WorkspacePaths) -> Result<PatchPlan, PureError> {
    let hunks = parse_patch(patch)?;
    let mut changes = Vec::new();
    let mut touched_paths = HashSet::new();

    for hunk in hunks {
        match hunk {
            Hunk::Add { path, content } => {
                let target = paths.resolve_for_write(&path).await?;
                paths.reject_symlink_write(&target).await?;
                reserve_patch_path(&mut touched_paths, &target)?;
                if tokio::fs::try_exists(&target).await? {
                    return Err(tool_error(format!(
                        "cannot add '{}': target already exists",
                        target.display()
                    )));
                }
                changes.push(PlannedChange::Add {
                    path: target,
                    content,
                });
            }
            Hunk::Delete { path } => {
                let target = paths.resolve_existing(&path).await?;
                paths.reject_symlink_write(&target).await?;
                reserve_patch_path(&mut touched_paths, &target)?;
                let metadata = tokio::fs::metadata(&target).await?;
                if !metadata.is_file() {
                    return Err(tool_error(format!(
                        "cannot delete '{}': path is not a file",
                        target.display()
                    )));
                }
                changes.push(PlannedChange::Delete { path: target });
            }
            Hunk::Update {
                path,
                move_path,
                chunks,
            } => {
                let source = paths.resolve_existing(&path).await?;
                paths.reject_symlink_write(&source).await?;
                reserve_patch_path(&mut touched_paths, &source)?;
                let old_content = tokio::fs::read_to_string(&source).await.map_err(|error| {
                    tool_error(format!("failed to read '{}': {error}", source.display()))
                })?;
                let new_content = if chunks.is_empty() {
                    old_content.clone()
                } else {
                    apply_chunks(&old_content, &source, &chunks)?
                };
                let mut delete_after_update = None;
                let target = match move_path {
                    Some(move_path) => {
                        let target = paths.resolve_for_write(&move_path).await?;
                        paths.reject_symlink_write(&target).await?;
                        reserve_patch_path(&mut touched_paths, &target)?;
                        if tokio::fs::try_exists(&target).await? {
                            return Err(tool_error(format!(
                                "cannot move to '{}': target already exists",
                                target.display()
                            )));
                        }
                        delete_after_update = Some(source.clone());
                        target
                    }
                    None => source,
                };
                changes.push(PlannedChange::Update {
                    path: target,
                    content: new_content,
                });
                if let Some(path) = delete_after_update {
                    changes.push(PlannedChange::Delete { path });
                }
            }
        }
    }

    Ok(PatchPlan { changes })
}

fn parse_patch(patch: &str) -> Result<Vec<Hunk>, PureError> {
    let patch = normalize_patch_input(patch)?;
    let lines: Vec<&str> = patch.trim().lines().collect();
    match (
        lines.first().map(|line| line.trim()),
        lines.last().map(|line| line.trim()),
    ) {
        (Some(BEGIN_PATCH), Some(END_PATCH)) => {}
        (Some(first), _) if first != BEGIN_PATCH => {
            return Err(tool_error("first line must be '*** Begin Patch'"));
        }
        (_, Some(last)) if last != END_PATCH => {
            return Err(tool_error("last line must be '*** End Patch'"));
        }
        _ => {
            return Err(tool_error(
                "patch is empty; first line must be '*** Begin Patch'",
            ));
        }
    }

    let mut hunks = Vec::new();
    let mut index = 1;
    while index + 1 < lines.len() {
        let line = lines[index].trim();
        if let Some(path) = line.strip_prefix(ADD_FILE) {
            let mut content = String::new();
            index += 1;
            while index + 1 < lines.len() {
                let line = lines[index];
                let Some(added) = line.strip_prefix('+') else {
                    break;
                };
                content.push_str(added);
                content.push('\n');
                index += 1;
            }
            if content.is_empty() {
                return Err(tool_error(format!("add hunk for '{path}' is empty")));
            }
            hunks.push(Hunk::Add {
                path: path.to_string(),
                content,
            });
        } else if let Some(path) = line.strip_prefix(DELETE_FILE) {
            hunks.push(Hunk::Delete {
                path: path.to_string(),
            });
            index += 1;
        } else if let Some(path) = line.strip_prefix(UPDATE_FILE) {
            index += 1;
            let move_path = lines
                .get(index)
                .and_then(|line| line.trim().strip_prefix(MOVE_TO))
                .map(ToOwned::to_owned);
            if move_path.is_some() {
                index += 1;
            }
            let mut chunks = Vec::new();
            while index + 1 < lines.len() {
                let line = lines[index];
                if line.trim().starts_with("*** ") {
                    break;
                }
                let (chunk, consumed) = parse_update_chunk(&lines[index..lines.len() - 1])?;
                chunks.push(chunk);
                index += consumed;
            }
            if chunks.is_empty() && move_path.is_none() {
                return Err(tool_error(format!("update hunk for '{path}' is empty")));
            }
            hunks.push(Hunk::Update {
                path: path.to_string(),
                move_path,
                chunks,
            });
        } else if line.is_empty() {
            index += 1;
        } else {
            return Err(invalid_hunk_header(line));
        }
    }

    if hunks.is_empty() {
        return Err(tool_error("patch does not contain any hunks"));
    }
    Ok(hunks)
}

fn normalize_patch_input(patch: &str) -> Result<Cow<'_, str>, PureError> {
    let trimmed = patch.trim();
    let lines: Vec<&str> = trimmed.lines().collect();
    let begin_indices: Vec<usize> = lines
        .iter()
        .enumerate()
        .filter_map(|(index, line)| (line.trim() == BEGIN_PATCH).then_some(index))
        .collect();
    let end_indices: Vec<usize> = lines
        .iter()
        .enumerate()
        .filter_map(|(index, line)| (line.trim() == END_PATCH).then_some(index))
        .collect();

    if begin_indices.len() > 1 || end_indices.len() > 1 {
        return Err(tool_error(
            "patch input contains multiple patch blocks; send exactly one *** Begin Patch block",
        ));
    }
    let Some(begin_index) = begin_indices.first().copied() else {
        return Ok(Cow::Borrowed(trimmed));
    };
    let Some(end_index) = end_indices.first().copied() else {
        return Err(tool_error("last line must be '*** End Patch'"));
    };
    if end_index < begin_index {
        return Err(tool_error("last line must be '*** End Patch'"));
    }
    if begin_index == 0 && end_index + 1 == lines.len() {
        return Ok(Cow::Borrowed(trimmed));
    }
    Ok(Cow::Owned(lines[begin_index..=end_index].join("\n")))
}

fn invalid_hunk_header(line: &str) -> PureError {
    let lower = line.to_ascii_lowercase();
    let guidance = if line.starts_with("--- ") || line.starts_with("+++ ") {
        "standard unified diff headers are not supported; use '*** Update File: <path>' with @@ chunks instead"
    } else if line.starts_with("*** File:") {
        "'*** File:' metadata headers are not supported; use one of the file operation headers"
    } else if lower.starts_with("insert ")
        || lower.starts_with("replace ")
        || lower.starts_with("delete ")
    {
        "natural-language edit instructions are not supported; express the edit as an Add/Delete/Update file hunk"
    } else {
        "unsupported patch hunk header"
    };
    tool_error(format!(
        "invalid hunk header: '{line}'. {guidance}; {VALID_HUNK_HEADERS}"
    ))
}

fn parse_update_chunk(lines: &[&str]) -> Result<(UpdateChunk, usize), PureError> {
    let mut index = 0;
    let context = match lines.first().copied() {
        Some("@@") => {
            index = 1;
            None
        }
        Some(line) if line.starts_with("@@ -") => {
            return Err(tool_error(
                "unified diff hunk ranges are not supported; use '@@' or '@@ <search context>'",
            ));
        }
        Some(line) if line.starts_with("@@ ") => {
            index = 1;
            Some(line.trim_start_matches("@@ ").to_string())
        }
        Some(_) => None,
        None => return Err(tool_error("update chunk is empty")),
    };

    let mut old_lines = Vec::new();
    let mut new_lines = Vec::new();
    let mut eof = false;
    while index < lines.len() {
        let line = lines[index];
        if line == EOF_MARKER {
            eof = true;
            index += 1;
            break;
        }
        if line.trim().starts_with("*** ") || line.starts_with("@@") {
            break;
        }
        match line.chars().next() {
            Some(' ') => {
                old_lines.push(line[1..].to_string());
                new_lines.push(line[1..].to_string());
            }
            Some('+') => new_lines.push(line[1..].to_string()),
            Some('-') => old_lines.push(line[1..].to_string()),
            None => {
                old_lines.push(String::new());
                new_lines.push(String::new());
            }
            Some(_) => {
                return Err(tool_error(format!(
                    "invalid update line: '{line}', expected ' ', '+' or '-'"
                )));
            }
        }
        index += 1;
    }

    if old_lines.is_empty() && new_lines.is_empty() {
        return Err(tool_error("update chunk does not contain changes"));
    }

    Ok((
        UpdateChunk {
            context,
            old_lines,
            new_lines,
            eof,
        },
        index,
    ))
}

fn apply_chunks(content: &str, path: &Path, chunks: &[UpdateChunk]) -> Result<String, PureError> {
    let mut lines: Vec<String> = content.split('\n').map(String::from).collect();
    if lines.last().is_some_and(String::is_empty) {
        lines.pop();
    }
    let mut cursor = 0;
    let mut replacements = Vec::new();

    for chunk in chunks {
        if let Some(context) = &chunk.context {
            let Some(context_index) =
                find_sequence(&lines, std::slice::from_ref(context), cursor, false)
            else {
                return Err(tool_error(format!(
                    "failed to find context '{context}' in {}",
                    path.display()
                )));
            };
            cursor = context_index + 1;
        }

        if chunk.old_lines.is_empty() {
            let insert_at = cursor.min(lines.len());
            replacements.push((insert_at, 0, chunk.new_lines.clone()));
            cursor = insert_at;
            continue;
        }

        let Some(start) = find_sequence(&lines, &chunk.old_lines, cursor, chunk.eof) else {
            return Err(tool_error(format!(
                "failed to find expected lines in {}:\n{}",
                path.display(),
                chunk.old_lines.join("\n")
            )));
        };
        replacements.push((start, chunk.old_lines.len(), chunk.new_lines.clone()));
        cursor = start + chunk.old_lines.len();
    }

    replacements.sort_by_key(|(start, _, _)| *start);
    for (start, old_len, new_lines) in replacements.into_iter().rev() {
        lines.splice(start..start + old_len, new_lines);
    }
    if !lines.last().is_some_and(String::is_empty) {
        lines.push(String::new());
    }
    Ok(lines.join("\n"))
}

fn find_sequence(lines: &[String], pattern: &[String], start: usize, eof: bool) -> Option<usize> {
    if pattern.is_empty() {
        return Some(start.min(lines.len()));
    }
    if pattern.len() > lines.len() {
        return None;
    }
    for index in start..=lines.len().saturating_sub(pattern.len()) {
        if eof && index + pattern.len() != lines.len() {
            continue;
        }
        if lines[index..index + pattern.len()] == *pattern {
            return Some(index);
        }
    }
    None
}

fn reserve_patch_path(seen: &mut HashSet<PathBuf>, path: &Path) -> Result<(), PureError> {
    if seen.insert(path.to_path_buf()) {
        return Ok(());
    }
    Err(tool_error(format!(
        "patch touches '{}' more than once; combine edits for one file into a single hunk with multiple @@ chunks",
        path.display()
    )))
}

fn tool_error(error: impl std::fmt::Display) -> PureError {
    PureError::ToolExecutionFailed {
        tool: "apply_patch".to_string(),
        error: error.to_string(),
    }
}

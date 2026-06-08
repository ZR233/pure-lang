use std::borrow::Cow;
use std::path::{Path, PathBuf};

use pl_protocol::PureError;

use super::path::WorkspacePaths;

const BEGIN_PATCH: &str = "*** Begin Patch";
const END_PATCH: &str = "*** End Patch";
const ADD_FILE: &str = "*** Add File: ";
const UPDATE_FILE: &str = "*** Update File: ";
const DELETE_FILE: &str = "*** Delete File: ";
const MOVE_TO: &str = "*** Move to: ";
const ENVIRONMENT_ID: &str = "*** Environment ID: ";
const EOF_MARKER: &str = "*** End of File";
const VALID_HUNK_HEADERS: &str = "valid hunk headers are '*** Add File: {path}', '*** Delete File: {path}', '*** Update File: {path}'";
const PATCH_RETRY_GUIDANCE: &str = "Recovery: read the target file again, then retry with a smaller Codex-style patch built from the current file contents. Do not repeat the same failed patch.";

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

fn parse_patch(patch: &str) -> Result<Vec<Hunk>, PureError> {
    let patch = normalize_patch_input(patch)?;
    let lines: Vec<&str> = patch.trim().lines().collect();
    match (
        lines.first().map(|line| line.trim()),
        lines.last().map(|line| line.trim()),
    ) {
        (Some(BEGIN_PATCH), Some(END_PATCH)) => {}
        (Some(first), _) if first != BEGIN_PATCH => {
            return Err(tool_error(format!(
                "first line must be '*** Begin Patch'. {PATCH_RETRY_GUIDANCE}"
            )));
        }
        (_, Some(last)) if last != END_PATCH => {
            return Err(tool_error(format!(
                "last line must be '*** End Patch'; send the complete patch including the closing marker. {PATCH_RETRY_GUIDANCE}"
            )));
        }
        _ => {
            return Err(tool_error(format!(
                "patch is empty; first line must be '*** Begin Patch'. {PATCH_RETRY_GUIDANCE}"
            )));
        }
    }

    let mut hunks = Vec::new();
    let mut index = 1;
    if let Some(line) = lines.get(index)
        && let Some(environment_id) = line.trim_start().strip_prefix(ENVIRONMENT_ID)
    {
        if environment_id.trim().is_empty() {
            return Err(tool_error("apply_patch environment_id cannot be empty"));
        }
        index += 1;
    }
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
                if line.trim().is_empty() {
                    index += 1;
                    continue;
                }
                if line.trim().starts_with("*** ") {
                    break;
                }
                let (chunk, consumed) =
                    parse_update_chunk(&lines[index..lines.len() - 1], index + 1)?;
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
            return Err(invalid_hunk_header(line, index + 1));
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
    if let Some(inner) = strip_heredoc_wrapper(&lines)? {
        return Ok(Cow::Owned(inner.join("\n")));
    }
    if lines
        .first()
        .is_some_and(|line| line.trim_start().starts_with("<<"))
    {
        return Ok(Cow::Borrowed(trimmed));
    }
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
        return Err(tool_error(format!(
            "last line must be '*** End Patch'; send the complete patch including the closing marker. {PATCH_RETRY_GUIDANCE}"
        )));
    };
    if end_index < begin_index {
        return Err(tool_error(format!(
            "last line must be '*** End Patch'. {PATCH_RETRY_GUIDANCE}"
        )));
    }
    if begin_index == 0 && end_index + 1 == lines.len() {
        return Ok(Cow::Borrowed(trimmed));
    }
    Ok(Cow::Owned(lines[begin_index..=end_index].join("\n")))
}

fn strip_heredoc_wrapper<'a>(lines: &'a [&'a str]) -> Result<Option<Vec<&'a str>>, PureError> {
    let [first, .., last] = lines else {
        return Ok(None);
    };
    let first = first.trim();
    if !matches!(first, "<<EOF" | "<<'EOF'" | "<<\"EOF\"") {
        return Ok(None);
    }
    if last.trim_end() != "EOF" {
        return Err(tool_error(
            "missing closing EOF marker for apply_patch heredoc",
        ));
    }
    if lines.len() < 4 {
        return Err(tool_error(
            "apply_patch heredoc does not contain a patch block",
        ));
    }
    Ok(Some(lines[1..lines.len() - 1].to_vec()))
}

fn invalid_hunk_header(line: &str, line_number: usize) -> PureError {
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
        "invalid hunk header at line {line_number}: '{line}'. {guidance}; {VALID_HUNK_HEADERS}. {PATCH_RETRY_GUIDANCE}"
    ))
}

fn parse_update_chunk(
    lines: &[&str],
    line_number: usize,
) -> Result<(UpdateChunk, usize), PureError> {
    let mut index = 0;
    let context = match lines.first().copied() {
        Some("@@") => {
            index = 1;
            None
        }
        Some(line) if line.starts_with("@@ -") => {
            return Err(tool_error(format!(
                "invalid update hunk at line {line_number}: unified diff hunk ranges are not supported; use '@@' or '@@ <search context>'"
            )));
        }
        Some(line) if line.starts_with("@@ ") => {
            index = 1;
            Some(line.trim_start_matches("@@ ").to_string())
        }
        Some(_) => None,
        None => {
            return Err(tool_error(format!(
                "update chunk at line {line_number} is empty"
            )));
        }
    };

    let mut old_lines = Vec::new();
    let mut new_lines = Vec::new();
    let mut eof = false;
    while index < lines.len() {
        let line = lines[index];
        if line.trim() == EOF_MARKER {
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
                old_lines.push(line.to_string());
                new_lines.push(line.to_string());
            }
        }
        index += 1;
    }

    if old_lines.is_empty() && new_lines.is_empty() {
        return Err(tool_error(format!(
            "update chunk at line {line_number} does not contain changes"
        )));
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
                    "failed to find context '{context}' in {}. {PATCH_RETRY_GUIDANCE}",
                    path.display(),
                )));
            };
            cursor = context_index + 1;
        }

        if chunk.old_lines.is_empty() {
            let insert_at = lines.len();
            replacements.push((insert_at, 0, chunk.new_lines.clone()));
            cursor = insert_at;
            continue;
        }

        let Some((start, old_len, new_lines)) = find_chunk_replacement(&lines, chunk, cursor)
        else {
            return Err(tool_error(format!(
                "failed to find expected lines in {}:\n{}\n{PATCH_RETRY_GUIDANCE}",
                path.display(),
                chunk.old_lines.join("\n")
            )));
        };
        replacements.push((start, old_len, new_lines));
        cursor = start + old_len;
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

fn find_chunk_replacement(
    lines: &[String],
    chunk: &UpdateChunk,
    cursor: usize,
) -> Option<(usize, usize, Vec<String>)> {
    let mut candidates = vec![(chunk.old_lines.clone(), chunk.new_lines.clone())];
    if let Some(candidate) = duplicated_edge_context_candidate(chunk) {
        candidates.push(candidate);
    }

    for (old_lines, new_lines) in candidates {
        if let Some(start) = find_sequence(lines, &old_lines, cursor, chunk.eof) {
            let matched_lines = lines[start..start + old_lines.len()].to_vec();
            let new_lines = preserve_matched_context_lines(&old_lines, &new_lines, &matched_lines);
            return Some((start, old_lines.len(), new_lines));
        }
        if old_lines.last().is_some_and(String::is_empty) {
            let old_lines = old_lines[..old_lines.len() - 1].to_vec();
            let new_lines = if new_lines.last().is_some_and(String::is_empty) {
                new_lines[..new_lines.len() - 1].to_vec()
            } else {
                new_lines
            };
            if let Some(start) = find_sequence(lines, &old_lines, cursor, chunk.eof) {
                let matched_lines = lines[start..start + old_lines.len()].to_vec();
                let new_lines =
                    preserve_matched_context_lines(&old_lines, &new_lines, &matched_lines);
                return Some((start, old_lines.len(), new_lines));
            }
        }
    }
    None
}

fn preserve_matched_context_lines(
    old_lines: &[String],
    new_lines: &[String],
    matched_lines: &[String],
) -> Vec<String> {
    let mut old_index = 0;
    new_lines
        .iter()
        .map(|line| {
            if old_index < old_lines.len() && lines_equivalent(line, &old_lines[old_index]) {
                let matched = matched_lines[old_index].clone();
                old_index += 1;
                matched
            } else {
                line.clone()
            }
        })
        .collect()
}

fn duplicated_edge_context_candidate(chunk: &UpdateChunk) -> Option<(Vec<String>, Vec<String>)> {
    if chunk.old_lines.len() != 2 || chunk.new_lines.len() <= chunk.old_lines.len() {
        return None;
    }
    let first_old = chunk.old_lines.first()?;
    let last_old = chunk.old_lines.last()?;
    if !lines_equivalent(first_old, last_old) {
        return None;
    }
    let first_new = chunk.new_lines.first()?;
    let last_new = chunk.new_lines.last()?;
    if !lines_equivalent(first_new, first_old) || !lines_equivalent(last_new, last_old) {
        return None;
    }
    Some((vec![last_old.clone()], chunk.new_lines[1..].to_vec()))
}

fn find_sequence(lines: &[String], pattern: &[String], start: usize, eof: bool) -> Option<usize> {
    if pattern.is_empty() {
        return Some(start.min(lines.len()));
    }
    if pattern.len() > lines.len() {
        return None;
    }
    let last_start = lines.len().saturating_sub(pattern.len());
    if eof {
        let end_indices = [last_start];
        if let Some(index) = find_sequence_in_indices(lines, pattern, end_indices) {
            return Some(index);
        }
    }
    if start > last_start {
        return None;
    }
    find_sequence_in_indices(lines, pattern, start..=last_start)
}

fn find_sequence_in_indices(
    lines: &[String],
    pattern: &[String],
    indices: impl IntoIterator<Item = usize> + Clone,
) -> Option<usize> {
    for index in indices.clone() {
        if lines[index..index + pattern.len()] == *pattern {
            return Some(index);
        }
    }
    for index in indices.clone() {
        if pattern
            .iter()
            .enumerate()
            .all(|(offset, expected)| lines[index + offset].trim_end() == expected.trim_end())
        {
            return Some(index);
        }
    }
    for index in indices.clone() {
        if pattern
            .iter()
            .enumerate()
            .all(|(offset, expected)| lines[index + offset].trim() == expected.trim())
        {
            return Some(index);
        }
    }
    indices.into_iter().find(|&index| {
        pattern.iter().enumerate().all(|(offset, expected)| {
            normalize_line_for_match(&lines[index + offset]) == normalize_line_for_match(expected)
        })
    })
}

fn lines_equivalent(left: &str, right: &str) -> bool {
    left.trim() == right.trim() || normalize_line_for_match(left) == normalize_line_for_match(right)
}

fn normalize_line_for_match(line: &str) -> String {
    line.trim()
        .chars()
        .map(|ch| match ch {
            '\u{2010}' | '\u{2011}' | '\u{2012}' | '\u{2013}' | '\u{2014}' | '\u{2015}'
            | '\u{2212}' => '-',
            '\u{2018}' | '\u{2019}' | '\u{201A}' | '\u{201B}' => '\'',
            '\u{201C}' | '\u{201D}' | '\u{201E}' | '\u{201F}' => '"',
            '\u{00A0}' | '\u{2002}' | '\u{2003}' | '\u{2004}' | '\u{2005}' | '\u{2006}'
            | '\u{2007}' | '\u{2008}' | '\u{2009}' | '\u{200A}' | '\u{202F}' | '\u{205F}'
            | '\u{3000}' => ' ',
            other => other,
        })
        .collect()
}

fn tool_error(error: impl std::fmt::Display) -> PureError {
    PureError::ToolExecutionFailed {
        tool: "apply_patch".to_string(),
        error: error.to_string(),
    }
}

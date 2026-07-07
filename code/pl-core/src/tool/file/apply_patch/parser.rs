use std::borrow::Cow;

use pl_protocol::PureError;

use super::tool_error;

const BEGIN_PATCH: &str = "*** Begin Patch";
const END_PATCH: &str = "*** End Patch";
const ADD_FILE: &str = "*** Add File: ";
const UPDATE_FILE: &str = "*** Update File: ";
const DELETE_FILE: &str = "*** Delete File: ";
const MOVE_TO: &str = "*** Move to: ";
const ENVIRONMENT_ID: &str = "*** Environment ID: ";
const EOF_MARKER: &str = "*** End of File";
const VALID_HUNK_HEADERS: &str = "valid hunk headers are '*** Add File: {path}', '*** Delete File: {path}', '*** Update File: {path}'";
pub(crate) const PATCH_RETRY_GUIDANCE: &str = "Recovery: read the target file again, then retry with a smaller Codex-style patch built from the current file contents. Do not repeat the same failed patch.";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Hunk {
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
pub(crate) struct UpdateChunk {
    pub(crate) context: Option<String>,
    pub(crate) old_lines: Vec<String>,
    pub(crate) new_lines: Vec<String>,
    pub(crate) eof: bool,
}

pub(crate) fn parse_patch(patch: &str) -> Result<Vec<Hunk>, PureError> {
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

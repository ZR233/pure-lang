use std::borrow::Cow;
use std::future::Future;
use std::path::{Path, PathBuf};

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

pub type PatchResult<T> = Result<T, PatchError>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PatchError {
    message: String,
}

impl PatchError {
    pub fn new(message: impl std::fmt::Display) -> Self {
        Self {
            message: message.to_string(),
        }
    }

    pub fn message(&self) -> &str {
        &self.message
    }

    pub fn into_message(self) -> String {
        self.message
    }
}

impl std::fmt::Display for PatchError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.message.fmt(formatter)
    }
}

impl std::error::Error for PatchError {}

/// 为 patch 结果提供面向用户的路径展示。
///
/// 调用方可用它把真实路径、容器路径或 workspace 相对路径转换为产品需要的显示格式。
pub trait PatchPathDisplay {
    fn display_path(&self, path: &Path) -> String;
}

/// apply_patch 的文件系统后端。
///
/// 该 trait 只负责路径解析、读写和删除，patch 语法、上下文匹配和失败摘要由
/// `pl-patch` 统一处理。实现方应在解析阶段完成产品自己的安全策略，例如 workspace
/// 边界、符号链接拒绝、Docker 容器路径映射等。
pub trait PatchBackend: PatchPathDisplay {
    fn resolve_existing<'a>(
        &'a self,
        path: &'a str,
    ) -> impl Future<Output = PatchResult<PathBuf>> + Send + 'a;

    fn resolve_for_write<'a>(
        &'a self,
        path: &'a str,
    ) -> impl Future<Output = PatchResult<PathBuf>> + Send + 'a;

    fn reject_symlink_write<'a>(
        &'a self,
        path: &'a Path,
    ) -> impl Future<Output = PatchResult<()>> + Send + 'a;

    fn ensure_file<'a>(
        &'a self,
        path: &'a Path,
    ) -> impl Future<Output = PatchResult<()>> + Send + 'a;

    fn read_to_string<'a>(
        &'a self,
        path: &'a Path,
    ) -> impl Future<Output = PatchResult<String>> + Send + 'a;

    fn read_optional_text<'a>(
        &'a self,
        path: &'a Path,
    ) -> impl Future<Output = PatchResult<Option<String>>> + Send + 'a;

    fn create_parent_dirs<'a>(
        &'a self,
        path: &'a Path,
    ) -> impl Future<Output = PatchResult<()>> + Send + 'a;

    fn write_text<'a>(
        &'a self,
        path: &'a Path,
        content: &'a str,
    ) -> impl Future<Output = PatchResult<()>> + Send + 'a;

    fn remove_file<'a>(
        &'a self,
        path: &'a Path,
    ) -> impl Future<Output = PatchResult<()>> + Send + 'a;
}

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
    pub fn file_changes(&self) -> Vec<PatchFileChange> {
        self.committed
            .iter()
            .map(|change| match change {
                CommittedChange::Add { path, .. } => PatchFileChange::Add { path: path.clone() },
                CommittedChange::Update { path, .. } => {
                    PatchFileChange::Update { path: path.clone() }
                }
                CommittedChange::Delete { path, .. } => {
                    PatchFileChange::Delete { path: path.clone() }
                }
                CommittedChange::Move { source, target, .. } => PatchFileChange::Move {
                    source: source.clone(),
                    target: target.clone(),
                },
            })
            .collect()
    }

    pub fn summary(&self, paths: &impl PatchPathDisplay) -> String {
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

    fn failure_suffix(&self, paths: &impl PatchPathDisplay) -> String {
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
pub enum PatchFileChange {
    Add { path: PathBuf },
    Update { path: PathBuf },
    Delete { path: PathBuf },
    Move { source: PathBuf, target: PathBuf },
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
    fn summary_line(&self, paths: &impl PatchPathDisplay) -> String {
        match self {
            Self::Add { path, .. } => format!("A {}\n", paths.display_path(path)),
            Self::Update { path, .. } => format!("M {}\n", paths.display_path(path)),
            Self::Delete { path, .. } => format!("D {}\n", paths.display_path(path)),
            Self::Move { target, .. } => format!("M {}\n", paths.display_path(target)),
        }
    }

    fn failure_line(&self, paths: &impl PatchPathDisplay) -> String {
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
                    paths.display_path(path),
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
                paths.display_path(path),
                old_content.len(),
                new_content.len()
            ),
            Self::Delete { path, content } => {
                format!("D {} ({} bytes)\n", paths.display_path(path), content.len())
            }
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
                    paths.display_path(source),
                    paths.display_path(target),
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

pub async fn apply_patch(
    patch: &str,
    backend: &(impl PatchBackend + Sync),
) -> PatchResult<PatchOutcome> {
    let hunks = parse_patch(patch)?;
    let mut outcome = PatchOutcome::default();

    for hunk in hunks {
        if let Err(error) = apply_hunk(hunk, backend, &mut outcome).await {
            return Err(PatchError::new(format!(
                "{error}{}",
                outcome.failure_suffix(backend)
            )));
        }
    }

    Ok(outcome)
}

async fn apply_hunk(
    hunk: Hunk,
    backend: &(impl PatchBackend + Sync),
    outcome: &mut PatchOutcome,
) -> PatchResult<()> {
    match hunk {
        Hunk::Add { path, content } => {
            let target = backend.resolve_for_write(&path).await?;
            backend.reject_symlink_write(&target).await?;
            let overwritten_content = backend.read_optional_text(&target).await?;
            backend.create_parent_dirs(&target).await?;
            write_text(backend, &target, &content, outcome).await?;
            outcome.committed.push(CommittedChange::Add {
                path: target,
                content,
                overwritten_content,
            });
        }
        Hunk::Delete { path } => {
            let target = backend.resolve_existing(&path).await?;
            backend.reject_symlink_write(&target).await?;
            backend.ensure_file(&target).await?;
            let content = backend.read_to_string(&target).await?;
            backend.remove_file(&target).await?;
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
            let source = backend.resolve_existing(&path).await?;
            backend.reject_symlink_write(&source).await?;
            let old_content = backend.read_to_string(&source).await?;
            let new_content = if chunks.is_empty() {
                old_content.clone()
            } else {
                apply_chunks(&old_content, &source, &chunks)?
            };

            if let Some(move_path) = move_path {
                let target = backend.resolve_for_write(&move_path).await?;
                backend.reject_symlink_write(&target).await?;
                if target == source {
                    write_text(backend, &source, &new_content, outcome).await?;
                    outcome.committed.push(CommittedChange::Update {
                        path: source,
                        old_content,
                        new_content,
                    });
                    return Ok(());
                }
                let overwritten_target_content = backend.read_optional_text(&target).await?;
                backend.create_parent_dirs(&target).await?;
                write_text(backend, &target, &new_content, outcome).await?;
                let target_commit_index = outcome.committed.len();
                outcome.committed.push(CommittedChange::Add {
                    path: target.clone(),
                    content: new_content.clone(),
                    overwritten_content: overwritten_target_content.clone(),
                });
                backend.remove_file(&source).await?;
                outcome.committed[target_commit_index] = CommittedChange::Move {
                    source,
                    target,
                    old_content,
                    new_content,
                    overwritten_target_content,
                };
            } else {
                write_text(backend, &source, &new_content, outcome).await?;
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
    backend: &(impl PatchBackend + Sync),
    path: &Path,
    content: &str,
    outcome: &mut PatchOutcome,
) -> PatchResult<()> {
    backend.write_text(path, content).await.inspect_err(|_| {
        outcome.exact = false;
    })
}

fn parse_patch(patch: &str) -> PatchResult<Vec<Hunk>> {
    let patch = normalize_patch_input(patch)?;
    let lines: Vec<&str> = patch.trim().lines().collect();
    match (
        lines.first().map(|line| line.trim()),
        lines.last().map(|line| line.trim()),
    ) {
        (Some(BEGIN_PATCH), Some(END_PATCH)) => {}
        (Some(first), _) if first != BEGIN_PATCH => {
            return Err(PatchError::new(format!(
                "first line must be '*** Begin Patch'. {PATCH_RETRY_GUIDANCE}"
            )));
        }
        (_, Some(last)) if last != END_PATCH => {
            return Err(PatchError::new(format!(
                "last line must be '*** End Patch'; send the complete patch including the closing marker. {PATCH_RETRY_GUIDANCE}"
            )));
        }
        _ => {
            return Err(PatchError::new(format!(
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
            return Err(PatchError::new(
                "apply_patch environment_id cannot be empty",
            ));
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
                return Err(PatchError::new(format!(
                    "update hunk for '{path}' is empty"
                )));
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
        return Err(PatchError::new("patch does not contain any hunks"));
    }
    Ok(hunks)
}

fn normalize_patch_input(patch: &str) -> PatchResult<Cow<'_, str>> {
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
        return Err(PatchError::new(
            "patch input contains multiple patch blocks; send exactly one *** Begin Patch block",
        ));
    }
    let Some(begin_index) = begin_indices.first().copied() else {
        return Ok(Cow::Borrowed(trimmed));
    };
    let Some(end_index) = end_indices.first().copied() else {
        return Err(PatchError::new(format!(
            "last line must be '*** End Patch'; send the complete patch including the closing marker. {PATCH_RETRY_GUIDANCE}"
        )));
    };
    if end_index < begin_index {
        return Err(PatchError::new(format!(
            "last line must be '*** End Patch'. {PATCH_RETRY_GUIDANCE}"
        )));
    }
    if begin_index == 0 && end_index + 1 == lines.len() {
        return Ok(Cow::Borrowed(trimmed));
    }
    Ok(Cow::Owned(lines[begin_index..=end_index].join("\n")))
}

fn strip_heredoc_wrapper<'a>(lines: &'a [&'a str]) -> PatchResult<Option<Vec<&'a str>>> {
    let [first, .., last] = lines else {
        return Ok(None);
    };
    let first = first.trim();
    if !matches!(first, "<<EOF" | "<<'EOF'" | "<<\"EOF\"") {
        return Ok(None);
    }
    if last.trim_end() != "EOF" {
        return Err(PatchError::new(
            "missing closing EOF marker for apply_patch heredoc",
        ));
    }
    if lines.len() < 4 {
        return Err(PatchError::new(
            "apply_patch heredoc does not contain a patch block",
        ));
    }
    Ok(Some(lines[1..lines.len() - 1].to_vec()))
}

fn invalid_hunk_header(line: &str, line_number: usize) -> PatchError {
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
    PatchError::new(format!(
        "invalid hunk header at line {line_number}: '{line}'. {guidance}; {VALID_HUNK_HEADERS}. {PATCH_RETRY_GUIDANCE}"
    ))
}

fn parse_update_chunk(lines: &[&str], line_number: usize) -> PatchResult<(UpdateChunk, usize)> {
    let mut index = 0;
    let context = match lines.first().copied() {
        Some("@@") => {
            index = 1;
            None
        }
        Some(line) if line.starts_with("@@ -") => {
            return Err(PatchError::new(format!(
                "invalid update hunk at line {line_number}: unified diff hunk ranges are not supported; use '@@' or '@@ <search context>'"
            )));
        }
        Some(line) if line.starts_with("@@ ") => {
            index = 1;
            Some(line.trim_start_matches("@@ ").to_string())
        }
        Some(_) => None,
        None => {
            return Err(PatchError::new(format!(
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
        return Err(PatchError::new(format!(
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

fn apply_chunks(content: &str, path: &Path, chunks: &[UpdateChunk]) -> PatchResult<String> {
    let mut lines: Vec<String> = content.split('\n').map(String::from).collect();
    if lines.last().is_some_and(String::is_empty) {
        lines.pop();
    }
    let mut cursor = 0;
    let mut replacements = Vec::new();
    let allow_preserved_json_context = supports_preserved_json_context(path);

    for chunk in chunks {
        if let Some(context) = &chunk.context {
            let Some(context_index) =
                find_sequence(&lines, std::slice::from_ref(context), cursor, false)
            else {
                return Err(PatchError::new(format!(
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

        let Some((start, old_len, new_lines)) =
            find_chunk_replacement(&lines, chunk, cursor, allow_preserved_json_context)
        else {
            return Err(PatchError::new(format!(
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
    allow_preserved_json_context: bool,
) -> Option<(usize, usize, Vec<String>)> {
    let mut candidates = vec![(chunk.old_lines.clone(), chunk.new_lines.clone())];
    if let Some(candidate) = duplicated_edge_context_candidate(chunk) {
        candidates.push(candidate);
    }

    for (old_lines, new_lines) in candidates {
        if let Some(start) = find_sequence(lines, &old_lines, cursor, chunk.eof).or_else(|| {
            if allow_preserved_json_context {
                find_preserved_json_context_sequence(
                    lines, &old_lines, &new_lines, cursor, chunk.eof,
                )
            } else {
                None
            }
        }) {
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
            if let Some(start) = find_sequence(lines, &old_lines, cursor, chunk.eof).or_else(|| {
                if allow_preserved_json_context {
                    find_preserved_json_context_sequence(
                        lines, &old_lines, &new_lines, cursor, chunk.eof,
                    )
                } else {
                    None
                }
            }) {
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
    let mut old_search_start = 0;
    new_lines
        .iter()
        .map(|line| {
            let Some(relative_index) = old_lines[old_search_start..]
                .iter()
                .position(|old_line| lines_equivalent(line, old_line))
            else {
                return line.clone();
            };
            let old_index = old_search_start + relative_index;
            old_search_start = old_index + 1;
            matched_lines[old_index].clone()
        })
        .collect()
}

fn supports_preserved_json_context(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            extension.eq_ignore_ascii_case("json") || extension.eq_ignore_ascii_case("arb")
        })
}

fn find_preserved_json_context_sequence(
    lines: &[String],
    old_lines: &[String],
    new_lines: &[String],
    start: usize,
    eof: bool,
) -> Option<usize> {
    if old_lines.is_empty() || old_lines.len() > lines.len() {
        return None;
    }
    let preserved = preserved_old_lines(old_lines, new_lines);
    if !preserved.iter().any(|preserved| *preserved) {
        return None;
    }
    let last_start = lines.len().saturating_sub(old_lines.len());
    let matches_at = |index: usize| {
        old_lines.iter().enumerate().all(|(offset, expected)| {
            let actual = &lines[index + offset];
            lines_equivalent(actual, expected)
                || preserved[offset] && same_json_property_key(actual, expected)
        })
    };
    if eof && matches_at(last_start) {
        return Some(last_start);
    }
    (start..=last_start).find(|index| matches_at(*index))
}

fn preserved_old_lines(old_lines: &[String], new_lines: &[String]) -> Vec<bool> {
    let mut preserved = vec![false; old_lines.len()];
    let mut new_search_start = 0;
    for (old_index, old_line) in old_lines.iter().enumerate() {
        let Some(relative_index) = new_lines[new_search_start..]
            .iter()
            .position(|new_line| lines_equivalent(old_line, new_line))
        else {
            continue;
        };
        preserved[old_index] = true;
        new_search_start += relative_index + 1;
    }
    preserved
}

fn same_json_property_key(left: &str, right: &str) -> bool {
    json_property_key(left)
        .zip(json_property_key(right))
        .is_some_and(|(left, right)| left == right)
}

fn json_property_key(line: &str) -> Option<&str> {
    let property = line.trim_start().strip_prefix('"')?;
    let key_end = property.find("\":")?;
    let key = &property[..key_end];
    (!key.contains("\\\"")).then_some(key)
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

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};

    use pretty_assertions::assert_eq;

    use super::*;

    #[derive(Debug, Default, Clone)]
    struct MemoryBackend {
        files: Arc<Mutex<HashMap<PathBuf, String>>>,
    }

    impl MemoryBackend {
        fn with_file(path: impl Into<PathBuf>, content: impl Into<String>) -> Self {
            let backend = Self::default();
            backend
                .files
                .lock()
                .unwrap()
                .insert(path.into(), content.into());
            backend
        }

        fn read(&self, path: impl AsRef<Path>) -> Option<String> {
            self.files.lock().unwrap().get(path.as_ref()).cloned()
        }
    }

    impl PatchPathDisplay for MemoryBackend {
        fn display_path(&self, path: &Path) -> String {
            path.display().to_string()
        }
    }

    impl PatchBackend for MemoryBackend {
        async fn resolve_existing(&self, path: &str) -> PatchResult<PathBuf> {
            let path = PathBuf::from(path);
            if self.files.lock().unwrap().contains_key(&path) {
                Ok(path)
            } else {
                Err(PatchError::new(format!(
                    "failed to resolve path '{}': not found",
                    path.display()
                )))
            }
        }

        async fn resolve_for_write(&self, path: &str) -> PatchResult<PathBuf> {
            Ok(PathBuf::from(path))
        }

        async fn reject_symlink_write(&self, _path: &Path) -> PatchResult<()> {
            Ok(())
        }

        async fn ensure_file(&self, path: &Path) -> PatchResult<()> {
            if self.files.lock().unwrap().contains_key(path) {
                Ok(())
            } else {
                Err(PatchError::new(format!(
                    "cannot delete '{}': path is not a file",
                    path.display()
                )))
            }
        }

        async fn read_to_string(&self, path: &Path) -> PatchResult<String> {
            self.files
                .lock()
                .unwrap()
                .get(path)
                .cloned()
                .ok_or_else(|| {
                    PatchError::new(format!("failed to read '{}': not found", path.display()))
                })
        }

        async fn read_optional_text(&self, path: &Path) -> PatchResult<Option<String>> {
            Ok(self.files.lock().unwrap().get(path).cloned())
        }

        async fn create_parent_dirs(&self, _path: &Path) -> PatchResult<()> {
            Ok(())
        }

        async fn write_text(&self, path: &Path, content: &str) -> PatchResult<()> {
            self.files
                .lock()
                .unwrap()
                .insert(path.to_path_buf(), content.to_string());
            Ok(())
        }

        async fn remove_file(&self, path: &Path) -> PatchResult<()> {
            self.files.lock().unwrap().remove(path);
            Ok(())
        }
    }

    #[test]
    fn invalid_header_reports_recovery_guidance() {
        let error = parse_patch("*** Begin Patch\n--- a/file.txt\n*** End Patch").unwrap_err();

        assert!(error.message().contains("unified diff"));
        assert!(error.message().contains("*** Update File:"));
        assert!(
            error
                .message()
                .contains("Recovery: read the target file again")
        );
    }

    #[tokio::test]
    async fn applies_add_then_update_in_order() {
        let backend = MemoryBackend::default();
        let patch = "*** Begin Patch\n*** Add File: notes.txt\n+new\n*** Update File: notes.txt\n@@\n-new\n+newer\n*** End Patch";

        let outcome = apply_patch(patch, &backend).await.unwrap();

        assert_eq!(backend.read("notes.txt"), Some("newer\n".to_string()));
        assert_eq!(
            outcome.summary(&backend),
            "Success. Updated the following files:\nA notes.txt\nM notes.txt\n"
        );
    }

    #[tokio::test]
    async fn unicode_context_uses_normalized_matching() {
        let backend = MemoryBackend::with_file(
            "unicode.txt",
            "import asyncio  # local import \u{2013} avoids top\u{2011}level dep\n",
        );
        let patch = "*** Begin Patch\n*** Update File: unicode.txt\n@@\n-import asyncio  # local import - avoids top-level dep\n+done\n*** End Patch";

        apply_patch(patch, &backend).await.unwrap();

        assert_eq!(backend.read("unicode.txt"), Some("done\n".to_string()));
    }

    #[tokio::test]
    async fn preserved_arb_keys_match_without_overwriting_current_values() {
        let backend = MemoryBackend::with_file(
            "app_zh.arb",
            "{\n  \"settingsModelField\": \"Model\",\n  \"settingsMcpTitle\": \"MCP\"\n}\n",
        );
        let patch = "*** Begin Patch\n*** Update File: app_zh.arb\n@@\n   \"settingsModelField\": \"模型\",\n+  \"settingsReasoningEffortField\": \"推理强度\",\n   \"settingsMcpTitle\": \"MCP\"\n*** End Patch";

        apply_patch(patch, &backend).await.unwrap();

        assert_eq!(
            backend.read("app_zh.arb"),
            Some(
                "{\n  \"settingsModelField\": \"Model\",\n  \"settingsReasoningEffortField\": \"推理强度\",\n  \"settingsMcpTitle\": \"MCP\"\n}\n"
                    .to_string()
            )
        );
    }

    #[tokio::test]
    async fn arb_value_replacement_still_requires_the_expected_value() {
        let backend =
            MemoryBackend::with_file("app_zh.arb", "{\n  \"settingsModelField\": \"Model\"\n}\n");
        let patch = "*** Begin Patch\n*** Update File: app_zh.arb\n@@\n-  \"settingsModelField\": \"模型\"\n+  \"settingsModelField\": \"模型名称\"\n*** End Patch";

        let error = apply_patch(patch, &backend).await.unwrap_err();

        assert!(error.message().contains("failed to find expected lines"));
        assert_eq!(
            backend.read("app_zh.arb"),
            Some("{\n  \"settingsModelField\": \"Model\"\n}\n".to_string())
        );
    }

    #[tokio::test]
    async fn json_shaped_text_files_do_not_use_key_matching() {
        let backend = MemoryBackend::with_file(
            "notes.txt",
            "\"settingsModelField\": \"Model\",\n\"settingsMcpTitle\": \"MCP\"\n",
        );
        let patch = "*** Begin Patch\n*** Update File: notes.txt\n@@\n \"settingsModelField\": \"模型\",\n+\"settingsReasoningEffortField\": \"推理强度\",\n \"settingsMcpTitle\": \"MCP\"\n*** End Patch";

        let error = apply_patch(patch, &backend).await.unwrap_err();

        assert!(error.message().contains("failed to find expected lines"));
        assert_eq!(
            backend.read("notes.txt"),
            Some("\"settingsModelField\": \"Model\",\n\"settingsMcpTitle\": \"MCP\"\n".to_string())
        );
    }

    #[tokio::test]
    async fn failure_reports_committed_prefix() {
        let backend = MemoryBackend::default();
        let patch = "*** Begin Patch\n*** Add File: created.txt\n+hello\n*** Update File: missing.txt\n@@\n-old\n+new\n*** End Patch";

        let error = apply_patch(patch, &backend).await.unwrap_err();

        assert!(
            error
                .message()
                .contains("failed to resolve path 'missing.txt'")
        );
        assert!(error.message().contains("Committed changes before failure"));
        assert!(error.message().contains("A created.txt"));
        assert_eq!(backend.read("created.txt"), Some("hello\n".to_string()));
    }
}

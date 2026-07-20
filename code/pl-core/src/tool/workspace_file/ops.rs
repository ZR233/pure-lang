use base64::Engine;
use pl_protocol::{PureError, Result};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;

use crate::tool::OutputTruncation;
use crate::tool::model_visible_tool_output;

use super::backend::{
    WorkspaceFileBackend, WorkspaceFileListRequest, WorkspaceFileReadRequest,
    WorkspaceFileSearchRequest, WorkspaceFileStatRequest,
};
use super::patch::apply_patch_to_backend;
use super::schema::{
    TOOL_APPLY_PATCH, TOOL_LIST_FILES, TOOL_READ_FILE, TOOL_SEARCH_FILES, WorkspaceFileToolKind,
};

const DEFAULT_READ_FILE_LINES: usize = 200;
const MAX_READ_FILE_LINES: usize = 500;
const DEFAULT_LIST_FILES_LIMIT: usize = 100;
const MAX_LIST_FILES_LIMIT: usize = 200;
const DEFAULT_SEARCH_MATCH_LIMIT: usize = 100;
const MAX_SEARCH_MATCH_LIMIT: usize = 200;

#[derive(Debug, Clone, PartialEq)]
pub struct WorkspaceFileToolExecution {
    pub success: bool,
    pub output: String,
    pub model_output: String,
    pub exit_code: Option<i32>,
    pub truncated: OutputTruncation,
}

impl WorkspaceFileToolExecution {
    fn json(output: Value) -> Result<Self> {
        let output = serde_json::to_string(&output).map_err(|error| {
            tool_error(
                "workspace_file",
                format!("failed to encode output: {error}"),
            )
        })?;
        Ok(Self {
            success: true,
            model_output: model_visible_tool_output(&output),
            output,
            exit_code: Some(0),
            truncated: OutputTruncation::empty(),
        })
    }
}

pub async fn execute_workspace_file_tool<B>(
    backend: &B,
    name: &str,
    arguments: Value,
    _cancellation_token: Option<CancellationToken>,
    workspace_epoch: u64,
) -> Result<Option<WorkspaceFileToolExecution>>
where
    B: WorkspaceFileBackend,
{
    let Some(kind) = WorkspaceFileToolKind::from_name(name) else {
        return Ok(None);
    };
    let value = match kind {
        WorkspaceFileToolKind::ReadFile => read_file(backend, arguments).await?,
        WorkspaceFileToolKind::ListFiles => list_files(backend, arguments, workspace_epoch).await?,
        WorkspaceFileToolKind::SearchFiles => {
            search_files(backend, arguments, workspace_epoch).await?
        }
        WorkspaceFileToolKind::ApplyPatch => apply_patch(backend, arguments).await?,
    };
    Ok(Some(WorkspaceFileToolExecution::json(value)?))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ReadFileInput {
    path: String,
    cwd: Option<String>,
    start_line: Option<usize>,
    max_lines: Option<usize>,
}

async fn read_file<B>(backend: &B, arguments: Value) -> Result<Value>
where
    B: WorkspaceFileBackend,
{
    let input: ReadFileInput = parse_input(arguments, TOOL_READ_FILE)?;
    let start_line = input.start_line.unwrap_or(1);
    if start_line == 0 {
        return Err(tool_error(TOOL_READ_FILE, "startLine is 1-based"));
    }
    let max_lines = input.max_lines.unwrap_or(DEFAULT_READ_FILE_LINES);
    if !(1..=MAX_READ_FILE_LINES).contains(&max_lines) {
        return Err(tool_error(
            TOOL_READ_FILE,
            format!("maxLines must be between 1 and {MAX_READ_FILE_LINES}"),
        ));
    }
    let stat = backend
        .stat(WorkspaceFileStatRequest {
            path: input.path.clone(),
            cwd: input.cwd.clone(),
        })
        .await?;
    if !stat.is_file {
        return Err(tool_error(
            TOOL_READ_FILE,
            format!("'{}' is not a regular file", input.path),
        ));
    }
    let content = backend
        .read_text(WorkspaceFileReadRequest {
            path: input.path.clone(),
            cwd: input.cwd.clone(),
        })
        .await?;
    let start = line_start_byte_offset(&content, start_line)
        .map_err(|error| tool_error(TOOL_READ_FILE, error))?;
    let end = line_end_byte_offset(&content, start, Some(max_lines));
    let text = content[start..end].to_string();
    let returned_lines = logical_line_count(&text);
    let end_line = if returned_lines == 0 {
        start_line.saturating_sub(1)
    } else {
        start_line.saturating_add(returned_lines.saturating_sub(1))
    };
    let next_start_line = (end < content.len()).then_some(end_line.saturating_add(1));
    Ok(json!({
        "path": input.path,
        "startLine": start_line,
        "endLine": end_line,
        "nextStartLine": next_start_line,
        "contentHash": crate::working_set::canonical_content_hash(content.as_bytes()),
        "text": text,
    }))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ListFilesInput {
    path: Option<String>,
    cwd: Option<String>,
    glob: Option<String>,
    limit: Option<usize>,
    cursor: Option<String>,
    include_dirs: Option<bool>,
}

async fn list_files<B>(backend: &B, arguments: Value, workspace_epoch: u64) -> Result<Value>
where
    B: WorkspaceFileBackend,
{
    let input: ListFilesInput = parse_input(arguments, TOOL_LIST_FILES)?;
    let path = path_or_current(input.path);
    let glob = input.glob.unwrap_or_else(|| "*".to_string());
    let include_dirs = input.include_dirs.unwrap_or(false);
    let limit = input.limit.unwrap_or(DEFAULT_LIST_FILES_LIMIT);
    if !(1..=MAX_LIST_FILES_LIMIT).contains(&limit) {
        return Err(tool_error(
            TOOL_LIST_FILES,
            format!("limit must be between 1 and {MAX_LIST_FILES_LIMIT}"),
        ));
    }
    let cursor_key = cursor_key(&json!({
        "path": path,
        "cwd": input.cwd,
        "glob": glob,
        "includeDirs": include_dirs,
        "workspaceEpoch": workspace_epoch,
    }));
    let cursor = decode_cursor(input.cursor.as_deref(), CursorKind::List, &cursor_key)?;
    let offset = cursor.offset;
    let result = backend
        .list(WorkspaceFileListRequest {
            path: path.clone(),
            cwd: input.cwd,
            glob: glob.clone(),
            max_files: offset.saturating_add(limit).saturating_add(1),
            include_dirs,
        })
        .await?;
    let end = offset.saturating_add(limit).min(result.files.len());
    let files = result.files.get(offset..end).unwrap_or_default().to_vec();
    let has_more = end < result.files.len() || result.truncated;
    let next_cursor = has_more.then(|| encode_cursor(CursorKind::List, &cursor_key, end));
    let result_hash =
        crate::working_set::canonical_content_hash(serde_json::to_string(&files)?.as_bytes());
    Ok(json!({
        "path": path,
        "glob": glob,
        "includeDirs": include_dirs,
        "files": files,
        "count": files.len(),
        "nextCursor": next_cursor,
        "cursorReset": cursor.reset,
        "resultHash": result_hash,
        "workspaceEpoch": workspace_epoch,
    }))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SearchFilesInput {
    query: String,
    path: Option<String>,
    cwd: Option<String>,
    glob: Option<String>,
    case_sensitive: Option<bool>,
    literal: Option<bool>,
    limit: Option<usize>,
    cursor: Option<String>,
    context_lines: Option<usize>,
}

async fn search_files<B>(backend: &B, arguments: Value, workspace_epoch: u64) -> Result<Value>
where
    B: WorkspaceFileBackend,
{
    let input: SearchFilesInput = parse_input(arguments, TOOL_SEARCH_FILES)?;
    let path = path_or_current(input.path);
    let limit = input.limit.unwrap_or(DEFAULT_SEARCH_MATCH_LIMIT);
    if !(1..=MAX_SEARCH_MATCH_LIMIT).contains(&limit) {
        return Err(tool_error(
            TOOL_SEARCH_FILES,
            format!("limit must be between 1 and {MAX_SEARCH_MATCH_LIMIT}"),
        ));
    }
    let case_sensitive = input.case_sensitive.unwrap_or(true);
    let literal = input.literal.unwrap_or(false);
    let context_lines = input.context_lines.unwrap_or(0);
    if context_lines > 20 {
        return Err(tool_error(
            TOOL_SEARCH_FILES,
            "contextLines must be between 0 and 20",
        ));
    }
    let cursor_key = cursor_key(&json!({
        "query": input.query,
        "path": path,
        "cwd": input.cwd,
        "glob": input.glob,
        "caseSensitive": case_sensitive,
        "literal": literal,
        "contextLines": context_lines,
        "workspaceEpoch": workspace_epoch,
    }));
    let cursor = decode_cursor(input.cursor.as_deref(), CursorKind::Search, &cursor_key)?;
    let offset = cursor.offset;
    let result = backend
        .search(WorkspaceFileSearchRequest {
            query: input.query.clone(),
            path: path.clone(),
            cwd: input.cwd,
            glob: input.glob.clone(),
            case_sensitive,
            literal,
            max_matches: offset.saturating_add(limit).saturating_add(1),
            context_lines,
        })
        .await?;
    let end = offset.saturating_add(limit).min(result.matches.len());
    let matches = result.matches.get(offset..end).unwrap_or_default();
    let files = group_search_matches(matches);
    let has_more = end < result.matches.len() || result.truncated;
    let next_cursor = has_more.then(|| encode_cursor(CursorKind::Search, &cursor_key, end));
    let result_hash =
        crate::working_set::canonical_content_hash(serde_json::to_string(matches)?.as_bytes());
    Ok(json!({
        "query": input.query,
        "path": path,
        "glob": input.glob,
        "files": files,
        "count": matches.len(),
        "nextCursor": next_cursor,
        "cursorReset": cursor.reset,
        "resultHash": result_hash,
        "workspaceEpoch": workspace_epoch,
    }))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum CursorKind {
    List,
    Search,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct WorkspaceCursor {
    kind: CursorKind,
    key: String,
    offset: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct CursorResolution {
    offset: usize,
    reset: bool,
}

fn cursor_key(value: &Value) -> String {
    crate::working_set::canonical_content_hash(value.to_string().as_bytes())
}

fn encode_cursor(kind: CursorKind, key: &str, offset: usize) -> String {
    let value = serde_json::to_vec(&WorkspaceCursor {
        kind,
        key: key.to_string(),
        offset,
    })
    .expect("workspace cursor serialization must succeed");
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(value)
}

fn decode_cursor(
    cursor: Option<&str>,
    expected_kind: CursorKind,
    expected_key: &str,
) -> Result<CursorResolution> {
    let Some(cursor) = cursor.map(str::trim).filter(|cursor| !cursor.is_empty()) else {
        return Ok(CursorResolution {
            offset: 0,
            reset: false,
        });
    };
    let Ok(bytes) = base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(cursor) else {
        return Ok(CursorResolution {
            offset: 0,
            reset: true,
        });
    };
    let Ok(cursor) = serde_json::from_slice::<WorkspaceCursor>(&bytes) else {
        return Ok(CursorResolution {
            offset: 0,
            reset: true,
        });
    };
    if cursor.kind != expected_kind || cursor.key != expected_key {
        return Err(tool_error(
            "workspace_cursor",
            "cursor does not belong to this request",
        ));
    }
    Ok(CursorResolution {
        offset: cursor.offset,
        reset: false,
    })
}

fn group_search_matches(matches: &[super::WorkspaceFileSearchMatch]) -> Vec<Value> {
    let mut grouped = std::collections::BTreeMap::<&str, Vec<Value>>::new();
    for item in matches {
        grouped.entry(&item.path).or_default().push(json!({
            "line": item.line,
            "column": item.column,
            "text": item.text,
        }));
    }
    grouped
        .into_iter()
        .map(|(path, matches)| json!({ "path": path, "matches": matches }))
        .collect()
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ApplyPatchInput {
    input: String,
    cwd: Option<String>,
}

async fn apply_patch<B>(backend: &B, arguments: Value) -> Result<Value>
where
    B: WorkspaceFileBackend,
{
    let input: ApplyPatchInput = parse_input(arguments, TOOL_APPLY_PATCH)?;
    let cwd = match input.cwd {
        Some(cwd) => cwd,
        None => backend.default_cwd().await?,
    };
    let output = apply_patch_to_backend(backend, cwd, &input.input).await?;
    serde_json::to_value(output).map_err(|error| {
        tool_error(
            TOOL_APPLY_PATCH,
            format!("failed to encode apply_patch output: {error}"),
        )
    })
}

fn parse_input<T: serde::de::DeserializeOwned>(arguments: Value, tool: &str) -> Result<T> {
    serde_json::from_value(arguments)
        .map_err(|error| tool_error(tool, format!("invalid input: {error}")))
}

fn path_or_current(path: Option<String>) -> String {
    path.filter(|path| !path.trim().is_empty())
        .unwrap_or_else(|| ".".to_string())
}

pub(crate) fn tool_error(tool: &str, error: impl std::fmt::Display) -> PureError {
    PureError::ToolExecutionFailed {
        tool: tool.to_string(),
        error: error.to_string(),
    }
}

fn line_start_byte_offset(content: &str, line_start: usize) -> std::result::Result<usize, String> {
    if line_start <= 1 {
        return Ok(0);
    }
    let mut current_line = 1;
    for (idx, ch) in content.char_indices() {
        if ch == '\n' {
            current_line += 1;
            if current_line == line_start {
                return Ok(idx + 1);
            }
        }
    }
    Err(format!(
        "startLine {line_start} exceeds file length ({current_line} lines)"
    ))
}

fn line_end_byte_offset(content: &str, start_byte: usize, line_count: Option<usize>) -> usize {
    let Some(line_count) = line_count else {
        return content.len();
    };
    let mut lines_seen = 1;
    for (relative_idx, ch) in content[start_byte..].char_indices() {
        if ch == '\n' {
            if lines_seen == line_count {
                return start_byte + relative_idx + 1;
            }
            lines_seen += 1;
        }
    }
    content.len()
}

fn logical_line_count(content: &str) -> usize {
    if content.is_empty() {
        return 0;
    }
    content.lines().count().max(1)
}

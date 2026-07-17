use pl_protocol::{PureError, Result};
use serde::Deserialize;
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;

use crate::tool::OutputTruncation;

use super::backend::{
    WorkspaceFileBackend, WorkspaceFileListRequest, WorkspaceFileReadRequest,
    WorkspaceFileSearchRequest, WorkspaceFileStatRequest,
};
use super::patch::apply_patch_to_backend;
use super::schema::{
    TOOL_APPLY_PATCH, TOOL_LIST_FILES, TOOL_READ_FILE, TOOL_SEARCH_FILES, WorkspaceFileToolKind,
};

const DEFAULT_READ_FILE_BYTES: usize = 50 * 1024;
const MAX_READ_FILE_BYTES: usize = 512 * 1024;
const DEFAULT_LIST_FILES_LIMIT: usize = 200;
const MAX_LIST_FILES_LIMIT: usize = 1_000;
const DEFAULT_SEARCH_MATCH_LIMIT: usize = 100;
const MAX_SEARCH_MATCH_LIMIT: usize = 2_000;

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
            model_output: output.clone(),
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
) -> Result<Option<WorkspaceFileToolExecution>>
where
    B: WorkspaceFileBackend,
{
    let Some(kind) = WorkspaceFileToolKind::from_name(name) else {
        return Ok(None);
    };
    let value = match kind {
        WorkspaceFileToolKind::ReadFile => read_file(backend, arguments).await?,
        WorkspaceFileToolKind::ListFiles => list_files(backend, arguments).await?,
        WorkspaceFileToolKind::SearchFiles => search_files(backend, arguments).await?,
        WorkspaceFileToolKind::ApplyPatch => apply_patch(backend, arguments).await?,
    };
    Ok(Some(WorkspaceFileToolExecution::json(value)?))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ReadFileInput {
    path: String,
    cwd: Option<String>,
    line_start: Option<usize>,
    line_count: Option<usize>,
    offset: Option<usize>,
    max_bytes: Option<usize>,
}

async fn read_file<B>(backend: &B, arguments: Value) -> Result<Value>
where
    B: WorkspaceFileBackend,
{
    let input: ReadFileInput = parse_input(arguments, TOOL_READ_FILE)?;
    let offset = input.offset.unwrap_or(0);
    if input.line_start.is_some() && offset > 0 {
        return Err(tool_error(
            TOOL_READ_FILE,
            "read_file cannot combine line_start with offset",
        ));
    }
    let max_bytes = input
        .max_bytes
        .unwrap_or(DEFAULT_READ_FILE_BYTES)
        .clamp(1, MAX_READ_FILE_BYTES);
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
    let (window, window_offset) = if let Some(line_start) = input.line_start {
        let start = line_start_byte_offset(&content, line_start)
            .map_err(|error| tool_error(TOOL_READ_FILE, error))?;
        let end = line_end_byte_offset(&content, start, input.line_count);
        (&content[start..end], 0)
    } else {
        let start = byte_offset_boundary(&content, offset)?;
        (&content[start..], offset)
    };
    let (text, truncated, bytes_omitted, next_offset) =
        bounded_text(window, max_bytes, window_offset);
    Ok(json!({
        "path": input.path,
        "text": text,
        "offset": window_offset,
        "bytesReturned": text.len(),
        "bytesOmitted": bytes_omitted,
        "truncated": truncated,
        "nextOffset": next_offset,
    }))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ListFilesInput {
    path: Option<String>,
    cwd: Option<String>,
    glob: Option<String>,
    max_files: Option<usize>,
    include_dirs: Option<bool>,
}

async fn list_files<B>(backend: &B, arguments: Value) -> Result<Value>
where
    B: WorkspaceFileBackend,
{
    let input: ListFilesInput = parse_input(arguments, TOOL_LIST_FILES)?;
    let path = path_or_current(input.path);
    let glob = input.glob.unwrap_or_else(|| "*".to_string());
    let include_dirs = input.include_dirs.unwrap_or(false);
    let max_files = input
        .max_files
        .unwrap_or(DEFAULT_LIST_FILES_LIMIT)
        .clamp(1, MAX_LIST_FILES_LIMIT);
    let result = backend
        .list(WorkspaceFileListRequest {
            path: path.clone(),
            cwd: input.cwd,
            glob: glob.clone(),
            max_files,
            include_dirs,
        })
        .await?;
    Ok(json!({
        "path": path,
        "glob": glob,
        "includeDirs": include_dirs,
        "files": result.files,
        "count": result.files.len(),
        "truncated": result.truncated,
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
    max_matches: Option<usize>,
    context_lines: Option<usize>,
}

async fn search_files<B>(backend: &B, arguments: Value) -> Result<Value>
where
    B: WorkspaceFileBackend,
{
    let input: SearchFilesInput = parse_input(arguments, TOOL_SEARCH_FILES)?;
    let path = path_or_current(input.path);
    let max_matches = input
        .max_matches
        .unwrap_or(DEFAULT_SEARCH_MATCH_LIMIT)
        .clamp(1, MAX_SEARCH_MATCH_LIMIT);
    let result = backend
        .search(WorkspaceFileSearchRequest {
            query: input.query.clone(),
            path: path.clone(),
            cwd: input.cwd,
            glob: input.glob.clone(),
            case_sensitive: input.case_sensitive.unwrap_or(true),
            literal: input.literal.unwrap_or(false),
            max_matches,
            context_lines: input.context_lines.unwrap_or(0).min(20),
        })
        .await?;
    Ok(json!({
        "query": input.query,
        "path": path,
        "glob": input.glob,
        "matches": result.matches,
        "count": result.matches.len(),
        "truncated": result.truncated,
    }))
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

pub(crate) fn bounded_text(
    value: &str,
    max_bytes: usize,
    offset: usize,
) -> (String, bool, usize, Option<usize>) {
    if value.len() <= max_bytes {
        return (value.to_string(), false, 0, None);
    }
    let mut end = max_bytes;
    while !value.is_char_boundary(end) {
        end = end.saturating_sub(1);
    }
    let text = value[..end].to_string();
    let omitted = value.len().saturating_sub(end);
    (text, true, omitted, Some(offset.saturating_add(end)))
}

fn byte_offset_boundary(content: &str, offset: usize) -> Result<usize> {
    if offset >= content.len() {
        return Ok(content.len());
    }
    if content.is_char_boundary(offset) {
        return Ok(offset);
    }
    Err(tool_error(
        TOOL_READ_FILE,
        format!("offset {offset} is not on a UTF-8 character boundary"),
    ))
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
        "line_start {line_start} exceeds file length ({current_line} lines)"
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

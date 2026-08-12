use base64::Engine;
use pl_protocol::{PureError, Result};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;

use crate::tool::OutputTruncation;
use crate::tool::model_visible_tool_output;
use crate::tool::text_document::{
    line_end_byte_offset, line_start_byte_offset, logical_line_count,
};

use super::backend::{
    WorkspaceFileBackend, WorkspaceFileListRequest, WorkspaceFileReadRequest,
    WorkspaceFileStatRequest,
};
use super::patch::apply_patch_to_backend;
use super::schema::{TOOL_APPLY_PATCH, TOOL_LIST_FILES, TOOL_READ_FILE, WorkspaceFileToolKind};

const DEFAULT_READ_FILE_LINES: usize = 200;
const MAX_READ_FILE_LINES: usize = 500;
const MAX_READ_PATH_SUGGESTIONS: usize = 5;
const DEFAULT_LIST_FILES_LIMIT: usize = 100;
const MAX_LIST_FILES_LIMIT: usize = 200;

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
    let stat_request = WorkspaceFileStatRequest {
        path: input.path.clone(),
        cwd: input.cwd.clone(),
    };
    let stat = match backend.stat(stat_request).await {
        Ok(stat) => stat,
        Err(error) => {
            return Err(unresolved_read_path_error(backend, &input.path, error).await);
        }
    };
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

async fn unresolved_read_path_error<B>(backend: &B, path: &str, error: PureError) -> PureError
where
    B: WorkspaceFileBackend,
{
    let candidates = same_name_candidates(backend, path).await;
    tool_error(
        TOOL_READ_FILE,
        format!(
            "{error}; recovery={}",
            json!({ "candidatePaths": candidates })
        ),
    )
}

async fn same_name_candidates<B>(backend: &B, path: &str) -> Vec<String>
where
    B: WorkspaceFileBackend,
{
    let Some(file_name) = path_file_name(path) else {
        return Vec::new();
    };
    let Ok(result) = backend
        .list(WorkspaceFileListRequest {
            path: ".".to_string(),
            cwd: None,
            glob: format!("**/{file_name}"),
            max_files: MAX_READ_PATH_SUGGESTIONS,
            include_dirs: false,
        })
        .await
    else {
        return Vec::new();
    };
    result
        .files
        .into_iter()
        .filter(|candidate| path_file_name(candidate).is_some_and(|name| name == file_name))
        .take(MAX_READ_PATH_SUGGESTIONS)
        .map(|candidate| {
            candidate
                .strip_prefix("./")
                .unwrap_or(&candidate)
                .replace('\\', "/")
        })
        .collect()
}

fn path_file_name(path: &str) -> Option<&str> {
    path.rsplit(['/', '\\'])
        .find(|component| !component.is_empty())
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
    let glob = input
        .glob
        .filter(|glob| !glob.trim().is_empty())
        .unwrap_or_else(|| "*".to_string());
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum CursorKind {
    List,
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
    serde_json::from_value(arguments).map_err(|error| {
        tool_error(
            tool,
            format!("invalid input: {error}. {}", input_guidance(tool)),
        )
    })
}

fn input_guidance(tool: &str) -> &'static str {
    match tool {
        TOOL_READ_FILE => "Allowed fields: path, cwd, startLine, maxLines",
        TOOL_LIST_FILES => "Allowed fields: path, cwd, glob, limit, cursor, includeDirs",
        TOOL_APPLY_PATCH => {
            "Allowed fields: input, cwd. Re-read changed files and generate a smaller patch after a context conflict"
        }
        _ => "Check the tool schema and remove unknown fields",
    }
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

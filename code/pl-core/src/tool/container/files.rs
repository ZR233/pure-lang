use std::collections::BTreeSet;

use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use pl_protocol::Result;
use serde::Deserialize;
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;

use super::backend::{
    ContainerBackend, ContainerCopyFromRequest, ContainerCopyToRequest, ContainerExecRequest,
};
use super::helpers::{
    bounded_text, parse_input, preview_error, shell_command, shell_quote, tool_error,
};
use super::schema::{
    TOOL_CONTAINER_CP_DOWNLOAD, TOOL_CONTAINER_CP_UPLOAD, TOOL_LIST_FILES, TOOL_READ_FILE,
    TOOL_SEARCH_FILES,
};

const DEFAULT_READ_FILE_BYTES: usize = 50 * 1024;
const MAX_READ_FILE_BYTES: usize = 512 * 1024;
const DEFAULT_LIST_FILES_LIMIT: usize = 200;
const MAX_LIST_FILES_LIMIT: usize = 1_000;
const DEFAULT_SEARCH_MATCH_LIMIT: usize = 100;
const MAX_SEARCH_MATCH_LIMIT: usize = 2_000;
const MAX_SEARCH_OUTPUT_TEXT_BYTES: usize = 4 * 1024;

#[derive(Debug, Deserialize)]
struct ReadFileInput {
    path: String,
    cwd: Option<String>,
    line_start: Option<usize>,
    line_count: Option<usize>,
    offset: Option<usize>,
    max_bytes: Option<usize>,
}

pub(super) async fn read_file<B>(backend: &B, arguments: Value) -> Result<Value>
where
    B: ContainerBackend,
{
    let input: ReadFileInput = parse_input(arguments, TOOL_READ_FILE)?;
    let offset = input.offset.unwrap_or(0);
    let max_bytes = input
        .max_bytes
        .unwrap_or(DEFAULT_READ_FILE_BYTES)
        .clamp(1, MAX_READ_FILE_BYTES);
    if input.line_start.is_some() && offset > 0 {
        return Err(tool_error(
            TOOL_READ_FILE,
            "read_file cannot combine line_start with offset",
        ));
    }
    let command = if let Some(line_start) = input.line_start {
        let line_count = input.line_count.unwrap_or(200).clamp(1, 10_000);
        let end = line_start.saturating_add(line_count).saturating_sub(1);
        format!(
            "if [ ! -f {path} ]; then echo __PL_FILE_MISSING__; exit 0; fi; sed -n '{start},{end}p' {path}",
            path = shell_quote(&input.path),
            start = line_start,
            end = end
        )
    } else {
        format!(
            "if [ ! -f {path} ]; then echo __PL_FILE_MISSING__; exit 0; fi; dd if={path} bs=1 skip={offset} count={count} 2>/dev/null",
            path = shell_quote(&input.path),
            offset = offset,
            count = max_bytes.saturating_add(1)
        )
    };
    let output = backend
        .exec(ContainerExecRequest {
            command,
            cwd: input.cwd,
            timeout_secs: Some(20),
            output_bytes_cap: None,
            cancellation_token: None,
        })
        .await?;
    if output.stdout.trim() == "__PL_FILE_MISSING__" {
        return Err(tool_error(
            TOOL_READ_FILE,
            format!("file not found: {}", input.path),
        ));
    }
    if output.status != 0 {
        return Err(tool_error(
            TOOL_READ_FILE,
            format!(
                "read_file failed: {}",
                preview_error(&output.stderr, &output.stdout)
            ),
        ));
    }
    let (text, truncated, bytes_omitted, next_offset) =
        bounded_text(&output.stdout, max_bytes, offset);
    Ok(json!({
        "path": input.path,
        "offset": offset,
        "bytes_returned": text.len(),
        "bytes_omitted": bytes_omitted,
        "truncated": truncated,
        "next_offset": next_offset,
        "text": text,
    }))
}

#[derive(Debug, Deserialize)]
struct ListFilesInput {
    path: Option<String>,
    cwd: Option<String>,
    glob: Option<String>,
    pattern: Option<String>,
    max_files: Option<usize>,
    include_dirs: Option<bool>,
}

pub(super) async fn list_files<B>(backend: &B, arguments: Value) -> Result<Value>
where
    B: ContainerBackend,
{
    let input: ListFilesInput = parse_input(arguments, TOOL_LIST_FILES)?;
    let path = input.path.unwrap_or_else(|| ".".to_string());
    let glob = input
        .glob
        .or(input.pattern)
        .unwrap_or_else(|| "*".to_string());
    let max_files = input
        .max_files
        .unwrap_or(DEFAULT_LIST_FILES_LIMIT)
        .clamp(1, MAX_LIST_FILES_LIMIT);
    let include_dirs = input.include_dirs.unwrap_or(false);
    let limit = max_files.saturating_add(1);
    let rg_command = format!(
        "if command -v rg >/dev/null 2>&1; then rg --files -g {glob} {path} | sort | head -n {limit}; else exit 127; fi",
        path = shell_quote(&path),
        glob = shell_quote(&glob),
        limit = limit
    );
    let mut output = backend
        .exec(ContainerExecRequest {
            command: rg_command,
            cwd: input.cwd.clone(),
            timeout_secs: Some(20),
            output_bytes_cap: None,
            cancellation_token: None,
        })
        .await?;
    if output.status == 127 {
        let type_filter = if include_dirs { "" } else { "-type f " };
        let command = format!(
            "find {path} {type_filter}-name {glob} | sort | head -n {limit}",
            path = shell_quote(&path),
            type_filter = type_filter,
            glob = shell_quote(&glob),
            limit = limit
        );
        output = backend
            .exec(ContainerExecRequest {
                command,
                cwd: input.cwd.clone(),
                timeout_secs: Some(20),
                output_bytes_cap: None,
                cancellation_token: None,
            })
            .await?;
    } else if include_dirs {
        let dir_command = format!(
            "find {path} -type d -name {glob} | sort | head -n {limit}",
            path = shell_quote(&path),
            glob = shell_quote(&glob),
            limit = limit
        );
        let dirs = backend
            .exec(ContainerExecRequest {
                command: dir_command,
                cwd: input.cwd.clone(),
                timeout_secs: Some(20),
                output_bytes_cap: None,
                cancellation_token: None,
            })
            .await?;
        if dirs.status == 0 {
            output.stdout.push_str(&dirs.stdout);
        }
    }
    if output.status != 0 && output.status != 1 {
        return Err(tool_error(
            TOOL_LIST_FILES,
            format!(
                "list_files failed: {}",
                preview_error(&output.stderr, &output.stdout)
            ),
        ));
    }
    let mut entries = output
        .stdout
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(str::to_string)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .take(max_files.saturating_add(1))
        .collect::<Vec<_>>();
    let truncated = entries.len() > max_files;
    entries.truncate(max_files);
    Ok(json!({
        "path": path,
        "glob": glob,
        "include_dirs": include_dirs,
        "files": entries,
        "count": entries.len(),
        "truncated": truncated,
    }))
}

#[derive(Debug, Deserialize)]
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

#[derive(Debug, Deserialize)]
struct RgJsonEvent {
    #[serde(rename = "type")]
    kind: String,
    data: Option<RgJsonData>,
}

#[derive(Debug, Deserialize)]
struct RgJsonData {
    path: RgJsonText,
    lines: RgJsonText,
    line_number: Option<usize>,
    #[serde(default)]
    submatches: Vec<RgJsonSubmatch>,
}

#[derive(Debug, Deserialize)]
struct RgJsonText {
    text: String,
}

#[derive(Debug, Deserialize)]
struct RgJsonSubmatch {
    start: usize,
}

pub(super) async fn search_files<B>(
    backend: &B,
    arguments: Value,
    cancellation_token: Option<CancellationToken>,
) -> Result<Value>
where
    B: ContainerBackend,
{
    let input: SearchFilesInput = parse_input(arguments, TOOL_SEARCH_FILES)?;
    let path = input.path.unwrap_or_else(|| ".".to_string());
    let case_sensitive = input.case_sensitive.unwrap_or(true);
    let literal = input.literal.unwrap_or(false);
    let context_lines = input.context_lines.unwrap_or(0).min(20);
    let max_matches = input
        .max_matches
        .unwrap_or(DEFAULT_SEARCH_MATCH_LIMIT)
        .clamp(1, MAX_SEARCH_MATCH_LIMIT);
    let mut args = vec![
        "rg".to_string(),
        "--json".to_string(),
        "--line-number".to_string(),
        "--column".to_string(),
        "--max-count".to_string(),
        max_matches.to_string(),
    ];
    if !case_sensitive {
        args.push("--ignore-case".to_string());
    }
    if literal {
        args.push("--fixed-strings".to_string());
    }
    if context_lines > 0 {
        args.push("--context".to_string());
        args.push(context_lines.to_string());
    }
    if let Some(glob) = &input.glob {
        args.push("--glob".to_string());
        args.push(glob.clone());
    }
    args.push("--".to_string());
    args.push(input.query.clone());
    args.push(path.clone());
    let command = format!(
        "if command -v rg >/dev/null 2>&1; then {rg}; else exit 127; fi",
        rg = shell_command(&args)
    );
    let output = backend
        .exec(ContainerExecRequest {
            command,
            cwd: input.cwd.clone(),
            timeout_secs: Some(30),
            output_bytes_cap: None,
            cancellation_token: cancellation_token.clone(),
        })
        .await?;
    if output.status == 1 {
        return Ok(json!({
            "query": input.query,
            "path": path,
            "glob": input.glob,
            "matches": [],
            "count": 0,
            "truncated": false,
        }));
    }
    if output.status == 127 {
        return search_files_with_grep(
            backend,
            GrepSearchRequest {
                query: input.query,
                path,
                cwd: input.cwd,
                glob: input.glob,
                case_sensitive,
                literal,
                max_matches,
                cancellation_token,
            },
        )
        .await;
    }
    if output.status != 0 {
        return Err(tool_error(
            TOOL_SEARCH_FILES,
            format!(
                "search_files failed: {}",
                preview_error(&output.stderr, &output.stdout)
            ),
        ));
    }
    let mut matches = Vec::new();
    for line in output.stdout.lines() {
        if matches.len() >= max_matches {
            break;
        }
        let Ok(event) = serde_json::from_str::<RgJsonEvent>(line) else {
            continue;
        };
        if event.kind != "match" {
            continue;
        }
        let Some(data) = event.data else {
            continue;
        };
        let text = data.lines.text;
        let (text, _, _, _) = bounded_text(&text, MAX_SEARCH_OUTPUT_TEXT_BYTES, 0);
        let column = data
            .submatches
            .first()
            .map(|m| m.start.saturating_add(1))
            .unwrap_or(1);
        matches.push(json!({
            "path": data.path.text,
            "line": data.line_number.unwrap_or(0),
            "column": column,
            "text": text,
        }));
    }
    Ok(json!({
        "query": input.query,
        "path": path,
        "glob": input.glob,
        "matches": matches,
        "count": matches.len(),
        "truncated": matches.len() >= max_matches,
    }))
}

struct GrepSearchRequest {
    query: String,
    path: String,
    cwd: Option<String>,
    glob: Option<String>,
    case_sensitive: bool,
    literal: bool,
    max_matches: usize,
    cancellation_token: Option<CancellationToken>,
}

async fn search_files_with_grep<B>(backend: &B, request: GrepSearchRequest) -> Result<Value>
where
    B: ContainerBackend,
{
    let mut grep_args = vec![
        "grep".to_string(),
        "-R".to_string(),
        "-n".to_string(),
        "-H".to_string(),
    ];
    if !request.case_sensitive {
        grep_args.push("-i".to_string());
    }
    if request.literal {
        grep_args.push("-F".to_string());
    }
    grep_args.push("--".to_string());
    grep_args.push(request.query.clone());
    grep_args.push(request.path.clone());
    let grep = shell_command(&grep_args);
    let command = if let Some(glob) = &request.glob {
        format!(
            "{grep} | grep {} | head -n {}",
            shell_quote(glob),
            request.max_matches.saturating_add(1)
        )
    } else {
        format!("{grep} | head -n {}", request.max_matches.saturating_add(1))
    };
    let output = backend
        .exec(ContainerExecRequest {
            command,
            cwd: request.cwd,
            timeout_secs: Some(30),
            output_bytes_cap: None,
            cancellation_token: request.cancellation_token,
        })
        .await?;
    if output.status != 0 && output.stdout.trim().is_empty() {
        return Ok(json!({
            "query": request.query,
            "path": request.path,
            "glob": request.glob,
            "matches": [],
            "count": 0,
            "truncated": false,
        }));
    }
    let mut matches = Vec::new();
    for raw in output
        .stdout
        .lines()
        .take(request.max_matches.saturating_add(1))
    {
        if matches.len() >= request.max_matches {
            break;
        }
        if let Some((file, rest)) = raw.split_once(':')
            && let Some((line, text)) = rest.split_once(':')
        {
            matches.push(json!({
                "path": file,
                "line": line.parse::<usize>().unwrap_or(0),
                "column": 1,
                "text": text,
            }));
        }
    }
    Ok(json!({
        "query": request.query,
        "path": request.path,
        "glob": request.glob,
        "matches": matches,
        "count": matches.len(),
        "truncated": output.stdout.lines().count() > request.max_matches,
    }))
}

#[derive(Debug, Deserialize)]
struct CopyUploadInput {
    path: String,
    content_base64: String,
}

pub(super) async fn copy_upload<B>(backend: &B, arguments: Value) -> Result<Value>
where
    B: ContainerBackend,
{
    let input: CopyUploadInput = parse_input(arguments, TOOL_CONTAINER_CP_UPLOAD)?;
    let bytes = BASE64
        .decode(input.content_base64.trim().as_bytes())
        .map_err(|error| {
            tool_error(TOOL_CONTAINER_CP_UPLOAD, format!("invalid base64: {error}"))
        })?;
    backend
        .copy_to(ContainerCopyToRequest {
            path: input.path.clone(),
            content: bytes.clone(),
        })
        .await?;
    Ok(json!({ "path": input.path, "bytes": bytes.len() }))
}

#[derive(Debug, Deserialize)]
struct CopyDownloadInput {
    path: String,
}

pub(super) async fn copy_download<B>(backend: &B, arguments: Value) -> Result<Value>
where
    B: ContainerBackend,
{
    let input: CopyDownloadInput = parse_input(arguments, TOOL_CONTAINER_CP_DOWNLOAD)?;
    let bytes = backend
        .copy_from(ContainerCopyFromRequest {
            path: input.path.clone(),
            archive: true,
        })
        .await?;
    Ok(json!({
        "path": input.path,
        "tar_base64": BASE64.encode(bytes),
    }))
}

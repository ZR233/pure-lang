use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use pl_protocol::PureError;

use super::helpers::{is_skipped_dir, parse_input, path_type, text_output, workspace};
use super::input::{ListFilesInput, PathInput, ReadFileInput, SearchFilesInput, path_schema};
use super::path::{WorkspacePaths, matches_pattern};
use crate::tool::truncation::{OutputTruncation, TruncatedOutput, TruncationStrategy};
use crate::tool::{BoxFuture, Tool, ToolContext, ToolInput, ToolOutput};

#[derive(Debug, Default)]
pub struct ReadFileTool {
    truncation: TruncationStrategy,
}

#[derive(Debug)]
pub struct ListFilesTool;

#[derive(Debug)]
pub struct SearchFilesTool;

#[derive(Debug)]
pub struct StatPathTool;

impl ReadFileTool {
    pub fn new() -> Self {
        Self::default()
    }
}

/// 单次读取的最大文件字节数（对齐 codex：512MB）。
const MAX_READ_FILE_BYTES: u64 = 512 * 1024 * 1024;

impl Tool for ReadFileTool {
    fn name(&self) -> &str {
        "read_file"
    }

    fn description(&self) -> &str {
        "Read a UTF-8 text file inside the workspace. Returns the raw file contents for a \
         1-based line range (`lineOffset`, `maxLines`) without line-number prefixes."
    }

    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": { "type": "string" },
                "lineOffset": { "type": "integer", "minimum": 1 },
                "maxLines": { "type": "integer", "minimum": 1 }
            },
            "required": ["path"],
            "additionalProperties": false
        })
    }

    fn supports_parallel_tool_calls(&self) -> bool {
        true
    }

    fn execute<'a>(
        &'a self,
        input: ToolInput,
        context: ToolContext,
    ) -> BoxFuture<'a, Result<ToolOutput, PureError>> {
        Box::pin(async move {
            let input: ReadFileInput = parse_input(input.arguments, self.name())?;
            let line_offset = input.line_offset.unwrap_or(1);
            if line_offset == 0 {
                return Err(super::helpers::tool_error(
                    self.name(),
                    "lineOffset must be >= 1",
                ));
            }
            if let Some(max_lines) = input.max_lines
                && max_lines == 0
            {
                return Err(super::helpers::tool_error(
                    self.name(),
                    "maxLines must be >= 1",
                ));
            }

            let paths = workspace(&context).await?;
            paths.reject_symlink_read(&input.path).await?;
            let path = paths.resolve_existing(&input.path).await?;
            let metadata = tokio::fs::metadata(&path).await?;
            if !metadata.is_file() {
                let path = &input.path;
                return Err(super::helpers::tool_error(
                    self.name(),
                    format!("'{path}' is not a regular file"),
                ));
            }
            if metadata.len() > MAX_READ_FILE_BYTES {
                return Err(super::helpers::tool_error(
                    self.name(),
                    format!(
                        "'{}' is too large to read ({} bytes; limit is {} bytes)",
                        input.path,
                        metadata.len(),
                        MAX_READ_FILE_BYTES
                    ),
                ));
            }
            let content = tokio::fs::read_to_string(&path).await.map_err(|error| {
                super::helpers::tool_error(self.name(), format!("failed to read file: {error}"))
            })?;
            let start_byte = line_start_byte_offset(&content, line_offset)
                .map_err(|error| super::helpers::tool_error(self.name(), error))?;
            let end_byte = line_end_byte_offset(&content, start_byte, input.max_lines);
            let selected = &content[start_byte..end_byte];
            let truncated = self.truncation.truncate(selected);
            let mut description = truncated.content.clone();
            if truncated.was_truncated {
                description.push_str(
                    "\n\nOutput was truncated; pass a smaller maxLines or a larger line_offset range to continue.",
                );
            }
            Ok(ToolOutput {
                description,
                truncated: OutputTruncation {
                    stdout: truncated,
                    stderr: TruncatedOutput::empty(),
                },
                output_file: PathBuf::new(),
                exit_code: Some(0),
                timed_out: false,
                runtime_events: Vec::new(),
            })
        })
    }
}

/// 1-based 行号 → 起始字节偏移。
///
/// `line_offset == 1` 返回 0。`line_offset` 超出文件行数时报错；文件以换行结尾时，
/// `line_offset == 行数 + 1` 返回 `content.len()`（空切片），与 codex 一致。
fn line_start_byte_offset(content: &str, line_offset: usize) -> Result<usize, String> {
    if line_offset <= 1 {
        return Ok(0);
    }
    let mut current_line = 1;
    for (idx, ch) in content.char_indices() {
        if ch == '\n' {
            current_line += 1;
            if current_line == line_offset {
                return Ok(idx + 1);
            }
        }
    }
    Err(format!(
        "line_offset {line_offset} exceeds file length ({current_line} lines)"
    ))
}

/// 从 `start_byte` 起最多 `max_lines` 行的结束字节偏移。
///
/// `max_lines == None` 时返回 `content.len()`（读到文件末尾）。
fn line_end_byte_offset(content: &str, start_byte: usize, max_lines: Option<usize>) -> usize {
    let Some(max_lines) = max_lines else {
        return content.len();
    };
    let mut lines_seen = 1;
    for (relative_idx, ch) in content[start_byte..].char_indices() {
        if ch == '\n' {
            if lines_seen == max_lines {
                return start_byte + relative_idx + 1;
            }
            lines_seen += 1;
        }
    }
    content.len()
}

impl Tool for ListFilesTool {
    fn name(&self) -> &str {
        "list_files"
    }

    fn description(&self) -> &str {
        "List files and directories inside the workspace."
    }

    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": { "type": "string" },
                "depth": { "type": "integer", "minimum": 0 },
                "pattern": { "type": "string" }
            },
            "additionalProperties": false
        })
    }

    fn supports_parallel_tool_calls(&self) -> bool {
        true
    }

    fn execute<'a>(
        &'a self,
        input: ToolInput,
        context: ToolContext,
    ) -> BoxFuture<'a, Result<ToolOutput, PureError>> {
        Box::pin(async move {
            let input: ListFilesInput = parse_input(input.arguments, self.name())?;
            let paths = workspace(&context).await?;
            let root = paths
                .resolve_existing(input.path.as_deref().unwrap_or("."))
                .await?;
            let mut entries = list_entries(&paths, &root, input.depth.unwrap_or(1)).await?;
            entries.retain(|entry| matches_pattern(entry, input.pattern.as_deref()));
            entries.sort();
            Ok(text_output(entries.join("\n")))
        })
    }
}

impl Tool for SearchFilesTool {
    fn name(&self) -> &str {
        "search_files"
    }

    fn description(&self) -> &str {
        "Search UTF-8 text files inside the workspace for literal text. `pattern` is the text to \
         find; `filePattern` optionally filters file paths."
    }

    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "pattern": {
                    "type": "string",
                    "description": "Literal text to find inside UTF-8 file contents."
                },
                "path": { "type": "string" },
                "filePattern": {
                    "type": "string",
                    "description": "Optional file path filter such as `*.rs` or `src/*`."
                },
                "maxResults": { "type": "integer", "minimum": 1 }
            },
            "required": ["pattern"],
            "additionalProperties": false
        })
    }

    fn supports_parallel_tool_calls(&self) -> bool {
        true
    }

    fn execute<'a>(
        &'a self,
        input: ToolInput,
        context: ToolContext,
    ) -> BoxFuture<'a, Result<ToolOutput, PureError>> {
        Box::pin(async move {
            let input: SearchFilesInput = parse_input(input.arguments, self.name())?;
            let paths = workspace(&context).await?;
            let root = paths
                .resolve_existing(input.path.as_deref().unwrap_or("."))
                .await?;
            let max_results = input.max_results.unwrap_or(50);
            let mut results = Vec::new();
            search_files(
                &paths,
                &root,
                &input.pattern,
                input.file_pattern.as_deref(),
                max_results,
                &mut results,
            )
            .await?;
            Ok(text_output(results.join("\n")))
        })
    }
}

impl Tool for StatPathTool {
    fn name(&self) -> &str {
        "stat_path"
    }

    fn description(&self) -> &str {
        "Return metadata for a workspace path."
    }

    fn input_schema(&self) -> serde_json::Value {
        path_schema()
    }

    fn supports_parallel_tool_calls(&self) -> bool {
        true
    }

    fn execute<'a>(
        &'a self,
        input: ToolInput,
        context: ToolContext,
    ) -> BoxFuture<'a, Result<ToolOutput, PureError>> {
        Box::pin(async move {
            let input: PathInput = parse_input(input.arguments, self.name())?;
            let paths = workspace(&context).await?;
            let path = paths.resolve_existing(&input.path).await?;
            let metadata = tokio::fs::metadata(&path).await?;
            let modified_at = metadata
                .modified()
                .ok()
                .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
                .map(|duration| duration.as_secs() as i64);
            Ok(text_output(
                serde_json::json!({
                    "path": paths.display_relative(&path),
                    "type": path_type(&metadata),
                    "len": metadata.len(),
                    "readonly": metadata.permissions().readonly(),
                    "modifiedAt": modified_at,
                })
                .to_string(),
            ))
        })
    }
}

async fn list_entries(
    paths: &WorkspacePaths,
    root: &Path,
    depth: usize,
) -> Result<Vec<String>, PureError> {
    let mut output = Vec::new();
    let mut stack = vec![(root.to_path_buf(), 0usize)];
    while let Some((dir, level)) = stack.pop() {
        let metadata = tokio::fs::metadata(&dir).await?;
        if metadata.is_file() {
            output.push(paths.display_relative(&dir));
            continue;
        }
        let mut entries = tokio::fs::read_dir(&dir).await?;
        while let Some(entry) = entries.next_entry().await? {
            let path = entry.path();
            let metadata = entry.metadata().await?;
            let suffix = if metadata.is_dir() { "/" } else { "" };
            output.push(format!("{}{}", paths.display_relative(&path), suffix));
            if metadata.is_dir() && level < depth && !is_skipped_dir(&path) {
                stack.push((path, level + 1));
            }
        }
    }
    Ok(output)
}

async fn search_files(
    paths: &WorkspacePaths,
    root: &Path,
    pattern: &str,
    file_pattern: Option<&str>,
    max_results: usize,
    results: &mut Vec<String>,
) -> Result<(), PureError> {
    let mut stack = vec![root.to_path_buf()];
    while let Some(path) = stack.pop() {
        if results.len() >= max_results {
            break;
        }
        let metadata = tokio::fs::metadata(&path).await?;
        if metadata.is_dir() {
            if is_skipped_dir(&path) {
                continue;
            }
            let mut entries = tokio::fs::read_dir(&path).await?;
            while let Some(entry) = entries.next_entry().await? {
                stack.push(entry.path());
            }
            continue;
        }
        let display = paths.display_relative(&path);
        if !matches_pattern(&display, file_pattern) {
            continue;
        }
        let Ok(content) = tokio::fs::read_to_string(&path).await else {
            continue;
        };
        for (line_index, line) in content.lines().enumerate() {
            if line.contains(pattern) {
                let line_num = line_index + 1;
                results.push(format!("{display}:{line_num}: {line}"));
                if results.len() >= max_results {
                    break;
                }
            }
        }
    }
    Ok(())
}

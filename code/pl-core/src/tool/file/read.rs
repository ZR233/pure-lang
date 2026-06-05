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

impl Tool for ReadFileTool {
    fn name(&self) -> &str {
        "read_file"
    }

    fn description(&self) -> &str {
        "Read a UTF-8 text file inside the workspace. Supports optional line offset and limit."
    }

    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": { "type": "string" },
                "offset": { "type": "integer", "minimum": 0 },
                "limit": { "type": "integer", "minimum": 1 }
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
            let paths = workspace(&context).await?;
            let path = paths.resolve_existing(&input.path).await?;
            let content = tokio::fs::read_to_string(&path).await.map_err(|error| {
                super::helpers::tool_error(self.name(), format!("failed to read file: {error}"))
            })?;
            let offset = input.offset.unwrap_or(0);
            let lines: Vec<&str> = content.lines().collect();
            let selected = lines
                .iter()
                .skip(offset)
                .take(input.limit.unwrap_or(usize::MAX))
                .copied()
                .collect::<Vec<_>>()
                .join("\n");
            let truncated = self.truncation.truncate(&selected);
            let mut description = truncated.content.clone();
            if truncated.was_truncated {
                description.push_str("\n\nOutput was truncated; read a smaller range to continue.");
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
            })
        })
    }
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
        "Search UTF-8 text files inside the workspace for literal text."
    }

    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "query": { "type": "string" },
                "path": { "type": "string" },
                "pattern": { "type": "string" },
                "maxResults": { "type": "integer", "minimum": 1 }
            },
            "required": ["query"],
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
                &input.query,
                input.pattern.as_deref(),
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
    query: &str,
    pattern: Option<&str>,
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
        if !matches_pattern(&display, pattern) {
            continue;
        }
        let Ok(content) = tokio::fs::read_to_string(&path).await else {
            continue;
        };
        for (line_index, line) in content.lines().enumerate() {
            if line.contains(query) {
                results.push(format!("{}:{}: {}", display, line_index + 1, line));
                if results.len() >= max_results {
                    break;
                }
            }
        }
    }
    Ok(())
}

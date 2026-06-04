mod apply_patch;
mod path;

use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use apply_patch::{APPLY_PATCH_LARK_GRAMMAR, apply_patch};
use path::{WorkspacePaths, matches_pattern};
use pl_model::ToolSchema;
use pl_protocol::PureError;
use serde::Deserialize;
use tokio::fs::OpenOptions;

use super::truncation::{OutputTruncation, TruncatedOutput, TruncationStrategy};
use super::{Tool, ToolContext, ToolInput, ToolOutput};

#[derive(Debug, Default)]
pub struct ReadFileTool {
    truncation: TruncationStrategy,
}

#[derive(Debug)]
pub struct WriteFileTool;

#[derive(Debug)]
pub struct ListFilesTool;

#[derive(Debug)]
pub struct SearchFilesTool;

#[derive(Debug)]
pub struct StatPathTool;

#[derive(Debug)]
pub struct CreateDirectoryTool;

#[derive(Debug)]
pub struct DeletePathTool;

#[derive(Debug)]
pub struct CopyPathTool;

#[derive(Debug)]
pub struct MovePathTool;

#[derive(Debug)]
pub struct ApplyPatchTool;

const APPLY_PATCH_TOOL_DESCRIPTION: &str = "Apply a Codex-style patch to workspace files. The patch must begin with *** Begin Patch and use *** Add File:, *** Delete File:, or *** Update File: hunk headers; do not use ---/+++ unified diff, *** File:, or natural-language edit instructions. Minimal update example:\n*** Begin Patch\n*** Update File: notes.txt\n@@\n-old line\n+new line\n*** End Patch";

const APPLY_PATCH_INPUT_DESCRIPTION: &str = "Complete Codex-style patch text beginning with *** Begin Patch and ending with *** End Patch. File operations must use *** Add File:, *** Delete File:, or *** Update File:. Do not use ---/+++ unified diff, *** File:, or natural-language edit instructions. Minimal update example:\n*** Begin Patch\n*** Update File: notes.txt\n@@\n-old line\n+new line\n*** End Patch";

const APPLY_PATCH_CUSTOM_TOOL_DESCRIPTION: &str = "Use the `apply_patch` tool to edit workspace files. This is a FREEFORM tool, so do not wrap the patch in JSON. The patch must begin with *** Begin Patch, end with *** End Patch, and each file operation must use *** Add File:, *** Delete File:, or *** Update File:. Do not use ---/+++ unified diff, *** File:, or natural-language edit instructions. Minimal update example:\n*** Begin Patch\n*** Update File: notes.txt\n@@\n-old line\n+new line\n*** End Patch";

impl ReadFileTool {
    pub fn new() -> Self {
        Self::default()
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ReadFileInput {
    path: String,
    offset: Option<usize>,
    limit: Option<usize>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct WriteFileInput {
    path: String,
    content: String,
    mode: WriteMode,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "lowercase")]
enum WriteMode {
    Create,
    Overwrite,
    Append,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ListFilesInput {
    path: Option<String>,
    depth: Option<usize>,
    pattern: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SearchFilesInput {
    query: String,
    path: Option<String>,
    pattern: Option<String>,
    max_results: Option<usize>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PathInput {
    path: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DeletePathInput {
    path: String,
    recursive: Option<bool>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CopyMoveInput {
    from: String,
    to: String,
    overwrite: Option<bool>,
}

fn parse_input<T: serde::de::DeserializeOwned>(
    arguments: serde_json::Value,
    tool: &str,
) -> Result<T, PureError> {
    serde_json::from_value(arguments).map_err(|error| PureError::ToolExecutionFailed {
        tool: tool.to_string(),
        error: format!("invalid input: {error}"),
    })
}

fn text_output(description: String) -> ToolOutput {
    let stdout = TruncatedOutput {
        original_length: description.len(),
        content: description,
        was_truncated: false,
    };
    ToolOutput {
        description: stdout.content.clone(),
        truncated: OutputTruncation {
            stdout,
            stderr: TruncatedOutput {
                content: String::new(),
                was_truncated: false,
                original_length: 0,
            },
        },
        output_file: PathBuf::new(),
        exit_code: Some(0),
        timed_out: false,
    }
}

fn tool_error(tool: &str, error: impl std::fmt::Display) -> PureError {
    PureError::ToolExecutionFailed {
        tool: tool.to_string(),
        error: error.to_string(),
    }
}

async fn workspace(context: &ToolContext) -> Result<WorkspacePaths, PureError> {
    WorkspacePaths::new(context.workspace_root.clone()).await
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
    ) -> super::BoxFuture<'a, Result<ToolOutput, PureError>> {
        Box::pin(async move {
            let input: ReadFileInput = parse_input(input.arguments, self.name())?;
            let paths = workspace(&context).await?;
            let path = paths.resolve_existing(&input.path).await?;
            let content = tokio::fs::read_to_string(&path).await.map_err(|error| {
                tool_error(self.name(), format!("failed to read file: {error}"))
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
                    stderr: TruncatedOutput {
                        content: String::new(),
                        was_truncated: false,
                        original_length: 0,
                    },
                },
                output_file: PathBuf::new(),
                exit_code: Some(0),
                timed_out: false,
            })
        })
    }
}

impl Tool for WriteFileTool {
    fn name(&self) -> &str {
        "write_file"
    }

    fn description(&self) -> &str {
        "Write UTF-8 text to a workspace file using create, overwrite, or append mode."
    }

    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": { "type": "string" },
                "content": { "type": "string" },
                "mode": { "type": "string", "enum": ["create", "overwrite", "append"] }
            },
            "required": ["path", "content", "mode"],
            "additionalProperties": false
        })
    }

    fn execute<'a>(
        &'a self,
        input: ToolInput,
        context: ToolContext,
    ) -> super::BoxFuture<'a, Result<ToolOutput, PureError>> {
        Box::pin(async move {
            let _write_guard = context.workspace_write_lock().await;
            let input: WriteFileInput = parse_input(input.arguments, self.name())?;
            let paths = workspace(&context).await?;
            let path = paths.resolve_for_write(&input.path).await?;
            paths.reject_symlink_write(&path).await?;
            if let Some(parent) = path.parent() {
                tokio::fs::create_dir_all(parent).await?;
            }
            match input.mode {
                WriteMode::Create => {
                    OpenOptions::new()
                        .write(true)
                        .create_new(true)
                        .open(&path)
                        .await?
                        .write_all(input.content.as_bytes())
                        .await?;
                }
                WriteMode::Overwrite => {
                    tokio::fs::write(&path, input.content).await?;
                }
                WriteMode::Append => {
                    OpenOptions::new()
                        .create(true)
                        .append(true)
                        .open(&path)
                        .await?
                        .write_all(input.content.as_bytes())
                        .await?;
                }
            }
            Ok(text_output(format!(
                "Wrote {}",
                paths.display_relative(&path)
            )))
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
    ) -> super::BoxFuture<'a, Result<ToolOutput, PureError>> {
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
    ) -> super::BoxFuture<'a, Result<ToolOutput, PureError>> {
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
    ) -> super::BoxFuture<'a, Result<ToolOutput, PureError>> {
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

impl Tool for CreateDirectoryTool {
    fn name(&self) -> &str {
        "create_directory"
    }

    fn description(&self) -> &str {
        "Create a directory inside the workspace."
    }

    fn input_schema(&self) -> serde_json::Value {
        path_schema()
    }

    fn execute<'a>(
        &'a self,
        input: ToolInput,
        context: ToolContext,
    ) -> super::BoxFuture<'a, Result<ToolOutput, PureError>> {
        Box::pin(async move {
            let _write_guard = context.workspace_write_lock().await;
            let input: PathInput = parse_input(input.arguments, self.name())?;
            let paths = workspace(&context).await?;
            let path = paths.resolve_for_write(&input.path).await?;
            paths.reject_symlink_write(&path).await?;
            tokio::fs::create_dir_all(&path).await?;
            Ok(text_output(format!(
                "Created directory {}",
                paths.display_relative(&path)
            )))
        })
    }
}

impl Tool for DeletePathTool {
    fn name(&self) -> &str {
        "delete_path"
    }

    fn description(&self) -> &str {
        "Delete a file or, when recursive is true, a directory inside the workspace."
    }

    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": { "type": "string" },
                "recursive": { "type": "boolean" }
            },
            "required": ["path"],
            "additionalProperties": false
        })
    }

    fn execute<'a>(
        &'a self,
        input: ToolInput,
        context: ToolContext,
    ) -> super::BoxFuture<'a, Result<ToolOutput, PureError>> {
        Box::pin(async move {
            let _write_guard = context.workspace_write_lock().await;
            let input: DeletePathInput = parse_input(input.arguments, self.name())?;
            let paths = workspace(&context).await?;
            let path = paths.resolve_existing(&input.path).await?;
            paths.reject_symlink_write(&path).await?;
            let metadata = tokio::fs::metadata(&path).await?;
            if metadata.is_dir() {
                if input.recursive.unwrap_or(false) {
                    tokio::fs::remove_dir_all(&path).await?;
                } else {
                    return Err(tool_error(
                        self.name(),
                        "directory delete requires recursive true",
                    ));
                }
            } else {
                tokio::fs::remove_file(&path).await?;
            }
            Ok(text_output(format!(
                "Deleted {}",
                paths.display_relative(&path)
            )))
        })
    }
}

impl Tool for CopyPathTool {
    fn name(&self) -> &str {
        "copy_path"
    }

    fn description(&self) -> &str {
        "Copy a file inside the workspace."
    }

    fn input_schema(&self) -> serde_json::Value {
        copy_move_schema()
    }

    fn execute<'a>(
        &'a self,
        input: ToolInput,
        context: ToolContext,
    ) -> super::BoxFuture<'a, Result<ToolOutput, PureError>> {
        Box::pin(async move {
            let _write_guard = context.workspace_write_lock().await;
            let input: CopyMoveInput = parse_input(input.arguments, self.name())?;
            let paths = workspace(&context).await?;
            let from = paths.resolve_existing(&input.from).await?;
            let to = paths.resolve_for_write(&input.to).await?;
            paths.reject_symlink_write(&to).await?;
            ensure_overwrite(&to, input.overwrite.unwrap_or(false), self.name()).await?;
            if let Some(parent) = to.parent() {
                tokio::fs::create_dir_all(parent).await?;
            }
            tokio::fs::copy(&from, &to).await?;
            Ok(text_output(format!(
                "Copied {} to {}",
                paths.display_relative(&from),
                paths.display_relative(&to)
            )))
        })
    }
}

impl Tool for MovePathTool {
    fn name(&self) -> &str {
        "move_path"
    }

    fn description(&self) -> &str {
        "Move or rename a file or directory inside the workspace."
    }

    fn input_schema(&self) -> serde_json::Value {
        copy_move_schema()
    }

    fn execute<'a>(
        &'a self,
        input: ToolInput,
        context: ToolContext,
    ) -> super::BoxFuture<'a, Result<ToolOutput, PureError>> {
        Box::pin(async move {
            let _write_guard = context.workspace_write_lock().await;
            let input: CopyMoveInput = parse_input(input.arguments, self.name())?;
            let paths = workspace(&context).await?;
            let from = paths.resolve_existing(&input.from).await?;
            let to = paths.resolve_for_write(&input.to).await?;
            paths.reject_symlink_write(&from).await?;
            paths.reject_symlink_write(&to).await?;
            ensure_overwrite(&to, input.overwrite.unwrap_or(false), self.name()).await?;
            if let Some(parent) = to.parent() {
                tokio::fs::create_dir_all(parent).await?;
            }
            tokio::fs::rename(&from, &to).await?;
            Ok(text_output(format!(
                "Moved {} to {}",
                paths.display_relative(&from),
                paths.display_relative(&to)
            )))
        })
    }
}

impl Tool for ApplyPatchTool {
    fn name(&self) -> &str {
        "apply_patch"
    }

    fn description(&self) -> &str {
        APPLY_PATCH_TOOL_DESCRIPTION
    }

    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "patch": {
                    "type": "string",
                    "description": APPLY_PATCH_INPUT_DESCRIPTION
                }
            },
            "required": ["patch"],
            "additionalProperties": false
        })
    }

    fn to_schema(&self) -> ToolSchema {
        ToolSchema::custom_grammar(
            self.name(),
            APPLY_PATCH_CUSTOM_TOOL_DESCRIPTION,
            "lark",
            APPLY_PATCH_LARK_GRAMMAR,
        )
    }

    fn execute<'a>(
        &'a self,
        input: ToolInput,
        context: ToolContext,
    ) -> super::BoxFuture<'a, Result<ToolOutput, PureError>> {
        Box::pin(async move {
            let _write_guard = context.workspace_write_lock().await;
            let patch = extract_patch(input.arguments)?;
            let paths = workspace(&context).await?;
            let outcome = apply_patch(&patch, &paths).await?;
            let summary = outcome.summary(&paths);
            Ok(text_output(summary))
        })
    }
}

fn extract_patch(arguments: serde_json::Value) -> Result<String, PureError> {
    match arguments {
        serde_json::Value::String(patch) => Ok(patch),
        serde_json::Value::Object(mut object) => object
            .remove("patch")
            .or_else(|| object.remove("input"))
            .and_then(|value| value.as_str().map(ToOwned::to_owned))
            .ok_or_else(|| tool_error("apply_patch", "missing patch input")),
        _ => Err(tool_error("apply_patch", "invalid apply_patch input")),
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

async fn ensure_overwrite(path: &Path, overwrite: bool, tool: &str) -> Result<(), PureError> {
    if !overwrite && tokio::fs::try_exists(path).await? {
        return Err(tool_error(
            tool,
            format!("target '{}' already exists", path.display()),
        ));
    }
    Ok(())
}

fn path_schema() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "path": { "type": "string" }
        },
        "required": ["path"],
        "additionalProperties": false
    })
}

fn copy_move_schema() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "from": { "type": "string" },
            "to": { "type": "string" },
            "overwrite": { "type": "boolean" }
        },
        "required": ["from", "to"],
        "additionalProperties": false
    })
}

fn path_type(metadata: &std::fs::Metadata) -> &'static str {
    if metadata.is_dir() {
        "directory"
    } else if metadata.is_file() {
        "file"
    } else {
        "other"
    }
}

fn is_skipped_dir(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| matches!(name, ".git" | "target" | "node_modules"))
}

use tokio::io::AsyncWriteExt;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::turn::TurnOptions;
    use pretty_assertions::assert_eq;

    fn unique_temp_dir(name: &str) -> PathBuf {
        let id = std::time::SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("pure-lang-{name}-{id}"))
    }

    async fn context(root: &Path) -> ToolContext {
        tokio::fs::create_dir_all(root).await.unwrap();
        let (event_tx, _event_rx) = tokio::sync::broadcast::channel(8);
        ToolContext {
            event_tx,
            options: TurnOptions::default(),
            mode: crate::turn::CompileMode::Auto,
            workspace_root: root.to_path_buf(),
            workspace_instructions: None,
            active_subagent: None,
            agent_control: crate::AgentControl::default(),
        }
    }

    fn input(arguments: serde_json::Value) -> ToolInput {
        ToolInput {
            arguments,
            session_id: "session".to_string(),
            tool_id: "tool".to_string(),
        }
    }

    #[tokio::test]
    async fn read_file_rejects_workspace_escape() {
        let root = unique_temp_dir("escape");
        let tool = ReadFileTool::new();
        let result = tool
            .execute(
                input(serde_json::json!({ "path": "../outside.txt" })),
                context(&root).await,
            )
            .await;

        assert!(result.is_err());
        let _ = tokio::fs::remove_dir_all(root).await;
    }

    #[tokio::test]
    async fn write_and_read_file_roundtrip() {
        let root = unique_temp_dir("roundtrip");
        let write = WriteFileTool;
        let read = ReadFileTool::new();
        write
            .execute(
                input(serde_json::json!({
                    "path": "notes/a.txt",
                    "content": "hello\nworld\n",
                    "mode": "create"
                })),
                context(&root).await,
            )
            .await
            .unwrap();

        let output = read
            .execute(
                input(serde_json::json!({ "path": "notes/a.txt", "offset": 1, "limit": 1 })),
                context(&root).await,
            )
            .await
            .unwrap();

        assert_eq!(output.description, "world");
        let _ = tokio::fs::remove_dir_all(root).await;
    }

    #[tokio::test]
    async fn write_file_waits_for_workspace_write_lock() {
        let root = unique_temp_dir("write-lock-tool");
        let context = context(&root).await;
        let guard = context.workspace_write_lock().await;
        let tool = WriteFileTool;
        let write_context = context.clone();
        let write_task = tokio::spawn(async move {
            tool.execute(
                input(serde_json::json!({
                    "path": "locked.txt",
                    "content": "after\n",
                    "mode": "create"
                })),
                write_context,
            )
            .await
        });
        tokio::task::yield_now().await;

        assert!(!write_task.is_finished());
        drop(guard);
        write_task.await.unwrap().unwrap();
        assert_eq!(
            tokio::fs::read_to_string(root.join("locked.txt"))
                .await
                .unwrap(),
            "after\n"
        );
        let _ = tokio::fs::remove_dir_all(root).await;
    }

    #[tokio::test]
    async fn apply_patch_adds_file() {
        let root = unique_temp_dir("patch-add");
        let tool = ApplyPatchTool;
        let patch = "*** Begin Patch\n*** Add File: src/lib.rs\n+pub fn ok() {}\n*** End Patch";

        let output = tool
            .execute(
                input(serde_json::json!({ "patch": patch })),
                context(&root).await,
            )
            .await
            .unwrap();

        assert!(
            output.description.contains("A src\\lib.rs")
                || output.description.contains("A src/lib.rs")
        );
        assert_eq!(
            tokio::fs::read_to_string(root.join("src/lib.rs"))
                .await
                .unwrap(),
            "pub fn ok() {}\n"
        );
        let _ = tokio::fs::remove_dir_all(root).await;
    }

    #[tokio::test]
    async fn apply_patch_context_mismatch_does_not_write() {
        let root = unique_temp_dir("patch-mismatch");
        tokio::fs::create_dir_all(root.join("src")).await.unwrap();
        tokio::fs::write(root.join("src/lib.rs"), "old\n")
            .await
            .unwrap();
        let tool = ApplyPatchTool;
        let patch =
            "*** Begin Patch\n*** Update File: src/lib.rs\n@@\n-missing\n+new\n*** End Patch";

        let result = tool
            .execute(
                input(serde_json::json!({ "patch": patch })),
                context(&root).await,
            )
            .await;

        assert!(result.is_err());
        assert_eq!(
            tokio::fs::read_to_string(root.join("src/lib.rs"))
                .await
                .unwrap(),
            "old\n"
        );
        let _ = tokio::fs::remove_dir_all(root).await;
    }

    #[tokio::test]
    async fn apply_patch_accepts_wrapped_single_patch_block() {
        let root = unique_temp_dir("patch-wrapper");
        let tool = ApplyPatchTool;
        let patch = "Here is the patch:\n```patch\n*** Begin Patch\n*** Add File: wrapped.txt\n+ok\n*** End Patch\n```";

        tool.execute(
            input(serde_json::json!({ "patch": patch })),
            context(&root).await,
        )
        .await
        .unwrap();

        assert_eq!(
            tokio::fs::read_to_string(root.join("wrapped.txt"))
                .await
                .unwrap(),
            "ok\n"
        );
        let _ = tokio::fs::remove_dir_all(root).await;
    }

    #[tokio::test]
    async fn apply_patch_accepts_heredoc_wrappers() {
        let root = unique_temp_dir("patch-heredoc-wrapper");
        let tool = ApplyPatchTool;
        for (index, start) in ["<<EOF", "<<'EOF'", "<<\"EOF\""].into_iter().enumerate() {
            let path = format!("wrapped-{index}.txt");
            let patch =
                format!("{start}\n*** Begin Patch\n*** Add File: {path}\n+ok\n*** End Patch\nEOF");

            tool.execute(
                input(serde_json::json!({ "patch": patch })),
                context(&root).await,
            )
            .await
            .unwrap();

            assert_eq!(
                tokio::fs::read_to_string(root.join(path)).await.unwrap(),
                "ok\n"
            );
        }
        let _ = tokio::fs::remove_dir_all(root).await;
    }

    #[tokio::test]
    async fn apply_patch_rejects_mismatched_heredoc_wrapper() {
        let root = unique_temp_dir("patch-mismatched-heredoc");
        let tool = ApplyPatchTool;
        let result = tool
            .execute(
                input(serde_json::json!({
                    "patch": "<<\"EOF'\n*** Begin Patch\n*** Add File: bad.txt\n+nope\n*** End Patch\nEOF"
                })),
                context(&root).await,
            )
            .await
            .unwrap_err();

        assert!(result.to_string().contains("first line must be"));
        assert!(!tokio::fs::try_exists(root.join("bad.txt")).await.unwrap());
        let _ = tokio::fs::remove_dir_all(root).await;
    }

    #[tokio::test]
    async fn apply_patch_accepts_environment_id_preamble() {
        let root = unique_temp_dir("patch-environment-id");
        let tool = ApplyPatchTool;
        let patch = "*** Begin Patch\n*** Environment ID: remote\n*** Add File: env.txt\n+ok\n*** End Patch";

        tool.execute(
            input(serde_json::json!({ "patch": patch })),
            context(&root).await,
        )
        .await
        .unwrap();

        assert_eq!(
            tokio::fs::read_to_string(root.join("env.txt"))
                .await
                .unwrap(),
            "ok\n"
        );
        let _ = tokio::fs::remove_dir_all(root).await;
    }

    #[tokio::test]
    async fn apply_patch_rejects_empty_environment_id_preamble() {
        let root = unique_temp_dir("patch-empty-environment-id");
        let tool = ApplyPatchTool;
        let result = tool
            .execute(
                input(serde_json::json!({
                    "patch": "*** Begin Patch\n*** Environment ID:   \n*** Add File: env.txt\n+ok\n*** End Patch"
                })),
                context(&root).await,
            )
            .await
            .unwrap_err();

        assert!(
            result
                .to_string()
                .contains("environment_id cannot be empty")
        );
        assert!(!tokio::fs::try_exists(root.join("env.txt")).await.unwrap());
        let _ = tokio::fs::remove_dir_all(root).await;
    }

    #[tokio::test]
    async fn apply_patch_adds_empty_file() {
        let root = unique_temp_dir("patch-empty-add");
        let tool = ApplyPatchTool;
        let patch = "*** Begin Patch\n*** Add File: empty.txt\n*** End Patch";

        tool.execute(
            input(serde_json::json!({ "patch": patch })),
            context(&root).await,
        )
        .await
        .unwrap();

        assert_eq!(
            tokio::fs::read_to_string(root.join("empty.txt"))
                .await
                .unwrap(),
            ""
        );
        let _ = tokio::fs::remove_dir_all(root).await;
    }

    #[tokio::test]
    async fn apply_patch_accepts_whitespace_padded_markers() {
        let root = unique_temp_dir("patch-padded-markers");
        tokio::fs::create_dir_all(&root).await.unwrap();
        tokio::fs::write(root.join("file.txt"), "one\n")
            .await
            .unwrap();
        let tool = ApplyPatchTool;
        let patch =
            " *** Begin Patch\n  *** Update File: file.txt\n@@\n-one\n+two\n *** End Patch ";

        tool.execute(
            input(serde_json::json!({ "patch": patch })),
            context(&root).await,
        )
        .await
        .unwrap();

        assert_eq!(
            tokio::fs::read_to_string(root.join("file.txt"))
                .await
                .unwrap(),
            "two\n"
        );
        let _ = tokio::fs::remove_dir_all(root).await;
    }

    #[tokio::test]
    async fn apply_patch_matches_unicode_punctuation_context() {
        let root = unique_temp_dir("patch-unicode-context");
        tokio::fs::create_dir_all(&root).await.unwrap();
        tokio::fs::write(
            root.join("unicode.txt"),
            "import asyncio  # local import \u{2013} avoids top\u{2011}level dep\nlet quote = \u{201C}ok\u{201D}\nspace = \"a\u{00A0}b\"\n",
        )
        .await
        .unwrap();
        let tool = ApplyPatchTool;
        let patch = "*** Begin Patch\n*** Update File: unicode.txt\n@@\n-import asyncio  # local import - avoids top-level dep\n-let quote = \"ok\"\n-space = \"a b\"\n+done\n*** End Patch";

        tool.execute(
            input(serde_json::json!({ "patch": patch })),
            context(&root).await,
        )
        .await
        .unwrap();

        assert_eq!(
            tokio::fs::read_to_string(root.join("unicode.txt"))
                .await
                .unwrap(),
            "done\n"
        );
        let _ = tokio::fs::remove_dir_all(root).await;
    }

    #[tokio::test]
    async fn apply_patch_skips_blank_lines_between_update_chunks() {
        let root = unique_temp_dir("patch-blank-between-chunks");
        tokio::fs::create_dir_all(&root).await.unwrap();
        tokio::fs::write(root.join("file.txt"), "one\n")
            .await
            .unwrap();
        let tool = ApplyPatchTool;
        let patch = "*** Begin Patch\n*** Update File: file.txt\n\n@@\n-one\n+two\n*** End Patch";

        tool.execute(
            input(serde_json::json!({ "patch": patch })),
            context(&root).await,
        )
        .await
        .unwrap();

        assert_eq!(
            tokio::fs::read_to_string(root.join("file.txt"))
                .await
                .unwrap(),
            "two\n"
        );
        let _ = tokio::fs::remove_dir_all(root).await;
    }

    #[tokio::test]
    async fn apply_patch_supports_deletion_only_update_and_eof_marker() {
        let root = unique_temp_dir("patch-delete-and-eof");
        tokio::fs::create_dir_all(&root).await.unwrap();
        tokio::fs::write(root.join("lines.txt"), "line1\nline2\nline3\n")
            .await
            .unwrap();
        tokio::fs::write(root.join("tail.txt"), "first\nsecond\n")
            .await
            .unwrap();
        let tool = ApplyPatchTool;
        let patch = "*** Begin Patch\n*** Update File: lines.txt\n@@\n line1\n-line2\n line3\n*** Update File: tail.txt\n@@\n first\n-second\n+second updated\n\n*** End of File\n*** End Patch";

        tool.execute(
            input(serde_json::json!({ "patch": patch })),
            context(&root).await,
        )
        .await
        .unwrap();

        assert_eq!(
            tokio::fs::read_to_string(root.join("lines.txt"))
                .await
                .unwrap(),
            "line1\nline3\n"
        );
        assert_eq!(
            tokio::fs::read_to_string(root.join("tail.txt"))
                .await
                .unwrap(),
            "first\nsecond updated\n"
        );
        let _ = tokio::fs::remove_dir_all(root).await;
    }

    #[tokio::test]
    async fn apply_patch_rejects_missing_end_marker() {
        let root = unique_temp_dir("patch-missing-end");
        let tool = ApplyPatchTool;
        let result = tool
            .execute(
                input(serde_json::json!({
                    "patch": "*** Begin Patch\n*** Add File: missing.txt\n+nope"
                })),
                context(&root).await,
            )
            .await
            .unwrap_err();

        assert!(result.to_string().contains("last line must be"));
        let _ = tokio::fs::remove_dir_all(root).await;
    }

    #[tokio::test]
    async fn apply_patch_rejects_unified_diff_header() {
        let root = unique_temp_dir("patch-unified");
        let tool = ApplyPatchTool;
        let result = tool
            .execute(
                input(serde_json::json!({
                    "patch": "*** Begin Patch\n--- a/file.txt\n+++ b/file.txt\n@@ -1 +1 @@\n-old\n+new\n*** End Patch"
                })),
                context(&root).await,
            )
            .await
            .unwrap_err();

        let error = result.to_string();
        assert!(error.contains("unified diff"));
        assert!(error.contains("*** Update File:"));
        let _ = tokio::fs::remove_dir_all(root).await;
    }

    #[tokio::test]
    async fn apply_patch_rejects_file_metadata_header() {
        let root = unique_temp_dir("patch-file-header");
        let tool = ApplyPatchTool;
        let result = tool
            .execute(
                input(serde_json::json!({
                    "patch": "*** Begin Patch\n*** File: src/lib.rs\n*** End Patch"
                })),
                context(&root).await,
            )
            .await
            .unwrap_err();

        let error = result.to_string();
        assert!(error.contains("*** File:"));
        assert!(error.contains("*** Update File:"));
        let _ = tokio::fs::remove_dir_all(root).await;
    }

    #[tokio::test]
    async fn apply_patch_move_only_update_moves_file() {
        let root = unique_temp_dir("patch-move-only");
        tokio::fs::create_dir_all(root.join("old")).await.unwrap();
        tokio::fs::write(root.join("old/name.txt"), "same\n")
            .await
            .unwrap();
        let tool = ApplyPatchTool;
        let patch = "*** Begin Patch\n*** Update File: old/name.txt\n*** Move to: new/name.txt\n*** End Patch";

        tool.execute(
            input(serde_json::json!({ "patch": patch })),
            context(&root).await,
        )
        .await
        .unwrap();

        assert!(
            !tokio::fs::try_exists(root.join("old/name.txt"))
                .await
                .unwrap()
        );
        assert_eq!(
            tokio::fs::read_to_string(root.join("new/name.txt"))
                .await
                .unwrap(),
            "same\n"
        );
        let _ = tokio::fs::remove_dir_all(root).await;
    }

    #[tokio::test]
    async fn apply_patch_appends_pure_addition_chunk_to_eof() {
        let root = unique_temp_dir("patch-pure-addition-eof");
        tokio::fs::create_dir_all(&root).await.unwrap();
        tokio::fs::write(
            root.join("page.html"),
            "<head>\n<title>x</title>\n</head>\n",
        )
        .await
        .unwrap();
        let tool = ApplyPatchTool;
        let patch = "*** Begin Patch\n*** Update File: page.html\n@@ <head>\n+<script></script>\n*** End Patch";

        tool.execute(
            input(serde_json::json!({ "patch": patch })),
            context(&root).await,
        )
        .await
        .unwrap();

        assert_eq!(
            tokio::fs::read_to_string(root.join("page.html"))
                .await
                .unwrap(),
            "<head>\n<title>x</title>\n</head>\n<script></script>\n"
        );
        let _ = tokio::fs::remove_dir_all(root).await;
    }

    #[tokio::test]
    async fn apply_patch_accepts_indented_context_without_extra_control_space() {
        let root = unique_temp_dir("patch-indented-context");
        tokio::fs::create_dir_all(&root).await.unwrap();
        tokio::fs::write(root.join("style.html"), "<style>\n    </style>\n")
            .await
            .unwrap();
        let tool = ApplyPatchTool;
        let patch =
            "*** Begin Patch\n*** Update File: style.html\n@@\n   </style>\n+tail\n*** End Patch";

        tool.execute(
            input(serde_json::json!({ "patch": patch })),
            context(&root).await,
        )
        .await
        .unwrap();

        assert_eq!(
            tokio::fs::read_to_string(root.join("style.html"))
                .await
                .unwrap(),
            "<style>\n    </style>\ntail\n"
        );
        let _ = tokio::fs::remove_dir_all(root).await;
    }

    #[tokio::test]
    async fn apply_patch_accepts_unprefixed_zero_indent_context_line() {
        let root = unique_temp_dir("patch-unprefixed-zero-indent-context");
        tokio::fs::create_dir_all(&root).await.unwrap();
        tokio::fs::write(root.join("page.html"), "<body>\nold\n</body>\n")
            .await
            .unwrap();
        let tool = ApplyPatchTool;
        let patch =
            "*** Begin Patch\n*** Update File: page.html\n@@\n-old\n+new\n</body>\n*** End Patch";

        tool.execute(
            input(serde_json::json!({ "patch": patch })),
            context(&root).await,
        )
        .await
        .unwrap();

        assert_eq!(
            tokio::fs::read_to_string(root.join("page.html"))
                .await
                .unwrap(),
            "<body>\nnew\n</body>\n"
        );
        let _ = tokio::fs::remove_dir_all(root).await;
    }

    #[tokio::test]
    async fn apply_patch_collapses_duplicated_edge_context_for_insert_before() {
        let root = unique_temp_dir("patch-duplicated-edge-context");
        tokio::fs::create_dir_all(&root).await.unwrap();
        tokio::fs::write(root.join("deepseek-intro.html"), "<style>\n    </style>\n")
            .await
            .unwrap();
        let tool = ApplyPatchTool;
        let patch = "*** Begin Patch\n*** Update File: deepseek-intro.html\n@@\n    </style>\n+        .cube { display: block; }\n     </style>\n*** End Patch";

        tool.execute(
            input(serde_json::json!({ "patch": patch })),
            context(&root).await,
        )
        .await
        .unwrap();

        assert_eq!(
            tokio::fs::read_to_string(root.join("deepseek-intro.html"))
                .await
                .unwrap(),
            "<style>\n        .cube { display: block; }\n    </style>\n"
        );
        let _ = tokio::fs::remove_dir_all(root).await;
    }

    #[tokio::test]
    async fn apply_patch_applies_repeated_update_hunks_in_order() {
        let root = unique_temp_dir("patch-repeated-update-target");
        tokio::fs::create_dir_all(&root).await.unwrap();
        tokio::fs::write(root.join("src.rs"), "one\ntwo\nthree\n")
            .await
            .unwrap();
        let tool = ApplyPatchTool;
        let patch = "*** Begin Patch\n*** Update File: src.rs\n@@\n-one\n+first\n*** Update File: src.rs\n@@\n-first\n+second\n*** End Patch";

        tool.execute(
            input(serde_json::json!({ "patch": patch })),
            context(&root).await,
        )
        .await
        .unwrap();

        assert_eq!(
            tokio::fs::read_to_string(root.join("src.rs"))
                .await
                .unwrap(),
            "second\ntwo\nthree\n"
        );
        let _ = tokio::fs::remove_dir_all(root).await;
    }

    #[tokio::test]
    async fn apply_patch_add_overwrites_existing_file() {
        let root = unique_temp_dir("patch-add-overwrite");
        tokio::fs::create_dir_all(&root).await.unwrap();
        tokio::fs::write(root.join("duplicate.txt"), "old\n")
            .await
            .unwrap();
        let tool = ApplyPatchTool;
        let patch = "*** Begin Patch\n*** Add File: duplicate.txt\n+new\n*** End Patch";

        let output = tool
            .execute(
                input(serde_json::json!({ "patch": patch })),
                context(&root).await,
            )
            .await
            .unwrap();

        assert!(output.description.contains("A duplicate.txt"));
        assert_eq!(
            tokio::fs::read_to_string(root.join("duplicate.txt"))
                .await
                .unwrap(),
            "new\n"
        );
        let _ = tokio::fs::remove_dir_all(root).await;
    }

    #[tokio::test]
    async fn apply_patch_move_overwrites_existing_target() {
        let root = unique_temp_dir("patch-move-overwrite");
        tokio::fs::create_dir_all(root.join("old")).await.unwrap();
        tokio::fs::create_dir_all(root.join("new")).await.unwrap();
        tokio::fs::write(root.join("old/name.txt"), "from\n")
            .await
            .unwrap();
        tokio::fs::write(root.join("new/name.txt"), "existing\n")
            .await
            .unwrap();
        let tool = ApplyPatchTool;
        let patch = "*** Begin Patch\n*** Update File: old/name.txt\n*** Move to: new/name.txt\n@@\n-from\n+to\n*** End Patch";

        tool.execute(
            input(serde_json::json!({ "patch": patch })),
            context(&root).await,
        )
        .await
        .unwrap();

        assert!(
            !tokio::fs::try_exists(root.join("old/name.txt"))
                .await
                .unwrap()
        );
        assert_eq!(
            tokio::fs::read_to_string(root.join("new/name.txt"))
                .await
                .unwrap(),
            "to\n"
        );
        let _ = tokio::fs::remove_dir_all(root).await;
    }

    #[tokio::test]
    async fn apply_patch_failure_keeps_committed_prefix() {
        let root = unique_temp_dir("patch-prefix-failure");
        tokio::fs::create_dir_all(&root).await.unwrap();
        let tool = ApplyPatchTool;
        let patch = "*** Begin Patch\n*** Add File: created.txt\n+hello\n*** Update File: missing.txt\n@@\n-old\n+new\n*** End Patch";

        let result = tool
            .execute(
                input(serde_json::json!({ "patch": patch })),
                context(&root).await,
            )
            .await
            .unwrap_err();

        let error = result.to_string();
        assert!(error.contains("failed to resolve path 'missing.txt'"));
        assert!(error.contains("Committed changes before failure"));
        assert!(error.contains("A created.txt"));
        assert_eq!(
            tokio::fs::read_to_string(root.join("created.txt"))
                .await
                .unwrap(),
            "hello\n"
        );
        let _ = tokio::fs::remove_dir_all(root).await;
    }

    #[tokio::test]
    async fn apply_patch_applies_add_then_update_in_order() {
        let root = unique_temp_dir("patch-add-then-update");
        tokio::fs::create_dir_all(&root).await.unwrap();
        tokio::fs::write(root.join("notes.txt"), "old\n")
            .await
            .unwrap();
        let tool = ApplyPatchTool;
        let patch = "*** Begin Patch\n*** Add File: notes.txt\n+new\n*** Update File: notes.txt\n@@\n-new\n+newer\n*** End Patch";

        let output = tool
            .execute(
                input(serde_json::json!({ "patch": patch })),
                context(&root).await,
            )
            .await
            .unwrap();

        assert!(output.description.contains("A notes.txt"));
        assert!(output.description.contains("M notes.txt"));
        assert_eq!(
            tokio::fs::read_to_string(root.join("notes.txt"))
                .await
                .unwrap(),
            "newer\n"
        );
        let _ = tokio::fs::remove_dir_all(root).await;
    }
}

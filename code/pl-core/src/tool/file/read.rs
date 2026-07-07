use std::time::UNIX_EPOCH;

use pl_protocol::PureError;

use super::helpers::{parse_input, path_type, text_output, workspace};
use super::input::{PathInput, path_schema};
use crate::tool::{BoxFuture, Tool, ToolContext, ToolInput, ToolOutput};
use crate::tool::{LocalWorkspaceFileBackend, WorkspaceFileToolKind, execute_workspace_file_tool};

#[derive(Debug, Default)]
pub struct ReadFileTool;

#[derive(Debug)]
pub struct ListFilesTool;

#[derive(Debug)]
pub struct SearchFilesTool;

#[derive(Debug)]
pub struct StatPathTool;

impl ReadFileTool {
    pub fn new() -> Self {
        Self
    }
}

impl Tool for ReadFileTool {
    fn name(&self) -> &str {
        WorkspaceFileToolKind::ReadFile.name()
    }

    fn description(&self) -> &str {
        WorkspaceFileToolKind::ReadFile.description()
    }

    fn input_schema(&self) -> serde_json::Value {
        WorkspaceFileToolKind::ReadFile.input_schema()
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
            execute_local_file_tool(WorkspaceFileToolKind::ReadFile, input, context).await
        })
    }
}

impl Tool for ListFilesTool {
    fn name(&self) -> &str {
        WorkspaceFileToolKind::ListFiles.name()
    }

    fn description(&self) -> &str {
        WorkspaceFileToolKind::ListFiles.description()
    }

    fn input_schema(&self) -> serde_json::Value {
        WorkspaceFileToolKind::ListFiles.input_schema()
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
            execute_local_file_tool(WorkspaceFileToolKind::ListFiles, input, context).await
        })
    }
}

impl Tool for SearchFilesTool {
    fn name(&self) -> &str {
        WorkspaceFileToolKind::SearchFiles.name()
    }

    fn description(&self) -> &str {
        WorkspaceFileToolKind::SearchFiles.description()
    }

    fn input_schema(&self) -> serde_json::Value {
        WorkspaceFileToolKind::SearchFiles.input_schema()
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
            execute_local_file_tool(WorkspaceFileToolKind::SearchFiles, input, context).await
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

async fn execute_local_file_tool(
    kind: WorkspaceFileToolKind,
    input: ToolInput,
    context: ToolContext,
) -> Result<ToolOutput, PureError> {
    let backend = LocalWorkspaceFileBackend::from_context(&context).await?;
    let execution = execute_workspace_file_tool(
        &backend,
        kind.name(),
        input.arguments,
        context.options.cancellation_token.clone(),
    )
    .await?
    .ok_or_else(|| super::helpers::tool_error(kind.name(), "unknown workspace file tool"))?;
    Ok(ToolOutput {
        description: execution.model_output,
        truncated: execution.truncated,
        output_file: std::path::PathBuf::new(),
        exit_code: execution.exit_code,
        timed_out: false,
        runtime_events: Vec::new(),
    })
}

pub(crate) mod apply_patch;
mod helpers;
mod input;
pub(crate) mod path;
mod read;
mod write;

#[cfg(test)]
mod tests;

use crate::tool::{
    BoxFuture, LocalWorkspaceFileBackend, Tool, ToolContext, ToolInput, ToolOutput,
    WorkspaceFileToolKind, execute_workspace_file_tool,
};

pub use read::{ListFilesTool, ReadFileTool, SearchFilesTool, StatPathTool};
pub use write::{CopyPathTool, CreateDirectoryTool, DeletePathTool, MovePathTool, WriteFileTool};

#[derive(Debug)]
pub struct ApplyPatchTool;

impl Tool for ApplyPatchTool {
    fn name(&self) -> &str {
        WorkspaceFileToolKind::ApplyPatch.name()
    }

    fn description(&self) -> &str {
        WorkspaceFileToolKind::ApplyPatch.description()
    }

    fn input_schema(&self) -> serde_json::Value {
        WorkspaceFileToolKind::ApplyPatch.input_schema()
    }

    fn execute<'a>(
        &'a self,
        input: ToolInput,
        context: ToolContext,
    ) -> BoxFuture<'a, Result<ToolOutput, pl_protocol::PureError>> {
        Box::pin(async move {
            let _write_guard = context.workspace_write_lock().await;
            let backend = LocalWorkspaceFileBackend::from_context(&context).await?;
            let execution = execute_workspace_file_tool(
                &backend,
                self.name(),
                input.arguments,
                context.options.cancellation_token.clone(),
            )
            .await?
            .ok_or_else(|| helpers::tool_error(self.name(), "unknown workspace file tool"))?;
            Ok(ToolOutput {
                description: execution.model_output,
                truncated: execution.truncated,
                output_file: std::path::PathBuf::new(),
                exit_code: execution.exit_code,
                timed_out: false,
                runtime_events: Vec::new(),
            })
        })
    }
}

use pl_protocol::PureError;
use tokio::fs::OpenOptions;
use tokio::io::AsyncWriteExt;

use super::helpers::*;
use super::input::*;
use crate::path_safety::remove_dir_all_no_follow_async;
use crate::tool::{
    BoxFuture, FunctionToolDefinition, Tool, ToolContext, ToolInput, ToolOutput,
    deserialize_tool_input,
};

#[derive(Debug)]
pub struct WriteFileTool;

#[derive(Debug)]
pub struct CreateDirectoryTool;

#[derive(Debug)]
pub struct DeletePathTool;

#[derive(Debug)]
pub struct CopyPathTool;

#[derive(Debug)]
pub struct MovePathTool;

impl Tool for WriteFileTool {
    fn name(&self) -> &str {
        "write_file"
    }

    fn description(&self) -> &str {
        "Write UTF-8 text to a workspace file using create, overwrite, or append mode."
    }

    fn input_schema(&self) -> serde_json::Value {
        FunctionToolDefinition::<WriteFileInput>::new(self.name(), self.description())
            .input_schema()
    }

    fn execute<'a>(
        &'a self,
        input: ToolInput,
        context: ToolContext,
    ) -> BoxFuture<'a, Result<ToolOutput, PureError>> {
        Box::pin(async move {
            context.ensure_workspace_writable()?;
            let _write_guard = context.workspace_write_lock().await;
            let input: WriteFileInput = deserialize_tool_input(self.name(), input.arguments)?;
            let paths = workspace(&context).await?;
            let path = paths.resolve_for_write(&input.path).await?;
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
            sync_lsp_changed(&context, &path).await;
            Ok(text_output(format!(
                "Wrote {}",
                paths.display_relative(&path)
            )))
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
        FunctionToolDefinition::<PathInput>::new(self.name(), self.description()).input_schema()
    }

    fn execute<'a>(
        &'a self,
        input: ToolInput,
        context: ToolContext,
    ) -> BoxFuture<'a, Result<ToolOutput, PureError>> {
        Box::pin(async move {
            context.ensure_workspace_writable()?;
            let _write_guard = context.workspace_write_lock().await;
            let input: PathInput = deserialize_tool_input(self.name(), input.arguments)?;
            let paths = workspace(&context).await?;
            let path = paths.resolve_for_write(&input.path).await?;
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
        "Delete a workspace file, empty directory, or recursive directory using an explicit mode."
    }

    fn input_schema(&self) -> serde_json::Value {
        FunctionToolDefinition::<DeletePathInput>::new(self.name(), self.description())
            .input_schema()
    }

    fn execute<'a>(
        &'a self,
        input: ToolInput,
        context: ToolContext,
    ) -> BoxFuture<'a, Result<ToolOutput, PureError>> {
        Box::pin(async move {
            context.ensure_workspace_writable()?;
            let _write_guard = context.workspace_write_lock().await;
            let input: DeletePathInput = deserialize_tool_input(self.name(), input.arguments)?;
            let paths = workspace(&context).await?;
            let path = paths.resolve_existing(&input.path).await?;
            let metadata = tokio::fs::metadata(&path).await?;
            match (metadata.is_dir(), input.delete_mode()) {
                (false, DeleteMode::File) => tokio::fs::remove_file(&path).await?,
                (false, DeleteMode::EmptyDirectory | DeleteMode::RecursiveDirectory) => {
                    return Err(tool_error(
                        self.name(),
                        "delete mode requires a directory but path is a file",
                    ));
                }
                (true, DeleteMode::File) => {
                    return Err(tool_error(
                        self.name(),
                        "delete mode file cannot delete a directory",
                    ));
                }
                (true, DeleteMode::EmptyDirectory) => tokio::fs::remove_dir(&path).await?,
                (true, DeleteMode::RecursiveDirectory) => {
                    remove_dir_all_no_follow_async(paths.root(), &path)
                        .await
                        .map_err(|error| tool_error(self.name(), error))?;
                }
            }
            sync_lsp_deleted(&context, &path).await;
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
        FunctionToolDefinition::<CopyMoveInput>::new(self.name(), self.description()).input_schema()
    }

    fn execute<'a>(
        &'a self,
        input: ToolInput,
        context: ToolContext,
    ) -> BoxFuture<'a, Result<ToolOutput, PureError>> {
        Box::pin(async move {
            context.ensure_workspace_writable()?;
            let _write_guard = context.workspace_write_lock().await;
            let input: CopyMoveInput = deserialize_tool_input(self.name(), input.arguments)?;
            let paths = workspace(&context).await?;
            let from = paths.resolve_existing(&input.from).await?;
            let to = paths.resolve_for_write(&input.to).await?;
            ensure_overwrite(
                &to,
                input.collision() == PathCollision::Overwrite,
                self.name(),
            )
            .await?;
            if let Some(parent) = to.parent() {
                tokio::fs::create_dir_all(parent).await?;
            }
            tokio::fs::copy(&from, &to).await?;
            sync_lsp_changed(&context, &to).await;
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
        FunctionToolDefinition::<CopyMoveInput>::new(self.name(), self.description()).input_schema()
    }

    fn execute<'a>(
        &'a self,
        input: ToolInput,
        context: ToolContext,
    ) -> BoxFuture<'a, Result<ToolOutput, PureError>> {
        Box::pin(async move {
            context.ensure_workspace_writable()?;
            let _write_guard = context.workspace_write_lock().await;
            let input: CopyMoveInput = deserialize_tool_input(self.name(), input.arguments)?;
            let paths = workspace(&context).await?;
            let from = paths.resolve_existing(&input.from).await?;
            let to = paths.resolve_for_write(&input.to).await?;
            ensure_overwrite(
                &to,
                input.collision() == PathCollision::Overwrite,
                self.name(),
            )
            .await?;
            if let Some(parent) = to.parent() {
                tokio::fs::create_dir_all(parent).await?;
            }
            tokio::fs::rename(&from, &to).await?;
            sync_lsp_deleted(&context, &from).await;
            sync_lsp_changed(&context, &to).await;
            Ok(text_output(format!(
                "Moved {} to {}",
                paths.display_relative(&from),
                paths.display_relative(&to)
            )))
        })
    }
}

async fn sync_lsp_changed(context: &ToolContext, path: &std::path::Path) {
    if let Some(registry) = &context.lsp_runtime {
        registry.notify_file_changed(path).await;
    }
}

async fn sync_lsp_deleted(context: &ToolContext, path: &std::path::Path) {
    if let Some(registry) = &context.lsp_runtime {
        registry.notify_file_deleted(path).await;
    }
}

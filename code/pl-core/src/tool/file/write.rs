use futures::FutureExt;
use pl_protocol::PureError;
use tokio::fs::OpenOptions;
use tokio::io::AsyncWriteExt;

use super::helpers::*;
use super::input::*;
use crate::path_safety::remove_dir_all_no_follow_async;
use crate::tool::{
    BoxFuture, Tool, ToolCallContext, ToolInput, ToolResult, ToolWorkspace, TypedTool,
    deserialize_tool_input, tool_error,
};
use crate::turn::ToolEffect;

#[derive(Debug, Clone)]
pub struct WriteFileTool {
    workspace: ToolWorkspace,
}

#[derive(Debug, Clone)]
pub struct CreateDirectoryTool {
    workspace: ToolWorkspace,
}

#[derive(Debug, Clone)]
pub struct DeletePathTool {
    workspace: ToolWorkspace,
}

#[derive(Debug, Clone)]
pub struct CopyPathTool {
    workspace: ToolWorkspace,
}

#[derive(Debug, Clone)]
pub struct MovePathTool {
    workspace: ToolWorkspace,
}

impl WriteFileTool {
    pub fn new(workspace: ToolWorkspace) -> Self {
        Self { workspace }
    }
}

impl CreateDirectoryTool {
    pub fn new(workspace: ToolWorkspace) -> Self {
        Self { workspace }
    }
}

impl DeletePathTool {
    pub fn new(workspace: ToolWorkspace) -> Self {
        Self { workspace }
    }
}

impl CopyPathTool {
    pub fn new(workspace: ToolWorkspace) -> Self {
        Self { workspace }
    }
}

impl MovePathTool {
    pub fn new(workspace: ToolWorkspace) -> Self {
        Self { workspace }
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
        TypedTool::<WriteFileInput>::new(self.name(), self.description()).input_schema()
    }

    fn effect(&self) -> Option<ToolEffect> {
        Some(ToolEffect::WorkspaceWrite)
    }

    fn execute<'a>(
        &'a self,
        input: ToolInput,
        context: ToolCallContext,
    ) -> BoxFuture<'a, Result<ToolResult, PureError>> {
        async move {
            self.workspace.ensure_workspace_writable()?;
            let _write_guard = self.workspace.write_lock().await;
            let input: WriteFileInput = deserialize_tool_input(self.name(), input.arguments)?;
            let paths = workspace(&self.workspace, &context).await?;
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
            self.workspace.notify_changed(&path).await;
            Ok(text_output(format!(
                "Wrote {}",
                paths.display_relative(&path)
            )))
        }
        .boxed()
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
        TypedTool::<PathInput>::new(self.name(), self.description()).input_schema()
    }

    fn effect(&self) -> Option<ToolEffect> {
        Some(ToolEffect::WorkspaceWrite)
    }

    fn execute<'a>(
        &'a self,
        input: ToolInput,
        context: ToolCallContext,
    ) -> BoxFuture<'a, Result<ToolResult, PureError>> {
        async move {
            self.workspace.ensure_workspace_writable()?;
            let _write_guard = self.workspace.write_lock().await;
            let input: PathInput = deserialize_tool_input(self.name(), input.arguments)?;
            let paths = workspace(&self.workspace, &context).await?;
            let path = paths.resolve_for_write(&input.path).await?;
            tokio::fs::create_dir_all(&path).await?;
            Ok(text_output(format!(
                "Created directory {}",
                paths.display_relative(&path)
            )))
        }
        .boxed()
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
        TypedTool::<DeletePathInput>::new(self.name(), self.description()).input_schema()
    }

    fn effect(&self) -> Option<ToolEffect> {
        Some(ToolEffect::WorkspaceWrite)
    }

    fn execute<'a>(
        &'a self,
        input: ToolInput,
        context: ToolCallContext,
    ) -> BoxFuture<'a, Result<ToolResult, PureError>> {
        async move {
            self.workspace.ensure_workspace_writable()?;
            let _write_guard = self.workspace.write_lock().await;
            let input: DeletePathInput = deserialize_tool_input(self.name(), input.arguments)?;
            let paths = workspace(&self.workspace, &context).await?;
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
            self.workspace.notify_deleted(&path).await;
            Ok(text_output(format!(
                "Deleted {}",
                paths.display_relative(&path)
            )))
        }
        .boxed()
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
        TypedTool::<CopyMoveInput>::new(self.name(), self.description()).input_schema()
    }

    fn effect(&self) -> Option<ToolEffect> {
        Some(ToolEffect::WorkspaceWrite)
    }

    fn execute<'a>(
        &'a self,
        input: ToolInput,
        context: ToolCallContext,
    ) -> BoxFuture<'a, Result<ToolResult, PureError>> {
        async move {
            self.workspace.ensure_workspace_writable()?;
            let _write_guard = self.workspace.write_lock().await;
            let input: CopyMoveInput = deserialize_tool_input(self.name(), input.arguments)?;
            let paths = workspace(&self.workspace, &context).await?;
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
            self.workspace.notify_changed(&to).await;
            Ok(text_output(format!(
                "Copied {} to {}",
                paths.display_relative(&from),
                paths.display_relative(&to)
            )))
        }
        .boxed()
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
        TypedTool::<CopyMoveInput>::new(self.name(), self.description()).input_schema()
    }

    fn effect(&self) -> Option<ToolEffect> {
        Some(ToolEffect::WorkspaceWrite)
    }

    fn execute<'a>(
        &'a self,
        input: ToolInput,
        context: ToolCallContext,
    ) -> BoxFuture<'a, Result<ToolResult, PureError>> {
        async move {
            self.workspace.ensure_workspace_writable()?;
            let _write_guard = self.workspace.write_lock().await;
            let input: CopyMoveInput = deserialize_tool_input(self.name(), input.arguments)?;
            let paths = workspace(&self.workspace, &context).await?;
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
            self.workspace.notify_deleted(&from).await;
            self.workspace.notify_changed(&to).await;
            Ok(text_output(format!(
                "Moved {} to {}",
                paths.display_relative(&from),
                paths.display_relative(&to)
            )))
        }
        .boxed()
    }
}

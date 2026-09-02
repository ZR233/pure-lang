use std::future::Future;

use pl_protocol::PureError;
use tokio::fs::OpenOptions;
use tokio::io::AsyncWriteExt;

use super::helpers::*;
use super::input::*;
use crate::path_safety::remove_dir_all_no_follow_async;
use crate::tool::{StaticTool, ToolCallContext, ToolPolicy, ToolResult, ToolWorkspace, tool_error};
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

impl StaticTool for WriteFileTool {
    type Input = WriteFileInput;

    fn definition(&self) -> crate::tool::StaticToolDefinition {
        crate::tool::StaticToolDefinition::new(
            crate::tool::ToolName::builtin("write_file"),
            "Write UTF-8 text to a workspace file using create, overwrite, or append mode.",
        )
    }

    fn policy(&self) -> ToolPolicy {
        ToolPolicy::default().with_effect(ToolEffect::WorkspaceWrite)
    }

    fn execute(
        &self,
        input: WriteFileInput,
        context: ToolCallContext,
    ) -> impl Future<Output = Result<ToolResult, PureError>> + Send {
        async move {
            self.workspace.ensure_workspace_writable()?;
            let _write_guard = self.workspace.write_lock().await;
            let paths = workspace(&self.workspace, &context).await?;
            let path = paths.resolve_for_write(&input.path).await?;
            self.workspace.ensure_path_writable(&path)?;
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
    }
}

impl StaticTool for CreateDirectoryTool {
    type Input = PathInput;

    fn definition(&self) -> crate::tool::StaticToolDefinition {
        crate::tool::StaticToolDefinition::new(
            crate::tool::ToolName::builtin("create_directory"),
            "Create a directory inside the workspace.",
        )
    }

    fn policy(&self) -> ToolPolicy {
        ToolPolicy::default().with_effect(ToolEffect::WorkspaceWrite)
    }

    fn execute(
        &self,
        input: PathInput,
        context: ToolCallContext,
    ) -> impl Future<Output = Result<ToolResult, PureError>> + Send {
        async move {
            self.workspace.ensure_workspace_writable()?;
            let _write_guard = self.workspace.write_lock().await;
            let paths = workspace(&self.workspace, &context).await?;
            let path = paths.resolve_for_write(&input.path).await?;
            self.workspace.ensure_path_writable(&path)?;
            tokio::fs::create_dir_all(&path).await?;
            Ok(text_output(format!(
                "Created directory {}",
                paths.display_relative(&path)
            )))
        }
    }
}

impl StaticTool for DeletePathTool {
    type Input = DeletePathInput;

    fn definition(&self) -> crate::tool::StaticToolDefinition {
        crate::tool::StaticToolDefinition::new(
            crate::tool::ToolName::builtin("delete_path"),
            "Delete a workspace file, empty directory, or recursive directory using an explicit mode.",
        )
    }

    fn policy(&self) -> ToolPolicy {
        ToolPolicy::default().with_effect(ToolEffect::WorkspaceWrite)
    }

    fn execute(
        &self,
        input: DeletePathInput,
        context: ToolCallContext,
    ) -> impl Future<Output = Result<ToolResult, PureError>> + Send {
        async move {
            self.workspace.ensure_workspace_writable()?;
            let _write_guard = self.workspace.write_lock().await;
            let paths = workspace(&self.workspace, &context).await?;
            let path = paths.resolve_existing(&input.path).await?;
            self.workspace.ensure_path_writable(&path)?;
            let metadata = tokio::fs::metadata(&path).await?;
            match (metadata.is_dir(), input.delete_mode()) {
                (false, DeleteMode::File) => tokio::fs::remove_file(&path).await?,
                (false, DeleteMode::EmptyDirectory | DeleteMode::RecursiveDirectory) => {
                    return Err(tool_error(
                        "delete_path",
                        "delete mode requires a directory but path is a file",
                    ));
                }
                (true, DeleteMode::File) => {
                    return Err(tool_error(
                        "delete_path",
                        "delete mode file cannot delete a directory",
                    ));
                }
                (true, DeleteMode::EmptyDirectory) => tokio::fs::remove_dir(&path).await?,
                (true, DeleteMode::RecursiveDirectory) => {
                    remove_dir_all_no_follow_async(paths.root(), &path)
                        .await
                        .map_err(|error| tool_error("delete_path", error))?;
                }
            }
            self.workspace.notify_deleted(&path).await;
            Ok(text_output(format!(
                "Deleted {}",
                paths.display_relative(&path)
            )))
        }
    }
}

impl StaticTool for CopyPathTool {
    type Input = CopyMoveInput;

    fn definition(&self) -> crate::tool::StaticToolDefinition {
        crate::tool::StaticToolDefinition::new(
            crate::tool::ToolName::builtin("copy_path"),
            "Copy a file inside the workspace.",
        )
    }

    fn policy(&self) -> ToolPolicy {
        ToolPolicy::default().with_effect(ToolEffect::WorkspaceWrite)
    }

    fn execute(
        &self,
        input: CopyMoveInput,
        context: ToolCallContext,
    ) -> impl Future<Output = Result<ToolResult, PureError>> + Send {
        async move {
            self.workspace.ensure_workspace_writable()?;
            let _write_guard = self.workspace.write_lock().await;
            let paths = workspace(&self.workspace, &context).await?;
            let from = paths.resolve_existing(&input.from).await?;
            let to = paths.resolve_for_write(&input.to).await?;
            self.workspace.ensure_path_writable(&to)?;
            ensure_overwrite(
                &to,
                input.collision() == PathCollision::Overwrite,
                "copy_path",
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
    }
}

impl StaticTool for MovePathTool {
    type Input = CopyMoveInput;

    fn definition(&self) -> crate::tool::StaticToolDefinition {
        crate::tool::StaticToolDefinition::new(
            crate::tool::ToolName::builtin("move_path"),
            "Move or rename a file or directory inside the workspace.",
        )
    }

    fn policy(&self) -> ToolPolicy {
        ToolPolicy::default().with_effect(ToolEffect::WorkspaceWrite)
    }

    fn execute(
        &self,
        input: CopyMoveInput,
        context: ToolCallContext,
    ) -> impl Future<Output = Result<ToolResult, PureError>> + Send {
        async move {
            self.workspace.ensure_workspace_writable()?;
            let _write_guard = self.workspace.write_lock().await;
            let paths = workspace(&self.workspace, &context).await?;
            let from = paths.resolve_existing(&input.from).await?;
            let to = paths.resolve_for_write(&input.to).await?;
            self.workspace.ensure_path_writable(&from)?;
            self.workspace.ensure_path_writable(&to)?;
            ensure_overwrite(
                &to,
                input.collision() == PathCollision::Overwrite,
                "move_path",
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
    }
}

use crate::tool_error;

use pl_protocol::PureError;

use super::helpers::*;
use super::input::*;

use crate::workspace::ToolWorkspace;
use crate::workspace::path_safety::remove_dir_all_no_follow_async;

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

impl pl_core::tool::opaque::Tool for CreateDirectoryTool {
    async fn execute(
        &self,
        input: pl_core::context::OpaquePayload,
        context: pl_core::tool::opaque::CallContext,
    ) -> std::result::Result<pl_core::tool::ToolOutput, pl_core::tool::opaque::ToolError> {
        let input: PathInput =
            serde_json::from_str(input.content()).map_err(pl_core::tool::opaque::ToolError::new)?;
        self.execute_input(input, context)
            .await
            .map_err(pl_core::tool::opaque::ToolError::new)
    }
}
impl CreateDirectoryTool {
    async fn execute_input(
        &self,
        input: PathInput,
        context: pl_core::tool::opaque::CallContext,
    ) -> Result<pl_core::tool::ToolOutput, PureError> {
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

impl pl_core::tool::opaque::Tool for DeletePathTool {
    async fn execute(
        &self,
        input: pl_core::context::OpaquePayload,
        context: pl_core::tool::opaque::CallContext,
    ) -> std::result::Result<pl_core::tool::ToolOutput, pl_core::tool::opaque::ToolError> {
        let input: DeletePathInput =
            serde_json::from_str(input.content()).map_err(pl_core::tool::opaque::ToolError::new)?;
        self.execute_input(input, context)
            .await
            .map_err(pl_core::tool::opaque::ToolError::new)
    }
}
impl DeletePathTool {
    async fn execute_input(
        &self,
        input: DeletePathInput,
        context: pl_core::tool::opaque::CallContext,
    ) -> Result<pl_core::tool::ToolOutput, PureError> {
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

impl pl_core::tool::opaque::Tool for CopyPathTool {
    async fn execute(
        &self,
        input: pl_core::context::OpaquePayload,
        context: pl_core::tool::opaque::CallContext,
    ) -> std::result::Result<pl_core::tool::ToolOutput, pl_core::tool::opaque::ToolError> {
        let input: CopyMoveInput =
            serde_json::from_str(input.content()).map_err(pl_core::tool::opaque::ToolError::new)?;
        self.execute_input(input, context)
            .await
            .map_err(pl_core::tool::opaque::ToolError::new)
    }
}
impl CopyPathTool {
    async fn execute_input(
        &self,
        input: CopyMoveInput,
        context: pl_core::tool::opaque::CallContext,
    ) -> Result<pl_core::tool::ToolOutput, PureError> {
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

impl pl_core::tool::opaque::Tool for MovePathTool {
    async fn execute(
        &self,
        input: pl_core::context::OpaquePayload,
        context: pl_core::tool::opaque::CallContext,
    ) -> std::result::Result<pl_core::tool::ToolOutput, pl_core::tool::opaque::ToolError> {
        let input: CopyMoveInput =
            serde_json::from_str(input.content()).map_err(pl_core::tool::opaque::ToolError::new)?;
        self.execute_input(input, context)
            .await
            .map_err(pl_core::tool::opaque::ToolError::new)
    }
}
impl MovePathTool {
    async fn execute_input(
        &self,
        input: CopyMoveInput,
        context: pl_core::tool::opaque::CallContext,
    ) -> Result<pl_core::tool::ToolOutput, PureError> {
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

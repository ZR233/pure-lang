use std::sync::Arc;

use crate::tool_error;
use pl_protocol::Result;

use crate::file::{CopyMoveInput, DeleteMode, DeletePathInput, PathCollision, PathInput};
use crate::workspace::ToolWorkspace;
use crate::{deserialize_tool_input, typed_tool_input_schema};
use pl_core::{
    context::{ContextContent, OpaquePayload},
    tool::{
        ToolOutput,
        opaque::{CallContext, Tool, ToolError},
    },
};

use super::RemoteWorkspaceFileBackend;

#[derive(Debug, Clone)]
pub struct RemoteWorkspaceMutationTool {
    kind: RemoteMutationKind,
    backend: Arc<RemoteWorkspaceFileBackend>,
    workspace: ToolWorkspace,
}

impl RemoteWorkspaceMutationTool {
    fn name(&self) -> &str {
        self.kind.name()
    }
}

#[derive(Debug, Clone, Copy)]
pub enum RemoteMutationKind {
    CreateDirectory,
    Delete,
    Copy,
    Move,
}

impl RemoteMutationKind {
    pub fn all() -> &'static [Self] {
        &[Self::CreateDirectory, Self::Delete, Self::Copy, Self::Move]
    }

    fn name(self) -> &'static str {
        match self {
            Self::CreateDirectory => "create_directory",
            Self::Delete => "delete_path",
            Self::Copy => "copy_path",
            Self::Move => "move_path",
        }
    }

    fn description(self) -> &'static str {
        match self {
            Self::CreateDirectory => "Create a directory inside the workspace.",
            Self::Delete => {
                "Delete a workspace file, empty directory, or recursive directory using an explicit mode."
            }
            Self::Copy => "Copy a file inside the workspace.",
            Self::Move => "Move or rename a file or directory inside the workspace.",
        }
    }

    fn schema(self) -> serde_json::Value {
        match self {
            Self::CreateDirectory => typed_tool_input_schema::<PathInput>(),
            Self::Delete => typed_tool_input_schema::<DeletePathInput>(),
            Self::Copy | Self::Move => typed_tool_input_schema::<CopyMoveInput>(),
        }
    }
}

impl RemoteWorkspaceMutationTool {
    pub fn new(
        kind: RemoteMutationKind,
        backend: Arc<RemoteWorkspaceFileBackend>,
        workspace: ToolWorkspace,
    ) -> Self {
        Self {
            kind,
            backend,
            workspace,
        }
    }
    pub fn declaration(&self) -> pl_protocol::ToolSpec {
        pl_protocol::ToolSpec::function(
            self.kind.name(),
            self.kind.description(),
            self.kind.schema(),
        )
    }
    async fn execute_input(&self, input: serde_json::Value) -> Result<ToolOutput> {
        self.workspace.ensure_workspace_writable()?;
        let _guard = self.workspace.write_lock().await;

        let text = match self.kind {
            RemoteMutationKind::CreateDirectory => self.create_directory(input).await?,
            RemoteMutationKind::Delete => self.delete(input).await?,
            RemoteMutationKind::Copy => self.copy_or_move(input, false).await?,
            RemoteMutationKind::Move => self.copy_or_move(input, true).await?,
        };
        Ok(text_result(text))
    }
}
impl Tool for RemoteWorkspaceMutationTool {
    async fn execute(
        &self,
        input: OpaquePayload,
        context: CallContext,
    ) -> std::result::Result<ToolOutput, ToolError> {
        if context.cancellation.is_cancelled() {
            return Err(ToolError::new(pl_core::thread::ThreadError::Cancelled));
        }
        let input = serde_json::from_str(input.content()).map_err(ToolError::new)?;
        self.execute_input(input).await.map_err(ToolError::new)
    }
}

impl RemoteWorkspaceMutationTool {
    async fn create_directory(&self, input: serde_json::Value) -> Result<String> {
        let input: PathInput = deserialize_tool_input(self.name(), input)?;
        self.workspace
            .ensure_relative_path_writable(None, &input.path)?;
        self.backend
            .create_directory(input.path.clone(), None)
            .await?;
        Ok(format!("Created directory {}", input.path))
    }

    async fn delete(&self, input: serde_json::Value) -> Result<String> {
        let input: DeletePathInput = deserialize_tool_input(self.name(), input)?;
        self.workspace
            .ensure_relative_path_writable(None, &input.path)?;
        let stat = self
            .backend
            .stat_optional(input.path.clone(), None)
            .await?
            .ok_or_else(|| tool_error(self.name(), "path does not exist"))?;
        match (stat.is_dir, input.mode) {
            (false, DeleteMode::File)
            | (true, DeleteMode::EmptyDirectory)
            | (true, DeleteMode::RecursiveDirectory) => {}
            (false, DeleteMode::EmptyDirectory | DeleteMode::RecursiveDirectory) => {
                return Err(tool_error(self.name(), "delete mode requires a directory"));
            }
            (true, DeleteMode::File) => {
                return Err(tool_error(
                    self.name(),
                    "delete mode file cannot delete a directory",
                ));
            }
        }
        self.backend
            .remove_path(
                input.path.clone(),
                None,
                matches!(input.mode, DeleteMode::RecursiveDirectory),
            )
            .await?;
        Ok(format!("Deleted {}", input.path))
    }

    async fn copy_or_move(&self, input: serde_json::Value, moving: bool) -> Result<String> {
        let input: CopyMoveInput = deserialize_tool_input(self.name(), input)?;
        if moving {
            self.workspace
                .ensure_relative_path_writable(None, &input.from)?;
        }
        self.workspace
            .ensure_relative_path_writable(None, &input.to)?;
        let source = self
            .backend
            .stat_optional(input.from.clone(), None)
            .await?
            .ok_or_else(|| tool_error(self.name(), "source does not exist"))?;
        if let Some(target) = self.backend.stat_optional(input.to.clone(), None).await? {
            if matches!(input.collision, PathCollision::FailIfExists) {
                return Err(tool_error(self.name(), "destination already exists"));
            }
            self.backend
                .remove_path(input.to.clone(), None, target.is_dir)
                .await?;
        }
        if moving {
            self.backend
                .rename_path(input.from.clone(), input.to.clone(), None)
                .await?;
            Ok(format!("Moved {} to {}", input.from, input.to))
        } else {
            self.backend
                .copy_path(input.from.clone(), input.to.clone(), None, source.is_dir)
                .await?;
            Ok(format!("Copied {} to {}", input.from, input.to))
        }
    }
}

fn text_result(text: String) -> ToolOutput {
    ToolOutput::new(
        OpaquePayload::text(text.clone()),
        vec![ContextContent::Text { text: text.into() }],
    )
}

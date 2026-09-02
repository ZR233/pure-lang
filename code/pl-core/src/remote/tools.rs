use std::future::Future;
use std::path::PathBuf;
use std::sync::Arc;

use pl_protocol::Result;
use schemars::JsonSchema;
use serde::Deserialize;

use crate::tool::{
    DynTool, StaticTool, ToolCallContext, ToolInput, ToolPolicy, ToolResult, ToolResultContent,
    ToolWorkspace, WorkspaceFileBackend, WorkspaceFileReadRequest, WorkspaceFileWriteRequest,
    deserialize_tool_input, tool_error, typed_tool_input_schema,
};
use crate::turn::ToolEffect;

use super::RemoteWorkspaceFileBackend;

/// Builds the ordinary local tool schemas whose mutations are executed by a remote file backend.
pub fn remote_workspace_mutation_tools(
    backend: Arc<RemoteWorkspaceFileBackend>,
    workspace: ToolWorkspace,
) -> Vec<DynTool> {
    RemoteMutationKind::all()
        .iter()
        .copied()
        .map(|kind| {
            RemoteWorkspaceMutationTool {
                kind,
                backend: backend.clone(),
                workspace: workspace.clone(),
            }
            .into()
        })
        .collect()
}

#[derive(Debug, Clone)]
struct RemoteWorkspaceMutationTool {
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
enum RemoteMutationKind {
    Write,
    Stat,
    CreateDirectory,
    Delete,
    Copy,
    Move,
}

impl RemoteMutationKind {
    fn all() -> &'static [Self] {
        &[
            Self::Write,
            Self::Stat,
            Self::CreateDirectory,
            Self::Delete,
            Self::Copy,
            Self::Move,
        ]
    }

    fn name(self) -> &'static str {
        match self {
            Self::Write => "write_file",
            Self::Stat => "stat_path",
            Self::CreateDirectory => "create_directory",
            Self::Delete => "delete_path",
            Self::Copy => "copy_path",
            Self::Move => "move_path",
        }
    }

    fn description(self) -> &'static str {
        match self {
            Self::Write => {
                "Write UTF-8 text to a workspace file using create, overwrite, or append mode."
            }
            Self::Stat => {
                "Return metadata for a workspace path, or `exists: false` when the path is absent."
            }
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
            Self::Write => typed_tool_input_schema::<WriteFileInput>(),
            Self::Stat | Self::CreateDirectory => typed_tool_input_schema::<PathInput>(),
            Self::Delete => typed_tool_input_schema::<DeletePathInput>(),
            Self::Copy | Self::Move => typed_tool_input_schema::<CopyMoveInput>(),
        }
    }
}

impl StaticTool for RemoteWorkspaceMutationTool {
    type Input = serde_json::Value;

    fn definition(&self) -> crate::tool::StaticToolDefinition {
        crate::tool::StaticToolDefinition::new(
            crate::tool::ToolName::builtin(self.kind.name()),
            self.kind.description(),
        )
    }

    fn input_schema(&self) -> serde_json::Value {
        self.kind.schema()
    }

    fn policy(&self) -> ToolPolicy {
        let effect = if matches!(self.kind, RemoteMutationKind::Stat) {
            ToolEffect::Read
        } else {
            ToolEffect::WorkspaceWrite
        };
        let mut policy = ToolPolicy::default().with_effect(effect);
        if matches!(self.kind, RemoteMutationKind::Stat) {
            policy = policy.with_parallel_tool_calls();
        }
        policy
    }

    fn execute(
        &self,
        input: serde_json::Value,
        _context: ToolCallContext,
    ) -> impl Future<Output = Result<ToolResult>> + Send {
        async move {
            if !matches!(self.kind, RemoteMutationKind::Stat) {
                self.workspace.ensure_workspace_writable()?;
            }
            let _guard = if matches!(self.kind, RemoteMutationKind::Stat) {
                None
            } else {
                Some(self.workspace.write_lock().await)
            };
            let input = ToolInput { arguments: input };
            let text = match self.kind {
                RemoteMutationKind::Write => self.write(input).await?,
                RemoteMutationKind::Stat => self.stat(input).await?,
                RemoteMutationKind::CreateDirectory => self.create_directory(input).await?,
                RemoteMutationKind::Delete => self.delete(input).await?,
                RemoteMutationKind::Copy => self.copy_or_move(input, false).await?,
                RemoteMutationKind::Move => self.copy_or_move(input, true).await?,
            };
            Ok(text_result(text))
        }
    }
}

impl RemoteWorkspaceMutationTool {
    async fn write(&self, input: ToolInput) -> Result<String> {
        let input: WriteFileInput = deserialize_tool_input(self.name(), input.arguments)?;
        self.workspace
            .ensure_relative_path_writable(None, &input.path)?;
        let existing = self.backend.stat_optional(input.path.clone(), None).await?;
        let content = match input.mode {
            WriteMode::Create if existing.is_some() => {
                return Err(tool_error(self.name(), "destination already exists"));
            }
            WriteMode::Create | WriteMode::Overwrite => input.content,
            WriteMode::Append => {
                let mut current = if existing.is_some() {
                    self.backend
                        .read_text(WorkspaceFileReadRequest {
                            path: input.path.clone(),
                            cwd: None,
                        })
                        .await?
                } else {
                    String::new()
                };
                current.push_str(&input.content);
                current
            }
        };
        self.backend
            .write_text(WorkspaceFileWriteRequest {
                path: input.path.clone(),
                cwd: None,
                content,
            })
            .await?;
        Ok(format!("Wrote {}", input.path))
    }

    async fn stat(&self, input: ToolInput) -> Result<String> {
        let input: PathInput = deserialize_tool_input(self.name(), input.arguments)?;
        let stat = self.backend.stat_optional(input.path.clone(), None).await?;
        Ok(match stat {
            Some(stat) => serde_json::json!({
                "path": stat.path,
                "exists": true,
                "type": if stat.is_dir { "directory" } else { "file" },
                "len": stat.len,
                "readonly": false,
                "modifiedAt": null,
            }),
            None => serde_json::json!({ "path": input.path, "exists": false }),
        }
        .to_string())
    }

    async fn create_directory(&self, input: ToolInput) -> Result<String> {
        let input: PathInput = deserialize_tool_input(self.name(), input.arguments)?;
        self.workspace
            .ensure_relative_path_writable(None, &input.path)?;
        self.backend
            .create_directory(input.path.clone(), None)
            .await?;
        Ok(format!("Created directory {}", input.path))
    }

    async fn delete(&self, input: ToolInput) -> Result<String> {
        let input: DeletePathInput = deserialize_tool_input(self.name(), input.arguments)?;
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

    async fn copy_or_move(&self, input: ToolInput, moving: bool) -> Result<String> {
        let input: CopyMoveInput = deserialize_tool_input(self.name(), input.arguments)?;
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

fn text_result(text: String) -> ToolResult {
    ToolResult {
        success: true,
        content: ToolResultContent::Text(text.clone()),
        model_output: text,
        model_attachments: Vec::new(),
        truncated: crate::tool::OutputTruncation::empty(),
        output_file: PathBuf::new(),
        exit_code: Some(0),
        timed_out: false,
        runtime_events: Vec::new(),
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct WriteFileInput {
    path: String,
    content: String,
    mode: WriteMode,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
enum WriteMode {
    Create,
    Overwrite,
    Append,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PathInput {
    path: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct DeletePathInput {
    path: String,
    mode: DeleteMode,
}

#[derive(Debug, Clone, Copy, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
enum DeleteMode {
    File,
    EmptyDirectory,
    RecursiveDirectory,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CopyMoveInput {
    from: String,
    to: String,
    collision: PathCollision,
}

#[derive(Debug, Clone, Copy, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
enum PathCollision {
    FailIfExists,
    Overwrite,
}

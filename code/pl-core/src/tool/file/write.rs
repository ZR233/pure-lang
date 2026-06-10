use pl_protocol::PureError;
use tokio::fs::OpenOptions;
use tokio::io::AsyncWriteExt;

use super::helpers::{ensure_overwrite, parse_input, text_output, tool_error, workspace};
use super::input::{
    CopyMoveInput, DeleteMode, DeletePathInput, PathCollision, PathInput, WriteFileInput,
    WriteMode, copy_move_schema, path_schema,
};
use crate::tool::{BoxFuture, Tool, ToolContext, ToolInput, ToolOutput};

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
    ) -> BoxFuture<'a, Result<ToolOutput, PureError>> {
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
    ) -> BoxFuture<'a, Result<ToolOutput, PureError>> {
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
        "Delete a workspace file, empty directory, or recursive directory using an explicit mode."
    }

    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": { "type": "string" },
                "mode": {
                    "type": "string",
                    "enum": ["file", "emptyDirectory", "recursiveDirectory"]
                }
            },
            "required": ["path", "mode"],
            "additionalProperties": false
        })
    }

    fn execute<'a>(
        &'a self,
        input: ToolInput,
        context: ToolContext,
    ) -> BoxFuture<'a, Result<ToolOutput, PureError>> {
        Box::pin(async move {
            let _write_guard = context.workspace_write_lock().await;
            let input: DeletePathInput = parse_input(input.arguments, self.name())?;
            let paths = workspace(&context).await?;
            let path = paths.resolve_existing(&input.path).await?;
            paths.reject_symlink_write(&path).await?;
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
                    tokio::fs::remove_dir_all(&path).await?;
                }
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
    ) -> BoxFuture<'a, Result<ToolOutput, PureError>> {
        Box::pin(async move {
            let _write_guard = context.workspace_write_lock().await;
            let input: CopyMoveInput = parse_input(input.arguments, self.name())?;
            let paths = workspace(&context).await?;
            let from = paths.resolve_existing(&input.from).await?;
            let to = paths.resolve_for_write(&input.to).await?;
            paths.reject_symlink_write(&to).await?;
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
    ) -> BoxFuture<'a, Result<ToolOutput, PureError>> {
        Box::pin(async move {
            let _write_guard = context.workspace_write_lock().await;
            let input: CopyMoveInput = parse_input(input.arguments, self.name())?;
            let paths = workspace(&context).await?;
            let from = paths.resolve_existing(&input.from).await?;
            let to = paths.resolve_for_write(&input.to).await?;
            paths.reject_symlink_write(&from).await?;
            paths.reject_symlink_write(&to).await?;
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
            Ok(text_output(format!(
                "Moved {} to {}",
                paths.display_relative(&from),
                paths.display_relative(&to)
            )))
        })
    }
}

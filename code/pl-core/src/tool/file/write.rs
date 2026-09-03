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

#[cfg(test)]
mod ops_tests {
    use pretty_assertions::assert_eq;

    use super::super::test_support::*;
    use super::*;
    use crate::{ToolApprovalContext, WorkspaceAccess};

    #[tokio::test]
    async fn write_file_waits_for_workspace_write_lock() {
        let root = unique_temp_dir("write-lock-tool");
        let context = context(&root).await;
        let workspace = tool_workspace(&root);
        let guard = workspace.write_lock().await;
        let tool = WriteFileTool::new(workspace);
        let write_context = context.clone();
        let write_task = tokio::spawn(async move {
            tool.execute_with_tool_input(
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
    async fn directory_workspace_allows_only_configured_project_prefixes() {
        let root = unique_temp_dir("directory-write-prefixes");
        tokio::fs::create_dir_all(root.join("allowed"))
            .await
            .unwrap();
        let tool = WriteFileTool::new(directory_workspace(&root, Some(&["allowed"])));

        tool.execute_with_tool_input(
            input(serde_json::json!({
                "path": "allowed/ok.txt",
                "content": "ok",
                "mode": "create"
            })),
            context(&root).await,
        )
        .await
        .unwrap();
        let error = tool
            .execute_with_tool_input(
                input(serde_json::json!({
                    "path": "denied.txt",
                    "content": "denied",
                    "mode": "create"
                })),
                context(&root).await,
            )
            .await
            .unwrap_err()
            .to_string();

        assert!(error.contains("writablePaths"), "{error}");
        assert_eq!(
            tokio::fs::read_to_string(root.join("allowed/ok.txt"))
                .await
                .unwrap(),
            "ok"
        );
        assert!(
            !tokio::fs::try_exists(root.join("denied.txt"))
                .await
                .unwrap()
        );
        let _ = tokio::fs::remove_dir_all(root).await;
    }

    #[tokio::test]
    async fn directory_workspace_empty_list_is_project_read_only_but_not_an_external_sandbox() {
        let root = unique_temp_dir("directory-empty");
        let outside = unique_temp_dir("directory-external");
        tokio::fs::create_dir_all(&root).await.unwrap();
        tokio::fs::create_dir_all(&outside).await.unwrap();
        let tool = WriteFileTool::new(directory_workspace(&root, Some(&[])));

        let project_error = tool
            .execute_with_tool_input(
                input(serde_json::json!({
                    "path": "denied.txt",
                    "content": "denied",
                    "mode": "create"
                })),
                context(&root).await,
            )
            .await
            .unwrap_err()
            .to_string();
        let external_context = context(&root).await.with_approval(ToolApprovalContext::new(
            crate::turn::PermissionMode::FullAccess,
            WorkspaceAccess::ExternalAllowed,
        ));
        tool.execute_with_tool_input(
            input(serde_json::json!({
                "path": outside.join("allowed.txt").to_string_lossy(),
                "content": "outside",
                "mode": "create"
            })),
            external_context,
        )
        .await
        .unwrap();

        assert!(project_error.contains("writablePaths"), "{project_error}");
        assert_eq!(
            tokio::fs::read_to_string(outside.join("allowed.txt"))
                .await
                .unwrap(),
            "outside"
        );
        let _ = tokio::fs::remove_dir_all(root).await;
        let _ = tokio::fs::remove_dir_all(outside).await;
    }

    #[tokio::test]
    async fn delete_path_modes_are_explicit() {
        let root = unique_temp_dir("delete-mode");
        tokio::fs::create_dir_all(root.join("empty")).await.unwrap();
        tokio::fs::create_dir_all(root.join("tree/nested"))
            .await
            .unwrap();
        tokio::fs::write(root.join("file.txt"), "file")
            .await
            .unwrap();
        tokio::fs::write(root.join("tree/nested/file.txt"), "file")
            .await
            .unwrap();
        let tool = DeletePathTool::new(tool_workspace(&root));

        tool.execute_with_tool_input(
            input(serde_json::json!({ "path": "file.txt", "mode": "file" })),
            context(&root).await,
        )
        .await
        .unwrap();
        tool.execute_with_tool_input(
            input(serde_json::json!({ "path": "empty", "mode": "emptyDirectory" })),
            context(&root).await,
        )
        .await
        .unwrap();
        tool.execute_with_tool_input(
            input(serde_json::json!({ "path": "tree", "mode": "recursiveDirectory" })),
            context(&root).await,
        )
        .await
        .unwrap();

        assert!(!tokio::fs::try_exists(root.join("file.txt")).await.unwrap());
        assert!(!tokio::fs::try_exists(root.join("empty")).await.unwrap());
        assert!(!tokio::fs::try_exists(root.join("tree")).await.unwrap());
        let _ = tokio::fs::remove_dir_all(root).await;
    }

    #[tokio::test]
    async fn copy_and_move_collision_modes_are_explicit() {
        let root = unique_temp_dir("collision-mode");
        tokio::fs::create_dir_all(&root).await.unwrap();
        tokio::fs::write(root.join("source.txt"), "new")
            .await
            .unwrap();
        tokio::fs::write(root.join("target.txt"), "old")
            .await
            .unwrap();
        let copy = CopyPathTool::new(tool_workspace(&root));
        let move_tool = MovePathTool::new(tool_workspace(&root));

        let fail = copy
            .execute_with_tool_input(
                input(serde_json::json!({
                    "from": "source.txt",
                    "to": "target.txt",
                    "collision": "failIfExists"
                })),
                context(&root).await,
            )
            .await;
        assert!(fail.is_err());

        copy.execute_with_tool_input(
            input(serde_json::json!({
                "from": "source.txt",
                "to": "target.txt",
                "collision": "overwrite"
            })),
            context(&root).await,
        )
        .await
        .unwrap();
        assert_eq!(
            tokio::fs::read_to_string(root.join("target.txt"))
                .await
                .unwrap(),
            "new"
        );

        tokio::fs::write(root.join("move-source.txt"), "moved")
            .await
            .unwrap();
        tokio::fs::write(root.join("move-target.txt"), "old")
            .await
            .unwrap();
        move_tool
            .execute_with_tool_input(
                input(serde_json::json!({
                    "from": "move-source.txt",
                    "to": "move-target.txt",
                    "collision": "overwrite"
                })),
                context(&root).await,
            )
            .await
            .unwrap();
        assert_eq!(
            tokio::fs::read_to_string(root.join("move-target.txt"))
                .await
                .unwrap(),
            "moved"
        );
        let _ = tokio::fs::remove_dir_all(root).await;
    }

    #[tokio::test]
    async fn modifying_tools_reject_link_ancestors() {
        let root = unique_temp_dir("reject-linked-writes");
        let outside = unique_temp_dir("reject-linked-writes-target");
        tokio::fs::create_dir_all(&root).await.unwrap();
        tokio::fs::create_dir_all(&outside).await.unwrap();
        tokio::fs::write(outside.join("source.txt"), "outside")
            .await
            .unwrap();
        tokio::fs::write(root.join("source.txt"), "inside")
            .await
            .unwrap();
        create_directory_symlink(&outside, &root.join("linked")).unwrap();

        let write = WriteFileTool::new(tool_workspace(&root))
            .execute_with_tool_input(
                input(serde_json::json!({
                    "path": "linked/new.txt",
                    "content": "blocked",
                    "mode": "create"
                })),
                context(&root).await,
            )
            .await;
        let create = super::super::CreateDirectoryTool::new(tool_workspace(&root))
            .execute_with_tool_input(
                input(serde_json::json!({ "path": "linked/new-directory" })),
                context(&root).await,
            )
            .await;
        let patch = apply_patch_tool(&root)
        .execute_with_tool_input(
            input(serde_json::json!({
                "input": "*** Begin Patch\n*** Add File: linked/patched.txt\n+blocked\n*** End Patch\n"
            })),
            context(&root).await,
        )
        .await;
        let copy_source = CopyPathTool::new(tool_workspace(&root))
            .execute_with_tool_input(
                input(serde_json::json!({
                    "from": "linked/source.txt",
                    "to": "copied.txt",
                    "collision": "failIfExists"
                })),
                context(&root).await,
            )
            .await;
        let copy_target = CopyPathTool::new(tool_workspace(&root))
            .execute_with_tool_input(
                input(serde_json::json!({
                    "from": "source.txt",
                    "to": "linked/copied.txt",
                    "collision": "failIfExists"
                })),
                context(&root).await,
            )
            .await;
        let move_target = MovePathTool::new(tool_workspace(&root))
            .execute_with_tool_input(
                input(serde_json::json!({
                    "from": "source.txt",
                    "to": "linked/moved.txt",
                    "collision": "failIfExists"
                })),
                context(&root).await,
            )
            .await;
        let delete = DeletePathTool::new(tool_workspace(&root))
            .execute_with_tool_input(
                input(serde_json::json!({
                    "path": "linked/source.txt",
                    "mode": "file"
                })),
                context(&root).await,
            )
            .await;

        for result in [
            write,
            create,
            patch,
            copy_source,
            copy_target,
            move_target,
            delete,
        ] {
            let error = result.unwrap_err().to_string();
            assert!(
                error.contains("symbolic link") && error.contains("reparse point"),
                "{error}"
            );
        }
        assert!(!outside.join("new.txt").exists());
        assert!(!outside.join("new-directory").exists());
        assert!(!outside.join("patched.txt").exists());
        assert!(!outside.join("copied.txt").exists());
        assert!(!outside.join("moved.txt").exists());
        assert_eq!(
            tokio::fs::read_to_string(outside.join("source.txt"))
                .await
                .unwrap(),
            "outside"
        );
        remove_directory_symlink(&root.join("linked")).unwrap();
        let _ = tokio::fs::remove_dir_all(root).await;
        let _ = tokio::fs::remove_dir_all(outside).await;
    }

    #[tokio::test]
    async fn recursive_delete_unlinks_child_without_touching_target() {
        let root = unique_temp_dir("safe-recursive-delete");
        let outside = unique_temp_dir("safe-recursive-delete-target");
        tokio::fs::create_dir_all(root.join("tree")).await.unwrap();
        tokio::fs::create_dir_all(&outside).await.unwrap();
        tokio::fs::write(outside.join("kept.txt"), "kept")
            .await
            .unwrap();
        create_directory_symlink(&outside, &root.join("tree/linked")).unwrap();

        DeletePathTool::new(tool_workspace(&root))
            .execute_with_tool_input(
                input(serde_json::json!({
                    "path": "tree",
                    "mode": "recursiveDirectory"
                })),
                context(&root).await,
            )
            .await
            .unwrap();

        assert!(!root.join("tree").exists());
        assert_eq!(
            tokio::fs::read_to_string(outside.join("kept.txt"))
                .await
                .unwrap(),
            "kept"
        );
        let _ = tokio::fs::remove_dir_all(root).await;
        let _ = tokio::fs::remove_dir_all(outside).await;
    }
}

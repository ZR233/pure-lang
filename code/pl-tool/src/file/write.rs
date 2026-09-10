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
#[cfg(test)]
mod ops_tests {
    use pretty_assertions::assert_eq;

    use super::super::test_support::*;
    use super::*;

    #[tokio::test]
    async fn write_file_waits_for_workspace_write_lock() {
        let root = unique_temp_dir("write-lock-tool");
        let context = context(&root).await;
        let workspace = tool_workspace(&root);
        let guard = workspace.write_lock().await;
        let tool = write_file_tool(workspace).await;
        let write_context = context.clone();
        let write_task = tokio::spawn(async move {
            tool.execute(
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
        let tool = write_file_tool(directory_workspace(&root, Some(&["allowed"]))).await;

        tool.execute(
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
            .execute(
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
        let tool = write_file_tool(directory_workspace(&root, Some(&[]))).await;

        let project_error = tool
            .execute(
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
        let mut external_context = context(&root).await;
        external_context.grant = Default::default();
        external_context.grant = external_context
            .grant
            .with_capability(crate::approval::HOST_WORKSPACE_ACCESS);
        tool.execute(
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

        tool.execute(
            input(serde_json::json!({ "path": "file.txt", "mode": "file" })),
            context(&root).await,
        )
        .await
        .unwrap();
        tool.execute(
            input(serde_json::json!({ "path": "empty", "mode": "emptyDirectory" })),
            context(&root).await,
        )
        .await
        .unwrap();
        tool.execute(
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
            .execute(
                input(serde_json::json!({
                    "from": "source.txt",
                    "to": "target.txt",
                    "collision": "failIfExists"
                })),
                context(&root).await,
            )
            .await;
        assert!(fail.is_err());

        copy.execute(
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
            .execute(
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

        let write = write_file_tool(tool_workspace(&root))
            .await
            .execute(
                input(serde_json::json!({
                    "path": "linked/new.txt",
                    "content": "blocked",
                    "mode": "create"
                })),
                context(&root).await,
            )
            .await;
        let create = super::super::CreateDirectoryTool::new(tool_workspace(&root))
            .execute(
                input(serde_json::json!({ "path": "linked/new-directory" })),
                context(&root).await,
            )
            .await;
        let patch = apply_patch_tool(&root).await
        .execute(
            input(serde_json::json!({
                "input": "*** Begin Patch\n*** Add File: linked/patched.txt\n+blocked\n*** End Patch\n"
            })),
            context(&root).await,
        )
        .await;
        let copy_source = CopyPathTool::new(tool_workspace(&root))
            .execute(
                input(serde_json::json!({
                    "from": "linked/source.txt",
                    "to": "copied.txt",
                    "collision": "failIfExists"
                })),
                context(&root).await,
            )
            .await;
        let copy_target = CopyPathTool::new(tool_workspace(&root))
            .execute(
                input(serde_json::json!({
                    "from": "source.txt",
                    "to": "linked/copied.txt",
                    "collision": "failIfExists"
                })),
                context(&root).await,
            )
            .await;
        let move_target = MovePathTool::new(tool_workspace(&root))
            .execute(
                input(serde_json::json!({
                    "from": "source.txt",
                    "to": "linked/moved.txt",
                    "collision": "failIfExists"
                })),
                context(&root).await,
            )
            .await;
        let delete = DeletePathTool::new(tool_workspace(&root))
            .execute(
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
            .execute(
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

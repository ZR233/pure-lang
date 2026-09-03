use std::future::Future;
use std::time::UNIX_EPOCH;

use pl_protocol::PureError;

use super::helpers::*;
use super::input::PathInput;
use crate::tool::{StaticTool, ToolCallContext, ToolPolicy, ToolResult, ToolWorkspace, tool_error};

#[derive(Debug, Clone)]
pub struct StatPathTool {
    workspace: ToolWorkspace,
}

impl StatPathTool {
    pub fn new(workspace: ToolWorkspace) -> Self {
        Self { workspace }
    }
}

impl StaticTool for StatPathTool {
    type Input = PathInput;

    fn definition(&self) -> crate::tool::StaticToolDefinition {
        crate::tool::StaticToolDefinition::new(
            crate::tool::ToolName::builtin("stat_path"),
            "Return metadata for a workspace path, or `exists: false` when the path is absent.",
        )
    }

    fn policy(&self) -> ToolPolicy {
        ToolPolicy::read_only()
            .with_parallel_tool_calls()
            .with_cache_policy(crate::tool::cache::ToolCachePolicy::UntilWorkspaceMutation)
    }

    fn execute(
        &self,
        input: PathInput,
        context: ToolCallContext,
    ) -> impl Future<Output = Result<ToolResult, PureError>> + Send {
        async move {
            let paths = workspace(&self.workspace, &context).await?;
            let path = paths.resolve_existing_or_parent(&input.path).await?;
            let metadata = match tokio::fs::metadata(&path).await {
                Ok(metadata) => metadata,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    return Ok(text_output(
                        serde_json::json!({
                            "path": paths.display_relative(&path),
                            "exists": false,
                        })
                        .to_string(),
                    ));
                }
                Err(error) => {
                    return Err(tool_error(
                        "stat_path",
                        format!("failed to inspect path '{}': {error}", input.path),
                    ));
                }
            };
            let modified_at = metadata
                .modified()
                .ok()
                .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
                .map(|duration| duration.as_secs() as i64);
            Ok(text_output(
                serde_json::json!({
                    "path": paths.display_relative(&path),
                    "exists": true,
                    "type": path_type(&metadata),
                    "len": metadata.len(),
                    "readonly": metadata.permissions().readonly(),
                    "modifiedAt": modified_at,
                })
                .to_string(),
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::super::test_support::*;
    use super::*;
    use crate::tool::{LocalWorkspaceFileTool, WorkspaceFileToolKind};
    fn read_output_json(output: &crate::tool::ToolResult) -> serde_json::Value {
        serde_json::from_str(&output.canonical_output()).unwrap()
    }

    #[tokio::test]
    async fn stat_path_reports_missing_workspace_path_without_tool_failure() {
        let root = unique_temp_dir("stat-missing");
        tokio::fs::create_dir_all(&root).await.unwrap();

        let output = StatPathTool::new(tool_workspace(&root))
            .execute_with_tool_input(
                input(serde_json::json!({ "path": "design" })),
                context(&root).await,
            )
            .await
            .unwrap();

        assert_eq!(
            read_output_json(&output),
            serde_json::json!({ "path": "design", "exists": false })
        );
        let _ = tokio::fs::remove_dir_all(root).await;
    }

    #[tokio::test]
    async fn stat_path_missing_target_still_rejects_workspace_escape() {
        let root = unique_temp_dir("stat-missing-escape");
        tokio::fs::create_dir_all(&root).await.unwrap();

        let error = StatPathTool::new(tool_workspace(&root))
            .execute_with_tool_input(
                input(serde_json::json!({ "path": "../missing" })),
                context(&root).await,
            )
            .await
            .unwrap_err()
            .to_string();

        assert!(error.contains("escapes the workspace"), "{error}");
        let _ = tokio::fs::remove_dir_all(root).await;
    }

    #[tokio::test]
    async fn read_file_uses_canonical_paged_json_output() {
        let root = unique_temp_dir("unified-read");
        tokio::fs::create_dir_all(&root).await.unwrap();
        tokio::fs::write(root.join("a.txt"), "one\ntwo\nthree\n")
            .await
            .unwrap();

        let output = read_file_tool(&root)
            .execute_with_tool_input(
                input(serde_json::json!({
                    "path": "a.txt",
                    "startLine": 2,
                    "maxLines": 1,
                })),
                context(&root).await,
            )
            .await
            .unwrap();
        let value = read_output_json(&output);

        assert_eq!(value["path"], "a.txt");
        assert_eq!(value["startLine"], 2);
        assert_eq!(value["endLine"], 2);
        assert_eq!(value["nextStartLine"], 3);
        assert_eq!(value["text"], "two\n");
        assert!(
            value["contentHash"]
                .as_str()
                .unwrap()
                .starts_with("sha256:")
        );
        let _ = tokio::fs::remove_dir_all(root).await;
    }

    #[tokio::test]
    async fn read_file_rejects_invalid_line_bounds() {
        let root = unique_temp_dir("invalid-read-bounds");
        tokio::fs::create_dir_all(&root).await.unwrap();
        tokio::fs::write(root.join("a.txt"), "one\n").await.unwrap();

        for arguments in [
            serde_json::json!({"path": "a.txt", "startLine": 0}),
            serde_json::json!({"path": "a.txt", "maxLines": 501}),
        ] {
            assert!(
                read_file_tool(&root)
                    .execute_with_tool_input(input(arguments), context(&root).await)
                    .await
                    .is_err()
            );
        }
        let _ = tokio::fs::remove_dir_all(root).await;
    }

    #[tokio::test]
    async fn read_file_missing_path_suggests_workspace_discovery() {
        let root = unique_temp_dir("missing-read-path");
        let candidate = root.join("code/pl-model/src/request.rs");
        tokio::fs::create_dir_all(candidate.parent().unwrap())
            .await
            .unwrap();
        tokio::fs::write(&candidate, "pub struct Request;\n")
            .await
            .unwrap();

        let error = read_file_tool(&root)
            .execute_with_tool_input(
                input(serde_json::json!({
                    "path": "code/pl-model/src/protocol/openai/request.rs"
                })),
                context(&root).await,
            )
            .await
            .unwrap_err()
            .to_string();

        assert!(error.contains("code/pl-model/src/request.rs"), "{error}");
        assert!(error.contains("candidatePaths"), "{error}");
        let _ = tokio::fs::remove_dir_all(root).await;
    }

    #[tokio::test]
    async fn confined_workspace_rejects_escape_even_with_full_access() {
        let root = unique_temp_dir("confined-full-access");
        let outside = unique_temp_dir("confined-full-access-outside");
        tokio::fs::create_dir_all(&root).await.unwrap();
        tokio::fs::create_dir_all(&outside).await.unwrap();
        let outside_file = outside.join("outside.txt");
        tokio::fs::write(&outside_file, "outside").await.unwrap();
        let tool = LocalWorkspaceFileTool::new(
            WorkspaceFileToolKind::ReadFile,
            ToolWorkspace::new(crate::tool::AgentWorkspace::confined(
                root.clone(),
                crate::tool::WorkspaceMutability::ReadWrite,
            )),
        );
        let tool_context =
            context(&root)
                .await
                .with_approval(crate::tool::ToolApprovalContext::new(
                    crate::turn::PermissionMode::FullAccess,
                    crate::tool::WorkspaceAccess::ExternalAllowed,
                ));

        let error = tool
            .execute_with_tool_input(
                input(serde_json::json!({ "path": outside_file })),
                tool_context,
            )
            .await
            .unwrap_err()
            .to_string();

        assert!(error.contains("outside trusted root"), "{error}");
        let _ = tokio::fs::remove_dir_all(root).await;
        let _ = tokio::fs::remove_dir_all(outside).await;
    }

    #[tokio::test]
    async fn read_file_pages_large_output_by_lines() {
        let root = unique_temp_dir("paged-read");
        tokio::fs::create_dir_all(&root).await.unwrap();
        let content = (1..=600)
            .map(|line| format!("line-{line}\n"))
            .collect::<String>();
        tokio::fs::write(root.join("big.txt"), content)
            .await
            .unwrap();

        let output = read_file_tool(&root)
            .execute_with_tool_input(
                input(serde_json::json!({ "path": "big.txt", "maxLines": 500 })),
                context(&root).await,
            )
            .await
            .unwrap();
        let value = read_output_json(&output);

        assert_eq!(value["endLine"], 500);
        assert_eq!(value["nextStartLine"], 501);
        assert!(value["text"].as_str().unwrap().contains("line-500\n"));
        let _ = tokio::fs::remove_dir_all(root).await;
    }

    #[tokio::test]
    async fn read_and_stat_reject_symbolic_links() {
        let root = unique_temp_dir("reject-symlink");
        let outside = unique_temp_dir("reject-symlink-target");
        tokio::fs::create_dir_all(&root).await.unwrap();
        tokio::fs::create_dir_all(&outside).await.unwrap();
        tokio::fs::write(outside.join("real.txt"), "content")
            .await
            .unwrap();
        create_directory_symlink(&outside, &root.join("linked")).unwrap();

        let read_error = read_file_tool(&root)
            .execute_with_tool_input(
                input(serde_json::json!({ "path": "linked/real.txt" })),
                context(&root).await,
            )
            .await
            .unwrap_err()
            .to_string();
        assert!(read_error.contains("symbolic link"));

        let stat_error = StatPathTool::new(tool_workspace(&root))
            .execute_with_tool_input(
                input(serde_json::json!({ "path": "linked/real.txt" })),
                context(&root).await,
            )
            .await
            .unwrap_err()
            .to_string();
        assert!(stat_error.contains("reparse point"), "{stat_error}");

        remove_directory_symlink(&root.join("linked")).unwrap();
        let _ = tokio::fs::remove_dir_all(root).await;
        let _ = tokio::fs::remove_dir_all(outside).await;
    }
}

#[cfg(test)]
mod list_tests {
    use pretty_assertions::assert_eq;

    use super::super::test_support::*;

    #[tokio::test]
    async fn list_files_returns_empty_for_missing_workspace_directory() {
        let root = unique_temp_dir("list-missing-directory");
        tokio::fs::create_dir_all(&root).await.unwrap();
        let tool = list_files_tool(&root);

        let output = tool
            .execute_with_tool_input(
                input(serde_json::json!({
                    "path": "design",
                    "includeDirs": true,
                })),
                context(&root).await,
            )
            .await
            .unwrap();

        let value: serde_json::Value = serde_json::from_str(&output.canonical_output()).unwrap();
        assert_eq!(value["files"], serde_json::json!([]));
        assert_eq!(value["count"], serde_json::json!(0));
        assert_eq!(value["nextCursor"], serde_json::Value::Null);
        let _ = tokio::fs::remove_dir_all(root).await;
    }

    #[tokio::test]
    async fn list_files_empty_fields_use_workspace_defaults_without_listing_root() {
        let root = unique_temp_dir("list-empty-fields");
        tokio::fs::create_dir_all(root.join("src")).await.unwrap();
        tokio::fs::write(root.join("README.md"), "workspace\n")
            .await
            .unwrap();
        let tool = list_files_tool(&root);

        let output = tool
            .execute_with_tool_input(
                input(serde_json::json!({
                    "path": "",
                    "cwd": "",
                    "glob": "",
                    "includeDirs": true,
                })),
                context(&root).await,
            )
            .await
            .unwrap();

        let value: serde_json::Value = serde_json::from_str(&output.canonical_output()).unwrap();
        assert_eq!(value["path"], serde_json::json!("."));
        assert_eq!(value["glob"], serde_json::json!("*"));
        assert_eq!(value["files"], serde_json::json!(["README.md", "src/"]));
        assert!(
            !value["files"]
                .as_array()
                .unwrap()
                .contains(&serde_json::json!("/"))
        );
        let _ = tokio::fs::remove_dir_all(root).await;
    }

    #[tokio::test]
    async fn list_files_directory_glob_matches_entries_relative_to_path() {
        let root = unique_temp_dir("list-path-relative-dirs");
        tokio::fs::create_dir_all(root.join("code/pl-core/src"))
            .await
            .unwrap();
        tokio::fs::create_dir_all(root.join("code/pl-model"))
            .await
            .unwrap();
        let tool = list_files_tool(&root);

        let output = tool
            .execute_with_tool_input(
                input(serde_json::json!({
                    "path": "code",
                    "glob": "*/",
                    "includeDirs": true,
                    "limit": 10,
                })),
                context(&root).await,
            )
            .await
            .unwrap();

        let value: serde_json::Value = serde_json::from_str(&output.canonical_output()).unwrap();
        let files = value["files"].as_array().unwrap();
        assert!(files.contains(&serde_json::json!("code/pl-core/")));
        assert!(files.contains(&serde_json::json!("code/pl-model/")));
        let _ = tokio::fs::remove_dir_all(root).await;
    }

    #[tokio::test]
    async fn list_files_globstar_matches_files_directly_under_prefix() {
        let root = unique_temp_dir("list-globstar-direct");
        tokio::fs::create_dir_all(root.join("design/nested"))
            .await
            .unwrap();
        tokio::fs::write(root.join("design/overview.md"), "# Overview\n")
            .await
            .unwrap();
        tokio::fs::write(root.join("design/nested/report.md"), "# Report\n")
            .await
            .unwrap();
        let tool = list_files_tool(&root);

        let output = tool
            .execute_with_tool_input(
                input(serde_json::json!({
                    "glob": "design/**/*.md",
                    "limit": 10,
                })),
                context(&root).await,
            )
            .await
            .unwrap();

        let value: serde_json::Value = serde_json::from_str(&output.canonical_output()).unwrap();
        assert_eq!(
            value["files"],
            serde_json::json!(["design/nested/report.md", "design/overview.md"])
        );
        let _ = tokio::fs::remove_dir_all(root).await;
    }

    #[tokio::test]
    async fn list_treats_empty_cursor_as_first_page() {
        let root = unique_temp_dir("empty-cursor-first-page");
        tokio::fs::create_dir_all(&root).await.unwrap();
        tokio::fs::write(root.join("README.md"), "maintainers\n")
            .await
            .unwrap();

        let listed = list_files_tool(&root)
            .execute_with_tool_input(
                input(serde_json::json!({
                    "path": "",
                    "glob": "README*",
                    "cursor": "",
                })),
                context(&root).await,
            )
            .await
            .expect("list first page");
        let listed: serde_json::Value = serde_json::from_str(&listed.canonical_output()).unwrap();

        assert_eq!(listed["count"], 1);
        assert_eq!(listed["cursorReset"], false);
        let _ = tokio::fs::remove_dir_all(root).await;
    }

    #[tokio::test]
    async fn malformed_cursor_is_explicitly_reset_to_first_page() {
        let root = unique_temp_dir("malformed-cursor-reset");
        tokio::fs::create_dir_all(&root).await.unwrap();
        tokio::fs::write(root.join("README.md"), "maintainers\n")
            .await
            .unwrap();

        let output = list_files_tool(&root)
            .execute_with_tool_input(
                input(serde_json::json!({
                    "path": "",
                    "glob": "README*",
                    "cursor": "x",
                })),
                context(&root).await,
            )
            .await
            .expect("reset malformed cursor");
        let value: serde_json::Value = serde_json::from_str(&output.canonical_output()).unwrap();

        assert_eq!(value["count"], 1);
        assert_eq!(value["cursorReset"], true);
        let _ = tokio::fs::remove_dir_all(root).await;
    }

    #[tokio::test]
    async fn list_skips_symbolic_link_directories() {
        let root = unique_temp_dir("skip-symbolic-link");
        let outside = unique_temp_dir("symbolic-link-target");
        tokio::fs::create_dir_all(&root).await.unwrap();
        tokio::fs::create_dir_all(&outside).await.unwrap();
        tokio::fs::write(root.join("visible.txt"), "needle\n")
            .await
            .unwrap();
        tokio::fs::write(outside.join("hidden.txt"), "needle\n")
            .await
            .unwrap();
        create_directory_symlink(&outside, &root.join("linked")).unwrap();

        let listed = list_files_tool(&root)
            .execute_with_tool_input(
                input(serde_json::json!({
                    "glob": "**/*.txt",
                    "includeDirs": true,
                    "limit": 10,
                })),
                context(&root).await,
            )
            .await
            .expect("list should skip symbolic link directories");
        let listed: serde_json::Value = serde_json::from_str(&listed.canonical_output()).unwrap();

        assert_eq!(listed["files"], serde_json::json!(["visible.txt"]));
        remove_directory_symlink(&root.join("linked")).unwrap();
        let _ = tokio::fs::remove_dir_all(root).await;
        let _ = tokio::fs::remove_dir_all(outside).await;
    }
}

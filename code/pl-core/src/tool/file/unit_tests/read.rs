use super::*;
use pretty_assertions::assert_eq;

fn read_output_json(output: &crate::tool::ToolResult) -> serde_json::Value {
    serde_json::from_str(&output.canonical_output()).unwrap()
}

#[tokio::test]
async fn stat_path_reports_missing_workspace_path_without_tool_failure() {
    let root = unique_temp_dir("stat-missing");
    tokio::fs::create_dir_all(&root).await.unwrap();

    let output = StatPathTool::new(tool_workspace(&root))
        .execute(
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
        .execute(
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
        .execute(
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
                .execute(input(arguments), context(&root).await)
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
        .execute(
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
    let tool_context = context(&root)
        .await
        .with_approval(crate::tool::ToolApprovalContext::new(
            crate::turn::PermissionMode::FullAccess,
            crate::tool::WorkspaceAccess::ExternalAllowed,
        ));

    let error = tool
        .execute(
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
        .execute(
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
        .execute(
            input(serde_json::json!({ "path": "linked/real.txt" })),
            context(&root).await,
        )
        .await
        .unwrap_err()
        .to_string();
    assert!(read_error.contains("symbolic link"));

    let stat_error = StatPathTool::new(tool_workspace(&root))
        .execute(
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

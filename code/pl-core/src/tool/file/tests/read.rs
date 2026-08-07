use super::*;
use crate::tool::StatPathTool;
use pretty_assertions::assert_eq;

fn read_output_text(output: &crate::tool::ToolOutput) -> String {
    serde_json::from_str::<serde_json::Value>(&output.description)
        .unwrap()
        .get("text")
        .and_then(serde_json::Value::as_str)
        .unwrap()
        .to_string()
}

fn read_output_json(output: &crate::tool::ToolOutput) -> serde_json::Value {
    serde_json::from_str(&output.description).unwrap()
}

#[tokio::test]
async fn stat_path_reports_missing_workspace_path_without_tool_failure() {
    let root = unique_temp_dir("stat-missing");
    tokio::fs::create_dir_all(&root).await.unwrap();

    let output = StatPathTool
        .execute(
            input(serde_json::json!({ "path": "design" })),
            context(&root).await,
        )
        .await
        .unwrap();

    assert_eq!(
        read_output_json(&output),
        serde_json::json!({
            "path": "design",
            "exists": false,
        })
    );
    let _ = tokio::fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn stat_path_missing_target_still_rejects_workspace_escape() {
    let root = unique_temp_dir("stat-missing-escape");
    tokio::fs::create_dir_all(&root).await.unwrap();

    let error = StatPathTool
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
async fn read_file_uses_unified_camel_case_json_output() {
    let root = unique_temp_dir("unified-read");
    let tool = read_file_tool();
    tokio::fs::create_dir_all(&root).await.unwrap();
    tokio::fs::write(root.join("a.txt"), "one\ntwo\nthree\n")
        .await
        .unwrap();

    let output = tool
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
    let value: serde_json::Value = serde_json::from_str(&output.description).unwrap();

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
async fn read_file_rejects_invalid_line_bounds_instead_of_clamping() {
    let root = unique_temp_dir("invalid-read-bounds");
    let tool = read_file_tool();
    tokio::fs::create_dir_all(&root).await.unwrap();
    tokio::fs::write(root.join("a.txt"), "one\n").await.unwrap();

    for arguments in [
        serde_json::json!({"path": "a.txt", "startLine": 0}),
        serde_json::json!({"path": "a.txt", "maxLines": 501}),
    ] {
        assert!(
            tool.execute(input(arguments), context(&root).await)
                .await
                .is_err()
        );
    }
    let _ = tokio::fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn read_file_missing_path_suggests_workspace_discovery() {
    let root = unique_temp_dir("missing-read-path");
    let tool = read_file_tool();
    let candidate = root.join("code/pl-model/src/request.rs");
    tokio::fs::create_dir_all(candidate.parent().unwrap())
        .await
        .unwrap();
    tokio::fs::write(&candidate, "pub struct Request;\n")
        .await
        .unwrap();
    let error = tool
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
    assert!(!error.contains("Do not repeat"), "{error}");
    let _ = tokio::fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn read_file_rejects_workspace_escape() {
    let root = unique_temp_dir("escape");
    let tool = read_file_tool();
    tokio::fs::create_dir_all(&root).await.unwrap();
    let result = tool
        .execute(
            input(serde_json::json!({ "path": "../outside.txt" })),
            context(&root).await,
        )
        .await;

    assert!(result.is_err());
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
    let mut tool_context = context(&root).await;
    tool_context.workspace = crate::tool::AgentWorkspace::confined(
        root.clone(),
        crate::tool::WorkspaceMutability::ReadWrite,
    );
    tool_context.options = tool_context
        .options
        .with_permission_mode(crate::turn::PermissionMode::FullAccess);

    let error = read_file_tool()
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
async fn read_file_default_reads_whole_file() {
    let root = unique_temp_dir("default-read");
    let tool = read_file_tool();
    tokio::fs::create_dir_all(&root).await.unwrap();
    tokio::fs::write(root.join("a.txt"), "hello\nworld\n")
        .await
        .unwrap();
    let output = tool
        .execute(
            input(serde_json::json!({ "path": "a.txt" })),
            context(&root).await,
        )
        .await
        .unwrap();

    assert_eq!(read_output_text(&output), "hello\nworld\n");
    let _ = tokio::fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn read_file_line_offset_starts_at_given_line() {
    let root = unique_temp_dir("line-offset");
    let tool = read_file_tool();
    tokio::fs::create_dir_all(&root).await.unwrap();
    tokio::fs::write(root.join("a.txt"), "one\ntwo\nthree\n")
        .await
        .unwrap();
    let output = tool
        .execute(
            input(serde_json::json!({ "path": "a.txt", "startLine": 2 })),
            context(&root).await,
        )
        .await
        .unwrap();

    assert_eq!(read_output_text(&output), "two\nthree\n");
    let _ = tokio::fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn read_file_max_lines_limits_output() {
    let root = unique_temp_dir("max-lines");
    let tool = read_file_tool();
    tokio::fs::create_dir_all(&root).await.unwrap();
    tokio::fs::write(root.join("a.txt"), "one\ntwo\nthree\n")
        .await
        .unwrap();
    let output = tool
        .execute(
            input(serde_json::json!({
                "path": "a.txt",
                "startLine": 1,
                "maxLines": 2,
            })),
            context(&root).await,
        )
        .await
        .unwrap();

    assert_eq!(read_output_text(&output), "one\ntwo\n");
    let _ = tokio::fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn read_file_line_offset_out_of_range_errors() {
    let root = unique_temp_dir("offset-oob");
    let tool = read_file_tool();
    tokio::fs::create_dir_all(&root).await.unwrap();
    tokio::fs::write(root.join("a.txt"), "one\ntwo\n")
        .await
        .unwrap();
    let result = tool
        .execute(
            input(serde_json::json!({ "path": "a.txt", "startLine": 10 })),
            context(&root).await,
        )
        .await;

    let error = result.unwrap_err().to_string();
    assert!(error.contains("exceeds file length"));
    let _ = tokio::fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn read_file_trailing_newline_offset_at_end_returns_empty() {
    // "one\ntwo\n" 有 2 行；startLine=3（行数+1）返回空切片，不报错（对齐 codex）
    let root = unique_temp_dir("offset-end");
    let tool = read_file_tool();
    tokio::fs::create_dir_all(&root).await.unwrap();
    tokio::fs::write(root.join("a.txt"), "one\ntwo\n")
        .await
        .unwrap();
    let output = tool
        .execute(
            input(serde_json::json!({ "path": "a.txt", "startLine": 3 })),
            context(&root).await,
        )
        .await
        .unwrap();

    assert_eq!(read_output_text(&output), "");
    let _ = tokio::fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn read_file_rejects_directory() {
    let root = unique_temp_dir("reject-dir");
    let tool = read_file_tool();
    tokio::fs::create_dir_all(&root).await.unwrap();
    tokio::fs::create_dir(root.join("subdir")).await.unwrap();
    let result = tool
        .execute(
            input(serde_json::json!({ "path": "subdir" })),
            context(&root).await,
        )
        .await;

    let error = result.unwrap_err().to_string();
    assert!(error.contains("not a regular file"));
    let _ = tokio::fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn read_file_rejects_non_utf8() {
    let root = unique_temp_dir("reject-nonutf8");
    let tool = read_file_tool();
    tokio::fs::create_dir_all(&root).await.unwrap();
    tokio::fs::write(root.join("a.bin"), [0xffu8, 0xfe, 0x00, 0x01])
        .await
        .unwrap();
    let result = tool
        .execute(
            input(serde_json::json!({ "path": "a.bin" })),
            context(&root).await,
        )
        .await;

    assert!(result.is_err());
    let _ = tokio::fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn read_file_empty_file() {
    let root = unique_temp_dir("empty");
    let tool = read_file_tool();
    tokio::fs::create_dir_all(&root).await.unwrap();
    tokio::fs::write(root.join("empty.txt"), "").await.unwrap();
    let output = tool
        .execute(
            input(serde_json::json!({ "path": "empty.txt" })),
            context(&root).await,
        )
        .await
        .unwrap();

    assert_eq!(read_output_text(&output), "");
    let _ = tokio::fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn read_file_pages_large_output_by_lines() {
    let root = unique_temp_dir("trunc");
    let tool = read_file_tool();
    tokio::fs::create_dir_all(&root).await.unwrap();
    let big = (1..=600)
        .map(|line| format!("line-{line}\n"))
        .collect::<String>();
    tokio::fs::write(root.join("big.txt"), &big).await.unwrap();
    let output = tool
        .execute(
            input(serde_json::json!({ "path": "big.txt", "maxLines": 500 })),
            context(&root).await,
        )
        .await
        .unwrap();

    let value = read_output_json(&output);
    assert_eq!(value["startLine"], serde_json::json!(1));
    assert_eq!(value["endLine"], serde_json::json!(500));
    assert_eq!(value["nextStartLine"], serde_json::json!(501));
    assert!(value["text"].as_str().unwrap().contains("line-500\n"));
    let _ = tokio::fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn read_file_rejects_symlink() {
    let root = unique_temp_dir("reject-symlink");
    let outside = unique_temp_dir("reject-symlink-target");
    let tool = read_file_tool();
    tokio::fs::create_dir_all(&root).await.unwrap();
    tokio::fs::create_dir_all(&outside).await.unwrap();
    tokio::fs::write(outside.join("real.txt"), "content")
        .await
        .unwrap();
    create_directory_symlink(&outside, &root.join("linked")).unwrap();
    let result = tool
        .execute(
            input(serde_json::json!({ "path": "linked/real.txt" })),
            context(&root).await,
        )
        .await;

    let error = result.unwrap_err().to_string();
    assert!(error.contains("symbolic link"));
    let stat_error = StatPathTool
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

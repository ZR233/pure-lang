use super::*;
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
async fn read_file_uses_unified_camel_case_json_output() {
    let root = unique_temp_dir("unified-read");
    let tool = ReadFileTool::new();
    tokio::fs::create_dir_all(&root).await.unwrap();
    tokio::fs::write(root.join("a.txt"), "one\ntwo\nthree\n")
        .await
        .unwrap();

    let output = tool
        .execute(
            input(serde_json::json!({
                "path": "a.txt",
                "lineStart": 2,
                "lineCount": 1,
            })),
            context(&root).await,
        )
        .await
        .unwrap();
    let value: serde_json::Value = serde_json::from_str(&output.description).unwrap();

    assert_eq!(
        value,
        serde_json::json!({
            "path": "a.txt",
            "offset": 0,
            "bytesReturned": 4,
            "bytesOmitted": 0,
            "truncated": false,
            "nextOffset": null,
            "text": "two\n",
        })
    );
    let _ = tokio::fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn read_file_rejects_workspace_escape() {
    let root = unique_temp_dir("escape");
    let tool = ReadFileTool::new();
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
async fn write_and_read_file_roundtrip() {
    let root = unique_temp_dir("roundtrip");
    let write = WriteFileTool;
    let read = ReadFileTool::new();
    write
        .execute(
            input(serde_json::json!({
                "path": "notes/a.txt",
                "content": "hello\nworld\n",
                "mode": "create"
            })),
            context(&root).await,
        )
        .await
        .unwrap();

    let output = read
        .execute(
            input(serde_json::json!({
                "path": "notes/a.txt",
                "lineStart": 2,
                "lineCount": 1,
            })),
            context(&root).await,
        )
        .await
        .unwrap();

    assert_eq!(read_output_text(&output), "world\n");
    let _ = tokio::fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn read_file_default_reads_whole_file() {
    let root = unique_temp_dir("default-read");
    let tool = ReadFileTool::new();
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
    let tool = ReadFileTool::new();
    tokio::fs::create_dir_all(&root).await.unwrap();
    tokio::fs::write(root.join("a.txt"), "one\ntwo\nthree\n")
        .await
        .unwrap();
    let output = tool
        .execute(
            input(serde_json::json!({ "path": "a.txt", "lineStart": 2 })),
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
    let tool = ReadFileTool::new();
    tokio::fs::create_dir_all(&root).await.unwrap();
    tokio::fs::write(root.join("a.txt"), "one\ntwo\nthree\n")
        .await
        .unwrap();
    let output = tool
        .execute(
            input(serde_json::json!({
                "path": "a.txt",
                "lineStart": 1,
                "lineCount": 2,
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
    let tool = ReadFileTool::new();
    tokio::fs::create_dir_all(&root).await.unwrap();
    tokio::fs::write(root.join("a.txt"), "one\ntwo\n")
        .await
        .unwrap();
    let result = tool
        .execute(
            input(serde_json::json!({ "path": "a.txt", "lineStart": 10 })),
            context(&root).await,
        )
        .await;

    let error = result.unwrap_err().to_string();
    assert!(error.contains("exceeds file length"));
    let _ = tokio::fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn read_file_trailing_newline_offset_at_end_returns_empty() {
    // "one\ntwo\n" 有 2 行；lineStart=3（行数+1）返回空切片，不报错（对齐 codex）
    let root = unique_temp_dir("offset-end");
    let tool = ReadFileTool::new();
    tokio::fs::create_dir_all(&root).await.unwrap();
    tokio::fs::write(root.join("a.txt"), "one\ntwo\n")
        .await
        .unwrap();
    let output = tool
        .execute(
            input(serde_json::json!({ "path": "a.txt", "lineStart": 3 })),
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
    let tool = ReadFileTool::new();
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
    let tool = ReadFileTool::new();
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
    let tool = ReadFileTool::new();
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
async fn read_file_truncates_large_output() {
    let root = unique_temp_dir("trunc");
    let tool = ReadFileTool::new();
    tokio::fs::create_dir_all(&root).await.unwrap();
    let big = "a".repeat(2500);
    tokio::fs::write(root.join("big.txt"), &big).await.unwrap();
    let output = tool
        .execute(
            input(serde_json::json!({ "path": "big.txt", "maxBytes": 10 })),
            context(&root).await,
        )
        .await
        .unwrap();

    let value = read_output_json(&output);
    assert_eq!(value["text"], serde_json::json!("aaaaaaaaaa"));
    assert_eq!(value["truncated"], serde_json::json!(true));
    assert_eq!(value["nextOffset"], serde_json::json!(10));
    let _ = tokio::fs::remove_dir_all(root).await;
}

#[cfg(unix)]
#[tokio::test]
async fn read_file_rejects_symlink() {
    use std::os::unix::fs::symlink;
    let root = unique_temp_dir("reject-symlink");
    let tool = ReadFileTool::new();
    tokio::fs::create_dir_all(&root).await.unwrap();
    tokio::fs::write(root.join("real.txt"), "content")
        .await
        .unwrap();
    symlink("real.txt", root.join("link.txt")).unwrap();
    let result = tool
        .execute(
            input(serde_json::json!({ "path": "link.txt" })),
            context(&root).await,
        )
        .await;

    let error = result.unwrap_err().to_string();
    assert!(error.contains("symbolic link"));
    let _ = tokio::fs::remove_dir_all(root).await;
}

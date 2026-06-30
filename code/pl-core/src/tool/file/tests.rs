use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use crate::tool::{Tool, ToolContext, ToolInput};
use crate::turn::TurnOptions;
use pretty_assertions::assert_eq;

use super::read::{ReadFileTool, SearchFilesTool};
use super::write::WriteFileTool;
use super::{ApplyPatchTool, CopyPathTool, DeletePathTool, MovePathTool};

fn unique_temp_dir(name: &str) -> PathBuf {
    let id = std::time::SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!("pure-lang-{name}-{id}"))
}

async fn context(root: &Path) -> ToolContext {
    tokio::fs::create_dir_all(root).await.unwrap();
    let (event_tx, _event_rx) = tokio::sync::broadcast::channel(8);
    ToolContext {
        event_tx,
        options: TurnOptions::default(),
        workspace_access: crate::tool::WorkspaceAccess::WorkspaceOnly,
        mode: crate::turn::CompileMode::Auto,
        workspace_root: root.to_path_buf(),
        workspace_instructions: None,
        instruction_snapshot: None,
        active_subagent: None,
        agent_control: crate::AgentControl::default(),
        lsp_runtime: None,
        parent_session: std::sync::Arc::new(crate::CoreSession::new()),
    }
}

fn input(arguments: serde_json::Value) -> ToolInput {
    ToolInput {
        arguments,
        session_id: "session".to_string(),
        tool_id: "tool".to_string(),
        revision_base: 0,
    }
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
            input(serde_json::json!({ "path": "notes/a.txt", "lineOffset": 2, "maxLines": 1 })),
            context(&root).await,
        )
        .await
        .unwrap();

    assert_eq!(output.description, "world\n");
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

    assert_eq!(output.description, "hello\nworld\n");
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
            input(serde_json::json!({ "path": "a.txt", "lineOffset": 2 })),
            context(&root).await,
        )
        .await
        .unwrap();

    assert_eq!(output.description, "two\nthree\n");
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
            input(serde_json::json!({ "path": "a.txt", "maxLines": 2 })),
            context(&root).await,
        )
        .await
        .unwrap();

    assert_eq!(output.description, "one\ntwo\n");
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
            input(serde_json::json!({ "path": "a.txt", "lineOffset": 10 })),
            context(&root).await,
        )
        .await;

    let error = result.unwrap_err().to_string();
    assert!(error.contains("exceeds file length"));
    let _ = tokio::fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn read_file_trailing_newline_offset_at_end_returns_empty() {
    // "one\ntwo\n" 有 2 行；lineOffset=3（行数+1）返回空切片，不报错（对齐 codex）
    let root = unique_temp_dir("offset-end");
    let tool = ReadFileTool::new();
    tokio::fs::create_dir_all(&root).await.unwrap();
    tokio::fs::write(root.join("a.txt"), "one\ntwo\n")
        .await
        .unwrap();
    let output = tool
        .execute(
            input(serde_json::json!({ "path": "a.txt", "lineOffset": 3 })),
            context(&root).await,
        )
        .await
        .unwrap();

    assert_eq!(output.description, "");
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

    assert_eq!(output.description, "");
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
            input(serde_json::json!({ "path": "big.txt" })),
            context(&root).await,
        )
        .await
        .unwrap();

    assert!(output.description.contains("Output was truncated"));
    assert!(output.description.contains("characters omitted"));
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

#[tokio::test]
async fn write_file_waits_for_workspace_write_lock() {
    let root = unique_temp_dir("write-lock-tool");
    let context = context(&root).await;
    let guard = context.workspace_write_lock().await;
    let tool = WriteFileTool;
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
    let tool = DeletePathTool;

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
    let copy = CopyPathTool;
    let move_tool = MovePathTool;

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

#[test]
fn file_tool_schemas_do_not_expose_legacy_bool_fields() {
    let delete_schema = DeletePathTool.input_schema();
    let copy_schema = CopyPathTool.input_schema();
    let move_schema = MovePathTool.input_schema();

    assert!(delete_schema["properties"].get("mode").is_some());
    assert!(delete_schema["properties"].get("recursive").is_none());
    assert!(copy_schema["properties"].get("collision").is_some());
    assert!(copy_schema["properties"].get("overwrite").is_none());
    assert!(move_schema["properties"].get("collision").is_some());
    assert!(move_schema["properties"].get("overwrite").is_none());
}

#[test]
fn search_files_schema_uses_pattern_for_content_and_file_pattern_for_paths() {
    let schema = SearchFilesTool.input_schema();

    assert!(schema["properties"].get("pattern").is_some());
    assert!(schema["properties"].get("filePattern").is_some());
    assert!(schema["properties"].get("query").is_none());
    assert_eq!(schema["required"], serde_json::json!(["pattern"]));
}

#[tokio::test]
async fn search_files_accepts_pattern_as_search_text() {
    let root = unique_temp_dir("search-pattern-text");
    tokio::fs::create_dir_all(root.join("src")).await.unwrap();
    tokio::fs::write(root.join("src/args.rs"), "fn parse_args() {}\n")
        .await
        .unwrap();
    let tool = SearchFilesTool;

    let output = tool
        .execute(
            input(serde_json::json!({
                "path": "src/args.rs",
                "pattern": "fn parse_args"
            })),
            context(&root).await,
        )
        .await
        .unwrap();

    assert!(output.description.contains("src"));
    assert!(output.description.contains("args.rs:1: fn parse_args() {}"));
    let _ = tokio::fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn search_files_file_pattern_filters_paths() {
    let root = unique_temp_dir("search-file-pattern");
    tokio::fs::create_dir_all(root.join("src")).await.unwrap();
    tokio::fs::write(root.join("src/lib.rs"), "needle\n")
        .await
        .unwrap();
    tokio::fs::write(root.join("src/readme.txt"), "needle\n")
        .await
        .unwrap();
    let tool = SearchFilesTool;

    let output = tool
        .execute(
            input(serde_json::json!({
                "path": "src",
                "pattern": "needle",
                "filePattern": "*.rs"
            })),
            context(&root).await,
        )
        .await
        .unwrap();

    assert!(output.description.contains("lib.rs:1: needle"));
    assert!(!output.description.contains("readme.txt"));
    let _ = tokio::fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn apply_patch_adds_file() {
    let root = unique_temp_dir("patch-add");
    let tool = ApplyPatchTool;
    let patch = "*** Begin Patch\n*** Add File: src/lib.rs\n+pub fn ok() {}\n*** End Patch";

    let output = tool
        .execute(
            input(serde_json::json!({ "patch": patch })),
            context(&root).await,
        )
        .await
        .unwrap();

    assert!(
        output.description.contains("A src\\lib.rs") || output.description.contains("A src/lib.rs")
    );
    assert_eq!(
        tokio::fs::read_to_string(root.join("src/lib.rs"))
            .await
            .unwrap(),
        "pub fn ok() {}\n"
    );
    let _ = tokio::fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn apply_patch_context_mismatch_does_not_write() {
    let root = unique_temp_dir("patch-mismatch");
    tokio::fs::create_dir_all(root.join("src")).await.unwrap();
    tokio::fs::write(root.join("src/lib.rs"), "old\n")
        .await
        .unwrap();
    let tool = ApplyPatchTool;
    let patch = "*** Begin Patch\n*** Update File: src/lib.rs\n@@\n-missing\n+new\n*** End Patch";

    let result = tool
        .execute(
            input(serde_json::json!({ "patch": patch })),
            context(&root).await,
        )
        .await;

    assert!(result.is_err());
    let error = result.unwrap_err().to_string();
    assert!(error.contains("Recovery: read the target file again"));
    assert!(error.contains("Do not repeat the same failed patch"));
    assert_eq!(
        tokio::fs::read_to_string(root.join("src/lib.rs"))
            .await
            .unwrap(),
        "old\n"
    );
    let _ = tokio::fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn apply_patch_accepts_wrapped_single_patch_block() {
    let root = unique_temp_dir("patch-wrapper");
    let tool = ApplyPatchTool;
    let patch = "Here is the patch:\n```patch\n*** Begin Patch\n*** Add File: wrapped.txt\n+ok\n*** End Patch\n```";

    tool.execute(
        input(serde_json::json!({ "patch": patch })),
        context(&root).await,
    )
    .await
    .unwrap();

    assert_eq!(
        tokio::fs::read_to_string(root.join("wrapped.txt"))
            .await
            .unwrap(),
        "ok\n"
    );
    let _ = tokio::fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn apply_patch_accepts_heredoc_wrappers() {
    let root = unique_temp_dir("patch-heredoc-wrapper");
    let tool = ApplyPatchTool;
    for (index, start) in ["<<EOF", "<<'EOF'", "<<\"EOF\""].into_iter().enumerate() {
        let path = format!("wrapped-{index}.txt");
        let patch =
            format!("{start}\n*** Begin Patch\n*** Add File: {path}\n+ok\n*** End Patch\nEOF");

        tool.execute(
            input(serde_json::json!({ "patch": patch })),
            context(&root).await,
        )
        .await
        .unwrap();

        assert_eq!(
            tokio::fs::read_to_string(root.join(path)).await.unwrap(),
            "ok\n"
        );
    }
    let _ = tokio::fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn apply_patch_rejects_mismatched_heredoc_wrapper() {
    let root = unique_temp_dir("patch-mismatched-heredoc");
    let tool = ApplyPatchTool;
    let result = tool
        .execute(
            input(serde_json::json!({
                "patch": "<<\"EOF'\n*** Begin Patch\n*** Add File: bad.txt\n+nope\n*** End Patch\nEOF"
            })),
            context(&root).await,
        )
        .await
        .unwrap_err();

    assert!(result.to_string().contains("first line must be"));
    assert!(!tokio::fs::try_exists(root.join("bad.txt")).await.unwrap());
    let _ = tokio::fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn apply_patch_accepts_environment_id_preamble() {
    let root = unique_temp_dir("patch-environment-id");
    let tool = ApplyPatchTool;
    let patch =
        "*** Begin Patch\n*** Environment ID: remote\n*** Add File: env.txt\n+ok\n*** End Patch";

    tool.execute(
        input(serde_json::json!({ "patch": patch })),
        context(&root).await,
    )
    .await
    .unwrap();

    assert_eq!(
        tokio::fs::read_to_string(root.join("env.txt"))
            .await
            .unwrap(),
        "ok\n"
    );
    let _ = tokio::fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn apply_patch_rejects_empty_environment_id_preamble() {
    let root = unique_temp_dir("patch-empty-environment-id");
    let tool = ApplyPatchTool;
    let result = tool
        .execute(
            input(serde_json::json!({
                "patch": "*** Begin Patch\n*** Environment ID:   \n*** Add File: env.txt\n+ok\n*** End Patch"
            })),
            context(&root).await,
        )
        .await
        .unwrap_err();

    assert!(
        result
            .to_string()
            .contains("environment_id cannot be empty")
    );
    assert!(!tokio::fs::try_exists(root.join("env.txt")).await.unwrap());
    let _ = tokio::fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn apply_patch_adds_empty_file() {
    let root = unique_temp_dir("patch-empty-add");
    let tool = ApplyPatchTool;
    let patch = "*** Begin Patch\n*** Add File: empty.txt\n*** End Patch";

    tool.execute(
        input(serde_json::json!({ "patch": patch })),
        context(&root).await,
    )
    .await
    .unwrap();

    assert_eq!(
        tokio::fs::read_to_string(root.join("empty.txt"))
            .await
            .unwrap(),
        ""
    );
    let _ = tokio::fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn apply_patch_accepts_whitespace_padded_markers() {
    let root = unique_temp_dir("patch-padded-markers");
    tokio::fs::create_dir_all(&root).await.unwrap();
    tokio::fs::write(root.join("file.txt"), "one\n")
        .await
        .unwrap();
    let tool = ApplyPatchTool;
    let patch = " *** Begin Patch\n  *** Update File: file.txt\n@@\n-one\n+two\n *** End Patch ";

    tool.execute(
        input(serde_json::json!({ "patch": patch })),
        context(&root).await,
    )
    .await
    .unwrap();

    assert_eq!(
        tokio::fs::read_to_string(root.join("file.txt"))
            .await
            .unwrap(),
        "two\n"
    );
    let _ = tokio::fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn apply_patch_matches_unicode_punctuation_context() {
    let root = unique_temp_dir("patch-unicode-context");
    tokio::fs::create_dir_all(&root).await.unwrap();
    tokio::fs::write(
        root.join("unicode.txt"),
        "import asyncio  # local import \u{2013} avoids top\u{2011}level dep\nlet quote = \u{201C}ok\u{201D}\nspace = \"a\u{00A0}b\"\n",
    )
    .await
    .unwrap();
    let tool = ApplyPatchTool;
    let patch = "*** Begin Patch\n*** Update File: unicode.txt\n@@\n-import asyncio  # local import - avoids top-level dep\n-let quote = \"ok\"\n-space = \"a b\"\n+done\n*** End Patch";

    tool.execute(
        input(serde_json::json!({ "patch": patch })),
        context(&root).await,
    )
    .await
    .unwrap();

    assert_eq!(
        tokio::fs::read_to_string(root.join("unicode.txt"))
            .await
            .unwrap(),
        "done\n"
    );
    let _ = tokio::fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn apply_patch_skips_blank_lines_between_update_chunks() {
    let root = unique_temp_dir("patch-blank-between-chunks");
    tokio::fs::create_dir_all(&root).await.unwrap();
    tokio::fs::write(root.join("file.txt"), "one\n")
        .await
        .unwrap();
    let tool = ApplyPatchTool;
    let patch = "*** Begin Patch\n*** Update File: file.txt\n\n@@\n-one\n+two\n*** End Patch";

    tool.execute(
        input(serde_json::json!({ "patch": patch })),
        context(&root).await,
    )
    .await
    .unwrap();

    assert_eq!(
        tokio::fs::read_to_string(root.join("file.txt"))
            .await
            .unwrap(),
        "two\n"
    );
    let _ = tokio::fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn apply_patch_supports_deletion_only_update_and_eof_marker() {
    let root = unique_temp_dir("patch-delete-and-eof");
    tokio::fs::create_dir_all(&root).await.unwrap();
    tokio::fs::write(root.join("lines.txt"), "line1\nline2\nline3\n")
        .await
        .unwrap();
    tokio::fs::write(root.join("tail.txt"), "first\nsecond\n")
        .await
        .unwrap();
    let tool = ApplyPatchTool;
    let patch = "*** Begin Patch\n*** Update File: lines.txt\n@@\n line1\n-line2\n line3\n*** Update File: tail.txt\n@@\n first\n-second\n+second updated\n\n*** End of File\n*** End Patch";

    tool.execute(
        input(serde_json::json!({ "patch": patch })),
        context(&root).await,
    )
    .await
    .unwrap();

    assert_eq!(
        tokio::fs::read_to_string(root.join("lines.txt"))
            .await
            .unwrap(),
        "line1\nline3\n"
    );
    assert_eq!(
        tokio::fs::read_to_string(root.join("tail.txt"))
            .await
            .unwrap(),
        "first\nsecond updated\n"
    );
    let _ = tokio::fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn apply_patch_rejects_missing_end_marker() {
    let root = unique_temp_dir("patch-missing-end");
    let tool = ApplyPatchTool;
    let result = tool
        .execute(
            input(serde_json::json!({
                "patch": "*** Begin Patch\n*** Add File: missing.txt\n+nope"
            })),
            context(&root).await,
        )
        .await
        .unwrap_err();

    let error = result.to_string();
    assert!(error.contains("last line must be"));
    assert!(error.contains("send the complete patch"));
    assert!(error.contains("Recovery: read the target file again"));
    let _ = tokio::fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn apply_patch_eof_marker_falls_back_to_normal_search() {
    let root = unique_temp_dir("patch-eof-fallback");
    tokio::fs::create_dir_all(&root).await.unwrap();
    tokio::fs::write(root.join("lines.txt"), "first\nmiddle\nlast\n")
        .await
        .unwrap();
    let tool = ApplyPatchTool;
    let patch = "*** Begin Patch\n*** Update File: lines.txt\n@@\n-middle\n+updated\n*** End of File\n*** End Patch";

    tool.execute(
        input(serde_json::json!({ "patch": patch })),
        context(&root).await,
    )
    .await
    .unwrap();

    assert_eq!(
        tokio::fs::read_to_string(root.join("lines.txt"))
            .await
            .unwrap(),
        "first\nupdated\nlast\n"
    );
    let _ = tokio::fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn apply_patch_rejects_unified_diff_header() {
    let root = unique_temp_dir("patch-unified");
    let tool = ApplyPatchTool;
    let result = tool
        .execute(
            input(serde_json::json!({
                "patch": "*** Begin Patch\n--- a/file.txt\n+++ b/file.txt\n@@ -1 +1 @@\n-old\n+new\n*** End Patch"
            })),
            context(&root).await,
        )
        .await
        .unwrap_err();

    let error = result.to_string();
    assert!(error.contains("unified diff"));
    assert!(error.contains("*** Update File:"));
    assert!(error.contains("Recovery: read the target file again"));
    let _ = tokio::fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn apply_patch_rejects_file_metadata_header() {
    let root = unique_temp_dir("patch-file-header");
    let tool = ApplyPatchTool;
    let result = tool
        .execute(
            input(serde_json::json!({
                "patch": "*** Begin Patch\n*** File: src/lib.rs\n*** End Patch"
            })),
            context(&root).await,
        )
        .await
        .unwrap_err();

    let error = result.to_string();
    assert!(error.contains("*** File:"));
    assert!(error.contains("*** Update File:"));
    assert!(error.contains("Recovery: read the target file again"));
    let _ = tokio::fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn apply_patch_rejects_natural_language_instruction_with_recovery_guidance() {
    let root = unique_temp_dir("patch-natural-language");
    let tool = ApplyPatchTool;
    let result = tool
        .execute(
            input(serde_json::json!({
                "patch": "*** Begin Patch\nInsert after '<meta name=\"viewport\">':\n+<script></script>\n*** End Patch"
            })),
            context(&root).await,
        )
        .await
        .unwrap_err();

    let error = result.to_string();
    assert!(error.contains("natural-language edit instructions"));
    assert!(error.contains("*** Update File:"));
    assert!(error.contains("Recovery: read the target file again"));
    let _ = tokio::fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn apply_patch_move_only_update_moves_file() {
    let root = unique_temp_dir("patch-move-only");
    tokio::fs::create_dir_all(root.join("old")).await.unwrap();
    tokio::fs::write(root.join("old/name.txt"), "same\n")
        .await
        .unwrap();
    let tool = ApplyPatchTool;
    let patch =
        "*** Begin Patch\n*** Update File: old/name.txt\n*** Move to: new/name.txt\n*** End Patch";

    tool.execute(
        input(serde_json::json!({ "patch": patch })),
        context(&root).await,
    )
    .await
    .unwrap();

    assert!(
        !tokio::fs::try_exists(root.join("old/name.txt"))
            .await
            .unwrap()
    );
    assert_eq!(
        tokio::fs::read_to_string(root.join("new/name.txt"))
            .await
            .unwrap(),
        "same\n"
    );
    let _ = tokio::fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn apply_patch_appends_pure_addition_chunk_to_eof() {
    let root = unique_temp_dir("patch-pure-addition-eof");
    tokio::fs::create_dir_all(&root).await.unwrap();
    tokio::fs::write(
        root.join("page.html"),
        "<head>\n<title>x</title>\n</head>\n",
    )
    .await
    .unwrap();
    let tool = ApplyPatchTool;
    let patch =
        "*** Begin Patch\n*** Update File: page.html\n@@ <head>\n+<script></script>\n*** End Patch";

    tool.execute(
        input(serde_json::json!({ "patch": patch })),
        context(&root).await,
    )
    .await
    .unwrap();

    assert_eq!(
        tokio::fs::read_to_string(root.join("page.html"))
            .await
            .unwrap(),
        "<head>\n<title>x</title>\n</head>\n<script></script>\n"
    );
    let _ = tokio::fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn apply_patch_accepts_indented_context_without_extra_control_space() {
    let root = unique_temp_dir("patch-indented-context");
    tokio::fs::create_dir_all(&root).await.unwrap();
    tokio::fs::write(root.join("style.html"), "<style>\n    </style>\n")
        .await
        .unwrap();
    let tool = ApplyPatchTool;
    let patch =
        "*** Begin Patch\n*** Update File: style.html\n@@\n   </style>\n+tail\n*** End Patch";

    tool.execute(
        input(serde_json::json!({ "patch": patch })),
        context(&root).await,
    )
    .await
    .unwrap();

    assert_eq!(
        tokio::fs::read_to_string(root.join("style.html"))
            .await
            .unwrap(),
        "<style>\n    </style>\ntail\n"
    );
    let _ = tokio::fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn apply_patch_accepts_unprefixed_zero_indent_context_line() {
    let root = unique_temp_dir("patch-unprefixed-zero-indent-context");
    tokio::fs::create_dir_all(&root).await.unwrap();
    tokio::fs::write(root.join("page.html"), "<body>\nold\n</body>\n")
        .await
        .unwrap();
    let tool = ApplyPatchTool;
    let patch =
        "*** Begin Patch\n*** Update File: page.html\n@@\n-old\n+new\n</body>\n*** End Patch";

    tool.execute(
        input(serde_json::json!({ "patch": patch })),
        context(&root).await,
    )
    .await
    .unwrap();

    assert_eq!(
        tokio::fs::read_to_string(root.join("page.html"))
            .await
            .unwrap(),
        "<body>\nnew\n</body>\n"
    );
    let _ = tokio::fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn apply_patch_accepts_html_insert_before_unprefixed_body_context() {
    let root = unique_temp_dir("patch-html-body-context");
    tokio::fs::create_dir_all(&root).await.unwrap();
    tokio::fs::write(
        root.join("page.html"),
        "<footer>\n  <p>done</p>\n</footer>\n\n</body>\n",
    )
    .await
    .unwrap();
    let tool = ApplyPatchTool;
    let patch = "*** Begin Patch\n*** Update File: page.html\n@@\n <footer>\n   <p>done</p>\n </footer>\n\n+<script type=\"module\">\n+console.log('ready');\n+</script>\n</body>\n*** End Patch";

    tool.execute(
        input(serde_json::json!({ "patch": patch })),
        context(&root).await,
    )
    .await
    .unwrap();

    assert_eq!(
        tokio::fs::read_to_string(root.join("page.html"))
            .await
            .unwrap(),
        "<footer>\n  <p>done</p>\n</footer>\n\n<script type=\"module\">\nconsole.log('ready');\n</script>\n</body>\n"
    );
    let _ = tokio::fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn apply_patch_collapses_duplicated_edge_context_for_insert_before() {
    let root = unique_temp_dir("patch-duplicated-edge-context");
    tokio::fs::create_dir_all(&root).await.unwrap();
    tokio::fs::write(root.join("deepseek-intro.html"), "<style>\n    </style>\n")
        .await
        .unwrap();
    let tool = ApplyPatchTool;
    let patch = "*** Begin Patch\n*** Update File: deepseek-intro.html\n@@\n    </style>\n+        .cube { display: block; }\n     </style>\n*** End Patch";

    tool.execute(
        input(serde_json::json!({ "patch": patch })),
        context(&root).await,
    )
    .await
    .unwrap();

    assert_eq!(
        tokio::fs::read_to_string(root.join("deepseek-intro.html"))
            .await
            .unwrap(),
        "<style>\n        .cube { display: block; }\n    </style>\n"
    );
    let _ = tokio::fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn apply_patch_applies_repeated_update_hunks_in_order() {
    let root = unique_temp_dir("patch-repeated-update-target");
    tokio::fs::create_dir_all(&root).await.unwrap();
    tokio::fs::write(root.join("src.rs"), "one\ntwo\nthree\n")
        .await
        .unwrap();
    let tool = ApplyPatchTool;
    let patch = "*** Begin Patch\n*** Update File: src.rs\n@@\n-one\n+first\n*** Update File: src.rs\n@@\n-first\n+second\n*** End Patch";

    tool.execute(
        input(serde_json::json!({ "patch": patch })),
        context(&root).await,
    )
    .await
    .unwrap();

    assert_eq!(
        tokio::fs::read_to_string(root.join("src.rs"))
            .await
            .unwrap(),
        "second\ntwo\nthree\n"
    );
    let _ = tokio::fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn apply_patch_add_overwrites_existing_file() {
    let root = unique_temp_dir("patch-add-overwrite");
    tokio::fs::create_dir_all(&root).await.unwrap();
    tokio::fs::write(root.join("duplicate.txt"), "old\n")
        .await
        .unwrap();
    let tool = ApplyPatchTool;
    let patch = "*** Begin Patch\n*** Add File: duplicate.txt\n+new\n*** End Patch";

    let output = tool
        .execute(
            input(serde_json::json!({ "patch": patch })),
            context(&root).await,
        )
        .await
        .unwrap();

    assert!(output.description.contains("A duplicate.txt"));
    assert_eq!(
        tokio::fs::read_to_string(root.join("duplicate.txt"))
            .await
            .unwrap(),
        "new\n"
    );
    let _ = tokio::fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn apply_patch_move_overwrites_existing_target() {
    let root = unique_temp_dir("patch-move-overwrite");
    tokio::fs::create_dir_all(root.join("old")).await.unwrap();
    tokio::fs::create_dir_all(root.join("new")).await.unwrap();
    tokio::fs::write(root.join("old/name.txt"), "from\n")
        .await
        .unwrap();
    tokio::fs::write(root.join("new/name.txt"), "existing\n")
        .await
        .unwrap();
    let tool = ApplyPatchTool;
    let patch = "*** Begin Patch\n*** Update File: old/name.txt\n*** Move to: new/name.txt\n@@\n-from\n+to\n*** End Patch";

    tool.execute(
        input(serde_json::json!({ "patch": patch })),
        context(&root).await,
    )
    .await
    .unwrap();

    assert!(
        !tokio::fs::try_exists(root.join("old/name.txt"))
            .await
            .unwrap()
    );
    assert_eq!(
        tokio::fs::read_to_string(root.join("new/name.txt"))
            .await
            .unwrap(),
        "to\n"
    );
    let _ = tokio::fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn apply_patch_failure_keeps_committed_prefix() {
    let root = unique_temp_dir("patch-prefix-failure");
    tokio::fs::create_dir_all(&root).await.unwrap();
    let tool = ApplyPatchTool;
    let patch = "*** Begin Patch\n*** Add File: created.txt\n+hello\n*** Update File: missing.txt\n@@\n-old\n+new\n*** End Patch";

    let result = tool
        .execute(
            input(serde_json::json!({ "patch": patch })),
            context(&root).await,
        )
        .await
        .unwrap_err();

    let error = result.to_string();
    assert!(error.contains("failed to resolve path 'missing.txt'"));
    assert!(error.contains("Committed changes before failure"));
    assert!(error.contains("A created.txt"));
    assert_eq!(
        tokio::fs::read_to_string(root.join("created.txt"))
            .await
            .unwrap(),
        "hello\n"
    );
    let _ = tokio::fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn apply_patch_applies_add_then_update_in_order() {
    let root = unique_temp_dir("patch-add-then-update");
    tokio::fs::create_dir_all(&root).await.unwrap();
    tokio::fs::write(root.join("notes.txt"), "old\n")
        .await
        .unwrap();
    let tool = ApplyPatchTool;
    let patch = "*** Begin Patch\n*** Add File: notes.txt\n+new\n*** Update File: notes.txt\n@@\n-new\n+newer\n*** End Patch";

    let output = tool
        .execute(
            input(serde_json::json!({ "patch": patch })),
            context(&root).await,
        )
        .await
        .unwrap();

    assert!(output.description.contains("A notes.txt"));
    assert!(output.description.contains("M notes.txt"));
    assert_eq!(
        tokio::fs::read_to_string(root.join("notes.txt"))
            .await
            .unwrap(),
        "newer\n"
    );
    let _ = tokio::fs::remove_dir_all(root).await;
}

use super::*;
use pretty_assertions::assert_eq;

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

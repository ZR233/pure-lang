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
fn file_tool_schemas_use_unified_camel_case_inputs() {
    let read_schema = read_file_tool().input_schema();
    let list_schema = list_files_tool().input_schema();
    let search_schema = search_files_tool().input_schema();

    assert!(read_schema["properties"].get("lineStart").is_some());
    assert!(read_schema["properties"].get("line_start").is_none());
    assert!(list_schema["properties"].get("maxFiles").is_some());
    assert!(list_schema["properties"].get("max_files").is_none());
    assert!(list_schema["properties"].get("depth").is_none());
    assert!(search_schema["properties"].get("query").is_some());
    assert!(search_schema["properties"].get("glob").is_some());
    assert!(search_schema["properties"].get("pattern").is_none());
    assert!(search_schema["properties"].get("filePattern").is_none());
    assert_eq!(search_schema["required"], serde_json::json!(["query"]));
}

#[tokio::test]
async fn list_files_returns_empty_for_missing_workspace_directory() {
    let root = unique_temp_dir("list-missing-directory");
    tokio::fs::create_dir_all(&root).await.unwrap();
    let tool = list_files_tool();

    let output = tool
        .execute(
            input(serde_json::json!({
                "path": "design",
                "includeDirs": true,
            })),
            context(&root).await,
        )
        .await
        .unwrap();

    let value: serde_json::Value = serde_json::from_str(&output.description).unwrap();
    assert_eq!(value["files"], serde_json::json!([]));
    assert_eq!(value["count"], serde_json::json!(0));
    assert_eq!(value["truncated"], serde_json::json!(false));
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
    let tool = list_files_tool();

    let output = tool
        .execute(
            input(serde_json::json!({
                "path": "code",
                "glob": "*/",
                "includeDirs": true,
                "maxFiles": 10,
            })),
            context(&root).await,
        )
        .await
        .unwrap();

    let value: serde_json::Value = serde_json::from_str(&output.description).unwrap();
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
    let tool = list_files_tool();

    let output = tool
        .execute(
            input(serde_json::json!({
                "glob": "design/**/*.md",
                "maxFiles": 10,
            })),
            context(&root).await,
        )
        .await
        .unwrap();

    let value: serde_json::Value = serde_json::from_str(&output.description).unwrap();
    assert_eq!(
        value["files"],
        serde_json::json!(["design/nested/report.md", "design/overview.md"])
    );
    let _ = tokio::fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn search_files_accepts_pattern_as_search_text() {
    let root = unique_temp_dir("search-pattern-text");
    tokio::fs::create_dir_all(root.join("src")).await.unwrap();
    tokio::fs::write(root.join("src/args.rs"), "fn parse_args() {}\n")
        .await
        .unwrap();
    let tool = search_files_tool();

    let output = tool
        .execute(
            input(serde_json::json!({
                "path": "src/args.rs",
                "query": "fn parse_args",
                "literal": true
            })),
            context(&root).await,
        )
        .await
        .unwrap();

    let value: serde_json::Value = serde_json::from_str(&output.description).unwrap();
    assert_eq!(value["count"], serde_json::json!(1));
    assert_eq!(
        value["matches"][0],
        serde_json::json!({
            "path": "src/args.rs",
            "line": 1,
            "column": 1,
            "text": "fn parse_args() {}",
        })
    );
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
    let tool = search_files_tool();

    let output = tool
        .execute(
            input(serde_json::json!({
                "path": "src",
                "query": "needle",
                "glob": "*.rs",
                "literal": true
            })),
            context(&root).await,
        )
        .await
        .unwrap();

    let value: serde_json::Value = serde_json::from_str(&output.description).unwrap();
    assert_eq!(value["count"], serde_json::json!(1));
    assert_eq!(value["matches"][0]["path"], serde_json::json!("src/lib.rs"));
    let _ = tokio::fs::remove_dir_all(root).await;
}

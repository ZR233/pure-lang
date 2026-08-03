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
    let tool = list_files_tool();

    let output = tool
        .execute(
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

    let value: serde_json::Value = serde_json::from_str(&output.description).unwrap();
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
    let tool = list_files_tool();

    let output = tool
        .execute(
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
                "limit": 10,
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
async fn list_cursor_is_bound_to_workspace_epoch() {
    let root = unique_temp_dir("list-cursor-epoch");
    tokio::fs::create_dir_all(&root).await.unwrap();
    tokio::fs::write(root.join("a.txt"), "a").await.unwrap();
    tokio::fs::write(root.join("b.txt"), "b").await.unwrap();
    let tool = list_files_tool();
    let context = context(&root).await;
    let first = tool
        .execute(
            input(serde_json::json!({"path": ".", "limit": 1})),
            context.clone(),
        )
        .await
        .unwrap();
    let first: serde_json::Value = serde_json::from_str(&first.description).unwrap();
    let cursor = first["nextCursor"].as_str().unwrap().to_string();
    assert_eq!(first["workspaceEpoch"], 0);

    context
        .tool_cache
        .record_effect(Some(crate::ToolEffect::WorkspaceWrite), true);
    let error = tool
        .execute(
            input(serde_json::json!({"path": ".", "limit": 1, "cursor": cursor})),
            context,
        )
        .await
        .expect_err("old cursor must not page a mutated workspace");

    assert!(error.to_string().contains("cursor does not belong"));
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
        value["files"][0],
        serde_json::json!({
            "path": "src/args.rs",
            "matches": [{
                "line": 1,
                "column": 1,
                "text": "fn parse_args() {}",
            }],
        })
    );
    let _ = tokio::fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn search_files_treats_empty_path_as_current_directory() {
    let root = unique_temp_dir("search-empty-path");
    tokio::fs::create_dir_all(&root).await.unwrap();
    tokio::fs::write(root.join("AGENTS.md"), "project constraints\n")
        .await
        .unwrap();
    let tool = search_files_tool();

    let output = tool
        .execute(
            input(serde_json::json!({
                "path": "",
                "query": "project constraints",
                "literal": true
            })),
            context(&root).await,
        )
        .await
        .unwrap();

    let value: serde_json::Value = serde_json::from_str(&output.description).unwrap();
    assert_eq!(value["path"], serde_json::json!("."));
    assert_eq!(value["count"], serde_json::json!(1));
    assert_eq!(value["files"][0]["path"], serde_json::json!("AGENTS.md"));
    let _ = tokio::fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn list_and_search_treat_empty_cursor_as_first_page() {
    let root = unique_temp_dir("empty-cursor-first-page");
    tokio::fs::create_dir_all(&root).await.unwrap();
    tokio::fs::write(root.join("README.md"), "maintainers\n")
        .await
        .unwrap();

    let listed = list_files_tool()
        .execute(
            input(serde_json::json!({
                "path": "",
                "glob": "README*",
                "cursor": "",
            })),
            context(&root).await,
        )
        .await
        .expect("list first page");
    let searched = search_files_tool()
        .execute(
            input(serde_json::json!({
                "path": "",
                "query": "maintainers",
                "literal": true,
                "cursor": "   ",
            })),
            context(&root).await,
        )
        .await
        .expect("search first page");
    let listed: serde_json::Value = serde_json::from_str(&listed.description).unwrap();
    let searched: serde_json::Value = serde_json::from_str(&searched.description).unwrap();

    assert_eq!(listed["count"], 1);
    assert_eq!(searched["count"], 1);
    assert_eq!(listed["cursorReset"], false);
    assert_eq!(searched["cursorReset"], false);
    let _ = tokio::fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn malformed_cursor_is_explicitly_reset_to_first_page() {
    let root = unique_temp_dir("malformed-cursor-reset");
    tokio::fs::create_dir_all(&root).await.unwrap();
    tokio::fs::write(root.join("README.md"), "maintainers\n")
        .await
        .unwrap();

    let output = search_files_tool()
        .execute(
            input(serde_json::json!({
                "path": "",
                "query": "maintainers",
                "literal": true,
                "cursor": "x",
            })),
            context(&root).await,
        )
        .await
        .expect("reset malformed cursor");
    let value: serde_json::Value = serde_json::from_str(&output.description).unwrap();

    assert_eq!(value["count"], 1);
    assert_eq!(value["cursorReset"], true);
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
    assert_eq!(value["files"][0]["path"], serde_json::json!("src/lib.rs"));
    let _ = tokio::fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn list_and_search_skip_symbolic_link_directories() {
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

    let listed = list_files_tool()
        .execute(
            input(serde_json::json!({
                "glob": "**/*.txt",
                "includeDirs": true,
                "limit": 10,
            })),
            context(&root).await,
        )
        .await
        .expect("list should skip symbolic link directories");
    let searched = search_files_tool()
        .execute(
            input(serde_json::json!({
                "query": "needle",
                "literal": true,
                "limit": 10,
            })),
            context(&root).await,
        )
        .await
        .expect("search should skip symbolic link directories");
    let listed: serde_json::Value = serde_json::from_str(&listed.description).unwrap();
    let searched: serde_json::Value = serde_json::from_str(&searched.description).unwrap();

    assert_eq!(listed["files"], serde_json::json!(["visible.txt"]));
    assert_eq!(searched["count"], 1);
    assert_eq!(searched["files"][0]["path"], "visible.txt");
    remove_directory_symlink(&root.join("linked")).unwrap();
    let _ = tokio::fs::remove_dir_all(root).await;
    let _ = tokio::fs::remove_dir_all(outside).await;
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

    let write = WriteFileTool
        .execute(
            input(serde_json::json!({
                "path": "linked/new.txt",
                "content": "blocked",
                "mode": "create"
            })),
            context(&root).await,
        )
        .await;
    let create = super::super::CreateDirectoryTool
        .execute(
            input(serde_json::json!({ "path": "linked/new-directory" })),
            context(&root).await,
        )
        .await;
    let patch = apply_patch_tool()
        .execute(
            input(serde_json::json!({
                "input": "*** Begin Patch\n*** Add File: linked/patched.txt\n+blocked\n*** End Patch\n"
            })),
            context(&root).await,
        )
        .await;
    let copy_source = CopyPathTool
        .execute(
            input(serde_json::json!({
                "from": "linked/source.txt",
                "to": "copied.txt",
                "collision": "failIfExists"
            })),
            context(&root).await,
        )
        .await;
    let copy_target = CopyPathTool
        .execute(
            input(serde_json::json!({
                "from": "source.txt",
                "to": "linked/copied.txt",
                "collision": "failIfExists"
            })),
            context(&root).await,
        )
        .await;
    let move_target = MovePathTool
        .execute(
            input(serde_json::json!({
                "from": "source.txt",
                "to": "linked/moved.txt",
                "collision": "failIfExists"
            })),
            context(&root).await,
        )
        .await;
    let delete = DeletePathTool
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

    DeletePathTool
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

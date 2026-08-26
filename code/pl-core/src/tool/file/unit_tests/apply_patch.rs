use super::*;
use pretty_assertions::assert_eq;

#[tokio::test]
async fn apply_patch_uses_unified_input_and_json_output() {
    let root = unique_temp_dir("patch-unified-output");
    let patch = "*** Begin Patch\n*** Add File: src/lib.rs\n+pub fn ok() {}\n*** End Patch";

    let output = apply_patch_tool(&root)
        .execute(
            input(serde_json::json!({ "input": patch })),
            context(&root).await,
        )
        .await
        .unwrap();
    let value: serde_json::Value = serde_json::from_str(&output.canonical_output()).unwrap();

    assert_eq!(value["cwd"], serde_json::json!("."));
    assert_eq!(value["added"], serde_json::json!(["src/lib.rs"]));
    assert_eq!(value["changedFiles"], serde_json::json!(["src/lib.rs"]));
    assert_eq!(value["stdout"], serde_json::json!("apply_patch completed"));
    assert_eq!(
        tokio::fs::read_to_string(root.join("src/lib.rs"))
            .await
            .unwrap(),
        "pub fn ok() {}\n"
    );
    let _ = tokio::fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn apply_patch_rejects_read_only_workspace() {
    let root = unique_temp_dir("patch-read-only");
    let tool = LocalWorkspaceFileTool::new(
        WorkspaceFileToolKind::ApplyPatch,
        ToolWorkspace::new(crate::tool::AgentWorkspace::confined(
            root.clone(),
            crate::tool::WorkspaceMutability::ReadOnly,
        )),
    );
    let patch = "*** Begin Patch\n*** Add File: denied.txt\n+denied\n*** End Patch";

    let error = tool
        .execute(
            input(serde_json::json!({ "input": patch })),
            context(&root).await,
        )
        .await
        .unwrap_err()
        .to_string();

    assert!(error.contains("read-only"), "{error}");
    assert!(
        !tokio::fs::try_exists(root.join("denied.txt"))
            .await
            .unwrap()
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
    let patch = "*** Begin Patch\n*** Update File: src/lib.rs\n@@\n-missing\n+new\n*** End Patch";

    let error = apply_patch_tool(&root)
        .execute(
            input(serde_json::json!({ "input": patch })),
            context(&root).await,
        )
        .await
        .unwrap_err()
        .to_string();

    assert!(error.contains("Recovery: read the target file again"));
    assert_eq!(
        tokio::fs::read_to_string(root.join("src/lib.rs"))
            .await
            .unwrap(),
        "old\n"
    );
    let _ = tokio::fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn apply_patch_accepts_and_validates_heredoc_wrappers() {
    let root = unique_temp_dir("patch-heredoc-wrapper");
    for (index, start) in ["<<EOF", "<<'EOF'", "<<\"EOF\""].into_iter().enumerate() {
        let path = format!("wrapped-{index}.txt");
        let patch =
            format!("{start}\n*** Begin Patch\n*** Add File: {path}\n+ok\n*** End Patch\nEOF");
        apply_patch_tool(&root)
            .execute(
                input(serde_json::json!({ "input": patch })),
                context(&root).await,
            )
            .await
            .unwrap();
        assert_eq!(
            tokio::fs::read_to_string(root.join(path)).await.unwrap(),
            "ok\n"
        );
    }

    let error = apply_patch_tool(&root)
        .execute(
            input(serde_json::json!({
                "input": "<<\"EOF'\n*** Begin Patch\n*** Add File: bad.txt\n+nope\n*** End Patch\nEOF"
            })),
            context(&root).await,
        )
        .await
        .unwrap_err();
    assert!(error.to_string().contains("first line must be"));
    assert!(!tokio::fs::try_exists(root.join("bad.txt")).await.unwrap());
    let _ = tokio::fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn apply_patch_rejects_natural_language_instruction_with_recovery_guidance() {
    let root = unique_temp_dir("patch-natural-language");
    let error = apply_patch_tool(&root)
        .execute(
            input(serde_json::json!({
                "input": "*** Begin Patch\nInsert after '<meta name=\"viewport\">':\n+<script></script>\n*** End Patch"
            })),
            context(&root).await,
        )
        .await
        .unwrap_err()
        .to_string();

    assert!(error.contains("natural-language edit instructions"));
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
    let patch =
        "*** Begin Patch\n*** Update File: old/name.txt\n*** Move to: new/name.txt\n*** End Patch";

    apply_patch_tool(&root)
        .execute(
            input(serde_json::json!({ "input": patch })),
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
    let patch = "*** Begin Patch\n*** Update File: old/name.txt\n*** Move to: new/name.txt\n@@\n-from\n+to\n*** End Patch";

    apply_patch_tool(&root)
        .execute(
            input(serde_json::json!({ "input": patch })),
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
async fn apply_patch_failure_reports_applied_changes() {
    let root = unique_temp_dir("patch-prefix-failure");
    tokio::fs::create_dir_all(&root).await.unwrap();
    let patch = "*** Begin Patch\n*** Add File: created.txt\n+hello\n*** Update File: missing.txt\n@@\n-old\n+new\n*** End Patch";

    let error = apply_patch_tool(&root)
        .execute(
            input(serde_json::json!({ "input": patch })),
            context(&root).await,
        )
        .await
        .unwrap_err()
        .to_string();

    assert!(error.contains("Changes applied before failure"));
    assert!(error.contains("A created.txt"));
    assert_eq!(
        tokio::fs::read_to_string(root.join("created.txt"))
            .await
            .unwrap(),
        "hello\n"
    );
    let _ = tokio::fs::remove_dir_all(root).await;
}

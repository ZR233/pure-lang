use super::*;
use pretty_assertions::assert_eq;

#[tokio::test]
async fn apply_patch_uses_unified_input_and_json_output() {
    let root = unique_temp_dir("patch-unified-output");
    let tool = apply_patch_tool();
    let patch = "*** Begin Patch\n*** Add File: src/lib.rs\n+pub fn ok() {}\n*** End Patch";

    let output = tool
        .execute(
            input(serde_json::json!({ "input": patch })),
            context(&root).await,
        )
        .await
        .unwrap();
    let value: serde_json::Value = serde_json::from_str(&output.description).unwrap();

    assert_eq!(value["cwd"], serde_json::json!("."));
    assert_eq!(value["added"], serde_json::json!(["src/lib.rs"]));
    assert_eq!(value["updated"], serde_json::json!([]));
    assert_eq!(value["deleted"], serde_json::json!([]));
    assert_eq!(value["changedFiles"], serde_json::json!(["src/lib.rs"]));
    assert_eq!(value["stdout"], serde_json::json!("apply_patch completed"));
    assert_eq!(value["stderr"], serde_json::json!(""));
    let _ = tokio::fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn apply_patch_adds_file() {
    let root = unique_temp_dir("patch-add");
    let tool = apply_patch_tool();
    let patch = "*** Begin Patch\n*** Add File: src/lib.rs\n+pub fn ok() {}\n*** End Patch";

    let output = tool
        .execute(
            input(serde_json::json!({ "input": patch })),
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
    let tool = apply_patch_tool();
    let patch = "*** Begin Patch\n*** Update File: src/lib.rs\n@@\n-missing\n+new\n*** End Patch";

    let result = tool
        .execute(
            input(serde_json::json!({ "input": patch })),
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
    let tool = apply_patch_tool();
    let patch = "Here is the patch:\n```patch\n*** Begin Patch\n*** Add File: wrapped.txt\n+ok\n*** End Patch\n```";

    tool.execute(
        input(serde_json::json!({ "input": patch })),
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
    let tool = apply_patch_tool();
    for (index, start) in ["<<EOF", "<<'EOF'", "<<\"EOF\""].into_iter().enumerate() {
        let path = format!("wrapped-{index}.txt");
        let patch =
            format!("{start}\n*** Begin Patch\n*** Add File: {path}\n+ok\n*** End Patch\nEOF");

        tool.execute(
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
    let _ = tokio::fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn apply_patch_rejects_mismatched_heredoc_wrapper() {
    let root = unique_temp_dir("patch-mismatched-heredoc");
    let tool = apply_patch_tool();
    let result = tool
        .execute(
            input(serde_json::json!({
                "input": "<<\"EOF'\n*** Begin Patch\n*** Add File: bad.txt\n+nope\n*** End Patch\nEOF"
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
    let tool = apply_patch_tool();
    let patch =
        "*** Begin Patch\n*** Environment ID: remote\n*** Add File: env.txt\n+ok\n*** End Patch";

    tool.execute(
        input(serde_json::json!({ "input": patch })),
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
    let tool = apply_patch_tool();
    let result = tool
        .execute(
            input(serde_json::json!({
                "input": "*** Begin Patch\n*** Environment ID:   \n*** Add File: env.txt\n+ok\n*** End Patch"
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
    let tool = apply_patch_tool();
    let patch = "*** Begin Patch\n*** Add File: empty.txt\n*** End Patch";

    tool.execute(
        input(serde_json::json!({ "input": patch })),
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
    let tool = apply_patch_tool();
    let patch = " *** Begin Patch\n  *** Update File: file.txt\n@@\n-one\n+two\n *** End Patch ";

    tool.execute(
        input(serde_json::json!({ "input": patch })),
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
    let tool = apply_patch_tool();
    let patch = "*** Begin Patch\n*** Update File: unicode.txt\n@@\n-import asyncio  # local import - avoids top-level dep\n-let quote = \"ok\"\n-space = \"a b\"\n+done\n*** End Patch";

    tool.execute(
        input(serde_json::json!({ "input": patch })),
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
    let tool = apply_patch_tool();
    let patch = "*** Begin Patch\n*** Update File: file.txt\n\n@@\n-one\n+two\n*** End Patch";

    tool.execute(
        input(serde_json::json!({ "input": patch })),
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
    let tool = apply_patch_tool();
    let patch = "*** Begin Patch\n*** Update File: lines.txt\n@@\n line1\n-line2\n line3\n*** Update File: tail.txt\n@@\n first\n-second\n+second updated\n\n*** End of File\n*** End Patch";

    tool.execute(
        input(serde_json::json!({ "input": patch })),
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
    let tool = apply_patch_tool();
    let result = tool
        .execute(
            input(serde_json::json!({
                "input": "*** Begin Patch\n*** Add File: missing.txt\n+nope"
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
    let tool = apply_patch_tool();
    let patch = "*** Begin Patch\n*** Update File: lines.txt\n@@\n-middle\n+updated\n*** End of File\n*** End Patch";

    tool.execute(
        input(serde_json::json!({ "input": patch })),
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
    let tool = apply_patch_tool();
    let result = tool
        .execute(
            input(serde_json::json!({
                "input": "*** Begin Patch\n--- a/file.txt\n+++ b/file.txt\n@@ -1 +1 @@\n-old\n+new\n*** End Patch"
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
    let tool = apply_patch_tool();
    let result = tool
        .execute(
            input(serde_json::json!({
                "input": "*** Begin Patch\n*** File: src/lib.rs\n*** End Patch"
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
    let tool = apply_patch_tool();
    let result = tool
        .execute(
            input(serde_json::json!({
                "input": "*** Begin Patch\nInsert after '<meta name=\"viewport\">':\n+<script></script>\n*** End Patch"
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
    let tool = apply_patch_tool();
    let patch =
        "*** Begin Patch\n*** Update File: old/name.txt\n*** Move to: new/name.txt\n*** End Patch";

    tool.execute(
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
async fn apply_patch_appends_pure_addition_chunk_to_eof() {
    let root = unique_temp_dir("patch-pure-addition-eof");
    tokio::fs::create_dir_all(&root).await.unwrap();
    tokio::fs::write(
        root.join("page.html"),
        "<head>\n<title>x</title>\n</head>\n",
    )
    .await
    .unwrap();
    let tool = apply_patch_tool();
    let patch =
        "*** Begin Patch\n*** Update File: page.html\n@@ <head>\n+<script></script>\n*** End Patch";

    tool.execute(
        input(serde_json::json!({ "input": patch })),
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
    let tool = apply_patch_tool();
    let patch =
        "*** Begin Patch\n*** Update File: style.html\n@@\n   </style>\n+tail\n*** End Patch";

    tool.execute(
        input(serde_json::json!({ "input": patch })),
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
    let tool = apply_patch_tool();
    let patch =
        "*** Begin Patch\n*** Update File: page.html\n@@\n-old\n+new\n</body>\n*** End Patch";

    tool.execute(
        input(serde_json::json!({ "input": patch })),
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
    let tool = apply_patch_tool();
    let patch = "*** Begin Patch\n*** Update File: page.html\n@@\n <footer>\n   <p>done</p>\n </footer>\n\n+<script type=\"module\">\n+console.log('ready');\n+</script>\n</body>\n*** End Patch";

    tool.execute(
        input(serde_json::json!({ "input": patch })),
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
    let tool = apply_patch_tool();
    let patch = "*** Begin Patch\n*** Update File: deepseek-intro.html\n@@\n    </style>\n+        .cube { display: block; }\n     </style>\n*** End Patch";

    tool.execute(
        input(serde_json::json!({ "input": patch })),
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
    let tool = apply_patch_tool();
    let patch = "*** Begin Patch\n*** Update File: src.rs\n@@\n-one\n+first\n*** Update File: src.rs\n@@\n-first\n+second\n*** End Patch";

    tool.execute(
        input(serde_json::json!({ "input": patch })),
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
    let tool = apply_patch_tool();
    let patch = "*** Begin Patch\n*** Add File: duplicate.txt\n+new\n*** End Patch";

    let output = tool
        .execute(
            input(serde_json::json!({ "input": patch })),
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
    let tool = apply_patch_tool();
    let patch = "*** Begin Patch\n*** Update File: old/name.txt\n*** Move to: new/name.txt\n@@\n-from\n+to\n*** End Patch";

    tool.execute(
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
async fn apply_patch_failure_keeps_committed_prefix() {
    let root = unique_temp_dir("patch-prefix-failure");
    tokio::fs::create_dir_all(&root).await.unwrap();
    let tool = apply_patch_tool();
    let patch = "*** Begin Patch\n*** Add File: created.txt\n+hello\n*** Update File: missing.txt\n@@\n-old\n+new\n*** End Patch";

    let result = tool
        .execute(
            input(serde_json::json!({ "input": patch })),
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
    let tool = apply_patch_tool();
    let patch = "*** Begin Patch\n*** Add File: notes.txt\n+new\n*** Update File: notes.txt\n@@\n-new\n+newer\n*** End Patch";

    let output = tool
        .execute(
            input(serde_json::json!({ "input": patch })),
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

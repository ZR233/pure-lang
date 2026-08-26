use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use pretty_assertions::assert_eq;

use super::*;
use crate::PureError;
use crate::tool::ToolResult;
use crate::turn::ToolEffect;

fn output(description: &str) -> ToolResult {
    ToolResult::success(description)
}

fn cache_request<'a>(
    tool_name: &'a str,
    arguments: &'a serde_json::Value,
    workspace_root: &'a Path,
    policy: ToolCachePolicy,
    call_id: impl Into<String>,
    executor_generation: u64,
) -> ToolCacheExecutionRequest<'a> {
    ToolCacheExecutionRequest {
        tool_name,
        arguments,
        workspace_root,
        policy,
        call_id: call_id.into(),
        executor_generation,
    }
}

fn read_file_output(start_line: u64, end_line: u64, next_start_line: Option<u64>) -> ToolResult {
    output(
        &serde_json::json!({
            "path": "src/lib.rs",
            "startLine": start_line,
            "endLine": end_line,
            "nextStartLine": next_start_line,
            "contentHash": "sha256:test",
            "text": "file contents",
        })
        .to_string(),
    )
}

#[test]
fn workspace_mutation_invalidates_workspace_but_not_project_view() {
    let cache = TurnToolCacheHandle::default();
    let root = Path::new("/workspace/repo");
    let workspace_args = serde_json::json!({"path": "src/lib.rs"});
    let project_args = serde_json::json!({"path": "/project/repo/AGENTS.md"});
    for (id, args) in [("workspace", &workspace_args), ("project", &project_args)] {
        cache.insert(
            "read_file",
            args,
            root,
            ToolCachePolicy::UntilWorkspaceMutation,
            id.to_string(),
            &output(id),
        );
    }

    cache.record_effect(Some(ToolEffect::WorkspaceWrite), true);

    assert_eq!(
        cache.lookup(
            "read_file",
            &workspace_args,
            root,
            ToolCachePolicy::UntilWorkspaceMutation,
        ),
        None
    );
    assert!(
        cache
            .lookup(
                "read_file",
                &project_args,
                root,
                ToolCachePolicy::UntilWorkspaceMutation,
            )
            .is_some()
    );
}

#[test]
fn product_write_can_invalidate_only_its_read_cache() {
    let cache = TurnToolCacheHandle::default();
    let root = Path::new("/workspace/repo");
    let github = serde_json::json!({"method": "GET", "path": "/repos/o/r/pulls/1"});
    let file = serde_json::json!({"path": "src/lib.rs"});
    cache.insert(
        "github_api_request",
        &github,
        root,
        ToolCachePolicy::WithinTurn,
        "github-call".to_string(),
        &output("github"),
    );
    cache.insert(
        "read_file",
        &file,
        root,
        ToolCachePolicy::WithinTurn,
        "file-call".to_string(),
        &output("file"),
    );

    cache.invalidate_tool("github_api_request");

    assert_eq!(
        cache.lookup(
            "github_api_request",
            &github,
            root,
            ToolCachePolicy::WithinTurn,
        ),
        None
    );
    assert!(
        cache
            .lookup("read_file", &file, root, ToolCachePolicy::WithinTurn)
            .is_some()
    );
}

#[tokio::test]
async fn concurrent_identical_reads_execute_once() {
    let cache = TurnToolCacheHandle::default();
    let executions = Arc::new(AtomicUsize::new(0));
    let mut tasks = Vec::new();
    for call in 0..4 {
        let cache = cache.clone();
        let executions = Arc::clone(&executions);
        tasks.push(tokio::spawn(async move {
            cache
                .snapshot()
                .execute_or_reuse(
                    cache_request(
                        "read_file",
                        &serde_json::json!({"path": "src/lib.rs", "startLine": 1}),
                        Path::new("/workspace/repo"),
                        ToolCachePolicy::UntilWorkspaceMutation,
                        format!("call-{call}"),
                        0,
                    ),
                    || async move {
                        executions.fetch_add(1, Ordering::SeqCst);
                        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                        Ok(output("full file range"))
                    },
                )
                .await
                .expect("infallible read")
        }));
    }

    let mut outputs = Vec::new();
    for task in tasks {
        outputs.push(task.await.expect("cache task joins"));
    }

    assert_eq!(executions.load(Ordering::SeqCst), 1);
    assert_eq!(
        outputs
            .iter()
            .filter(|output| output.model_output().contains("\"cacheHit\":true"))
            .count(),
        3
    );
}

#[tokio::test]
async fn executor_generation_is_part_of_the_execution_cache_key() {
    let cache = TurnToolCacheHandle::default();
    let snapshot = cache.snapshot();
    let executions = Arc::new(AtomicUsize::new(0));
    let arguments = serde_json::json!({"query": "stable"});
    let root = Path::new("/workspace/repo");

    for generation in [7, 8, 7] {
        let executions = executions.clone();
        snapshot
            .execute_or_reuse(
                cache_request(
                    "lookup",
                    &arguments,
                    root,
                    ToolCachePolicy::WithinTurn,
                    format!("call-{generation}"),
                    generation,
                ),
                || async move {
                    executions.fetch_add(1, Ordering::SeqCst);
                    Ok(output(&format!("generation-{generation}")))
                },
            )
            .await
            .expect("cacheable lookup");
    }

    assert_eq!(executions.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn provider_response_snapshot_keeps_failure_key_stable_while_epoch_advances() {
    let cache = TurnToolCacheHandle::default();
    let snapshot = cache.snapshot();
    let executions = Arc::new(AtomicUsize::new(0));
    let first_started = Arc::new(tokio::sync::Notify::new());
    let release_first = Arc::new(tokio::sync::Notify::new());
    let arguments = serde_json::json!({
        "path": "missing.rs",
        "startLine": 1,
    });
    let root = Path::new("/workspace/repo");

    let first = {
        let snapshot = snapshot.clone();
        let executions = Arc::clone(&executions);
        let first_started = Arc::clone(&first_started);
        let release_first = Arc::clone(&release_first);
        let arguments = arguments.clone();
        tokio::spawn(async move {
            snapshot
                .execute_or_reuse(
                    cache_request(
                        "read_file",
                        &arguments,
                        root,
                        ToolCachePolicy::UntilWorkspaceMutation,
                        "first",
                        0,
                    ),
                    || async move {
                        executions.fetch_add(1, Ordering::SeqCst);
                        first_started.notify_one();
                        release_first.notified().await;
                        Err(PureError::ToolExecutionFailed {
                            tool: "read_file".to_string(),
                            error: "path does not exist".to_string(),
                        })
                    },
                )
                .await
        })
    };

    first_started.notified().await;
    cache.record_effect(Some(ToolEffect::Process), true);

    let second = {
        let snapshot = snapshot.clone();
        let executions = Arc::clone(&executions);
        let arguments = arguments.clone();
        tokio::spawn(async move {
            snapshot
                .execute_or_reuse(
                    cache_request(
                        "read_file",
                        &arguments,
                        root,
                        ToolCachePolicy::UntilWorkspaceMutation,
                        "second",
                        0,
                    ),
                    || async move {
                        executions.fetch_add(1, Ordering::SeqCst);
                        Err(PureError::ToolExecutionFailed {
                            tool: "read_file".to_string(),
                            error: "duplicate execution".to_string(),
                        })
                    },
                )
                .await
        })
    };

    tokio::task::yield_now().await;
    release_first.notify_one();
    let first_error = first
        .await
        .expect("first cache task joins")
        .expect_err("first read fails");
    let second_error = second
        .await
        .expect("second cache task joins")
        .expect_err("duplicate read returns compact failure");

    assert!(!first_error.to_string().contains("duplicateFailure"));
    assert!(second_error.to_string().contains("duplicateFailure"));
    assert!(second_error.to_string().contains("first"));
    assert_eq!(executions.load(Ordering::SeqCst), 1);

    let executions_after_batch = Arc::clone(&executions);
    let third_error = cache
        .snapshot()
        .execute_or_reuse(
            cache_request(
                "read_file",
                &arguments,
                root,
                ToolCachePolicy::UntilWorkspaceMutation,
                "third",
                0,
            ),
            || async move {
                executions_after_batch.fetch_add(1, Ordering::SeqCst);
                Err(PureError::ToolExecutionFailed {
                    tool: "read_file".to_string(),
                    error: "path still does not exist".to_string(),
                })
            },
        )
        .await
        .expect_err("new provider response uses the advanced epoch");
    assert!(!third_error.to_string().contains("duplicateFailure"));
    assert_eq!(executions.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn covered_read_file_range_returns_compact_receipt_without_reexecution() {
    let cache = TurnToolCacheHandle::default();
    let executions = Arc::new(AtomicUsize::new(0));
    let root = Path::new("/workspace/repo");

    let first_executions = Arc::clone(&executions);
    cache
        .snapshot()
        .execute_or_reuse(
            cache_request(
                "read_file",
                &serde_json::json!({
                    "path": "src/lib.rs",
                    "startLine": 1,
                    "maxLines": 430,
                }),
                root,
                ToolCachePolicy::UntilWorkspaceMutation,
                "first",
                0,
            ),
            || async move {
                first_executions.fetch_add(1, Ordering::SeqCst);
                Ok(read_file_output(1, 430, Some(431)))
            },
        )
        .await
        .expect("first read succeeds");

    let covered_executions = Arc::clone(&executions);
    let covered = cache
        .snapshot()
        .execute_or_reuse(
            cache_request(
                "read_file",
                &serde_json::json!({
                    "path": "src/lib.rs",
                    "startLine": 100,
                    "maxLines": 200,
                }),
                root,
                ToolCachePolicy::UntilWorkspaceMutation,
                "covered",
                0,
            ),
            || async move {
                covered_executions.fetch_add(1, Ordering::SeqCst);
                Ok(read_file_output(100, 299, Some(300)))
            },
        )
        .await
        .expect("covered read is reused");
    assert_eq!(executions.load(Ordering::SeqCst), 1);
    assert!(covered.model_output().contains("\"cacheHit\":true"));
    assert!(
        covered
            .model_output()
            .contains("\"reuseKind\":\"coveredReadRange\"")
    );

    let expanded_executions = Arc::clone(&executions);
    cache
        .snapshot()
        .execute_or_reuse(
            cache_request(
                "read_file",
                &serde_json::json!({
                    "path": "src/lib.rs",
                    "startLine": 300,
                    "maxLines": 200,
                }),
                root,
                ToolCachePolicy::UntilWorkspaceMutation,
                "expanded",
                0,
            ),
            || async move {
                expanded_executions.fetch_add(1, Ordering::SeqCst);
                Ok(read_file_output(300, 499, Some(500)))
            },
        )
        .await
        .expect("uncovered suffix executes");
    assert_eq!(executions.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn deterministic_read_failure_is_reused_until_any_workspace_mutation_attempt() {
    let cache = TurnToolCacheHandle::default();
    let executions = Arc::new(AtomicUsize::new(0));
    let arguments = serde_json::json!({"path": "missing.rs"});
    let root = Path::new("/workspace/repo");

    for call_id in ["first", "second"] {
        let executions = Arc::clone(&executions);
        let error = cache
            .snapshot()
            .execute_or_reuse(
                cache_request(
                    "read_file",
                    &arguments,
                    root,
                    ToolCachePolicy::UntilWorkspaceMutation,
                    call_id,
                    0,
                ),
                || async move {
                    executions.fetch_add(1, Ordering::SeqCst);
                    Err(PureError::ToolExecutionFailed {
                        tool: "read_file".to_string(),
                        error: "path does not exist".to_string(),
                    })
                },
            )
            .await
            .unwrap_err();
        if call_id == "second" {
            assert!(error.to_string().contains("duplicateFailure"));
            assert!(error.to_string().contains("first"));
        }
    }
    assert_eq!(executions.load(Ordering::SeqCst), 1);

    cache.record_effect(Some(ToolEffect::WorkspaceWrite), false);
    let executions_after_mutation = Arc::clone(&executions);
    let _ = cache
        .snapshot()
        .execute_or_reuse(
            cache_request(
                "read_file",
                &arguments,
                root,
                ToolCachePolicy::UntilWorkspaceMutation,
                "third",
                0,
            ),
            || async move {
                executions_after_mutation.fetch_add(1, Ordering::SeqCst);
                Err(PureError::ToolExecutionFailed {
                    tool: "read_file".to_string(),
                    error: "path still does not exist".to_string(),
                })
            },
        )
        .await;
    assert_eq!(executions.load(Ordering::SeqCst), 2);
}

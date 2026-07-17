use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use pretty_assertions::assert_eq;

use super::*;
use crate::studio::task_coordinator::{
    AllocateExecutor, CreateMergeRecord, CreateTaskRun, MergeStatus, StudioTaskSpawnRequest,
    UpdateMergeRecord,
};
use crate::{
    AgentKernel, AgentKernelToolRequest, CoreAgentProfile, StudioMode, StudioStore,
    TurnEngineBuilder,
};

const DESIGN_PATCH: &str =
    "*** Begin Patch\n*** Update File: design/spec.md\n@@\n-before\n+after\n*** End Patch";

#[tokio::test]
async fn design_patch_commits_and_atomically_opens_executor_gate() {
    let fixture = DesignFixture::new("success").await;
    let before = fixture.run.expected_head.clone();

    let denied = fixture
        .store
        .allocate_executor(allocation(&fixture.session_id, "before-design"))
        .await;
    let denied = match denied {
        Ok(_) => panic!("executor allocation must remain gated before durable design"),
        Err(error) => error,
    };
    assert!(denied.to_string().contains("requires task phase"));

    let output = fixture.update(DESIGN_PATCH).await.unwrap();
    let run = fixture
        .store
        .read_task_run(&fixture.run.id)
        .await
        .unwrap()
        .unwrap();
    let lease = fixture
        .store
        .read_branch_lease(&fixture.run.id)
        .await
        .unwrap()
        .unwrap();

    assert_eq!(output.task_run_id, fixture.run.id);
    assert_eq!(output.previous_head, before);
    assert_eq!(output.changed_files, vec!["design/spec.md".to_string()]);
    assert_eq!(run.phase, TaskRunPhase::Implementing);
    assert_eq!(
        run.design_commit.as_deref(),
        Some(output.design_commit.as_str())
    );
    assert_eq!(run.expected_head, output.design_commit);
    assert_eq!(lease.expected_head, run.expected_head);
    assert_eq!(
        git_output(&fixture.repository, &["rev-parse", "HEAD"]),
        run.expected_head
    );
    assert_eq!(
        std::fs::read_to_string(fixture.repository.join("design/spec.md")).unwrap(),
        "after\n"
    );

    fixture
        .store
        .allocate_executor(allocation(&fixture.session_id, "after-design"))
        .await
        .expect("executor allocation should open immediately after durable design");
    fixture.cleanup().await;
}

#[tokio::test]
async fn design_commit_uses_studio_identity_without_changing_repository_config() {
    let fixture = DesignFixture::new("studio-commit-identity").await;
    git(&fixture.repository, &["config", "user.name", ""]);
    git(&fixture.repository, &["config", "user.email", ""]);

    fixture.update(DESIGN_PATCH).await.unwrap();

    assert_eq!(
        git_output(&fixture.repository, &["log", "-1", "--pretty=%an <%ae>"]),
        "Pure Studio <pure-studio@local>"
    );
    assert_eq!(
        git_output(
            &fixture.repository,
            &["config", "--local", "--get", "user.name"]
        ),
        ""
    );
    assert_eq!(
        git_output(
            &fixture.repository,
            &["config", "--local", "--get", "user.email"]
        ),
        ""
    );
    fixture.cleanup().await;
}

#[tokio::test]
async fn executor_retry_limit_uses_owned_paths_instead_of_mutable_title() {
    let fixture = DesignFixture::new("stable-retry-identity").await;
    fixture.update(DESIGN_PATCH).await.unwrap();

    for attempt in 1..=3 {
        let agent_id = format!("agent-retry-{attempt}");
        let allocation = fixture
            .store
            .allocate_executor(AllocateExecutor {
                session_id: fixture.session_id.clone(),
                title: format!("renamed work unit {attempt}"),
                owned_paths: vec!["src/shared.rs".to_string()],
                agent_id: agent_id.clone(),
                owner_path: "/root".to_string(),
                requested_by_call_id: format!("call-retry-{attempt}"),
            })
            .await
            .unwrap();
        assert_eq!(allocation.work_unit.attempt, attempt);
        fixture
            .store
            .fail_executor(&allocation.work_unit.id, &agent_id, "retry")
            .await
            .unwrap();
    }

    let result = fixture
        .store
        .allocate_executor(AllocateExecutor {
            session_id: fixture.session_id.clone(),
            title: "another renamed work unit".to_string(),
            owned_paths: vec!["src/shared.rs".to_string()],
            agent_id: "agent-retry-4".to_string(),
            owner_path: "/root".to_string(),
            requested_by_call_id: "call-retry-4".to_string(),
        })
        .await;
    let error = match result {
        Ok(_) => panic!("renaming a work unit must not reset its retry budget"),
        Err(error) => error,
    };
    assert!(error.to_string().contains("attempt must be within 1..=3"));
    fixture.cleanup().await;
}

#[tokio::test]
async fn root_tool_uses_captured_studio_session_and_reports_patch_cause() {
    let fixture = DesignFixture::new("captured-session").await;
    let tool = fixture
        .coordinator
        .task_update_design_tool(fixture.session_id.clone());
    let kernel = AgentKernel::builder(
        TurnEngineBuilder::from_provider_info(pl_model::ProviderInfo::deepseek(None)).unwrap(),
    )
    .with_profile(CoreAgentProfile::host_provided(fixture.repository.clone()))
    .with_registered_tool(tool)
    .build()
    .await;
    let (event_tx, _) = tokio::sync::broadcast::channel(16);

    let error = kernel
        .execute_tool(AgentKernelToolRequest::new(
            "task_update_design",
            serde_json::json!({
                "patch": "*** Add File: design/missing-wrapper.md\n+content"
            }),
            "turn-id-is-not-studio-session-id",
            "call-invalid-design",
            event_tx.clone(),
        ))
        .await
        .unwrap_err();
    let message = error.to_string();
    assert!(
        message.contains("first line must be '*** Begin Patch'"),
        "missing parser cause in tool error: {message}"
    );

    let output = kernel
        .execute_tool(AgentKernelToolRequest::new(
            "task_update_design",
            serde_json::json!({ "patch": DESIGN_PATCH }),
            "turn-id-is-not-studio-session-id",
            "call-design",
            event_tx,
        ))
        .await
        .unwrap();
    let result: serde_json::Value = serde_json::from_str(&output.description).unwrap();

    assert_eq!(result["taskRunId"], fixture.run.id);
    fixture.cleanup().await;
}

#[tokio::test]
async fn invalid_or_mixed_paths_are_rejected_before_any_write() {
    let fixture = DesignFixture::new("path-guards").await;
    let before = fixture.head();
    let original = fixture.design_text();
    let patches = [
        "*** Begin Patch\n*** Add File: src/lib.rs\n+source\n*** End Patch",
        "*** Begin Patch\n*** Add File: design/../src/lib.rs\n+source\n*** End Patch",
        "*** Begin Patch\n*** Add File: C:/outside.md\n+outside\n*** End Patch",
        "*** Begin Patch\n*** Update File: design/spec.md\n*** Move to: src/spec.md\n@@\n-before\n+after\n*** End Patch",
        "*** Begin Patch\n*** Add File: design/ignored.md\n+ignored\n*** End Patch",
        "*** Begin Patch\n*** Update File: design/spec.md\n@@\n-before\n+after\n*** Add File: src/lib.rs\n+source\n*** End Patch",
    ];

    for patch in patches {
        assert!(
            fixture.update(patch).await.is_err(),
            "patch should be rejected: {patch}"
        );
        assert_eq!(fixture.head(), before);
        assert_eq!(fixture.design_text(), original);
        assert!(fixture.status().is_empty());
    }
    fixture.cleanup().await;
}

#[tokio::test]
async fn path_prevalidation_rejects_every_source_and_move_destination_before_apply() {
    let fixture = DesignFixture::new("path-prevalidation").await;
    for raw in [
        "src/lib.rs",
        "design/../src/lib.rs",
        "C:/outside.md",
        "design/ignored.md",
    ] {
        assert!(
            validate_design_path(&fixture.repository, raw)
                .await
                .is_err(),
            "path must fail prevalidation: {raw}"
        );
    }
    let move_outside = "*** Begin Patch\n*** Update File: design/spec.md\n*** Move to: src/spec.md\n@@\n-before\n+after\n*** End Patch";
    assert!(
        validate_design_patch(&fixture.repository, move_outside)
            .await
            .is_err()
    );
    let mixed = "*** Begin Patch\n*** Update File: design/spec.md\n@@\n-before\n+after\n*** Add File: src/lib.rs\n+source\n*** End Patch";
    assert!(
        validate_design_patch(&fixture.repository, mixed)
            .await
            .is_err()
    );
    fixture.cleanup().await;
}

#[tokio::test]
async fn symlink_target_is_rejected_before_write() {
    let fixture = DesignFixture::new("symlink-guard").await;
    let outside = fixture.repository.with_extension("outside");
    std::fs::create_dir_all(&outside).unwrap();
    create_directory_symlink(&outside, &fixture.repository.join("design/link"));
    git(&fixture.repository, &["add", "design/link"]);
    git(
        &fixture.repository,
        &["commit", "-m", "tracked design symlink"],
    );
    let symlink_head = fixture.head();
    assert!(
        fixture
            .store
            .compare_and_set_task_head(&fixture.run.id, &fixture.run.expected_head, &symlink_head)
            .await
            .unwrap()
    );

    let error = fixture
        .update("*** Begin Patch\n*** Add File: design/link/escape.md\n+escape\n*** End Patch")
        .await
        .expect_err("symlink escape must be rejected");

    assert!(
        error.to_string().contains("symbolic link"),
        "unexpected error: {error:#}"
    );
    assert!(!outside.join("escape.md").exists());
    fixture.cleanup().await;
    let _ = std::fs::remove_dir_all(outside);
}

#[tokio::test]
async fn empty_noop_and_later_hunk_failure_restore_exact_state() {
    let fixture = DesignFixture::new("rollback").await;
    let before = fixture.head();
    for patch in [
        "*** Begin Patch\n*** End Patch",
        "*** Begin Patch\n*** Update File: design/spec.md\n*** End Patch",
        "*** Begin Patch\n*** Update File: design/spec.md\n@@\n-before\n+temporary\n*** Update File: design/missing.md\n@@\n-missing\n+changed\n*** End Patch",
    ] {
        assert!(fixture.update(patch).await.is_err());
        assert_eq!(fixture.head(), before);
        assert_eq!(fixture.design_text(), "before\n");
        assert!(fixture.status().is_empty());
    }
    fixture.cleanup().await;
}

#[tokio::test]
async fn dirty_wrong_branch_detached_and_head_drift_are_rejected() {
    let dirty = DesignFixture::new("dirty").await;
    std::fs::write(dirty.repository.join("external.txt"), "dirty\n").unwrap();
    assert!(
        dirty
            .update(DESIGN_PATCH)
            .await
            .unwrap_err()
            .to_string()
            .contains("clean")
    );
    dirty.cleanup().await;

    let wrong = DesignFixture::new("wrong-branch").await;
    git(&wrong.repository, &["checkout", "-b", "other"]);
    assert!(wrong.update(DESIGN_PATCH).await.is_err());
    wrong.cleanup().await;

    let detached = DesignFixture::new("detached").await;
    git(&detached.repository, &["checkout", "--detach"]);
    assert!(
        detached
            .update(DESIGN_PATCH)
            .await
            .unwrap_err()
            .to_string()
            .contains("named branch")
    );
    detached.cleanup().await;

    let drift = DesignFixture::new("head-drift").await;
    std::fs::write(drift.repository.join("external.txt"), "committed\n").unwrap();
    git(&drift.repository, &["add", "external.txt"]);
    git(&drift.repository, &["commit", "-m", "external"]);
    assert!(drift.update(DESIGN_PATCH).await.is_err());
    drift.cleanup().await;
}

#[tokio::test]
async fn missing_ambiguous_and_terminal_session_scope_are_rejected() {
    let fixture = DesignFixture::new("session-scope").await;
    let head = fixture.head();
    assert!(
        fixture
            .coordinator
            .update_design("wrong-studio-session", &fixture.repository, DESIGN_PATCH)
            .await
            .unwrap_err()
            .to_string()
            .contains("not found")
    );

    fixture
        .store
        .create_task_run_with_lease(CreateTaskRun {
            session_id: fixture.session_id.clone(),
            phase: TaskRunPhase::DesignUpdating,
            plan: "ambiguous".to_string(),
            workspace_root: fixture.repository.to_string_lossy().to_string(),
            git_common_dir: fixture
                .repository
                .join("other-common-dir")
                .to_string_lossy()
                .to_string(),
            branch: "other-branch".to_string(),
            head_commit: head.clone(),
        })
        .await
        .unwrap();
    assert!(
        fixture
            .update(DESIGN_PATCH)
            .await
            .unwrap_err()
            .to_string()
            .contains("multiple active")
    );

    fixture.cleanup().await;

    let terminal = DesignFixture::new("terminal-session-scope").await;
    terminal
        .coordinator
        .finish_task(&terminal.run.id, TaskRunPhase::Cancelled, None)
        .await
        .unwrap();
    assert!(
        terminal
            .update(DESIGN_PATCH)
            .await
            .unwrap_err()
            .to_string()
            .contains("not found")
    );
    terminal.cleanup().await;
}

#[tokio::test]
async fn concurrent_identical_calls_serialize_to_one_commit_and_one_cas() {
    let fixture = DesignFixture::new("concurrent").await;
    let start = Arc::new(tokio::sync::Barrier::new(3));
    let mut calls = Vec::new();
    for _ in 0..2 {
        let coordinator = fixture.coordinator.clone();
        let session_id = fixture.session_id.clone();
        let repository = fixture.repository.clone();
        let start = start.clone();
        calls.push(tokio::spawn(async move {
            start.wait().await;
            coordinator
                .update_design(&session_id, &repository, DESIGN_PATCH)
                .await
        }));
    }
    start.wait().await;
    let results = futures::future::join_all(calls).await;
    let successes = results
        .into_iter()
        .filter(|result| result.as_ref().is_ok_and(|inner| inner.is_ok()))
        .count();

    assert_eq!(successes, 1);
    assert_eq!(
        git_output(
            &fixture.repository,
            &[
                "rev-list",
                "--count",
                &format!("{}..HEAD", fixture.run.base_commit)
            ]
        ),
        "1"
    );
    assert!(fixture.status().is_empty());
    fixture.cleanup().await;
}

#[tokio::test]
async fn commit_hook_failure_restores_index_and_worktree() {
    let fixture = DesignFixture::new("commit-failure").await;
    let hook = fixture.repository.join(".git/hooks/pre-commit");
    std::fs::write(&hook, "#!/bin/sh\nexit 1\n").unwrap();
    make_executable(&hook);
    let before = fixture.head();

    assert!(fixture.update(DESIGN_PATCH).await.is_err());

    assert_eq!(fixture.head(), before);
    assert_eq!(fixture.design_text(), "before\n");
    assert!(fixture.status().is_empty());
    fixture.cleanup().await;
}

#[tokio::test]
async fn sqlite_failure_safely_compensates_git_commit() {
    let fixture = DesignFixture::new("cas-compensation").await;
    inject_design_transaction_failure(&fixture.store).await;
    let before = fixture.head();

    assert!(fixture.update(DESIGN_PATCH).await.is_err());

    let run = fixture
        .store
        .read_task_run(&fixture.run.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(run.phase, TaskRunPhase::DesignUpdating);
    assert_eq!(run.expected_head, before);
    assert_eq!(fixture.head(), before);
    git(
        &fixture.repository,
        &[
            "diff",
            "--exit-code",
            &fixture.run.base_commit,
            "HEAD",
            "--",
            "design/spec.md",
        ],
    );
    assert!(fixture.status().is_empty());
    fixture.cleanup().await;
}

#[tokio::test]
async fn unsafe_sqlite_compensation_blocks_without_overwriting_external_change() {
    let fixture = DesignFixture::new("unsafe-compensation").await;
    inject_design_transaction_failure(&fixture.store).await;
    let barrier = DesignCommitTestBarrier::new();
    fixture
        .coordinator
        .set_design_after_commit_barrier(barrier.clone());
    let coordinator = fixture.coordinator.clone();
    let session_id = fixture.session_id.clone();
    let repository = fixture.repository.clone();
    let update = tokio::spawn(async move {
        coordinator
            .update_design(&session_id, &repository, DESIGN_PATCH)
            .await
    });
    tokio::time::timeout(
        std::time::Duration::from_secs(10),
        barrier.wait_until_committed(),
    )
    .await
    .unwrap();
    std::fs::write(
        fixture.repository.join("design/spec.md"),
        "external-after-commit\n",
    )
    .unwrap();
    barrier.release().await;

    assert!(update.await.unwrap().is_err());
    let run = fixture
        .store
        .read_task_run(&fixture.run.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(run.phase, TaskRunPhase::Blocked);
    assert_eq!(fixture.design_text(), "external-after-commit\n");
    fixture.cleanup().await;
}

#[tokio::test]
async fn failed_safe_compensation_blocks_exact_run() {
    let fixture = DesignFixture::new("compensation-failure").await;
    inject_design_transaction_failure(&fixture.store).await;
    fixture.coordinator.fail_design_compensation_for_test();

    assert!(fixture.update(DESIGN_PATCH).await.is_err());

    let run = fixture
        .store
        .read_task_run(&fixture.run.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(run.phase, TaskRunPhase::Blocked);
    assert!(
        run.status_message
            .unwrap_or_default()
            .contains("compensation failed")
    );
    fixture.cleanup().await;
}

#[tokio::test]
async fn commit_success_then_dirty_inspection_blocks_without_rolling_back_external_content() {
    let fixture = DesignFixture::new("post-commit-dirty").await;
    let barrier = DesignCommitTestBarrier::new();
    fixture
        .coordinator
        .set_design_after_commit_barrier(barrier.clone());
    let coordinator = fixture.coordinator.clone();
    let session_id = fixture.session_id.clone();
    let repository = fixture.repository.clone();
    let update = tokio::spawn(async move {
        coordinator
            .update_design(&session_id, &repository, DESIGN_PATCH)
            .await
    });
    tokio::time::timeout(
        std::time::Duration::from_secs(10),
        barrier.wait_until_committed(),
    )
    .await
    .unwrap();
    std::fs::write(
        fixture.repository.join("design/spec.md"),
        "external-after-focused-commit\n",
    )
    .unwrap();
    barrier.release().await;

    assert!(update.await.unwrap().is_err());
    let run = fixture
        .store
        .read_task_run(&fixture.run.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(run.phase, TaskRunPhase::Blocked);
    assert_eq!(fixture.design_text(), "external-after-focused-commit\n");
    fixture.cleanup().await;
}

#[tokio::test]
async fn external_clean_commit_after_focused_commit_is_never_persisted_as_design_commit() {
    let fixture = DesignFixture::new("external-clean-commit").await;
    let barrier = DesignCommitTestBarrier::new();
    fixture
        .coordinator
        .set_design_after_commit_barrier(barrier.clone());
    let coordinator = fixture.coordinator.clone();
    let session_id = fixture.session_id.clone();
    let repository = fixture.repository.clone();
    let update = tokio::spawn(async move {
        coordinator
            .update_design(&session_id, &repository, DESIGN_PATCH)
            .await
    });
    barrier.wait_until_committed().await;
    std::fs::write(fixture.repository.join("external.rs"), "external\n").unwrap();
    git(&fixture.repository, &["add", "external.rs"]);
    git(
        &fixture.repository,
        &["commit", "-m", "external clean commit"],
    );
    let external_head = fixture.head();
    barrier.release().await;

    assert!(update.await.unwrap().is_err());
    let run = fixture
        .store
        .read_task_run(&fixture.run.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(run.phase, TaskRunPhase::Blocked);
    assert_ne!(run.design_commit.as_deref(), Some(external_head.as_str()));
    assert_eq!(fixture.head(), external_head);
    fixture.cleanup().await;
}

#[tokio::test]
async fn commit_hook_staged_source_injection_blocks_without_accepting_mixed_commit() {
    let fixture = DesignFixture::new("hook-source-injection").await;
    let hook = fixture.repository.join(".git/hooks/pre-commit");
    std::fs::write(
        &hook,
        "#!/bin/sh\nprintf 'injected\\n' > source.rs\ngit add -- source.rs\n",
    )
    .unwrap();
    make_executable(&hook);

    assert!(fixture.update(DESIGN_PATCH).await.is_err());

    let run = fixture
        .store
        .read_task_run(&fixture.run.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(run.phase, TaskRunPhase::Blocked);
    assert_eq!(run.expected_head, fixture.run.expected_head);
    assert!(run.design_commit.is_none());
    assert_eq!(
        git_output(
            &fixture.repository,
            &["show", "--format=", "--name-only", "HEAD"]
        ),
        "design/spec.md\nsource.rs"
    );
    fixture.cleanup().await;
}

#[tokio::test]
async fn commit_hook_cannot_replace_validated_design_content() {
    let fixture = DesignFixture::new("hook-design-content-injection").await;
    let hook = fixture.repository.join(".git/hooks/pre-commit");
    std::fs::write(
        &hook,
        "#!/bin/sh\nprintf 'hook-mutated\\n' > design/spec.md\ngit add -- design/spec.md\n",
    )
    .unwrap();
    make_executable(&hook);

    assert!(fixture.update(DESIGN_PATCH).await.is_err());

    let run = fixture
        .store
        .read_task_run(&fixture.run.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(run.phase, TaskRunPhase::Blocked);
    assert!(run.design_commit.is_none());
    assert_eq!(fixture.design_text(), "hook-mutated\n");
    fixture.cleanup().await;
}

#[tokio::test]
async fn failed_commit_hook_source_residue_blocks_and_preserves_source() {
    let fixture = DesignFixture::new("failed-hook-source-residue").await;
    let hook = fixture.repository.join(".git/hooks/pre-commit");
    std::fs::write(
        &hook,
        "#!/bin/sh\nprintf 'hook-residue\\n' > source.rs\nexit 1\n",
    )
    .unwrap();
    make_executable(&hook);

    assert!(fixture.update(DESIGN_PATCH).await.is_err());

    let run = fixture
        .store
        .read_task_run(&fixture.run.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(run.phase, TaskRunPhase::Blocked);
    assert!(
        run.status_message
            .as_deref()
            .is_some_and(|message| message.contains("repository was not clean after rollback"))
    );
    assert_eq!(
        std::fs::read_to_string(fixture.repository.join("source.rs")).unwrap(),
        "hook-residue\n"
    );
    fixture.cleanup().await;
}

#[tokio::test]
async fn rollback_rejects_symlink_ancestor_race_without_writing_outside_workspace() {
    let fixture = DesignFixture::new("rollback-symlink-race").await;
    let outside = fixture.repository.with_extension("rollback-outside");
    std::fs::create_dir_all(&outside).unwrap();
    std::fs::write(outside.join("spec.md"), "outside\n").unwrap();
    let hook = fixture.repository.join(".git/hooks/pre-commit");
    std::fs::write(&hook, "#!/bin/sh\nexit 1\n").unwrap();
    make_executable(&hook);
    let barrier = DesignCommitTestBarrier::new();
    fixture
        .coordinator
        .set_design_before_rollback_barrier(barrier.clone());
    let coordinator = fixture.coordinator.clone();
    let session_id = fixture.session_id.clone();
    let repository = fixture.repository.clone();
    let update = tokio::spawn(async move {
        coordinator
            .update_design(&session_id, &repository, DESIGN_PATCH)
            .await
    });
    barrier.wait_until_committed().await;
    let design_backup = fixture.repository.join("design-backup");
    std::fs::rename(fixture.repository.join("design"), &design_backup).unwrap();
    create_directory_symlink(&outside, &fixture.repository.join("design"));
    barrier.release().await;

    assert!(update.await.unwrap().is_err());
    let run = fixture
        .store
        .read_task_run(&fixture.run.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(run.phase, TaskRunPhase::Blocked);
    assert!(
        run.status_message
            .as_deref()
            .is_some_and(|message| message.contains("rollback could not safely restore"))
    );
    assert_eq!(
        std::fs::read_to_string(outside.join("spec.md")).unwrap(),
        "outside\n"
    );

    remove_directory_symlink(&fixture.repository.join("design"));
    std::fs::rename(design_backup, fixture.repository.join("design")).unwrap();
    fixture.cleanup().await;
    std::fs::remove_dir_all(outside).unwrap();
}

#[tokio::test]
async fn durable_design_cas_then_external_commit_blocks_and_preserves_durable_heads() {
    assert_durable_design_final_scope_failure(
        "durable-design-external-commit",
        FinalScopeDrift::Commit,
    )
    .await;
}

#[tokio::test]
async fn durable_design_cas_then_branch_switch_blocks_and_preserves_durable_heads() {
    assert_durable_design_final_scope_failure(
        "durable-design-branch-switch",
        FinalScopeDrift::Branch,
    )
    .await;
}

#[tokio::test]
async fn durable_design_cas_then_dirty_workspace_blocks_and_preserves_durable_heads() {
    assert_durable_design_final_scope_failure(
        "durable-design-dirty-workspace",
        FinalScopeDrift::Dirty,
    )
    .await;
}

#[tokio::test]
async fn same_commit_on_another_branch_is_not_safe_to_compensate() {
    let fixture = DesignFixture::new("same-commit-other-branch").await;
    inject_design_transaction_failure(&fixture.store).await;
    let barrier = DesignCommitTestBarrier::new();
    fixture
        .coordinator
        .set_design_before_head_persist_barrier(barrier.clone());
    let coordinator = fixture.coordinator.clone();
    let session_id = fixture.session_id.clone();
    let repository = fixture.repository.clone();
    let update = tokio::spawn(async move {
        coordinator
            .update_design(&session_id, &repository, DESIGN_PATCH)
            .await
    });
    barrier.wait_until_committed().await;
    let exact_commit = fixture.head();
    git(
        &fixture.repository,
        &["switch", "-c", "external-same-commit"],
    );
    barrier.release().await;

    assert!(update.await.unwrap().is_err());
    let run = fixture
        .store
        .read_task_run(&fixture.run.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(run.phase, TaskRunPhase::Blocked);
    assert_eq!(fixture.head(), exact_commit);
    assert_eq!(
        git_output(
            &fixture.repository,
            &["symbolic-ref", "--quiet", "--short", "HEAD"]
        ),
        "external-same-commit"
    );
    fixture.cleanup().await;
}

#[tokio::test]
async fn revert_pre_commit_hook_failure_blocks_and_preserves_git_state() {
    assert_revert_hook_failure("revert-pre-commit-failure", "pre-commit").await;
}

#[tokio::test]
async fn revert_commit_msg_hook_failure_blocks_and_preserves_git_state() {
    assert_revert_hook_failure("revert-commit-msg-failure", "commit-msg").await;
}

#[tokio::test]
async fn revert_post_commit_dirty_failure_blocks_and_preserves_external_content() {
    let fixture = DesignFixture::new("revert-post-commit-dirty").await;
    fixture.update(DESIGN_PATCH).await.unwrap();
    let barrier = DesignCommitTestBarrier::new();
    fixture
        .coordinator
        .set_design_after_commit_barrier(barrier.clone());
    let coordinator = fixture.coordinator.clone();
    let run_id = fixture.run.id.clone();
    let revert = tokio::spawn(async move {
        coordinator
            .revert_design_for_no_source_cancel(&run_id)
            .await
    });
    barrier.wait_until_committed().await;
    std::fs::write(
        fixture.repository.join("design/spec.md"),
        "external-after-revert\n",
    )
    .unwrap();
    barrier.release().await;

    assert!(revert.await.unwrap().is_err());
    let run = fixture
        .store
        .read_task_run(&fixture.run.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(run.phase, TaskRunPhase::Blocked);
    assert_eq!(fixture.design_text(), "external-after-revert\n");
    fixture.cleanup().await;
}

#[tokio::test]
async fn revert_unsafe_compensation_blocks_without_rewriting_external_clean_commit() {
    let fixture = DesignFixture::new("revert-unsafe-compensation").await;
    fixture.update(DESIGN_PATCH).await.unwrap();
    fixture
        .store
        .execute_test_sql(
            "CREATE TRIGGER fail_revert_lease_update BEFORE UPDATE OF expected_head ON branch_leases \
             BEGIN SELECT RAISE(FAIL, 'injected revert transaction failure'); END;",
        )
        .await;
    let barrier = DesignCommitTestBarrier::new();
    fixture
        .coordinator
        .set_design_after_commit_barrier(barrier.clone());
    let coordinator = fixture.coordinator.clone();
    let run_id = fixture.run.id.clone();
    let revert = tokio::spawn(async move {
        coordinator
            .revert_design_for_no_source_cancel(&run_id)
            .await
    });
    barrier.wait_until_committed().await;
    std::fs::write(fixture.repository.join("external.rs"), "external\n").unwrap();
    git(&fixture.repository, &["add", "external.rs"]);
    git(
        &fixture.repository,
        &["commit", "-m", "external after revert"],
    );
    let external_head = fixture.head();
    barrier.release().await;

    assert!(revert.await.unwrap().is_err());
    let run = fixture
        .store
        .read_task_run(&fixture.run.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(run.phase, TaskRunPhase::Blocked);
    assert_eq!(fixture.head(), external_head);
    fixture.cleanup().await;
}

#[tokio::test]
async fn durable_revert_cas_then_dirty_workspace_blocks_with_advanced_heads() {
    let fixture = DesignFixture::new("durable-revert-dirty-workspace").await;
    fixture.update(DESIGN_PATCH).await.unwrap();
    let barrier = DesignCommitTestBarrier::new();
    fixture
        .coordinator
        .set_design_after_head_persist_barrier(barrier.clone());
    let coordinator = fixture.coordinator.clone();
    let run_id = fixture.run.id.clone();
    let revert = tokio::spawn(async move {
        coordinator
            .revert_design_for_no_source_cancel(&run_id)
            .await
    });
    barrier.wait_until_committed().await;
    let durable_revert_head = fixture.head();
    std::fs::write(fixture.repository.join("external.rs"), "external-dirty\n").unwrap();
    barrier.release().await;

    assert!(revert.await.unwrap().is_err());
    let run = fixture
        .store
        .read_task_run(&fixture.run.id)
        .await
        .unwrap()
        .unwrap();
    let lease = fixture
        .store
        .read_branch_lease(&fixture.run.id)
        .await
        .unwrap();
    assert_eq!(run.phase, TaskRunPhase::Blocked);
    assert_eq!(run.expected_head, durable_revert_head);
    assert!(lease.is_none());
    assert_eq!(
        std::fs::read_to_string(fixture.repository.join("external.rs")).unwrap(),
        "external-dirty\n"
    );
    fixture.cleanup().await;
}

#[tokio::test]
async fn no_source_cancel_creates_revert_and_atomically_advances_heads() {
    let fixture = DesignFixture::new("cancel-revert").await;
    let design = fixture.update(DESIGN_PATCH).await.unwrap();

    let reverted = fixture
        .coordinator
        .revert_design_for_no_source_cancel(&fixture.run.id)
        .await
        .unwrap();
    let run = fixture
        .store
        .read_task_run(&fixture.run.id)
        .await
        .unwrap()
        .unwrap();
    let lease = fixture
        .store
        .read_branch_lease(&fixture.run.id)
        .await
        .unwrap()
        .unwrap();

    assert_eq!(reverted.previous_head, design.design_commit);
    assert_ne!(reverted.revert_commit, reverted.previous_head);
    assert_eq!(run.expected_head, reverted.revert_commit);
    assert_eq!(lease.expected_head, run.expected_head);
    git(
        &fixture.repository,
        &[
            "diff",
            "--exit-code",
            &fixture.run.base_commit,
            "HEAD",
            "--",
            "design/spec.md",
        ],
    );
    assert_eq!(
        git_output(
            &fixture.repository,
            &["show", "-s", "--format=%P", &reverted.revert_commit]
        ),
        design.design_commit
    );
    assert!(fixture.status().is_empty());
    fixture.cleanup().await;
}

#[tokio::test]
async fn held_branch_mutation_guard_composes_revert_and_terminalization_without_allocation_gap() {
    let fixture = DesignFixture::new("locked-cancel-composition").await;
    fixture.update(DESIGN_PATCH).await.unwrap();
    let guard = fixture.coordinator.lock_branch_mutation().await;
    let request = StudioTaskSpawnRequest {
        agent_id: "agent-during-stop".to_string(),
        session_id: fixture.session_id.clone(),
        task_name: "during-stop".to_string(),
        role: "executor".to_string(),
        owned_paths: vec!["src/during_stop.rs".to_string()],
        requested_by_call_id: "call-during-stop".to_string(),
    };
    let coordinator = fixture.coordinator.clone();
    let allocation = tokio::spawn(async move { coordinator.prepare_agent_spawn(&request).await });

    fixture
        .coordinator
        .revert_design_for_no_source_cancel_locked(&fixture.run.id, &guard)
        .await
        .unwrap();
    fixture
        .coordinator
        .finish_task(&fixture.run.id, TaskRunPhase::Cancelled, None)
        .await
        .unwrap();
    drop(guard);

    let error = allocation
        .await
        .unwrap()
        .expect_err("allocation must wait for terminalization and then reject terminal phase");
    assert!(error.to_string().contains("active task run not found"));
    let run = fixture
        .store
        .read_task_run(&fixture.run.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(run.phase, TaskRunPhase::Cancelled);
    fixture.cleanup().await;
}

#[tokio::test]
async fn branch_mutation_guard_from_another_coordinator_is_rejected() {
    let guard_owner = DesignFixture::new("guard-owner").await;
    let target = DesignFixture::new("guard-target").await;
    let design = target.update(DESIGN_PATCH).await.unwrap();
    let guard = guard_owner.coordinator.lock_branch_mutation().await;

    let error = target
        .coordinator
        .revert_design_for_no_source_cancel_locked(&target.run.id, &guard)
        .await
        .expect_err("a guard from another coordinator must not authorize mutation");

    assert!(error.to_string().contains("another coordinator"));
    assert_eq!(target.head(), design.design_commit);
    drop(guard);
    guard_owner.cleanup().await;
    target.cleanup().await;
}

#[tokio::test]
async fn implementing_and_reworking_accept_reconciliation_while_reviewing_rejects_it() {
    let fixture = DesignFixture::new("reconciliation-phases").await;
    fixture.update(DESIGN_PATCH).await.unwrap();

    let implementing = fixture
        .update(&replace_patch("after", "after-implementing"))
        .await
        .unwrap();
    let run = fixture
        .store
        .read_task_run(&fixture.run.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(run.phase, TaskRunPhase::Implementing);
    assert_eq!(
        run.design_commit.as_deref(),
        Some(implementing.design_commit.as_str())
    );

    fixture
        .store
        .transition_task_run(&fixture.run.id, TaskRunPhase::Reviewing, None)
        .await
        .unwrap();
    let reviewing_head = fixture.head();
    assert!(
        fixture
            .update(&replace_patch("after-implementing", "reviewing-change"))
            .await
            .is_err()
    );
    assert_eq!(fixture.head(), reviewing_head);

    fixture
        .store
        .transition_task_run(&fixture.run.id, TaskRunPhase::Reworking, None)
        .await
        .unwrap();
    let reworking = fixture
        .update(&replace_patch("after-implementing", "after-reworking"))
        .await
        .unwrap();
    let run = fixture
        .store
        .read_task_run(&fixture.run.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(run.phase, TaskRunPhase::Reworking);
    assert_eq!(
        run.design_commit.as_deref(),
        Some(reworking.design_commit.as_str())
    );
    fixture.cleanup().await;
}

#[tokio::test]
async fn merging_and_resolving_conflict_reject_design_updates_without_moving_head() {
    let fixture = DesignFixture::new("merge-phase-guards").await;
    fixture.update(DESIGN_PATCH).await.unwrap();
    fixture
        .store
        .transition_task_run(&fixture.run.id, TaskRunPhase::Merging, None)
        .await
        .unwrap();
    let head = fixture.head();
    assert!(
        fixture
            .update(&replace_patch("after", "merging-change"))
            .await
            .is_err()
    );
    assert_eq!(fixture.head(), head);

    fixture
        .store
        .transition_task_run(&fixture.run.id, TaskRunPhase::ResolvingConflict, None)
        .await
        .unwrap();
    assert!(
        fixture
            .update(&replace_patch("after", "conflict-change"))
            .await
            .is_err()
    );
    assert_eq!(fixture.head(), head);
    fixture.cleanup().await;
}

#[tokio::test]
async fn accepted_source_merge_requires_a_final_design_consistency_commit() {
    let fixture = DesignFixture::new("source-merge-consistency").await;
    let design = fixture.update(DESIGN_PATCH).await.unwrap();
    std::fs::write(fixture.repository.join("source.rs"), "source\n").unwrap();
    git(&fixture.repository, &["add", "source.rs"]);
    git(
        &fixture.repository,
        &["commit", "-m", "merge accepted source"],
    );
    let merged_head = fixture.head();
    assert!(
        fixture
            .store
            .compare_and_set_task_head(&fixture.run.id, &design.design_commit, &merged_head)
            .await
            .unwrap()
    );
    let merge = fixture
        .store
        .create_merge_record(CreateMergeRecord {
            task_run_id: fixture.run.id.clone(),
            agent_id: "agent-source".to_string(),
            expected_head: design.design_commit,
            source_commit: merged_head.clone(),
            conflict_files: Vec::new(),
        })
        .await
        .unwrap();
    fixture
        .store
        .update_merge_record(
            &merge.id,
            UpdateMergeRecord {
                status: MergeStatus::Merged,
                resolution_summary: Some("accepted".to_string()),
                verification: Some(vec!["cargo test".to_string()]),
                attempt: 1,
            },
        )
        .await
        .unwrap();

    let run = fixture
        .store
        .read_task_run(&fixture.run.id)
        .await
        .unwrap()
        .unwrap();
    assert!(!design_commit_is_current(&run));

    let consistency = fixture
        .update(&replace_patch("after", "after-source-merge"))
        .await
        .unwrap();
    let run = fixture
        .store
        .read_task_run(&fixture.run.id)
        .await
        .unwrap()
        .unwrap();
    assert!(design_commit_is_current(&run));
    assert_eq!(
        run.design_commit.as_deref(),
        Some(consistency.design_commit.as_str())
    );
    fixture.cleanup().await;
}

struct DesignFixture {
    repository: PathBuf,
    store: StudioStore,
    coordinator: Arc<TaskCoordinator>,
    session_id: String,
    run: TaskRunRecord,
}

impl DesignFixture {
    async fn new(name: &str) -> Self {
        let repository = init_repository(name);
        std::fs::create_dir_all(repository.join("design")).unwrap();
        std::fs::write(repository.join("design/spec.md"), "before\n").unwrap();
        std::fs::write(repository.join(".gitignore"), "design/ignored.md\n").unwrap();
        git(&repository, &["add", "design/spec.md", ".gitignore"]);
        git(&repository, &["commit", "-m", "initial design"]);
        let store = StudioStore::open_memory().await.unwrap();
        let project = store.upsert_project(&repository).await.unwrap();
        let session = store
            .create_session(&project.id, "Task", StudioMode::Task)
            .await
            .unwrap();
        let coordinator = Arc::new(TaskCoordinator::new(store.clone()));
        let run = coordinator
            .start_confirmed_task(&session.id, "plan", &repository)
            .await
            .unwrap();
        Self {
            repository,
            store,
            coordinator,
            session_id: session.id,
            run,
        }
    }

    async fn update(&self, patch: &str) -> anyhow::Result<DesignUpdateOutput> {
        self.coordinator
            .update_design(&self.session_id, &self.repository, patch)
            .await
    }

    fn head(&self) -> String {
        git_output(&self.repository, &["rev-parse", "HEAD"])
    }

    fn status(&self) -> String {
        git_output(
            &self.repository,
            &["status", "--porcelain=v1", "--untracked-files=all"],
        )
    }

    fn design_text(&self) -> String {
        std::fs::read_to_string(self.repository.join("design/spec.md")).unwrap()
    }

    async fn cleanup(self) {
        self.coordinator.suspend();
        let _ = std::fs::remove_dir_all(self.repository);
    }
}

fn allocation(session_id: &str, suffix: &str) -> AllocateExecutor {
    AllocateExecutor {
        session_id: session_id.to_string(),
        title: suffix.to_string(),
        owned_paths: vec![format!("src/{suffix}.rs")],
        agent_id: format!("agent-{suffix}"),
        owner_path: "/root".to_string(),
        requested_by_call_id: format!("call-{suffix}"),
    }
}

fn replace_patch(before: &str, after: &str) -> String {
    format!(
        "*** Begin Patch\n*** Update File: design/spec.md\n@@\n-{before}\n+{after}\n*** End Patch"
    )
}

async fn inject_design_transaction_failure(store: &StudioStore) {
    store
        .execute_test_sql(
            "CREATE TRIGGER fail_design_update BEFORE UPDATE OF design_commit ON task_runs \
             BEGIN SELECT RAISE(FAIL, 'injected design transaction failure'); END;",
        )
        .await;
}

#[derive(Clone, Copy)]
enum FinalScopeDrift {
    Commit,
    Branch,
    Dirty,
}

async fn assert_durable_design_final_scope_failure(name: &str, drift: FinalScopeDrift) {
    let fixture = DesignFixture::new(name).await;
    let barrier = DesignCommitTestBarrier::new();
    fixture
        .coordinator
        .set_design_after_head_persist_barrier(barrier.clone());
    let coordinator = fixture.coordinator.clone();
    let session_id = fixture.session_id.clone();
    let repository = fixture.repository.clone();
    let update = tokio::spawn(async move {
        coordinator
            .update_design(&session_id, &repository, DESIGN_PATCH)
            .await
    });
    barrier.wait_until_committed().await;
    let durable_design_head = fixture.head();
    match drift {
        FinalScopeDrift::Commit => {
            std::fs::write(fixture.repository.join("external.rs"), "external-commit\n").unwrap();
            git(&fixture.repository, &["add", "external.rs"]);
            git(
                &fixture.repository,
                &["commit", "-m", "external after durable design CAS"],
            );
        }
        FinalScopeDrift::Branch => {
            git(
                &fixture.repository,
                &["switch", "-c", "external-after-durable-cas"],
            );
        }
        FinalScopeDrift::Dirty => {
            std::fs::write(fixture.repository.join("external.rs"), "external-dirty\n").unwrap();
        }
    }
    let external_head = fixture.head();
    barrier.release().await;

    assert!(update.await.unwrap().is_err());
    let run = fixture
        .store
        .read_task_run(&fixture.run.id)
        .await
        .unwrap()
        .unwrap();
    let lease = fixture
        .store
        .read_branch_lease(&fixture.run.id)
        .await
        .unwrap();
    assert_eq!(run.phase, TaskRunPhase::Blocked);
    assert_eq!(run.expected_head, durable_design_head);
    assert_eq!(
        run.design_commit.as_deref(),
        Some(durable_design_head.as_str())
    );
    assert!(lease.is_none());
    assert_eq!(fixture.head(), external_head);
    match drift {
        FinalScopeDrift::Commit => assert_ne!(external_head, durable_design_head),
        FinalScopeDrift::Branch => assert_eq!(
            git_output(
                &fixture.repository,
                &["symbolic-ref", "--quiet", "--short", "HEAD"]
            ),
            "external-after-durable-cas"
        ),
        FinalScopeDrift::Dirty => assert_eq!(
            std::fs::read_to_string(fixture.repository.join("external.rs")).unwrap(),
            "external-dirty\n"
        ),
    }
    fixture.cleanup().await;
}

async fn assert_revert_hook_failure(name: &str, hook_name: &str) {
    let fixture = DesignFixture::new(name).await;
    let design = fixture.update(DESIGN_PATCH).await.unwrap();
    let hook = fixture.repository.join(format!(".git/hooks/{hook_name}"));
    std::fs::write(&hook, "#!/bin/sh\nexit 1\n").unwrap();
    make_executable(&hook);

    assert!(
        fixture
            .coordinator
            .revert_design_for_no_source_cancel(&fixture.run.id)
            .await
            .is_err()
    );

    let run = fixture
        .store
        .read_task_run(&fixture.run.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(run.phase, TaskRunPhase::Blocked);
    assert!(
        run.status_message
            .as_deref()
            .is_some_and(|message| message.contains("Git state was preserved"))
    );
    assert_eq!(fixture.head(), design.design_commit);
    assert_eq!(
        git_output(
            &fixture.repository,
            &["rev-parse", "--verify", "REVERT_HEAD"]
        ),
        design.design_commit
    );
    assert_eq!(fixture.design_text().replace("\r\n", "\n"), "before\n");
    assert_eq!(fixture.status(), "M  design/spec.md");
    fixture.cleanup().await;
}

fn init_repository(name: &str) -> PathBuf {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = std::env::temp_dir().join(format!(
        "pure-task-design-{name}-{}-{stamp}",
        std::process::id()
    ));
    std::fs::create_dir_all(&path).unwrap();
    git(&path, &["init"]);
    git(&path, &["checkout", "-b", "main"]);
    git(&path, &["config", "user.email", "pure@example.invalid"]);
    git(&path, &["config", "user.name", "Pure Test"]);
    std::fs::write(path.join("README.md"), "initial\n").unwrap();
    git(&path, &["add", "README.md"]);
    git(&path, &["commit", "-m", "initial"]);
    path
}

fn git(repository: &Path, args: &[&str]) {
    let output = Command::new("git")
        .arg("-C")
        .arg(repository)
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {} failed: {}",
        args.join(" "),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn git_output(repository: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .arg("-C")
        .arg(repository)
        .args(args)
        .output()
        .unwrap();
    assert!(output.status.success(), "git {} failed", args.join(" "));
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

#[cfg(unix)]
fn create_directory_symlink(target: &Path, link: &Path) {
    std::os::unix::fs::symlink(target, link).unwrap();
}

#[cfg(unix)]
fn remove_directory_symlink(link: &Path) {
    std::fs::remove_file(link).unwrap();
}

#[cfg(windows)]
fn create_directory_symlink(target: &Path, link: &Path) {
    std::os::windows::fs::symlink_dir(target, link).unwrap();
}

#[cfg(windows)]
fn remove_directory_symlink(link: &Path) {
    std::fs::remove_dir(link).unwrap();
}

#[cfg(unix)]
fn make_executable(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let mut permissions = std::fs::metadata(path).unwrap().permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(path, permissions).unwrap();
}

#[cfg(windows)]
fn make_executable(_path: &Path) {}

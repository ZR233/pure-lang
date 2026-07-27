use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use sea_orm::{ConnectionTrait, DatabaseBackend, Statement};

use super::merge::{
    MergeCleanupTestBarrier, MergeCommitTestBarrier, MergeFailureTestPoint,
    MergeVerificationCommand, MergeVerifier, select_merge_verification_commands,
};
use super::*;
use crate::tool::{
    SubagentContext, Tool, ToolContext, ToolInput, WorkspaceAccess, strict_tool_input_schema,
};
use crate::{
    AgentKernel, AgentKernelToolRequest, AgentSession, CoreAgentProfile, StudioMode, StudioStore,
    ToolEffect, TurnEngineBuilder, TurnOptions, TurnToolCacheHandle, TurnWorkingSetHandle,
};

#[derive(Debug, Clone, Copy, Default)]
struct TestRuntimeMarker;

#[tokio::test]
async fn invalid_executor_owned_paths_fail_before_product_allocation() {
    let fixture = ReviewFixture::new("invalid-executor-owned-paths").await;
    let worktree_root_existed = fixture.repository.join(".pure/worktrees").exists();
    let branches_before = git_output(&fixture.repository, &["branch", "--list"]);
    for (index, owned_paths) in [
        Vec::<String>::new(),
        vec!["../src".to_string()],
        vec!["src/*".to_string()],
        vec!["src/**".to_string(), "src/lib.rs".to_string()],
    ]
    .into_iter()
    .enumerate()
    {
        let request = StudioTaskSpawnRequest {
            agent_id: format!("invalid-executor-{index}"),
            session_id: fixture.session_id.clone(),
            task_name: "invalid executor".to_string(),
            role: "executor".to_string(),
            owned_paths,
            requested_by_call_id: format!("call-invalid-{index}"),
        };

        fixture
            .coordinator
            .prepare_agent_spawn(&request)
            .await
            .expect_err("invalid ownedPaths must fail before allocation");
    }

    assert!(
        fixture
            .store
            .list_work_units(&fixture.run_id)
            .await
            .unwrap()
            .is_empty()
    );
    assert!(
        fixture
            .store
            .list_agent_outcomes(&fixture.run_id)
            .await
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        fixture.repository.join(".pure/worktrees").exists(),
        worktree_root_existed
    );
    assert_eq!(
        git_output(&fixture.repository, &["branch", "--list"]),
        branches_before
    );
    fixture.cleanup();
}

#[tokio::test]
async fn reviewer_harness_authorization_is_one_shot_and_has_no_work_unit() {
    let fixture = ReviewFixture::new("review-harness-authorization").await;
    let round = fixture
        .store
        .begin_task_review(&fixture.session_id, "call-review")
        .await
        .unwrap();
    let request = StudioTaskSpawnRequest {
        agent_id: "agent-reviewer".to_string(),
        session_id: fixture.session_id.clone(),
        task_name: "review_round_1".to_string(),
        role: "reviewer".to_string(),
        owned_paths: Vec::new(),
        requested_by_call_id: "call-review".to_string(),
    };
    let preparation = fixture
        .coordinator
        .prepare_agent_spawn(&request)
        .await
        .unwrap();
    assert!(preparation.lifecycle_token().is_some());
    fixture
        .coordinator
        .activate_agent_spawn(&request, &preparation)
        .await
        .unwrap();
    assert!(
        fixture
            .coordinator
            .prepare_agent_spawn(&request)
            .await
            .is_err()
    );
    assert!(
        fixture
            .store
            .list_work_units(&fixture.run_id)
            .await
            .unwrap()
            .is_empty()
    );
    let outcomes = fixture
        .store
        .list_agent_outcomes(&fixture.run_id)
        .await
        .unwrap();
    assert_eq!(outcomes.len(), 1);
    assert_eq!(outcomes[0].role, "reviewer");
    assert_eq!(outcomes[0].owner_path, "/root");
    assert_eq!(outcomes[0].requested_by_call_id, "call-review");
    assert_eq!(outcomes[0].status, AgentOutcomeStatus::Running);
    let rounds = fixture
        .store
        .list_review_rounds(&fixture.run_id)
        .await
        .unwrap();
    assert_eq!(rounds[0].id, round.id);
    assert_eq!(
        rounds[0].reviewer_agent_id.as_deref(),
        Some("agent-reviewer")
    );
    fixture.cleanup();
}

#[tokio::test]
async fn review_tools_have_role_visibility_and_prompt_only_indexes_design() {
    let fixture = ReviewFixture::new("review-tool-visibility-prompt").await;
    let exit_tool = fixture
        .coordinator
        .review_exit_tool(&fixture.session_id, None);
    assert_eq!(exit_tool.effect(), Some(ToolEffect::Read));
    let run = fixture
        .store
        .read_task_run(&fixture.run_id)
        .await
        .unwrap()
        .unwrap();
    let prompt = super::review::prompt::build_review_prompt(&fixture.coordinator, &run)
        .await
        .unwrap();
    assert!(prompt.contains("review this implementation"));
    assert!(prompt.contains("- design/guide.md"));
    assert!(!prompt.contains("# Review design"));
    fixture.cleanup();
}

#[tokio::test]
async fn review_preflight_requires_design_consistency_for_current_head() {
    let fixture = ReviewFixture::new("review-design-consistency-preflight").await;
    let branch_guard = fixture.coordinator.lock_branch_mutation().await;

    let error = fixture
        .coordinator
        .preflight_task_review_locked(&fixture.session_id, &branch_guard)
        .await
        .expect_err("review must not start before final design consistency");
    assert!(
        error
            .to_string()
            .contains("task_update_design to commit final design consistency")
    );

    let run = fixture
        .store
        .read_task_run(&fixture.run_id)
        .await
        .unwrap()
        .unwrap();
    fixture
        .store
        .advance_task_design_head(&run.id, &run.expected_head, &run.expected_head)
        .await
        .unwrap();
    let ready = fixture
        .coordinator
        .preflight_task_review_locked(&fixture.session_id, &branch_guard)
        .await
        .unwrap();
    assert_eq!(
        ready.design_commit.as_deref(),
        Some(ready.expected_head.as_str())
    );

    drop(branch_guard);
    fixture.cleanup();
}

#[tokio::test]
async fn reviewer_terminal_without_review_exit_fails_round_and_restores_phase() {
    let fixture = ReviewFixture::new("reviewer-terminal-without-exit").await;
    fixture
        .store
        .begin_task_review(&fixture.session_id, "call-review-terminal")
        .await
        .unwrap();
    let (_, outcome) = fixture
        .store
        .authorize_reviewer_spawn(
            &fixture.session_id,
            "call-review-terminal",
            "agent-reviewer-terminal",
        )
        .await
        .unwrap();
    fixture
        .store
        .update_spawned_outcome(
            &outcome.id,
            "agent-reviewer-terminal",
            AgentOutcomeStatus::Running,
            None,
        )
        .await
        .unwrap();

    let recording = fixture
        .coordinator
        .record_terminal_agent_state(
            &fixture.session_id,
            &StudioAgentTerminalChange {
                agent_id: "agent-reviewer-terminal".to_string(),
                role: "reviewer".to_string(),
                outcome: crate::TurnOutcomeKind::Completed,
                summary: Some("returned text instead of review_exit".to_string()),
                error: None,
            },
        )
        .await
        .unwrap();

    assert!(matches!(
        recording,
        TerminalAgentStateRecording::Changed { .. }
    ));
    let run = fixture
        .store
        .read_task_run(&fixture.run_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(run.phase, TaskRunPhase::Implementing);
    let round = fixture
        .store
        .list_review_rounds(&fixture.run_id)
        .await
        .unwrap()
        .pop()
        .unwrap();
    assert_eq!(round.verdict, ReviewVerdict::Failed);
    let outcome = fixture
        .store
        .list_agent_outcomes(&fixture.run_id)
        .await
        .unwrap()
        .pop()
        .unwrap();
    assert_eq!(outcome.status, AgentOutcomeStatus::Failed);
    let retry = fixture
        .store
        .begin_task_review(&fixture.session_id, "call-review-retry")
        .await
        .unwrap();
    assert_eq!(retry.round, 2);
    assert_eq!(retry.head_commit, round.head_commit);
    fixture.cleanup();
}

#[tokio::test]
async fn review_exit_requires_real_design_trace_and_persists_matching_pass() {
    let fixture = ReviewFixture::new("review-exit-trace").await;
    fixture
        .store
        .begin_task_review(&fixture.session_id, "call-review-exit")
        .await
        .unwrap();
    let (_, outcome) = fixture
        .store
        .authorize_reviewer_spawn(
            &fixture.session_id,
            "call-review-exit",
            "agent-reviewer-exit",
        )
        .await
        .unwrap();
    fixture
        .store
        .update_spawned_outcome(
            &outcome.id,
            "agent-reviewer-exit",
            AgentOutcomeStatus::Running,
            None,
        )
        .await
        .unwrap();
    let history = AgentSession::from_messages(vec![
        crate::tool_result_history_message(
            "call-search".to_string(),
            "search_files".to_string(),
            r#"{"query":"review design"}"#.to_string(),
            r#"{"matches":["design/guide.md"]}"#.to_string(),
        ),
        crate::tool_result_history_message(
            "call-read".to_string(),
            "read_file".to_string(),
            r#"{"path":"design/guide.md"}"#.to_string(),
            r##"{"path":"design/guide.md","startLine":1,"endLine":1,"nextStartLine":null,"contentHash":"fixture","text":"# Review design\n"}"##.to_string(),
        ),
    ]);
    let tool = fixture
        .coordinator
        .review_exit_tool(fixture.session_id.clone(), None);
    let (event_tx, _) = tokio::sync::broadcast::channel(16);
    let output = tool
        .execute(
            ToolInput {
                arguments: serde_json::json!({
                    "verdict":"pass",
                    "summary":"implementation matches the reviewed design",
                    "designReferences":[{"path":"design/guide.md","section":"Review design"}],
                    "findings":[]
                }),
                session_id: "reviewer-turn".to_string(),
                tool_id: "call-review-exit".to_string(),
                revision_base: 0,
            },
            ToolContext {
                event_tx,
                options: TurnOptions::default(),
                workspace_access: WorkspaceAccess::WorkspaceOnly,
                workspace_root: fixture.repository.clone(),
                workspace_instructions: None,
                instruction_snapshot: None,
                provider_call_id: Some("call-review-exit".to_string()),
                active_subagent: Some(SubagentContext {
                    id: "agent-reviewer-exit".to_string(),
                    parent_id: Some("/root".to_string()),
                    agent_path: Some("/root/review_round_1".to_string()),
                    role: "reviewer".to_string(),
                    task: "review".to_string(),
                    depth: 1,
                }),
                lsp_runtime: None,
                parent_session: Arc::new(history),
                working_set: TurnWorkingSetHandle::default(),
                tool_cache: TurnToolCacheHandle::default(),
            },
        )
        .await
        .unwrap();
    assert!(output.ends_turn());
    let rounds = fixture
        .store
        .list_review_rounds(&fixture.run_id)
        .await
        .unwrap();
    assert_eq!(rounds[0].verdict, ReviewVerdict::Pass);
    assert_eq!(rounds[0].design_references[0].path, "design/guide.md");
    let outcomes = fixture
        .store
        .list_agent_outcomes(&fixture.run_id)
        .await
        .unwrap();
    assert_eq!(outcomes[0].status, AgentOutcomeStatus::Completed);
    assert_eq!(
        outcomes[0].review.as_ref().unwrap().verdict,
        ReviewVerdict::Pass
    );
    fixture.cleanup();
}

#[tokio::test]
async fn task_terminal_tools_are_typed_branch_control_and_planner_only() {
    let coordinator = Arc::new(TaskCoordinator::new(
        StudioStore::open_memory().await.unwrap(),
    ));
    let complete = coordinator.task_complete_tool("studio-session");
    assert_eq!(complete.effect(), Some(ToolEffect::BranchControl));
    assert_eq!(complete.input_schema(), strict_tool_input_schema([]));
}

#[tokio::test]
async fn completion_requires_current_design_and_pass_then_atomically_releases_lease() {
    let fixture = ReviewFixture::new("task-completion-gate").await;
    let run = fixture
        .store
        .read_task_run(&fixture.run_id)
        .await
        .unwrap()
        .unwrap();
    let error = fixture
        .store
        .complete_reviewed_task(&fixture.session_id, &run.expected_head, "verified")
        .await
        .unwrap_err();
    assert!(error.to_string().contains("reviewing task run"));
    fixture
        .store
        .advance_task_design_head(&run.id, &run.expected_head, &run.expected_head)
        .await
        .unwrap();
    fixture
        .store
        .begin_task_review(&fixture.session_id, "call-complete-review")
        .await
        .unwrap();
    let (_, outcome) = fixture
        .store
        .authorize_reviewer_spawn(
            &fixture.session_id,
            "call-complete-review",
            "agent-complete-review",
        )
        .await
        .unwrap();
    fixture
        .store
        .update_spawned_outcome(
            &outcome.id,
            "agent-complete-review",
            AgentOutcomeStatus::Running,
            None,
        )
        .await
        .unwrap();
    fixture
        .store
        .complete_task_review(
            &fixture.session_id,
            "agent-complete-review",
            AgentReview {
                verdict: ReviewVerdict::Pass,
                summary: "pass".to_string(),
                design_references: vec![ReviewDesignReference {
                    path: "design/guide.md".to_string(),
                    section: "Review design".to_string(),
                }],
                findings: Vec::new(),
            },
        )
        .await
        .unwrap();

    let completed = fixture
        .store
        .complete_reviewed_task(&fixture.session_id, &run.expected_head, "verified")
        .await
        .unwrap();

    assert_eq!(completed.phase, TaskRunPhase::Completed);
    assert_eq!(completed.status_message.as_deref(), Some("verified"));
    assert!(
        fixture
            .store
            .read_branch_lease(&fixture.run_id)
            .await
            .unwrap()
            .is_none()
    );
    fixture.cleanup();
}

#[tokio::test]
async fn task_complete_rejects_a_durable_stop_request() {
    let fixture = ReviewFixture::new("task-completion-stop-request").await;
    let run = fixture
        .store
        .read_task_run(&fixture.run_id)
        .await
        .unwrap()
        .unwrap();
    fixture
        .store
        .advance_task_design_head(&run.id, &run.expected_head, &run.expected_head)
        .await
        .unwrap();
    fixture
        .store
        .begin_task_review(&fixture.session_id, "call-stop-review")
        .await
        .unwrap();
    let (_, outcome) = fixture
        .store
        .authorize_reviewer_spawn(&fixture.session_id, "call-stop-review", "agent-stop-review")
        .await
        .unwrap();
    fixture
        .store
        .update_spawned_outcome(
            &outcome.id,
            "agent-stop-review",
            AgentOutcomeStatus::Running,
            None,
        )
        .await
        .unwrap();
    fixture
        .store
        .complete_task_review(
            &fixture.session_id,
            "agent-stop-review",
            AgentReview {
                verdict: ReviewVerdict::Pass,
                summary: "pass".to_string(),
                design_references: vec![ReviewDesignReference {
                    path: "design/guide.md".to_string(),
                    section: "Review design".to_string(),
                }],
                findings: Vec::new(),
            },
        )
        .await
        .unwrap();
    fixture
        .store
        .request_task_stop(&fixture.run_id, &run.expected_head, "user requested stop")
        .await
        .unwrap();

    let error = fixture
        .store
        .complete_reviewed_task(&fixture.session_id, &run.expected_head, "verified")
        .await
        .unwrap_err();

    assert!(error.to_string().contains("after stop was requested"));
    assert!(
        fixture
            .store
            .read_branch_lease(&fixture.run_id)
            .await
            .unwrap()
            .is_some()
    );
    fixture.cleanup();
}

#[tokio::test]
async fn task_stop_settles_transient_agents_and_atomically_releases_lease() {
    let fixture = ReviewFixture::new("task-stop-terminalization").await;
    let run = fixture
        .store
        .read_task_run(&fixture.run_id)
        .await
        .unwrap()
        .unwrap();

    fixture
        .store
        .request_task_stop(&run.id, &run.expected_head, "user stopped task")
        .await
        .unwrap();
    fixture
        .store
        .begin_task_stop(&run.id, &run.expected_head)
        .await
        .unwrap();
    fixture
        .store
        .settle_agents_for_task_stop(&run.id, "user stopped task")
        .await
        .unwrap();
    let cancelled = fixture
        .store
        .cancel_task_and_release_lease(&run.id, &run.expected_head, "user stopped task")
        .await
        .unwrap();

    assert_eq!(cancelled.phase, TaskRunPhase::Cancelled);
    assert_eq!(
        cancelled.status_message.as_deref(),
        Some("user stopped task")
    );
    assert!(
        fixture
            .store
            .read_branch_lease(&fixture.run_id)
            .await
            .unwrap()
            .is_none()
    );
    fixture.cleanup();
}

#[tokio::test]
async fn task_stop_preflight_rejects_stale_design_before_stopping() {
    let fixture = ReviewFixture::new("task-stop-stale-design-preflight").await;
    let run = fixture
        .store
        .read_task_run(&fixture.run_id)
        .await
        .unwrap()
        .unwrap();
    let merge = fixture
        .store
        .create_merge_record(CreateMergeRecord {
            task_run_id: run.id.clone(),
            agent_id: "agent-merged".to_string(),
            expected_head: run.expected_head.clone(),
            source_commit: run.expected_head.clone(),
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
                resolution_summary: None,
                verification: Some(Vec::new()),
                attempt: 1,
            },
        )
        .await
        .unwrap();
    let branch_guard = fixture.coordinator.lock_branch_mutation().await;

    let error = fixture
        .coordinator
        .preflight_task_stop_locked(&fixture.session_id, &branch_guard)
        .await
        .expect_err("stop must reject stale design before entering stopping");
    assert!(
        error
            .to_string()
            .contains("final design consistency update")
    );
    let unchanged = fixture
        .store
        .read_task_run(&fixture.run_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(unchanged.phase, TaskRunPhase::Implementing);
    assert!(!unchanged.stop_requested);

    drop(branch_guard);
    fixture.cleanup();
}

#[test]
fn production_merge_verifier_selects_repo_rust_fmt_and_flutter_project_analyze() {
    let selected = select_merge_verification_commands(
        Path::new("C:/repo"),
        &[
            "code/pl-core/src/lib.rs".to_string(),
            "code/pure-studio-flutter/windows/runner/main.cpp".to_string(),
            "code/pure-studio-flutter/lib/l10n/app_zh.arb".to_string(),
        ],
    );

    assert_eq!(
        selected,
        vec![
            MergeVerificationCommand {
                working_directory: PathBuf::from("C:/repo"),
                command: vec![
                    "cargo".to_string(),
                    "fmt".to_string(),
                    "--all".to_string(),
                    "--check".to_string(),
                ],
            },
            MergeVerificationCommand {
                working_directory: PathBuf::from("C:/repo/code/pure-studio-flutter"),
                command: vec![
                    "flutter".to_string(),
                    "--no-version-check".to_string(),
                    "analyze".to_string(),
                ],
            },
        ]
    );
}

#[tokio::test]
async fn begin_task_merge_atomically_resolves_exact_delivered_executor_scope() {
    let fixture = DeliveryFixture::new("merge-store-scope", vec!["src/**"]).await;
    fixture.commit_file("src/lib.rs");
    let source_head = git_output(&fixture.worktree, &["rev-parse", "HEAD"]);
    let delivery = fixture.submit(&source_head).await.unwrap();
    let run = fixture
        .store
        .transition_task_run(&fixture.task_run_id, TaskRunPhase::Implementing, None)
        .await
        .unwrap();

    let scope = fixture
        .store
        .begin_task_merge(BeginTaskMerge {
            session_id: fixture.session_id.clone(),
            agent_id: fixture.subagent.id.clone(),
            expected_head: run.expected_head.clone(),
            pre_index_tree: run.expected_head.clone(),
            changed_files: delivery.changed_files.clone(),
        })
        .await
        .unwrap();

    assert_eq!(scope.origin_phase, TaskRunPhase::Implementing);
    assert_eq!(scope.run.phase, TaskRunPhase::Merging);
    assert_eq!(scope.work_unit.id, fixture.work_unit_id);
    assert_eq!(scope.work_unit.status, WorkUnitStatus::Delivered);
    assert_eq!(scope.outcome.id, fixture.outcome_id);
    assert_eq!(scope.outcome.status, AgentOutcomeStatus::Completed);
    assert_eq!(scope.delivery, delivery);
    assert_eq!(scope.merge.status, MergeStatus::Pending);
    assert_eq!(scope.merge.expected_head, run.expected_head);
    assert_eq!(scope.merge.source_commit, source_head);
    let evidence = scope.merge.evidence.as_ref().unwrap();
    assert_eq!(evidence.version, 1);
    assert_eq!(evidence.origin_phase, TaskRunPhase::Implementing);
    assert_eq!(evidence.work_unit_id, fixture.work_unit_id);
    assert_eq!(evidence.outcome_id, fixture.outcome_id);
    assert_eq!(evidence.pre_index_tree, run.expected_head);
    assert_eq!(evidence.changed_files, vec!["src/lib.rs".to_string()]);
    fixture.cleanup();
}

struct PassingMergeVerifier;

impl MergeVerifier for PassingMergeVerifier {
    fn verify(
        &self,
        _request: MergeVerificationRequest,
    ) -> impl std::future::Future<Output = anyhow::Result<Vec<MergeVerificationStep>>> + Send {
        std::future::ready(Ok(vec![MergeVerificationStep {
            command: vec!["test-verifier".to_string()],
            success: true,
            output: "passed".to_string(),
        }]))
    }
}

struct FailingMergeVerifier;

impl MergeVerifier for FailingMergeVerifier {
    fn verify(
        &self,
        _request: MergeVerificationRequest,
    ) -> impl std::future::Future<Output = anyhow::Result<Vec<MergeVerificationStep>>> + Send {
        std::future::ready(Ok(vec![MergeVerificationStep {
            command: vec!["test-verifier".to_string()],
            success: false,
            output: "focused check failed".to_string(),
        }]))
    }
}

struct AbortSabotagingVerifier;

impl MergeVerifier for AbortSabotagingVerifier {
    async fn verify(
        &self,
        request: MergeVerificationRequest,
    ) -> anyhow::Result<Vec<MergeVerificationStep>> {
        let git_dir = git_output(
            Path::new(&request.workspace_root),
            &["rev-parse", "--git-dir"],
        );
        let git_dir = PathBuf::from(git_dir);
        let git_dir = if git_dir.is_absolute() {
            git_dir
        } else {
            Path::new(&request.workspace_root).join(git_dir)
        };
        std::fs::remove_file(git_dir.join("MERGE_HEAD")).unwrap();
        Ok(vec![MergeVerificationStep {
            command: vec!["sabotage-abort".to_string()],
            success: false,
            output: "injected verifier failure".to_string(),
        }])
    }
}

#[tokio::test]
async fn clean_delivery_merges_with_metadata_atomic_head_cas_and_worktree_cleanup() {
    let fixture = DeliveryFixture::new("merge-clean", vec!["src/**"]).await;
    fixture.commit_file("src/lib.rs");
    let source_head = git_output(&fixture.worktree, &["rev-parse", "HEAD"]);
    let delivery = fixture.submit(&source_head).await.unwrap();
    let run = fixture
        .store
        .transition_task_run(&fixture.task_run_id, TaskRunPhase::Implementing, None)
        .await
        .unwrap();
    let supervisor = TestRuntimeMarker;
    let (event_tx, _) = tokio::sync::broadcast::channel(16);

    let output = fixture
        .coordinator
        .merge_agent_with_verifier(
            &fixture.session_id,
            &fixture.subagent.id,
            &run.expected_head,
            &supervisor,
            &event_tx,
            "call-merge-clean",
            &PassingMergeVerifier,
        )
        .await
        .unwrap();

    assert_eq!(output.status, MergeStatus::Merged);
    assert_eq!(output.previous_head, run.expected_head);
    assert_eq!(output.source_commit, source_head);
    assert_eq!(output.changed_files, delivery.changed_files);
    let merge_head = output.new_head.as_deref().unwrap();
    assert_eq!(
        git_output(&fixture.repository, &["rev-parse", "HEAD"]),
        merge_head
    );
    assert_eq!(
        git_output(
            &fixture.repository,
            &["rev-list", "--count", "-1", merge_head]
        ),
        "1"
    );
    let parents = git_output(
        &fixture.repository,
        &["show", "-s", "--format=%P", merge_head],
    );
    assert_eq!(parents, format!("{} {}", run.expected_head, source_head));
    let message = git_output(
        &fixture.repository,
        &["show", "-s", "--format=%B", merge_head],
    );
    for trailer in [
        format!("Pure-Task-Run: {}", fixture.task_run_id),
        "Pure-Source-Agent: agent-1".to_string(),
        format!("Pure-Previous-Head: {}", run.expected_head),
        format!("Pure-Source-Commit: {source_head}"),
    ] {
        assert!(message.contains(&trailer), "missing trailer {trailer}");
    }
    let durable_run = fixture
        .store
        .read_task_run(&fixture.task_run_id)
        .await
        .unwrap()
        .unwrap();
    let durable_lease = fixture
        .store
        .read_branch_lease(&fixture.task_run_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(durable_run.phase, TaskRunPhase::Implementing);
    assert_eq!(durable_run.expected_head, merge_head);
    assert_eq!(durable_lease.expected_head, merge_head);
    assert_eq!(fixture.work_unit().await.status, WorkUnitStatus::Merged);
    assert_eq!(
        fixture.outcome().await.status,
        AgentOutcomeStatus::Completed
    );
    assert!(!fixture.worktree.exists());
    assert_eq!(output.cleanup.status, "discarded");
    fixture.cleanup();
}

#[tokio::test]
async fn commit_hook_allowed_path_rewrite_fails_full_tree_proof_and_blocks_without_reset() {
    let fixture = DeliveryFixture::new("merge-hook-tree-proof", vec!["src/**"]).await;
    fixture.commit_file("src/lib.rs");
    let source_head = git_output(&fixture.worktree, &["rev-parse", "HEAD"]);
    fixture.submit(&source_head).await.unwrap();
    let run = fixture
        .store
        .transition_task_run(&fixture.task_run_id, TaskRunPhase::Implementing, None)
        .await
        .unwrap();
    let hook = fixture.repository.join(".git/hooks/pre-commit");
    std::fs::write(
        &hook,
        "#!/bin/sh\nprintf 'hook-mutated\\n' > src/lib.rs\ngit add -- src/lib.rs\n",
    )
    .unwrap();
    make_test_hook_executable(&hook);
    let (event_tx, _) = tokio::sync::broadcast::channel(16);

    let error = fixture
        .coordinator
        .merge_agent_with_verifier(
            &fixture.session_id,
            &fixture.subagent.id,
            &run.expected_head,
            &TestRuntimeMarker,
            &event_tx,
            "call-hook-tree-proof",
            &PassingMergeVerifier,
        )
        .await
        .expect_err("commit hook rewrite must fail the captured tree proof");

    assert!(error.to_string().contains("commit proof failed"));
    let durable_run = fixture
        .store
        .read_task_run(&fixture.task_run_id)
        .await
        .unwrap()
        .unwrap();
    let record = fixture
        .store
        .list_merge_records(&fixture.task_run_id)
        .await
        .unwrap()
        .pop()
        .unwrap();
    assert_eq!(durable_run.phase, TaskRunPhase::Blocked);
    assert_eq!(record.status, MergeStatus::Failed);
    assert!(
        record
            .evidence
            .as_ref()
            .and_then(|evidence| evidence.compensation.as_deref())
            .is_some_and(|detail| detail.contains("preserved without reset"))
    );
    assert_eq!(
        std::fs::read_to_string(fixture.repository.join("src/lib.rs")).unwrap(),
        "hook-mutated\n"
    );
    assert!(fixture.worktree.exists());
    assert!(!fixture.coordinator.process_lease_is_held(&durable_run));
    fixture.cleanup();
}

#[tokio::test]
async fn post_commit_branch_or_head_drift_fails_full_proof_without_unsafe_reset() {
    for mutation in ["branch", "head"] {
        let fixture = Arc::new(
            DeliveryFixture::new(&format!("merge-proof-{mutation}"), vec!["src/**"]).await,
        );
        fixture.commit_file("src/lib.rs");
        let source_head = git_output(&fixture.worktree, &["rev-parse", "HEAD"]);
        fixture.submit(&source_head).await.unwrap();
        let run = fixture
            .store
            .transition_task_run(&fixture.task_run_id, TaskRunPhase::Implementing, None)
            .await
            .unwrap();
        let barrier = MergeCommitTestBarrier::new();
        fixture
            .coordinator
            .set_merge_before_proof_barrier(barrier.clone());
        let task_fixture = fixture.clone();
        let merge = tokio::spawn(async move {
            let (event_tx, _) = tokio::sync::broadcast::channel(16);
            task_fixture
                .coordinator
                .merge_agent_with_verifier(
                    &task_fixture.session_id,
                    &task_fixture.subagent.id,
                    &run.expected_head,
                    &TestRuntimeMarker,
                    &event_tx,
                    "call-proof-drift",
                    &PassingMergeVerifier,
                )
                .await
        });
        barrier.wait_until_committed().await;
        match mutation {
            "branch" => git(
                &fixture.repository,
                &["switch", "-c", "external-proof-branch"],
            ),
            "head" => {
                std::fs::write(fixture.repository.join("external.txt"), "external\n").unwrap();
                git(&fixture.repository, &["add", "external.txt"]);
                git(
                    &fixture.repository,
                    &["commit", "-m", "external proof head"],
                );
            }
            _ => unreachable!(),
        }
        let preserved_head = git_output(&fixture.repository, &["rev-parse", "HEAD"]);
        barrier.release().await;
        merge
            .await
            .unwrap()
            .expect_err("post-commit Git drift must fail full proof");

        let durable_run = fixture
            .store
            .read_task_run(&fixture.task_run_id)
            .await
            .unwrap()
            .unwrap();
        let record = fixture
            .store
            .list_merge_records(&fixture.task_run_id)
            .await
            .unwrap()
            .pop()
            .unwrap();
        assert_eq!(durable_run.phase, TaskRunPhase::Blocked);
        assert_eq!(record.status, MergeStatus::Failed);
        assert_eq!(
            git_output(&fixture.repository, &["rev-parse", "HEAD"]),
            preserved_head
        );
        assert!(fixture.worktree.exists());
        assert!(!fixture.coordinator.process_lease_is_held(&durable_run));
        Arc::try_unwrap(fixture).ok().unwrap().cleanup();
    }
}

#[tokio::test]
async fn accepted_merge_final_scope_drift_blocks_without_downgrading_or_cleanup() {
    for mutation in ["dirty", "branch", "head"] {
        let fixture = Arc::new(
            DeliveryFixture::new(&format!("merge-post-cas-{mutation}"), vec!["src/**"]).await,
        );
        fixture.commit_file("src/lib.rs");
        let source_head = git_output(&fixture.worktree, &["rev-parse", "HEAD"]);
        fixture.submit(&source_head).await.unwrap();
        let run = fixture
            .store
            .transition_task_run(&fixture.task_run_id, TaskRunPhase::Implementing, None)
            .await
            .unwrap();
        let barrier = MergeCommitTestBarrier::new();
        fixture
            .coordinator
            .set_merge_after_acceptance_barrier(barrier.clone());
        let task_fixture = fixture.clone();
        let merge = tokio::spawn(async move {
            let (event_tx, _) = tokio::sync::broadcast::channel(16);
            task_fixture
                .coordinator
                .merge_agent_with_verifier(
                    &task_fixture.session_id,
                    &task_fixture.subagent.id,
                    &run.expected_head,
                    &TestRuntimeMarker,
                    &event_tx,
                    "call-post-cas-drift",
                    &PassingMergeVerifier,
                )
                .await
        });
        barrier.wait_until_committed().await;
        match mutation {
            "dirty" => {
                std::fs::write(fixture.repository.join("external.txt"), "external\n").unwrap();
            }
            "branch" => {
                git(&fixture.repository, &["switch", "-c", "external-branch"]);
            }
            "head" => {
                std::fs::write(fixture.repository.join("external.txt"), "external\n").unwrap();
                git(&fixture.repository, &["add", "external.txt"]);
                git(&fixture.repository, &["commit", "-m", "external commit"]);
            }
            _ => unreachable!(),
        }
        barrier.release().await;
        merge.await.unwrap().expect_err("post-CAS drift must block");

        let durable_run = fixture
            .store
            .read_task_run(&fixture.task_run_id)
            .await
            .unwrap()
            .unwrap();
        let record = fixture
            .store
            .list_merge_records(&fixture.task_run_id)
            .await
            .unwrap()
            .pop()
            .unwrap();
        assert_eq!(durable_run.phase, TaskRunPhase::Blocked);
        assert_eq!(record.status, MergeStatus::Merged);
        assert_eq!(fixture.work_unit().await.status, WorkUnitStatus::Merged);
        assert_eq!(
            record
                .evidence
                .as_ref()
                .and_then(|evidence| evidence.cleanup.as_ref())
                .map(|cleanup| cleanup.status.as_str()),
            Some("deferred")
        );
        assert!(fixture.worktree.exists());
        assert!(!fixture.coordinator.process_lease_is_held(&durable_run));
        Arc::try_unwrap(fixture).ok().unwrap().cleanup();
    }
}

#[tokio::test]
async fn accepted_merge_task_run_read_failure_blocks_and_defers_cleanup() {
    let fixture = DeliveryFixture::new("merge-post-cas-read-failure", vec!["src/**"]).await;
    fixture.commit_file("src/lib.rs");
    let source_head = git_output(&fixture.worktree, &["rev-parse", "HEAD"]);
    fixture.submit(&source_head).await.unwrap();
    let run = fixture
        .store
        .transition_task_run(&fixture.task_run_id, TaskRunPhase::Implementing, None)
        .await
        .unwrap();
    fixture.coordinator.fail_next_merge_post_accept_read();
    let (event_tx, _) = tokio::sync::broadcast::channel(16);

    fixture
        .coordinator
        .merge_agent_with_verifier(
            &fixture.session_id,
            &fixture.subagent.id,
            &run.expected_head,
            &TestRuntimeMarker,
            &event_tx,
            "call-post-cas-read-failure",
            &PassingMergeVerifier,
        )
        .await
        .expect_err("post-CAS task run read failure must block accepted merge");

    let durable_run = fixture
        .store
        .read_task_run(&fixture.task_run_id)
        .await
        .unwrap()
        .unwrap();
    let record = fixture
        .store
        .list_merge_records(&fixture.task_run_id)
        .await
        .unwrap()
        .pop()
        .unwrap();
    assert_eq!(durable_run.phase, TaskRunPhase::Blocked);
    assert_eq!(record.status, MergeStatus::Merged);
    assert_eq!(fixture.work_unit().await.status, WorkUnitStatus::Merged);
    assert_eq!(
        record
            .evidence
            .as_ref()
            .and_then(|evidence| evidence.cleanup.as_ref())
            .map(|cleanup| cleanup.status.as_str()),
        Some("deferred")
    );
    assert!(fixture.worktree.exists());
    assert!(!fixture.coordinator.process_lease_is_held(&durable_run));
    fixture.cleanup();
}

#[tokio::test]
async fn accepted_delivery_cleanup_is_idempotent_after_resources_are_absent() {
    let fixture = DeliveryFixture::new("merge-cleanup-idempotent", vec!["src/**"]).await;
    fixture.commit_file("src/lib.rs");
    let source_head = git_output(&fixture.worktree, &["rev-parse", "HEAD"]);
    let delivery = fixture.submit(&source_head).await.unwrap();
    let run = fixture
        .store
        .transition_task_run(&fixture.task_run_id, TaskRunPhase::Implementing, None)
        .await
        .unwrap();
    let scope = fixture
        .store
        .begin_task_merge(BeginTaskMerge {
            session_id: fixture.session_id.clone(),
            agent_id: fixture.subagent.id.clone(),
            expected_head: run.expected_head,
            pre_index_tree: git_output(&fixture.repository, &["write-tree"]),
            changed_files: delivery.changed_files,
        })
        .await
        .unwrap();
    let first = merge::cleanup_accepted_delivery(&scope, None).await;
    let second = merge::cleanup_accepted_delivery(&scope, None).await;

    assert_eq!(first.status, "discarded");
    assert_eq!(second.status, "alreadyAbsent");
    assert!(!fixture.worktree.exists());
    fixture.cleanup();
}

#[tokio::test]
async fn cleanup_final_evidence_failure_replays_after_restart_without_new_merge_commit() {
    let fixture = Arc::new(IncrementalMergeFixture::new("merge-cleanup-evidence-replay").await);
    let delivered = fixture
        .deliver("agent_cleanup_replay", "src/lib.rs", "delivered\n")
        .await;
    let outcome = fixture
        .store
        .list_agent_outcomes(&fixture.run.id)
        .await
        .unwrap()
        .into_iter()
        .find(|outcome| outcome.agent_id == delivered.agent_id)
        .unwrap();
    let worktree = PathBuf::from(outcome.delivery.as_ref().unwrap().worktree.path.clone());
    let original_head = fixture.run.expected_head.clone();
    let barrier = MergeCleanupTestBarrier::new();
    fixture
        .coordinator
        .set_merge_cleanup_barrier(barrier.clone());
    let task_fixture = fixture.clone();
    let first_agent_id = delivered.agent_id.clone();
    let first_expected_head = original_head.clone();
    let merge = tokio::spawn(async move {
        let (event_tx, _) = tokio::sync::broadcast::channel(16);
        task_fixture
            .coordinator
            .merge_agent_with_verifier(
                &task_fixture.session_id,
                &first_agent_id,
                &first_expected_head,
                &TestRuntimeMarker,
                &event_tx,
                "call-cleanup-evidence-failure",
                &PassingMergeVerifier,
            )
            .await
    });
    barrier.wait_until_entered().await;
    fixture
        .store
        .execute_test_sql(
            "CREATE TRIGGER fail_cleanup_final BEFORE UPDATE ON merge_records \
             WHEN OLD.status = 'merged' BEGIN SELECT RAISE(FAIL, 'cleanup final failure'); END",
        )
        .await;
    barrier.release().await;
    merge
        .await
        .unwrap()
        .expect_err("final cleanup evidence persistence must fail");

    assert!(!worktree.exists());
    let merge_head = git_output(&fixture.repository, &["rev-parse", "HEAD"]);
    let attempting = fixture
        .store
        .list_merge_records(&fixture.run.id)
        .await
        .unwrap()
        .pop()
        .unwrap();
    assert_eq!(attempting.status, MergeStatus::Merged);
    assert_eq!(
        attempting
            .evidence
            .as_ref()
            .and_then(|evidence| evidence.cleanup.as_ref())
            .map(|cleanup| cleanup.status.as_str()),
        Some("attempting")
    );
    fixture
        .store
        .execute_test_sql("DROP TRIGGER fail_cleanup_final")
        .await;
    fixture.coordinator.suspend();
    let recovered = Arc::new(TaskCoordinator::new(fixture.store.clone()));
    let recovered_runs = recovered.recover_active_tasks().await.unwrap();
    let recovered_run = fixture
        .store
        .read_task_run(&fixture.run.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        recovered_run.phase,
        TaskRunPhase::Implementing,
        "recovered runs: {recovered_runs:?}; status: {:?}",
        recovered_run.status_message
    );
    let recovered_cleanup = fixture
        .store
        .list_merge_records(&fixture.run.id)
        .await
        .unwrap()
        .pop()
        .unwrap();
    assert_eq!(
        recovered_cleanup
            .evidence
            .as_ref()
            .and_then(|evidence| evidence.cleanup.as_ref())
            .map(|cleanup| cleanup.status.as_str()),
        Some("alreadyAbsent")
    );
    let (event_tx, _) = tokio::sync::broadcast::channel(16);

    let replayed = recovered
        .merge_agent_with_verifier(
            &fixture.session_id,
            &delivered.agent_id,
            &original_head,
            &TestRuntimeMarker,
            &event_tx,
            "call-cleanup-evidence-replay",
            &PassingMergeVerifier,
        )
        .await
        .unwrap();

    assert_eq!(replayed.status, MergeStatus::Merged);
    assert_eq!(replayed.new_head.as_deref(), Some(merge_head.as_str()));
    assert_eq!(replayed.cleanup.status, "alreadyAbsent");
    assert_eq!(
        fixture
            .store
            .list_merge_records(&fixture.run.id)
            .await
            .unwrap()
            .len(),
        1
    );
    recovered.suspend();
    Arc::try_unwrap(fixture).ok().unwrap().cleanup();
}

#[tokio::test]
async fn restart_protects_deferred_cleanup_resources_from_generic_gc() {
    let fixture = Arc::new(IncrementalMergeFixture::new("merge-deferred-restart-protect").await);
    let delivered = fixture
        .deliver("agent_deferred_restart", "src/lib.rs", "delivered\n")
        .await;
    let outcome = fixture
        .store
        .list_agent_outcomes(&fixture.run.id)
        .await
        .unwrap()
        .into_iter()
        .find(|outcome| outcome.agent_id == delivered.agent_id)
        .unwrap();
    let delivery = outcome.delivery.unwrap();
    let worktree = PathBuf::from(&delivery.worktree.path);
    let barrier = MergeCommitTestBarrier::new();
    fixture
        .coordinator
        .set_merge_after_acceptance_barrier(barrier.clone());
    let task_fixture = fixture.clone();
    let merge = tokio::spawn(async move {
        let (event_tx, _) = tokio::sync::broadcast::channel(16);
        task_fixture
            .coordinator
            .merge_agent_with_verifier(
                &task_fixture.session_id,
                &delivered.agent_id,
                &task_fixture.run.expected_head,
                &TestRuntimeMarker,
                &event_tx,
                "call-deferred-restart",
                &PassingMergeVerifier,
            )
            .await
    });
    barrier.wait_until_committed().await;
    std::fs::write(fixture.repository.join("external.txt"), "external\n").unwrap();
    barrier.release().await;
    merge
        .await
        .unwrap()
        .expect_err("dirty post-CAS scope must defer cleanup");
    std::fs::remove_file(fixture.repository.join("external.txt")).unwrap();
    fixture.coordinator.suspend();
    let recovered = TaskCoordinator::new(fixture.store.clone());

    assert!(recovered.recover_active_tasks().await.unwrap().is_empty());
    assert!(worktree.exists());
    assert_eq!(
        git_output(
            &fixture.repository,
            &[
                "rev-parse",
                &format!("refs/heads/{}", delivery.worktree.branch)
            ]
        ),
        delivery.head_commit
    );
    recovered.suspend();
    Arc::try_unwrap(fixture).ok().unwrap().cleanup();
}

#[tokio::test]
async fn restart_protects_failed_cleanup_tip_drift_without_deleting_evidence() {
    let fixture = Arc::new(IncrementalMergeFixture::new("merge-failed-restart-protect").await);
    let delivered = fixture
        .deliver("agent_failed_restart", "src/lib.rs", "delivered\n")
        .await;
    let outcome = fixture
        .store
        .list_agent_outcomes(&fixture.run.id)
        .await
        .unwrap()
        .into_iter()
        .find(|outcome| outcome.agent_id == delivered.agent_id)
        .unwrap();
    let delivery = outcome.delivery.unwrap();
    let worktree = PathBuf::from(&delivery.worktree.path);
    let barrier = MergeCleanupTestBarrier::new();
    fixture
        .coordinator
        .set_merge_cleanup_barrier(barrier.clone());
    let task_fixture = fixture.clone();
    let merge = tokio::spawn(async move {
        let (event_tx, _) = tokio::sync::broadcast::channel(16);
        task_fixture
            .coordinator
            .merge_agent_with_verifier(
                &task_fixture.session_id,
                &delivered.agent_id,
                &task_fixture.run.expected_head,
                &TestRuntimeMarker,
                &event_tx,
                "call-failed-restart",
                &PassingMergeVerifier,
            )
            .await
    });
    barrier.wait_until_entered().await;
    git(
        &fixture.repository,
        &[
            "update-ref",
            &format!("refs/heads/{}", delivery.worktree.branch),
            &delivery.base_commit,
            &delivery.head_commit,
        ],
    );
    barrier.release().await;
    let output = merge.await.unwrap().unwrap();
    assert_eq!(output.cleanup.status, "failed");
    fixture.coordinator.suspend();
    let recovered = TaskCoordinator::new(fixture.store.clone());

    assert!(recovered.recover_active_tasks().await.unwrap().is_empty());
    let durable_run = fixture
        .store
        .read_task_run(&fixture.run.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(durable_run.phase, TaskRunPhase::Blocked);
    assert!(worktree.exists());
    assert_eq!(
        git_output(
            &fixture.repository,
            &[
                "rev-parse",
                &format!("refs/heads/{}", delivery.worktree.branch)
            ]
        ),
        delivery.base_commit
    );
    recovered.suspend();
    Arc::try_unwrap(fixture).ok().unwrap().cleanup();
}

#[tokio::test]
async fn verifier_failure_aborts_to_exact_prestate_blocks_and_preserves_delivery() {
    let fixture = DeliveryFixture::new("merge-verifier-failure", vec!["src/**"]).await;
    fixture.commit_file("src/lib.rs");
    let source_head = git_output(&fixture.worktree, &["rev-parse", "HEAD"]);
    fixture.submit(&source_head).await.unwrap();
    let run = fixture
        .store
        .transition_task_run(&fixture.task_run_id, TaskRunPhase::Implementing, None)
        .await
        .unwrap();
    let (event_tx, _) = tokio::sync::broadcast::channel(16);

    let error = fixture
        .coordinator
        .merge_agent_with_verifier(
            &fixture.session_id,
            &fixture.subagent.id,
            &run.expected_head,
            &TestRuntimeMarker,
            &event_tx,
            "call-merge-verifier-failure",
            &FailingMergeVerifier,
        )
        .await
        .expect_err("failed coordinator verification must reject merge");

    assert!(error.to_string().contains("verification"));
    assert_eq!(
        git_output(&fixture.repository, &["rev-parse", "HEAD"]),
        run.expected_head
    );
    assert!(
        git_output(
            &fixture.repository,
            &["status", "--porcelain=v1", "--untracked-files=all"]
        )
        .is_empty()
    );
    let merge_head = Command::new("git")
        .arg("-C")
        .arg(&fixture.repository)
        .args(["rev-parse", "--verify", "MERGE_HEAD"])
        .output()
        .unwrap();
    assert!(!merge_head.status.success());
    let durable_run = fixture
        .store
        .read_task_run(&fixture.task_run_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(durable_run.phase, TaskRunPhase::Blocked);
    assert_eq!(fixture.work_unit().await.status, WorkUnitStatus::Delivered);
    assert_eq!(
        fixture.outcome().await.status,
        AgentOutcomeStatus::Completed
    );
    assert!(fixture.worktree.exists());
    let merges = fixture
        .store
        .list_merge_records(&fixture.task_run_id)
        .await
        .unwrap();
    assert_eq!(merges.len(), 1);
    assert_eq!(merges[0].status, MergeStatus::Failed);
    assert!(!fixture.coordinator.process_lease_is_held(&durable_run));
    assert!(
        fixture
            .store
            .read_branch_lease(&durable_run.id)
            .await
            .unwrap()
            .is_none()
    );
    fixture.cleanup();
}

#[tokio::test]
async fn abort_failure_still_persists_failed_blocked_evidence_and_releases_lease() {
    let fixture = DeliveryFixture::new("merge-abort-failure", vec!["src/**"]).await;
    fixture.commit_file("src/lib.rs");
    let source_head = git_output(&fixture.worktree, &["rev-parse", "HEAD"]);
    fixture.submit(&source_head).await.unwrap();
    let run = fixture
        .store
        .transition_task_run(&fixture.task_run_id, TaskRunPhase::Implementing, None)
        .await
        .unwrap();
    let (event_tx, _) = tokio::sync::broadcast::channel(16);

    fixture
        .coordinator
        .merge_agent_with_verifier(
            &fixture.session_id,
            &fixture.subagent.id,
            &run.expected_head,
            &TestRuntimeMarker,
            &event_tx,
            "call-abort-failure",
            &AbortSabotagingVerifier,
        )
        .await
        .expect_err("sabotaged abort must still fail durably");

    let durable_run = fixture
        .store
        .read_task_run(&fixture.task_run_id)
        .await
        .unwrap()
        .unwrap();
    let record = fixture
        .store
        .list_merge_records(&fixture.task_run_id)
        .await
        .unwrap()
        .pop()
        .unwrap();
    assert_eq!(durable_run.phase, TaskRunPhase::Blocked);
    assert_eq!(record.status, MergeStatus::Failed);
    assert!(
        record
            .evidence
            .as_ref()
            .and_then(|evidence| evidence.compensation.as_deref())
            .is_some_and(|detail| detail.contains("unsafe abort failure"))
    );
    assert!(!fixture.coordinator.process_lease_is_held(&durable_run));
    assert!(fixture.worktree.exists());
    fixture.cleanup();
}

#[tokio::test]
async fn abort_poststate_drift_still_persists_failed_blocked_evidence() {
    let fixture = Arc::new(DeliveryFixture::new("merge-abort-poststate", vec!["src/**"]).await);
    fixture.commit_file("src/lib.rs");
    let source_head = git_output(&fixture.worktree, &["rev-parse", "HEAD"]);
    fixture.submit(&source_head).await.unwrap();
    let run = fixture
        .store
        .transition_task_run(&fixture.task_run_id, TaskRunPhase::Implementing, None)
        .await
        .unwrap();
    let barrier = MergeCommitTestBarrier::new();
    fixture
        .coordinator
        .set_merge_after_abort_barrier(barrier.clone());
    let task_fixture = fixture.clone();
    let merge = tokio::spawn(async move {
        let (event_tx, _) = tokio::sync::broadcast::channel(16);
        task_fixture
            .coordinator
            .merge_agent_with_verifier(
                &task_fixture.session_id,
                &task_fixture.subagent.id,
                &run.expected_head,
                &TestRuntimeMarker,
                &event_tx,
                "call-abort-poststate",
                &FailingMergeVerifier,
            )
            .await
    });
    barrier.wait_until_committed().await;
    std::fs::write(fixture.repository.join("external.txt"), "external\n").unwrap();
    barrier.release().await;
    merge
        .await
        .unwrap()
        .expect_err("post-abort drift must still fail durably");

    let durable_run = fixture
        .store
        .read_task_run(&fixture.task_run_id)
        .await
        .unwrap()
        .unwrap();
    let record = fixture
        .store
        .list_merge_records(&fixture.task_run_id)
        .await
        .unwrap()
        .pop()
        .unwrap();
    assert_eq!(durable_run.phase, TaskRunPhase::Blocked);
    assert_eq!(record.status, MergeStatus::Failed);
    assert!(
        record
            .evidence
            .as_ref()
            .and_then(|evidence| evidence.compensation.as_deref())
            .is_some_and(|detail| detail.contains("prestate validation failed"))
    );
    assert!(fixture.repository.join("external.txt").exists());
    assert!(!fixture.coordinator.process_lease_is_held(&durable_run));
    Arc::try_unwrap(fixture).ok().unwrap().cleanup();
}

#[tokio::test]
async fn accepted_merge_releases_branch_guard_before_worktree_cleanup() {
    let fixture = Arc::new(DeliveryFixture::new("merge-cleanup-lock", vec!["src/**"]).await);
    fixture.commit_file("src/lib.rs");
    let source_head = git_output(&fixture.worktree, &["rev-parse", "HEAD"]);
    fixture.submit(&source_head).await.unwrap();
    let run = fixture
        .store
        .transition_task_run(&fixture.task_run_id, TaskRunPhase::Implementing, None)
        .await
        .unwrap();
    let barrier = MergeCleanupTestBarrier::new();
    fixture
        .coordinator
        .set_merge_cleanup_barrier(barrier.clone());
    let task_fixture = fixture.clone();
    let merge = tokio::spawn(async move {
        let (event_tx, _) = tokio::sync::broadcast::channel(16);
        task_fixture
            .coordinator
            .merge_agent_with_verifier(
                &task_fixture.session_id,
                &task_fixture.subagent.id,
                &run.expected_head,
                &TestRuntimeMarker,
                &event_tx,
                "call-cleanup-lock",
                &PassingMergeVerifier,
            )
            .await
    });

    barrier.wait_until_entered().await;
    let guard = tokio::time::timeout(
        std::time::Duration::from_secs(1),
        fixture.coordinator.lock_branch_mutation(),
    )
    .await
    .expect("cleanup must not retain the branch mutation guard");
    drop(guard);
    barrier.release().await;
    merge.await.unwrap().unwrap();

    Arc::try_unwrap(fixture).ok().unwrap().cleanup();
}

#[tokio::test]
async fn durable_merge_cas_failure_compensates_exact_clean_merge_commit() {
    let fixture = DeliveryFixture::new("merge-cas-compensation", vec!["src/**"]).await;
    fixture.commit_file("src/lib.rs");
    let source_head = git_output(&fixture.worktree, &["rev-parse", "HEAD"]);
    fixture.submit(&source_head).await.unwrap();
    let run = fixture
        .store
        .transition_task_run(&fixture.task_run_id, TaskRunPhase::Implementing, None)
        .await
        .unwrap();
    fixture
        .store
        .execute_test_sql(
            "CREATE TRIGGER fail_merge_work_unit_cas BEFORE UPDATE OF status ON work_units \
             WHEN NEW.status = 'merged' BEGIN SELECT RAISE(FAIL, 'injected merge CAS failure'); END;",
        )
        .await;
    let (event_tx, _) = tokio::sync::broadcast::channel(16);

    let error = fixture
        .coordinator
        .merge_agent_with_verifier(
            &fixture.session_id,
            &fixture.subagent.id,
            &run.expected_head,
            &TestRuntimeMarker,
            &event_tx,
            "call-merge-cas-failure",
            &PassingMergeVerifier,
        )
        .await
        .expect_err("durable CAS failure must reject and compensate the Git commit");

    assert!(error.to_string().contains("durable merge CAS failed"));
    assert_eq!(
        git_output(&fixture.repository, &["rev-parse", "HEAD"]),
        run.expected_head
    );
    assert!(
        git_output(
            &fixture.repository,
            &["status", "--porcelain=v1", "--untracked-files=all"]
        )
        .is_empty()
    );
    let durable_run = fixture
        .store
        .read_task_run(&fixture.task_run_id)
        .await
        .unwrap()
        .unwrap();
    let lease = fixture
        .store
        .read_branch_lease(&fixture.task_run_id)
        .await
        .unwrap();
    assert_eq!(durable_run.phase, TaskRunPhase::Blocked);
    assert_eq!(durable_run.expected_head, run.expected_head);
    assert!(lease.is_none());
    assert_eq!(fixture.work_unit().await.status, WorkUnitStatus::Delivered);
    let merge = fixture
        .store
        .list_merge_records(&fixture.task_run_id)
        .await
        .unwrap()
        .pop()
        .unwrap();
    assert_eq!(
        merge.status,
        MergeStatus::Failed,
        "run status: {:?}",
        durable_run.status_message
    );
    assert!(
        merge
            .evidence
            .as_ref()
            .and_then(|evidence| evidence.compensation.as_deref())
            .is_some_and(|detail| detail.contains("reset to previous HEAD"))
    );
    fixture.cleanup();
}

#[tokio::test]
async fn mark_verifying_persistence_failure_aborts_blocks_and_releases_lease() {
    let fixture = DeliveryFixture::new("merge-mark-verifying-failure", vec!["src/**"]).await;
    fixture.commit_file("src/lib.rs");
    let source_head = git_output(&fixture.worktree, &["rev-parse", "HEAD"]);
    fixture.submit(&source_head).await.unwrap();
    let run = fixture
        .store
        .transition_task_run(&fixture.task_run_id, TaskRunPhase::Implementing, None)
        .await
        .unwrap();
    fixture
        .store
        .execute_test_sql(
            "CREATE TRIGGER fail_mark_verifying BEFORE UPDATE OF status ON merge_records \
             WHEN NEW.status = 'verifying' BEGIN SELECT RAISE(FAIL, 'injected mark verifying failure'); END;",
        )
        .await;
    let (event_tx, _) = tokio::sync::broadcast::channel(16);

    fixture
        .coordinator
        .merge_agent_with_verifier(
            &fixture.session_id,
            &fixture.subagent.id,
            &run.expected_head,
            &TestRuntimeMarker,
            &event_tx,
            "call-mark-verifying-failure",
            &PassingMergeVerifier,
        )
        .await
        .expect_err("mark-verifying persistence failure must close the merge lifecycle");

    fixture
        .store
        .execute_test_sql("DROP TRIGGER fail_mark_verifying")
        .await;
    let durable_run = fixture
        .store
        .read_task_run(&fixture.task_run_id)
        .await
        .unwrap()
        .unwrap();
    let merge = fixture
        .store
        .list_merge_records(&fixture.task_run_id)
        .await
        .unwrap()
        .pop()
        .unwrap();
    assert_eq!(durable_run.phase, TaskRunPhase::Blocked);
    assert_eq!(merge.status, MergeStatus::Failed);
    assert_eq!(
        git_output(&fixture.repository, &["rev-parse", "HEAD"]),
        run.expected_head
    );
    assert!(
        git_output(
            &fixture.repository,
            &["status", "--porcelain=v1", "--untracked-files=all"]
        )
        .is_empty()
    );
    assert!(!fixture.coordinator.process_lease_is_held(&durable_run));
    fixture.cleanup();
}

#[tokio::test]
async fn clean_merge_infrastructure_failures_close_the_durable_lifecycle() {
    for (name, point) in [
        ("merge-write-tree-failure", MergeFailureTestPoint::WriteTree),
        (
            "merge-commit-start-failure",
            MergeFailureTestPoint::CommitRunnerBeforeStart,
        ),
        (
            "merge-commit-finished-failure",
            MergeFailureTestPoint::CommitRunnerAfterSuccess,
        ),
        (
            "merge-post-commit-head-failure",
            MergeFailureTestPoint::PostCommitRevParse,
        ),
    ] {
        let fixture = DeliveryFixture::new(name, vec!["src/**"]).await;
        fixture.commit_file("src/lib.rs");
        let source_head = git_output(&fixture.worktree, &["rev-parse", "HEAD"]);
        fixture.submit(&source_head).await.unwrap();
        let run = fixture
            .store
            .transition_task_run(&fixture.task_run_id, TaskRunPhase::Implementing, None)
            .await
            .unwrap();
        fixture.coordinator.fail_next_merge_at(point);
        let (event_tx, _) = tokio::sync::broadcast::channel(16);

        fixture
            .coordinator
            .merge_agent_with_verifier(
                &fixture.session_id,
                &fixture.subagent.id,
                &run.expected_head,
                &TestRuntimeMarker,
                &event_tx,
                "call-clean-infrastructure-failure",
                &PassingMergeVerifier,
            )
            .await
            .expect_err("post-merge infrastructure failure must close the merge lifecycle");

        let durable_run = fixture
            .store
            .read_task_run(&fixture.task_run_id)
            .await
            .unwrap()
            .unwrap();
        let merge = fixture
            .store
            .list_merge_records(&fixture.task_run_id)
            .await
            .unwrap()
            .pop()
            .unwrap();
        assert_eq!(durable_run.phase, TaskRunPhase::Blocked, "{name}");
        assert_eq!(merge.status, MergeStatus::Failed, "{name}");
        assert!(!fixture.coordinator.process_lease_is_held(&durable_run));
        assert_eq!(
            git_output(&fixture.repository, &["rev-parse", "HEAD"]),
            run.expected_head,
            "{name}"
        );
        assert!(
            git_output(
                &fixture.repository,
                &["status", "--porcelain=v1", "--untracked-files=all"]
            )
            .is_empty(),
            "{name}"
        );
        fixture.cleanup();
    }
}

#[tokio::test]
async fn conflict_infrastructure_failures_abort_block_and_release_lease() {
    for (name, point) in [
        (
            "merge-conflict-manifest-failure",
            MergeFailureTestPoint::ConflictManifest,
        ),
        (
            "merge-conflict-persistence-failure",
            MergeFailureTestPoint::ConflictPersistence,
        ),
    ] {
        let fixture = ConflictMergeFixture::text(name).await;
        fixture.coordinator.fail_next_merge_at(point);
        let (event_tx, _) = tokio::sync::broadcast::channel(16);

        fixture
            .coordinator
            .merge_agent_with_verifier(
                &fixture.session_id,
                &fixture.agent_id,
                &fixture.expected_head,
                &TestRuntimeMarker,
                &event_tx,
                "call-conflict-infrastructure-failure",
                &PassingMergeVerifier,
            )
            .await
            .expect_err("conflict infrastructure failure must close the merge lifecycle");

        let durable_run = fixture
            .store
            .read_task_run(&fixture.task_run_id)
            .await
            .unwrap()
            .unwrap();
        let merge = fixture
            .store
            .list_merge_records(&fixture.task_run_id)
            .await
            .unwrap()
            .pop()
            .unwrap();
        assert_eq!(durable_run.phase, TaskRunPhase::Blocked, "{name}");
        assert_eq!(merge.status, MergeStatus::Failed, "{name}");
        assert!(!fixture.coordinator.process_lease_is_held(&durable_run));
        assert_eq!(
            git_output(&fixture.repository, &["rev-parse", "HEAD"]),
            fixture.expected_head,
            "{name}"
        );
        assert!(
            git_output(
                &fixture.repository,
                &["status", "--porcelain=v1", "--untracked-files=all"]
            )
            .is_empty(),
            "{name}"
        );
        fixture.cleanup_conflict();
    }
}

#[tokio::test]
async fn merge_failure_persistence_falls_back_to_exact_run_block() {
    let fixture = DeliveryFixture::new("merge-failure-persistence-fallback", vec!["src/**"]).await;
    fixture.commit_file("src/lib.rs");
    let source_head = git_output(&fixture.worktree, &["rev-parse", "HEAD"]);
    fixture.submit(&source_head).await.unwrap();
    let run = fixture
        .store
        .transition_task_run(&fixture.task_run_id, TaskRunPhase::Implementing, None)
        .await
        .unwrap();
    fixture
        .coordinator
        .fail_next_merge_at(MergeFailureTestPoint::FailurePersistence);
    let (event_tx, _) = tokio::sync::broadcast::channel(16);

    fixture
        .coordinator
        .merge_agent_with_verifier(
            &fixture.session_id,
            &fixture.subagent.id,
            &run.expected_head,
            &TestRuntimeMarker,
            &event_tx,
            "call-failure-persistence-fallback",
            &FailingMergeVerifier,
        )
        .await
        .expect_err("failure persistence error must fall back to blocking the exact run");

    let durable_run = fixture
        .store
        .read_task_run(&fixture.task_run_id)
        .await
        .unwrap()
        .unwrap();
    let merge = fixture
        .store
        .list_merge_records(&fixture.task_run_id)
        .await
        .unwrap()
        .pop()
        .unwrap();
    assert_eq!(durable_run.phase, TaskRunPhase::Blocked);
    assert!(matches!(
        merge.status,
        MergeStatus::Pending | MergeStatus::Verifying
    ));
    assert!(!fixture.coordinator.process_lease_is_held(&durable_run));
    assert_eq!(
        git_output(&fixture.repository, &["rev-parse", "HEAD"]),
        run.expected_head
    );
    fixture.cleanup();
}

#[tokio::test]
async fn unsafe_merge_cas_compensation_blocks_without_overwriting_external_dirty_change() {
    let fixture = Arc::new(DeliveryFixture::new("merge-cas-unsafe", vec!["src/**"]).await);
    fixture.commit_file("src/lib.rs");
    let source_head = git_output(&fixture.worktree, &["rev-parse", "HEAD"]);
    fixture.submit(&source_head).await.unwrap();
    let run = fixture
        .store
        .transition_task_run(&fixture.task_run_id, TaskRunPhase::Implementing, None)
        .await
        .unwrap();
    fixture
        .store
        .execute_test_sql(
            "CREATE TRIGGER fail_unsafe_merge_cas BEFORE UPDATE OF status ON work_units \
             WHEN NEW.status = 'merged' BEGIN SELECT RAISE(FAIL, 'injected unsafe CAS failure'); END;",
        )
        .await;
    let barrier = MergeCommitTestBarrier::new();
    fixture
        .coordinator
        .set_merge_after_commit_barrier(barrier.clone());
    let task_fixture = fixture.clone();
    let merge = tokio::spawn(async move {
        let (event_tx, _) = tokio::sync::broadcast::channel(16);
        task_fixture
            .coordinator
            .merge_agent_with_verifier(
                &task_fixture.session_id,
                &task_fixture.subagent.id,
                &run.expected_head,
                &TestRuntimeMarker,
                &event_tx,
                "call-merge-cas-unsafe",
                &PassingMergeVerifier,
            )
            .await
    });
    barrier.wait_until_committed().await;
    std::fs::write(fixture.repository.join("external.txt"), "preserve me\n").unwrap();
    let merge_commit = git_output(&fixture.repository, &["rev-parse", "HEAD"]);
    barrier.release().await;

    let error = merge
        .await
        .unwrap()
        .expect_err("unsafe CAS compensation must reject the merge");
    assert!(error.to_string().contains("durable merge CAS failed"));
    assert_eq!(
        std::fs::read_to_string(fixture.repository.join("external.txt")).unwrap(),
        "preserve me\n"
    );
    assert_eq!(
        git_output(&fixture.repository, &["rev-parse", "HEAD"]),
        merge_commit
    );
    let durable_run = fixture
        .store
        .read_task_run(&fixture.task_run_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(durable_run.phase, TaskRunPhase::Blocked);
    assert_eq!(durable_run.expected_head, fixture.base_commit);
    let record = fixture
        .store
        .list_merge_records(&fixture.task_run_id)
        .await
        .unwrap()
        .pop()
        .unwrap();
    assert_eq!(record.status, MergeStatus::Failed);
    assert!(
        record
            .evidence
            .as_ref()
            .and_then(|evidence| evidence.compensation.as_deref())
            .is_some_and(|detail| detail.contains("unsafe"))
    );
    assert!(fixture.worktree.exists());

    std::fs::remove_file(fixture.repository.join("external.txt")).unwrap();
    Arc::try_unwrap(fixture).ok().unwrap().cleanup();
}

#[tokio::test]
async fn two_deliveries_from_same_base_merge_incrementally_and_are_consumed_once() {
    let fixture = IncrementalMergeFixture::new("merge-incremental").await;
    let first = fixture
        .deliver("agent-first", "src/first.rs", "first\n")
        .await;
    let second = fixture
        .deliver("agent-second", "src/second.rs", "second\n")
        .await;
    let initial_head = fixture.run.expected_head.clone();
    let (event_tx, _) = tokio::sync::broadcast::channel(16);

    let first_output = fixture
        .coordinator
        .merge_agent_with_verifier(
            &fixture.session_id,
            &first.agent_id,
            &initial_head,
            &TestRuntimeMarker,
            &event_tx,
            "call-first",
            &PassingMergeVerifier,
        )
        .await
        .unwrap();
    let first_head = first_output.new_head.unwrap();
    let second_output = fixture
        .coordinator
        .merge_agent_with_verifier(
            &fixture.session_id,
            &second.agent_id,
            &first_head,
            &TestRuntimeMarker,
            &event_tx,
            "call-second",
            &PassingMergeVerifier,
        )
        .await
        .unwrap();
    let second_head = second_output.new_head.unwrap();

    assert_ne!(first_head, initial_head);
    assert_ne!(second_head, first_head);
    assert_eq!(
        std::fs::read_to_string(fixture.repository.join("src/first.rs")).unwrap(),
        "first\n"
    );
    assert_eq!(
        std::fs::read_to_string(fixture.repository.join("src/second.rs")).unwrap(),
        "second\n"
    );
    assert_eq!(
        fixture
            .store
            .read_work_unit(&first.work_unit_id)
            .await
            .unwrap()
            .unwrap()
            .status,
        WorkUnitStatus::Merged
    );
    assert_eq!(
        fixture
            .store
            .read_work_unit(&second.work_unit_id)
            .await
            .unwrap()
            .unwrap()
            .status,
        WorkUnitStatus::Merged
    );
    assert_eq!(
        fixture
            .store
            .list_merge_records(&fixture.run.id)
            .await
            .unwrap()
            .len(),
        2
    );
    let before_repeat = git_output(&fixture.repository, &["rev-parse", "HEAD"]);
    assert!(
        fixture
            .coordinator
            .merge_agent_with_verifier(
                &fixture.session_id,
                &first.agent_id,
                &second_head,
                &TestRuntimeMarker,
                &event_tx,
                "call-first-repeat",
                &PassingMergeVerifier,
            )
            .await
            .is_err()
    );
    assert_eq!(
        git_output(&fixture.repository, &["rev-parse", "HEAD"]),
        before_repeat
    );
    fixture.cleanup();
}

#[tokio::test]
async fn older_accepted_cleanup_replays_after_later_delivery_advances_task_head() {
    let fixture = Arc::new(IncrementalMergeFixture::new("merge-old-cleanup-replay").await);
    let first = fixture
        .deliver("agent_old_cleanup", "src/first.rs", "first\n")
        .await;
    let second = fixture
        .deliver("agent_later_merge", "src/second.rs", "second\n")
        .await;
    let initial_head = fixture.run.expected_head.clone();
    let barrier = MergeCleanupTestBarrier::new();
    fixture
        .coordinator
        .set_merge_cleanup_barrier(barrier.clone());
    let task_fixture = fixture.clone();
    let first_agent_id = first.agent_id.clone();
    let first_merge = tokio::spawn(async move {
        let (event_tx, _) = tokio::sync::broadcast::channel(16);
        task_fixture
            .coordinator
            .merge_agent_with_verifier(
                &task_fixture.session_id,
                &first_agent_id,
                &initial_head,
                &TestRuntimeMarker,
                &event_tx,
                "call-old-cleanup-first",
                &PassingMergeVerifier,
            )
            .await
    });
    barrier.wait_until_entered().await;
    fixture
        .store
        .execute_test_sql(
            "CREATE TRIGGER fail_old_cleanup_final BEFORE UPDATE ON merge_records \
             WHEN OLD.status = 'merged' BEGIN SELECT RAISE(FAIL, 'old cleanup final failure'); END",
        )
        .await;
    barrier.release().await;
    first_merge
        .await
        .unwrap()
        .expect_err("first cleanup final persistence must fail");
    fixture
        .store
        .execute_test_sql("DROP TRIGGER fail_old_cleanup_final")
        .await;
    let first_head = git_output(&fixture.repository, &["rev-parse", "HEAD"]);
    let (event_tx, _) = tokio::sync::broadcast::channel(16);
    let second_output = fixture
        .coordinator
        .merge_agent_with_verifier(
            &fixture.session_id,
            &second.agent_id,
            &first_head,
            &TestRuntimeMarker,
            &event_tx,
            "call-later-merge",
            &PassingMergeVerifier,
        )
        .await
        .unwrap();
    let current_head = second_output.new_head.unwrap();

    let replayed = fixture
        .coordinator
        .merge_agent_with_verifier(
            &fixture.session_id,
            &first.agent_id,
            &fixture.run.expected_head,
            &TestRuntimeMarker,
            &event_tx,
            "call-old-cleanup-replay",
            &PassingMergeVerifier,
        )
        .await
        .unwrap();

    assert_eq!(replayed.status, MergeStatus::Merged);
    assert_eq!(replayed.cleanup.status, "alreadyAbsent");
    assert_eq!(
        git_output(&fixture.repository, &["rev-parse", "HEAD"]),
        current_head
    );
    assert_eq!(
        fixture
            .store
            .list_merge_records(&fixture.run.id)
            .await
            .unwrap()
            .len(),
        2
    );
    Arc::try_unwrap(fixture).ok().unwrap().cleanup();
}

#[tokio::test]
async fn text_conflict_persists_stage_manifest_keeps_merge_state_and_queues_once() {
    let fixture = ConflictMergeFixture::text("merge-text-conflict").await;
    let (event_tx, _) = tokio::sync::broadcast::channel(16);

    let output = fixture
        .coordinator
        .merge_agent_with_verifier(
            &fixture.session_id,
            &fixture.agent_id,
            &fixture.expected_head,
            &TestRuntimeMarker,
            &event_tx,
            "call-text-conflict",
            &PassingMergeVerifier,
        )
        .await
        .unwrap();

    assert_eq!(output.status, MergeStatus::Conflicted);
    assert_eq!(output.conflict_files, vec!["src/shared.txt".to_string()]);
    assert_eq!(
        git_output(&fixture.repository, &["rev-parse", "HEAD"]),
        fixture.expected_head
    );
    assert_eq!(
        git_output(
            &fixture.repository,
            &["rev-parse", "--verify", "MERGE_HEAD"]
        ),
        fixture.source_head
    );
    let run = fixture
        .store
        .read_task_run(&fixture.task_run_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(run.phase, TaskRunPhase::ResolvingConflict);
    let record = fixture
        .store
        .list_merge_records(&fixture.task_run_id)
        .await
        .unwrap()
        .pop()
        .unwrap();
    assert_eq!(record.status, MergeStatus::Conflicted);
    let manifest = record
        .evidence
        .as_ref()
        .and_then(|evidence| evidence.conflict_manifest.as_ref())
        .unwrap();
    assert_eq!(manifest.merge_head, fixture.source_head);
    assert_eq!(manifest.conflicts.len(), 1);
    assert_eq!(manifest.conflicts[0].kind, ConflictKind::Text);
    assert_eq!(
        manifest.conflicts[0]
            .stages
            .iter()
            .map(|stage| stage.stage)
            .collect::<Vec<_>>(),
        vec![1, 2, 3]
    );
    assert!(fixture.worktree.exists());
    let conflict_claim = fixture
        .store
        .claim_merge_conflict_continuation(&fixture.session_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(conflict_claim.task_run_id, fixture.task_run_id);
    assert_eq!(conflict_claim.agent_id, fixture.agent_id);
    assert_eq!(
        fixture
            .store
            .claim_merge_conflict_continuation(&fixture.session_id)
            .await
            .unwrap(),
        None
    );
    fixture.cleanup_conflict();
}

#[tokio::test]
async fn planner_conflict_tools_resolve_verify_continue_and_cleanup_delivery() {
    let fixture = ConflictMergeFixture::text("merge-conflict-tools-complete").await;
    let (event_tx, _) = tokio::sync::broadcast::channel(16);
    let output = fixture
        .coordinator
        .merge_agent_with_verifier(
            &fixture.session_id,
            &fixture.agent_id,
            &fixture.expected_head,
            &TestRuntimeMarker,
            &event_tx,
            "call-conflict-tools-complete",
            &PassingMergeVerifier,
        )
        .await
        .unwrap();

    let conflicts = fixture
        .coordinator
        .list_active_conflicts(&fixture.session_id, &output.merge_id)
        .await
        .unwrap();
    assert_eq!(conflicts.len(), 1);
    assert!(!conflicts[0].resolved);
    let read = fixture
        .coordinator
        .read_active_conflict(&fixture.session_id, &output.merge_id, "src/shared.txt")
        .await
        .unwrap();
    assert_eq!(read.base.content.as_deref(), Some("base\n"));
    assert_eq!(read.ours.content.as_deref(), Some("planner branch\n"));
    assert_eq!(read.theirs.content.as_deref(), Some("executor\n"));

    let resolved = fixture
        .coordinator
        .resolve_active_conflict(
            &fixture.session_id,
            &output.merge_id,
            "src/shared.txt",
            super::merge::conflict_tools::ConflictResolutionChoice::Ours,
        )
        .await
        .unwrap();
    assert!(resolved.unresolved_paths.is_empty());
    let verification = fixture
        .coordinator
        .verify_active_conflict(&fixture.session_id, &output.merge_id)
        .await
        .unwrap();
    assert!(verification.success);
    assert_eq!(verification.attempt, 1);

    let completed = fixture
        .coordinator
        .continue_active_conflict(
            &fixture.session_id,
            &output.merge_id,
            "kept the planner branch content",
            None,
        )
        .await
        .unwrap();
    assert_eq!(completed.status, MergeStatus::Merged);
    let durable_run = fixture
        .store
        .read_task_run(&fixture.task_run_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(durable_run.phase, TaskRunPhase::Implementing);
    assert_eq!(durable_run.expected_head, completed.new_head.unwrap());
    assert!(!fixture.worktree.exists());
    let completion_claims = fixture
        .store
        .claim_merge_completion_continuation(&fixture.session_id)
        .await
        .unwrap();
    assert_eq!(completion_claims.len(), 1);
    let completion_claim = &completion_claims[0];
    assert_eq!(completion_claim.task_run_id, fixture.task_run_id);
    assert_eq!(completion_claim.agent_id, fixture.agent_id);
    assert_eq!(
        completion_claim.signal_id,
        format!("merge-completion:{}", output.merge_id)
    );
    assert_eq!(
        fixture
            .store
            .claim_merge_completion_continuation(&fixture.session_id)
            .await
            .unwrap(),
        Vec::new()
    );
    fixture.cleanup_conflict();
}

#[tokio::test]
async fn third_conflict_verification_failure_aborts_restores_and_blocks() {
    let fixture = ConflictMergeFixture::text("merge-conflict-tools-three-failures").await;
    let (event_tx, _) = tokio::sync::broadcast::channel(16);
    let output = fixture
        .coordinator
        .merge_agent_with_verifier(
            &fixture.session_id,
            &fixture.agent_id,
            &fixture.expected_head,
            &TestRuntimeMarker,
            &event_tx,
            "call-conflict-tools-three-failures",
            &PassingMergeVerifier,
        )
        .await
        .unwrap();

    for attempt in 1..=3 {
        let verification = fixture
            .coordinator
            .verify_active_conflict(&fixture.session_id, &output.merge_id)
            .await
            .unwrap();
        assert!(!verification.success);
        assert_eq!(verification.attempt, attempt);
        assert_eq!(verification.aborted, attempt == 3);
    }
    let run = fixture
        .store
        .read_task_run(&fixture.task_run_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(run.phase, TaskRunPhase::Blocked);
    assert_eq!(
        git_output(&fixture.repository, &["rev-parse", "HEAD"]),
        fixture.expected_head
    );
    assert!(
        git_output(
            &fixture.repository,
            &["status", "--porcelain=v1", "--untracked-files=all"]
        )
        .is_empty()
    );
    fixture.cleanup_conflict();
}

#[tokio::test]
async fn conflict_resolution_rejects_path_escape_and_explicit_abort_restores_prestate() {
    let fixture = ConflictMergeFixture::text("merge-conflict-tools-abort").await;
    let (event_tx, _) = tokio::sync::broadcast::channel(16);
    let output = fixture
        .coordinator
        .merge_agent_with_verifier(
            &fixture.session_id,
            &fixture.agent_id,
            &fixture.expected_head,
            &TestRuntimeMarker,
            &event_tx,
            "call-conflict-tools-abort",
            &PassingMergeVerifier,
        )
        .await
        .unwrap();
    let error = fixture
        .coordinator
        .resolve_active_conflict(
            &fixture.session_id,
            &output.merge_id,
            "../outside.txt",
            super::merge::conflict_tools::ConflictResolutionChoice::Ours,
        )
        .await
        .unwrap_err();
    assert!(error.to_string().contains("normalized"));

    let aborted = fixture
        .coordinator
        .abort_active_conflict(
            &fixture.session_id,
            &output.merge_id,
            "planner cannot resolve this conflict safely",
        )
        .await
        .unwrap();
    assert_eq!(aborted.status, MergeStatus::Aborted);
    assert_eq!(
        git_output(&fixture.repository, &["rev-parse", "HEAD"]),
        fixture.expected_head
    );
    assert!(
        git_output(
            &fixture.repository,
            &["status", "--porcelain=v1", "--untracked-files=all"]
        )
        .is_empty()
    );
    fixture.cleanup_conflict();
}

#[tokio::test]
async fn binary_conflict_rejects_patch_and_accepts_explicit_theirs() {
    let fixture = ConflictMergeFixture::binary("merge-conflict-tools-binary").await;
    let (event_tx, _) = tokio::sync::broadcast::channel(16);
    let output = fixture
        .coordinator
        .merge_agent_with_verifier(
            &fixture.session_id,
            &fixture.agent_id,
            &fixture.expected_head,
            &TestRuntimeMarker,
            &event_tx,
            "call-conflict-tools-binary",
            &PassingMergeVerifier,
        )
        .await
        .unwrap();
    let patch = "*** Begin Patch\n*** Update File: src/shared.bin\n@@\n-old\n+new\n*** End Patch";
    assert!(
        fixture
            .coordinator
            .resolve_active_conflict(
                &fixture.session_id,
                &output.merge_id,
                "src/shared.bin",
                super::merge::conflict_tools::ConflictResolutionChoice::Patch(patch.to_string()),
            )
            .await
            .unwrap_err()
            .to_string()
            .contains("require ours, theirs, or delete")
    );
    let resolved = fixture
        .coordinator
        .resolve_active_conflict(
            &fixture.session_id,
            &output.merge_id,
            "src/shared.bin",
            super::merge::conflict_tools::ConflictResolutionChoice::Theirs,
        )
        .await
        .unwrap();
    assert!(resolved.unresolved_paths.is_empty());
    assert_eq!(
        std::fs::read(fixture.repository.join("src/shared.bin")).unwrap(),
        b"executor\0blob"
    );
    fixture.cleanup_conflict();
}

#[tokio::test]
async fn binary_conflict_is_classified_from_stage_blob_content() {
    let fixture = ConflictMergeFixture::binary("merge-binary-conflict").await;
    let (event_tx, _) = tokio::sync::broadcast::channel(16);

    fixture
        .coordinator
        .merge_agent_with_verifier(
            &fixture.session_id,
            &fixture.agent_id,
            &fixture.expected_head,
            &TestRuntimeMarker,
            &event_tx,
            "call-binary-conflict",
            &PassingMergeVerifier,
        )
        .await
        .unwrap();

    let record = fixture
        .store
        .list_merge_records(&fixture.task_run_id)
        .await
        .unwrap()
        .pop()
        .unwrap();
    let conflict = &record
        .evidence
        .as_ref()
        .unwrap()
        .conflict_manifest
        .as_ref()
        .unwrap()
        .conflicts[0];
    assert_eq!(conflict.path, "src/shared.bin");
    assert_eq!(conflict.kind, ConflictKind::Binary);
    assert!(conflict.binary);
    assert!(
        conflict
            .stages
            .iter()
            .all(|stage| !stage.object_id.is_empty())
    );
    fixture.cleanup_conflict();
}

#[tokio::test]
async fn rename_delete_conflict_persists_source_destination_and_stage_evidence() {
    let fixture = ConflictMergeFixture::rename_delete("merge-rename-delete").await;
    let (event_tx, _) = tokio::sync::broadcast::channel(16);

    fixture
        .coordinator
        .merge_agent_with_verifier(
            &fixture.session_id,
            &fixture.agent_id,
            &fixture.expected_head,
            &TestRuntimeMarker,
            &event_tx,
            "call-rename-delete",
            &PassingMergeVerifier,
        )
        .await
        .unwrap();

    let record = fixture
        .store
        .list_merge_records(&fixture.task_run_id)
        .await
        .unwrap()
        .pop()
        .unwrap();
    let manifest = record
        .evidence
        .as_ref()
        .unwrap()
        .conflict_manifest
        .as_ref()
        .unwrap();
    let conflict = manifest
        .conflicts
        .iter()
        .find(|conflict| conflict.kind == ConflictKind::RenameDelete)
        .expect("rename/delete conflict must be classified");
    assert_eq!(conflict.rename_source.as_deref(), Some("src/old.txt"));
    assert_eq!(conflict.rename_destination.as_deref(), Some("src/new.txt"));
    assert!(!conflict.stages.is_empty());
    fixture.cleanup_conflict();
}

#[tokio::test]
async fn restart_recovery_accepts_exact_durable_conflict_state_instead_of_blocking_dirty_tree() {
    let fixture = ConflictMergeFixture::text("merge-conflict-recovery").await;
    let (event_tx, _) = tokio::sync::broadcast::channel(16);
    fixture
        .coordinator
        .merge_agent_with_verifier(
            &fixture.session_id,
            &fixture.agent_id,
            &fixture.expected_head,
            &TestRuntimeMarker,
            &event_tx,
            "call-conflict-recovery",
            &PassingMergeVerifier,
        )
        .await
        .unwrap();
    fixture.coordinator.suspend();
    let recovered_coordinator = TaskCoordinator::new(fixture.store.clone());

    let recovered = recovered_coordinator.recover_active_tasks().await.unwrap();

    assert_eq!(recovered.len(), 1);
    assert_eq!(recovered[0].id, fixture.task_run_id);
    assert_eq!(recovered[0].phase, TaskRunPhase::ResolvingConflict);
    assert_eq!(
        git_output(
            &fixture.repository,
            &["rev-parse", "--verify", "MERGE_HEAD"]
        ),
        fixture.source_head
    );
    recovered_coordinator.suspend();
    fixture.cleanup_conflict();
}

#[tokio::test]
async fn restart_recovery_aborts_exact_verifying_merge_and_blocks_with_evidence() {
    let fixture = IncrementalMergeFixture::new("merge-verifying-recovery").await;
    let delivered = fixture
        .deliver("agent_verifying_recovery", "src/lib.rs", "delivered\n")
        .await;
    let outcome = fixture
        .store
        .list_agent_outcomes(&fixture.run.id)
        .await
        .unwrap()
        .into_iter()
        .find(|outcome| outcome.agent_id == delivered.agent_id)
        .unwrap();
    let delivery = outcome.delivery.unwrap();
    let work_unit = fixture
        .store
        .read_work_unit(&delivered.work_unit_id)
        .await
        .unwrap()
        .unwrap();
    let run = fixture.run.clone();
    let pre_index_tree = git_output(&fixture.repository, &["write-tree"]);
    let scope = fixture
        .store
        .begin_task_merge(BeginTaskMerge {
            session_id: fixture.session_id.clone(),
            agent_id: delivered.agent_id,
            expected_head: run.expected_head.clone(),
            pre_index_tree,
            changed_files: delivery.changed_files,
        })
        .await
        .unwrap();
    git(
        &fixture.repository,
        &["merge", "--no-ff", "--no-commit", &work_unit.branch],
    );
    fixture
        .store
        .mark_task_merge_verifying(&scope.merge.id)
        .await
        .unwrap();
    fixture.coordinator.suspend();
    let recovered = TaskCoordinator::new(fixture.store.clone());

    let runs = recovered.recover_active_tasks().await.unwrap();

    assert!(runs.is_empty());
    let durable_run = fixture
        .store
        .read_task_run(&fixture.run.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(durable_run.phase, TaskRunPhase::Blocked);
    let merge = fixture
        .store
        .list_merge_records(&fixture.run.id)
        .await
        .unwrap()
        .pop()
        .unwrap();
    assert_eq!(
        merge.status,
        MergeStatus::Failed,
        "run status: {:?}",
        durable_run.status_message
    );
    assert!(
        merge
            .evidence
            .as_ref()
            .and_then(|evidence| evidence.compensation.as_deref())
            .is_some_and(|detail| detail.contains("restart recovery aborted"))
    );
    assert_eq!(
        git_output(&fixture.repository, &["rev-parse", "HEAD"]),
        run.expected_head
    );
    assert!(
        git_output(
            &fixture.repository,
            &["status", "--porcelain=v1", "--untracked-files=all"]
        )
        .is_empty()
    );
    let merge_head = Command::new("git")
        .arg("-C")
        .arg(&fixture.repository)
        .args(["rev-parse", "--verify", "MERGE_HEAD"])
        .output()
        .unwrap();
    assert!(!merge_head.status.success());
    recovered.suspend();
    fixture.cleanup();
}

#[tokio::test]
async fn restart_recovery_releases_unstarted_pending_merge_for_exact_delivery_retry() {
    let fixture = IncrementalMergeFixture::new("merge-pending-retry").await;
    let delivered = fixture
        .deliver("agent_pending_retry", "src/lib.rs", "delivered\n")
        .await;
    let outcome = fixture
        .store
        .list_agent_outcomes(&fixture.run.id)
        .await
        .unwrap()
        .into_iter()
        .find(|outcome| outcome.agent_id == delivered.agent_id)
        .unwrap();
    let delivery = outcome.delivery.unwrap();
    let pre_index_tree = git_output(&fixture.repository, &["write-tree"]);
    let pending = fixture
        .store
        .begin_task_merge(BeginTaskMerge {
            session_id: fixture.session_id.clone(),
            agent_id: delivered.agent_id.clone(),
            expected_head: fixture.run.expected_head.clone(),
            pre_index_tree,
            changed_files: delivery.changed_files,
        })
        .await
        .unwrap();
    fixture.coordinator.suspend();
    let recovered = Arc::new(TaskCoordinator::new(fixture.store.clone()));

    let runs = recovered.recover_active_tasks().await.unwrap();

    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0].phase, TaskRunPhase::Implementing);
    let failed = fixture
        .store
        .list_merge_records(&fixture.run.id)
        .await
        .unwrap()
        .into_iter()
        .find(|record| record.id == pending.merge.id)
        .unwrap();
    assert_eq!(failed.status, MergeStatus::Failed);
    let (event_tx, _) = tokio::sync::broadcast::channel(16);
    let merged = recovered
        .merge_agent_with_verifier(
            &fixture.session_id,
            &delivered.agent_id,
            &fixture.run.expected_head,
            &TestRuntimeMarker,
            &event_tx,
            "call-pending-retry",
            &PassingMergeVerifier,
        )
        .await
        .unwrap();

    assert_eq!(merged.status, MergeStatus::Merged);
    assert_ne!(
        merged.new_head.as_deref(),
        Some(fixture.run.expected_head.as_str())
    );
    assert_eq!(
        fixture
            .store
            .list_merge_records(&fixture.run.id)
            .await
            .unwrap()
            .len(),
        2
    );
    recovered.suspend();
    fixture.cleanup();
}

#[tokio::test]
async fn cleanup_tip_drift_preserves_accepted_merge_and_persists_retryable_failure_evidence() {
    let fixture = Arc::new(DeliveryFixture::new("merge-cleanup-failure", vec!["src/**"]).await);
    fixture.commit_file("src/lib.rs");
    let source_head = git_output(&fixture.worktree, &["rev-parse", "HEAD"]);
    fixture.submit(&source_head).await.unwrap();
    let run = fixture
        .store
        .transition_task_run(&fixture.task_run_id, TaskRunPhase::Implementing, None)
        .await
        .unwrap();
    let barrier = MergeCleanupTestBarrier::new();
    fixture
        .coordinator
        .set_merge_cleanup_barrier(barrier.clone());
    let task_fixture = fixture.clone();
    let merge = tokio::spawn(async move {
        let (event_tx, _) = tokio::sync::broadcast::channel(16);
        task_fixture
            .coordinator
            .merge_agent_with_verifier(
                &task_fixture.session_id,
                &task_fixture.subagent.id,
                &run.expected_head,
                &TestRuntimeMarker,
                &event_tx,
                "call-cleanup-failure",
                &PassingMergeVerifier,
            )
            .await
    });
    barrier.wait_until_entered().await;
    git(
        &fixture.repository,
        &[
            "update-ref",
            &format!("refs/heads/{}", fixture.branch),
            &fixture.base_commit,
            &source_head,
        ],
    );
    barrier.release().await;

    let output = merge.await.unwrap().unwrap();
    assert_eq!(output.status, MergeStatus::Merged);
    assert_eq!(output.cleanup.status, "failed");
    assert!(
        output
            .cleanup
            .detail
            .as_deref()
            .is_some_and(|detail| detail.contains("drifted"))
    );
    assert!(fixture.worktree.exists());
    let durable_run = fixture
        .store
        .read_task_run(&fixture.task_run_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(durable_run.phase, TaskRunPhase::Implementing);
    assert_eq!(durable_run.expected_head, output.new_head.unwrap());
    assert_eq!(fixture.work_unit().await.status, WorkUnitStatus::Merged);
    let record = fixture
        .store
        .list_merge_records(&fixture.task_run_id)
        .await
        .unwrap()
        .pop()
        .unwrap();
    assert_eq!(
        record
            .evidence
            .as_ref()
            .and_then(|evidence| evidence.cleanup.as_ref())
            .map(|cleanup| cleanup.status.as_str()),
        Some("failed")
    );
    Arc::try_unwrap(fixture).ok().unwrap().cleanup();
}

#[tokio::test]
async fn stale_expected_head_and_dirty_main_are_rejected_before_merge_side_effects() {
    for (name, dirty, expected_head) in [
        ("merge-stale-caller", false, "deadbeef"),
        ("merge-dirty-main", true, ""),
    ] {
        let fixture = DeliveryFixture::new(name, vec!["src/**"]).await;
        fixture.commit_file("src/lib.rs");
        let source_head = git_output(&fixture.worktree, &["rev-parse", "HEAD"]);
        fixture.submit(&source_head).await.unwrap();
        let run = fixture
            .store
            .transition_task_run(&fixture.task_run_id, TaskRunPhase::Implementing, None)
            .await
            .unwrap();
        if dirty {
            std::fs::write(fixture.repository.join("external.txt"), "dirty\n").unwrap();
        }
        let caller_head = if expected_head.is_empty() {
            run.expected_head.as_str()
        } else {
            expected_head
        };
        let (event_tx, _) = tokio::sync::broadcast::channel(16);

        assert!(
            fixture
                .coordinator
                .merge_agent_with_verifier(
                    &fixture.session_id,
                    &fixture.subagent.id,
                    caller_head,
                    &TestRuntimeMarker,
                    &event_tx,
                    "call-preflight-reject",
                    &PassingMergeVerifier,
                )
                .await
                .is_err()
        );
        assert_eq!(
            fixture
                .store
                .read_task_run(&fixture.task_run_id)
                .await
                .unwrap()
                .unwrap()
                .phase,
            TaskRunPhase::Implementing
        );
        assert!(
            fixture
                .store
                .list_merge_records(&fixture.task_run_id)
                .await
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            git_output(&fixture.repository, &["rev-parse", "HEAD"]),
            run.expected_head
        );
        if dirty {
            std::fs::remove_file(fixture.repository.join("external.txt")).unwrap();
        }
        fixture.cleanup();
    }
}

#[tokio::test]
async fn stale_delivery_branch_tip_is_rejected_before_merge_record_creation() {
    let fixture = DeliveryFixture::new("merge-stale-delivery", vec!["src/**"]).await;
    fixture.commit_file("src/lib.rs");
    let source_head = git_output(&fixture.worktree, &["rev-parse", "HEAD"]);
    fixture.submit(&source_head).await.unwrap();
    let run = fixture
        .store
        .transition_task_run(&fixture.task_run_id, TaskRunPhase::Implementing, None)
        .await
        .unwrap();
    git(
        &fixture.worktree,
        &["reset", "--hard", &fixture.base_commit],
    );
    let (event_tx, _) = tokio::sync::broadcast::channel(16);

    let error = fixture
        .coordinator
        .merge_agent_with_verifier(
            &fixture.session_id,
            &fixture.subagent.id,
            &run.expected_head,
            &TestRuntimeMarker,
            &event_tx,
            "call-stale-delivery",
            &PassingMergeVerifier,
        )
        .await
        .expect_err("delivery branch tip drift must be rejected");

    assert!(error.to_string().contains("outside the task coordinator"));
    assert!(
        fixture
            .store
            .list_merge_records(&fixture.task_run_id)
            .await
            .unwrap()
            .is_empty()
    );
    fixture.cleanup();
}

#[tokio::test]
async fn merge_and_design_update_serialize_through_the_same_branch_mutation_guard() {
    let fixture = Arc::new(IncrementalMergeFixture::new("merge-design-serialization").await);
    let delivery = fixture
        .deliver("agent_merge_design", "src/merge_design.rs", "merged\n")
        .await;
    let barrier = MergeCommitTestBarrier::new();
    fixture
        .coordinator
        .set_merge_after_commit_barrier(barrier.clone());
    let merge_fixture = fixture.clone();
    let expected_head = fixture.run.expected_head.clone();
    let merge = tokio::spawn(async move {
        let (event_tx, _) = tokio::sync::broadcast::channel(16);
        merge_fixture
            .coordinator
            .merge_agent_with_verifier(
                &merge_fixture.session_id,
                &delivery.agent_id,
                &expected_head,
                &TestRuntimeMarker,
                &event_tx,
                "call-merge-design",
                &PassingMergeVerifier,
            )
            .await
    });
    barrier.wait_until_committed().await;
    let design_fixture = fixture.clone();
    let design = tokio::spawn(async move {
        design_fixture
            .coordinator
            .update_design(
                &design_fixture.session_id,
                &design_fixture.repository,
                "*** Begin Patch\n*** Update File: design/spec.md\n@@\n-before\n+after\n*** End Patch",
            )
            .await
    });
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    assert!(
        !design.is_finished(),
        "design update must wait for merge branch guard"
    );
    barrier.release().await;

    let merge_output = merge.await.unwrap().unwrap();
    let design_output = design.await.unwrap().unwrap();
    assert_eq!(design_output.previous_head, merge_output.new_head.unwrap());
    assert_eq!(
        std::fs::read_to_string(fixture.repository.join("design/spec.md")).unwrap(),
        "after\n"
    );
    Arc::try_unwrap(fixture).ok().unwrap().cleanup();
}

#[tokio::test]
async fn merged_work_unit_is_not_downgraded_by_late_terminal_event() {
    let fixture = DeliveryFixture::new("merge-late-terminal", vec!["src/**"]).await;
    fixture.commit_file("src/lib.rs");
    let source_head = git_output(&fixture.worktree, &["rev-parse", "HEAD"]);
    fixture.submit(&source_head).await.unwrap();
    fixture
        .store
        .update_work_unit(
            &fixture.work_unit_id,
            WorkUnitStatus::Merged,
            Some(fixture.subagent.id.clone()),
        )
        .await
        .unwrap();

    drain_agent_state(
        &fixture,
        crate::TurnOutcomeKind::Failed,
        None,
        Some("late executor error"),
    )
    .await;

    assert_eq!(fixture.work_unit().await.status, WorkUnitStatus::Merged);
    assert_eq!(
        fixture.outcome().await.status,
        AgentOutcomeStatus::Completed
    );
    fixture.cleanup();
}

#[tokio::test]
async fn task_update_design_tool_has_typed_schema_branch_effect_and_planner_only_visibility() {
    let coordinator = Arc::new(TaskCoordinator::new(
        StudioStore::open_memory().await.unwrap(),
    ));
    let tool = coordinator.task_update_design_tool("studio-session");

    assert_eq!(tool.name(), "task_update_design");
    assert!(tool.description().contains("*** Begin Patch"));
    assert!(tool.description().contains("+# Design"));
    assert_eq!(tool.effect(), Some(ToolEffect::BranchControl));
    assert_eq!(
        tool.input_schema(),
        serde_json::json!({
            "type": "object",
            "properties": { "patch": { "type": "string" } },
            "required": ["patch"],
            "additionalProperties": false
        })
    );
}

#[tokio::test]
async fn clean_committed_delivery_persists_exact_receipt_and_completes_records() {
    let fixture = DeliveryFixture::new("delivery-success", vec!["src/**"]).await;
    std::fs::create_dir_all(fixture.worktree.join("src")).unwrap();
    std::fs::write(
        fixture.worktree.join("src/lib.rs"),
        "pub fn delivered() {}\n",
    )
    .unwrap();
    git(&fixture.worktree, &["add", "src/lib.rs"]);
    git(&fixture.worktree, &["commit", "-m", "deliver"]);
    let head = git_output(&fixture.worktree, &["rev-parse", "HEAD"]);

    let delivery = fixture
        .coordinator
        .submit_delivery(
            &fixture.subagent,
            &fixture.worktree,
            &head,
            "cargo test passed",
        )
        .await
        .unwrap();

    assert_eq!(
        delivery,
        AgentDelivery {
            worktree: AgentWorktreeDelivery {
                path: std::fs::canonicalize(&fixture.worktree)
                    .unwrap()
                    .to_string_lossy()
                    .to_string(),
                branch: fixture.branch.clone(),
            },
            base_commit: fixture.base_commit.clone(),
            head_commit: head,
            changed_files: vec!["src/lib.rs".to_string()],
            verification_summary: "cargo test passed".to_string(),
        }
    );
    let outcome = fixture.outcome().await;
    let work_unit = fixture.work_unit().await;
    assert_eq!(outcome.status, AgentOutcomeStatus::Completed);
    assert_eq!(outcome.delivery, Some(delivery));
    assert_eq!(work_unit.status, WorkUnitStatus::Delivered);
    fixture.cleanup();
}

#[tokio::test]
async fn repeated_successful_delivery_does_not_reopen_completed_records() {
    let fixture = DeliveryFixture::new("repeat-success", vec!["src/**"]).await;
    fixture.commit_file("src/lib.rs");
    let head = git_output(&fixture.worktree, &["rev-parse", "HEAD"]);
    let delivered = fixture.submit(&head).await.unwrap();

    let error = fixture
        .submit(&head)
        .await
        .expect_err("completed delivery cannot be submitted again");

    assert!(error.to_string().contains("already finalized"));
    let outcome = fixture.outcome().await;
    assert_eq!(outcome.status, AgentOutcomeStatus::Completed);
    assert_eq!(outcome.error, None);
    assert_eq!(outcome.delivery, Some(delivered));
    assert_eq!(fixture.work_unit().await.status, WorkUnitStatus::Delivered);
    fixture.cleanup();
}

#[tokio::test]
async fn invalid_retry_after_success_does_not_downgrade_or_record_error() {
    let fixture = DeliveryFixture::new("invalid-retry-after-success", vec!["src/**"]).await;
    fixture.commit_file("src/lib.rs");
    let head = git_output(&fixture.worktree, &["rev-parse", "HEAD"]);
    let delivered = fixture.submit(&head).await.unwrap();
    std::fs::write(fixture.worktree.join("src/lib.rs"), "dirty retry\n").unwrap();

    let error = fixture
        .submit(&head)
        .await
        .expect_err("completed delivery cannot enter waiting state");

    assert!(error.to_string().contains("already finalized"));
    let outcome = fixture.outcome().await;
    assert_eq!(outcome.status, AgentOutcomeStatus::Completed);
    assert_eq!(outcome.error, None);
    assert_eq!(outcome.delivery, Some(delivered));
    assert_eq!(fixture.work_unit().await.status, WorkUnitStatus::Delivered);
    fixture.cleanup();
}

#[tokio::test]
async fn completed_executor_event_without_delivery_waits_for_delivery() {
    let fixture = DeliveryFixture::new("terminal-without-delivery", vec!["src/**"]).await;
    drain_agent_state(
        &fixture,
        crate::TurnOutcomeKind::Completed,
        Some("implementation finished"),
        None,
    )
    .await;

    fixture.assert_waiting().await;
    drain_agent_state(
        &fixture,
        crate::TurnOutcomeKind::Completed,
        Some("duplicate completion"),
        None,
    )
    .await;
    let outcome = fixture.outcome().await;
    assert_eq!(outcome.summary.as_deref(), Some("implementation finished"));
    assert_eq!(
        outcome.error.as_deref(),
        Some("executor completed without a successful delivery")
    );
    fixture.cleanup();
}

#[tokio::test]
async fn delivery_recovery_is_claimed_once_and_second_terminal_fails() {
    let fixture = DeliveryFixture::new("delivery-recovery-once", vec!["src/**"]).await;
    drain_agent_state(
        &fixture,
        crate::TurnOutcomeKind::Completed,
        Some("implementation finished"),
        None,
    )
    .await;

    let claim = fixture
        .store
        .claim_delivery_recovery(&fixture.task_run_id, &fixture.subagent.id)
        .await
        .unwrap()
        .expect("first terminal must reserve the single recovery");
    assert_eq!(claim.recovery_count, 1);
    let replayed = fixture
        .store
        .claim_delivery_recovery(&fixture.task_run_id, &fixture.subagent.id)
        .await
        .unwrap()
        .expect("an undispatched recovery claim must survive process restart");
    assert_eq!(
        replayed, claim,
        "replaying a pending dispatch must not consume another recovery"
    );

    drain_agent_state(
        &fixture,
        crate::TurnOutcomeKind::Completed,
        Some("recovery returned without delivery"),
        None,
    )
    .await;

    let outcome = fixture.outcome().await;
    assert_eq!(outcome.status, AgentOutcomeStatus::Failed);
    assert_eq!(outcome.delivery_recovery_count, 1);
    assert_eq!(
        outcome.error.as_deref(),
        Some("executor delivery recovery finished without submit_delivery")
    );
    assert_eq!(fixture.work_unit().await.status, WorkUnitStatus::Failed);
    fixture.cleanup();
}

#[tokio::test]
async fn delivery_recovery_dispatch_is_deduplicated_by_durable_turn_metadata() {
    let fixture = DeliveryFixture::new("delivery-recovery-dispatch", vec!["src/**"]).await;
    drain_agent_state(
        &fixture,
        crate::TurnOutcomeKind::Completed,
        Some("implementation finished"),
        None,
    )
    .await;
    let claim = fixture
        .store
        .claim_delivery_recovery(&fixture.task_run_id, &fixture.subagent.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        fixture
            .store
            .delivery_recovery_dispatch(&claim)
            .await
            .unwrap(),
        None
    );

    fixture
        .store
        .database()
        .execute(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "INSERT INTO agent_turns
             (agent_id, turn_id, session_id, status, usage_json, metadata_json)
             VALUES (?, ?, ?, 'queued', '{}', ?)",
            [
                fixture.subagent.id.clone().into(),
                "turn-recovery".to_string().into(),
                fixture.session_id.clone().into(),
                serde_json::json!({
                    "deliveryRecoveryDispatchId": claim.dispatch_id(),
                })
                .to_string()
                .into(),
            ],
        ))
        .await
        .unwrap();
    assert_eq!(
        fixture
            .store
            .delivery_recovery_dispatch(&claim)
            .await
            .unwrap(),
        Some(DeliveryRecoveryDispatch::Pending)
    );

    for status in ["waiting_tool", "waiting_interaction", "waiting_agents"] {
        fixture
            .store
            .database()
            .execute(Statement::from_sql_and_values(
                DatabaseBackend::Sqlite,
                "UPDATE agent_turns
                 SET status = ?
                 WHERE agent_id = ? AND turn_id = 'turn-recovery'",
                [
                    status.to_string().into(),
                    fixture.subagent.id.clone().into(),
                ],
            ))
            .await
            .unwrap();
        assert_eq!(
            fixture
                .store
                .delivery_recovery_dispatch(&claim)
                .await
                .unwrap(),
            Some(DeliveryRecoveryDispatch::Pending)
        );
    }

    fixture
        .store
        .database()
        .execute(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "UPDATE agent_turns
             SET status = 'budget_limited', reason = 'recovery budget exhausted'
             WHERE agent_id = ? AND turn_id = 'turn-recovery'",
            [fixture.subagent.id.clone().into()],
        ))
        .await
        .unwrap();
    assert_eq!(
        fixture
            .store
            .delivery_recovery_dispatch(&claim)
            .await
            .unwrap(),
        Some(DeliveryRecoveryDispatch::Terminal {
            outcome: crate::TurnOutcomeKind::BudgetLimited,
            reason: Some("recovery budget exhausted".to_string()),
        })
    );

    fixture
        .store
        .database()
        .execute(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "UPDATE agent_turns
             SET status = 'completed', reason = 'recovery finished'
             WHERE agent_id = ? AND turn_id = 'turn-recovery'",
            [fixture.subagent.id.clone().into()],
        ))
        .await
        .unwrap();
    assert_eq!(
        fixture
            .store
            .delivery_recovery_dispatch(&claim)
            .await
            .unwrap(),
        Some(DeliveryRecoveryDispatch::Terminal {
            outcome: crate::TurnOutcomeKind::Completed,
            reason: Some("recovery finished".to_string()),
        })
    );
    fixture.cleanup();
}

#[tokio::test]
async fn unchanged_executor_worktree_fails_without_consuming_recovery() {
    let fixture = DeliveryFixture::new("delivery-no-changes", vec!["src/**"]).await;
    drain_agent_state(
        &fixture,
        crate::TurnOutcomeKind::Completed,
        Some("finished without implementation"),
        None,
    )
    .await;

    assert_eq!(
        fixture
            .coordinator
            .inspect_delivery_recovery_need(&fixture.task_run_id, &fixture.subagent.id)
            .await
            .unwrap(),
        DeliveryRecoveryNeed::NoDelivery
    );
    fixture
        .store
        .fail_executor_without_delivery(&fixture.task_run_id, &fixture.subagent.id)
        .await
        .unwrap();

    let outcome = fixture.outcome().await;
    assert_eq!(outcome.status, AgentOutcomeStatus::Failed);
    assert_eq!(outcome.delivery_recovery_count, 0);
    assert!(
        outcome
            .error
            .as_deref()
            .is_some_and(|error| error.contains("without delivery"))
    );
    assert_eq!(fixture.work_unit().await.status, WorkUnitStatus::Failed);
    fixture.cleanup();
}

#[tokio::test]
async fn stop_request_preserves_lease_and_accepts_existing_executor_delivery() {
    let fixture = DeliveryFixture::new("stop-request-delivery-open", vec!["src/**"]).await;
    fixture.commit_file("src/lib.rs");
    let head = git_output(&fixture.worktree, &["rev-parse", "HEAD"]);
    let run = fixture
        .store
        .read_task_run(&fixture.task_run_id)
        .await
        .unwrap()
        .unwrap();

    let requested = fixture
        .store
        .request_task_stop(&run.id, &run.expected_head, "user requested stop")
        .await
        .unwrap();

    assert!(requested.stop_requested);
    assert_eq!(requested.phase, run.phase);
    assert_eq!(
        requested.stop_requested_reason.as_deref(),
        Some("user requested stop")
    );
    assert!(
        fixture
            .store
            .read_branch_lease(&fixture.task_run_id)
            .await
            .unwrap()
            .is_some()
    );
    let delivery = fixture.submit(&head).await.unwrap();
    assert_eq!(fixture.outcome().await.delivery, Some(delivery));
    fixture.cleanup();
}

#[tokio::test]
async fn task_stop_preflight_preserves_executor_waiting_for_delivery() {
    let fixture = DeliveryFixture::new("stop-waiting-delivery", vec!["src/**"]).await;
    drain_agent_state(
        &fixture,
        crate::TurnOutcomeKind::Completed,
        Some("implementation committed"),
        None,
    )
    .await;
    let branch_guard = fixture.coordinator.lock_branch_mutation().await;

    let error = fixture
        .coordinator
        .preflight_task_stop_locked(&fixture.session_id, &branch_guard)
        .await
        .expect_err("recoverable executor delivery must defer task_stop");

    assert!(error.to_string().contains("task_stop deferred"));
    let TaskContinuationResolution::Active(snapshot) = fixture
        .coordinator
        .store
        .load_task_continuation_resolution(&fixture.task_run_id)
        .await
        .unwrap()
    else {
        panic!("waiting delivery task must remain active");
    };
    let prompt = snapshot.render_prompt().unwrap();
    assert!(prompt.contains("自动投递"));
    assert!(prompt.contains(fixture.subagent.id.as_str()));
    assert!(prompt.contains("不要再调用"));
    fixture.assert_waiting().await;
    drop(branch_guard);
    fixture.cleanup();
}

#[tokio::test]
async fn completed_turn_preserves_successful_delivery() {
    let fixture = DeliveryFixture::new("terminal-after-delivery", vec!["src/**"]).await;
    fixture.commit_file("src/lib.rs");
    let head = git_output(&fixture.worktree, &["rev-parse", "HEAD"]);
    let delivery = fixture.submit(&head).await.unwrap();

    drain_agent_state(
        &fixture,
        crate::TurnOutcomeKind::Completed,
        Some("turn completed"),
        None,
    )
    .await;

    assert_eq!(
        fixture.outcome().await.status,
        AgentOutcomeStatus::Completed
    );
    assert_eq!(fixture.outcome().await.delivery, Some(delivery));
    assert_eq!(fixture.work_unit().await.status, WorkUnitStatus::Delivered);
    fixture.cleanup();
}

#[tokio::test]
async fn successful_delivery_terminal_is_changed_once_then_idempotent() {
    let fixture = DeliveryFixture::new("delivered-terminal-one-shot", vec!["src/**"]).await;
    fixture.commit_file("src/lib.rs");
    let head = git_output(&fixture.worktree, &["rev-parse", "HEAD"]);
    fixture.submit(&head).await.unwrap();
    let change = StudioAgentTerminalChange {
        agent_id: fixture.subagent.id.clone(),
        role: "executor".to_string(),
        outcome: crate::TurnOutcomeKind::Completed,
        summary: Some("turn completed".to_string()),
        error: None,
    };

    let first = fixture
        .coordinator
        .record_terminal_agent_state(&fixture.session_id, &change)
        .await
        .unwrap();
    let duplicate = fixture
        .coordinator
        .record_terminal_agent_state(&fixture.session_id, &change)
        .await
        .unwrap();

    assert!(matches!(
        first,
        TerminalAgentStateRecording::Changed { task_run_id, .. }
            if task_run_id == fixture.task_run_id
    ));
    assert!(matches!(
        duplicate,
        TerminalAgentStateRecording::Projected(_)
    ));
    fixture.cleanup();
}

#[tokio::test]
async fn successful_delivery_preserves_terminal_observation_after_waiting() {
    let fixture = DeliveryFixture::new("delivery-after-waiting-terminal", vec!["src/**"]).await;
    let change = StudioAgentTerminalChange {
        agent_id: fixture.subagent.id.clone(),
        role: "executor".to_string(),
        outcome: crate::TurnOutcomeKind::Completed,
        summary: Some("turn completed".to_string()),
        error: None,
    };
    fixture
        .coordinator
        .record_terminal_agent_state(&fixture.session_id, &change)
        .await
        .unwrap();
    fixture.commit_file("src/lib.rs");
    let head = git_output(&fixture.worktree, &["rev-parse", "HEAD"]);
    fixture.submit(&head).await.unwrap();

    let delivered = fixture
        .coordinator
        .record_terminal_agent_state(&fixture.session_id, &change)
        .await
        .unwrap();
    let duplicate = fixture
        .coordinator
        .record_terminal_agent_state(&fixture.session_id, &change)
        .await
        .unwrap();

    assert!(matches!(
        delivered,
        TerminalAgentStateRecording::Projected(_)
    ));
    assert!(matches!(
        duplicate,
        TerminalAgentStateRecording::Projected(_)
    ));
    fixture.cleanup();
}

#[tokio::test]
async fn errored_and_interrupted_events_persist_terminal_task_states() {
    for (name, turn_outcome, outcome_status, work_unit_status) in [
        (
            "terminal-error",
            crate::TurnOutcomeKind::Failed,
            AgentOutcomeStatus::Failed,
            WorkUnitStatus::Failed,
        ),
        (
            "terminal-interrupted",
            crate::TurnOutcomeKind::Cancelled,
            AgentOutcomeStatus::Cancelled,
            WorkUnitStatus::Cancelled,
        ),
    ] {
        let fixture = DeliveryFixture::new(name, vec!["src/**"]).await;
        drain_agent_state(
            &fixture,
            turn_outcome,
            Some("terminal summary"),
            Some("boom"),
        )
        .await;

        let outcome = fixture.outcome().await;
        assert_eq!(outcome.status, outcome_status);
        assert_eq!(outcome.summary.as_deref(), Some("terminal summary"));
        assert_eq!(outcome.error.as_deref(), Some("boom"));
        assert_eq!(fixture.work_unit().await.status, work_unit_status);
        fixture.cleanup();
    }
}

#[tokio::test]
async fn duplicate_terminal_event_does_not_reopen_or_duplicate_outcome() {
    let fixture = DeliveryFixture::new("duplicate-terminal", vec!["src/**"]).await;
    drain_agent_state(
        &fixture,
        crate::TurnOutcomeKind::Failed,
        Some("first"),
        Some("first error"),
    )
    .await;
    drain_agent_state(
        &fixture,
        crate::TurnOutcomeKind::Completed,
        Some("second"),
        None,
    )
    .await;

    let outcomes = fixture
        .store
        .list_agent_outcomes(&fixture.task_run_id)
        .await
        .unwrap();
    assert_eq!(outcomes.len(), 1);
    assert_eq!(outcomes[0].status, AgentOutcomeStatus::Failed);
    assert_eq!(outcomes[0].summary.as_deref(), Some("first"));
    assert_eq!(outcomes[0].error.as_deref(), Some("first error"));
    assert_eq!(fixture.work_unit().await.status, WorkUnitStatus::Failed);
    fixture.cleanup();
}

#[tokio::test]
async fn terminal_recording_reports_only_committed_durable_changes() {
    let fixture = DeliveryFixture::new("terminal-change-signal", vec!["src/**"]).await;
    let change = StudioAgentTerminalChange {
        agent_id: fixture.subagent.id.clone(),
        role: "executor".to_string(),
        outcome: crate::TurnOutcomeKind::Failed,
        summary: Some("failed".to_string()),
        error: Some("boom".to_string()),
    };

    let changed = fixture
        .coordinator
        .record_terminal_agent_state(&fixture.session_id, &change)
        .await
        .unwrap();
    let duplicate = fixture
        .coordinator
        .record_terminal_agent_state(&fixture.session_id, &change)
        .await
        .unwrap();

    assert!(matches!(
        changed,
        TerminalAgentStateRecording::Changed {
            task_run_id,
            projection: _,
            ..
        } if task_run_id == fixture.task_run_id
    ));
    assert!(matches!(
        duplicate,
        TerminalAgentStateRecording::Projected(_)
    ));
    fixture.cleanup();
}

#[tokio::test]
async fn dirty_tracked_and_untracked_deliveries_wait_for_retry() {
    for (name, untracked) in [("dirty-tracked", false), ("dirty-untracked", true)] {
        let fixture = DeliveryFixture::new(name, vec!["src/**"]).await;
        std::fs::create_dir_all(fixture.worktree.join("src")).unwrap();
        std::fs::write(fixture.worktree.join("src/lib.rs"), "committed\n").unwrap();
        git(&fixture.worktree, &["add", "src/lib.rs"]);
        git(&fixture.worktree, &["commit", "-m", "deliver"]);
        let head = git_output(&fixture.worktree, &["rev-parse", "HEAD"]);
        let dirty_path = if untracked {
            "src/untracked.rs"
        } else {
            "src/lib.rs"
        };
        std::fs::write(fixture.worktree.join(dirty_path), "dirty\n").unwrap();

        let error = fixture.submit(&head).await.expect_err("dirty delivery");

        assert!(error.to_string().contains("clean working tree"));
        fixture.assert_waiting().await;
        fixture.cleanup();
    }
}

#[tokio::test]
async fn invalid_head_and_scope_deliveries_wait_for_retry() {
    let unchanged = DeliveryFixture::new("unchanged-head", vec!["src/**"]).await;
    let error = unchanged
        .submit(&unchanged.base_commit)
        .await
        .expect_err("unchanged HEAD");
    assert!(error.to_string().contains("must advance beyond base"));
    unchanged.assert_waiting().await;
    unchanged.cleanup();

    let mismatch = DeliveryFixture::new("head-mismatch", vec!["src/**"]).await;
    mismatch.commit_file("src/lib.rs");
    let error = mismatch
        .submit("0000000000000000000000000000000000000000")
        .await
        .expect_err("supplied HEAD mismatch");
    assert!(error.to_string().contains("does not match worktree HEAD"));
    mismatch.assert_waiting().await;
    mismatch.cleanup();

    let out_of_scope = DeliveryFixture::new("out-of-scope", vec!["src/**"]).await;
    out_of_scope.commit_file("design/notes.md");
    let head = git_output(&out_of_scope.worktree, &["rev-parse", "HEAD"]);
    let error = out_of_scope
        .submit(&head)
        .await
        .expect_err("out-of-scope delivery");
    assert!(error.to_string().contains("outside ownedPaths"));
    out_of_scope.assert_waiting().await;
    out_of_scope.cleanup();
}

#[tokio::test]
async fn delivery_head_must_descend_from_the_assigned_base() {
    let fixture = DeliveryFixture::new("diverged-head", vec!["src/**"]).await;
    std::fs::remove_file(fixture.worktree.join("README.md")).unwrap();
    std::fs::create_dir_all(fixture.worktree.join("src")).unwrap();
    std::fs::write(fixture.worktree.join("src/lib.rs"), "diverged\n").unwrap();
    git(&fixture.worktree, &["add", "-A"]);
    let tree = git_output(&fixture.worktree, &["write-tree"]);
    let head = git_output(
        &fixture.worktree,
        &["commit-tree", &tree, "-m", "diverged delivery"],
    );
    git(&fixture.worktree, &["reset", "--hard", &head]);

    let error = fixture
        .submit(&head)
        .await
        .expect_err("delivery must descend from base");

    assert!(error.to_string().contains("descend from base"));
    fixture.assert_waiting().await;
    fixture.cleanup();
}

#[tokio::test]
async fn delivery_uses_work_unit_base_after_task_expected_head_advances() {
    let fixture = DeliveryFixture::new("stable-work-unit-base", vec!["src/**"]).await;
    std::fs::write(fixture.repository.join("other.txt"), "other delivery\n").unwrap();
    git(&fixture.repository, &["add", "other.txt"]);
    git(
        &fixture.repository,
        &["commit", "-m", "merge other delivery"],
    );
    let advanced_head = git_output(&fixture.repository, &["rev-parse", "HEAD"]);
    assert!(
        fixture
            .store
            .compare_and_set_task_head(&fixture.task_run_id, &fixture.base_commit, &advanced_head,)
            .await
            .unwrap()
    );
    fixture.commit_file("src/lib.rs");
    let executor_head = git_output(&fixture.worktree, &["rev-parse", "HEAD"]);

    let delivery = fixture.submit(&executor_head).await.unwrap();

    assert_eq!(delivery.base_commit, fixture.base_commit);
    assert_eq!(delivery.changed_files, vec!["src/lib.rs".to_string()]);
    fixture.cleanup();
}

#[tokio::test]
async fn delivery_rejects_main_workspace_and_other_worktree_from_same_repository() {
    let main_workspace = DeliveryFixture::new("reject-main-workspace", vec!["src/**"]).await;
    let main_head = git_output(&main_workspace.repository, &["rev-parse", "HEAD"]);
    let error = main_workspace
        .coordinator
        .submit_delivery(
            &main_workspace.subagent,
            &main_workspace.repository,
            &main_head,
            "cargo test passed",
        )
        .await
        .expect_err("planner workspace is not the assigned executor worktree");
    assert!(error.to_string().contains("assigned worktree"));
    main_workspace.assert_waiting().await;
    main_workspace.cleanup();

    let other_worktree = DeliveryFixture::new("reject-other-worktree", vec!["src/**"]).await;
    let other_path = other_worktree.repository.with_extension("other-worktree");
    let other_path_text = other_path.to_string_lossy().to_string();
    git(
        &other_worktree.repository,
        &[
            "worktree",
            "add",
            "-b",
            "unassigned-worktree",
            &other_path_text,
            &other_worktree.base_commit,
        ],
    );
    std::fs::create_dir_all(other_path.join("src")).unwrap();
    std::fs::write(other_path.join("src/lib.rs"), "unassigned\n").unwrap();
    git(&other_path, &["add", "src/lib.rs"]);
    git(&other_path, &["commit", "-m", "unassigned delivery"]);
    let other_head = git_output(&other_path, &["rev-parse", "HEAD"]);
    let error = other_worktree
        .coordinator
        .submit_delivery(
            &other_worktree.subagent,
            &other_path,
            &other_head,
            "cargo test passed",
        )
        .await
        .expect_err("other worktree is not assigned");
    assert!(error.to_string().contains("assigned worktree"));
    other_worktree.assert_waiting().await;
    let _ = Command::new("git")
        .arg("-C")
        .arg(&other_worktree.repository)
        .args(["worktree", "remove", "--force", &other_path_text])
        .output();
    remove_repository(other_path);
    other_worktree.cleanup();

    let subdirectory = DeliveryFixture::new("reject-worktree-subdirectory", vec!["src/**"]).await;
    subdirectory.commit_file("src/lib.rs");
    let head = git_output(&subdirectory.worktree, &["rev-parse", "HEAD"]);
    let error = subdirectory
        .coordinator
        .submit_delivery(
            &subdirectory.subagent,
            subdirectory.worktree.join("src"),
            &head,
            "cargo test passed",
        )
        .await
        .expect_err("caller path must be the assigned worktree root");
    assert!(error.to_string().contains("assigned worktree"));
    subdirectory.assert_waiting().await;
    subdirectory.cleanup();
}

#[tokio::test]
async fn rename_from_outside_owned_paths_is_rejected() {
    let fixture = DeliveryFixture::new("rename-outside-owned-paths", vec!["src/**"]).await;
    std::fs::create_dir_all(fixture.worktree.join("src")).unwrap();
    git(&fixture.worktree, &["mv", "README.md", "src/README.md"]);
    git(
        &fixture.worktree,
        &["commit", "-m", "rename into owned path"],
    );
    let head = git_output(&fixture.worktree, &["rev-parse", "HEAD"]);

    let error = fixture
        .submit(&head)
        .await
        .expect_err("rename source is outside owned paths");

    assert!(error.to_string().contains("README.md"));
    fixture.assert_waiting().await;
    fixture.cleanup();
}

#[tokio::test]
async fn invalid_owned_path_wrong_role_and_attempt_four_wait_for_retry() {
    for (name, owned_path) in [
        ("traversal-owned-path", "../escape/**"),
        ("absolute-owned-path", "C:/escape/**"),
    ] {
        let invalid_path = DeliveryFixture::new(name, vec![owned_path]).await;
        invalid_path.commit_file("src/lib.rs");
        let head = git_output(&invalid_path.worktree, &["rev-parse", "HEAD"]);
        let error = invalid_path
            .submit(&head)
            .await
            .expect_err("invalid owned path");
        assert!(error.to_string().contains("invalid owned path"));
        invalid_path.assert_waiting().await;
        invalid_path.cleanup();
    }

    let wrong_role = DeliveryFixture::new("wrong-role", vec!["src/**"]).await;
    wrong_role.commit_file("src/lib.rs");
    let head = git_output(&wrong_role.worktree, &["rev-parse", "HEAD"]);
    let mut explorer = wrong_role.subagent.clone();
    explorer.role = "explorer".to_string();
    let error = wrong_role
        .coordinator
        .submit_delivery(&explorer, &wrong_role.worktree, &head, "cargo test passed")
        .await
        .expect_err("wrong role");
    assert!(error.to_string().contains("executor"));
    wrong_role.assert_waiting().await;
    wrong_role.cleanup();

    let attempt_four = DeliveryFixture::new_with_attempt("attempt-four", vec!["src/**"], 4).await;
    attempt_four.commit_file("src/lib.rs");
    let head = git_output(&attempt_four.worktree, &["rev-parse", "HEAD"]);
    let error = attempt_four.submit(&head).await.expect_err("attempt four");
    assert!(error.to_string().contains("attempt must be within 1..=3"));
    attempt_four.assert_waiting().await;
    attempt_four.cleanup();
}

#[tokio::test]
async fn wrong_owner_missing_work_unit_and_empty_summary_are_actionable() {
    let wrong_owner = DeliveryFixture::new("wrong-owner", vec!["src/**"]).await;
    wrong_owner.commit_file("src/lib.rs");
    let head = git_output(&wrong_owner.worktree, &["rev-parse", "HEAD"]);
    let mut other_owner = wrong_owner.subagent.clone();
    other_owner.parent_id = Some("other-planner".to_string());
    let error = wrong_owner
        .coordinator
        .submit_delivery(
            &other_owner,
            &wrong_owner.worktree,
            &head,
            "cargo test passed",
        )
        .await
        .expect_err("wrong owner");
    assert!(error.to_string().contains("does not own this task outcome"));
    wrong_owner.assert_waiting().await;
    wrong_owner.cleanup();

    let missing = DeliveryFixture::new_without_work_unit("missing-work-unit", vec!["src/**"]).await;
    missing.commit_file("src/lib.rs");
    let head = git_output(&missing.worktree, &["rev-parse", "HEAD"]);
    let error = missing.submit(&head).await.expect_err("missing work unit");
    assert!(error.to_string().contains("no work unit"));
    assert_eq!(
        missing.outcome().await.status,
        AgentOutcomeStatus::WaitingForDelivery
    );
    missing.cleanup();

    let empty_summary = DeliveryFixture::new("empty-summary", vec!["src/**"]).await;
    empty_summary.commit_file("src/lib.rs");
    let head = git_output(&empty_summary.worktree, &["rev-parse", "HEAD"]);
    let error = empty_summary
        .coordinator
        .submit_delivery(
            &empty_summary.subagent,
            &empty_summary.worktree,
            &head,
            "  ",
        )
        .await
        .expect_err("empty verification summary");
    assert!(error.to_string().contains("verificationSummary"));
    empty_summary.assert_waiting().await;
    empty_summary.cleanup();
}

#[tokio::test]
async fn exact_and_backslash_directory_owned_paths_are_normalized() {
    let fixture = DeliveryFixture::new("owned-path-shapes", vec!["README.md", r"src\**"]).await;
    std::fs::write(fixture.worktree.join("README.md"), "updated\n").unwrap();
    std::fs::create_dir_all(fixture.worktree.join("src")).unwrap();
    std::fs::write(fixture.worktree.join("src/lib.rs"), "delivered\n").unwrap();
    git(&fixture.worktree, &["add", "README.md", "src/lib.rs"]);
    git(&fixture.worktree, &["commit", "-m", "deliver"]);
    let head = git_output(&fixture.worktree, &["rev-parse", "HEAD"]);

    let delivery = fixture.submit(&head).await.unwrap();

    assert_eq!(
        delivery.changed_files,
        vec!["README.md".to_string(), "src/lib.rs".to_string()]
    );
    fixture.cleanup();
}

#[tokio::test]
async fn delivery_owned_path_case_matching_follows_platform_semantics() {
    let fixture = DeliveryFixture::new("owned-path-case", vec!["Src/**"]).await;
    fixture.commit_file("src/lib.rs");
    let head = git_output(&fixture.worktree, &["rev-parse", "HEAD"]);

    let result = fixture.submit(&head).await;

    if cfg!(windows) {
        let delivery = result.expect("Windows ownedPaths matching is case-insensitive");
        assert_eq!(delivery.changed_files, vec!["src/lib.rs".to_string()]);
    } else {
        let error = result.expect_err("Unix ownedPaths matching is case-sensitive");
        assert!(error.to_string().contains("outside ownedPaths"));
    }
    fixture.cleanup();
}

#[tokio::test]
async fn submit_delivery_tool_has_typed_schema_branch_effect_and_role_visibility() {
    let coordinator = Arc::new(TaskCoordinator::new(
        StudioStore::open_memory().await.unwrap(),
    ));
    let tool = coordinator.submit_delivery_tool("test-session".to_string(), None);

    assert_eq!(tool.name(), "submit_delivery");
    assert_eq!(tool.effect(), Some(ToolEffect::BranchControl));
    assert_eq!(
        tool.input_schema(),
        serde_json::json!({
            "type": "object",
            "properties": {
                "headCommit": { "type": "string" },
                "verificationSummary": { "type": "string" }
            },
            "required": ["headCommit", "verificationSummary"],
            "additionalProperties": false
        })
    );
}

#[tokio::test]
async fn child_dispatch_resolves_delivery_without_task_session_in_tool_input() {
    let fixture = DeliveryFixture::new("delivery-tool-handler", vec!["src/**"]).await;
    fixture.commit_file("src/lib.rs");
    let head = git_output(&fixture.worktree, &["rev-parse", "HEAD"]);
    let tool = fixture
        .coordinator
        .submit_delivery_tool(fixture.session_id.clone(), None);
    let (event_tx, _) = tokio::sync::broadcast::channel(16);
    let kernel = AgentKernel::builder(
        TurnEngineBuilder::from_provider_info(pl_model::ProviderInfo::deepseek(None)).unwrap(),
    )
    .with_profile(CoreAgentProfile::host_provided(fixture.worktree.clone()))
    .with_registered_tool(tool)
    .with_subagent_context(fixture.subagent.clone())
    .build()
    .await;

    let output = kernel
        .execute_tool(AgentKernelToolRequest::new(
            "submit_delivery",
            serde_json::json!({
                "headCommit": head,
                "verificationSummary": "cargo test passed"
            }),
            "child-turn-not-task-session",
            "call-submit",
            event_tx,
        ))
        .await
        .unwrap();
    let delivery: AgentDelivery = serde_json::from_str(&output.description).unwrap();

    assert_eq!(delivery, fixture.outcome().await.delivery.unwrap());
    fixture.cleanup();
}

#[tokio::test]
async fn submit_delivery_commits_after_executor_terminal_was_already_observed() {
    let fixture = DeliveryFixture::new("delivery-after-terminal-observed", vec!["src/**"]).await;
    drain_agent_state(
        &fixture,
        crate::TurnOutcomeKind::Completed,
        Some("implementation finished"),
        None,
    )
    .await;
    fixture.assert_waiting().await;
    fixture.commit_file("src/lib.rs");
    let head = git_output(&fixture.worktree, &["rev-parse", "HEAD"]);
    let tool = fixture
        .coordinator
        .submit_delivery_tool(fixture.session_id.clone(), None);
    let (event_tx, _) = tokio::sync::broadcast::channel(16);
    let kernel = AgentKernel::builder(
        TurnEngineBuilder::from_provider_info(pl_model::ProviderInfo::deepseek(None)).unwrap(),
    )
    .with_profile(CoreAgentProfile::host_provided(fixture.worktree.clone()))
    .with_registered_tool(tool)
    .with_subagent_context(fixture.subagent.clone())
    .build()
    .await;

    kernel
        .execute_tool(AgentKernelToolRequest::new(
            "submit_delivery",
            serde_json::json!({
                "headCommit": head,
                "verificationSummary": "cargo test passed"
            }),
            "child-turn-after-terminal",
            "call-submit-after-terminal",
            event_tx,
        ))
        .await
        .unwrap();

    assert_eq!(
        fixture.outcome().await.status,
        AgentOutcomeStatus::Completed
    );
    assert_eq!(fixture.work_unit().await.status, WorkUnitStatus::Delivered);
    let terminal_observed = fixture
        .store
        .database()
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "SELECT terminal_observed
             FROM agent_outcomes
             WHERE id = ?",
            [fixture.outcome_id.clone().into()],
        ))
        .await
        .unwrap()
        .unwrap()
        .try_get::<i64>("", "terminal_observed")
        .unwrap();
    assert_eq!(
        terminal_observed, 1,
        "delivery must not erase an already-observed terminal event"
    );
    fixture.cleanup();
}

struct DeliveryFixture {
    coordinator: Arc<TaskCoordinator>,
    store: StudioStore,
    session_id: String,
    task_run_id: String,
    work_unit_id: String,
    outcome_id: String,
    repository: PathBuf,
    worktree: PathBuf,
    branch: String,
    base_commit: String,
    subagent: SubagentContext,
}

struct IncrementalDelivery {
    agent_id: String,
    work_unit_id: String,
}

struct IncrementalMergeFixture {
    repository: PathBuf,
    store: StudioStore,
    coordinator: Arc<TaskCoordinator>,
    session_id: String,
    run: TaskRunRecord,
}

impl IncrementalMergeFixture {
    async fn new(name: &str) -> Self {
        let repository = init_repository(name);
        std::fs::create_dir_all(repository.join("design")).unwrap();
        std::fs::write(repository.join("design/spec.md"), "before\n").unwrap();
        std::fs::write(repository.join(".gitignore"), ".pure/\n").unwrap();
        git(&repository, &["add", ".gitignore", "design/spec.md"]);
        git(&repository, &["commit", "-m", "ignore task worktrees"]);
        let store = task_store(&repository).await;
        let session = task_session(&store, &repository).await;
        let coordinator = Arc::new(TaskCoordinator::new(store.clone()));
        let run = coordinator
            .start_confirmed_task(&session.id, "plan", &repository)
            .await
            .unwrap();
        let run = store
            .transition_task_run(&run.id, TaskRunPhase::Implementing, None)
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

    async fn deliver(
        &self,
        agent_id: &str,
        relative_path: &str,
        content: &str,
    ) -> IncrementalDelivery {
        let worktree = crate::agent::worktree::git_compatible_path(
            self.repository
                .join(".pure/worktrees")
                .join(&self.run.id)
                .join(agent_id),
        );
        let branch = format!("pure-task-{}-{agent_id}", self.run.id);
        let worktree_text = worktree.to_string_lossy().to_string();
        git(
            &self.repository,
            &[
                "worktree",
                "add",
                "-b",
                &branch,
                &worktree_text,
                &self.run.expected_head,
            ],
        );
        let work_unit = self
            .store
            .create_work_unit(CreateWorkUnit {
                task_run_id: self.run.id.clone(),
                title: format!("Implement {agent_id}"),
                owned_paths: vec![relative_path.to_string()],
                base_commit: self.run.expected_head.clone(),
                worktree_path: std::fs::canonicalize(&worktree)
                    .unwrap()
                    .to_string_lossy()
                    .to_string(),
                branch: branch.clone(),
                attempt: 1,
            })
            .await
            .unwrap();
        self.store
            .update_work_unit(
                &work_unit.id,
                WorkUnitStatus::Running,
                Some(agent_id.to_string()),
            )
            .await
            .unwrap();
        self.store
            .create_agent_outcome(CreateAgentOutcome {
                task_run_id: self.run.id.clone(),
                work_unit_id: Some(work_unit.id.clone()),
                agent_id: agent_id.to_string(),
                owner_path: "/root".to_string(),
                initiated_by: "planner".to_string(),
                requested_by_call_id: format!("call-{agent_id}"),
                role: "executor".to_string(),
                status: AgentOutcomeStatus::Running,
                attempt: 1,
            })
            .await
            .unwrap();
        let path = worktree.join(relative_path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, content).unwrap();
        git(&worktree, &["add", relative_path]);
        git(&worktree, &["commit", "-m", &format!("deliver {agent_id}")]);
        let head = git_output(&worktree, &["rev-parse", "HEAD"]);
        let subagent = SubagentContext {
            id: agent_id.to_string(),
            parent_id: Some("/root".to_string()),
            agent_path: Some(format!("/root/{agent_id}")),
            role: "executor".to_string(),
            task: format!("Implement {agent_id}"),
            depth: 1,
        };
        self.coordinator
            .submit_delivery(&subagent, &worktree, &head, "test passed")
            .await
            .unwrap();
        IncrementalDelivery {
            agent_id: agent_id.to_string(),
            work_unit_id: work_unit.id,
        }
    }

    fn cleanup(self) {
        drop(self.coordinator);
        remove_repository(self.repository);
    }
}

struct ReviewFixture {
    repository: PathBuf,
    store: StudioStore,
    coordinator: Arc<TaskCoordinator>,
    session_id: String,
    run_id: String,
}

impl ReviewFixture {
    async fn new(name: &str) -> Self {
        let repository = init_repository(name);
        std::fs::create_dir_all(repository.join("design")).unwrap();
        std::fs::write(repository.join("design/guide.md"), "# Review design\n").unwrap();
        git(&repository, &["add", "design/guide.md"]);
        git(&repository, &["commit", "-m", "add review design"]);
        let store = task_store(&repository).await;
        let session = task_session(&store, &repository).await;
        let coordinator = Arc::new(TaskCoordinator::new(store.clone()));
        let run = coordinator
            .start_confirmed_task(&session.id, "review this implementation", &repository)
            .await
            .unwrap();
        let run = store
            .transition_task_run(&run.id, TaskRunPhase::Implementing, None)
            .await
            .unwrap();
        Self {
            repository,
            store,
            coordinator,
            session_id: session.id,
            run_id: run.id,
        }
    }

    fn cleanup(self) {
        self.coordinator.suspend();
        remove_repository(self.repository);
    }
}

struct ConflictMergeFixture {
    repository: PathBuf,
    worktree: PathBuf,
    store: StudioStore,
    coordinator: Arc<TaskCoordinator>,
    session_id: String,
    task_run_id: String,
    agent_id: String,
    expected_head: String,
    source_head: String,
}

#[derive(Clone, Copy)]
enum ConflictFixtureKind {
    Text,
    Binary,
    RenameDelete,
}

impl ConflictMergeFixture {
    async fn text(name: &str) -> Self {
        Self::new(name, ConflictFixtureKind::Text).await
    }

    async fn binary(name: &str) -> Self {
        Self::new(name, ConflictFixtureKind::Binary).await
    }

    async fn rename_delete(name: &str) -> Self {
        Self::new(name, ConflictFixtureKind::RenameDelete).await
    }

    async fn new(name: &str, kind: ConflictFixtureKind) -> Self {
        let repository = init_repository(name);
        std::fs::create_dir_all(repository.join("src")).unwrap();
        let base_path = match kind {
            ConflictFixtureKind::Text => "src/shared.txt",
            ConflictFixtureKind::Binary => "src/shared.bin",
            ConflictFixtureKind::RenameDelete => "src/old.txt",
        };
        let base_content: &[u8] = match kind {
            ConflictFixtureKind::Text | ConflictFixtureKind::RenameDelete => b"base\n",
            ConflictFixtureKind::Binary => b"base\0blob",
        };
        std::fs::write(repository.join(base_path), base_content).unwrap();
        std::fs::write(repository.join(".gitignore"), ".pure/\n").unwrap();
        git(&repository, &["add", base_path, ".gitignore"]);
        git(&repository, &["commit", "-m", "add shared base"]);
        let store = task_store(&repository).await;
        let session = task_session(&store, &repository).await;
        let coordinator = Arc::new(TaskCoordinator::new(store.clone()));
        let run = coordinator
            .start_confirmed_task(&session.id, "plan", &repository)
            .await
            .unwrap();
        let run = store
            .transition_task_run(&run.id, TaskRunPhase::Implementing, None)
            .await
            .unwrap();
        let agent_id = "agent-conflict".to_string();
        let worktree = crate::agent::worktree::git_compatible_path(
            repository
                .join(".pure/worktrees")
                .join(&run.id)
                .join(&agent_id),
        );
        let branch = format!("pure-task-{}-{agent_id}", run.id);
        let worktree_text = worktree.to_string_lossy().to_string();
        git(
            &repository,
            &[
                "worktree",
                "add",
                "-b",
                &branch,
                &worktree_text,
                &run.expected_head,
            ],
        );
        let work_unit = store
            .create_work_unit(CreateWorkUnit {
                task_run_id: run.id.clone(),
                title: "Implement conflicting edit".to_string(),
                owned_paths: match kind {
                    ConflictFixtureKind::Text => vec!["src/shared.txt".to_string()],
                    ConflictFixtureKind::Binary => vec!["src/shared.bin".to_string()],
                    ConflictFixtureKind::RenameDelete => {
                        vec!["src/old.txt".to_string(), "src/new.txt".to_string()]
                    }
                },
                base_commit: run.expected_head.clone(),
                worktree_path: std::fs::canonicalize(&worktree)
                    .unwrap()
                    .to_string_lossy()
                    .to_string(),
                branch: branch.clone(),
                attempt: 1,
            })
            .await
            .unwrap();
        store
            .update_work_unit(
                &work_unit.id,
                WorkUnitStatus::Running,
                Some(agent_id.clone()),
            )
            .await
            .unwrap();
        store
            .create_agent_outcome(CreateAgentOutcome {
                task_run_id: run.id.clone(),
                work_unit_id: Some(work_unit.id),
                agent_id: agent_id.clone(),
                owner_path: "/root".to_string(),
                initiated_by: "planner".to_string(),
                requested_by_call_id: "call-conflict".to_string(),
                role: "executor".to_string(),
                status: AgentOutcomeStatus::Running,
                attempt: 1,
            })
            .await
            .unwrap();
        match kind {
            ConflictFixtureKind::Text => {
                std::fs::write(worktree.join("src/shared.txt"), "executor\n").unwrap();
                git(&worktree, &["add", "src/shared.txt"]);
            }
            ConflictFixtureKind::Binary => {
                std::fs::write(worktree.join("src/shared.bin"), b"executor\0blob").unwrap();
                git(&worktree, &["add", "src/shared.bin"]);
            }
            ConflictFixtureKind::RenameDelete => {
                git(&worktree, &["mv", "src/old.txt", "src/new.txt"]);
            }
        }
        git(&worktree, &["commit", "-m", "executor conflicting edit"]);
        let source_head = git_output(&worktree, &["rev-parse", "HEAD"]);
        coordinator
            .submit_delivery(
                &SubagentContext {
                    id: agent_id.clone(),
                    parent_id: Some("/root".to_string()),
                    agent_path: Some(format!("/root/{agent_id}")),
                    role: "executor".to_string(),
                    task: "Implement conflicting edit".to_string(),
                    depth: 1,
                },
                &worktree,
                &source_head,
                "test passed",
            )
            .await
            .unwrap();
        match kind {
            ConflictFixtureKind::Text => {
                std::fs::write(repository.join("src/shared.txt"), "planner branch\n").unwrap();
                git(&repository, &["add", "src/shared.txt"]);
            }
            ConflictFixtureKind::Binary => {
                std::fs::write(repository.join("src/shared.bin"), b"planner\0blob").unwrap();
                git(&repository, &["add", "src/shared.bin"]);
            }
            ConflictFixtureKind::RenameDelete => {
                git(&repository, &["rm", "src/old.txt"]);
            }
        }
        git(&repository, &["commit", "-m", "main conflicting edit"]);
        let expected_head = git_output(&repository, &["rev-parse", "HEAD"]);
        assert!(
            store
                .compare_and_set_task_head(&run.id, &run.expected_head, &expected_head)
                .await
                .unwrap()
        );
        Self {
            repository,
            worktree,
            store,
            coordinator,
            session_id: session.id,
            task_run_id: run.id,
            agent_id,
            expected_head,
            source_head,
        }
    }

    fn cleanup_conflict(self) {
        let _ = Command::new("git")
            .arg("-C")
            .arg(&self.repository)
            .args(["merge", "--abort"])
            .output();
        self.coordinator.suspend();
        remove_repository(self.repository);
    }
}

impl DeliveryFixture {
    async fn new(name: &str, owned_paths: Vec<&str>) -> Self {
        Self::new_configured(name, owned_paths, 1, true).await
    }

    async fn new_with_attempt(name: &str, owned_paths: Vec<&str>, attempt: u32) -> Self {
        Self::new_configured(name, owned_paths, attempt, true).await
    }

    async fn new_without_work_unit(name: &str, owned_paths: Vec<&str>) -> Self {
        Self::new_configured(name, owned_paths, 1, false).await
    }

    async fn new_configured(
        name: &str,
        owned_paths: Vec<&str>,
        attempt: u32,
        link_work_unit: bool,
    ) -> Self {
        let repository = init_repository(name);
        let worktree = repository.with_extension("executor-worktree");
        let store = task_store(&repository).await;
        let session = task_session(&store, &repository).await;
        let coordinator = Arc::new(TaskCoordinator::new(store.clone()));
        let run = coordinator
            .start_confirmed_task(&session.id, "plan", &repository)
            .await
            .unwrap();
        let branch = format!("pure-task-{}-agent-1", run.id);
        let worktree_text = worktree.to_string_lossy().to_string();
        git(
            &repository,
            &[
                "worktree",
                "add",
                "-b",
                &branch,
                &worktree_text,
                &run.base_commit,
            ],
        );
        let work_unit = store
            .create_work_unit(CreateWorkUnit {
                task_run_id: run.id.clone(),
                title: "Implement delivery".to_string(),
                owned_paths: owned_paths.into_iter().map(str::to_string).collect(),
                base_commit: run.expected_head.clone(),
                worktree_path: std::fs::canonicalize(&worktree)
                    .unwrap()
                    .to_string_lossy()
                    .to_string(),
                branch: branch.clone(),
                attempt,
            })
            .await
            .unwrap();
        let work_unit = store
            .update_work_unit(
                &work_unit.id,
                WorkUnitStatus::Running,
                Some("agent-1".to_string()),
            )
            .await
            .unwrap();
        let subagent = SubagentContext {
            id: "agent-1".to_string(),
            parent_id: Some("/root".to_string()),
            agent_path: Some("root/agent-1".to_string()),
            role: "executor".to_string(),
            task: "Implement delivery".to_string(),
            depth: 1,
        };
        let task_run_id = run.id.clone();
        let outcome = store
            .create_agent_outcome(CreateAgentOutcome {
                task_run_id: run.id,
                work_unit_id: link_work_unit.then(|| work_unit.id.clone()),
                agent_id: subagent.id.clone(),
                owner_path: "/root".to_string(),
                initiated_by: "planner".to_string(),
                requested_by_call_id: "call-spawn".to_string(),
                role: subagent.role.clone(),
                status: AgentOutcomeStatus::Running,
                attempt,
            })
            .await
            .unwrap();
        Self {
            coordinator,
            store,
            session_id: session.id,
            task_run_id,
            work_unit_id: work_unit.id,
            outcome_id: outcome.id,
            repository,
            worktree,
            branch,
            base_commit: run.base_commit,
            subagent,
        }
    }

    fn commit_file(&self, relative_path: &str) {
        let path = self.worktree.join(relative_path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, "delivered\n").unwrap();
        git(&self.worktree, &["add", relative_path]);
        git(&self.worktree, &["commit", "-m", "deliver"]);
    }

    async fn submit(&self, head: &str) -> anyhow::Result<AgentDelivery> {
        self.coordinator
            .submit_delivery(&self.subagent, &self.worktree, head, "cargo test passed")
            .await
    }

    async fn outcome(&self) -> AgentOutcomeRecord {
        self.store
            .list_agent_outcomes(&self.task_run_id)
            .await
            .unwrap()
            .into_iter()
            .find(|outcome| outcome.id == self.outcome_id)
            .unwrap()
    }

    async fn work_unit(&self) -> WorkUnitRecord {
        self.store
            .read_work_unit(&self.work_unit_id)
            .await
            .unwrap()
            .unwrap()
    }

    async fn assert_waiting(&self) {
        assert_eq!(
            self.outcome().await.status,
            AgentOutcomeStatus::WaitingForDelivery
        );
        assert_eq!(
            self.work_unit().await.status,
            WorkUnitStatus::WaitingForDelivery
        );
    }

    fn cleanup(self) {
        drop(self.coordinator);
        let worktree_text = self.worktree.to_string_lossy().to_string();
        let _ = Command::new("git")
            .arg("-C")
            .arg(&self.repository)
            .args(["worktree", "remove", "--force", &worktree_text])
            .output();
        remove_repository(self.worktree);
        remove_repository(self.repository);
    }
}

async fn drain_agent_state(
    fixture: &DeliveryFixture,
    outcome: crate::TurnOutcomeKind,
    summary: Option<&str>,
    error: Option<&str>,
) {
    fixture
        .coordinator
        .record_terminal_agent_state(
            &fixture.session_id,
            &StudioAgentTerminalChange {
                agent_id: fixture.subagent.id.clone(),
                role: "executor".to_string(),
                outcome,
                summary: summary.map(str::to_string),
                error: error.map(str::to_string),
            },
        )
        .await
        .unwrap();
}

#[tokio::test]
async fn task_start_initializes_non_repository_and_preserves_the_baseline_on_lease_failure() {
    let project = temporary_project("start-initializes-files");
    std::fs::write(project.join("README.md"), "initial\n").unwrap();
    std::fs::write(project.join(".gitignore"), "ignored.txt\n").unwrap();
    std::fs::write(project.join("ignored.txt"), "secret\n").unwrap();
    let store = task_store(&project).await;
    let session = task_session(&store, &project).await;
    let competing_session = store
        .create_session(&session.project_id, "Competing task", StudioMode::Task)
        .await
        .unwrap();
    let coordinator = TaskCoordinator::new(store.clone());

    let run = coordinator
        .start_confirmed_task(&session.id, "plan", &project)
        .await
        .unwrap();

    assert_eq!(run.branch, "main");
    assert_eq!(git_output(&project, &["rev-list", "--count", "HEAD"]), "1");
    assert_eq!(
        git_output(&project, &["log", "-1", "--pretty=%s"]),
        "chore: initialize Pure Studio workspace"
    );
    assert_eq!(
        git_output(&project, &["ls-tree", "-r", "--name-only", "HEAD"]),
        ".gitignore\nREADME.md"
    );
    assert!(git_output(&project, &["status", "--porcelain=v1"]).is_empty());
    let internal_worktree = project.join(".pure/worktrees/run/agent");
    std::fs::create_dir_all(&internal_worktree).unwrap();
    std::fs::write(internal_worktree.join("internal.txt"), "runtime\n").unwrap();
    let command_output = project.join("target/pure/session/tool");
    std::fs::create_dir_all(&command_output).unwrap();
    std::fs::write(command_output.join("output.log"), "command output\n").unwrap();
    assert!(git_output(&project, &["status", "--porcelain=v1"]).is_empty());
    let private_excludes = std::fs::read_to_string(project.join(".git/info/exclude")).unwrap();
    for expected in [".pure/worktrees/", "target/pure/"] {
        assert!(
            private_excludes.lines().any(|line| line.trim() == expected),
            "missing private exclude `{expected}`"
        );
    }

    coordinator
        .start_confirmed_task(&competing_session.id, "competing plan", &project)
        .await
        .expect_err("active branch lease must still reject a competing task");
    assert_eq!(git_output(&project, &["rev-list", "--count", "HEAD"]), "1");

    coordinator
        .finish_task(
            &run.id,
            TaskRunPhase::Cancelled,
            Some("test complete".into()),
        )
        .await
        .unwrap();
    remove_repository(project);
}

#[tokio::test]
async fn task_start_initializes_an_empty_project_with_an_empty_commit() {
    let project = temporary_project("start-initializes-empty");
    let store = task_store(&project).await;
    let session = task_session(&store, &project).await;
    let coordinator = TaskCoordinator::new(store);

    let run = coordinator
        .start_confirmed_task(&session.id, "plan", &project)
        .await
        .unwrap();

    assert_eq!(run.branch, "main");
    assert!(git_output(&project, &["status", "--porcelain=v1"]).is_empty());
    assert!(git_output(&project, &["ls-tree", "-r", "--name-only", "HEAD"]).is_empty());
    coordinator
        .finish_task(
            &run.id,
            TaskRunPhase::Cancelled,
            Some("test complete".into()),
        )
        .await
        .unwrap();
    remove_repository(project);
}

#[tokio::test]
async fn task_start_commits_an_unborn_branch_with_temporary_identity_only() {
    let project = temporary_project("start-initializes-unborn");
    git(&project, &["init", "-b", "draft"]);
    git(&project, &["config", "user.name", ""]);
    git(&project, &["config", "user.email", ""]);
    std::fs::write(project.join("README.md"), "initial\n").unwrap();
    let store = task_store(&project).await;
    let session = task_session(&store, &project).await;
    let coordinator = TaskCoordinator::new(store);

    let run = coordinator
        .start_confirmed_task(&session.id, "plan", &project)
        .await
        .unwrap();

    assert_eq!(run.branch, "draft");
    assert_eq!(
        git_output(&project, &["log", "-1", "--pretty=%an <%ae>"]),
        "Pure Studio <pure-studio@local>"
    );
    assert!(git_output(&project, &["config", "--local", "--get", "user.name"]).is_empty());
    assert!(git_output(&project, &["config", "--local", "--get", "user.email"]).is_empty());
    coordinator
        .finish_task(
            &run.id,
            TaskRunPhase::Cancelled,
            Some("test complete".into()),
        )
        .await
        .unwrap();
    remove_repository(project);
}

#[tokio::test]
async fn task_start_keeps_unborn_repository_and_no_run_when_initial_commit_hook_fails() {
    let project = temporary_project("start-initial-hook-failure");
    git(&project, &["init", "-b", "main"]);
    std::fs::write(project.join("README.md"), "initial\n").unwrap();
    let hook = project.join(".git/hooks/pre-commit");
    std::fs::write(&hook, "#!/bin/sh\nexit 1\n").unwrap();
    make_executable(&hook);
    let store = task_store(&project).await;
    let session = task_session(&store, &project).await;
    let coordinator = TaskCoordinator::new(store.clone());

    let error = coordinator
        .start_confirmed_task(&session.id, "plan", &project)
        .await
        .expect_err("a failing initial commit hook must stop task creation");

    assert!(error.to_string().contains("initial Git commit failed"));
    assert!(project.join(".git").is_dir());
    assert!(store.list_active_task_runs().await.unwrap().is_empty());
    let head = Command::new("git")
        .arg("-C")
        .arg(&project)
        .args(["rev-parse", "--verify", "HEAD"])
        .output()
        .unwrap();
    assert!(!head.status.success());
    remove_repository(project);
}

#[tokio::test]
async fn task_start_requires_clean_named_branch() {
    let repository = init_repository("start-guards");
    let store = task_store(&repository).await;
    let session = task_session(&store, &repository).await;
    let coordinator = TaskCoordinator::new(store);

    std::fs::write(repository.join("dirty.txt"), "dirty").unwrap();
    let dirty = coordinator
        .start_confirmed_task(&session.id, "plan", &repository)
        .await
        .expect_err("dirty repository must be rejected");
    assert!(dirty.to_string().contains("clean working tree"));

    std::fs::remove_file(repository.join("dirty.txt")).unwrap();
    git(&repository, &["checkout", "--detach"]);
    let detached = coordinator
        .start_confirmed_task(&session.id, "plan", &repository)
        .await
        .expect_err("detached HEAD must be rejected");
    assert!(detached.to_string().contains("detached HEAD"));

    remove_repository(repository);
}

#[tokio::test]
async fn task_start_rejects_existing_merge_rebase_and_corrupt_git_state() {
    let repository = init_repository("start-operation-guards");
    let store = task_store(&repository).await;
    let session = task_session(&store, &repository).await;
    let coordinator = TaskCoordinator::new(store);

    std::fs::write(repository.join(".git/MERGE_HEAD"), "invalid\n").unwrap();
    let merge = coordinator
        .start_confirmed_task(&session.id, "plan", &repository)
        .await
        .expect_err("merge state must be rejected");
    assert!(merge.to_string().contains("merge is in progress"));
    std::fs::remove_file(repository.join(".git/MERGE_HEAD")).unwrap();

    std::fs::create_dir(repository.join(".git/rebase-merge")).unwrap();
    let rebase = coordinator
        .start_confirmed_task(&session.id, "plan", &repository)
        .await
        .expect_err("rebase state must be rejected");
    assert!(rebase.to_string().contains("rebase is in progress"));
    remove_repository(repository);

    let corrupt = temporary_project("start-corrupt-git");
    std::fs::write(corrupt.join(".git"), "not a gitdir\n").unwrap();
    let store = task_store(&corrupt).await;
    let session = task_session(&store, &corrupt).await;
    let error = TaskCoordinator::new(store)
        .start_confirmed_task(&session.id, "plan", &corrupt)
        .await
        .expect_err("corrupt Git metadata must not be replaced");
    assert!(
        error
            .to_string()
            .contains("rev-parse --show-toplevel failed")
    );
    assert_eq!(
        std::fs::read_to_string(corrupt.join(".git")).unwrap(),
        "not a gitdir\n"
    );
    remove_repository(corrupt);
}

#[tokio::test]
async fn task_head_drift_blocks_the_persisted_run() {
    let repository = init_repository("head-drift");
    let store = task_store(&repository).await;
    let session = task_session(&store, &repository).await;
    let coordinator = TaskCoordinator::new(store.clone());
    let run = coordinator
        .start_confirmed_task(&session.id, "plan", &repository)
        .await
        .unwrap();

    let competing = TaskCoordinator::new(store.clone());
    let lease_error = competing
        .start_confirmed_task(&session.id, "another plan", &repository)
        .await
        .expect_err("one process may not own the branch twice");
    assert!(lease_error.to_string().contains("already owned"));

    std::fs::write(repository.join("external.txt"), "external").unwrap();
    git(&repository, &["add", "external.txt"]);
    git(&repository, &["commit", "-m", "external change"]);

    assert!(!coordinator.verify_expected_head(&run.id).await.unwrap());
    let blocked = store.read_task_run(&run.id).await.unwrap().unwrap();
    assert_eq!(blocked.phase, TaskRunPhase::Blocked);
    assert!(
        blocked
            .status_message
            .as_deref()
            .unwrap_or_default()
            .contains("HEAD drifted")
    );

    coordinator
        .finish_task(
            &run.id,
            TaskRunPhase::Cancelled,
            Some("stopped".to_string()),
        )
        .await
        .unwrap();
    assert!(store.read_branch_lease(&run.id).await.unwrap().is_none());
    remove_repository(repository);
}

#[tokio::test]
async fn coordinator_recovers_active_task_after_restart() {
    let repository = init_repository("recovery");
    let store = task_store(&repository).await;
    let session = task_session(&store, &repository).await;
    let run = {
        let coordinator = TaskCoordinator::new(store.clone());
        coordinator
            .start_confirmed_task(&session.id, "plan", &repository)
            .await
            .unwrap()
    };

    let recovered_coordinator = TaskCoordinator::new(store.clone());
    let recovered = recovered_coordinator.recover_active_tasks().await.unwrap();

    assert_eq!(recovered, vec![run.clone()]);
    assert!(
        recovered_coordinator
            .verify_expected_head(&run.id)
            .await
            .unwrap()
    );
    recovered_coordinator
        .finish_task(&run.id, TaskRunPhase::Cancelled, None)
        .await
        .unwrap();
    remove_repository(repository);
}

#[tokio::test]
async fn recovery_blocks_run_before_continuation_when_agent_pairs_are_invalid() {
    let repository = init_repository("recovery-agent-mismatch");
    let store = task_store(&repository).await;
    let session = task_session(&store, &repository).await;
    let run = {
        let coordinator = TaskCoordinator::new(store.clone());
        coordinator
            .start_confirmed_task(&session.id, "plan", &repository)
            .await
            .unwrap()
    };
    let unit = store
        .create_work_unit(CreateWorkUnit {
            task_run_id: run.id.clone(),
            title: "mismatch".to_string(),
            owned_paths: vec!["code/**".to_string()],
            base_commit: run.base_commit.clone(),
            worktree_path: repository
                .join(".pure/worktrees/run/agent-a")
                .to_string_lossy()
                .to_string(),
            branch: "pure-task-run-agent-a".to_string(),
            attempt: 1,
        })
        .await
        .unwrap();
    store
        .update_work_unit(
            &unit.id,
            WorkUnitStatus::Running,
            Some("agent-a".to_string()),
        )
        .await
        .unwrap();
    store
        .create_agent_outcome(CreateAgentOutcome {
            task_run_id: run.id.clone(),
            work_unit_id: Some(unit.id.clone()),
            agent_id: "agent-b".to_string(),
            owner_path: "/root".to_string(),
            initiated_by: "planner".to_string(),
            requested_by_call_id: "call".to_string(),
            role: "executor".to_string(),
            status: AgentOutcomeStatus::Running,
            attempt: 1,
        })
        .await
        .unwrap();

    let recovered = TaskCoordinator::new(store.clone())
        .recover_active_tasks()
        .await
        .unwrap();
    let blocked = store.read_task_run(&run.id).await.unwrap().unwrap();

    assert!(recovered.is_empty());
    assert_eq!(blocked.phase, TaskRunPhase::Blocked);
    assert!(
        blocked
            .status_message
            .as_deref()
            .unwrap_or_default()
            .contains("agent restart reconciliation failed")
    );
    assert_eq!(
        store
            .read_work_unit(&unit.id)
            .await
            .unwrap()
            .unwrap()
            .status,
        WorkUnitStatus::Running
    );
    remove_repository(repository);
}

#[tokio::test]
async fn recovery_preserves_restart_cancelled_worktree_and_cleans_orphan_leaf() {
    let repository = init_repository("recovery-worktree-reconcile");
    std::fs::write(repository.join(".gitignore"), ".pure/\n").unwrap();
    git(&repository, &["add", ".gitignore"]);
    git(&repository, &["commit", "-m", "ignore runtime"]);
    let store = task_store(&repository).await;
    let session = task_session(&store, &repository).await;
    let run = {
        let coordinator = TaskCoordinator::new(store.clone());
        coordinator
            .start_confirmed_task(&session.id, "plan", &repository)
            .await
            .unwrap()
    };
    let protected_path = repository
        .join(".pure/worktrees")
        .join(&run.id)
        .join("agent-owned");
    let protected_branch = format!("pure-task-{}-agent-owned", run.id);
    let protected_path_arg = protected_path.to_string_lossy().to_string();
    git(
        &repository,
        &[
            "worktree",
            "add",
            "-b",
            &protected_branch,
            &protected_path_arg,
            "HEAD",
        ],
    );
    let unit = store
        .create_work_unit(CreateWorkUnit {
            task_run_id: run.id.clone(),
            title: "owned".to_string(),
            owned_paths: vec!["code/**".to_string()],
            base_commit: run.base_commit.clone(),
            worktree_path: protected_path_arg,
            branch: protected_branch.clone(),
            attempt: 1,
        })
        .await
        .unwrap();
    store
        .update_work_unit(
            &unit.id,
            WorkUnitStatus::Running,
            Some("agent-owned".to_string()),
        )
        .await
        .unwrap();
    store
        .create_agent_outcome(CreateAgentOutcome {
            task_run_id: run.id.clone(),
            work_unit_id: Some(unit.id.clone()),
            agent_id: "agent-owned".to_string(),
            owner_path: "/root".to_string(),
            initiated_by: "planner".to_string(),
            requested_by_call_id: "call-owned".to_string(),
            role: "executor".to_string(),
            status: AgentOutcomeStatus::Running,
            attempt: 1,
        })
        .await
        .unwrap();
    let orphan_parent = repository.join(".pure/worktrees/orphan-run");
    let orphan_path = orphan_parent.join("orphan-agent");
    let orphan_path_arg = orphan_path.to_string_lossy().to_string();
    let orphan_branch = "pure-task-orphan-run-orphan-agent";
    git(
        &repository,
        &[
            "worktree",
            "add",
            "-b",
            orphan_branch,
            &orphan_path_arg,
            "HEAD",
        ],
    );
    std::fs::write(orphan_parent.join("audit.txt"), "keep").unwrap();

    let recovered = TaskCoordinator::new(store.clone())
        .recover_active_tasks()
        .await
        .unwrap();

    assert_eq!(recovered.len(), 1);
    assert!(protected_path.is_dir());
    assert!(!orphan_path.exists());
    assert!(orphan_parent.join("audit.txt").exists());
    assert!(git_output(&repository, &["branch", "--list", orphan_branch]).is_empty());
    assert_eq!(
        store
            .read_work_unit(&unit.id)
            .await
            .unwrap()
            .unwrap()
            .status,
        WorkUnitStatus::Cancelled
    );
    remove_repository(repository);
}

#[tokio::test]
async fn recovery_groups_linked_workspaces_by_canonical_git_common_directory() {
    let repository = init_repository("recovery-common-directory-group");
    std::fs::write(repository.join(".gitignore"), ".pure/\n").unwrap();
    git(&repository, &["add", ".gitignore"]);
    git(&repository, &["commit", "-m", "ignore runtime"]);
    let linked_workspace = repository.join(".pure/worktrees/user-workspace/linked");
    let linked_workspace_arg = linked_workspace.to_string_lossy().to_string();
    let linked_workspace_branch = "pure-agent-linked-user-workspace";
    git(
        &repository,
        &[
            "worktree",
            "add",
            "-b",
            linked_workspace_branch,
            &linked_workspace_arg,
            "HEAD",
        ],
    );
    let store = task_store(&repository).await;
    let main_session = task_session(&store, &repository).await;
    let linked_session = task_session(&store, &linked_workspace).await;
    let (main_run, linked_run) = {
        let coordinator = TaskCoordinator::new(store.clone());
        let main_run = coordinator
            .start_confirmed_task(&main_session.id, "main plan", &repository)
            .await
            .unwrap();
        let linked_run = coordinator
            .start_confirmed_task(&linked_session.id, "linked plan", &linked_workspace)
            .await
            .unwrap();
        (main_run, linked_run)
    };
    let main_owned =
        create_running_recovery_worktree(&store, &main_run, "main-owned", &repository).await;
    let linked_owned =
        create_running_recovery_worktree(&store, &linked_run, "linked-owned", &repository).await;
    let orphan_path = linked_workspace.join(".pure/worktrees/orphan-run/orphan-agent");
    let orphan_path_arg = orphan_path.to_string_lossy().to_string();
    let orphan_branch = "pure-task-orphan-run-shared-common-dir";
    git(
        &repository,
        &[
            "worktree",
            "add",
            "-b",
            orphan_branch,
            &orphan_path_arg,
            "HEAD",
        ],
    );

    let recovered = TaskCoordinator::new(store)
        .recover_active_tasks()
        .await
        .unwrap();
    let mut recovered_ids = recovered.into_iter().map(|run| run.id).collect::<Vec<_>>();
    let mut expected_ids = vec![main_run.id, linked_run.id];
    recovered_ids.sort();
    expected_ids.sort();

    assert_eq!(recovered_ids, expected_ids);
    assert!(main_owned.is_dir());
    assert!(linked_owned.is_dir());
    assert!(linked_workspace.is_dir());
    assert!(!orphan_path.exists());
    assert!(!git_output(&repository, &["branch", "--list", linked_workspace_branch]).is_empty());
    assert!(git_output(&repository, &["branch", "--list", orphan_branch]).is_empty());
    remove_repository(linked_workspace);
    remove_repository(repository);
}

#[tokio::test]
async fn recovery_preflight_failure_preserves_all_worktrees_before_group_gc() {
    let repository = init_repository("recovery-common-directory-preflight");
    std::fs::write(repository.join(".gitignore"), ".pure/\n").unwrap();
    git(&repository, &["add", ".gitignore"]);
    git(&repository, &["commit", "-m", "ignore runtime"]);
    let store = task_store(&repository).await;
    let active_session = task_session(&store, &repository).await;
    let active_run = {
        let coordinator = TaskCoordinator::new(store.clone());
        coordinator
            .start_confirmed_task(&active_session.id, "active plan", &repository)
            .await
            .unwrap()
    };
    let missing_workspace = repository.with_file_name(format!(
        "{}-missing",
        repository.file_name().unwrap().to_string_lossy()
    ));
    let missing_project = store.upsert_project(&missing_workspace).await.unwrap();
    let missing_session = store
        .create_session(&missing_project.id, "Blocked", StudioMode::Task)
        .await
        .unwrap();
    let git_common_dir = std::fs::canonicalize(repository.join(".git")).unwrap();
    let blocked_run = store
        .create_task_run_with_lease(CreateTaskRun {
            session_id: missing_session.id,
            phase: TaskRunPhase::Blocked,
            plan: "blocked plan".to_string(),
            workspace_root: missing_workspace.to_string_lossy().to_string(),
            git_common_dir: git_common_dir.to_string_lossy().to_string(),
            branch: "missing-workspace-branch".to_string(),
            head_commit: active_run.base_commit.clone(),
        })
        .await
        .unwrap()
        .0;
    let protected_path = repository
        .join(".pure/worktrees")
        .join(&blocked_run.id)
        .join("blocked-owned");
    let protected_path_arg = protected_path.to_string_lossy().to_string();
    let protected_branch = format!("pure-task-{}-blocked-owned", blocked_run.id);
    git(
        &repository,
        &[
            "worktree",
            "add",
            "-b",
            &protected_branch,
            &protected_path_arg,
            "HEAD",
        ],
    );
    let unit = store
        .create_work_unit(CreateWorkUnit {
            task_run_id: blocked_run.id.clone(),
            title: "blocked owned".to_string(),
            owned_paths: vec!["code/**".to_string()],
            base_commit: blocked_run.base_commit.clone(),
            worktree_path: protected_path_arg,
            branch: protected_branch.clone(),
            attempt: 1,
        })
        .await
        .unwrap();
    store
        .update_work_unit(
            &unit.id,
            WorkUnitStatus::Running,
            Some("blocked-owned".to_string()),
        )
        .await
        .unwrap();
    store
        .create_agent_outcome(CreateAgentOutcome {
            task_run_id: blocked_run.id,
            work_unit_id: Some(unit.id),
            agent_id: "blocked-owned".to_string(),
            owner_path: "/root".to_string(),
            initiated_by: "planner".to_string(),
            requested_by_call_id: "call-blocked-owned".to_string(),
            role: "executor".to_string(),
            status: AgentOutcomeStatus::Running,
            attempt: 1,
        })
        .await
        .unwrap();
    let orphan_path = repository.join(".pure/worktrees/orphan-run/preflight-orphan");
    let orphan_path_arg = orphan_path.to_string_lossy().to_string();
    let orphan_branch = "pure-task-orphan-run-preflight";
    git(
        &repository,
        &[
            "worktree",
            "add",
            "-b",
            orphan_branch,
            &orphan_path_arg,
            "HEAD",
        ],
    );

    let error = TaskCoordinator::new(store)
        .recover_active_tasks()
        .await
        .expect_err("an unresolved known workspace must stop reconciliation before GC");

    assert!(error.to_string().contains("known task workspace"));
    assert!(protected_path.is_dir());
    assert!(orphan_path.is_dir());
    assert!(!git_output(&repository, &["branch", "--list", &protected_branch]).is_empty());
    assert!(!git_output(&repository, &["branch", "--list", orphan_branch]).is_empty());
    remove_repository(repository);
}

#[tokio::test]
async fn recovery_allows_pending_allocation_before_worktree_creation() {
    let repository = init_repository("recovery-pending-before-create");
    let store = task_store(&repository).await;
    let session = task_session(&store, &repository).await;
    let run = {
        let coordinator = TaskCoordinator::new(store.clone());
        coordinator
            .start_confirmed_task(&session.id, "plan", &repository)
            .await
            .unwrap()
    };
    let path = Path::new(&run.workspace_root)
        .join(".pure/worktrees")
        .join(&run.id)
        .join("agent-pending");
    let unit = store
        .create_work_unit(CreateWorkUnit {
            task_run_id: run.id.clone(),
            title: "pending".to_string(),
            owned_paths: vec!["code/**".to_string()],
            base_commit: run.base_commit.clone(),
            worktree_path: path.to_string_lossy().to_string(),
            branch: format!("pure-task-{}-agent-pending", run.id),
            attempt: 1,
        })
        .await
        .unwrap();
    store
        .update_work_unit(
            &unit.id,
            WorkUnitStatus::Pending,
            Some("agent-pending".to_string()),
        )
        .await
        .unwrap();
    store
        .create_agent_outcome(CreateAgentOutcome {
            task_run_id: run.id.clone(),
            work_unit_id: Some(unit.id.clone()),
            agent_id: "agent-pending".to_string(),
            owner_path: "/root".to_string(),
            initiated_by: "planner".to_string(),
            requested_by_call_id: "call-pending".to_string(),
            role: "executor".to_string(),
            status: AgentOutcomeStatus::Queued,
            attempt: 1,
        })
        .await
        .unwrap();

    let recovered = TaskCoordinator::new(store.clone())
        .recover_active_tasks()
        .await
        .unwrap();

    let current = store.read_task_run(&run.id).await.unwrap().unwrap();
    assert_eq!(recovered.len(), 1, "{current:?}");
    assert!(!path.exists());
    assert_eq!(
        store
            .read_work_unit(&unit.id)
            .await
            .unwrap()
            .unwrap()
            .status,
        WorkUnitStatus::Cancelled
    );
    remove_repository(repository);
}

#[tokio::test]
async fn recovery_reconciles_terminal_workspace_without_active_run_once() {
    let repository = init_repository("recovery-terminal-workspace");
    std::fs::write(repository.join(".gitignore"), ".pure/\n").unwrap();
    git(&repository, &["add", ".gitignore"]);
    git(&repository, &["commit", "-m", "ignore runtime"]);
    let store = task_store(&repository).await;
    let session = task_session(&store, &repository).await;
    let coordinator = TaskCoordinator::new(store.clone());
    let run = coordinator
        .start_confirmed_task(&session.id, "plan", &repository)
        .await
        .unwrap();
    let merged_path = crate::agent::worktree::git_compatible_path(
        Path::new(&run.workspace_root)
            .join(".pure/worktrees")
            .join(&run.id)
            .join("agent-merged"),
    );
    let merged_branch = format!("pure-task-{}-agent-merged", run.id);
    let merged_path_arg = merged_path.to_string_lossy().to_string();
    git(
        &repository,
        &[
            "worktree",
            "add",
            "-b",
            &merged_branch,
            &merged_path_arg,
            "HEAD",
        ],
    );
    let unit = store
        .create_work_unit(CreateWorkUnit {
            task_run_id: run.id.clone(),
            title: "merged".to_string(),
            owned_paths: vec!["code/**".to_string()],
            base_commit: run.base_commit.clone(),
            worktree_path: merged_path_arg.clone(),
            branch: merged_branch.clone(),
            attempt: 1,
        })
        .await
        .unwrap();
    store
        .update_work_unit(
            &unit.id,
            WorkUnitStatus::Delivered,
            Some("agent-merged".to_string()),
        )
        .await
        .unwrap();
    let outcome = store
        .create_agent_outcome(CreateAgentOutcome {
            task_run_id: run.id.clone(),
            work_unit_id: Some(unit.id),
            agent_id: "agent-merged".to_string(),
            owner_path: "/root".to_string(),
            initiated_by: "planner".to_string(),
            requested_by_call_id: "call-merged".to_string(),
            role: "executor".to_string(),
            status: AgentOutcomeStatus::Completed,
            attempt: 1,
        })
        .await
        .unwrap();
    store
        .update_agent_outcome(
            &outcome.id,
            UpdateAgentOutcome {
                status: AgentOutcomeStatus::Completed,
                summary: Some("merged".to_string()),
                error: None,
                delivery: Some(AgentDelivery {
                    worktree: AgentWorktreeDelivery {
                        path: merged_path_arg,
                        branch: merged_branch.clone(),
                    },
                    base_commit: run.base_commit.clone(),
                    head_commit: run.base_commit.clone(),
                    changed_files: Vec::new(),
                    verification_summary: "passed".to_string(),
                }),
                review: None,
            },
        )
        .await
        .unwrap();
    store
        .transition_task_run(&run.id, TaskRunPhase::Implementing, None)
        .await
        .unwrap();
    let merge = store
        .begin_task_merge(BeginTaskMerge {
            session_id: session.id.clone(),
            agent_id: "agent-merged".to_string(),
            expected_head: run.expected_head.clone(),
            pre_index_tree: run.expected_head.clone(),
            changed_files: Vec::new(),
        })
        .await
        .unwrap()
        .merge;
    store.mark_task_merge_verifying(&merge.id).await.unwrap();
    store
        .complete_task_merge(CompleteTaskMerge {
            merge_id: merge.id.clone(),
            expected_head: run.expected_head.clone(),
            merge_commit: run.expected_head.clone(),
            verification_steps: Vec::new(),
        })
        .await
        .unwrap();
    store
        .record_merge_cleanup(
            &merge.id,
            MergeCleanupEvidence {
                status: "discarded".to_string(),
                detail: Some("simulated accepted cleanup before restart".to_string()),
            },
        )
        .await
        .unwrap();
    coordinator
        .finish_task(&run.id, TaskRunPhase::Cancelled, None)
        .await
        .unwrap();

    let orphan_path = crate::agent::worktree::git_compatible_path(
        Path::new(&run.workspace_root).join(".pure/worktrees/orphan-run/orphan-agent"),
    );
    let orphan_path_arg = orphan_path.to_string_lossy().to_string();
    let orphan_branch = "pure-task-orphan-run-orphan-agent";
    git(
        &repository,
        &[
            "worktree",
            "add",
            "-b",
            orphan_branch,
            &orphan_path_arg,
            "HEAD",
        ],
    );

    let recovered = TaskCoordinator::new(store.clone())
        .recover_active_tasks()
        .await
        .unwrap();
    let repeated = TaskCoordinator::new(store)
        .recover_active_tasks()
        .await
        .unwrap();

    assert!(recovered.is_empty());
    assert!(repeated.is_empty());
    assert!(!merged_path.exists());
    assert!(!orphan_path.exists());
    assert!(git_output(&repository, &["branch", "--list", &merged_branch]).is_empty());
    assert!(git_output(&repository, &["branch", "--list", orphan_branch]).is_empty());
    remove_repository(repository);
}

async fn task_store(repository: &Path) -> StudioStore {
    let store = StudioStore::open_memory().await.unwrap();
    store.upsert_project(repository).await.unwrap();
    store
}

async fn task_session(store: &StudioStore, repository: &Path) -> crate::studio::SessionRecord {
    let project = store.upsert_project(repository).await.unwrap();
    store
        .create_session(&project.id, "Task", StudioMode::Task)
        .await
        .unwrap()
}

async fn create_running_recovery_worktree(
    store: &StudioStore,
    run: &TaskRunRecord,
    agent_id: &str,
    git_repository: &Path,
) -> PathBuf {
    let worktree = crate::agent::worktree::git_compatible_path(
        Path::new(&run.workspace_root)
            .join(".pure/worktrees")
            .join(&run.id)
            .join(agent_id),
    );
    let branch = format!("pure-task-{}-{agent_id}", run.id);
    let worktree_arg = worktree.to_string_lossy().to_string();
    git(
        git_repository,
        &["worktree", "add", "-b", &branch, &worktree_arg, "HEAD"],
    );
    let unit = store
        .create_work_unit(CreateWorkUnit {
            task_run_id: run.id.clone(),
            title: agent_id.to_string(),
            owned_paths: vec![format!("code/{agent_id}/**")],
            base_commit: run.base_commit.clone(),
            worktree_path: worktree_arg,
            branch,
            attempt: 1,
        })
        .await
        .unwrap();
    store
        .update_work_unit(
            &unit.id,
            WorkUnitStatus::Running,
            Some(agent_id.to_string()),
        )
        .await
        .unwrap();
    store
        .create_agent_outcome(CreateAgentOutcome {
            task_run_id: run.id.clone(),
            work_unit_id: Some(unit.id),
            agent_id: agent_id.to_string(),
            owner_path: "/root".to_string(),
            initiated_by: "planner".to_string(),
            requested_by_call_id: format!("call-{agent_id}"),
            role: "executor".to_string(),
            status: AgentOutcomeStatus::Running,
            attempt: 1,
        })
        .await
        .unwrap();
    worktree
}

fn init_repository(name: &str) -> PathBuf {
    let path = temporary_project(name);
    git(&path, &["init"]);
    git(&path, &["checkout", "-b", "main"]);
    git(&path, &["config", "user.email", "pure@example.invalid"]);
    git(&path, &["config", "user.name", "Pure Test"]);
    git(&path, &["config", "core.autocrlf", "false"]);
    git(&path, &["config", "commit.gpgSign", "false"]);
    git(&path, &["config", "merge.renames", "true"]);
    std::fs::write(path.join("README.md"), "initial\n").unwrap();
    git(&path, &["add", "README.md"]);
    git(&path, &["commit", "-m", "initial"]);
    path
}

fn temporary_project(name: &str) -> PathBuf {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = std::env::temp_dir().join(format!(
        "pure-task-coordinator-{name}-{}-{stamp}",
        std::process::id()
    ));
    std::fs::create_dir_all(&path).unwrap();
    path
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
    assert!(
        output.status.success(),
        "git {} failed: {}",
        args.join(" "),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

fn remove_repository(path: PathBuf) {
    let _ = std::fs::remove_dir_all(path);
}

#[cfg(unix)]
fn make_test_hook_executable(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let mut permissions = std::fs::metadata(path).unwrap().permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(path, permissions).unwrap();
}

#[cfg(windows)]
fn make_test_hook_executable(_path: &Path) {}

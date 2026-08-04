use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use sea_orm::{ConnectionTrait, DatabaseBackend, Statement};

use super::merge::{MergeCommitTestBarrier, MergeVerifier};
use super::*;
use crate::tool::{SubagentContext, Tool, ToolContext, ToolInput, WorkspaceAccess};
use crate::{
    AgentSession, StudioMode, StudioRecoveryIssueCategory, StudioStore, TurnOptions,
    TurnToolCacheHandle, TurnWorkingSetHandle,
};

#[derive(Debug, Clone, Copy, Default)]
struct TestRuntimeMarker;

#[tokio::test]
async fn reviewer_harness_authorization_is_one_shot_and_has_no_work_unit() {
    let fixture = ReviewFixture::new("review-harness-authorization").await;
    let round = fixture
        .store
        .begin_integrated_review(&fixture.session_id, "call-review")
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
async fn reviewer_terminal_without_review_exit_fails_round_and_restores_phase() {
    let fixture = ReviewFixture::new("reviewer-terminal-without-exit").await;
    fixture
        .store
        .begin_integrated_review(&fixture.session_id, "call-review-terminal")
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

    fixture
        .store
        .settle_reviewer_turn_finished(
            "agent-reviewer-terminal",
            crate::TurnOutcomeKind::Completed,
            Some("returned text instead of review_exit"),
        )
        .await
        .unwrap();

    let run = fixture
        .store
        .read_task_run(&fixture.run_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(run.phase, TaskRunPhase::Reworking);
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
        .begin_integrated_review(&fixture.session_id, "call-review-retry")
        .await
        .unwrap();
    assert_eq!(retry.round, 2);
    assert_eq!(retry.reviewed_head, round.reviewed_head);
    fixture.cleanup();
}

#[tokio::test]
async fn review_exit_requires_real_design_trace_and_persists_matching_pass() {
    let fixture = ReviewFixture::new("review-exit-trace").await;
    fixture
        .store
        .begin_integrated_review(&fixture.session_id, "call-review-exit")
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
                    parent_id: Some(
                        crate::studio::agent_host::root_agent_id(&fixture.session_id).to_string(),
                    ),
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
    fixture.cleanup();
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
        .begin_integrated_review(&fixture.session_id, "call-complete-review")
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
async fn task_stop_settles_transient_agents_and_atomically_releases_lease() {
    let fixture = ReviewFixture::new("task-stop-terminalization").await;
    let run = fixture
        .store
        .read_task_run(&fixture.run_id)
        .await
        .unwrap()
        .unwrap();

    let requested = fixture
        .store
        .request_task_stop(
            &run.id,
            &run.expected_head,
            TaskStopOrigin::UserRequest,
            &TaskStopReason::new("user stopped task").unwrap(),
        )
        .await
        .unwrap();
    fixture
        .store
        .begin_task_stop(&run.id, &run.expected_head, requested.task_generation)
        .await
        .unwrap();
    fixture
        .store
        .settle_agents_for_task_stop(&run.id, requested.task_generation, "user stopped task")
        .await
        .unwrap();
    let cancelled = fixture
        .store
        .cancel_task_and_release_lease(
            &run.id,
            &run.expected_head,
            requested.task_generation,
            "user stopped task",
        )
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
async fn restart_resumes_stopping_without_starting_a_model() {
    let fixture = ReviewFixture::new("task-stop-restart").await;
    let run = fixture
        .store
        .read_task_run(&fixture.run_id)
        .await
        .unwrap()
        .unwrap();
    let requested = fixture
        .store
        .request_task_stop(
            &run.id,
            &run.expected_head,
            TaskStopOrigin::PlannerDecision,
            &TaskStopReason::new("resume deterministic stop").unwrap(),
        )
        .await
        .unwrap();
    fixture
        .store
        .begin_task_stop(&run.id, &run.expected_head, requested.task_generation)
        .await
        .unwrap();
    fixture.coordinator.suspend();

    let recovered = Arc::new(TaskCoordinator::new(fixture.store.clone()));
    let mut terminal_facts = recovered.subscribe_terminal_facts();
    let report = recovered.recover_active_tasks().await.unwrap();
    let cancelled = fixture
        .store
        .read_task_run(&fixture.run_id)
        .await
        .unwrap()
        .unwrap();

    assert!(report.recovered_runs.is_empty());
    assert!(report.issues.is_empty());
    assert_eq!(cancelled.phase, TaskRunPhase::Cancelled);
    assert_eq!(
        terminal_facts.try_recv().unwrap(),
        fixture.run_id,
        "recovered stopping must publish its durable terminal fact"
    );
    assert!(
        fixture
            .store
            .read_branch_lease(&fixture.run_id)
            .await
            .unwrap()
            .is_none()
    );

    drop(recovered);
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
    assert_eq!(
        durable_run.terminal_generation,
        Some(durable_run.task_generation)
    );
    assert_eq!(fixture.work_unit().await.status, WorkUnitStatus::Merging);
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
    assert_eq!(fixture.work_unit().await.status, WorkUnitStatus::Merging);
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
async fn restart_recovery_aborts_exact_verifying_merge_and_blocks_with_evidence() {
    let fixture = IncrementalMergeFixture::new("merge-verifying-recovery").await;
    let delivered = fixture
        .deliver("agent_verifying_recovery", "src/lib.rs", "delivered\n")
        .await;
    let delivery =
        approved_delivery_for_work_unit(&fixture.store, &fixture.run.id, &delivered.work_unit_id)
            .await;
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
    let delivery =
        approved_delivery_for_work_unit(&fixture.store, &fixture.run.id, &delivered.work_unit_id)
            .await;
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

    fixture
        .store
        .settle_executor_turn_finished(
            &fixture.subagent.id,
            crate::TurnOutcomeKind::Failed,
            Some("late executor error"),
        )
        .await
        .unwrap();

    assert_eq!(fixture.work_unit().await.status, WorkUnitStatus::Merged);
    assert_eq!(
        fixture.outcome().await.status,
        AgentOutcomeStatus::Completed
    );
    fixture.cleanup();
}

#[tokio::test]
async fn budget_limited_executor_keeps_awaiting_completion_contract() {
    let fixture = DeliveryFixture::new("budget-awaiting-completion", vec!["src/**"]).await;

    fixture
        .store
        .settle_executor_turn_finished(
            &fixture.subagent.id,
            crate::TurnOutcomeKind::BudgetLimited,
            Some("active wall-clock budget reached"),
        )
        .await
        .unwrap();

    assert_eq!(
        fixture.work_unit().await.status,
        WorkUnitStatus::AwaitingCompletion
    );
    let outcome = fixture.outcome().await;
    assert_eq!(outcome.status, AgentOutcomeStatus::Failed);
    assert_eq!(
        outcome.error.as_deref(),
        Some("active wall-clock budget reached")
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
        let outcome = self
            .store
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
        approve_delivery(
            &self.store,
            &self.session_id,
            &work_unit,
            &outcome,
            AgentDelivery {
                worktree: AgentWorktreeDelivery {
                    path: std::fs::canonicalize(&worktree)
                        .unwrap()
                        .to_string_lossy()
                        .to_string(),
                    branch,
                },
                base_commit: work_unit.base_commit.clone(),
                head_commit: head,
                changed_files: vec![relative_path.to_string()],
                verification_summary: "test passed".to_string(),
            },
        )
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

impl ConflictMergeFixture {
    async fn text(name: &str) -> Self {
        Self::new(name).await
    }

    async fn new(name: &str) -> Self {
        let repository = init_repository(name);
        std::fs::create_dir_all(repository.join("src")).unwrap();
        let base_path = "src/shared.txt";
        std::fs::write(repository.join(base_path), b"base\n").unwrap();
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
                owned_paths: vec!["src/shared.txt".to_string()],
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
        let outcome = store
            .create_agent_outcome(CreateAgentOutcome {
                task_run_id: run.id.clone(),
                work_unit_id: Some(work_unit.id.clone()),
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
        std::fs::write(worktree.join("src/shared.txt"), "executor\n").unwrap();
        git(&worktree, &["add", "src/shared.txt"]);
        git(&worktree, &["commit", "-m", "executor conflicting edit"]);
        let source_head = git_output(&worktree, &["rev-parse", "HEAD"]);
        let changed_files =
            super::git::changed_files_between(&worktree, &run.expected_head, &source_head)
                .await
                .unwrap();
        approve_delivery(
            &store,
            &session.id,
            &work_unit,
            &outcome,
            AgentDelivery {
                worktree: AgentWorktreeDelivery {
                    path: std::fs::canonicalize(&worktree)
                        .unwrap()
                        .to_string_lossy()
                        .to_string(),
                    branch: branch.clone(),
                },
                base_commit: work_unit.base_commit.clone(),
                head_commit: source_head.clone(),
                changed_files,
                verification_summary: "test passed".to_string(),
            },
        )
        .await
        .unwrap();
        std::fs::write(repository.join("src/shared.txt"), "planner branch\n").unwrap();
        git(&repository, &["add", "src/shared.txt"]);
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
        store
            .advance_task_design_head(&run.id, &run.expected_head, &run.expected_head)
            .await
            .unwrap();
        let run = store.read_task_run(&run.id).await.unwrap().unwrap();
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
        let snapshot = super::git::inspect_repository(&self.worktree, true).await?;
        let resolved = super::git::resolve_commit_oid(&self.worktree, head).await?;
        if resolved != snapshot.head {
            anyhow::bail!("headCommit does not match worktree HEAD");
        }
        let changed_files =
            super::git::changed_files_between(&self.worktree, &self.base_commit, &resolved).await?;
        let work_unit = self.work_unit().await;
        let outcome = self.outcome().await;
        let delivery = AgentDelivery {
            worktree: AgentWorktreeDelivery {
                path: std::fs::canonicalize(&self.worktree)?
                    .to_string_lossy()
                    .to_string(),
                branch: self.branch.clone(),
            },
            base_commit: self.base_commit.clone(),
            head_commit: resolved,
            changed_files,
            verification_summary: "cargo test passed".to_string(),
        };
        approve_delivery(
            &self.store,
            &self.session_id,
            &work_unit,
            &outcome,
            delivery,
        )
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

async fn approve_delivery(
    store: &StudioStore,
    session_id: &str,
    work_unit: &WorkUnitRecord,
    outcome: &AgentOutcomeRecord,
    delivery: AgentDelivery,
) -> anyhow::Result<AgentDelivery> {
    let completion = store
        .create_work_completion(
            &outcome.id,
            &work_unit.id,
            WorkCompletionKind::Delivery,
            Some(&delivery),
            &delivery.verification_summary,
        )
        .await?;
    let requested_by_call_id = format!("review-{}", completion.id);
    store
        .begin_delivery_review(session_id, &outcome.agent_id, &requested_by_call_id)
        .await?;
    let reviewer_agent_id = format!("reviewer-{}", completion.id);
    let (_, reviewer_outcome) = store
        .authorize_reviewer_spawn(session_id, &requested_by_call_id, &reviewer_agent_id)
        .await?;
    store
        .update_spawned_outcome(
            &reviewer_outcome.id,
            &reviewer_agent_id,
            AgentOutcomeStatus::Running,
            None,
        )
        .await?;
    store
        .complete_task_review(
            session_id,
            &reviewer_agent_id,
            AgentReview {
                verdict: ReviewVerdict::Pass,
                summary: "delivery review passed".to_string(),
                design_references: Vec::new(),
                findings: Vec::new(),
            },
        )
        .await?;
    persist_closed_executor_snapshot(store, outcome).await?;
    Ok(delivery)
}

async fn approved_delivery_for_work_unit(
    store: &StudioStore,
    task_run_id: &str,
    work_unit_id: &str,
) -> AgentDelivery {
    let completion = store
        .list_work_completions(task_run_id)
        .await
        .unwrap()
        .into_iter()
        .filter(|completion| completion.work_unit_id == work_unit_id)
        .max_by_key(|completion| completion.revision)
        .unwrap();
    assert_eq!(completion.status, WorkCompletionStatus::Approved);
    AgentDelivery {
        worktree: AgentWorktreeDelivery {
            path: completion.worktree_path,
            branch: completion.branch,
        },
        base_commit: completion.base_commit,
        head_commit: completion.head_commit.unwrap(),
        changed_files: completion.changed_files,
        verification_summary: completion.verification_summary,
    }
}

async fn persist_closed_executor_snapshot(
    store: &StudioStore,
    outcome: &AgentOutcomeRecord,
) -> anyhow::Result<()> {
    let snapshot = pl_core::AgentSnapshot {
        identity: pl_core::AgentIdentity {
            id: pl_core::AgentId::new(outcome.agent_id.clone())?,
            parent_id: Some(pl_core::AgentId::new("root")?),
            role: pl_core::AgentRoleId::new("executor")?,
            depth: 1,
        },
        lifecycle: pl_core::AgentLifecycleState::Closed,
        activity: pl_core::AgentActivityState::Idle,
        active_turn_id: None,
        pending_inputs: 0,
        progress: None,
        last_turn: None,
        revision: 1,
        event_sequence: 1,
        updated_at: 1,
    };
    store
        .database()
        .execute_raw(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "INSERT INTO agent_runtime_states (agent_id, revision, snapshot_json, updated_at)
             VALUES (?, ?, ?, ?)",
            [
                outcome.agent_id.clone().into(),
                1_i64.into(),
                serde_json::to_string(&snapshot)?.into(),
                1_i64.into(),
            ],
        ))
        .await?;
    Ok(())
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
async fn recovery_preflight_failure_reports_issues_and_preserves_affected_group() {
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

    let safe_repository = init_repository("recovery-safe-common-directory");
    std::fs::write(safe_repository.join(".gitignore"), ".pure/\n").unwrap();
    git(&safe_repository, &["add", ".gitignore"]);
    git(&safe_repository, &["commit", "-m", "ignore runtime"]);
    let safe_session = task_session(&store, &safe_repository).await;
    let safe_run = {
        let coordinator = TaskCoordinator::new(store.clone());
        coordinator
            .start_confirmed_task(&safe_session.id, "safe plan", &safe_repository)
            .await
            .unwrap()
    };
    let safe_owned =
        create_running_recovery_worktree(&store, &safe_run, "safe-owned", &safe_repository).await;
    let safe_orphan = safe_repository.join(".pure/worktrees/orphan-run/safe-preflight-orphan");
    let safe_orphan_arg = safe_orphan.to_string_lossy().to_string();
    let safe_orphan_branch = "pure-task-orphan-run-safe-preflight";
    git(
        &safe_repository,
        &[
            "worktree",
            "add",
            "-b",
            safe_orphan_branch,
            &safe_orphan_arg,
            "HEAD",
        ],
    );

    let report = TaskCoordinator::new(store)
        .recover_active_tasks()
        .await
        .expect("an unresolved group must degrade into recovery issues");

    assert_eq!(
        report
            .recovered_runs
            .iter()
            .map(|run| run.id.as_str())
            .collect::<Vec<_>>(),
        vec![safe_run.id.as_str()]
    );
    assert_eq!(report.issues.len(), 2);
    assert!(
        report
            .issues
            .iter()
            .all(|issue| issue.category == StudioRecoveryIssueCategory::Repository)
    );
    assert!(
        report
            .issues
            .iter()
            .all(|issue| issue.message.contains("known task workspace"))
    );
    assert!(protected_path.is_dir());
    assert!(orphan_path.is_dir());
    assert!(safe_owned.is_dir());
    assert!(!safe_orphan.exists());
    assert!(!git_output(&repository, &["branch", "--list", &protected_branch]).is_empty());
    assert!(!git_output(&repository, &["branch", "--list", orphan_branch]).is_empty());
    assert!(git_output(&safe_repository, &["branch", "--list", safe_orphan_branch]).is_empty());
    remove_repository(safe_repository);
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

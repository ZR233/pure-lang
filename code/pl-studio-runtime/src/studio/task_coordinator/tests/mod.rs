use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use super::merge::TaskRecordMergeInput;
use super::spawn::TaskExecutorVerificationCommandV1;
use super::*;
use crate::tool::{SubagentContext, Tool, ToolContext, ToolInput, WorkspaceAccess};
use crate::{
    AgentSession, StudioMode, StudioRecoveryIssueAction, StudioRecoveryIssueCategory, StudioStore,
    TurnOptions, TurnToolCacheHandle, TurnWorkingSetHandle,
};

#[tokio::test]
async fn executor_spawn_call_is_idempotent_and_carries_durable_handoff() {
    let repository = init_repository("executor-spawn-idempotent-handoff");
    let store = task_store(&repository).await;
    let session = task_session(&store, &repository).await;
    let coordinator = Arc::new(TaskCoordinator::new(store.clone()));
    let run = coordinator
        .start_confirmed_task(&session.id, "confirmed plan", &repository)
        .await
        .unwrap();
    store
        .advance_task_design_head(&run.id, &run.expected_head, &run.expected_head)
        .await
        .unwrap();
    let request = StudioTaskSpawnRequest {
        agent_id: "thread-task-stable".to_string(),
        root_thread_id: session.id.clone(),
        task_name: "implement model transport".to_string(),
        role: "executor".to_string(),
        scope_hints: vec!["code/pl-model".to_string()],
        requested_by_call_id: "call-stable".to_string(),
        review_round_id: None,
        assignment: Some("move transport selection to ModelInfo".to_string()),
        acceptance_criteria: vec!["model-level routing is tested".to_string()],
        dependencies: Vec::new(),
        evidence: Vec::new(),
        verification_commands: vec![TaskExecutorVerificationCommandV1 {
            command: "cargo test -p pl-model".to_string(),
            cwd: ".".to_string(),
            purpose: "verify model transport".to_string(),
        }],
    };

    let first = coordinator.prepare_agent_spawn(&request).await.unwrap();
    let second = coordinator.prepare_agent_spawn(&request).await.unwrap();
    let semantic_duplicate = coordinator
        .reserve_executor_spawn(AllocateExecutor {
            thread_id: session.id.clone(),
            title: request.task_name.clone(),
            scope_hints: request.scope_hints.clone(),
            agent_id: "thread-task-duplicate".to_string(),
            requested_by_call_id: "call-duplicate".to_string(),
        })
        .await
        .unwrap();

    assert_eq!(first.lifecycle_token(), second.lifecycle_token());
    assert!(semantic_duplicate.reused);
    assert_eq!(
        semantic_duplicate.work_unit.requested_by_call_id,
        "call-stable"
    );
    assert_eq!(
        semantic_duplicate.work_unit.executor_thread_id.as_deref(),
        Some("thread-task-stable")
    );
    assert_eq!(store.list_work_units(&run.id).await.unwrap().len(), 1);
    let section = first.initial_context().first().expect("handoff section");
    let handoff = TaskExecutorHandoffV1::from_context_section(section).unwrap();
    assert_eq!(handoff.task_run_id, run.id);
    assert_eq!(handoff.requesting_call_id, "call-stable");
    assert_eq!(handoff.assignment, "move transport selection to ModelInfo");
    assert_eq!(handoff.scope_hints, vec!["code/pl-model"]);
    assert_eq!(
        handoff.verification.commands[0].command,
        "cargo test -p pl-model"
    );

    coordinator.suspend();
    remove_repository(repository);
}

#[tokio::test]
async fn reviewer_harness_authorization_is_one_shot_and_has_no_work_unit() {
    let fixture = ReviewFixture::new("review-harness-authorization").await;
    let round = fixture
        .store
        .begin_integrated_review(&fixture.root_thread_id, "call-review")
        .await
        .unwrap();
    let request = StudioTaskSpawnRequest {
        agent_id: "agent-reviewer".to_string(),
        root_thread_id: fixture.root_thread_id.clone(),
        task_name: "review_round_1".to_string(),
        role: "reviewer".to_string(),
        scope_hints: Vec::new(),
        review_round_id: Some(round.id.clone()),
        requested_by_call_id: "call-review".to_string(),
        assignment: None,
        acceptance_criteria: Vec::new(),
        dependencies: Vec::new(),
        evidence: Vec::new(),
        verification_commands: Vec::new(),
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
    let rounds = fixture
        .store
        .list_review_rounds(&fixture.run_id)
        .await
        .unwrap();
    assert_eq!(rounds[0].id, round.id);
    assert_eq!(
        rounds[0].reviewer_thread_id.as_deref(),
        Some("agent-reviewer")
    );
    assert_eq!(rounds[0].requested_by_call_id, "call-review");
    assert_eq!(rounds[0].reviewer_status, ThreadExecutionStatus::Running);
    fixture.cleanup();
}

#[tokio::test]
async fn reviewer_terminal_without_review_exit_fails_round_and_restores_phase() {
    let fixture = ReviewFixture::new("reviewer-terminal-without-exit").await;
    fixture
        .store
        .begin_integrated_review(&fixture.root_thread_id, "call-review-terminal")
        .await
        .unwrap();
    let round = fixture
        .store
        .authorize_reviewer_spawn(
            &fixture.root_thread_id,
            "call-review-terminal",
            "agent-reviewer-terminal",
        )
        .await
        .unwrap();
    fixture
        .store
        .activate_reviewer(&round.id, "agent-reviewer-terminal")
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
    assert_eq!(round.reviewer_status, ThreadExecutionStatus::Failed);
    let wakes = fixture
        .store
        .list_pending_task_planner_wakes()
        .await
        .unwrap();
    assert_eq!(wakes.len(), 1);
    assert!(matches!(
        &wakes[0].source,
        TaskPlannerWakeSource::Review {
            review_round_id,
            scope: ReviewScope::Integrated,
        } if review_round_id == &round.id
    ));
    let retry = fixture
        .store
        .begin_integrated_review(&fixture.root_thread_id, "call-review-retry")
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
        .begin_integrated_review(&fixture.root_thread_id, "call-review-exit")
        .await
        .unwrap();
    let round = fixture
        .store
        .authorize_reviewer_spawn(
            &fixture.root_thread_id,
            "call-review-exit",
            "agent-reviewer-exit",
        )
        .await
        .unwrap();
    fixture
        .store
        .activate_reviewer(&round.id, "agent-reviewer-exit")
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
        .review_exit_tool(fixture.root_thread_id.clone(), None);
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
                workspace: pl_core::AgentWorkspace::local(fixture.repository.clone()),
                workspace_instructions: None,
                instruction_snapshot: None,
                provider_call_id: Some("call-review-exit".to_string()),
                active_subagent: Some(SubagentContext {
                    id: "agent-reviewer-exit".to_string(),
                    parent_id: Some(
                        crate::studio::agent_host::root_agent_id(&fixture.root_thread_id)
                            .to_string(),
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
    assert_eq!(rounds[0].reviewer_status, ThreadExecutionStatus::Completed);
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
        .complete_reviewed_task(&fixture.root_thread_id, &run.expected_head)
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
        .begin_integrated_review(&fixture.root_thread_id, "call-complete-review")
        .await
        .unwrap();
    let round = fixture
        .store
        .authorize_reviewer_spawn(
            &fixture.root_thread_id,
            "call-complete-review",
            "agent-complete-review",
        )
        .await
        .unwrap();
    fixture
        .store
        .activate_reviewer(&round.id, "agent-complete-review")
        .await
        .unwrap();
    fixture
        .store
        .complete_task_review(
            &fixture.root_thread_id,
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
        .complete_reviewed_task(&fixture.root_thread_id, &run.expected_head)
        .await
        .unwrap();

    assert_eq!(completed.phase, TaskRunPhase::Completed);
    assert_eq!(completed.status_message, None);
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
async fn task_complete_does_not_run_project_checks_for_flutter_changes() {
    let fixture = DeliveryFixture::new(
        "task-completion-with-flutter-changes",
        vec!["code/pure-studio"],
    )
    .await;
    fixture.commit_file("code/pure-studio/lib/invalid_for_analyzer.dart");
    let delivery_head = git_output(&fixture.worktree, &["rev-parse", "HEAD"]);
    let delivery = fixture.submit(&delivery_head).await.unwrap();
    let run = fixture
        .store
        .read_task_run(&fixture.task_run_id)
        .await
        .unwrap()
        .unwrap();
    let resulting_head =
        integrate_planner_delivery(&fixture, MergeMethod::CherryPick, &delivery.head_commit);
    fixture
        .coordinator
        .record_planner_merge(
            &fixture.root_thread_id,
            TaskRecordMergeInput {
                executor_agent_id: fixture.subagent.id.clone(),
                completion_revision: 1,
                expected_previous_head: run.expected_head,
                resulting_head: resulting_head.clone(),
                method: MergeMethod::CherryPick,
                summary: "record Flutter delivery".to_string(),
            },
            None,
        )
        .await
        .unwrap();

    std::fs::create_dir_all(fixture.repository.join("design")).unwrap();
    std::fs::write(
        fixture.repository.join("design/guide.md"),
        "# Completion design\n",
    )
    .unwrap();
    git(&fixture.repository, &["add", "design/guide.md"]);
    git(
        &fixture.repository,
        &["commit", "-m", "document Flutter delivery"],
    );
    let design_head = git_output(&fixture.repository, &["rev-parse", "HEAD"]);
    assert!(
        fixture
            .store
            .advance_task_design_head(&fixture.task_run_id, &resulting_head, &design_head)
            .await
            .unwrap()
    );

    let round = fixture
        .store
        .begin_integrated_review(&fixture.root_thread_id, "call-final-review")
        .await
        .unwrap();
    fixture
        .store
        .authorize_reviewer_spawn(
            &fixture.root_thread_id,
            "call-final-review",
            "agent-final-review",
        )
        .await
        .unwrap();
    fixture
        .store
        .activate_reviewer(&round.id, "agent-final-review")
        .await
        .unwrap();

    let (rejected_ends_turn, rejected_json) =
        call_task_complete(&fixture, "call-complete-before-review").await;
    assert!(!rejected_ends_turn);
    assert_eq!(rejected_json["status"], "rejected");
    assert_eq!(rejected_json["code"], "reviewMissing");
    assert!(rejected_json.get("verification").is_none());

    fixture
        .store
        .complete_task_review(
            &fixture.root_thread_id,
            "agent-final-review",
            AgentReview {
                verdict: ReviewVerdict::Pass,
                summary: "integrated review passed".to_string(),
                design_references: vec![ReviewDesignReference {
                    path: "design/guide.md".to_string(),
                    section: "Completion design".to_string(),
                }],
                findings: Vec::new(),
            },
        )
        .await
        .unwrap();

    let (ends_turn, json) = call_task_complete(&fixture, "call-complete").await;
    assert!(ends_turn);
    assert_eq!(json["status"], "completed");
    assert_eq!(json["run"]["phase"], "completed");
    assert_eq!(json["run"]["statusMessage"], serde_json::Value::Null);
    assert!(json.get("verification").is_none());
    assert!(
        fixture
            .store
            .read_branch_lease(&fixture.task_run_id)
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

#[tokio::test]
async fn merged_work_unit_is_not_downgraded_by_late_terminal_event() {
    let fixture = DeliveryFixture::new("merge-late-terminal", vec!["src"]).await;
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
            &executor_outcome(crate::TurnOutcomeKind::Failed, Some("late executor error")),
        )
        .await
        .unwrap();

    assert_eq!(fixture.work_unit().await.status, WorkUnitStatus::Merged);
    assert_eq!(
        fixture.work_unit().await.execution_status,
        ThreadExecutionStatus::Completed
    );
    fixture.cleanup();
}

#[tokio::test]
async fn wall_clock_budget_rolls_executor_into_the_next_slice() {
    let fixture = DeliveryFixture::new("budget-awaiting-completion", vec!["src"]).await;

    let continuation = fixture
        .store
        .settle_executor_turn_finished(&fixture.subagent.id, &wall_clock_outcome("turn-budget-1"))
        .await
        .unwrap()
        .expect("continuation request");

    let work_unit = fixture.work_unit().await;
    assert_eq!(work_unit.status, WorkUnitStatus::Running);
    assert_eq!(
        work_unit.execution_status,
        ThreadExecutionStatus::BudgetLimited
    );
    assert_eq!(work_unit.execution_error, None);
    assert_eq!(work_unit.budget_slice_count, 2);
    assert_eq!(
        work_unit.continuation_state,
        ExecutorContinuationState::PendingStart
    );
    assert_eq!(continuation.work_unit_id, fixture.work_unit_id);
    assert_eq!(continuation.source_turn_id, "turn-budget-1");
    assert_eq!(continuation.slice_count, 2);

    let duplicate = fixture
        .store
        .settle_executor_turn_finished(&fixture.subagent.id, &wall_clock_outcome("turn-budget-1"))
        .await
        .unwrap()
        .expect("idempotent continuation request");
    assert_eq!(duplicate, continuation);
    assert_eq!(fixture.work_unit().await.budget_slice_count, 2);
    fixture.cleanup();
}

#[tokio::test]
async fn fourth_wall_clock_slice_needs_attention_and_planner_message_starts_new_tranche() {
    let fixture = DeliveryFixture::new("budget-four-slices", vec!["src"]).await;

    for source_slice in 1..4 {
        let turn_id = format!("turn-budget-{source_slice}");
        let continuation = fixture
            .store
            .settle_executor_turn_finished(&fixture.subagent.id, &wall_clock_outcome(&turn_id))
            .await
            .unwrap()
            .expect("continuation before fourth slice");
        assert_eq!(continuation.slice_count, source_slice + 1);
        fixture
            .store
            .mark_executor_turn_started(&fixture.subagent.id)
            .await
            .unwrap();
    }

    assert!(
        fixture
            .store
            .settle_executor_turn_finished(
                &fixture.subagent.id,
                &wall_clock_outcome("turn-budget-4"),
            )
            .await
            .unwrap()
            .is_none()
    );
    let exhausted = fixture.work_unit().await;
    assert_eq!(exhausted.status, WorkUnitStatus::NeedsAttention);
    assert_eq!(exhausted.budget_slice_count, 4);
    assert_eq!(
        exhausted.continuation_state,
        ExecutorContinuationState::NeedsAttention
    );

    fixture.cleanup();
}

#[tokio::test]
async fn non_wall_clock_and_rollover_failure_do_not_auto_continue() {
    for (slug, outcome, expected_error) in [
        (
            "budget-tool-call",
            budget_outcome(
                "turn-tool-call",
                crate::BudgetLimitKind::ToolCall,
                true,
                None,
            ),
            "tool budget reached",
        ),
        (
            "budget-compaction-failed",
            budget_outcome(
                "turn-compaction-failed",
                crate::BudgetLimitKind::WallClock,
                false,
                Some("rollover compaction failed"),
            ),
            "rollover compaction failed",
        ),
    ] {
        let fixture = DeliveryFixture::new(slug, vec!["src"]).await;
        assert!(
            fixture
                .store
                .settle_executor_turn_finished(&fixture.subagent.id, &outcome)
                .await
                .unwrap()
                .is_none()
        );
        let work_unit = fixture.work_unit().await;
        assert_eq!(work_unit.status, WorkUnitStatus::NeedsAttention);
        assert_eq!(work_unit.budget_slice_count, 1);
        assert_eq!(
            work_unit.continuation_state,
            ExecutorContinuationState::NeedsAttention
        );
        assert_eq!(work_unit.execution_error.as_deref(), Some(expected_error));
        fixture.cleanup();
    }
}

#[tokio::test]
async fn planner_git_methods_are_recorded_and_cleanup_is_idempotent() {
    for method in [
        MergeMethod::Merge,
        MergeMethod::CherryPick,
        MergeMethod::Squash,
        MergeMethod::Rebase,
        MergeMethod::Manual,
    ] {
        let fixture = DeliveryFixture::new(
            &format!("record-{}", method.as_str()),
            vec!["src/hinted.rs"],
        )
        .await;
        fixture.commit_file("src/outside_hint.rs");
        let source_head = git_output(&fixture.worktree, &["rev-parse", "HEAD"]);
        let delivery = fixture.submit(&source_head).await.unwrap();
        let run = fixture
            .store
            .read_task_run(&fixture.task_run_id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(run.phase, TaskRunPhase::Merging);
        let resulting_head = integrate_planner_delivery(&fixture, method, &delivery.head_commit);
        let input = TaskRecordMergeInput {
            executor_agent_id: fixture.subagent.id.clone(),
            completion_revision: 1,
            expected_previous_head: run.expected_head.clone(),
            resulting_head: resulting_head.clone(),
            method,
            summary: format!("recorded {} integration", method.as_str()),
        };

        let record = fixture
            .coordinator
            .record_planner_merge(&fixture.root_thread_id, input.clone(), None)
            .await
            .unwrap();
        assert_eq!(record.method, method);
        assert_eq!(record.resulting_head, resulting_head);
        assert!(matches!(
            record.cleanup.status.as_str(),
            "discarded" | "alreadyAbsent"
        ));
        assert!(!fixture.worktree.exists());
        assert_eq!(fixture.work_unit().await.status, WorkUnitStatus::Merged);

        let retried = fixture
            .coordinator
            .record_planner_merge(&fixture.root_thread_id, input, None)
            .await
            .unwrap();
        assert_eq!(retried.id, record.id);
        fixture.cleanup();
    }
}

#[tokio::test]
async fn task_record_merge_rejects_stale_dirty_and_unfinished_git_state() {
    for failure in ["stale", "dirty", "unfinished"] {
        let fixture =
            DeliveryFixture::new(&format!("record-reject-{failure}"), vec!["src/delivery.rs"])
                .await;
        fixture.commit_file("src/delivery.rs");
        let source_head = git_output(&fixture.worktree, &["rev-parse", "HEAD"]);
        let delivery = fixture.submit(&source_head).await.unwrap();
        let run = fixture
            .store
            .read_task_run(&fixture.task_run_id)
            .await
            .unwrap()
            .unwrap();
        let resulting_head =
            integrate_planner_delivery(&fixture, MergeMethod::Merge, &delivery.head_commit);
        let mut input = TaskRecordMergeInput {
            executor_agent_id: fixture.subagent.id.clone(),
            completion_revision: 1,
            expected_previous_head: run.expected_head,
            resulting_head,
            method: MergeMethod::Merge,
            summary: format!("reject {failure}"),
        };
        match failure {
            "stale" => input.expected_previous_head = delivery.head_commit,
            "dirty" => {
                std::fs::write(fixture.repository.join("untracked.txt"), "dirty\n").unwrap();
            }
            "unfinished" => {
                let marker = git_output(
                    &fixture.repository,
                    &["rev-parse", "--git-path", "MERGE_HEAD"],
                );
                let marker = PathBuf::from(marker);
                let marker = if marker.is_absolute() {
                    marker
                } else {
                    fixture.repository.join(marker)
                };
                std::fs::write(marker, format!("{}\n", delivery.head_commit)).unwrap();
            }
            _ => unreachable!(),
        }

        let error = fixture
            .coordinator
            .record_planner_merge(&fixture.root_thread_id, input, None)
            .await
            .unwrap_err()
            .to_string();
        match failure {
            "stale" => assert!(error.contains("expectedPreviousHead"), "{error}"),
            "dirty" => assert!(error.contains("clean"), "{error}"),
            "unfinished" => assert!(error.contains("unfinished Git merge"), "{error}"),
            _ => unreachable!(),
        }
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
}

#[tokio::test]
async fn task_status_exposes_only_relative_worktree_locators() {
    let fixture = DeliveryFixture::new("task-status-redaction", vec!["src"]).await;
    fixture.commit_file("src/status.rs");
    let source_head = git_output(&fixture.worktree, &["rev-parse", "HEAD"]);
    fixture.submit(&source_head).await.unwrap();
    let tool = fixture
        .coordinator
        .task_status_tool(fixture.root_thread_id.clone(), None);
    let (event_tx, _) = tokio::sync::broadcast::channel(16);
    let output = tool
        .execute(
            ToolInput {
                arguments: serde_json::json!({}),
                session_id: fixture.root_thread_id.clone(),
                tool_id: "call-task-status".to_string(),
                revision_base: 0,
            },
            ToolContext {
                event_tx,
                options: TurnOptions::default(),
                workspace_access: WorkspaceAccess::WorkspaceOnly,
                workspace: pl_core::AgentWorkspace::local(fixture.repository.clone()),
                workspace_instructions: None,
                instruction_snapshot: None,
                provider_call_id: Some("call-task-status".to_string()),
                active_subagent: None,
                lsp_runtime: None,
                parent_session: Arc::new(AgentSession::from_messages(Vec::new())),
                working_set: TurnWorkingSetHandle::default(),
                tool_cache: TurnToolCacheHandle::default(),
            },
        )
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_str(&output.into_model_output()).unwrap();
    let expected_locator = format!(
        ".pure/worktrees/{}/{}",
        fixture.task_run_id, fixture.subagent.id
    );
    assert_eq!(
        json["workUnits"][0]["relativeWorktreePath"],
        expected_locator
    );
    assert_eq!(
        json["completions"][0]["relativeWorktreePath"],
        expected_locator
    );
    assert!(
        !json
            .to_string()
            .contains(&fixture.worktree.to_string_lossy().to_string())
    );
    fixture.cleanup();
}

#[tokio::test]
async fn closed_thread_projection_barrier_waits_for_durable_status() {
    let fixture = DeliveryFixture::new("closed-projection-barrier", vec!["src"]).await;
    persist_closed_executor_thread(
        &fixture.store,
        &fixture.root_thread_id,
        &fixture.subagent.id,
    )
    .await
    .unwrap();
    let updated_at = crate::studio::ids::unix_seconds() + 1;
    fixture
        .store
        .update_thread_status(&fixture.subagent.id, "idle", None, None, updated_at)
        .await
        .unwrap();
    let store = fixture.store.clone();
    let executor_id = fixture.subagent.id.clone();
    let projection = tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
        store
            .update_thread_status(&executor_id, "closed", None, None, updated_at + 1)
            .await
    });

    fixture
        .coordinator
        .await_closed_thread(&fixture.subagent.id)
        .await
        .unwrap();

    projection.await.unwrap().unwrap();
    assert_eq!(
        fixture
            .store
            .read_thread(&fixture.subagent.id)
            .await
            .unwrap()
            .unwrap()
            .status,
        "closed"
    );
    fixture.cleanup();
}

#[tokio::test]
async fn delivery_review_prompt_uses_relative_locator_without_absolute_workspace_paths() {
    let fixture = DeliveryFixture::new("review-prompt-redaction", vec!["src"]).await;
    fixture.commit_file("src/prompt.rs");
    let source_head = git_output(&fixture.worktree, &["rev-parse", "HEAD"]);
    fixture
        .store
        .create_work_completion(
            &fixture.work_unit_id,
            WorkCompletionKind::Delivery,
            Some(&AgentDelivery {
                worktree: AgentWorktreeDelivery {
                    path: std::fs::canonicalize(&fixture.worktree)
                        .unwrap()
                        .to_string_lossy()
                        .to_string(),
                    branch: fixture.branch.clone(),
                },
                base_commit: fixture.base_commit.clone(),
                head_commit: source_head,
                changed_files: vec!["src/prompt.rs".to_string()],
                verification_summary: "focused checks passed".to_string(),
            }),
            "focused checks passed",
        )
        .await
        .unwrap();
    let round = fixture
        .store
        .begin_delivery_review(
            &fixture.root_thread_id,
            &fixture.subagent.id,
            "call-review-prompt",
        )
        .await
        .unwrap();

    let prompt = super::review::prompt::build_review_prompt(&fixture.coordinator, &round)
        .await
        .unwrap();
    let expected_locator = format!(
        ".pure/worktrees/{}/{}",
        fixture.task_run_id, fixture.subagent.id
    );

    assert!(prompt.contains(&expected_locator));
    assert!(!prompt.contains(&fixture.worktree.to_string_lossy().to_string()));
    assert!(!prompt.contains(&fixture.repository.to_string_lossy().to_string()));
    fixture.cleanup();
}

struct DeliveryFixture {
    coordinator: Arc<TaskCoordinator>,
    store: StudioStore,
    root_thread_id: String,
    task_run_id: String,
    work_unit_id: String,
    repository: PathBuf,
    worktree: PathBuf,
    branch: String,
    base_commit: String,
    subagent: SubagentContext,
}

struct ReviewFixture {
    repository: PathBuf,
    store: StudioStore,
    coordinator: Arc<TaskCoordinator>,
    root_thread_id: String,
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
            root_thread_id: session.id,
            run_id: run.id,
        }
    }

    fn cleanup(self) {
        self.coordinator.suspend();
        remove_repository(self.repository);
    }
}

impl DeliveryFixture {
    async fn new(name: &str, scope_hints: Vec<&str>) -> Self {
        Self::new_configured(name, scope_hints, 1, true).await
    }

    async fn new_configured(
        name: &str,
        scope_hints: Vec<&str>,
        attempt: u32,
        _link_work_unit: bool,
    ) -> Self {
        let repository = init_repository(name);
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
        let worktree = crate::agent::worktree::git_compatible_path(
            repository
                .join(".pure/worktrees")
                .join(&run.id)
                .join("agent-1"),
        );
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
                scope_hints: scope_hints.into_iter().map(str::to_string).collect(),
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
        store
            .activate_executor(&work_unit.id, &subagent.id)
            .await
            .unwrap();
        Self {
            coordinator,
            store,
            root_thread_id: session.id,
            task_run_id,
            work_unit_id: work_unit.id,
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
        approve_delivery(&self.store, &self.root_thread_id, &work_unit, delivery).await
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

fn integrate_planner_delivery(
    fixture: &DeliveryFixture,
    method: MergeMethod,
    delivery_head: &str,
) -> String {
    match method {
        MergeMethod::Merge => git(
            &fixture.repository,
            &[
                "merge",
                "--no-ff",
                &fixture.branch,
                "-m",
                "test merge integration",
            ],
        ),
        MergeMethod::CherryPick => git(&fixture.repository, &["cherry-pick", delivery_head]),
        MergeMethod::Squash => {
            git(&fixture.repository, &["merge", "--squash", &fixture.branch]);
            git(
                &fixture.repository,
                &["commit", "-m", "test squash integration"],
            );
        }
        MergeMethod::Rebase => {
            git(&fixture.worktree, &["rebase", "main"]);
            git(
                &fixture.repository,
                &["merge", "--ff-only", &fixture.branch],
            );
        }
        MergeMethod::Manual => {
            let source = std::fs::read_to_string(fixture.worktree.join("src/outside_hint.rs"))
                .or_else(|_| std::fs::read_to_string(fixture.worktree.join("src/delivery.rs")))
                .unwrap();
            let destination = if fixture.worktree.join("src/outside_hint.rs").exists() {
                "src/outside_hint.rs"
            } else {
                "src/delivery.rs"
            };
            let path = fixture.repository.join(destination);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, source).unwrap();
            git(&fixture.repository, &["add", destination]);
            git(
                &fixture.repository,
                &["commit", "-m", "test manual integration"],
            );
        }
    }
    git_output(&fixture.repository, &["rev-parse", "HEAD"])
}

async fn call_task_complete(fixture: &DeliveryFixture, tool_id: &str) -> (bool, serde_json::Value) {
    let tool = fixture
        .coordinator
        .task_complete_tool(fixture.root_thread_id.clone());
    let (event_tx, _) = tokio::sync::broadcast::channel(16);
    let output = tool
        .execute(
            ToolInput {
                arguments: serde_json::json!({}),
                session_id: fixture.root_thread_id.clone(),
                tool_id: tool_id.to_string(),
                revision_base: 0,
            },
            ToolContext {
                event_tx,
                options: TurnOptions::default(),
                workspace_access: WorkspaceAccess::WorkspaceOnly,
                workspace: pl_core::AgentWorkspace::local(fixture.repository.clone()),
                workspace_instructions: None,
                instruction_snapshot: None,
                provider_call_id: Some(tool_id.to_string()),
                active_subagent: None,
                lsp_runtime: None,
                parent_session: Arc::new(AgentSession::from_messages(Vec::new())),
                working_set: TurnWorkingSetHandle::default(),
                tool_cache: TurnToolCacheHandle::default(),
            },
        )
        .await
        .unwrap();
    let ends_turn = output.ends_turn();
    let json = serde_json::from_str(&output.into_model_output()).unwrap();
    (ends_turn, json)
}

async fn approve_delivery(
    store: &StudioStore,
    root_thread_id: &str,
    work_unit: &WorkUnitRecord,
    delivery: AgentDelivery,
) -> anyhow::Result<AgentDelivery> {
    let completion = store
        .create_work_completion(
            &work_unit.id,
            WorkCompletionKind::Delivery,
            Some(&delivery),
            &delivery.verification_summary,
        )
        .await?;
    let requested_by_call_id = format!("review-{}", completion.id);
    let executor_thread_id = work_unit
        .executor_thread_id
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("work unit has no executor Thread"))?;
    store
        .begin_delivery_review(root_thread_id, executor_thread_id, &requested_by_call_id)
        .await?;
    let reviewer_agent_id = format!("reviewer-{}", completion.id);
    let reviewer_round = store
        .authorize_reviewer_spawn(root_thread_id, &requested_by_call_id, &reviewer_agent_id)
        .await?;
    store
        .activate_reviewer(&reviewer_round.id, &reviewer_agent_id)
        .await?;
    store
        .complete_task_review(
            root_thread_id,
            &reviewer_agent_id,
            AgentReview {
                verdict: ReviewVerdict::Pass,
                summary: "delivery review passed".to_string(),
                design_references: Vec::new(),
                findings: Vec::new(),
            },
        )
        .await?;
    store
        .settle_executor_turn_finished(
            executor_thread_id,
            &executor_outcome(crate::TurnOutcomeKind::Completed, None),
        )
        .await?;
    persist_closed_executor_thread(store, root_thread_id, executor_thread_id).await?;
    Ok(delivery)
}

fn executor_outcome(kind: crate::TurnOutcomeKind, reason: Option<&str>) -> crate::AgentTurnOutcome {
    crate::AgentTurnOutcome {
        turn_id: crate::TurnId::new(format!("turn-{kind:?}")).expect("turn id"),
        thread_id: crate::ThreadId::new("executor-thread").expect("thread id"),
        kind,
        reason: reason.map(str::to_string),
        failure: None,
        budget_limit: None,
        rollover_compacted: false,
        rollover_compaction_error: None,
        usage: Default::default(),
        finished_at: 1,
    }
}

fn wall_clock_outcome(turn_id: &str) -> crate::AgentTurnOutcome {
    budget_outcome(turn_id, crate::BudgetLimitKind::WallClock, true, None)
}

fn budget_outcome(
    turn_id: &str,
    kind: crate::BudgetLimitKind,
    rollover_compacted: bool,
    rollover_compaction_error: Option<&str>,
) -> crate::AgentTurnOutcome {
    crate::AgentTurnOutcome {
        turn_id: crate::TurnId::new(turn_id).expect("turn id"),
        thread_id: crate::ThreadId::new("executor-thread").expect("thread id"),
        kind: crate::TurnOutcomeKind::BudgetLimited,
        reason: Some(if kind == crate::BudgetLimitKind::ToolCall {
            "tool budget reached".to_string()
        } else {
            "active wall-clock budget reached".to_string()
        }),
        failure: None,
        budget_limit: Some(crate::BudgetLimitSnapshot {
            kind,
            usage: crate::BudgetUsage {
                model_steps: 3,
                tool_calls: 5,
                wait_calls: 1,
                elapsed_ms: 1_800_000,
            },
        }),
        rollover_compacted,
        rollover_compaction_error: rollover_compaction_error.map(str::to_string),
        usage: Default::default(),
        finished_at: 1,
    }
}

async fn persist_closed_executor_thread(
    store: &StudioStore,
    root_thread_id: &str,
    executor_thread_id: &str,
) -> anyhow::Result<()> {
    store
        .create_child_thread(crate::studio::ChildThreadSpec {
            id: executor_thread_id.to_string(),
            parent_thread_id: root_thread_id.to_string(),
            agent_path: executor_thread_id.to_string(),
            role: "executor".to_string(),
            title: executor_thread_id.to_string(),
        })
        .await?;
    store
        .update_thread_status(
            executor_thread_id,
            "closed",
            None,
            None,
            crate::studio::ids::unix_seconds(),
        )
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
        .create_thread(&session.project_id, "Competing task", StudioMode::Task)
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
async fn merging_restart_keeps_clean_unchanged_task_resumable() {
    let fixture = DeliveryFixture::new("merging-recovery-unchanged", vec!["src"]).await;
    fixture.commit_file("src/delivery.rs");
    let delivery_head = git_output(&fixture.worktree, &["rev-parse", "HEAD"]);
    fixture.submit(&delivery_head).await.unwrap();
    let before = fixture
        .store
        .read_task_run(&fixture.task_run_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(before.phase, TaskRunPhase::Merging);
    fixture.coordinator.suspend();

    let recovered = TaskCoordinator::new(fixture.store.clone());
    let report = recovered.recover_active_tasks().await.unwrap();

    assert_eq!(report.recovered_runs, vec![before.clone()]);
    assert!(report.issues.is_empty());
    assert_eq!(
        fixture
            .store
            .read_task_run(&before.id)
            .await
            .unwrap()
            .unwrap()
            .phase,
        TaskRunPhase::Merging
    );
    assert!(
        fixture
            .store
            .read_branch_lease(&before.id)
            .await
            .unwrap()
            .is_some()
    );
    recovered.suspend();
    fixture.cleanup();
}

#[tokio::test]
async fn changed_head_restart_retries_then_records_planner_merge() {
    let fixture = DeliveryFixture::new("merging-recovery-changed-head", vec!["src"]).await;
    fixture.commit_file("src/delivery.rs");
    let delivery_head = git_output(&fixture.worktree, &["rev-parse", "HEAD"]);
    let delivery = fixture.submit(&delivery_head).await.unwrap();
    let before = fixture
        .store
        .read_task_run(&fixture.task_run_id)
        .await
        .unwrap()
        .unwrap();
    let resulting_head =
        integrate_planner_delivery(&fixture, MergeMethod::CherryPick, &delivery.head_commit);
    fixture.coordinator.suspend();

    let recovered = TaskCoordinator::new(fixture.store.clone());
    let report = recovered.recover_active_tasks().await.unwrap();

    assert!(report.recovered_runs.is_empty());
    assert_eq!(report.issues.len(), 1);
    let issue = &report.issues[0];
    assert_eq!(issue.category, StudioRecoveryIssueCategory::Merge);
    assert_eq!(issue.action, StudioRecoveryIssueAction::Retry);
    assert!(issue.message.contains("HEAD changed"));
    let blocked = fixture
        .store
        .read_task_run(&before.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(blocked.phase, TaskRunPhase::Blocked);
    assert_eq!(blocked.terminal_generation, Some(blocked.task_generation));
    assert!(
        fixture
            .store
            .read_branch_lease(&before.id)
            .await
            .unwrap()
            .is_none()
    );

    let retried = recovered.retry_recovery_issue(issue).await.unwrap();
    assert_eq!(retried.phase, TaskRunPhase::Merging);
    assert_eq!(retried.expected_head, before.expected_head);
    assert_eq!(retried.task_generation, blocked.task_generation + 1);
    assert_eq!(retried.terminal_generation, None);
    assert!(
        fixture
            .store
            .read_branch_lease(&before.id)
            .await
            .unwrap()
            .is_some()
    );

    let record = recovered
        .record_planner_merge(
            &fixture.root_thread_id,
            TaskRecordMergeInput {
                executor_agent_id: fixture.subagent.id.clone(),
                completion_revision: 1,
                expected_previous_head: before.expected_head,
                resulting_head: resulting_head.clone(),
                method: MergeMethod::CherryPick,
                summary: "record recovered planner cherry-pick".to_string(),
            },
            None,
        )
        .await
        .unwrap();
    assert_eq!(record.resulting_head, resulting_head);
    assert_eq!(
        fixture
            .store
            .list_merge_records(&fixture.task_run_id)
            .await
            .unwrap(),
        vec![record]
    );
    recovered.suspend();
    fixture.cleanup();
}

#[tokio::test]
async fn unfinished_merge_restart_retry_preserves_git_operation_for_planner() {
    let fixture = DeliveryFixture::new("merging-recovery-unfinished", vec!["src"]).await;
    fixture.commit_file("src/delivery.rs");
    let delivery_head = git_output(&fixture.worktree, &["rev-parse", "HEAD"]);
    fixture.submit(&delivery_head).await.unwrap();
    git(
        &fixture.repository,
        &["merge", "--no-ff", "--no-commit", fixture.branch.as_str()],
    );
    let merge_head = PathBuf::from(git_output(
        &fixture.repository,
        &["rev-parse", "--git-path", "MERGE_HEAD"],
    ));
    let merge_head = if merge_head.is_absolute() {
        merge_head
    } else {
        fixture.repository.join(merge_head)
    };
    assert!(merge_head.is_file());
    assert!(!git_output(&fixture.repository, &["status", "--porcelain=v1"]).is_empty());
    fixture.coordinator.suspend();

    let recovered = TaskCoordinator::new(fixture.store.clone());
    let report = recovered.recover_active_tasks().await.unwrap();
    assert!(report.recovered_runs.is_empty());
    assert_eq!(report.issues.len(), 1);
    assert_eq!(report.issues[0].action, StudioRecoveryIssueAction::Retry);
    assert!(report.issues[0].message.contains("unfinished Git merge"));

    let retried = recovered
        .retry_recovery_issue(&report.issues[0])
        .await
        .unwrap();
    assert_eq!(retried.phase, TaskRunPhase::Merging);
    assert!(
        merge_head.is_file(),
        "retry must not abort the Planner merge"
    );
    assert!(!git_output(&fixture.repository, &["status", "--porcelain=v1"]).is_empty());

    git(&fixture.repository, &["merge", "--abort"]);
    recovered.suspend();
    fixture.cleanup();
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
        .create_thread(&missing_project.id, "Blocked", StudioMode::Task)
        .await
        .unwrap();
    let git_common_dir = std::fs::canonicalize(repository.join(".git")).unwrap();
    let blocked_run = store
        .create_task_run_with_lease(CreateTaskRun {
            root_thread_id: missing_session.id,
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
            scope_hints: vec!["code".to_string()],
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
        .activate_executor(&unit.id, "blocked-owned")
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

async fn task_session(store: &StudioStore, repository: &Path) -> crate::studio::ThreadRecord {
    let project = store.upsert_project(repository).await.unwrap();
    store
        .create_thread(&project.id, "Task", StudioMode::Task)
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
            scope_hints: vec![format!("code/{agent_id}")],
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
    store.activate_executor(&unit.id, agent_id).await.unwrap();
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

use super::*;
use crate::studio::task_coordinator::*;

fn create_input(session_id: &str, phase: TaskRunPhase) -> CreateTaskRun {
    CreateTaskRun {
        session_id: session_id.to_string(),
        phase,
        plan: "# Plan\n\nImplement it".to_string(),
        workspace_root: "C:/work/task".to_string(),
        git_common_dir: "C:/work/task/.git".to_string(),
        branch: "main".to_string(),
        head_commit: "1111111".to_string(),
    }
}

#[tokio::test]
async fn task_run_and_branch_lease_are_created_atomically() {
    let store = StudioStore::open_memory().await.unwrap();
    let project = store.upsert_project("C:/work/task").await.unwrap();
    let session = store
        .create_session(&project.id, "Task", StudioMode::Task)
        .await
        .unwrap();
    let competing_session = store
        .create_session(&project.id, "Other task", StudioMode::Task)
        .await
        .unwrap();

    let (run, lease) = store
        .create_task_run_with_lease(create_input(&session.id, TaskRunPhase::Planning))
        .await
        .unwrap();
    let error = store
        .create_task_run_with_lease(create_input(&competing_session.id, TaskRunPhase::Planning))
        .await
        .expect_err("same branch must have one lease");

    assert!(error.to_string().contains("already leased"));
    assert_eq!(run.phase, TaskRunPhase::Planning);
    assert_eq!(run.base_commit, "1111111");
    assert_eq!(run.expected_head, "1111111");
    assert_eq!(lease.task_run_id, run.id);
    assert_eq!(lease.expected_head, "1111111");
    assert_eq!(store.list_active_task_runs().await.unwrap(), vec![run]);
}

#[tokio::test]
async fn task_stop_gate_is_durable_and_keeps_lease_for_terminalization() {
    let store = StudioStore::open_memory().await.unwrap();
    let project = store.upsert_project("C:/work/task-stop").await.unwrap();
    let session = store
        .create_session(&project.id, "Task", StudioMode::Task)
        .await
        .unwrap();
    let mut input = create_input(&session.id, TaskRunPhase::Implementing);
    input.workspace_root = "C:/work/task-stop".to_string();
    input.git_common_dir = "C:/work/task-stop/.git".to_string();
    let (run, _) = store.create_task_run_with_lease(input).await.unwrap();

    let requested = store
        .request_task_stop(
            &run.id,
            &run.expected_head,
            TaskStopOrigin::UserRequest,
            &TaskStopReason::new("test stop").unwrap(),
        )
        .await
        .unwrap();
    assert!(requested.stop_requested);
    assert_eq!(requested.phase, TaskRunPhase::Implementing);
    assert!(store.read_branch_lease(&run.id).await.unwrap().is_some());
    let allocation = store
        .allocate_executor(AllocateExecutor {
            session_id: session.id.clone(),
            title: "must not start after request".to_string(),
            owned_paths: vec!["src/**".to_string()],
            agent_id: "agent-after-request".to_string(),
            owner_path: "/root".to_string(),
            requested_by_call_id: "call-after-request".to_string(),
        })
        .await;
    let error = match allocation {
        Ok(_) => panic!("stop request must reject executor allocation"),
        Err(error) => error,
    };
    assert!(error.to_string().contains("after task stop was requested"));

    let stopping = store
        .begin_task_stop(&run.id, &run.expected_head, requested.task_generation)
        .await
        .unwrap();
    assert_eq!(stopping.phase, TaskRunPhase::Stopping);
    assert!(store.read_branch_lease(&run.id).await.unwrap().is_some());

    let cancelled = store
        .cancel_task_and_release_lease(
            &run.id,
            &run.expected_head,
            requested.task_generation,
            "test stop",
        )
        .await
        .unwrap();
    assert_eq!(cancelled.phase, TaskRunPhase::Cancelled);
    assert_eq!(
        cancelled.terminal_generation,
        Some(requested.task_generation)
    );
    assert_eq!(
        store
            .cancel_task_and_release_lease(
                &run.id,
                &run.expected_head,
                requested.task_generation,
                "duplicate stop",
            )
            .await
            .unwrap()
            .terminal_generation,
        Some(requested.task_generation),
        "the same generation must reuse its one durable terminal fact"
    );
}

#[tokio::test]
async fn recovery_cleanup_records_one_terminal_generation_and_releases_the_lease() {
    let store = StudioStore::open_memory().await.unwrap();
    let project = store
        .upsert_project("C:/work/recovery-cleanup-terminal")
        .await
        .unwrap();
    let session = store
        .create_session(&project.id, "Task", StudioMode::Task)
        .await
        .unwrap();
    let mut input = create_input(&session.id, TaskRunPhase::Implementing);
    input.workspace_root = "C:/work/recovery-cleanup-terminal".to_string();
    input.git_common_dir = "C:/work/recovery-cleanup-terminal/.git".to_string();
    let (run, _) = store.create_task_run_with_lease(input).await.unwrap();

    store.authorize_recovery_cleanup(&run.id).await.unwrap();
    let cancelled = store.read_task_run(&run.id).await.unwrap().unwrap();

    assert_eq!(cancelled.phase, TaskRunPhase::Cancelled);
    assert!(cancelled.stop_requested);
    assert_eq!(
        cancelled.stop_requested_origin,
        Some(TaskStopOrigin::UserRequest)
    );
    assert_eq!(cancelled.task_generation, 1);
    assert_eq!(cancelled.terminal_generation, Some(1));
    assert!(store.read_branch_lease(&run.id).await.unwrap().is_none());
}

#[tokio::test]
async fn stopping_task_rejects_completion_without_mutating_executor_records() {
    let fixture = ExecutorFixture::new("stopping-completion").await;
    let requested = fixture
        .store
        .request_task_stop(
            &fixture.run.id,
            &fixture.run.expected_head,
            TaskStopOrigin::UserRequest,
            &TaskStopReason::new("test stop").unwrap(),
        )
        .await
        .unwrap();
    fixture
        .store
        .begin_task_stop(
            &fixture.run.id,
            &fixture.run.expected_head,
            requested.task_generation,
        )
        .await
        .unwrap();

    let error = fixture
        .store
        .create_work_completion(
            &fixture.outcome.id,
            &fixture.work_unit.id,
            WorkCompletionKind::Delivery,
            Some(&fixture.delivery("2222222")),
            "cargo test passed",
        )
        .await
        .expect_err("stopping task must reject executor completion");

    assert!(
        error
            .to_string()
            .contains("not accepting executor completion")
    );
    assert_eq!(fixture.outcome().await.status, AgentOutcomeStatus::Running);
    assert_eq!(fixture.work_unit().await.status, WorkUnitStatus::Running);
    assert!(
        fixture
            .store
            .list_work_completions(&fixture.run.id)
            .await
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn executor_discard_preserves_terminal_evidence_and_marks_cleanup() {
    for terminal_before_discard in [false, true] {
        let fixture = ExecutorFixture::new(if terminal_before_discard {
            "discard-terminal"
        } else {
            "discard-active"
        })
        .await;
        if terminal_before_discard {
            fixture
                .store
                .update_work_unit(
                    &fixture.work_unit.id,
                    WorkUnitStatus::Failed,
                    Some(fixture.agent_id.clone()),
                )
                .await
                .unwrap();
            fixture
                .store
                .update_agent_outcome(
                    &fixture.outcome.id,
                    UpdateAgentOutcome {
                        status: AgentOutcomeStatus::Failed,
                        summary: None,
                        error: Some("executor failed before discard".to_string()),
                    },
                )
                .await
                .unwrap();
        }

        fixture
            .store
            .settle_executor_close(
                &fixture.run.session_id,
                &fixture.work_unit.id,
                &fixture.agent_id,
            )
            .await
            .unwrap();

        let unit = fixture.work_unit().await;
        let outcome = fixture.outcome().await;
        assert_eq!(
            unit.worktree_disposition,
            TaskWorktreeDisposition::CleanupRequested
        );
        assert!(matches!(
            unit.status,
            WorkUnitStatus::Cancelled | WorkUnitStatus::Failed
        ));
        assert!(matches!(
            outcome.status,
            AgentOutcomeStatus::Cancelled | AgentOutcomeStatus::Failed
        ));
        assert_eq!(
            outcome.error.as_deref(),
            Some(if terminal_before_discard {
                "executor failed before discard"
            } else {
                "executor discarded by planner"
            })
        );
    }
}

#[tokio::test]
async fn executor_close_preflight_rejects_reviewing_delivery_without_side_effects() {
    let fixture = ExecutorFixture::new("close-preflight-reviewing").await;
    let completion = fixture
        .store
        .create_work_completion(
            &fixture.outcome.id,
            &fixture.work_unit.id,
            WorkCompletionKind::Delivery,
            Some(&fixture.delivery("2222222")),
            "cargo test passed",
        )
        .await
        .unwrap();

    let error = fixture
        .store
        .preflight_executor_close(
            &fixture.run.session_id,
            &fixture.work_unit.id,
            &fixture.agent_id,
        )
        .await
        .expect_err("ready-for-review executor must not begin closing");

    assert!(error.to_string().contains("cannot close"));
    assert_eq!(
        fixture.work_unit().await.status,
        WorkUnitStatus::ReadyForReview
    );
    assert_eq!(
        fixture.work_unit().await.worktree_disposition,
        TaskWorktreeDisposition::Protect
    );
    assert_eq!(
        fixture.outcome().await.status,
        AgentOutcomeStatus::Completed
    );
    assert_eq!(
        fixture
            .store
            .list_work_completions(&fixture.run.id)
            .await
            .unwrap(),
        vec![completion]
    );
}

#[tokio::test]
async fn approved_executor_close_preserves_resources_until_merge() {
    let fixture = ExecutorFixture::new("close-approved").await;
    fixture
        .store
        .create_work_completion(
            &fixture.outcome.id,
            &fixture.work_unit.id,
            WorkCompletionKind::Delivery,
            Some(&fixture.delivery("2222222")),
            "cargo test passed",
        )
        .await
        .unwrap();
    fixture
        .finish_delivery_review(
            "reviewer-approved-close",
            "review-call-approved-close",
            AgentReview {
                verdict: ReviewVerdict::Pass,
                summary: "delivery is ready".to_string(),
                design_references: Vec::new(),
                findings: Vec::new(),
            },
        )
        .await;

    assert_eq!(
        fixture
            .store
            .preflight_executor_close(
                &fixture.run.session_id,
                &fixture.work_unit.id,
                &fixture.agent_id,
            )
            .await
            .unwrap(),
        ExecutorCloseDisposition::PreserveForMerge
    );
    assert_eq!(
        fixture
            .store
            .settle_executor_close(
                &fixture.run.session_id,
                &fixture.work_unit.id,
                &fixture.agent_id,
            )
            .await
            .unwrap(),
        ExecutorCloseDisposition::PreserveForMerge
    );
    assert_eq!(fixture.work_unit().await.status, WorkUnitStatus::Approved);
    assert_eq!(
        fixture.work_unit().await.worktree_disposition,
        TaskWorktreeDisposition::Protect
    );
    assert_eq!(
        fixture.outcome().await.status,
        AgentOutcomeStatus::Completed
    );
}

#[tokio::test]
async fn executor_message_admission_reopens_only_after_review_findings() {
    let fixture = ExecutorFixture::new("message-admission").await;
    fixture
        .store
        .create_work_completion(
            &fixture.outcome.id,
            &fixture.work_unit.id,
            WorkCompletionKind::Delivery,
            Some(&fixture.delivery("2222222")),
            "cargo test passed",
        )
        .await
        .unwrap();

    let error = fixture
        .store
        .authorize_executor_message(&fixture.run.session_id, &fixture.agent_id)
        .await
        .expect_err("ready-for-review executor must not receive another prompt");
    assert!(error.to_string().contains("readyForReview"));
    let error = fixture
        .store
        .mark_executor_turn_started(&fixture.agent_id)
        .await
        .expect_err("turn preparation must independently reject ready-for-review executors");
    assert!(error.to_string().contains("readyForReview"));

    fixture
        .finish_delivery_review(
            "reviewer-message-admission",
            "review-call-message-admission",
            AgentReview {
                verdict: ReviewVerdict::ChangesRequired,
                summary: "one correction is required".to_string(),
                design_references: Vec::new(),
                findings: vec![ReviewFinding {
                    severity: "major".to_string(),
                    title: "fix behavior".to_string(),
                    body: "apply the reviewed correction".to_string(),
                    path: Some("src/lib.rs".to_string()),
                    line: Some(1),
                    design_references: Vec::new(),
                }],
            },
        )
        .await;

    fixture
        .store
        .authorize_executor_message(&fixture.run.session_id, &fixture.agent_id)
        .await
        .unwrap();
}

#[tokio::test]
async fn task_stop_settlement_cancels_unmerged_completion_and_authorizes_cleanup() {
    let fixture = ExecutorFixture::new("stop-ready-for-review").await;
    let completion = fixture
        .store
        .create_work_completion(
            &fixture.outcome.id,
            &fixture.work_unit.id,
            WorkCompletionKind::Delivery,
            Some(&fixture.delivery("2222222")),
            "cargo test passed",
        )
        .await
        .unwrap();
    let requested = fixture
        .store
        .request_task_stop(
            &fixture.run.id,
            &fixture.run.expected_head,
            TaskStopOrigin::PlannerDecision,
            &TaskStopReason::new("review infrastructure failed").unwrap(),
        )
        .await
        .unwrap();
    fixture
        .store
        .begin_task_stop(
            &fixture.run.id,
            &fixture.run.expected_head,
            requested.task_generation,
        )
        .await
        .unwrap();

    fixture
        .store
        .settle_agents_for_task_stop(
            &fixture.run.id,
            requested.task_generation,
            "review infrastructure failed",
        )
        .await
        .unwrap();

    assert_eq!(fixture.work_unit().await.status, WorkUnitStatus::Cancelled);
    assert_eq!(
        fixture.work_unit().await.worktree_disposition,
        TaskWorktreeDisposition::CleanupRequested
    );
    assert_eq!(
        fixture.outcome().await.status,
        AgentOutcomeStatus::Cancelled
    );
    assert_eq!(
        fixture
            .store
            .list_work_completions(&fixture.run.id)
            .await
            .unwrap(),
        vec![completion],
        "immutable completion evidence must remain available"
    );
}

#[tokio::test]
async fn completion_review_rework_loop_keeps_every_revision_immutable() {
    let fixture = ExecutorFixture::new("review-loop").await;

    for revision in 1..=4 {
        let head = format!("{revision}222222");
        let completion = fixture
            .store
            .create_work_completion(
                &fixture.outcome.id,
                &fixture.work_unit.id,
                WorkCompletionKind::Delivery,
                Some(&fixture.delivery(&head)),
                &format!("verification revision {revision}"),
            )
            .await
            .unwrap();
        assert_eq!(completion.revision, revision);
        assert_eq!(completion.status, WorkCompletionStatus::ReadyForReview);

        let finding = ReviewFinding {
            severity: "major".to_string(),
            title: format!("revision {revision} needs work"),
            body: "apply the requested correction".to_string(),
            path: Some("src/lib.rs".to_string()),
            line: Some(revision),
            design_references: Vec::new(),
        };
        fixture
            .finish_delivery_review(
                &format!("reviewer-{revision}"),
                &format!("review-call-{revision}"),
                AgentReview {
                    verdict: ReviewVerdict::ChangesRequired,
                    summary: format!("revision {revision} has a finding"),
                    design_references: Vec::new(),
                    findings: vec![finding],
                },
            )
            .await;

        assert_eq!(
            fixture.work_unit().await.status,
            WorkUnitStatus::ChangesRequested
        );
        fixture
            .store
            .mark_executor_turn_started(&fixture.agent_id)
            .await
            .unwrap();
        assert_eq!(fixture.outcome().await.status, AgentOutcomeStatus::Running);
    }

    let final_completion = fixture
        .store
        .create_work_completion(
            &fixture.outcome.id,
            &fixture.work_unit.id,
            WorkCompletionKind::Delivery,
            Some(&fixture.delivery("9222222")),
            "final verification passed",
        )
        .await
        .unwrap();
    assert_eq!(final_completion.revision, 5);
    fixture
        .finish_delivery_review(
            "reviewer-pass",
            "review-call-pass",
            AgentReview {
                verdict: ReviewVerdict::Pass,
                summary: "delivery is ready".to_string(),
                design_references: Vec::new(),
                findings: Vec::new(),
            },
        )
        .await;

    let completions = fixture
        .store
        .list_work_completions(&fixture.run.id)
        .await
        .unwrap();
    assert_eq!(completions.len(), 5);
    assert!(
        completions[..4]
            .iter()
            .all(|completion| completion.status == WorkCompletionStatus::ChangesRequired)
    );
    assert_eq!(completions[4].status, WorkCompletionStatus::Approved);
    assert_eq!(fixture.work_unit().await.status, WorkUnitStatus::Approved);
}

#[tokio::test]
async fn no_delivery_requires_review_and_stale_integrated_head_is_rejected() {
    let fixture = ExecutorFixture::new("no-delivery").await;
    let completion = fixture
        .store
        .create_work_completion(
            &fixture.outcome.id,
            &fixture.work_unit.id,
            WorkCompletionKind::NoDelivery,
            None,
            "inspection proved no source change was needed",
        )
        .await
        .unwrap();
    assert_eq!(completion.status, WorkCompletionStatus::ReadyForReview);
    assert_eq!(
        fixture.work_unit().await.status,
        WorkUnitStatus::ReadyForReview
    );

    fixture
        .finish_delivery_review(
            "reviewer-no-delivery",
            "review-call-no-delivery",
            AgentReview {
                verdict: ReviewVerdict::Pass,
                summary: "no-delivery result is valid".to_string(),
                design_references: Vec::new(),
                findings: Vec::new(),
            },
        )
        .await;
    assert_eq!(fixture.work_unit().await.status, WorkUnitStatus::NoDelivery);

    let round = fixture
        .store
        .begin_integrated_review(&fixture.run.session_id, "integrated-call")
        .await
        .unwrap();
    assert_eq!(round.scope, ReviewScope::Integrated);
    let (_, reviewer) = fixture
        .store
        .authorize_reviewer_spawn(
            &fixture.run.session_id,
            "integrated-call",
            "integrated-reviewer",
        )
        .await
        .unwrap();
    fixture
        .store
        .update_agent_outcome(
            &reviewer.id,
            UpdateAgentOutcome {
                status: AgentOutcomeStatus::Running,
                summary: None,
                error: None,
            },
        )
        .await
        .unwrap();
    assert!(
        fixture
            .store
            .compare_and_set_task_head(&fixture.run.id, "1111111", "3333333")
            .await
            .unwrap()
    );

    let error = fixture
        .store
        .complete_task_review(
            &fixture.run.session_id,
            "integrated-reviewer",
            AgentReview {
                verdict: ReviewVerdict::Pass,
                summary: "stale review must not pass".to_string(),
                design_references: Vec::new(),
                findings: Vec::new(),
            },
        )
        .await
        .expect_err("integrated review must be bound to its exact Task HEAD");
    assert!(
        error
            .to_string()
            .contains("no longer matches current Task HEAD")
    );
    let reviews = fixture
        .store
        .list_review_rounds(&fixture.run.id)
        .await
        .unwrap();
    assert_eq!(reviews.len(), 2);
    assert_eq!(reviews[1].verdict, ReviewVerdict::Pending);
}

#[tokio::test]
async fn delivery_review_rejects_a_stale_completion_revision() {
    let fixture = ExecutorFixture::new("stale-delivery-review").await;
    let completion = fixture
        .store
        .create_work_completion(
            &fixture.outcome.id,
            &fixture.work_unit.id,
            WorkCompletionKind::Delivery,
            Some(&fixture.delivery("2222222")),
            "verification passed",
        )
        .await
        .unwrap();
    let round = fixture
        .store
        .begin_delivery_review(
            &fixture.run.session_id,
            &fixture.agent_id,
            "stale-review-call",
        )
        .await
        .unwrap();
    let (_, reviewer) = fixture
        .store
        .authorize_reviewer_spawn(
            &fixture.run.session_id,
            "stale-review-call",
            "stale-reviewer",
        )
        .await
        .unwrap();
    fixture
        .store
        .update_agent_outcome(
            &reviewer.id,
            UpdateAgentOutcome {
                status: AgentOutcomeStatus::Running,
                summary: None,
                error: None,
            },
        )
        .await
        .unwrap();

    fixture
        .store
        .execute_test_sql(&format!(
            "UPDATE work_completions SET revision = 99 WHERE id = '{}'",
            completion.id
        ))
        .await;

    let error = fixture
        .store
        .complete_task_review(
            &fixture.run.session_id,
            "stale-reviewer",
            AgentReview {
                verdict: ReviewVerdict::Pass,
                summary: "must not approve a stale completion".to_string(),
                design_references: Vec::new(),
                findings: Vec::new(),
            },
        )
        .await
        .expect_err("review must stay bound to the completion revision it was created for");
    assert!(
        error
            .to_string()
            .contains("delivery review target changed after reviewer creation")
    );

    let stored_round = fixture
        .store
        .list_review_rounds(&fixture.run.id)
        .await
        .unwrap()
        .into_iter()
        .find(|candidate| candidate.id == round.id)
        .unwrap();
    assert_eq!(stored_round.verdict, ReviewVerdict::Pending);
    assert_eq!(fixture.work_unit().await.status, WorkUnitStatus::Reviewing);
    let reviewer_outcome = fixture
        .store
        .list_agent_outcomes(&fixture.run.id)
        .await
        .unwrap()
        .into_iter()
        .find(|outcome| outcome.agent_id == "stale-reviewer")
        .unwrap();
    assert_eq!(reviewer_outcome.status, AgentOutcomeStatus::Running);
}

#[tokio::test]
async fn duplicate_reviewer_terminal_settlement_preserves_the_first_failure() {
    let fixture = ExecutorFixture::new("duplicate-reviewer-terminal").await;
    fixture
        .store
        .create_work_completion(
            &fixture.outcome.id,
            &fixture.work_unit.id,
            WorkCompletionKind::Delivery,
            Some(&fixture.delivery("2222222")),
            "verification passed",
        )
        .await
        .unwrap();
    let round = fixture
        .store
        .begin_delivery_review(
            &fixture.run.session_id,
            &fixture.agent_id,
            "terminal-review-call",
        )
        .await
        .unwrap();
    let (_, reviewer) = fixture
        .store
        .authorize_reviewer_spawn(
            &fixture.run.session_id,
            "terminal-review-call",
            "terminal-reviewer",
        )
        .await
        .unwrap();
    fixture
        .store
        .update_agent_outcome(
            &reviewer.id,
            UpdateAgentOutcome {
                status: AgentOutcomeStatus::Running,
                summary: None,
                error: None,
            },
        )
        .await
        .unwrap();

    fixture
        .store
        .settle_reviewer_turn_finished(
            "terminal-reviewer",
            crate::TurnOutcomeKind::Failed,
            Some("first reviewer failure"),
        )
        .await
        .unwrap();
    fixture
        .store
        .settle_reviewer_turn_finished(
            "terminal-reviewer",
            crate::TurnOutcomeKind::Cancelled,
            Some("late duplicate terminal event"),
        )
        .await
        .unwrap();

    let stored_round = fixture
        .store
        .list_review_rounds(&fixture.run.id)
        .await
        .unwrap()
        .into_iter()
        .find(|candidate| candidate.id == round.id)
        .unwrap();
    assert_eq!(stored_round.verdict, ReviewVerdict::Failed);
    assert_eq!(
        stored_round.summary.as_deref(),
        Some("first reviewer failure")
    );
    let reviewer_outcome = fixture
        .store
        .list_agent_outcomes(&fixture.run.id)
        .await
        .unwrap()
        .into_iter()
        .find(|outcome| outcome.agent_id == "terminal-reviewer")
        .unwrap();
    assert_eq!(reviewer_outcome.status, AgentOutcomeStatus::Failed);
    assert_eq!(
        reviewer_outcome.error.as_deref(),
        Some("first reviewer failure")
    );
    assert_eq!(
        fixture.work_unit().await.status,
        WorkUnitStatus::ReadyForReview
    );
}

#[tokio::test]
async fn failed_delivery_reviewer_allows_a_fresh_round_for_the_same_completion() {
    let fixture = ExecutorFixture::new("delivery-review-retry").await;
    let completion = fixture
        .store
        .create_work_completion(
            &fixture.outcome.id,
            &fixture.work_unit.id,
            WorkCompletionKind::Delivery,
            Some(&fixture.delivery("2222222")),
            "verification passed",
        )
        .await
        .unwrap();
    let first_round = fixture
        .store
        .begin_delivery_review(
            &fixture.run.session_id,
            &fixture.agent_id,
            "first-review-call",
        )
        .await
        .unwrap();
    let (_, first_reviewer) = fixture
        .store
        .authorize_reviewer_spawn(
            &fixture.run.session_id,
            "first-review-call",
            "first-reviewer",
        )
        .await
        .unwrap();
    fixture
        .store
        .update_agent_outcome(
            &first_reviewer.id,
            UpdateAgentOutcome {
                status: AgentOutcomeStatus::Running,
                summary: None,
                error: None,
            },
        )
        .await
        .unwrap();
    fixture
        .store
        .settle_reviewer_turn_finished(
            "first-reviewer",
            crate::TurnOutcomeKind::Failed,
            Some("review_exit validation failed"),
        )
        .await
        .unwrap();

    let replay_error = fixture
        .store
        .begin_delivery_review(
            &fixture.run.session_id,
            &fixture.agent_id,
            "first-review-call",
        )
        .await
        .expect_err("one provider call must not authorize two review rounds");
    assert!(
        replay_error
            .to_string()
            .contains("provider call already authorized a review")
    );

    let retry_round = fixture
        .store
        .begin_delivery_review(
            &fixture.run.session_id,
            &fixture.agent_id,
            "retry-review-call",
        )
        .await
        .unwrap();
    assert_eq!(retry_round.round, first_round.round + 1);
    assert_eq!(
        retry_round.completion_id.as_deref(),
        Some(completion.id.as_str())
    );
    assert_eq!(retry_round.completion_revision, Some(completion.revision));
    assert_eq!(retry_round.reviewed_head, first_round.reviewed_head);

    let (_, retry_reviewer) = fixture
        .store
        .authorize_reviewer_spawn(
            &fixture.run.session_id,
            "retry-review-call",
            "retry-reviewer",
        )
        .await
        .unwrap();
    fixture
        .store
        .update_agent_outcome(
            &retry_reviewer.id,
            UpdateAgentOutcome {
                status: AgentOutcomeStatus::Running,
                summary: None,
                error: None,
            },
        )
        .await
        .unwrap();
    let completed_round = fixture
        .store
        .complete_task_review(
            &fixture.run.session_id,
            "retry-reviewer",
            AgentReview {
                verdict: ReviewVerdict::Pass,
                summary: "fresh review passed".to_string(),
                design_references: Vec::new(),
                findings: Vec::new(),
            },
        )
        .await
        .unwrap();

    assert_eq!(completed_round.id, retry_round.id);
    assert_eq!(completed_round.verdict, ReviewVerdict::Pass);
    assert_eq!(fixture.work_unit().await.status, WorkUnitStatus::Approved);
    let stored_completion = fixture
        .store
        .list_work_completions(&fixture.run.id)
        .await
        .unwrap()
        .into_iter()
        .find(|candidate| candidate.id == completion.id)
        .unwrap();
    assert_eq!(stored_completion.status, WorkCompletionStatus::Approved);
    let rounds = fixture
        .store
        .list_review_rounds(&fixture.run.id)
        .await
        .unwrap();
    assert_eq!(rounds.len(), 2);
    assert_eq!(rounds[0].verdict, ReviewVerdict::Failed);
    assert_eq!(rounds[1].verdict, ReviewVerdict::Pass);
}

#[tokio::test]
async fn restart_reconciliation_pauses_explicit_completion_states_without_starting_models() {
    let store = StudioStore::open_memory().await.unwrap();
    let project = store.upsert_project("C:/work/restart").await.unwrap();
    let session = store
        .create_session(&project.id, "Task", StudioMode::Task)
        .await
        .unwrap();
    let mut input = create_input(&session.id, TaskRunPhase::Implementing);
    input.workspace_root = "C:/work/restart".to_string();
    input.git_common_dir = "C:/work/restart/.git".to_string();
    let (run, _) = store.create_task_run_with_lease(input).await.unwrap();

    create_recovery_executor(
        &store,
        &run,
        "pending",
        WorkUnitStatus::Pending,
        AgentOutcomeStatus::Queued,
    )
    .await;
    create_recovery_executor(
        &store,
        &run,
        "running",
        WorkUnitStatus::Running,
        AgentOutcomeStatus::Running,
    )
    .await;
    create_recovery_executor(
        &store,
        &run,
        "awaiting-failed",
        WorkUnitStatus::AwaitingCompletion,
        AgentOutcomeStatus::Failed,
    )
    .await;
    create_recovery_executor(
        &store,
        &run,
        "ready",
        WorkUnitStatus::ReadyForReview,
        AgentOutcomeStatus::Completed,
    )
    .await;
    create_recovery_executor(
        &store,
        &run,
        "changes",
        WorkUnitStatus::ChangesRequested,
        AgentOutcomeStatus::Completed,
    )
    .await;
    create_recovery_executor(
        &store,
        &run,
        "no-delivery",
        WorkUnitStatus::NoDelivery,
        AgentOutcomeStatus::Completed,
    )
    .await;

    let first = store
        .reconcile_task_agents_after_restart(&run.id)
        .await
        .unwrap();
    let second = store
        .reconcile_task_agents_after_restart(&run.id)
        .await
        .unwrap();
    let units = store.list_work_units(&run.id).await.unwrap();
    let outcomes = store.list_agent_outcomes(&run.id).await.unwrap();

    assert_eq!(first.cancelled_work_units, 1);
    assert_eq!(first.cancelled_outcomes, 2);
    assert_eq!(second.cancelled_work_units, 0);
    assert_eq!(second.cancelled_outcomes, 0);
    assert_eq!(units[0].status, WorkUnitStatus::Cancelled);
    assert_eq!(units[1].status, WorkUnitStatus::AwaitingCompletion);
    assert_eq!(units[2].status, WorkUnitStatus::AwaitingCompletion);
    assert_eq!(units[3].status, WorkUnitStatus::ReadyForReview);
    assert_eq!(units[4].status, WorkUnitStatus::ChangesRequested);
    assert_eq!(units[5].status, WorkUnitStatus::NoDelivery);
    assert_eq!(outcomes[0].status, AgentOutcomeStatus::Cancelled);
    assert_eq!(outcomes[1].status, AgentOutcomeStatus::Cancelled);
    assert_eq!(outcomes[2].status, AgentOutcomeStatus::Failed);
    assert!(
        outcomes[3..]
            .iter()
            .all(|outcome| outcome.status == AgentOutcomeStatus::Completed)
    );
}

#[tokio::test]
async fn restart_reconciliation_fails_delivery_reviewer_and_reopens_exact_completion() {
    let fixture = ExecutorFixture::new("restart-delivery-review").await;
    let completion = fixture
        .store
        .create_work_completion(
            &fixture.outcome.id,
            &fixture.work_unit.id,
            WorkCompletionKind::Delivery,
            Some(&fixture.delivery("2222222")),
            "cargo test passed",
        )
        .await
        .unwrap();
    let round = fixture
        .store
        .begin_delivery_review(
            &fixture.run.session_id,
            &fixture.agent_id,
            "restart-delivery-review-call",
        )
        .await
        .unwrap();
    let (_, reviewer) = fixture
        .store
        .authorize_reviewer_spawn(
            &fixture.run.session_id,
            "restart-delivery-review-call",
            "restart-delivery-reviewer",
        )
        .await
        .unwrap();
    fixture
        .store
        .update_agent_outcome(
            &reviewer.id,
            UpdateAgentOutcome {
                status: AgentOutcomeStatus::Running,
                summary: None,
                error: None,
            },
        )
        .await
        .unwrap();

    let first = fixture
        .store
        .reconcile_task_agents_after_restart(&fixture.run.id)
        .await
        .unwrap();
    let second = fixture
        .store
        .reconcile_task_agents_after_restart(&fixture.run.id)
        .await
        .unwrap();

    assert_eq!(first.cancelled_outcomes, 1);
    assert_eq!(second.cancelled_outcomes, 0);
    assert_eq!(
        fixture.work_unit().await.status,
        WorkUnitStatus::ReadyForReview
    );
    let stored_completion = fixture
        .store
        .list_work_completions(&fixture.run.id)
        .await
        .unwrap()
        .into_iter()
        .find(|candidate| candidate.id == completion.id)
        .unwrap();
    assert_eq!(
        stored_completion.status,
        WorkCompletionStatus::ReadyForReview
    );
    let stored_round = fixture
        .store
        .list_review_rounds(&fixture.run.id)
        .await
        .unwrap()
        .into_iter()
        .find(|candidate| candidate.id == round.id)
        .unwrap();
    assert_eq!(stored_round.verdict, ReviewVerdict::Failed);
    assert_eq!(
        stored_round.summary.as_deref(),
        Some("reviewer interrupted by application restart before review_exit")
    );
    let reviewer = fixture
        .store
        .list_agent_outcomes(&fixture.run.id)
        .await
        .unwrap()
        .into_iter()
        .find(|outcome| outcome.agent_id == "restart-delivery-reviewer")
        .unwrap();
    assert_eq!(reviewer.status, AgentOutcomeStatus::Cancelled);
}

#[tokio::test]
async fn restart_reconciliation_fails_integrated_reviewer_and_enters_reworking() {
    let fixture = ExecutorFixture::new("restart-integrated-review").await;
    fixture
        .store
        .create_work_completion(
            &fixture.outcome.id,
            &fixture.work_unit.id,
            WorkCompletionKind::NoDelivery,
            None,
            "no changes required",
        )
        .await
        .unwrap();
    fixture
        .finish_delivery_review(
            "no-delivery-reviewer",
            "no-delivery-review-call",
            AgentReview {
                verdict: ReviewVerdict::Pass,
                summary: "no delivery is correct".to_string(),
                design_references: vec![ReviewDesignReference {
                    path: "design/16-task-orchestration.md".to_string(),
                    section: "Executor 完成与交付审查".to_string(),
                }],
                findings: Vec::new(),
            },
        )
        .await;
    let round = fixture
        .store
        .begin_integrated_review(&fixture.run.session_id, "restart-integrated-review-call")
        .await
        .unwrap();
    let (_, reviewer) = fixture
        .store
        .authorize_reviewer_spawn(
            &fixture.run.session_id,
            "restart-integrated-review-call",
            "restart-integrated-reviewer",
        )
        .await
        .unwrap();
    fixture
        .store
        .update_agent_outcome(
            &reviewer.id,
            UpdateAgentOutcome {
                status: AgentOutcomeStatus::Running,
                summary: None,
                error: None,
            },
        )
        .await
        .unwrap();

    fixture
        .store
        .reconcile_task_agents_after_restart(&fixture.run.id)
        .await
        .unwrap();
    fixture
        .store
        .reconcile_task_agents_after_restart(&fixture.run.id)
        .await
        .unwrap();

    let run = fixture
        .store
        .read_task_run(&fixture.run.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(run.phase, TaskRunPhase::Reworking);
    let stored_round = fixture
        .store
        .list_review_rounds(&fixture.run.id)
        .await
        .unwrap()
        .into_iter()
        .find(|candidate| candidate.id == round.id)
        .unwrap();
    assert_eq!(stored_round.verdict, ReviewVerdict::Failed);
    let reviewer = fixture
        .store
        .list_agent_outcomes(&fixture.run.id)
        .await
        .unwrap()
        .into_iter()
        .find(|outcome| outcome.agent_id == "restart-integrated-reviewer")
        .unwrap();
    assert_eq!(reviewer.status, AgentOutcomeStatus::Cancelled);
}

#[tokio::test]
async fn task_phase_and_expected_head_updates_are_guarded() {
    let store = StudioStore::open_memory().await.unwrap();
    let project = store.upsert_project("C:/work/task").await.unwrap();
    let session = store
        .create_session(&project.id, "Task", StudioMode::Task)
        .await
        .unwrap();
    let (run, _) = store
        .create_task_run_with_lease(create_input(&session.id, TaskRunPhase::Planning))
        .await
        .unwrap();

    let pending = store
        .transition_task_run(&run.id, TaskRunPhase::PendingConfirmation, None)
        .await
        .unwrap();
    let invalid = store
        .transition_task_run(&run.id, TaskRunPhase::Reviewing, None)
        .await
        .expect_err("invalid phase jump must fail");
    assert!(
        invalid
            .to_string()
            .contains("invalid task phase transition")
    );
    assert_eq!(pending.phase, TaskRunPhase::PendingConfirmation);
    assert!(
        store
            .compare_and_set_task_head(&run.id, "1111111", "2222222")
            .await
            .unwrap()
    );
    assert!(
        !store
            .compare_and_set_task_head(&run.id, "1111111", "3333333")
            .await
            .unwrap()
    );
    assert_eq!(
        store
            .read_branch_lease(&run.id)
            .await
            .unwrap()
            .unwrap()
            .expected_head,
        "2222222"
    );
}

#[tokio::test]
async fn concurrent_task_phase_transition_rejects_the_stale_writer() {
    let store = StudioStore::open_memory().await.unwrap();
    let project = store.upsert_project("C:/work/task-cas").await.unwrap();
    let session = store
        .create_session(&project.id, "Task", StudioMode::Task)
        .await
        .unwrap();
    let (run, _) = store
        .create_task_run_with_lease(create_input(&session.id, TaskRunPhase::Planning))
        .await
        .unwrap();
    let read_barrier = tokio::sync::Barrier::new(2);

    let (first, second) = tokio::join!(
        store.transition_task_run_after_read(
            &run.id,
            TaskRunPhase::PendingConfirmation,
            None,
            Some(&read_barrier),
        ),
        store.transition_task_run_after_read(
            &run.id,
            TaskRunPhase::PendingConfirmation,
            None,
            Some(&read_barrier),
        ),
    );

    assert_eq!(usize::from(first.is_ok()) + usize::from(second.is_ok()), 1);
    let conflict = first.err().or_else(|| second.err()).unwrap();
    assert!(
        conflict
            .to_string()
            .contains("task phase changed concurrently")
    );
    assert_eq!(
        store.read_task_run(&run.id).await.unwrap().unwrap().phase,
        TaskRunPhase::PendingConfirmation
    );
}

struct ExecutorFixture {
    store: StudioStore,
    run: TaskRunRecord,
    work_unit: WorkUnitRecord,
    outcome: AgentOutcomeRecord,
    agent_id: String,
}

impl ExecutorFixture {
    async fn new(name: &str) -> Self {
        let store = StudioStore::open_memory().await.unwrap();
        let project = store
            .upsert_project(&format!("C:/work/{name}"))
            .await
            .unwrap();
        let session = store
            .create_session(&project.id, "Task", StudioMode::Task)
            .await
            .unwrap();
        let mut input = create_input(&session.id, TaskRunPhase::Implementing);
        input.workspace_root = format!("C:/work/{name}");
        input.git_common_dir = format!("C:/work/{name}/.git");
        let (run, _) = store.create_task_run_with_lease(input).await.unwrap();
        let agent_id = format!("agent-{name}");
        let work_unit = store
            .create_work_unit(CreateWorkUnit {
                task_run_id: run.id.clone(),
                title: "Implement core".to_string(),
                owned_paths: vec!["src/**".to_string()],
                base_commit: run.base_commit.clone(),
                worktree_path: format!("C:/work/{name}/.pure/worktrees/{}", run.id),
                branch: format!("pure-task-{}-{name}", run.id),
                attempt: 1,
            })
            .await
            .unwrap();
        let work_unit = store
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
                requested_by_call_id: "spawn-call".to_string(),
                role: "executor".to_string(),
                status: AgentOutcomeStatus::Running,
                attempt: 1,
            })
            .await
            .unwrap();
        Self {
            store,
            run,
            work_unit,
            outcome,
            agent_id,
        }
    }

    fn delivery(&self, head_commit: &str) -> AgentDelivery {
        AgentDelivery {
            worktree: AgentWorktreeDelivery {
                path: self.work_unit.worktree_path.clone(),
                branch: self.work_unit.branch.clone(),
            },
            base_commit: self.work_unit.base_commit.clone(),
            head_commit: head_commit.to_string(),
            changed_files: vec!["src/lib.rs".to_string()],
            verification_summary: "cargo test passed".to_string(),
        }
    }

    async fn finish_delivery_review(
        &self,
        reviewer_agent_id: &str,
        requested_by_call_id: &str,
        review: AgentReview,
    ) -> ReviewRoundRecord {
        let round = self
            .store
            .begin_delivery_review(&self.run.session_id, &self.agent_id, requested_by_call_id)
            .await
            .unwrap();
        assert_eq!(round.scope, ReviewScope::Delivery);
        let (_, reviewer) = self
            .store
            .authorize_reviewer_spawn(
                &self.run.session_id,
                requested_by_call_id,
                reviewer_agent_id,
            )
            .await
            .unwrap();
        self.store
            .update_agent_outcome(
                &reviewer.id,
                UpdateAgentOutcome {
                    status: AgentOutcomeStatus::Running,
                    summary: None,
                    error: None,
                },
            )
            .await
            .unwrap();
        self.store
            .complete_task_review(&self.run.session_id, reviewer_agent_id, review)
            .await
            .unwrap()
    }

    async fn work_unit(&self) -> WorkUnitRecord {
        self.store
            .read_work_unit(&self.work_unit.id)
            .await
            .unwrap()
            .unwrap()
    }

    async fn outcome(&self) -> AgentOutcomeRecord {
        self.store
            .list_agent_outcomes(&self.run.id)
            .await
            .unwrap()
            .into_iter()
            .find(|outcome| outcome.id == self.outcome.id)
            .unwrap()
    }
}

async fn create_recovery_executor(
    store: &StudioStore,
    run: &TaskRunRecord,
    name: &str,
    unit_status: WorkUnitStatus,
    outcome_status: AgentOutcomeStatus,
) {
    let agent_id = format!("agent-{name}");
    let unit = store
        .create_work_unit(CreateWorkUnit {
            task_run_id: run.id.clone(),
            title: name.to_string(),
            owned_paths: vec![format!("code/{name}/**")],
            base_commit: run.base_commit.clone(),
            worktree_path: format!("C:/work/restart/.pure/worktrees/run/{name}"),
            branch: format!("pure-task-run-{name}"),
            attempt: 1,
        })
        .await
        .unwrap();
    store
        .update_work_unit(&unit.id, unit_status, Some(agent_id.clone()))
        .await
        .unwrap();
    store
        .create_agent_outcome(CreateAgentOutcome {
            task_run_id: run.id.clone(),
            work_unit_id: Some(unit.id),
            agent_id,
            owner_path: "/root".to_string(),
            initiated_by: "planner".to_string(),
            requested_by_call_id: format!("call-{name}"),
            role: "executor".to_string(),
            status: outcome_status,
            attempt: 1,
        })
        .await
        .unwrap();
}

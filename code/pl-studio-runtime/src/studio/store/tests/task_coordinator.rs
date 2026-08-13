use super::*;
use crate::studio::task_coordinator::*;

fn create_input(root_thread_id: &str, phase: TaskRunPhase) -> CreateTaskRun {
    CreateTaskRun {
        root_thread_id: root_thread_id.to_string(),
        phase,
        plan: "# Plan\n\nImplement it".to_string(),
        workspace_root: "C:/work/task".to_string(),
        git_common_dir: "C:/work/task/.git".to_string(),
        branch: "main".to_string(),
        head_commit: "1111111".to_string(),
    }
}

async fn allocation_fixture(
    name: &str,
    phase: TaskRunPhase,
) -> (StudioStore, String, TaskRunRecord) {
    let store = StudioStore::open_memory().await.unwrap();
    let workspace_root = format!("C:/work/{name}");
    let project = store.upsert_project(&workspace_root).await.unwrap();
    let session = store
        .create_thread(&project.id, "Task", StudioMode::Task)
        .await
        .unwrap();
    let mut input = create_input(&session.id, phase);
    input.workspace_root = workspace_root.clone();
    input.git_common_dir = format!("{workspace_root}/.git");
    let (run, _) = store.create_task_run_with_lease(input).await.unwrap();
    (store, session.id, run)
}

#[tokio::test]
async fn task_run_and_branch_lease_are_created_atomically() {
    let store = StudioStore::open_memory().await.unwrap();
    let project = store.upsert_project("C:/work/task").await.unwrap();
    let session = store
        .create_thread(&project.id, "Task", StudioMode::Task)
        .await
        .unwrap();
    let competing_session = store
        .create_thread(&project.id, "Other task", StudioMode::Task)
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
async fn blocked_merge_retry_atomically_restores_phase_generation_and_lease() {
    let store = StudioStore::open_memory().await.unwrap();
    let project = store.upsert_project("C:/work/retry-merge").await.unwrap();
    let session = store
        .create_thread(&project.id, "Retry merge", StudioMode::Task)
        .await
        .unwrap();
    let mut input = create_input(&session.id, TaskRunPhase::Merging);
    input.workspace_root = "C:/work/retry-merge".to_string();
    input.git_common_dir = "C:/work/retry-merge/.git".to_string();
    let (run, _) = store.create_task_run_with_lease(input).await.unwrap();

    let reason = format!("{MERGE_RECOVERY_BLOCK_PREFIX} HEAD changed before task_record_merge");
    let blocked = store
        .block_task_and_release_lease(&run.id, &reason)
        .await
        .unwrap();
    assert_eq!(blocked.phase, TaskRunPhase::Blocked);
    assert_eq!(blocked.terminal_generation, Some(blocked.task_generation));
    assert!(store.read_branch_lease(&run.id).await.unwrap().is_none());
    assert_eq!(
        store
            .list_retryable_blocked_merge_task_runs()
            .await
            .unwrap(),
        vec![blocked.clone()]
    );

    let retried = store.retry_blocked_merge_task(&blocked).await.unwrap();
    assert_eq!(retried.phase, TaskRunPhase::Merging);
    assert_eq!(retried.task_generation, blocked.task_generation + 1);
    assert_eq!(retried.terminal_generation, None);
    assert_eq!(retried.status_message, None);
    let lease = store.read_branch_lease(&run.id).await.unwrap().unwrap();
    assert_eq!(lease.git_common_dir, retried.git_common_dir);
    assert_eq!(lease.branch, retried.branch);
    assert_eq!(lease.expected_head, retried.expected_head);
    assert!(
        store
            .list_retryable_blocked_merge_task_runs()
            .await
            .unwrap()
            .is_empty()
    );
    assert!(
        store
            .retry_blocked_merge_task(&blocked)
            .await
            .unwrap_err()
            .to_string()
            .contains("state changed")
    );
}

#[tokio::test]
async fn task_stop_gate_is_durable_and_keeps_lease_for_terminalization() {
    let store = StudioStore::open_memory().await.unwrap();
    let project = store.upsert_project("C:/work/task-stop").await.unwrap();
    let session = store
        .create_thread(&project.id, "Task", StudioMode::Task)
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
            thread_id: session.id.clone(),
            title: "must not start after request".to_string(),
            scope_hints: vec!["src".to_string()],
            agent_id: "agent-after-request".to_string(),
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
async fn task_recovery_clears_stop_with_cas_and_is_idempotent() {
    let (store, _thread_id, run) =
        allocation_fixture("task-recovery-stop", TaskRunPhase::Implementing).await;
    let requested = store
        .request_task_stop(
            &run.id,
            &run.expected_head,
            TaskStopOrigin::UserRequest,
            &TaskStopReason::new("pause before recovery").unwrap(),
        )
        .await
        .unwrap();

    let stale_error = store
        .clear_task_stop_for_recovery(
            &run.id,
            requested.task_generation + 1,
            TaskRunPhase::Implementing,
            &run.expected_head,
        )
        .await
        .unwrap_err();
    assert!(stale_error.to_string().contains("facts changed"));
    assert!(
        store
            .read_task_run(&run.id)
            .await
            .unwrap()
            .unwrap()
            .stop_requested
    );

    let phase_error = store
        .clear_task_stop_for_recovery(
            &run.id,
            requested.task_generation,
            TaskRunPhase::Reviewing,
            &run.expected_head,
        )
        .await
        .unwrap_err();
    assert!(phase_error.to_string().contains("during phase reviewing"));
    assert!(
        store
            .read_task_run(&run.id)
            .await
            .unwrap()
            .unwrap()
            .stop_requested
    );

    assert!(
        store
            .clear_task_stop_for_recovery(
                &run.id,
                requested.task_generation,
                TaskRunPhase::Implementing,
                &run.expected_head,
            )
            .await
            .unwrap()
    );
    let resumed = store.read_task_run(&run.id).await.unwrap().unwrap();
    assert!(!resumed.stop_requested);
    assert_eq!(resumed.stop_requested_origin, None);
    assert_eq!(resumed.stop_requested_reason, None);
    assert_eq!(resumed.stop_requested_at, None);
    assert!(
        !store
            .clear_task_stop_for_recovery(
                &run.id,
                requested.task_generation,
                TaskRunPhase::Implementing,
                &run.expected_head,
            )
            .await
            .unwrap()
    );
}

#[tokio::test]
async fn executor_allocation_reuses_call_id_and_active_semantic_assignment() {
    let (store, thread_id, run) = allocation_fixture(
        "executor-allocation-idempotency",
        TaskRunPhase::Implementing,
    )
    .await;
    let first = store
        .allocate_executor(AllocateExecutor {
            thread_id: thread_id.clone(),
            title: "  implement   model transport  ".to_string(),
            scope_hints: vec!["tests".to_string(), "src".to_string(), "src".to_string()],
            agent_id: "agent-first".to_string(),
            requested_by_call_id: "call-first".to_string(),
        })
        .await
        .unwrap();
    assert!(!first.reused);
    assert_eq!(first.work_unit.title, "implement model transport");
    assert_eq!(first.work_unit.scope_hints, vec!["src", "tests"]);

    let repeated_call = store
        .allocate_executor(AllocateExecutor {
            thread_id: thread_id.clone(),
            title: "implement model transport".to_string(),
            scope_hints: vec!["src".to_string(), "tests".to_string()],
            agent_id: "agent-first".to_string(),
            requested_by_call_id: "call-first".to_string(),
        })
        .await
        .unwrap();
    assert!(repeated_call.reused);
    assert_eq!(repeated_call.work_unit.id, first.work_unit.id);

    let repeated_assignment = store
        .allocate_executor(AllocateExecutor {
            thread_id: thread_id.clone(),
            title: "implement model transport".to_string(),
            scope_hints: vec!["tests".to_string(), "src".to_string()],
            agent_id: "agent-duplicate".to_string(),
            requested_by_call_id: "call-duplicate".to_string(),
        })
        .await
        .unwrap();
    assert!(repeated_assignment.reused);
    assert_eq!(repeated_assignment.work_unit.id, first.work_unit.id);
    assert_eq!(
        repeated_assignment.work_unit.executor_thread_id.as_deref(),
        Some("agent-first")
    );
    assert_eq!(
        repeated_assignment.work_unit.requested_by_call_id,
        "call-first"
    );

    let different_title = store
        .allocate_executor(AllocateExecutor {
            thread_id: thread_id.clone(),
            title: "implement studio bridge".to_string(),
            scope_hints: vec!["src".to_string(), "tests".to_string()],
            agent_id: "agent-title".to_string(),
            requested_by_call_id: "call-title".to_string(),
        })
        .await
        .unwrap();
    assert!(!different_title.reused);
    assert_ne!(different_title.work_unit.id, first.work_unit.id);

    let different_scope = store
        .allocate_executor(AllocateExecutor {
            thread_id,
            title: "implement model transport".to_string(),
            scope_hints: vec!["design".to_string()],
            agent_id: "agent-scope".to_string(),
            requested_by_call_id: "call-scope".to_string(),
        })
        .await
        .unwrap();
    assert!(!different_scope.reused);
    assert_ne!(different_scope.work_unit.id, first.work_unit.id);
    assert_eq!(store.list_work_units(&run.id).await.unwrap().len(), 3);
}

#[tokio::test]
async fn executor_allocation_creates_new_attempt_after_terminal_or_in_reworking() {
    let (store, thread_id, _) =
        allocation_fixture("executor-allocation-terminal", TaskRunPhase::Implementing).await;
    let first = store
        .allocate_executor(AllocateExecutor {
            thread_id: thread_id.clone(),
            title: "implement model transport".to_string(),
            scope_hints: vec!["src".to_string()],
            agent_id: "agent-first".to_string(),
            requested_by_call_id: "call-first".to_string(),
        })
        .await
        .unwrap();
    store
        .fail_executor(&first.work_unit.id, "agent-first", "terminal test")
        .await
        .unwrap();
    let after_terminal = store
        .allocate_executor(AllocateExecutor {
            thread_id,
            title: "implement model transport".to_string(),
            scope_hints: vec!["src".to_string()],
            agent_id: "agent-second".to_string(),
            requested_by_call_id: "call-second".to_string(),
        })
        .await
        .unwrap();
    assert!(!after_terminal.reused);
    assert_eq!(after_terminal.work_unit.attempt, 2);

    let (store, thread_id, _) =
        allocation_fixture("executor-allocation-reworking", TaskRunPhase::Reworking).await;
    let first = store
        .allocate_executor(AllocateExecutor {
            thread_id: thread_id.clone(),
            title: "implement model transport".to_string(),
            scope_hints: vec!["src".to_string()],
            agent_id: "agent-first".to_string(),
            requested_by_call_id: "call-first".to_string(),
        })
        .await
        .unwrap();
    let rework = store
        .allocate_executor(AllocateExecutor {
            thread_id,
            title: "implement model transport".to_string(),
            scope_hints: vec!["src".to_string()],
            agent_id: "agent-rework".to_string(),
            requested_by_call_id: "call-rework".to_string(),
        })
        .await
        .unwrap();
    assert!(!rework.reused);
    assert_ne!(rework.work_unit.id, first.work_unit.id);
    assert_eq!(rework.work_unit.attempt, 2);
}

#[tokio::test]
async fn executor_close_normalizes_cancelled_awaiting_completion_and_is_idempotent() {
    let fixture = ExecutorFixture::new("close-cancelled-awaiting-completion").await;
    fixture
        .store
        .execute_test_sql(&format!(
            "UPDATE work_units SET status = 'awaitingCompletion', execution_status = 'cancelled', continuation_revision = 7 WHERE id = '{}'",
            fixture.work_unit.id
        ))
        .await;

    assert_eq!(
        fixture
            .store
            .preflight_executor_close(
                &fixture.run.root_thread_id,
                &fixture.work_unit.id,
                &fixture.agent_id,
            )
            .await
            .unwrap(),
        ExecutorCloseDisposition::Discard
    );
    assert_eq!(
        fixture
            .store
            .settle_executor_close(
                &fixture.run.root_thread_id,
                &fixture.work_unit.id,
                &fixture.agent_id,
            )
            .await
            .unwrap(),
        ExecutorCloseDisposition::Discard
    );
    let settled = fixture.work_unit().await;
    assert_eq!(settled.status, WorkUnitStatus::Cancelled);
    assert_eq!(settled.execution_status, ThreadExecutionStatus::Cancelled);
    assert_eq!(settled.continuation_revision, 8);
    assert_eq!(
        settled.worktree_disposition,
        TaskWorktreeDisposition::CleanupRequested
    );

    assert_eq!(
        fixture
            .store
            .settle_executor_close(
                &fixture.run.root_thread_id,
                &fixture.work_unit.id,
                &fixture.agent_id,
            )
            .await
            .unwrap(),
        ExecutorCloseDisposition::Discard
    );
    assert_eq!(fixture.work_unit().await.continuation_revision, 8);
}

#[tokio::test]
async fn executor_follow_up_start_restores_a_valid_running_pair_atomically() {
    let fixture = ExecutorFixture::new("follow-up-restores-running-pair").await;
    fixture
        .store
        .execute_test_sql(&format!(
            "UPDATE work_units SET status = 'awaitingCompletion', execution_status = 'failed', execution_error = 'transient transport failure', continuation_revision = 3 WHERE id = '{}'",
            fixture.work_unit.id
        ))
        .await;

    fixture
        .store
        .mark_executor_turn_started(&fixture.agent_id)
        .await
        .unwrap();
    let started = fixture.work_unit().await;
    assert_eq!(started.status, WorkUnitStatus::Running);
    assert_eq!(started.execution_status, ThreadExecutionStatus::Running);
    assert_eq!(started.execution_error, None);
    assert_eq!(started.continuation_revision, 4);

    fixture
        .store
        .mark_executor_turn_started(&fixture.agent_id)
        .await
        .unwrap();
    assert_eq!(fixture.work_unit().await.continuation_revision, 4);

    let recovery = fixture
        .store
        .reconcile_task_agents_after_restart(&fixture.run.id)
        .await
        .unwrap();
    assert_eq!(recovery.cancelled_thread_executions, 1);
    let recovered = fixture.work_unit().await;
    assert_eq!(recovered.status, WorkUnitStatus::AwaitingCompletion);
    assert_eq!(recovered.execution_status, ThreadExecutionStatus::Cancelled);
}

#[tokio::test]
async fn executor_close_still_rejects_completion_review_states() {
    let fixture = ExecutorFixture::new("close-review-active").await;
    for status in [
        WorkUnitStatus::ReadyForReview,
        WorkUnitStatus::Reviewing,
        WorkUnitStatus::ChangesRequested,
    ] {
        fixture
            .store
            .execute_test_sql(&format!(
                "UPDATE work_units SET status = '{}', execution_status = 'completed' WHERE id = '{}'",
                status.as_str(),
                fixture.work_unit.id
            ))
            .await;
        let error = fixture
            .store
            .preflight_executor_close(
                &fixture.run.root_thread_id,
                &fixture.work_unit.id,
                &fixture.agent_id,
            )
            .await
            .unwrap_err();
        assert!(error.to_string().contains("completion review is active"));
    }
}

#[tokio::test]
async fn completion_review_rework_loop_keeps_every_revision_immutable() {
    let fixture = ExecutorFixture::new("review-loop").await;

    for revision in 1..=4 {
        let head = format!("{revision}222222");
        let completion = fixture
            .store
            .create_work_completion(
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
            recommendation: format!(
                "replace the placeholder in revision {revision} with the validated value"
            ),
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
        assert_eq!(fixture.work_unit().await.status, WorkUnitStatus::Running);
        assert_eq!(
            fixture.work_unit().await.execution_status,
            ThreadExecutionStatus::Running
        );
    }

    let final_completion = fixture
        .store
        .create_work_completion(
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
async fn rejected_review_persists_partial_coverage_without_advancing_or_waking() {
    let fixture = ExecutorFixture::new("review-rejection-coverage").await;
    fixture
        .store
        .create_work_completion(
            &fixture.work_unit.id,
            WorkCompletionKind::Delivery,
            Some(&fixture.delivery("1222222")),
            "verification passed",
        )
        .await
        .unwrap();
    fixture
        .store
        .begin_delivery_review(
            &fixture.run.root_thread_id,
            &fixture.agent_id,
            "review-call-rejected",
        )
        .await
        .unwrap();
    let round = fixture
        .store
        .authorize_reviewer_spawn(
            &fixture.run.root_thread_id,
            "review-call-rejected",
            "reviewer-rejected",
        )
        .await
        .unwrap();
    fixture
        .store
        .activate_reviewer(&round.id, "reviewer-rejected")
        .await
        .unwrap();
    let mut rejected_coverage = round.file_reviews.clone().unwrap();
    rejected_coverage.last_diagnostics = Some(ReviewExitDiagnostics {
        submitted_count: 0,
        missing_files: vec!["src/lib.rs".to_string()],
        unreviewed_files: Vec::new(),
        duplicate_files: Vec::new(),
        extra_files: Vec::new(),
        invalid_paths: Vec::new(),
        violations: Vec::new(),
    });

    let rejected = fixture
        .store
        .record_review_rejection(
            &fixture.run.root_thread_id,
            "reviewer-rejected",
            rejected_coverage,
        )
        .await
        .unwrap();

    assert_eq!(rejected.verdict, ReviewVerdict::Pending);
    assert_eq!(rejected.reviewer_status, ThreadExecutionStatus::Running);
    assert_eq!(rejected.summary, None);
    assert!(rejected.findings.is_empty());
    let persisted_coverage = rejected.file_reviews.as_ref().unwrap();
    assert_eq!(persisted_coverage.diagnostics_revision, 1);
    assert_eq!(
        persisted_coverage
            .last_diagnostics
            .as_ref()
            .unwrap()
            .missing_files,
        vec!["src/lib.rs"]
    );
    assert_eq!(fixture.work_unit().await.status, WorkUnitStatus::Reviewing);
    assert_eq!(
        fixture
            .store
            .read_task_run(&fixture.run.id)
            .await
            .unwrap()
            .unwrap()
            .phase,
        TaskRunPhase::Implementing
    );
    assert!(
        fixture
            .store
            .list_pending_task_planner_wakes()
            .await
            .unwrap()
            .is_empty()
    );

    let accepted = fixture
        .store
        .complete_task_review(
            &fixture.run.root_thread_id,
            "reviewer-rejected",
            AgentReview {
                verdict: ReviewVerdict::Pass,
                summary: "all files reviewed".to_string(),
                design_references: Vec::new(),
                findings: Vec::new(),
            },
            persisted_coverage.accepted_attempt(),
        )
        .await
        .unwrap();
    assert_eq!(accepted.verdict, ReviewVerdict::Pass);
    assert!(accepted.file_reviews.as_ref().unwrap().is_complete());
    assert_eq!(fixture.work_unit().await.status, WorkUnitStatus::Approved);
    let wakes = fixture
        .store
        .list_pending_task_planner_wakes()
        .await
        .unwrap();
    assert_eq!(wakes.len(), 1);

    assert!(
        fixture
            .store
            .complete_task_review(
                &fixture.run.root_thread_id,
                "reviewer-rejected",
                AgentReview {
                    verdict: ReviewVerdict::Pass,
                    summary: "duplicate".to_string(),
                    design_references: Vec::new(),
                    findings: Vec::new(),
                },
                accepted.file_reviews.as_ref().unwrap().accepted_attempt(),
            )
            .await
            .is_err()
    );
    assert_eq!(
        fixture
            .store
            .list_pending_task_planner_wakes()
            .await
            .unwrap()
            .len(),
        1
    );
}

#[tokio::test]
async fn failed_rework_persists_one_recoverable_planner_wake_for_the_same_executor() {
    let fixture = ExecutorFixture::new("failed-rework-planner-wake").await;
    fixture
        .store
        .create_work_completion(
            &fixture.work_unit.id,
            WorkCompletionKind::Delivery,
            Some(&fixture.delivery("2222222")),
            "initial verification",
        )
        .await
        .unwrap();
    let round = fixture
        .finish_delivery_review(
            "reviewer-failed-rework",
            "review-call-failed-rework",
            AgentReview {
                verdict: ReviewVerdict::ChangesRequired,
                summary: "a correction is required".to_string(),
                design_references: Vec::new(),
                findings: vec![ReviewFinding {
                    severity: "major".to_string(),
                    title: "correct the implementation".to_string(),
                    body: "apply the requested correction".to_string(),
                    recommendation: "swap the branch order so the validated path returns first"
                        .to_string(),
                    path: Some("src/lib.rs".to_string()),
                    line: Some(1),
                    design_references: Vec::new(),
                }],
            },
        )
        .await;
    let review_mail_id = format!("task-review-continuation:{}", round.id);
    fixture
        .store
        .execute_test_sql(&format!(
            "INSERT INTO thread_inputs (id, thread_id, mail_id, turn_id, content, metadata_json, presentation, state, claimed_turn_id, checkpoint_seq, queue_ordinal, queued_at, claimed_at, consumed_at) VALUES ('{review_mail_id}', '{}', '{review_mail_id}', 'turn-review', 'review wake', '{{}}', 'hidden', 'consumed', 'turn-review', 1, 0, 1, 1, 1)",
            fixture.run.root_thread_id
        ))
        .await;
    fixture
        .store
        .mark_executor_turn_started(&fixture.agent_id)
        .await
        .unwrap();

    let outcome = pl_core::AgentTurnOutcome {
        turn_id: pl_core::TurnId::new("turn-rework-failed").unwrap(),
        thread_id: pl_core::ThreadId::new(fixture.agent_id.clone()).unwrap(),
        kind: pl_core::TurnOutcomeKind::Failed,
        reason: Some("turn must finalize with report_completion".to_string()),
        failure: None,
        budget_limit: None,
        rollover_compacted: false,
        rollover_compaction_error: None,
        usage: Default::default(),
        finished_at: 2,
    };
    fixture
        .store
        .settle_executor_turn_finished(&fixture.agent_id, &outcome)
        .await
        .unwrap();

    let failed = fixture.work_unit().await;
    assert_eq!(failed.id, fixture.work_unit.id);
    assert_eq!(
        failed.executor_thread_id.as_deref(),
        Some(fixture.agent_id.as_str())
    );
    assert_eq!(failed.status, WorkUnitStatus::AwaitingCompletion);
    assert_eq!(failed.execution_status, ThreadExecutionStatus::Failed);
    assert_eq!(
        failed.continuation_state,
        ExecutorContinuationState::PlannerWakePending
    );
    assert_eq!(
        failed.continuation_source_turn_id.as_deref(),
        Some("turn-rework-failed")
    );

    let wakes = fixture
        .store
        .list_pending_task_planner_wakes()
        .await
        .unwrap();
    assert_eq!(wakes.len(), 1);
    let wake = &wakes[0];
    assert!(matches!(
        &wake.source,
        TaskPlannerWakeSource::ExecutorTerminal {
            work_unit_id,
            executor_thread_id,
            source_turn_id,
        } if work_unit_id == &fixture.work_unit.id
            && executor_thread_id == &fixture.agent_id
            && source_turn_id == "turn-rework-failed"
    ));
    assert!(
        !fixture
            .store
            .task_planner_wake_was_delivered(wake)
            .await
            .unwrap()
    );

    fixture
        .store
        .settle_executor_turn_finished(&fixture.agent_id, &outcome)
        .await
        .unwrap();
    let duplicate = fixture
        .store
        .list_pending_task_planner_wakes()
        .await
        .unwrap();
    assert_eq!(duplicate, wakes);
    assert_eq!(
        fixture
            .store
            .list_work_units(&fixture.run.id)
            .await
            .unwrap()
            .len(),
        1
    );

    fixture
        .store
        .mark_executor_turn_started(&fixture.agent_id)
        .await
        .unwrap();
    let resumed = fixture.work_unit().await;
    assert_eq!(resumed.id, fixture.work_unit.id);
    assert_eq!(resumed.status, WorkUnitStatus::Running);
    assert_eq!(resumed.execution_status, ThreadExecutionStatus::Running);
    assert_eq!(resumed.continuation_state, ExecutorContinuationState::None);
    assert!(
        fixture
            .store
            .list_pending_task_planner_wakes()
            .await
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn restart_reconciliation_fails_delivery_reviewer_and_reopens_exact_completion() {
    let fixture = ExecutorFixture::new("restart-delivery-review").await;
    let completion = fixture
        .store
        .create_work_completion(
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
            &fixture.run.root_thread_id,
            &fixture.agent_id,
            "restart-delivery-review-call",
        )
        .await
        .unwrap();
    let reviewer = fixture
        .store
        .authorize_reviewer_spawn(
            &fixture.run.root_thread_id,
            "restart-delivery-review-call",
            "restart-delivery-reviewer",
        )
        .await
        .unwrap();
    fixture
        .store
        .activate_reviewer(&reviewer.id, "restart-delivery-reviewer")
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

    assert_eq!(first.cancelled_thread_executions, 1);
    assert_eq!(second.cancelled_thread_executions, 0);
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
    assert_eq!(
        stored_round.reviewer_status,
        ThreadExecutionStatus::Cancelled
    );
}

#[tokio::test]
async fn concurrent_task_phase_transition_rejects_the_stale_writer() {
    let store = StudioStore::open_memory().await.unwrap();
    let project = store.upsert_project("C:/work/task-cas").await.unwrap();
    let session = store
        .create_thread(&project.id, "Task", StudioMode::Task)
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

#[tokio::test]
async fn fatal_provider_failure_terminalizes_task_and_releases_lease() {
    let (store, root_thread_id, run) =
        allocation_fixture("fatal-provider-failure", TaskRunPhase::Implementing).await;
    let failure = provider_failure(
        pl_protocol::ProviderFailureKind::Authentication,
        pl_protocol::RetryDisposition::Permanent,
        "Invalid API key",
    );

    let settlement = store
        .record_task_agent_failure(RecordTaskAgentFailure {
            root_thread_id: root_thread_id.clone(),
            source_thread_id: root_thread_id,
            source_turn_id: "turn-fatal".to_string(),
            source_agent_id: "root-agent".to_string(),
            source_role: "planner".to_string(),
            failure,
        })
        .await
        .unwrap()
        .expect("active Task must record failure");

    assert!(settlement.terminalized);
    assert_eq!(settlement.run.phase, TaskRunPhase::Failed);
    assert!(settlement.run.terminal_failure_id.is_some());
    assert!(store.read_branch_lease(&run.id).await.unwrap().is_none());
    let failures = store.list_task_failures(&run.id).await.unwrap();
    assert_eq!(failures.len(), 1);
    assert_eq!(failures[0].disposition, TaskFailureDisposition::Fatal);
}

#[tokio::test]
async fn first_concurrent_fatal_failure_wins_terminal_identity() {
    let (store, root_thread_id, run) =
        allocation_fixture("concurrent-fatal-failure", TaskRunPhase::Implementing).await;
    let first_store = store.clone();
    let second_store = store.clone();
    let first_root = root_thread_id.clone();
    let second_root = root_thread_id.clone();

    let (first, second) = tokio::join!(
        first_store.record_task_agent_failure(RecordTaskAgentFailure {
            root_thread_id: first_root.clone(),
            source_thread_id: first_root,
            source_turn_id: "turn-fatal-a".to_string(),
            source_agent_id: "executor-a".to_string(),
            source_role: "executor".to_string(),
            failure: provider_failure(
                pl_protocol::ProviderFailureKind::Authentication,
                pl_protocol::RetryDisposition::Permanent,
                "invalid key a",
            ),
        }),
        second_store.record_task_agent_failure(RecordTaskAgentFailure {
            root_thread_id: second_root.clone(),
            source_thread_id: second_root,
            source_turn_id: "turn-fatal-b".to_string(),
            source_agent_id: "reviewer-b".to_string(),
            source_role: "reviewer".to_string(),
            failure: provider_failure(
                pl_protocol::ProviderFailureKind::Authorization,
                pl_protocol::RetryDisposition::Permanent,
                "permission denied b",
            ),
        }),
    );

    let first = first.unwrap();
    let second = second.unwrap();
    assert_eq!(
        usize::from(first.is_some()) + usize::from(second.is_some()),
        1
    );
    let stored_run = store.read_task_run(&run.id).await.unwrap().unwrap();
    assert_eq!(stored_run.phase, TaskRunPhase::Failed);
    let failures = store.list_task_failures(&run.id).await.unwrap();
    assert_eq!(failures.len(), 1);
    assert_eq!(
        stored_run.terminal_failure_id.as_deref(),
        Some(failures[0].id.as_str())
    );
}

#[tokio::test]
async fn recoverable_provider_failure_keeps_task_and_lease_active() {
    let (store, root_thread_id, run) =
        allocation_fixture("recoverable-provider-failure", TaskRunPhase::Implementing).await;
    let failure = provider_failure(
        pl_protocol::ProviderFailureKind::Transport,
        pl_protocol::RetryDisposition::Retryable {
            retry_after_ms: Some(250),
        },
        "connection timed out",
    );

    let settlement = store
        .record_task_agent_failure(RecordTaskAgentFailure {
            root_thread_id: root_thread_id.clone(),
            source_thread_id: root_thread_id,
            source_turn_id: "turn-recoverable".to_string(),
            source_agent_id: "root-agent".to_string(),
            source_role: "planner".to_string(),
            failure,
        })
        .await
        .unwrap()
        .expect("active Task must record failure");

    assert!(!settlement.terminalized);
    assert_eq!(settlement.run.phase, TaskRunPhase::Implementing);
    assert!(settlement.run.terminal_failure_id.is_none());
    assert!(store.read_branch_lease(&run.id).await.unwrap().is_some());
    assert_eq!(
        store.list_task_failures(&run.id).await.unwrap()[0].disposition,
        TaskFailureDisposition::Recoverable
    );
}

#[tokio::test]
async fn fatal_executor_failure_preserves_worktree_for_manual_cleanup() {
    let fixture = ExecutorFixture::new("fatal-executor-protect").await;

    fixture
        .store
        .record_task_agent_failure(RecordTaskAgentFailure {
            root_thread_id: fixture.run.root_thread_id.clone(),
            source_thread_id: fixture.agent_id.clone(),
            source_turn_id: "turn-executor-fatal".to_string(),
            source_agent_id: fixture.agent_id.clone(),
            source_role: "executor".to_string(),
            failure: provider_failure(
                pl_protocol::ProviderFailureKind::Authorization,
                pl_protocol::RetryDisposition::Permanent,
                "permission denied",
            ),
        })
        .await
        .unwrap();

    let unit = fixture
        .store
        .list_work_units(&fixture.run.id)
        .await
        .unwrap()
        .pop()
        .expect("work unit remains durable");
    assert_eq!(unit.status, WorkUnitStatus::Failed);
    assert_eq!(unit.execution_status, ThreadExecutionStatus::Failed);
    assert_eq!(unit.worktree_disposition, TaskWorktreeDisposition::Protect);
    assert_eq!(unit.worktree_path, fixture.work_unit.worktree_path);
    assert_eq!(unit.branch, fixture.work_unit.branch);
}

fn provider_failure(
    kind: pl_protocol::ProviderFailureKind,
    retry: pl_protocol::RetryDisposition,
    message: &str,
) -> pl_protocol::TurnFailure {
    pl_protocol::TurnFailure {
        category: pl_protocol::TurnFailureCategory::Provider,
        provider_kind: Some(kind),
        code: None,
        http_status: None,
        message: message.to_string(),
        retry,
    }
}

struct ExecutorFixture {
    store: StudioStore,
    run: TaskRunRecord,
    work_unit: WorkUnitRecord,
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
            .create_thread(&project.id, "Task", StudioMode::Task)
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
                scope_hints: vec!["src".to_string()],
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
        store
            .activate_executor(&work_unit.id, &agent_id)
            .await
            .unwrap();
        Self {
            store,
            run,
            work_unit,
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
            .begin_delivery_review(
                &self.run.root_thread_id,
                &self.agent_id,
                requested_by_call_id,
            )
            .await
            .unwrap();
        assert_eq!(round.scope, ReviewScope::Delivery);
        let reviewer = self
            .store
            .authorize_reviewer_spawn(
                &self.run.root_thread_id,
                requested_by_call_id,
                reviewer_agent_id,
            )
            .await
            .unwrap();
        self.store
            .activate_reviewer(&reviewer.id, reviewer_agent_id)
            .await
            .unwrap();
        self.store
            .complete_task_review(
                &self.run.root_thread_id,
                reviewer_agent_id,
                review,
                reviewer.file_reviews.as_ref().unwrap().accepted_attempt(),
            )
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
}

#[tokio::test]
async fn task_runtime_refresh_tracks_executor_durable_revision() {
    let fixture = ExecutorFixture::new("task-runtime-executor-progress").await;
    fixture
        .store
        .create_child_thread(crate::studio::ChildThreadSpec {
            id: fixture.agent_id.clone(),
            parent_thread_id: fixture.run.root_thread_id.clone(),
            agent_path: fixture.agent_id.clone(),
            role: "executor".to_string(),
            title: "Task executor".to_string(),
        })
        .await
        .unwrap();
    fixture
        .store
        .execute_test_sql(&format!(
            "UPDATE threads SET runtime_revision = 7 WHERE id = '{}'",
            fixture.agent_id
        ))
        .await;

    let product_events = crate::StudioProductEventRuntime::new(fixture.store.clone());
    let first = product_events
        .refresh_task(&fixture.run.root_thread_id)
        .await
        .unwrap()
        .unwrap();
    let crate::StudioProductEventKind::TaskChanged {
        task: Some(first_task),
        ..
    } = first.kind
    else {
        panic!("expected task snapshot");
    };
    assert_eq!(first_task.work_units[0].executor_progress_revision, 7);
    assert!(
        product_events
            .refresh_task(&fixture.run.root_thread_id)
            .await
            .unwrap()
            .is_none()
    );

    fixture
        .store
        .execute_test_sql(&format!(
            "UPDATE threads SET runtime_revision = 8 WHERE id = '{}'",
            fixture.agent_id
        ))
        .await;
    let second = product_events
        .refresh_task(&fixture.run.root_thread_id)
        .await
        .unwrap()
        .unwrap();
    let crate::StudioProductEventKind::TaskChanged {
        task: Some(second_task),
        ..
    } = second.kind
    else {
        panic!("expected task snapshot after executor progress");
    };
    assert_eq!(second_task.work_units[0].executor_progress_revision, 8);
}

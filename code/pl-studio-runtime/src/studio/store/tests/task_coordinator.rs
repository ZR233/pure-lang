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
            owned_paths: vec!["src/**".to_string()],
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
            .complete_task_review(&self.run.root_thread_id, reviewer_agent_id, review)
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

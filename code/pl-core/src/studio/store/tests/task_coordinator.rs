use super::*;
use crate::studio::task_coordinator::*;

fn create_input(session_id: &str) -> CreateTaskRun {
    CreateTaskRun {
        session_id: session_id.to_string(),
        phase: TaskRunPhase::Planning,
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
        .create_session(&project.id, "Task", CompileMode::Task)
        .await
        .unwrap();
    let competing_session = store
        .create_session(&project.id, "Other task", CompileMode::Task)
        .await
        .unwrap();

    let (run, lease) = store
        .create_task_run_with_lease(create_input(&session.id))
        .await
        .unwrap();
    let error = store
        .create_task_run_with_lease(create_input(&competing_session.id))
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
async fn task_phase_and_expected_head_updates_are_guarded() {
    let store = StudioStore::open_memory().await.unwrap();
    let project = store.upsert_project("C:/work/task").await.unwrap();
    let session = store
        .create_session(&project.id, "Task", CompileMode::Task)
        .await
        .unwrap();
    let (run, _) = store
        .create_task_run_with_lease(create_input(&session.id))
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
async fn coordinator_child_records_round_trip_typed_payloads() {
    let store = StudioStore::open_memory().await.unwrap();
    let project = store.upsert_project("C:/work/task").await.unwrap();
    let session = store
        .create_session(&project.id, "Task", CompileMode::Task)
        .await
        .unwrap();
    let (run, _) = store
        .create_task_run_with_lease(create_input(&session.id))
        .await
        .unwrap();

    let work_unit = store
        .create_work_unit(CreateWorkUnit {
            task_run_id: run.id.clone(),
            title: "Implement core".to_string(),
            owned_paths: vec!["code/pl-core/**".to_string()],
            base_commit: "1111111".to_string(),
            worktree_path: "C:/work/task/.pure/worktrees/run/agent-1".to_string(),
            branch: "pure-task-run-agent-1".to_string(),
            attempt: 1,
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
    assert_eq!(
        store.list_work_units(&run.id).await.unwrap(),
        vec![work_unit.clone()]
    );
    assert_eq!(work_unit.base_commit, "1111111");
    assert_eq!(
        work_unit.worktree_path,
        "C:/work/task/.pure/worktrees/run/agent-1"
    );
    assert_eq!(work_unit.branch, "pure-task-run-agent-1");

    let outcome = store
        .create_agent_outcome(CreateAgentOutcome {
            task_run_id: run.id.clone(),
            work_unit_id: Some(work_unit.id),
            agent_id: "agent-1".to_string(),
            owner_path: "root".to_string(),
            initiated_by: "planner".to_string(),
            requested_by_call_id: "call-spawn".to_string(),
            role: "executor".to_string(),
            status: AgentOutcomeStatus::Running,
            attempt: 1,
        })
        .await
        .unwrap();
    let delivery = AgentDelivery {
        worktree: AgentWorktreeDelivery {
            path: "C:/work/task/.pure/worktrees/run/agent-1".to_string(),
            branch: "pure-task-run-agent-1".to_string(),
        },
        base_commit: "1111111".to_string(),
        head_commit: "2222222".to_string(),
        changed_files: vec!["code/pl-core/src/lib.rs".to_string()],
        verification_summary: "cargo test passed".to_string(),
    };
    let outcome = store
        .update_agent_outcome(
            &outcome.id,
            UpdateAgentOutcome {
                status: AgentOutcomeStatus::Completed,
                summary: Some("implemented".to_string()),
                error: None,
                delivery: Some(delivery.clone()),
                review: None,
            },
        )
        .await
        .unwrap();
    assert_eq!(outcome.delivery, Some(delivery));
    assert_eq!(
        store.list_agent_outcomes(&run.id).await.unwrap(),
        vec![outcome]
    );

    let merge = store
        .create_merge_record(CreateMergeRecord {
            task_run_id: run.id.clone(),
            agent_id: "agent-1".to_string(),
            expected_head: "1111111".to_string(),
            source_commit: "2222222".to_string(),
            conflict_files: vec!["code/pl-core/src/lib.rs".to_string()],
        })
        .await
        .unwrap();
    let merge = store
        .update_merge_record(
            &merge.id,
            UpdateMergeRecord {
                status: MergeStatus::Merged,
                resolution_summary: Some("kept both changes".to_string()),
                verification: Some(vec!["cargo test".to_string()]),
                attempt: 1,
            },
        )
        .await
        .unwrap();
    assert_eq!(
        store.list_merge_records(&run.id).await.unwrap(),
        vec![merge]
    );

    let review = store
        .create_review_round(CreateReviewRound {
            task_run_id: run.id.clone(),
            round: 1,
            head_commit: "3333333".to_string(),
            reviewer_agent_id: Some("agent-reviewer".to_string()),
        })
        .await
        .unwrap();
    let reference = ReviewDesignReference {
        path: "design/16-task-orchestration.md".to_string(),
        section: "Reviewer".to_string(),
    };
    let review = store
        .update_review_round(
            &review.id,
            CompleteReviewRound {
                verdict: ReviewVerdict::Pass,
                summary: "matches design".to_string(),
                design_references: vec![reference.clone()],
                findings: Vec::new(),
            },
        )
        .await
        .unwrap();
    assert_eq!(review.design_references, vec![reference]);
    assert_eq!(
        store.list_review_rounds(&run.id).await.unwrap(),
        vec![review]
    );
}

#[tokio::test]
async fn completed_delivery_rolls_back_outcome_when_work_unit_update_fails() {
    let (store, run, work_unit, outcome) = delivery_transition_fixture().await;
    install_work_unit_transition_failure(&store, "delivered").await;

    let error = store
        .complete_agent_delivery(&outcome.id, &work_unit.id, delivery_receipt())
        .await
        .expect_err("work unit update failure must roll back outcome update");

    assert!(
        error
            .to_string()
            .contains("injected work unit transition failure")
    );
    let persisted_outcome = store
        .list_agent_outcomes(&run.id)
        .await
        .unwrap()
        .into_iter()
        .find(|record| record.id == outcome.id)
        .unwrap();
    let persisted_work_unit = store.read_work_unit(&work_unit.id).await.unwrap().unwrap();
    assert_eq!(persisted_outcome.status, AgentOutcomeStatus::Running);
    assert_eq!(persisted_outcome.delivery, None);
    assert_eq!(persisted_work_unit.status, WorkUnitStatus::Running);
}

#[tokio::test]
async fn waiting_delivery_rolls_back_outcome_when_work_unit_update_fails() {
    let (store, run, work_unit, outcome) = delivery_transition_fixture().await;
    install_work_unit_transition_failure(&store, "waitingForDelivery").await;

    let error = store
        .mark_agent_delivery_waiting(
            &outcome.id,
            Some(work_unit.id.as_str()),
            "validation failed",
        )
        .await
        .expect_err("work unit update failure must roll back outcome update");

    assert!(
        error
            .to_string()
            .contains("injected work unit transition failure")
    );
    let persisted_outcome = store
        .list_agent_outcomes(&run.id)
        .await
        .unwrap()
        .into_iter()
        .find(|record| record.id == outcome.id)
        .unwrap();
    let persisted_work_unit = store.read_work_unit(&work_unit.id).await.unwrap().unwrap();
    assert_eq!(persisted_outcome.status, AgentOutcomeStatus::Running);
    assert_eq!(persisted_outcome.error, None);
    assert_eq!(persisted_work_unit.status, WorkUnitStatus::Running);
}

async fn delivery_transition_fixture() -> (
    StudioStore,
    TaskRunRecord,
    WorkUnitRecord,
    AgentOutcomeRecord,
) {
    let store = StudioStore::open_memory().await.unwrap();
    let project = store.upsert_project("C:/work/task").await.unwrap();
    let session = store
        .create_session(&project.id, "Task", CompileMode::Task)
        .await
        .unwrap();
    let (run, _) = store
        .create_task_run_with_lease(create_input(&session.id))
        .await
        .unwrap();
    let work_unit = store
        .create_work_unit(CreateWorkUnit {
            task_run_id: run.id.clone(),
            title: "Implement core".to_string(),
            owned_paths: vec!["code/pl-core/**".to_string()],
            base_commit: "1111111".to_string(),
            worktree_path: "C:/work/task/.pure/worktrees/run/agent-1".to_string(),
            branch: "pure-task-run-agent-1".to_string(),
            attempt: 1,
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
    let outcome = store
        .create_agent_outcome(CreateAgentOutcome {
            task_run_id: run.id.clone(),
            work_unit_id: Some(work_unit.id.clone()),
            agent_id: "agent-1".to_string(),
            owner_path: "root".to_string(),
            initiated_by: "planner".to_string(),
            requested_by_call_id: "call-spawn".to_string(),
            role: "executor".to_string(),
            status: AgentOutcomeStatus::Running,
            attempt: 1,
        })
        .await
        .unwrap();
    (store, run, work_unit, outcome)
}

fn delivery_receipt() -> AgentDelivery {
    AgentDelivery {
        worktree: AgentWorktreeDelivery {
            path: "C:/work/task/.pure/worktrees/run/agent-1".to_string(),
            branch: "pure-task-run-agent-1".to_string(),
        },
        base_commit: "1111111".to_string(),
        head_commit: "2222222".to_string(),
        changed_files: vec!["code/pl-core/src/lib.rs".to_string()],
        verification_summary: "cargo test passed".to_string(),
    }
}

async fn install_work_unit_transition_failure(store: &StudioStore, status: &str) {
    store
        .db
        .execute(Statement::from_string(
            DatabaseBackend::Sqlite,
            format!(
                "CREATE TRIGGER fail_work_unit_transition
                 BEFORE UPDATE OF status ON work_units
                 WHEN NEW.status = '{status}'
                 BEGIN
                   SELECT RAISE(ABORT, 'injected work unit transition failure');
                 END;"
            ),
        ))
        .await
        .unwrap();
}

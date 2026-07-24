use super::*;
use crate::studio::store::task::ContinuationSnapshotTestBarrier;
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
        .create_session(&project.id, "Task", StudioMode::Task)
        .await
        .unwrap();
    let competing_session = store
        .create_session(&project.id, "Other task", StudioMode::Task)
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
async fn task_stop_gate_is_durable_and_keeps_lease_for_terminalization() {
    let store = StudioStore::open_memory().await.unwrap();
    let project = store.upsert_project("C:/work/task-stop").await.unwrap();
    let session = store
        .create_session(&project.id, "Task", StudioMode::Task)
        .await
        .unwrap();
    let mut input = create_input(&session.id);
    input.workspace_root = "C:/work/task-stop".to_string();
    input.git_common_dir = "C:/work/task-stop/.git".to_string();
    input.phase = TaskRunPhase::Implementing;
    let (run, _) = store.create_task_run_with_lease(input).await.unwrap();

    let requested = store
        .request_task_stop(&run.id, &run.expected_head, "test stop")
        .await
        .unwrap();
    assert!(requested.stop_requested);
    assert_eq!(requested.phase, TaskRunPhase::Implementing);
    assert!(store.read_branch_lease(&run.id).await.unwrap().is_some());
    let requested_allocation = store
        .allocate_executor(AllocateExecutor {
            session_id: session.id.clone(),
            title: "must not start after request".to_string(),
            owned_paths: vec!["src/**".to_string()],
            agent_id: "agent-after-request".to_string(),
            owner_path: "/root".to_string(),
            requested_by_call_id: "call-after-request".to_string(),
        })
        .await;
    let requested_allocation = match requested_allocation {
        Ok(_) => panic!("stop request must reject executor allocation"),
        Err(error) => error,
    };
    assert!(
        requested_allocation
            .to_string()
            .contains("after task stop was requested")
    );
    let stopping = store
        .begin_task_stop(&run.id, &run.expected_head)
        .await
        .unwrap();

    assert_eq!(stopping.phase, TaskRunPhase::Stopping);
    assert!(store.read_branch_lease(&run.id).await.unwrap().is_some());
    let allocation = store
        .allocate_executor(AllocateExecutor {
            session_id: session.id,
            title: "must not start".to_string(),
            owned_paths: vec!["src/**".to_string()],
            agent_id: "agent-after-stop".to_string(),
            owner_path: "/root".to_string(),
            requested_by_call_id: "call-after-stop".to_string(),
        })
        .await;
    let error = match allocation {
        Ok(_) => panic!("stopping gate must reject executor allocation"),
        Err(error) => error,
    };
    assert!(error.to_string().contains("task stop was requested"));
}

#[tokio::test]
async fn stopping_task_rejects_executor_delivery_without_mutating_records() {
    let (store, run, work_unit, outcome) = delivery_transition_fixture().await;
    store
        .request_task_stop(&run.id, &run.expected_head, "test stop")
        .await
        .unwrap();
    store
        .begin_task_stop(&run.id, &run.expected_head)
        .await
        .unwrap();

    let error = store
        .complete_agent_delivery(&outcome.id, &work_unit.id, delivery_receipt())
        .await
        .expect_err("stopping task must reject executor delivery");

    assert!(error.to_string().contains("not accepting agent delivery"));
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
async fn stopping_task_rejects_new_explorer_outcome() {
    let store = StudioStore::open_memory().await.unwrap();
    let project = store
        .upsert_project("C:/work/task-stop-explorer")
        .await
        .unwrap();
    let session = store
        .create_session(&project.id, "Task", StudioMode::Task)
        .await
        .unwrap();
    let mut input = create_input(&session.id);
    input.workspace_root = "C:/work/task-stop-explorer".to_string();
    input.git_common_dir = "C:/work/task-stop-explorer/.git".to_string();
    let (run, _) = store.create_task_run_with_lease(input).await.unwrap();
    store
        .request_task_stop(&run.id, &run.expected_head, "test stop")
        .await
        .unwrap();
    store
        .begin_task_stop(&run.id, &run.expected_head)
        .await
        .unwrap();

    let outcome = store
        .create_explorer_outcome(
            &session.id,
            CreateAgentOutcome {
                task_run_id: run.id.clone(),
                work_unit_id: None,
                agent_id: "agent-after-stop".to_string(),
                owner_path: "/root".to_string(),
                initiated_by: "planner".to_string(),
                requested_by_call_id: "call-after-stop".to_string(),
                role: "explorer".to_string(),
                status: AgentOutcomeStatus::Queued,
                attempt: 1,
            },
        )
        .await
        .unwrap();

    assert_eq!(outcome, None);
    assert!(store.list_agent_outcomes(&run.id).await.unwrap().is_empty());
}

#[tokio::test]
async fn worktree_owner_queries_aggregate_common_directory_and_all_runs() {
    let store = StudioStore::open_memory().await.unwrap();
    let mut runs = Vec::new();
    for (index, (workspace_root, git_common_dir)) in [
        ("C:/work/main", "C:/work/main/.git"),
        ("C:/work/linked", "C:/work/main/.git"),
        ("C:/other/main", "C:/other/main/.git"),
    ]
    .into_iter()
    .enumerate()
    {
        let project = store.upsert_project(workspace_root).await.unwrap();
        let session = store
            .create_session(&project.id, &format!("Task {index}"), StudioMode::Task)
            .await
            .unwrap();
        let mut input = create_input(&session.id);
        input.workspace_root = workspace_root.to_string();
        input.git_common_dir = git_common_dir.to_string();
        input.branch = format!("task-{index}");
        runs.push(store.create_task_run_with_lease(input).await.unwrap().0);
    }

    let common = store
        .list_task_worktree_owners_by_git_common_dir("C:/work/main/.git")
        .await
        .unwrap();
    let all = store.list_all_task_worktree_owners().await.unwrap();
    let mut common_ids = common
        .into_iter()
        .map(|owner| owner.run.id)
        .collect::<Vec<_>>();
    let mut all_ids = all
        .into_iter()
        .map(|owner| owner.run.id)
        .collect::<Vec<_>>();
    let mut expected_common = vec![runs[0].id.clone(), runs[1].id.clone()];
    let mut expected_all = runs.into_iter().map(|run| run.id).collect::<Vec<_>>();
    common_ids.sort();
    all_ids.sort();
    expected_common.sort();
    expected_all.sort();

    assert_eq!(common_ids, expected_common);
    assert_eq!(all_ids, expected_all);
}

#[tokio::test]
async fn restart_reconciliation_cancels_transient_agents_and_preserves_delivery() {
    let store = StudioStore::open_memory().await.unwrap();
    let project = store.upsert_project("C:/work/task").await.unwrap();
    let session = store
        .create_session(&project.id, "Task", StudioMode::Task)
        .await
        .unwrap();
    let (run, _) = store
        .create_task_run_with_lease(create_input(&session.id))
        .await
        .unwrap();

    for (name, unit_status, outcome_status) in [
        (
            "pending",
            WorkUnitStatus::Pending,
            AgentOutcomeStatus::Queued,
        ),
        (
            "running",
            WorkUnitStatus::Running,
            AgentOutcomeStatus::Running,
        ),
        (
            "waiting",
            WorkUnitStatus::WaitingForDelivery,
            AgentOutcomeStatus::WaitingForDelivery,
        ),
    ] {
        create_recovery_executor(&store, &run, name, unit_status, outcome_status, None).await;
    }
    let delivery = AgentDelivery {
        worktree: AgentWorktreeDelivery {
            path: "C:/work/task/.pure/worktrees/run/delivered".to_string(),
            branch: "pure-task-run-delivered".to_string(),
        },
        base_commit: run.base_commit.clone(),
        head_commit: "2222222".to_string(),
        changed_files: vec!["code/pl-core/src/lib.rs".to_string()],
        verification_summary: "passed".to_string(),
    };
    create_recovery_executor(
        &store,
        &run,
        "delivered",
        WorkUnitStatus::Delivered,
        AgentOutcomeStatus::Completed,
        Some(delivery.clone()),
    )
    .await;
    create_recovery_executor(
        &store,
        &run,
        "merged",
        WorkUnitStatus::Merged,
        AgentOutcomeStatus::Completed,
        Some(AgentDelivery {
            worktree: AgentWorktreeDelivery {
                path: "C:/work/task/.pure/worktrees/run/merged".to_string(),
                branch: "pure-task-run-merged".to_string(),
            },
            ..delivery.clone()
        }),
    )
    .await;
    create_recovery_executor(
        &store,
        &run,
        "failed",
        WorkUnitStatus::Failed,
        AgentOutcomeStatus::Failed,
        None,
    )
    .await;
    create_recovery_executor(
        &store,
        &run,
        "cancelled",
        WorkUnitStatus::Cancelled,
        AgentOutcomeStatus::Cancelled,
        None,
    )
    .await;
    let explorer = store
        .create_explorer_outcome(
            &session.id,
            CreateAgentOutcome {
                task_run_id: run.id.clone(),
                work_unit_id: None,
                agent_id: "agent-explorer".to_string(),
                owner_path: "/root".to_string(),
                initiated_by: "planner".to_string(),
                requested_by_call_id: "call-explorer".to_string(),
                role: "explorer".to_string(),
                status: AgentOutcomeStatus::Running,
                attempt: 1,
            },
        )
        .await
        .unwrap()
        .unwrap();
    let queued_explorer = store
        .create_explorer_outcome(
            &session.id,
            CreateAgentOutcome {
                task_run_id: run.id.clone(),
                work_unit_id: None,
                agent_id: "agent-explorer-queued".to_string(),
                owner_path: "/root".to_string(),
                initiated_by: "planner".to_string(),
                requested_by_call_id: "call-explorer-queued".to_string(),
                role: "explorer".to_string(),
                status: AgentOutcomeStatus::Queued,
                attempt: 1,
            },
        )
        .await
        .unwrap()
        .unwrap();

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

    assert_eq!(first.cancelled_work_units, 3);
    assert_eq!(first.cancelled_outcomes, 5);
    assert_eq!(second.cancelled_work_units, 0);
    assert_eq!(second.cancelled_outcomes, 0);
    assert!(
        units
            .iter()
            .take(3)
            .all(|unit| unit.status == WorkUnitStatus::Cancelled)
    );
    assert_eq!(units[3].status, WorkUnitStatus::Delivered);
    assert_eq!(outcomes[3].delivery, Some(delivery));
    assert_eq!(outcomes[3].status, AgentOutcomeStatus::Completed);
    assert_eq!(units[4].status, WorkUnitStatus::Merged);
    assert_eq!(outcomes[4].status, AgentOutcomeStatus::Completed);
    assert_eq!(outcomes[5].status, AgentOutcomeStatus::Failed);
    assert_eq!(outcomes[6].status, AgentOutcomeStatus::Cancelled);
    assert_eq!(
        outcomes
            .iter()
            .find(|outcome| outcome.id == explorer.id)
            .unwrap()
            .status,
        AgentOutcomeStatus::Cancelled
    );
    assert_eq!(
        outcomes
            .iter()
            .find(|outcome| outcome.id == queued_explorer.id)
            .unwrap()
            .status,
        AgentOutcomeStatus::Cancelled
    );
    assert!(matches!(
        store
            .record_terminal_agent_state(
                &session.id,
                &StudioAgentTerminalChange {
                    agent_id: "agent-running".to_string(),
                    role: "executor".to_string(),
                    outcome: crate::TurnOutcomeKind::Cancelled,
                    summary: None,
                    error: None,
                },
            )
            .await
            .unwrap(),
        TerminalAgentStateRecording::Projected(_)
    ));
}

#[tokio::test]
async fn restart_reconciliation_rolls_back_every_change_on_pair_mismatch() {
    let store = StudioStore::open_memory().await.unwrap();
    let project = store.upsert_project("C:/work/task").await.unwrap();
    let session = store
        .create_session(&project.id, "Task", StudioMode::Task)
        .await
        .unwrap();
    let (run, _) = store
        .create_task_run_with_lease(create_input(&session.id))
        .await
        .unwrap();
    create_recovery_executor(
        &store,
        &run,
        "valid",
        WorkUnitStatus::Running,
        AgentOutcomeStatus::Running,
        None,
    )
    .await;
    let mismatch = store
        .create_work_unit(CreateWorkUnit {
            task_run_id: run.id.clone(),
            title: "mismatch".to_string(),
            owned_paths: vec!["code/mismatch/**".to_string()],
            base_commit: run.base_commit.clone(),
            worktree_path: "C:/work/task/.pure/worktrees/run/mismatch".to_string(),
            branch: "pure-task-run-mismatch".to_string(),
            attempt: 1,
        })
        .await
        .unwrap();
    store
        .update_work_unit(
            &mismatch.id,
            WorkUnitStatus::Running,
            Some("agent-mismatch-a".to_string()),
        )
        .await
        .unwrap();
    store
        .create_agent_outcome(CreateAgentOutcome {
            task_run_id: run.id.clone(),
            work_unit_id: Some(mismatch.id),
            agent_id: "agent-mismatch-b".to_string(),
            owner_path: "/root".to_string(),
            initiated_by: "planner".to_string(),
            requested_by_call_id: "call-mismatch".to_string(),
            role: "executor".to_string(),
            status: AgentOutcomeStatus::Running,
            attempt: 1,
        })
        .await
        .unwrap();

    let error = store
        .reconcile_task_agents_after_restart(&run.id)
        .await
        .expect_err("mismatch must roll back the run-scoped transaction");

    assert!(error.to_string().contains("do not match"));
    assert!(
        store
            .list_work_units(&run.id)
            .await
            .unwrap()
            .iter()
            .all(|unit| unit.status == WorkUnitStatus::Running)
    );
    assert!(
        store
            .list_agent_outcomes(&run.id)
            .await
            .unwrap()
            .iter()
            .all(|outcome| outcome.status == AgentOutcomeStatus::Running)
    );
}

async fn create_recovery_executor(
    store: &StudioStore,
    run: &TaskRunRecord,
    name: &str,
    unit_status: WorkUnitStatus,
    outcome_status: AgentOutcomeStatus,
    delivery: Option<AgentDelivery>,
) {
    let agent_id = format!("agent-{name}");
    let unit = store
        .create_work_unit(CreateWorkUnit {
            task_run_id: run.id.clone(),
            title: name.to_string(),
            owned_paths: vec![format!("code/{name}/**")],
            base_commit: run.base_commit.clone(),
            worktree_path: format!("C:/work/task/.pure/worktrees/run/{name}"),
            branch: format!("pure-task-run-{name}"),
            attempt: 1,
        })
        .await
        .unwrap();
    store
        .update_work_unit(&unit.id, unit_status, Some(agent_id.clone()))
        .await
        .unwrap();
    let outcome = store
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
    if delivery.is_some() {
        store
            .update_agent_outcome(
                &outcome.id,
                UpdateAgentOutcome {
                    status: outcome_status,
                    summary: Some(name.to_string()),
                    error: None,
                    delivery,
                    review: None,
                },
            )
            .await
            .unwrap();
    }
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
        .create_session(&project.id, "Task", StudioMode::Task)
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
async fn continuation_snapshot_contains_exact_durable_task_state() {
    let store = StudioStore::open_memory().await.unwrap();
    let project = store.upsert_project("C:/work/task").await.unwrap();
    let session = store
        .create_session(&project.id, "Task", StudioMode::Task)
        .await
        .unwrap();
    let (run, lease) = store
        .create_task_run_with_lease(create_input(&session.id))
        .await
        .unwrap();
    let run = store
        .transition_task_run(
            &run.id,
            TaskRunPhase::PendingConfirmation,
            Some("waiting".into()),
        )
        .await
        .unwrap();
    let work_unit = store
        .create_work_unit(CreateWorkUnit {
            task_run_id: run.id.clone(),
            title: "Implement continuation".to_string(),
            owned_paths: vec!["code/pl-core/**".to_string()],
            base_commit: run.base_commit.clone(),
            worktree_path: "C:/work/task/.pure/worktrees/run/agent-1".to_string(),
            branch: "pure-task-run-agent-1".to_string(),
            attempt: 2,
        })
        .await
        .unwrap();
    let work_unit = store
        .update_work_unit(
            &work_unit.id,
            WorkUnitStatus::Delivered,
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
            attempt: 2,
        })
        .await
        .unwrap();
    let delivery = AgentDelivery {
        worktree: AgentWorktreeDelivery {
            path: work_unit.worktree_path.clone(),
            branch: work_unit.branch.clone(),
        },
        base_commit: run.base_commit.clone(),
        head_commit: "2222222".to_string(),
        changed_files: vec!["code/pl-core/src/studio/runtime/mod.rs".to_string()],
        verification_summary: "cargo test passed".to_string(),
    };
    let outcome = store
        .update_agent_outcome(
            &outcome.id,
            UpdateAgentOutcome {
                status: AgentOutcomeStatus::Completed,
                summary: Some("implemented continuation".to_string()),
                error: None,
                delivery: Some(delivery),
                review: None,
            },
        )
        .await
        .unwrap();
    let merge = store
        .create_merge_record(CreateMergeRecord {
            task_run_id: run.id.clone(),
            agent_id: "agent-1".to_string(),
            expected_head: run.expected_head.clone(),
            source_commit: "2222222".to_string(),
            conflict_files: vec!["code/pl-core/src/studio/runtime/mod.rs".to_string()],
        })
        .await
        .unwrap();
    let review = store
        .create_review_round(CreateReviewRound {
            task_run_id: run.id.clone(),
            round: 2,
            head_commit: run.expected_head.clone(),
            reviewer_agent_id: Some("agent-reviewer".to_string()),
        })
        .await
        .unwrap();

    let resolution = store
        .load_task_continuation_resolution(&run.id)
        .await
        .unwrap();
    let TaskContinuationResolution::Active(snapshot) = resolution else {
        panic!("active task must resolve to a continuation snapshot");
    };
    let prompt = snapshot.render_prompt().unwrap();

    assert_eq!(snapshot.run, run);
    assert_eq!(snapshot.branch_lease, lease);
    assert_eq!(snapshot.work_units, vec![work_unit]);
    assert_eq!(snapshot.agent_outcomes, vec![outcome]);
    assert_eq!(snapshot.merge_records, vec![merge]);
    assert_eq!(snapshot.review_rounds, vec![review]);
    for child_run_id in snapshot
        .work_units
        .iter()
        .map(|record| record.task_run_id.as_str())
        .chain(
            snapshot
                .agent_outcomes
                .iter()
                .map(|record| record.task_run_id.as_str()),
        )
        .chain(
            snapshot
                .merge_records
                .iter()
                .map(|record| record.task_run_id.as_str()),
        )
        .chain(
            snapshot
                .review_rounds
                .iter()
                .map(|record| record.task_run_id.as_str()),
        )
    {
        assert_eq!(child_run_id, run.id);
    }
    assert!(prompt.contains("continuation"));
    assert!(prompt.contains("pendingConfirmation"));
    assert!(prompt.contains("main"));
    assert!(prompt.contains("1111111"));
    assert!(prompt.contains("implemented continuation"));
    assert!(prompt.contains("pure-task-run-agent-1"));
    assert!(prompt.contains("mergeRecords"));
    assert!(prompt.contains("reviewRounds"));
    assert!(prompt.contains("不要无限等待代理"));
}

#[tokio::test]
async fn terminal_continuation_resolution_does_not_require_branch_lease() {
    let store = StudioStore::open_memory().await.unwrap();
    let project = store
        .upsert_project("C:/work/terminal-snapshot")
        .await
        .unwrap();
    let session = store
        .create_session(&project.id, "Terminal snapshot", StudioMode::Task)
        .await
        .unwrap();
    let (run, _) = store
        .create_task_run_with_lease(create_input(&session.id))
        .await
        .unwrap();
    let terminal_run = store
        .transition_task_run(
            &run.id,
            TaskRunPhase::Blocked,
            Some("terminal before continuation".to_string()),
        )
        .await
        .unwrap();
    store.release_branch_lease(&run.id).await.unwrap();

    let resolution = store
        .load_task_continuation_resolution(&run.id)
        .await
        .expect("terminal continuation resolution must not require a branch lease");

    assert_eq!(
        resolution,
        TaskContinuationResolution::Terminal(Box::new(terminal_run))
    );
}

#[tokio::test]
async fn concurrent_terminal_transition_resolves_active_snapshot_or_typed_terminal() {
    let store = StudioStore::open_memory().await.unwrap();
    let project = store
        .upsert_project("C:/work/concurrent-terminal-snapshot")
        .await
        .unwrap();
    let session = store
        .create_session(&project.id, "Concurrent snapshot", StudioMode::Task)
        .await
        .unwrap();
    let (run, lease) = store
        .create_task_run_with_lease(create_input(&session.id))
        .await
        .unwrap();
    let barrier = ContinuationSnapshotTestBarrier::new();
    let reader_store = store.clone();
    let reader_run_id = run.id.clone();
    let reader_barrier = barrier.clone();
    let reader = tokio::spawn(async move {
        reader_store
            .load_task_continuation_resolution_with_barrier(&reader_run_id, &reader_barrier)
            .await
    });
    barrier.wait_until_entered().await;

    let writer_store = store.clone();
    let writer_run_id = run.id.clone();
    let (writer_started_tx, writer_started_rx) = tokio::sync::oneshot::channel();
    let writer = tokio::spawn(async move {
        let _ = writer_started_tx.send(());
        writer_store
            .terminalize_task_and_release_lease_for_test(&writer_run_id)
            .await
    });
    writer_started_rx.await.unwrap();
    barrier.release().await;

    let resolution = reader.await.unwrap().unwrap();
    match resolution {
        TaskContinuationResolution::Active(snapshot) => {
            assert_eq!(snapshot.run, run);
            assert_eq!(snapshot.branch_lease, lease);
        }
        TaskContinuationResolution::Terminal(terminal) => {
            assert_eq!(terminal.id, run.id);
            assert!(terminal.phase.is_terminal());
        }
    }
    writer.await.unwrap().unwrap();
    let final_resolution = store
        .load_task_continuation_resolution(&run.id)
        .await
        .unwrap();
    assert!(matches!(
        final_resolution,
        TaskContinuationResolution::Terminal(terminal) if terminal.id == run.id
    ));
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
        .create_session(&project.id, "Task", StudioMode::Task)
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

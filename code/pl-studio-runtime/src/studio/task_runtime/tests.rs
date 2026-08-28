use super::*;
use crate::studio::task_coordinator::{CreateTaskRun, TaskExecutorBlueprint};
use crate::{StudioMode, StudioTaskState};

fn agent_failure(
    root_thread_id: &str,
    source_thread_id: &str,
    source_turn_id: &str,
    source_role: &str,
    disposition: TaskIssueDisposition,
) -> RecordTaskAgentFailure {
    RecordTaskAgentFailure {
        root_thread_id: root_thread_id.to_string(),
        source_thread_id: source_thread_id.to_string(),
        source_turn_id: source_turn_id.to_string(),
        source_agent_id: source_thread_id.to_string(),
        source_role: source_role.to_string(),
        failure: pl_protocol::TurnFailure::permanent(
            pl_protocol::TurnFailureCategory::Internal,
            "agent faulted",
        ),
        disposition,
    }
}

#[tokio::test]
async fn executor_blueprint_is_hot_immediately_and_survives_cold_activation() {
    let store = StudioStore::open_memory().await.expect("memory store");
    let workspace = std::env::temp_dir().join("pure-task-runtime-blueprint-fact");
    let project = store.upsert_project(&workspace).await.expect("project");
    let thread = store
        .create_thread(&project.id, "Task", StudioMode::Task)
        .await
        .expect("thread");
    let events = ProductEventBus::new(
        store.clone(),
        crate::studio::agent_host::ThreadWriteBehindWriter::new(store.clone()),
    );
    let runtime = TaskRuntime::new(store.clone(), events);
    runtime
        .initialize(store.list_active_task_runs().await.expect("active runs"))
        .await
        .expect("initialize runtime");
    runtime
        .create_task(CreateTaskRun {
            project_id: project.id,
            root_thread_id: thread.id.clone(),
            request: "implement".to_string(),
            workspace_root: workspace.to_string_lossy().to_string(),
        })
        .await
        .expect("create task");
    runtime
        .submit_plan(&thread.id, "plan", 0, 0)
        .await
        .expect("submit plan");
    runtime
        .apply_run_command(
            &thread.id,
            1,
            0,
            TaskCommand::ConfirmPlan { plan_revision: 1 },
        )
        .await
        .expect("confirm plan");
    runtime
        .apply_run_command(
            &thread.id,
            2,
            0,
            TaskCommand::FinishDocumentEditing {
                summary: "documents ready".to_string(),
            },
        )
        .await
        .expect("finish design");

    let blueprint = TaskExecutorBlueprint::for_test("implement feature", vec!["src".to_string()])
        .normalize_and_validate()
        .expect("valid blueprint");
    let fingerprint = blueprint.fingerprint().expect("fingerprint");
    let allocation = runtime
        .allocate_executor(AllocateExecutor {
            thread_id: thread.id.clone(),
            title: blueprint.task_name.clone(),
            scope_hints: blueprint.scope.scope_hints.clone(),
            blueprint: blueprint.clone(),
            agent_id: "executor-blueprint".to_string(),
            requested_by_call_id: "spawn-blueprint".to_string(),
        })
        .await
        .expect("allocate executor");
    let assert_blueprint_projection = |snapshot: &StudioTaskRuntime| {
        let unit = snapshot
            .work_units
            .iter()
            .find(|unit| unit.id == allocation.work_unit.id)
            .expect("projected work unit");
        assert_eq!(
            unit.blueprint_fingerprint.as_deref(),
            Some(fingerprint.as_str())
        );
        assert_eq!(
            unit.objective.as_deref(),
            Some(blueprint.objective.as_str())
        );
        assert_eq!(unit.implementation_step_count, 1);
        assert_eq!(unit.acceptance_criterion_count, 1);
        assert_eq!(unit.verification_count, 1);
    };
    assert_blueprint_projection(&runtime.snapshot(&thread.id).await.expect("hot snapshot"));

    runtime
        .record_executor_worktree_base(&allocation.work_unit.id, "executor-blueprint", "base-1")
        .await
        .expect("update canonical work unit");
    let expected_snapshot = runtime
        .snapshot(&thread.id)
        .await
        .expect("updated hot snapshot");
    assert_blueprint_projection(&expected_snapshot);
    let durable_revision = runtime
        .aggregate(&thread.id)
        .await
        .expect("resident aggregate")
        .hot_revision;
    runtime
        .await_durable(&thread.id, durable_revision)
        .await
        .expect("blueprint durability");
    runtime.writer().shutdown().await.expect("shutdown writer");

    let reopened_events = ProductEventBus::new(
        store.clone(),
        crate::studio::agent_host::ThreadWriteBehindWriter::new(store.clone()),
    );
    let reopened = TaskRuntime::new(store.clone(), reopened_events);
    reopened
        .initialize(
            store
                .list_active_task_runs()
                .await
                .expect("reopened active runs"),
        )
        .await
        .expect("reopen runtime");
    let reopened_snapshot = reopened
        .snapshot(&thread.id)
        .await
        .expect("reopened hot snapshot");
    assert_blueprint_projection(&reopened_snapshot);
    assert_eq!(reopened_snapshot, expected_snapshot);
    reopened
        .writer()
        .shutdown()
        .await
        .expect("shutdown reopened writer");
}

#[tokio::test]
async fn sqlite_mutation_does_not_override_resident_hot_task() {
    let store = StudioStore::open_memory().await.expect("memory store");
    let workspace = std::env::temp_dir().join("pure-task-runtime-hot-authority");
    let project = store.upsert_project(&workspace).await.expect("project");
    let thread = store
        .create_thread(&project.id, "Task", StudioMode::Task)
        .await
        .expect("thread");
    store
        .create_task_run(CreateTaskRun {
            project_id: project.id,
            root_thread_id: thread.id.clone(),
            request: "implement".to_string(),
            workspace_root: workspace.to_string_lossy().to_string(),
        })
        .await
        .expect("task run");
    let events = ProductEventBus::new(
        store.clone(),
        crate::studio::agent_host::ThreadWriteBehindWriter::new(store.clone()),
    );
    let runtime = TaskRuntime::new(store.clone(), events);
    runtime
        .initialize(store.list_active_task_runs().await.expect("active runs"))
        .await
        .expect("initialize runtime");
    assert!(matches!(
        runtime.snapshot(&thread.id).await.unwrap().state,
        StudioTaskState::Planning(_)
    ));
    assert_eq!(runtime.active_thread_ids().await, vec![thread.id.clone()]);

    store
        .submit_task_plan(&thread.id, "plan", "call-1", 0, 0)
        .await
        .expect("persisted adapter mutation");
    let still_hot = runtime.snapshot(&thread.id).await.unwrap();
    assert!(matches!(still_hot.state, StudioTaskState::Planning(_)));
    assert_eq!(still_hot.revision, 0);

    let still_authoritative = runtime.snapshot(&thread.id).await.unwrap();
    assert!(matches!(
        still_authoritative.state,
        StudioTaskState::Planning(_)
    ));
    assert_eq!(still_authoritative.revision, 0);
}

#[tokio::test]
async fn hot_task_commits_publish_before_explicit_durability_barrier() {
    let store = StudioStore::open_memory().await.expect("memory store");
    let workspace = std::env::temp_dir().join("pure-task-runtime-write-behind");
    let project = store.upsert_project(&workspace).await.expect("project");
    let thread = store
        .create_thread(&project.id, "Task", StudioMode::Task)
        .await
        .expect("thread");
    let events = ProductEventBus::new(
        store.clone(),
        crate::studio::agent_host::ThreadWriteBehindWriter::new(store.clone()),
    );
    let runtime = TaskRuntime::new(store.clone(), events);
    runtime
        .initialize(store.list_active_task_runs().await.expect("active runs"))
        .await
        .expect("initialize runtime");
    let writer = runtime.writer();

    let created = runtime
        .create_task(CreateTaskRun {
            project_id: project.id,
            root_thread_id: thread.id.clone(),
            request: "implement".to_string(),
            workspace_root: workspace.to_string_lossy().to_string(),
        })
        .await
        .expect("hot task creation");
    assert!(matches!(created.state, TaskRunState::Planning(_)));
    assert!(matches!(
        runtime.snapshot(&thread.id).await.unwrap().state,
        StudioTaskState::Planning(_)
    ));

    let submitted = runtime
        .submit_plan(&thread.id, "plan", 0, 0)
        .await
        .expect("hot plan submission");
    assert_eq!(submitted.revision, 1);
    assert!(matches!(
        runtime.snapshot(&thread.id).await.unwrap().state,
        StudioTaskState::PendingConfirmation(_)
    ));

    runtime
        .await_durable(&thread.id, 2)
        .await
        .expect("Task owner durability");
    let persisted = store
        .find_latest_task_run_for_root_thread(&thread.id)
        .await
        .expect("read durable Task")
        .expect("durable Task exists");
    assert_eq!(persisted, submitted);
    writer.shutdown().await.expect("shutdown writer");
}

#[tokio::test]
async fn reopened_active_task_seeds_owner_revision_from_durable_receipt() {
    let store = StudioStore::open_memory().await.expect("memory store");
    let workspace = std::env::temp_dir().join("pure-task-runtime-owner-receipt");
    let project = store.upsert_project(&workspace).await.expect("project");
    let thread = store
        .create_thread(&project.id, "Task", StudioMode::Task)
        .await
        .expect("thread");
    let first = TaskRuntime::new(
        store.clone(),
        ProductEventBus::new(
            store.clone(),
            crate::studio::agent_host::ThreadWriteBehindWriter::new(store.clone()),
        ),
    );
    first
        .initialize(store.list_active_task_runs().await.expect("active runs"))
        .await
        .expect("initialize first runtime");
    first
        .create_task(CreateTaskRun {
            project_id: project.id,
            root_thread_id: thread.id.clone(),
            request: "implement".to_string(),
            workspace_root: workspace.to_string_lossy().to_string(),
        })
        .await
        .expect("create hot Task");
    first
        .await_durable(&thread.id, 1)
        .await
        .expect("first owner revision durable");
    first
        .writer()
        .shutdown()
        .await
        .expect("shutdown first writer");

    let reopened = TaskRuntime::new(
        store.clone(),
        ProductEventBus::new(
            store.clone(),
            crate::studio::agent_host::ThreadWriteBehindWriter::new(store.clone()),
        ),
    );
    reopened
        .initialize(store.list_active_task_runs().await.expect("reopened runs"))
        .await
        .expect("initialize reopened runtime");
    let restored = reopened
        .aggregate(&thread.id)
        .await
        .expect("active Task restored");
    assert_eq!(restored.hot_revision, 1);
    assert_eq!(restored.durable_revision, 1);

    reopened
        .submit_plan(&thread.id, "plan", 0, 0)
        .await
        .expect("first post-reopen mutation");
    reopened
        .await_durable(&thread.id, 2)
        .await
        .expect("post-reopen owner revision durable");
    reopened
        .writer()
        .shutdown()
        .await
        .expect("shutdown reopened writer");
}

#[tokio::test]
async fn terminal_task_evicts_only_after_durability_and_cold_activates_again() {
    let store = StudioStore::open_memory().await.expect("memory store");
    let workspace = std::env::temp_dir().join("pure-task-runtime-cold-activation");
    let project = store.upsert_project(&workspace).await.expect("project");
    let thread = store
        .create_thread(&project.id, "Task", StudioMode::Task)
        .await
        .expect("thread");
    let events = ProductEventBus::new(
        store.clone(),
        crate::studio::agent_host::ThreadWriteBehindWriter::new(store.clone()),
    );
    let runtime = TaskRuntime::new(store.clone(), events.clone());
    runtime
        .initialize(store.list_active_task_runs().await.expect("active runs"))
        .await
        .expect("initialize runtime");
    runtime
        .create_task(CreateTaskRun {
            project_id: project.id,
            root_thread_id: thread.id.clone(),
            request: "implement".to_string(),
            workspace_root: workspace.to_string_lossy().to_string(),
        })
        .await
        .expect("hot task creation");
    runtime
        .complete_task(
            &thread.id,
            0,
            0,
            TaskOutcome::Failed {
                kind: TaskFailureKind::Fatal,
                summary: "failed".to_string(),
                evidence: "test".to_string(),
                cause: "test failure".to_string(),
                completed_at: crate::studio::ids::unix_seconds(),
            },
        )
        .await
        .expect("terminal hot commit");
    runtime
        .await_durable(&thread.id, 2)
        .await
        .expect("terminal durability");

    assert!(runtime.evict_durable(&thread.id).await);
    assert!(runtime.aggregate(&thread.id).await.is_none());
    let directory = events.read_task_directory().await.expect("task directory");
    assert_eq!(
        directory
            .state
            .value()
            .expect("ready directory")
            .tasks
            .len(),
        1
    );

    let restored = runtime
        .activate(&thread.id)
        .await
        .expect("cold activation")
        .expect("durable Task exists");
    assert!(restored.facts.run.kind().is_terminal());
    assert_eq!(restored.hot_revision, restored.durable_revision);
    runtime.writer().shutdown().await.expect("shutdown writer");
}

#[tokio::test]
async fn faulted_root_terminalizes_task_once_as_fatal() {
    let store = StudioStore::open_memory().await.expect("memory store");
    let workspace = std::env::temp_dir().join("pure-task-runtime-root-fault");
    let project = store.upsert_project(&workspace).await.expect("project");
    let thread = store
        .create_thread(&project.id, "Task", StudioMode::Task)
        .await
        .expect("thread");
    let runtime = TaskRuntime::new(
        store.clone(),
        ProductEventBus::new(
            store.clone(),
            crate::studio::agent_host::ThreadWriteBehindWriter::new(store.clone()),
        ),
    );
    runtime
        .initialize(store.list_active_task_runs().await.expect("active runs"))
        .await
        .expect("initialize runtime");
    runtime
        .create_task(CreateTaskRun {
            project_id: project.id,
            root_thread_id: thread.id.clone(),
            request: "implement".to_string(),
            workspace_root: workspace.to_string_lossy().to_string(),
        })
        .await
        .expect("hot task creation");

    let settlement = runtime
        .record_agent_failure(agent_failure(
            &thread.id,
            &thread.id,
            "turn-root-fault",
            "planner",
            TaskIssueDisposition::Fatal,
        ))
        .await
        .expect("record root fault")
        .expect("Task settlement");
    assert!(settlement.terminalized);
    let aggregate = runtime.aggregate(&thread.id).await.expect("hot Task");
    assert!(aggregate.facts.run.kind().is_terminal());
    assert_eq!(aggregate.facts.issues.len(), 1);
    let hot_revision = aggregate.hot_revision;

    assert!(
        runtime
            .record_agent_failure(agent_failure(
                &thread.id,
                &thread.id,
                "turn-root-fault",
                "planner",
                TaskIssueDisposition::Fatal,
            ))
            .await
            .expect("replay root fault")
            .is_none()
    );
    assert_eq!(
        runtime.aggregate(&thread.id).await.unwrap().hot_revision,
        hot_revision
    );
    runtime.writer().shutdown().await.expect("shutdown writer");
}

#[tokio::test]
async fn faulted_executor_fails_work_unit_and_leaves_one_hot_planner_wake() {
    let store = StudioStore::open_memory().await.expect("memory store");
    let workspace = std::env::temp_dir().join("pure-task-runtime-executor-fault");
    let project = store.upsert_project(&workspace).await.expect("project");
    let thread = store
        .create_thread(&project.id, "Task", StudioMode::Task)
        .await
        .expect("thread");
    let runtime = TaskRuntime::new(
        store.clone(),
        ProductEventBus::new(
            store.clone(),
            crate::studio::agent_host::ThreadWriteBehindWriter::new(store.clone()),
        ),
    );
    runtime
        .initialize(store.list_active_task_runs().await.expect("active runs"))
        .await
        .expect("initialize runtime");
    let run = runtime
        .create_task(CreateTaskRun {
            project_id: project.id,
            root_thread_id: thread.id.clone(),
            request: "implement".to_string(),
            workspace_root: workspace.to_string_lossy().to_string(),
        })
        .await
        .expect("hot task creation");
    runtime
        .submit_plan(&thread.id, "plan", 0, 0)
        .await
        .expect("submit plan");
    runtime
        .apply_run_command(
            &thread.id,
            1,
            0,
            TaskCommand::ConfirmPlan { plan_revision: 1 },
        )
        .await
        .expect("confirm plan");
    runtime
        .apply_run_command(
            &thread.id,
            2,
            0,
            TaskCommand::FinishDocumentEditing {
                summary: "documents ready".to_string(),
            },
        )
        .await
        .expect("finish document editing");
    let task_run_id = run.id.clone();
    let worktree = workspace.clone();
    runtime
        .commit_facts(&thread.id, move |current| {
            let mut facts = current.clone();
            facts.work_units.push(WorkUnit {
                context: WorkUnitContext {
                    id: "work-unit-faulted-executor".to_string(),
                    task_run_id,
                    title: "executor fault".to_string(),
                    scope_hints: Vec::new(),
                    blueprint: None,
                    base_commit: "HEAD".to_string(),
                    worktree_path: worktree.to_string_lossy().to_string(),
                    branch: "pure-task-executor-fault".to_string(),
                    attempt: 1,
                    supersedes_work_unit_id: None,
                    executor_thread_id: Some("executor-faulted".to_string()),
                    requested_by_call_id: "spawn-executor-faulted".to_string(),
                },
                state: WorkUnitState::pending(),
                revision: 0,
                created_at: crate::studio::ids::unix_seconds(),
                updated_at: crate::studio::ids::unix_seconds(),
            });
            apply_work_unit_command_in_facts(
                &mut facts,
                "work-unit-faulted-executor",
                WorkUnitCommand::Activate,
                crate::studio::ids::unix_seconds(),
            )?;
            apply_work_unit_command_in_facts(
                &mut facts,
                "work-unit-faulted-executor",
                WorkUnitCommand::StartTurn {
                    turn_id: "turn-executor-faulted".to_string(),
                    reset_budget: false,
                },
                crate::studio::ids::unix_seconds(),
            )?;
            facts.refresh_projection()?;
            Ok(facts)
        })
        .await
        .expect("activate executor WorkUnit");
    runtime
        .record_agent_failure(agent_failure(
            &thread.id,
            "executor-faulted",
            "turn-executor-faulted",
            "executor",
            TaskIssueDisposition::Recoverable,
        ))
        .await
        .expect("record executor fault");
    runtime
        .fail_faulted_executor("executor-faulted", "turn-executor-faulted", "agent faulted")
        .await
        .expect("fail WorkUnit");

    let aggregate = runtime.aggregate(&thread.id).await.expect("hot Task");
    assert_eq!(aggregate.facts.issues.len(), 1);
    assert_eq!(
        aggregate.facts.work_units[0].kind(),
        WorkUnitStateKind::Failed
    );
    assert_eq!(
        runtime
            .pending_planner_wakes(Some(&thread.id))
            .await
            .expect("pending wakes")
            .len(),
        1
    );
    let hot_revision = aggregate.hot_revision;
    runtime
        .record_agent_failure(agent_failure(
            &thread.id,
            "executor-faulted",
            "turn-executor-faulted",
            "executor",
            TaskIssueDisposition::Recoverable,
        ))
        .await
        .expect("replay executor fault");
    runtime
        .fail_faulted_executor("executor-faulted", "turn-executor-faulted", "agent faulted")
        .await
        .expect("replay WorkUnit failure");
    assert_eq!(
        runtime.aggregate(&thread.id).await.unwrap().hot_revision,
        hot_revision
    );
    runtime.writer().shutdown().await.expect("shutdown writer");
}

#[tokio::test]
async fn faulted_reviewer_fails_review_and_wakes_healthy_planner() {
    let store = StudioStore::open_memory().await.expect("memory store");
    let workspace = std::env::temp_dir().join("pure-task-runtime-reviewer-fault");
    let project = store.upsert_project(&workspace).await.expect("project");
    let thread = store
        .create_thread(&project.id, "Task", StudioMode::Task)
        .await
        .expect("thread");
    let runtime = TaskRuntime::new(
        store.clone(),
        ProductEventBus::new(
            store.clone(),
            crate::studio::agent_host::ThreadWriteBehindWriter::new(store.clone()),
        ),
    );
    runtime
        .initialize(store.list_active_task_runs().await.expect("active runs"))
        .await
        .expect("initialize runtime");
    runtime
        .create_task(CreateTaskRun {
            project_id: project.id,
            root_thread_id: thread.id.clone(),
            request: "implement".to_string(),
            workspace_root: workspace.to_string_lossy().to_string(),
        })
        .await
        .expect("hot task creation");
    runtime
        .submit_plan(&thread.id, "plan", 0, 0)
        .await
        .expect("submit plan");
    runtime
        .apply_run_command(
            &thread.id,
            1,
            0,
            TaskCommand::ConfirmPlan { plan_revision: 1 },
        )
        .await
        .expect("confirm plan");
    runtime
        .apply_run_command(
            &thread.id,
            2,
            0,
            TaskCommand::FinishDocumentEditing {
                summary: "documents ready".to_string(),
            },
        )
        .await
        .expect("finish document editing");
    let reviewed_head = "head-reviewer-fault".to_string();
    let changed_files = vec!["src/lib.rs".to_string()];
    runtime
        .commit_facts(&thread.id, move |current| {
            let mut facts = current.clone();
            let mut round = new_review_round(
                &facts,
                ReviewScope::Integrated,
                None,
                None,
                None,
                reviewed_head.clone(),
                "start-integrated-review".to_string(),
                changed_files.clone(),
            )?;
            apply_review_command(
                &mut round,
                ReviewRoundCommand::Dispatch {
                    reviewer_thread_id: "reviewer-faulted".to_string(),
                },
                crate::studio::ids::unix_seconds(),
            )?;
            apply_review_command(
                &mut round,
                ReviewRoundCommand::Start {
                    reviewer_thread_id: "reviewer-faulted".to_string(),
                },
                crate::studio::ids::unix_seconds(),
            )?;
            let decision = facts.run.decide(TaskCommand::BeginIntegratedReview {
                target: IntegratedReviewTarget {
                    review_round_id: round.id.clone(),
                    reviewed_head,
                    changed_files,
                },
            })?;
            facts.run.state = decision.next_state;
            facts.reviews.push(round);
            facts.refresh_projection()?;
            Ok(facts)
        })
        .await
        .expect("start integrated reviewer");
    runtime
        .record_agent_failure(agent_failure(
            &thread.id,
            "reviewer-faulted",
            "turn-reviewer-faulted",
            "reviewer",
            TaskIssueDisposition::Recoverable,
        ))
        .await
        .expect("record reviewer fault");
    runtime
        .settle_reviewer_turn_finished(
            "reviewer-faulted",
            &TurnOutcome::failed(pl_protocol::TurnFailure::permanent(
                pl_protocol::TurnFailureCategory::Internal,
                "reviewer faulted",
            )),
        )
        .await
        .expect("fail review");

    let aggregate = runtime.aggregate(&thread.id).await.expect("hot Task");
    assert_eq!(aggregate.facts.issues.len(), 1);
    assert_eq!(
        aggregate.facts.reviews[0].kind(),
        ReviewRoundStateKind::Failed
    );
    assert_eq!(aggregate.facts.run.kind(), TaskRunStateKind::Working);
    assert_eq!(
        runtime
            .pending_planner_wakes(Some(&thread.id))
            .await
            .expect("pending wakes")
            .len(),
        1
    );
    runtime.writer().shutdown().await.expect("shutdown writer");
}

#[tokio::test]
async fn child_fact_commit_advances_run_revision_and_noop_replay_does_not_advance_owner() {
    let store = StudioStore::open_memory().await.expect("memory store");
    let workspace = std::env::temp_dir().join("pure-task-runtime-child-revision");
    let project = store.upsert_project(&workspace).await.expect("project");
    let thread = store
        .create_thread(&project.id, "Task", StudioMode::Task)
        .await
        .expect("thread");
    let events = ProductEventBus::new(
        store.clone(),
        crate::studio::agent_host::ThreadWriteBehindWriter::new(store.clone()),
    );
    let runtime = TaskRuntime::new(store.clone(), events);
    runtime
        .initialize(store.list_active_task_runs().await.expect("active runs"))
        .await
        .expect("initialize runtime");
    let created = runtime
        .create_task(CreateTaskRun {
            project_id: project.id,
            root_thread_id: thread.id.clone(),
            request: "implement".to_string(),
            workspace_root: workspace.to_string_lossy().to_string(),
        })
        .await
        .expect("hot task creation");

    let replay = runtime
        .commit_facts(&thread.id, |current| {
            let mut facts = current.clone();
            facts.run.updated_at = facts.run.updated_at.saturating_add(1);
            Ok(facts)
        })
        .await
        .expect("no-op replay");
    assert_eq!(replay.run.revision, 0);
    assert_eq!(runtime.aggregate(&thread.id).await.unwrap().hot_revision, 1);

    let task_run_id = created.id.clone();
    let now = super::super::ids::unix_seconds();
    let committed = runtime
        .commit_facts(&thread.id, move |current| {
            let mut facts = current.clone();
            facts.work_units.push(WorkUnit {
                context: WorkUnitContext {
                    id: "work-unit-child-revision".to_string(),
                    task_run_id,
                    title: "child fact".to_string(),
                    scope_hints: Vec::new(),
                    blueprint: None,
                    base_commit: "HEAD".to_string(),
                    worktree_path: workspace
                        .join(".pure/worktrees/child")
                        .to_string_lossy()
                        .to_string(),
                    branch: "pure-task-child-revision".to_string(),
                    attempt: 1,
                    supersedes_work_unit_id: None,
                    executor_thread_id: Some("executor-child-revision".to_string()),
                    requested_by_call_id: "spawn-child-revision".to_string(),
                },
                state: WorkUnitState::pending(),
                revision: 0,
                created_at: now,
                updated_at: now,
            });
            facts.refresh_projection()?;
            Ok(facts)
        })
        .await
        .expect("child fact commit");
    assert_eq!(committed.run.revision, 1);
    assert_eq!(runtime.aggregate(&thread.id).await.unwrap().hot_revision, 2);

    runtime
        .await_durable(&thread.id, 2)
        .await
        .expect("child fact durability");
    assert_eq!(
        store
            .read_task_run(&created.id)
            .await
            .expect("read task")
            .expect("task exists")
            .revision,
        1
    );
    assert_eq!(
        store
            .list_work_units(&created.id)
            .await
            .expect("read work units")
            .len(),
        1
    );
    runtime.writer().shutdown().await.expect("shutdown writer");
}

#[tokio::test]
async fn planner_wake_is_computed_and_deduplicated_from_hot_task_facts() {
    let store = StudioStore::open_memory().await.expect("memory store");
    let workspace = std::env::temp_dir().join("pure-task-runtime-hot-planner-wake");
    let project = store.upsert_project(&workspace).await.expect("project");
    let thread = store
        .create_thread(&project.id, "Task", StudioMode::Task)
        .await
        .expect("thread");
    let events = ProductEventBus::new(
        store.clone(),
        crate::studio::agent_host::ThreadWriteBehindWriter::new(store.clone()),
    );
    let runtime = TaskRuntime::new(store.clone(), events);
    runtime
        .initialize(store.list_active_task_runs().await.expect("active runs"))
        .await
        .expect("initialize runtime");
    let run = runtime
        .create_task(CreateTaskRun {
            project_id: project.id,
            root_thread_id: thread.id.clone(),
            request: "implement".to_string(),
            workspace_root: workspace.to_string_lossy().to_string(),
        })
        .await
        .expect("hot task creation");
    let now = crate::studio::ids::unix_seconds();
    let task_run_id = run.id.clone();
    runtime
        .commit_facts(&thread.id, move |current| {
            let mut facts = current.clone();
            let work_unit_id = "work-unit-hot-wake".to_string();
            facts.work_units.push(WorkUnit {
                context: WorkUnitContext {
                    id: work_unit_id.clone(),
                    task_run_id,
                    title: "wake planner".to_string(),
                    scope_hints: Vec::new(),
                    blueprint: None,
                    base_commit: "HEAD".to_string(),
                    worktree_path: workspace
                        .join(".pure/worktrees/wake")
                        .to_string_lossy()
                        .to_string(),
                    branch: "pure-task-hot-wake".to_string(),
                    attempt: 1,
                    supersedes_work_unit_id: None,
                    executor_thread_id: Some("executor-hot-wake".to_string()),
                    requested_by_call_id: "spawn-hot-wake".to_string(),
                },
                state: WorkUnitState::pending(),
                revision: 0,
                created_at: now,
                updated_at: now,
            });
            apply_work_unit_command_in_facts(
                &mut facts,
                &work_unit_id,
                WorkUnitCommand::Activate,
                now,
            )?;
            apply_work_unit_command_in_facts(
                &mut facts,
                &work_unit_id,
                WorkUnitCommand::StartTurn {
                    turn_id: "turn-hot-wake".to_string(),
                    reset_budget: false,
                },
                now,
            )?;
            apply_work_unit_command_in_facts(
                &mut facts,
                &work_unit_id,
                WorkUnitCommand::FinishTurn {
                    outcome: ExecutorTerminalOutcome::Completed {
                        source_turn_id: "turn-hot-wake".to_string(),
                        detail: "executor stopped without a completion".to_string(),
                    },
                },
                now,
            )?;
            facts.refresh_projection()?;
            Ok(facts)
        })
        .await
        .expect("commit terminal executor fact");

    let wakes = runtime
        .pending_planner_wakes(Some(&thread.id))
        .await
        .expect("compute hot wakes");
    assert_eq!(wakes.len(), 1);
    assert!(matches!(
        wakes[0].source,
        TaskPlannerWakeSource::ExecutorTerminal { .. }
    ));
    runtime
        .mark_planner_wake_delivered(&wakes[0])
        .await
        .expect("mark hot wake delivered");
    assert!(
        runtime
            .pending_planner_wakes(Some(&thread.id))
            .await
            .expect("recompute hot wakes")
            .is_empty()
    );
    runtime.writer().shutdown().await.expect("shutdown writer");
}

#[tokio::test]
async fn initialize_loads_only_non_terminal_task_aggregates() {
    let store = StudioStore::open_memory().await.expect("memory store");
    let workspace = std::env::temp_dir().join("pure-task-runtime-active-only");
    let project = store.upsert_project(&workspace).await.expect("project");
    let active = store
        .create_thread(&project.id, "Active", StudioMode::Task)
        .await
        .expect("thread");
    let finished = store
        .create_thread(&project.id, "Finished", StudioMode::Task)
        .await
        .expect("thread");
    for thread in [&active, &finished] {
        store
            .create_task_run(CreateTaskRun {
                project_id: project.id.clone(),
                root_thread_id: thread.id.clone(),
                request: "implement".to_string(),
                workspace_root: workspace.to_string_lossy().to_string(),
            })
            .await
            .expect("task run");
    }
    // 直接把第二个 run 写成终态行（Completed 是六状态中唯一终态）。
    let finished_run = store
        .find_active_task_run_for_root_thread(&finished.id)
        .await
        .expect("query")
        .expect("seeded run");
    let finished_task = store
        .read_task_run(&finished_run.id)
        .await
        .expect("read seeded run")
        .expect("seeded run row");
    let completed = finished_task
        .state
        .complete(super::super::task_coordinator::TaskOutcome::Succeeded {
            summary: "done".to_string(),
            completed_at: 1,
            review_gate: super::super::task_coordinator::TaskReviewGate::NotRequiredNoDelivery,
        })
        .expect("complete decision");
    sea_orm::ConnectionTrait::execute_unprepared(
        store.database(),
        &format!(
            "UPDATE task_runs SET state_json = '{}' WHERE id = '{}'",
            serde_json::to_string(&completed).unwrap(),
            finished_run.id
        ),
    )
    .await
    .unwrap();

    let events = ProductEventBus::new(
        store.clone(),
        crate::studio::agent_host::ThreadWriteBehindWriter::new(store.clone()),
    );
    let runtime = TaskRuntime::new(store.clone(), events);
    runtime
        .initialize(store.list_active_task_runs().await.expect("active runs"))
        .await
        .expect("initialize runtime");

    // 终态 Task 不参与启动装载；它保持冷数据，经显式 activate 可再驻留。
    assert!(runtime.snapshot(&active.id).await.is_some());
    assert!(runtime.snapshot(&finished.id).await.is_none());
    assert!(
        runtime
            .activate(&finished.id)
            .await
            .expect("cold activate")
            .is_some()
    );
}

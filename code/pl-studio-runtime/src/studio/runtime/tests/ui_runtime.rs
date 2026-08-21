use super::*;
use crate::studio::task_coordinator::{
    CreateTaskRun, CreateWorkUnit, ExecutorContinuationState, TaskPlannerWakeSource,
    TaskRunStateKind, WorkUnitState, test_task_git_fingerprint,
};
use crate::{StudioProductEventKind, ThreadVisibility};
use pl_protocol::ThreadItemContent;
use pretty_assertions::assert_eq;
use sea_orm::{ActiveModelTrait, ActiveValue::Set, ConnectionTrait, EntityTrait};

#[tokio::test]
async fn opening_a_project_keeps_an_empty_thread_directory() {
    let workspace = tempfile::tempdir().unwrap();
    let store = StudioStore::open_memory().await.unwrap();
    let runtime = StudioRuntime::new(
        store.clone(),
        ConfigStore::new(crate::config::ConfigPaths::from_home(workspace.path())),
    )
    .unwrap();

    let project = runtime.open_project(workspace.path()).await.unwrap();

    assert!(
        store
            .list_root_threads(&project.id)
            .await
            .unwrap()
            .is_empty()
    );
    assert!(store.list_threads(&project.id).await.unwrap().is_empty());
}

#[tokio::test]
async fn start_new_thread_creates_the_root_only_after_valid_input() {
    let (base_url, handle, accepted_rx, release_tx) = serve_delayed_sse().await;
    let home = tempfile::tempdir().unwrap();
    let workspace = tempfile::tempdir().unwrap();
    let config_store = ConfigStore::new(crate::config::ConfigPaths::from_home(home.path()));
    config_store.save(&test_config(base_url)).unwrap();
    let store = StudioStore::open_memory().await.unwrap();
    let runtime = StudioRuntime::new(store.clone(), config_store).unwrap();
    let project = runtime.open_project(workspace.path()).await.unwrap();

    let empty_error = runtime
        .start_new_thread(StudioStartNewThreadRequest {
            project_id: project.id.clone(),
            title: "New Session".to_string(),
            prompt: "   ".to_string(),
            attachment_ids: Vec::new(),
            mode: StudioMode::Simple,
            options: StudioSubmitPromptOptions::default(),
        })
        .await
        .unwrap_err();
    assert!(empty_error.to_string().contains("prompt is empty"));
    assert!(
        store
            .list_root_threads(&project.id)
            .await
            .unwrap()
            .is_empty()
    );

    let invalid_project_error = runtime
        .start_new_thread(StudioStartNewThreadRequest {
            project_id: "missing-project".to_string(),
            title: "New Session".to_string(),
            prompt: "hello".to_string(),
            attachment_ids: Vec::new(),
            mode: StudioMode::Simple,
            options: StudioSubmitPromptOptions::default(),
        })
        .await
        .unwrap_err();
    assert!(
        invalid_project_error
            .to_string()
            .contains("selected Project not found")
    );
    assert!(
        store
            .list_root_threads(&project.id)
            .await
            .unwrap()
            .is_empty()
    );

    let started = runtime
        .start_new_thread(StudioStartNewThreadRequest {
            project_id: project.id.clone(),
            title: "New Session".to_string(),
            prompt: "hello from the start page".to_string(),
            attachment_ids: Vec::new(),
            mode: StudioMode::Simple,
            options: StudioSubmitPromptOptions::default(),
        })
        .await
        .unwrap();

    assert_eq!(started.thread.project_id, project.id);
    assert_eq!(started.thread.mode, StudioMode::Simple);
    assert_eq!(started.thread.role, "executor");
    assert_eq!(started.submission.thread_id, started.thread.id);
    assert_eq!(store.list_root_threads(&project.id).await.unwrap().len(), 1);
    tokio::time::timeout(TEST_RUNTIME_TIMEOUT, accepted_rx)
        .await
        .unwrap()
        .unwrap();
    let _ = release_tx.send(());
    runtime.shutdown().await;
    handle.await.unwrap();
}

#[tokio::test]
async fn start_new_thread_honors_the_requested_task_mode() {
    let (base_url, handle, accepted_rx, release_tx) = serve_delayed_sse().await;
    let home = tempfile::tempdir().unwrap();
    let workspace = tempfile::tempdir().unwrap();
    let config_store = ConfigStore::new(crate::config::ConfigPaths::from_home(home.path()));
    config_store.save(&test_config(base_url)).unwrap();
    let store = StudioStore::open_memory().await.unwrap();
    let runtime = StudioRuntime::new(store.clone(), config_store).unwrap();
    let project = runtime.open_project(workspace.path()).await.unwrap();

    let started = runtime
        .start_new_thread(StudioStartNewThreadRequest {
            project_id: project.id.clone(),
            title: "New Session".to_string(),
            prompt: "plan something for me".to_string(),
            attachment_ids: Vec::new(),
            mode: StudioMode::Task,
            options: StudioSubmitPromptOptions::default(),
        })
        .await
        .unwrap();

    assert_eq!(started.thread.mode, StudioMode::Task);
    assert_eq!(started.thread.role, "planner");
    let roots = store.list_root_threads(&project.id).await.unwrap();
    assert_eq!(roots.len(), 1);
    assert_eq!(roots[0].mode, StudioMode::Task);
    assert_eq!(roots[0].role, "planner");

    tokio::time::timeout(TEST_RUNTIME_TIMEOUT, accepted_rx)
        .await
        .unwrap()
        .unwrap();
    let _ = release_tx.send(());
    runtime.shutdown().await;
    handle.await.unwrap();
}

#[tokio::test]
async fn start_new_thread_compensates_a_synchronous_submit_failure() {
    let home = tempfile::tempdir().unwrap();
    let workspace = tempfile::tempdir().unwrap();
    let config_store = ConfigStore::new(crate::config::ConfigPaths::from_home(home.path()));
    config_store
        .save(&test_config("http://127.0.0.1:9".to_string()))
        .unwrap();
    let store = StudioStore::open_memory().await.unwrap();
    let runtime = StudioRuntime::new(store.clone(), config_store).unwrap();
    let project = runtime.open_project(workspace.path()).await.unwrap();

    let error = runtime
        .start_new_thread(StudioStartNewThreadRequest {
            project_id: project.id.clone(),
            title: "Compensated session".to_string(),
            prompt: "must not become visible".to_string(),
            attachment_ids: Vec::new(),
            mode: StudioMode::Simple,
            options: StudioSubmitPromptOptions {
                turn_policy: pl_core::AgentTurnSubmitPolicy::SteerOnly,
                ..StudioSubmitPromptOptions::default()
            },
        })
        .await
        .expect_err("an idle new Thread cannot accept a steer-only first prompt");

    assert!(
        error
            .to_string()
            .contains("steerTurn requires an active Turn")
    );
    assert!(
        store
            .list_root_threads(&project.id)
            .await
            .unwrap()
            .is_empty()
    );
    assert!(store.list_threads(&project.id).await.unwrap().is_empty());
    let all_thread_ids = store.list_project_thread_ids(&project.id).await.unwrap();
    assert_eq!(all_thread_ids.len(), 1);
    let compensated_id = &all_thread_ids[0];
    assert_eq!(
        store
            .read_thread(compensated_id)
            .await
            .unwrap()
            .unwrap()
            .visibility,
        ThreadVisibility::Archived
    );
    assert!(!runtime.residency.snapshot().await.contains(compensated_id));
    assert!(
        runtime
            .try_get_thread_handle(compensated_id)
            .await
            .unwrap()
            .is_none()
    );
    runtime.shutdown().await;
}

#[tokio::test]
async fn archive_thread_suggests_next_then_previous_in_directory_order() {
    let workspace = tempfile::tempdir().unwrap();
    let store = StudioStore::open_memory().await.unwrap();
    let runtime = StudioRuntime::new(
        store.clone(),
        ConfigStore::new(crate::config::ConfigPaths::from_home(workspace.path())),
    )
    .unwrap();
    let project = runtime.open_project(workspace.path()).await.unwrap();
    for title in ["First", "Second", "Third"] {
        store
            .create_thread(&project.id, title, StudioMode::Simple)
            .await
            .unwrap();
    }
    let roots = store.list_root_threads(&project.id).await.unwrap();

    let middle = runtime
        .archive_thread(roots[1].id.clone())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        middle.next_root.as_ref().map(|thread| &thread.id),
        Some(&roots[2].id)
    );

    let tail = runtime
        .archive_thread(roots[2].id.clone())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        tail.next_root.as_ref().map(|thread| &thread.id),
        Some(&roots[0].id)
    );

    let last = runtime
        .archive_thread(roots[0].id.clone())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(last.next_root, None);
    assert!(
        store
            .list_root_threads(&project.id)
            .await
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn mode_switch_refreshes_authoritative_thread_snapshot() {
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let workspace = std::env::temp_dir().join(format!("pure-mode-switch-{unique}"));
    std::fs::create_dir_all(&workspace).unwrap();
    let store = StudioStore::open_memory().await.unwrap();
    let runtime = StudioRuntime::new(
        store,
        ConfigStore::new(crate::config::ConfigPaths::from_home(&workspace)),
    )
    .unwrap();
    let project = runtime.open_project(&workspace).await.unwrap();
    let thread = runtime
        .create_thread(&project.id, "Mode switch")
        .await
        .unwrap();

    let initial = runtime.thread_snapshot(&thread.id).await.unwrap();
    assert_eq!(initial.thread.mode, pl_protocol::ThreadMode::Simple);

    runtime
        .set_thread_mode(&thread.id, StudioMode::Task)
        .await
        .unwrap();
    let changed = runtime.thread_snapshot(&thread.id).await.unwrap();

    assert_eq!(changed.thread.mode, pl_protocol::ThreadMode::Task);
    assert_eq!(changed.thread.role, "planner");
    let framework = runtime.agent_framework().await.unwrap();
    let actor = framework
        .handle()
        .snapshot(crate::studio::agent_host::root_agent_id(&changed.thread.id))
        .await
        .unwrap();
    assert_eq!(actor.identity.role, StudioRole::Planner.id());
    let mut subscription = runtime
        .subscribe_thread(pl_protocol::ThreadSubscriptionRequest {
            thread_id: thread.id.clone(),
        })
        .await
        .unwrap();
    assert!(matches!(
        subscription.recv().await,
        Some(pl_protocol::ThreadSubscriptionUpdate::Snapshot { snapshot })
            if snapshot.thread.mode == pl_protocol::ThreadMode::Task
                && snapshot.thread.role == "planner"
    ));
    let child_id = format!("{}-child", thread.id);
    let child = runtime
        .store
        .create_child_thread(crate::studio::ChildThreadSpec {
            id: child_id.clone(),
            parent_thread_id: thread.id,
            agent_path: child_id,
            role: "reviewer".to_string(),
            title: "Mode switch child".to_string(),
        })
        .await
        .unwrap();

    let error = runtime
        .set_thread_mode(&child.id, StudioMode::Simple)
        .await
        .unwrap_err();
    let unchanged_child = runtime.store.read_thread(&child.id).await.unwrap().unwrap();

    assert!(error.to_string().contains("root Thread"));
    assert_eq!(unchanged_child.mode, StudioMode::Task);
    assert_eq!(unchanged_child.role, "reviewer");
    let _ = std::fs::remove_dir_all(workspace);
}

#[tokio::test]
async fn mode_switch_is_rejected_while_a_task_is_active() {
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let workspace = std::env::temp_dir().join(format!("pure-mode-switch-active-{unique}"));
    std::fs::create_dir_all(&workspace).unwrap();
    let store = StudioStore::open_memory().await.unwrap();
    let runtime = StudioRuntime::new(
        store.clone(),
        ConfigStore::new(crate::config::ConfigPaths::from_home(&workspace)),
    )
    .unwrap();
    let project = runtime.open_project(&workspace).await.unwrap();
    let thread = store
        .create_thread(&project.id, "Active task mode switch", StudioMode::Task)
        .await
        .unwrap();
    store
        .create_task_run_with_lease(CreateTaskRun {
            root_thread_id: thread.id.clone(),
            plan: "# Plan\n\nImplement the requested change.".to_string(),
            workspace_root: workspace.to_string_lossy().into_owned(),
            git_common_dir: workspace.join(".git").to_string_lossy().into_owned(),
            branch: "main".to_string(),
            head_commit: "1111111".to_string(),
            design_baseline: test_task_git_fingerprint(
                workspace.to_string_lossy(),
                workspace.join(".git").to_string_lossy(),
                "main",
                "1111111",
            ),
        })
        .await
        .unwrap();

    let error = runtime
        .set_thread_mode(&thread.id, StudioMode::Simple)
        .await
        .unwrap_err();

    assert!(error.to_string().contains("while a task is active"));
    let unchanged = store.read_thread(&thread.id).await.unwrap().unwrap();
    assert_eq!(unchanged.mode, StudioMode::Task);
    assert_eq!(unchanged.role, "planner");
    let _ = std::fs::remove_dir_all(workspace);
}

#[tokio::test]
async fn thread_snapshot_does_not_register_an_inactive_actor() {
    let workspace = tempfile::tempdir().unwrap();
    let store = StudioStore::open_memory().await.unwrap();
    let runtime = StudioRuntime::new(
        store.clone(),
        ConfigStore::new(crate::config::ConfigPaths::from_home(workspace.path())),
    )
    .unwrap();
    let project = runtime.open_project(workspace.path()).await.unwrap();
    let thread = store
        .create_thread(&project.id, "Snapshot", StudioMode::Simple)
        .await
        .unwrap();

    let before = store
        .read_thread_runtime_revision(&thread.id)
        .await
        .unwrap();
    let first = runtime.thread_snapshot(&thread.id).await.unwrap();
    let second = runtime.thread_snapshot(&thread.id).await.unwrap();

    assert_eq!(before, 0);
    assert_eq!(first, second);
    assert_eq!(
        store
            .read_thread_runtime_revision(&thread.id)
            .await
            .unwrap(),
        0
    );
    assert!(
        runtime
            .try_get_thread_handle(&thread.id)
            .await
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn restored_task_root_derives_planner_role_from_mode_at_registration() {
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let workspace = std::env::temp_dir().join(format!("pure-role-derive-{unique}"));
    std::fs::create_dir_all(&workspace).unwrap();
    let store = StudioStore::open_memory().await.unwrap();
    let runtime = StudioRuntime::new(
        store.clone(),
        ConfigStore::new(crate::config::ConfigPaths::from_home(&workspace)),
    )
    .unwrap();
    let project = runtime.open_project(&workspace).await.unwrap();
    let thread = runtime
        .create_thread(&project.id, "Legacy task root")
        .await
        .unwrap();
    let row = crate::studio::entity::thread::Entity::find_by_id(thread.id.clone())
        .one(store.database())
        .await
        .unwrap()
        .unwrap();
    let mut active: crate::studio::entity::thread::ActiveModel = row.into();
    active.mode = Set("task".to_string());
    active.role = Set("executor".to_string());
    active.update(store.database()).await.unwrap();

    runtime.start_runtime().await.unwrap();
    // 惰性驻留：显式激活后按需注册；root 角色按 mode 派生，目录 role 列
    // 由 actor 投影回写，不再依赖启动修复。
    runtime.ensure_thread_agent(&thread.id).await.unwrap();
    let snapshot = runtime.thread_snapshot(&thread.id).await.unwrap();
    let framework = runtime.agent_framework().await.unwrap();
    let actor = framework
        .handle()
        .snapshot(crate::studio::agent_host::root_agent_id(&thread.id))
        .await
        .unwrap();
    let stored = store.read_thread(&thread.id).await.unwrap().unwrap();

    assert_eq!(snapshot.thread.mode, pl_protocol::ThreadMode::Task);
    assert_eq!(snapshot.thread.role, "planner");
    assert_eq!(stored.role, "planner");
    assert_eq!(actor.identity.role, StudioRole::Planner.id());
    let _ = std::fs::remove_dir_all(workspace);
}

#[tokio::test]
async fn restart_thread_registration_materializes_a_missing_durable_planner_wake_once() {
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let workspace = std::env::temp_dir().join(format!("pure-planner-wake-{unique}"));
    std::fs::create_dir_all(&workspace).unwrap();
    let store = StudioStore::open_memory().await.unwrap();
    let project = store.upsert_project(&workspace).await.unwrap();
    let thread = store
        .create_thread(&project.id, "Planner wake", StudioMode::Task)
        .await
        .unwrap();
    let (run, _) = store
        .create_task_run_with_lease(CreateTaskRun {
            root_thread_id: thread.id.clone(),
            plan: "# Plan\n\nImplement the requested change.".to_string(),
            workspace_root: workspace.to_string_lossy().into_owned(),
            git_common_dir: workspace.join(".git").to_string_lossy().into_owned(),
            branch: "main".to_string(),
            head_commit: "1111111".to_string(),
            design_baseline: test_task_git_fingerprint(
                workspace.to_string_lossy(),
                workspace.join(".git").to_string_lossy(),
                "main",
                "1111111",
            ),
        })
        .await
        .unwrap();
    let run = store
        .transition_task_run(
            &run.id,
            TaskRunStateKind::Implementing,
            Some("test design finalized".to_string()),
        )
        .await
        .unwrap();
    let executor_thread_id = format!("{}-executor", thread.id);
    let unit = store
        .create_work_unit(CreateWorkUnit {
            task_run_id: run.id.clone(),
            title: "Implement".to_string(),
            scope_hints: Vec::new(),
            base_commit: run.base_commit.clone(),
            worktree_path: workspace.join("executor").to_string_lossy().into_owned(),
            branch: "task-executor".to_string(),
            attempt: 1,
        })
        .await
        .unwrap();
    let running = WorkUnitState::running(unit.state.clone().into_progress());
    store
        .update_work_unit(&unit.id, running, Some(executor_thread_id.clone()))
        .await
        .unwrap();
    store
        .activate_executor(&unit.id, &executor_thread_id)
        .await
        .unwrap();
    store
        .settle_executor_turn_finished(
            &executor_thread_id,
            &pl_core::AgentTurnOutcome {
                turn_id: pl_core::TurnId::new("turn-terminal").unwrap(),
                thread_id: pl_core::ThreadId::new(executor_thread_id.clone()).unwrap(),
                kind: pl_core::TurnOutcomeKind::Failed,
                reason: Some("executor failed".to_string()),
                failure: None,
                budget_limit: None,
                rollover_compacted: false,
                rollover_compaction_error: None,
                usage: Default::default(),
                finished_at: 1,
            },
        )
        .await
        .unwrap();
    let wake = store
        .list_pending_task_planner_wakes()
        .await
        .unwrap()
        .into_iter()
        .find(|wake| {
            matches!(
                &wake.source,
                TaskPlannerWakeSource::ExecutorTerminal { work_unit_id, .. }
                    if work_unit_id == &unit.id
            )
        })
        .unwrap();
    assert_eq!(
        store
            .read_work_unit(&unit.id)
            .await
            .unwrap()
            .unwrap()
            .continuation_state(),
        ExecutorContinuationState::PlannerWakePending
    );
    assert!(!store.task_planner_wake_was_delivered(&wake).await.unwrap());

    let runtime = StudioRuntime::new(
        store.clone(),
        ConfigStore::new(crate::config::ConfigPaths::from_home(&workspace)),
    )
    .unwrap();
    runtime.start_runtime().await.unwrap();
    runtime.thread_snapshot(&thread.id).await.unwrap();
    assert!(store.task_planner_wake_was_delivered(&wake).await.unwrap());
    let rows = crate::studio::entity::thread_input::Entity::find_by_id(wake.mail_id())
        .all(store.database())
        .await
        .unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].thread_id, thread.id);

    runtime.shutdown().await;
    let _ = std::fs::remove_dir_all(workspace);
}

#[tokio::test]
async fn archive_thread_rejects_child_and_cascades_from_idle_root() {
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let workspace = std::env::temp_dir().join(format!("pure-thread-archive-{unique}"));
    std::fs::create_dir_all(&workspace).unwrap();
    let store = StudioStore::open_memory().await.unwrap();
    let runtime = StudioRuntime::new(
        store.clone(),
        ConfigStore::new(crate::config::ConfigPaths::from_home(&workspace)),
    )
    .unwrap();
    let project = runtime.open_project(&workspace).await.unwrap();
    let root = store
        .create_thread(&project.id, "Archive root", StudioMode::Simple)
        .await
        .unwrap();
    let child_id = format!("{}-child", root.id);
    let child = store
        .create_child_thread(crate::studio::ChildThreadSpec {
            id: child_id.clone(),
            parent_thread_id: root.id.clone(),
            agent_path: child_id,
            role: "executor".to_string(),
            title: "Archive child".to_string(),
        })
        .await
        .unwrap();
    runtime.ensure_thread_agent(&child.id).await.unwrap();
    for thread_id in [&root.id, &child.id] {
        assert!(
            runtime
                .try_get_thread_handle(thread_id)
                .await
                .unwrap()
                .is_some()
        );
    }

    let error = runtime
        .archive_thread(child.id.clone())
        .await
        .expect_err("a child Thread must not be archived directly");
    assert!(error.to_string().contains("root Thread"));
    assert_eq!(store.list_threads(&project.id).await.unwrap().len(), 2);

    let archived = runtime
        .archive_thread(root.id.clone())
        .await
        .unwrap()
        .unwrap();

    assert_eq!(archived.archived_root_id, root.id);
    let mut removed = archived.removed_thread_ids;
    removed.sort();
    let mut expected_removed = vec![root.id.clone(), child.id.clone()];
    expected_removed.sort();
    assert_eq!(removed, expected_removed);
    assert_eq!(archived.next_root, None);
    let roots = store.list_root_threads(&project.id).await.unwrap();
    assert!(roots.is_empty());
    for thread_id in [&root.id, &child.id] {
        assert!(
            runtime
                .try_get_thread_handle(thread_id)
                .await
                .unwrap()
                .is_none()
        );
        assert!(!runtime.residency.snapshot().await.contains(thread_id));
    }
    let _ = std::fs::remove_dir_all(workspace);
}

#[tokio::test]
async fn concurrent_submit_and_archive_are_serialized_by_the_lifecycle_lock() {
    let (base_url, server, accepted_rx, release_tx) = serve_delayed_sse().await;
    let home = tempfile::tempdir().unwrap();
    let workspace = tempfile::tempdir().unwrap();
    let config_store = ConfigStore::new(crate::config::ConfigPaths::from_home(home.path()));
    config_store.save(&test_config(base_url)).unwrap();
    let store = StudioStore::open_memory().await.unwrap();
    let runtime = StudioRuntime::new(store.clone(), config_store).unwrap();
    let project = runtime.open_project(workspace.path()).await.unwrap();
    let thread = store
        .create_thread(&project.id, "Serialized root", StudioMode::Simple)
        .await
        .unwrap();

    let guard = runtime.lifecycle_lock.lock().await;
    let submit_runtime = runtime.clone();
    let submit_thread_id = thread.id.clone();
    let mut submit = tokio::spawn(async move {
        submit_runtime
            .submit_prompt(StudioSubmitPromptRequest {
                thread_id: submit_thread_id,
                prompt: "start before archive".to_string(),
                attachment_ids: Vec::new(),
                options: StudioSubmitPromptOptions::default(),
            })
            .await
    });
    assert!(
        tokio::time::timeout(std::time::Duration::from_millis(25), &mut submit)
            .await
            .is_err(),
        "submit must wait while the lifecycle lock is held"
    );
    let archive_runtime = runtime.clone();
    let archive_thread_id = thread.id.clone();
    let archive =
        tokio::spawn(async move { archive_runtime.archive_thread(archive_thread_id).await });
    drop(guard);

    submit.await.unwrap().unwrap();
    let archive_error = archive
        .await
        .unwrap()
        .expect_err("archive must observe the Turn registered by the earlier submit");
    assert!(
        archive_error
            .to_string()
            .contains("active turn or pending input")
    );
    assert!(store.read_thread(&thread.id).await.unwrap().is_some());
    tokio::time::timeout(TEST_RUNTIME_TIMEOUT, accepted_rx)
        .await
        .unwrap()
        .unwrap();
    let _ = release_tx.send(());
    runtime.shutdown().await;
    server.await.unwrap();
}

#[tokio::test]
async fn active_task_locks_session_mode_and_projects_coordinator_runtime() {
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let home = std::env::temp_dir().join(format!("pure-task-runtime-home-{unique}"));
    let workspace = std::env::temp_dir().join(format!("pure-task-runtime-workspace-{unique}"));
    std::fs::create_dir_all(&workspace).unwrap();
    for arguments in [
        vec!["init"],
        vec!["config", "user.email", "pure@example.com"],
        vec!["config", "user.name", "Pure Tests"],
    ] {
        assert!(
            std::process::Command::new("git")
                .arg("-C")
                .arg(&workspace)
                .args(arguments)
                .status()
                .unwrap()
                .success()
        );
    }
    std::fs::write(workspace.join("README.md"), "task runtime\n").unwrap();
    assert!(
        std::process::Command::new("git")
            .arg("-C")
            .arg(&workspace)
            .args(["add", "README.md"])
            .status()
            .unwrap()
            .success()
    );
    assert!(
        std::process::Command::new("git")
            .arg("-C")
            .arg(&workspace)
            .args(["commit", "-m", "init"])
            .status()
            .unwrap()
            .success()
    );
    let config_store = ConfigStore::new(crate::config::ConfigPaths::from_home(&home));
    let store = StudioStore::open_memory().await.unwrap();
    let runtime = StudioRuntime::new(store.clone(), config_store).unwrap();
    let project = runtime.open_project(&workspace).await.unwrap();
    let session = store
        .create_thread(&project.id, "Task runtime", StudioMode::Task)
        .await
        .unwrap();
    let run = runtime
        .task_coordinator
        .start_confirmed_task(&session.id, "implement task runtime", &workspace)
        .await
        .unwrap();

    let error = runtime
        .set_thread_mode(&session.id, StudioMode::Simple)
        .await
        .unwrap_err();
    let archive_error = runtime
        .archive_thread(session.id.clone())
        .await
        .expect_err("an active Task must prevent Thread archival");
    let task = runtime
        .thread_task_view(&session.id)
        .await
        .unwrap()
        .unwrap();

    assert!(error.to_string().contains("task is active"));
    assert!(archive_error.to_string().contains("task is active"));
    assert_eq!(task.run_id, run.id);
    assert!(matches!(
        task.state,
        crate::protocol::StudioTaskState::DesignUpdating(_)
    ));
    assert_eq!(task.branch, run.branch);
    runtime
        .task_coordinator
        .finish_task(&run.id, TaskRunStateKind::Cancelled, None)
        .await
        .unwrap();
    let _ = std::fs::remove_dir_all(home);
    let _ = std::fs::remove_dir_all(workspace);
}

#[tokio::test]
async fn active_turn_and_pending_input_prevent_thread_mutation() {
    let (base_url, handle, accepted_rx, release_tx) = serve_delayed_sse().await;
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let home = std::env::temp_dir().join(format!("pure-thread-archive-home-{unique}"));
    let workspace = std::env::temp_dir().join(format!("pure-thread-archive-workspace-{unique}"));
    tokio::fs::create_dir_all(&workspace).await.unwrap();
    let config_store = ConfigStore::new(crate::config::ConfigPaths::from_home(&home));
    config_store.save(&test_config(base_url)).unwrap();
    let store = StudioStore::open_memory().await.unwrap();
    let runtime = StudioRuntime::new(store.clone(), config_store).unwrap();
    let project = runtime.open_project(&workspace).await.unwrap();
    let thread = store
        .create_thread(&project.id, "Busy root", StudioMode::Simple)
        .await
        .unwrap();

    runtime
        .submit_prompt(StudioSubmitPromptRequest {
            thread_id: thread.id.clone(),
            prompt: "stay active while archive is attempted".to_string(),
            attachment_ids: Vec::new(),
            options: StudioSubmitPromptOptions::default(),
        })
        .await
        .unwrap();
    tokio::time::timeout(TEST_RUNTIME_TIMEOUT, accepted_rx)
        .await
        .unwrap()
        .unwrap();

    let active_mode_error = runtime
        .set_thread_mode(&thread.id, StudioMode::Task)
        .await
        .expect_err("an active Turn must prevent mode switching");
    assert!(active_mode_error.to_string().contains("Thread is running"));

    runtime
        .submit_prompt(StudioSubmitPromptRequest {
            thread_id: thread.id.clone(),
            prompt: "queue this input behind the active turn".to_string(),
            attachment_ids: Vec::new(),
            options: StudioSubmitPromptOptions::default(),
        })
        .await
        .unwrap();
    let framework = runtime.agent_framework().await.unwrap();
    let snapshot = framework
        .handle()
        .snapshot(crate::studio::agent_host::root_agent_id(&thread.id))
        .await
        .unwrap();
    assert!(snapshot.active_turn_id.is_some());
    assert_eq!(snapshot.pending_inputs, 1);

    let pending_mode_error = runtime
        .set_thread_mode(&thread.id, StudioMode::Task)
        .await
        .expect_err("pending input must prevent mode switching");
    let archive_error = runtime
        .archive_thread(thread.id.clone())
        .await
        .expect_err("an active Turn must prevent Thread archival");

    assert!(pending_mode_error.to_string().contains("pending input"));
    assert!(
        archive_error
            .to_string()
            .contains("active turn or pending input")
    );
    let unchanged = store.read_thread(&thread.id).await.unwrap().unwrap();
    assert_eq!(unchanged.mode, StudioMode::Simple);
    assert_eq!(unchanged.role, "executor");
    assert_eq!(store.list_root_threads(&project.id).await.unwrap().len(), 1);
    let _ = release_tx.send(());
    runtime.shutdown().await;
    handle.await.unwrap();
    let _ = tokio::fs::remove_dir_all(home).await;
    let _ = tokio::fs::remove_dir_all(workspace).await;
}

#[tokio::test]
async fn ui_submit_and_stop_are_core_runtime_apis() {
    let (base_url, handle, accepted_rx, release_tx) = serve_delayed_sse().await;
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let home = std::env::temp_dir().join(format!("pure-ui-runtime-home-{unique}"));
    let workspace = std::env::temp_dir().join(format!("pure-ui-runtime-workspace-{unique}"));
    tokio::fs::create_dir_all(&workspace).await.unwrap();
    let config_store = ConfigStore::new(crate::config::ConfigPaths::from_home(&home));
    config_store.save(&test_config(base_url)).unwrap();
    let store = StudioStore::open_memory().await.unwrap();
    let runtime = StudioRuntime::new(store.clone(), config_store).unwrap();
    let project = runtime.open_project(&workspace).await.unwrap();
    let session = store
        .create_thread(&project.id, "UI runtime", StudioMode::Simple)
        .await
        .unwrap();

    let submitted = runtime
        .submit_prompt(StudioSubmitPromptRequest {
            thread_id: session.id.clone(),
            prompt: "wait until stopped".to_string(),
            attachment_ids: Vec::new(),
            options: StudioSubmitPromptOptions::default(),
        })
        .await
        .unwrap();

    assert_eq!(submitted.thread_id, session.id);
    assert_eq!(runtime.active_turns_for_test().await.len(), 1);
    tokio::time::timeout(TEST_RUNTIME_TIMEOUT, accepted_rx)
        .await
        .unwrap()
        .unwrap();
    let stopped = runtime.stop_prompt(session.id.clone()).await.unwrap();

    assert_eq!(stopped.thread_id, session.id);
    assert!(stopped.stopped);
    let _ = release_tx.send(());
    wait_for_no_active_turn(&runtime).await;
    handle.await.unwrap();
    let _ = tokio::fs::remove_dir_all(home).await;
    let _ = tokio::fs::remove_dir_all(workspace).await;
}

#[tokio::test]
async fn paused_task_resume_submits_one_hidden_durable_input() {
    let (base_url, server, accepted_rx, release_tx) = serve_delayed_sse().await;
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let home = std::env::temp_dir().join(format!("pure-task-resume-home-{unique}"));
    let workspace = std::env::temp_dir().join(format!("pure-task-resume-workspace-{unique}"));
    std::fs::create_dir_all(&workspace).unwrap();
    for arguments in [
        vec!["init", "-b", "main"],
        vec!["config", "user.email", "pure@example.com"],
        vec!["config", "user.name", "Pure Tests"],
    ] {
        assert!(
            std::process::Command::new("git")
                .arg("-C")
                .arg(&workspace)
                .args(arguments)
                .status()
                .unwrap()
                .success()
        );
    }
    std::fs::write(workspace.join("README.md"), "task resume\n").unwrap();
    for arguments in [vec!["add", "README.md"], vec!["commit", "-m", "init"]] {
        assert!(
            std::process::Command::new("git")
                .arg("-C")
                .arg(&workspace)
                .args(arguments)
                .status()
                .unwrap()
                .success()
        );
    }

    let config_store = ConfigStore::new(crate::config::ConfigPaths::from_home(&home));
    config_store.save(&test_config(base_url)).unwrap();
    let store = StudioStore::open_memory().await.unwrap();
    let runtime = StudioRuntime::new(store.clone(), config_store).unwrap();
    let project = runtime.open_project(&workspace).await.unwrap();
    let session = store
        .create_thread(&project.id, "Paused Task", StudioMode::Task)
        .await
        .unwrap();
    let run = runtime
        .task_coordinator
        .start_confirmed_task(&session.id, "resume canonical task", &workspace)
        .await
        .unwrap();
    store
        .update_thread_status(
            &session.id,
            pl_protocol::ThreadStatus::Idle,
            None,
            None,
            crate::studio::ids::unix_seconds(),
        )
        .await
        .unwrap();

    let resumed = runtime.resume_task(session.id.clone()).await.unwrap();
    assert_eq!(resumed.thread_id, session.id);
    tokio::time::timeout(TEST_RUNTIME_TIMEOUT, accepted_rx)
        .await
        .unwrap()
        .unwrap();

    let owner = crate::studio::agent_host::root_agent_id(&session.id);
    let input = store
        .database()
        .query_one_raw(sea_orm::Statement::from_sql_and_values(
            sea_orm::DatabaseBackend::Sqlite,
            "SELECT mail_id, content, metadata_json, presentation
             FROM thread_inputs WHERE thread_id = ? AND state = 'active'",
            [owner.to_string().into()],
        ))
        .await
        .unwrap()
        .unwrap();
    assert!(
        input
            .try_get::<String>("", "mail_id")
            .unwrap()
            .starts_with(&format!("task-resume:{}:", run.id))
    );
    assert_eq!(
        input.try_get::<String>("", "presentation").unwrap(),
        "hidden"
    );
    let metadata: serde_json::Value =
        serde_json::from_str(&input.try_get::<String>("", "metadata_json").unwrap()).unwrap();
    assert_eq!(metadata["kind"], "taskResume");
    assert_eq!(metadata["taskRunId"], run.id);
    assert!(
        input
            .try_get::<String>("", "content")
            .unwrap()
            .contains("Read task_status and list_agents")
    );
    assert!(
        runtime
            .thread_snapshot(&session.id)
            .await
            .unwrap()
            .items
            .iter()
            .all(|item| !matches!(&item.content, ThreadItemContent::UserMessage { .. }))
    );
    runtime
        .resume_task(session.id.clone())
        .await
        .expect_err("a running or no-longer-paused Planner must reject duplicate resume");
    let active_input_count = store
        .database()
        .query_one_raw(sea_orm::Statement::from_sql_and_values(
            sea_orm::DatabaseBackend::Sqlite,
            "SELECT COUNT(*) AS count FROM thread_inputs
             WHERE thread_id = ? AND state = 'active'",
            [owner.to_string().into()],
        ))
        .await
        .unwrap()
        .unwrap()
        .try_get::<i64>("", "count")
        .unwrap();
    assert_eq!(active_input_count, 1);

    let _ = release_tx.send(());
    wait_for_no_active_turn(&runtime).await;
    server.await.unwrap();
    runtime
        .task_coordinator
        .finish_task(&run.id, TaskRunStateKind::Cancelled, None)
        .await
        .unwrap();
    runtime.shutdown().await;
    let _ = std::fs::remove_dir_all(home);
    let _ = std::fs::remove_dir_all(workspace);
}

#[tokio::test]
async fn project_cleanup_closes_active_root_and_quarantines_project() {
    let (base_url, handle, accepted_rx, release_tx) = serve_delayed_sse().await;
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let home = std::env::temp_dir().join(format!("pure-project-cleanup-home-{unique}"));
    let workspace = std::env::temp_dir().join(format!("pure-project-cleanup-workspace-{unique}"));
    tokio::fs::create_dir_all(&workspace).await.unwrap();
    let config_store = ConfigStore::new(crate::config::ConfigPaths::from_home(&home));
    config_store.save(&test_config(base_url)).unwrap();
    let store = StudioStore::open_memory().await.unwrap();
    let runtime = StudioRuntime::new(store.clone(), config_store).unwrap();
    let project = runtime.open_project(&workspace).await.unwrap();
    let session = store
        .create_thread(&project.id, "Project cleanup", StudioMode::Simple)
        .await
        .unwrap();

    runtime
        .submit_prompt(StudioSubmitPromptRequest {
            thread_id: session.id.clone(),
            prompt: "stay active until project cleanup".to_string(),
            attachment_ids: Vec::new(),
            options: StudioSubmitPromptOptions::default(),
        })
        .await
        .unwrap();
    tokio::time::timeout(TEST_RUNTIME_TIMEOUT, accepted_rx)
        .await
        .unwrap()
        .unwrap();

    let preview = runtime.preview_project_cleanup(&project.id).await.unwrap();
    let before_project_revision = runtime
        .product_events()
        .read_project_directory()
        .await
        .unwrap()
        .meta
        .revision;
    let before_thread_revision = runtime
        .product_events()
        .read_thread_directory()
        .await
        .unwrap()
        .meta
        .revision;
    let mut events = runtime.product_events().subscribe();
    runtime
        .cleanup_project(&project.id, &preview.expected_revision)
        .await
        .unwrap();
    let (project_directory, thread_directory) = tokio::time::timeout(TEST_RUNTIME_TIMEOUT, async {
        let project_directory = loop {
            if let StudioProductEventKind::ProjectDirectoryChanged(state) =
                events.recv().await.unwrap().kind
            {
                break state;
            }
        };
        let thread_directory = loop {
            if let StudioProductEventKind::ThreadDirectoryChanged(state) =
                events.recv().await.unwrap().kind
            {
                break state;
            }
        };
        (project_directory, thread_directory)
    })
    .await
    .expect("project cleanup must publish fresh project and Thread directories");
    let framework = runtime.agent_framework().await.unwrap();
    let root_error = framework
        .handle()
        .snapshot(crate::studio::agent_host::root_agent_id(&session.id))
        .await
        .unwrap_err();

    assert!(matches!(
        root_error,
        pl_core::AgentRuntimeError::NotFound(_)
    ));
    assert!(runtime.active_turns_for_test().await.is_empty());
    assert!(runtime.list_projects().await.unwrap().is_empty());
    assert!(project_directory.projects.is_empty());
    // 目录事件是增量 payload：清理项目时受影响 Thread 以 removal 形式发布。
    assert!(thread_directory.removed.contains(&session.id));
    assert!(thread_directory.upserted.is_empty());
    assert!(project_directory.meta.revision > before_project_revision);
    assert!(thread_directory.meta.revision > before_thread_revision);
    assert!(
        store
            .list_pending_interactions(&session.id)
            .await
            .unwrap()
            .is_empty()
    );
    let _ = release_tx.send(());
    handle.await.unwrap();
    let _ = tokio::fs::remove_dir_all(home).await;
    let _ = tokio::fs::remove_dir_all(workspace).await;
}

#[tokio::test]
async fn ui_submit_retries_http_overload_and_completes_session() {
    let overload = serde_json::json!({
        "error": {
            "type": "server_error",
            "code": "server_is_overloaded",
            "message": "Our servers are currently overloaded. Please try again later."
        }
    })
    .to_string();
    let completed = concat!(
        "data: {\"type\":\"response.output_item.added\",\"item\":{\"id\":\"msg_1\",\"type\":\"message\",\"role\":\"assistant\",\"phase\":\"final_answer\"}}\n\n",
        "data: {\"type\":\"response.output_text.delta\",\"item_id\":\"msg_1\",\"delta\":\"done\"}\n\n",
        "data: {\"type\":\"response.output_item.done\",\"item\":{\"id\":\"msg_1\",\"type\":\"message\",\"role\":\"assistant\",\"phase\":\"final_answer\",\"content\":[{\"type\":\"output_text\",\"text\":\"done\"}]}}\n\n",
        "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_1\",\"usage\":{\"input_tokens\":1,\"output_tokens\":1,\"total_tokens\":2}}}\n\n",
        "data: [DONE]\n\n"
    )
    .to_string();
    let (base_url, handle) = serve_http_sequence(vec![
        TestHttpResponse::service_unavailable(overload),
        TestHttpResponse::sse(completed),
    ])
    .await;
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let home = std::env::temp_dir().join(format!("pure-ui-overload-home-{unique}"));
    let workspace = std::env::temp_dir().join(format!("pure-ui-overload-workspace-{unique}"));
    tokio::fs::create_dir_all(&workspace).await.unwrap();
    let config_store = ConfigStore::new(crate::config::ConfigPaths::from_home(&home));
    config_store.save(&test_config(base_url)).unwrap();
    let store = StudioStore::open_memory().await.unwrap();
    let runtime = StudioRuntime::new(store.clone(), config_store).unwrap();
    let project = runtime.open_project(&workspace).await.unwrap();
    let session = store
        .create_thread(&project.id, "UI overload retry", StudioMode::Simple)
        .await
        .unwrap();

    runtime
        .submit_prompt(StudioSubmitPromptRequest {
            thread_id: session.id.clone(),
            prompt: "complete after overload".to_string(),
            attachment_ids: Vec::new(),
            options: StudioSubmitPromptOptions::default(),
        })
        .await
        .unwrap();

    wait_for_no_active_turn(&runtime).await;
    let snapshot = runtime.thread_snapshot(&session.id).await.unwrap();
    let request_count = handle.await.unwrap();

    assert_eq!(request_count, 2);
    assert!(snapshot.active_turn.is_none(), "{snapshot:#?}");
    assert!(snapshot.items.iter().any(|item| {
        matches!(
            &item.content,
            ThreadItemContent::AgentMessage { text, .. } if text == "done"
        )
    }));
    let _ = tokio::fs::remove_dir_all(home).await;
    let _ = tokio::fs::remove_dir_all(workspace).await;
}

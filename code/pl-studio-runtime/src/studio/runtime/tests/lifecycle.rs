use super::*;
use crate::{StudioProductEventKind, StudioRecoveryIssueCategory, StudioRecoveryIssueScope};
use pretty_assertions::assert_eq;
use sea_orm::{ConnectionTrait, DatabaseBackend, Statement};

#[tokio::test]
async fn initialize_runtime_isolates_unavailable_registered_project() {
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let missing_workspace = std::env::temp_dir().join(format!("pure-missing-project-{unique}"));
    let healthy_workspace = std::env::temp_dir().join(format!("pure-healthy-project-{unique}"));
    let home = std::env::temp_dir().join(format!("pure-missing-project-home-{unique}"));
    tokio::fs::create_dir_all(&healthy_workspace).await.unwrap();
    let store = StudioStore::open_memory().await.unwrap();
    let healthy_project = store.upsert_project(&healthy_workspace).await.unwrap();
    let project = store.upsert_project(&missing_workspace).await.unwrap();
    let runtime = StudioRuntime::with_runtime_state(
        store,
        ConfigStore::new(crate::config::ConfigPaths::from_home(&home)),
        StudioRuntimeState::new(),
    );

    let snapshot = runtime.initialize_runtime().await.unwrap();

    assert_eq!(snapshot.status, StudioRuntimeStatus::Ready);
    assert_eq!(snapshot.recovery_issues.len(), 1);
    let issue = &snapshot.recovery_issues[0];
    assert_eq!(issue.scope, StudioRecoveryIssueScope::Project);
    assert_eq!(issue.category, StudioRecoveryIssueCategory::Repository);
    assert_eq!(issue.action, StudioRecoveryIssueAction::RemoveProject);
    assert_eq!(issue.project_id.as_deref(), Some(project.id.as_str()));
    assert_eq!(issue.thread_id, None);
    assert_eq!(issue.task_run_id, None);
    assert!(issue.message.contains("Project workspace is unavailable"));
    assert!(
        issue
            .message
            .contains(&missing_workspace.display().to_string())
    );
    assert_ne!(
        issue.project_id.as_deref(),
        Some(healthy_project.id.as_str())
    );
    let _ = tokio::fs::remove_dir_all(healthy_workspace).await;
    let _ = tokio::fs::remove_dir_all(home).await;
}

#[tokio::test]
async fn open_project_validates_path_before_persisting() {
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!("pure-open-project-{unique}"));
    let missing_workspace = root.join("missing");
    let file_path = root.join("file.txt");
    let valid_workspace = root.join("workspace");
    let home = root.join("home");
    tokio::fs::create_dir_all(&valid_workspace).await.unwrap();
    tokio::fs::write(&file_path, "not a workspace")
        .await
        .unwrap();
    let store = StudioStore::open_memory().await.unwrap();
    let runtime = StudioRuntime::new(
        store.clone(),
        ConfigStore::new(crate::config::ConfigPaths::from_home(&home)),
    );

    let missing_error = runtime.open_project(&missing_workspace).await.unwrap_err();
    let file_error = runtime.open_project(&file_path).await.unwrap_err();

    assert!(
        missing_error
            .to_string()
            .contains("workspace directory not found")
    );
    assert!(
        file_error
            .to_string()
            .contains("workspace path is not a directory")
    );
    assert!(store.list_projects().await.unwrap().is_empty());

    let project = runtime.open_project(&valid_workspace).await.unwrap();

    assert_eq!(project.path, valid_workspace.to_string_lossy());
    assert_eq!(store.list_projects().await.unwrap(), vec![project]);
    let _ = tokio::fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn archive_project_refuses_a_durable_active_task() {
    use crate::studio::task_coordinator::{CreateTaskRun, TaskRunPhase};

    let store = StudioStore::open_memory().await.unwrap();
    let project = store
        .upsert_project("C:/work/archive-active-task")
        .await
        .unwrap();
    let thread = store
        .create_thread(&project.id, "Active task", StudioMode::Task)
        .await
        .unwrap();
    store
        .create_task_run_with_lease(CreateTaskRun {
            root_thread_id: thread.id.clone(),
            phase: TaskRunPhase::Planning,
            plan: "# Plan".to_string(),
            workspace_root: "C:/work/archive-active-task".to_string(),
            git_common_dir: "C:/work/archive-active-task/.git".to_string(),
            branch: "main".to_string(),
            head_commit: "1111111".to_string(),
        })
        .await
        .unwrap();
    let home = std::env::temp_dir().join(format!(
        "pure-archive-active-task-home-{}",
        std::process::id()
    ));
    let runtime = StudioRuntime::new(
        store.clone(),
        ConfigStore::new(crate::config::ConfigPaths::from_home(&home)),
    );

    let error = runtime.archive_project(&project.id).await.unwrap_err();

    assert!(error.to_string().contains("project has an active task"));
    assert_eq!(store.list_projects().await.unwrap(), vec![project]);
    assert_eq!(
        store
            .read_thread(&thread.id)
            .await
            .unwrap()
            .unwrap()
            .visibility,
        crate::studio::records::ThreadVisibility::Active
    );
    let _ = tokio::fs::remove_dir_all(home).await;
}

#[tokio::test]
async fn update_shutdown_refuses_active_task_and_stops_idle_runtime() {
    use crate::studio::task_coordinator::{CreateTaskRun, TaskRunPhase};

    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let busy_store = StudioStore::open_memory().await.unwrap();
    let project = busy_store
        .upsert_project("C:/work/update-busy")
        .await
        .unwrap();
    let session = busy_store
        .create_thread(&project.id, "Update busy", StudioMode::Task)
        .await
        .unwrap();
    busy_store
        .create_task_run_with_lease(CreateTaskRun {
            root_thread_id: session.id,
            phase: TaskRunPhase::Planning,
            plan: "# Plan".to_string(),
            workspace_root: "C:/work/update-busy".to_string(),
            git_common_dir: "C:/work/update-busy/.git".to_string(),
            branch: "main".to_string(),
            head_commit: "1111111".to_string(),
        })
        .await
        .unwrap();
    let busy_home = std::env::temp_dir().join(format!("pure-update-busy-{unique}"));
    let busy_runtime = StudioRuntime::with_runtime_state(
        busy_store,
        ConfigStore::new(crate::config::ConfigPaths::from_home(&busy_home)),
        StudioRuntimeState::new(),
    );

    assert!(
        busy_runtime
            .shutdown_runtime_if_idle()
            .await
            .unwrap()
            .is_none()
    );
    assert_ne!(
        busy_runtime.runtime_snapshot().status,
        StudioRuntimeStatus::Stopped
    );

    let idle_home = std::env::temp_dir().join(format!("pure-update-idle-{unique}"));
    let idle_runtime = StudioRuntime::with_runtime_state(
        StudioStore::open_memory().await.unwrap(),
        ConfigStore::new(crate::config::ConfigPaths::from_home(&idle_home)),
        StudioRuntimeState::new(),
    );
    idle_runtime.initialize_runtime().await.unwrap();
    let stopped = idle_runtime
        .shutdown_runtime_if_idle()
        .await
        .unwrap()
        .expect("idle runtime should stop for update");
    assert_eq!(stopped.status, StudioRuntimeStatus::Stopped);

    let _ = tokio::fs::remove_dir_all(busy_home).await;
    let _ = tokio::fs::remove_dir_all(idle_home).await;
}

#[tokio::test]
async fn failed_task_preflight_keeps_plan_confirmation_pending() {
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let repository = std::env::temp_dir().join(format!("pure-plan-preflight-{unique}"));
    std::fs::create_dir_all(&repository).unwrap();
    for args in [
        vec!["init", "-b", "main"],
        vec!["config", "user.email", "pure@example.com"],
        vec!["config", "user.name", "Pure Tests"],
    ] {
        let status = std::process::Command::new("git")
            .arg("-C")
            .arg(&repository)
            .args(args)
            .status()
            .unwrap();
        assert!(status.success());
    }
    std::fs::write(repository.join("README.md"), "initial\n").unwrap();
    for args in [vec!["add", "README.md"], vec!["commit", "-m", "initial"]] {
        let status = std::process::Command::new("git")
            .arg("-C")
            .arg(&repository)
            .args(args)
            .status()
            .unwrap();
        assert!(status.success());
    }
    std::fs::write(repository.join("dirty.txt"), "dirty\n").unwrap();

    assert_failed_task_preflight_keeps_confirmation_pending(&repository, "dirty").await;
    let _ = std::fs::remove_dir_all(repository);
}

async fn assert_failed_task_preflight_keeps_confirmation_pending(
    repository: &std::path::Path,
    suffix: &str,
) {
    let store = StudioStore::open_memory().await.unwrap();
    let project = store.upsert_project(&repository).await.unwrap();
    let session = store
        .create_thread(&project.id, "Task", StudioMode::Task)
        .await
        .unwrap();
    let interaction = pending_interaction(
        &format!("plan-confirm-{suffix}"),
        &session.id,
        InteractionKind::PlanConfirmation,
        InteractionPayload::PlanConfirmation {
            plan_id: format!("plan-{suffix}"),
            content: "Implement the plan after updating `design/task.md`.".to_string(),
        },
    );
    store.upsert_interaction(&interaction).await.unwrap();
    let home = std::env::temp_dir().join(format!(
        "pure-plan-preflight-home-{suffix}-{}",
        std::process::id()
    ));
    let runtime = StudioRuntime::with_runtime_state(
        store.clone(),
        ConfigStore::new(crate::config::ConfigPaths::from_home(&home)),
        StudioRuntimeState::new(),
    );

    runtime
        .resolve_interaction(
            interaction.interaction_id.clone(),
            crate::InteractionResolution::PlanConfirmation {
                decision: crate::PlanConfirmationResolution::ImplementFreshContext,
                content: None,
                reason: None,
            },
        )
        .await
        .expect_err("repository preparation must fail before resolving confirmation");

    let stored = store
        .read_interaction(&interaction.interaction_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(stored.status, InteractionStatus::Pending);
    assert!(store.list_active_task_runs().await.unwrap().is_empty());
    let _ = std::fs::remove_dir_all(home);
}

#[tokio::test]
async fn initialize_runtime_recovers_user_input_and_cancels_tool_approval() {
    let store = StudioStore::open_memory().await.unwrap();
    let project = store.upsert_project("C:/work/recovered").await.unwrap();
    let session = store
        .create_thread(&project.id, "Recovered", StudioMode::Simple)
        .await
        .unwrap();
    store
        .upsert_interaction(&pending_interaction(
            "ask-recovered",
            &session.id,
            InteractionKind::UserInput,
            InteractionPayload::UserInput {
                questions: Vec::new(),
            },
        ))
        .await
        .unwrap();
    store
        .upsert_interaction(&pending_interaction(
            "approval-recovered",
            &session.id,
            InteractionKind::ToolApproval,
            InteractionPayload::ToolApproval {
                name: "exec".to_string(),
                arguments: serde_json::json!({"command": "echo hi"}),
                working_directory: None,
                parent_agent_id: None,
            },
        ))
        .await
        .unwrap();
    store
        .upsert_interaction(&pending_interaction(
            "plan-recovered",
            &session.id,
            InteractionKind::PlanConfirmation,
            InteractionPayload::PlanConfirmation {
                plan_id: "plan-1".to_string(),
                content: "Plan".to_string(),
            },
        ))
        .await
        .unwrap();
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let home = std::env::temp_dir().join(format!("pure-recovered-runtime-home-{unique}"));
    let runtime = StudioRuntime::with_runtime_state(
        store.clone(),
        ConfigStore::new(crate::config::ConfigPaths::from_home(&home)),
        StudioRuntimeState::new(),
    );

    let snapshot = runtime.initialize_runtime().await.unwrap();

    assert_eq!(snapshot.status, StudioRuntimeStatus::Ready);
    assert_eq!(snapshot.active_turns, Vec::new());
    let ask = store
        .read_interaction("ask-recovered")
        .await
        .unwrap()
        .unwrap();
    let approval = store
        .read_interaction("approval-recovered")
        .await
        .unwrap()
        .unwrap();
    let plan = store
        .read_interaction("plan-recovered")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(ask.status, InteractionStatus::Pending);
    assert_eq!(approval.status, InteractionStatus::Cancelled);
    assert_eq!(plan.status, InteractionStatus::Pending);
    let canonical = runtime.thread_snapshot(&session.id).await.unwrap();
    assert_eq!(canonical.interactions.len(), 2);
    assert!(canonical.interactions.iter().all(|interaction| {
        matches!(
            interaction.kind,
            InteractionKind::UserInput | InteractionKind::PlanConfirmation
        ) && interaction.status == InteractionStatus::Pending
    }));
    let _ = tokio::fs::remove_dir_all(home).await;
}

#[tokio::test]
async fn detached_user_input_resolution_queues_one_hidden_explicit_input() {
    let (base_url, server, accepted_rx, release_tx) = serve_delayed_sse().await;
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let home = std::env::temp_dir().join(format!("pure-detached-input-home-{unique}"));
    let workspace = std::env::temp_dir().join(format!("pure-detached-input-workspace-{unique}"));
    tokio::fs::create_dir_all(&workspace).await.unwrap();
    let config_store = ConfigStore::new(crate::config::ConfigPaths::from_home(&home));
    config_store.save(&test_config(base_url)).unwrap();
    let store = StudioStore::open_memory().await.unwrap();
    let runtime = StudioRuntime::new(store.clone(), config_store);
    let project = runtime.open_project(&workspace).await.unwrap();
    let session = store
        .create_thread(&project.id, "Detached input", StudioMode::Simple)
        .await
        .unwrap();
    let interaction = pending_interaction(
        "ask-detached",
        &session.id,
        InteractionKind::UserInput,
        InteractionPayload::UserInput {
            questions: vec![crate::UserQuestion {
                id: "architecture".to_string(),
                header: "架构".to_string(),
                question: "选择配置边界".to_string(),
                is_other: false,
                is_secret: false,
                options: None,
            }],
        },
    );
    store.upsert_interaction(&interaction).await.unwrap();
    let resolution = crate::InteractionResolution::UserInput {
        answers: std::collections::HashMap::from([(
            "architecture".to_string(),
            crate::UserInputAnswer {
                answers: vec!["typed canonical route".to_string()],
            },
        )]),
    };

    runtime
        .resolve_interaction(interaction.interaction_id.clone(), resolution.clone())
        .await
        .unwrap();
    tokio::time::timeout(TEST_RUNTIME_TIMEOUT, accepted_rx)
        .await
        .unwrap()
        .unwrap();

    let owner = crate::studio::agent_host::root_agent_id(&session.id);
    let row = store
        .database()
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "SELECT mail_id, content, metadata_json, presentation
             FROM thread_inputs WHERE thread_id = ? AND state = 'active'",
            [owner.to_string().into()],
        ))
        .await
        .unwrap()
        .expect("hidden explicit input should be active");
    assert_eq!(
        row.try_get::<String>("", "mail_id").unwrap(),
        format!("interaction-resolution:{}", interaction.interaction_id)
    );
    assert_eq!(row.try_get::<String>("", "presentation").unwrap(), "hidden");
    let metadata: serde_json::Value =
        serde_json::from_str(&row.try_get::<String>("", "metadata_json").unwrap()).unwrap();
    assert_eq!(
        metadata["interactionResolutionId"],
        interaction.interaction_id
    );
    let message: serde_json::Value =
        serde_json::from_str(&row.try_get::<String>("", "content").unwrap()).unwrap();
    assert_eq!(message["type"], "studioInteractionResolution");
    assert_eq!(message["interactionId"], interaction.interaction_id);
    assert_eq!(message["originTurnId"], interaction.scope.turn_id);
    assert_eq!(
        message["resolution"]["answers"]["architecture"]["answers"][0],
        "typed canonical route"
    );
    assert!(
        runtime
            .thread_snapshot(&session.id)
            .await
            .unwrap()
            .items
            .iter()
            .all(|item| !matches!(
                &item.content,
                pl_protocol::ThreadItemContent::UserMessage { .. }
            ))
    );

    runtime
        .resolve_interaction(interaction.interaction_id.clone(), resolution)
        .await
        .unwrap();
    let active_input_count = store
        .database()
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
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
    runtime.shutdown().await;
    let _ = tokio::fs::remove_dir_all(workspace).await;
    let _ = tokio::fs::remove_dir_all(home).await;
}

#[tokio::test]
async fn start_runtime_emits_mcp_health_snapshot() {
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let home = std::env::temp_dir().join(format!("pure-mcp-health-runtime-home-{unique}"));
    let runtime = StudioRuntime::new(
        StudioStore::open_memory().await.unwrap(),
        ConfigStore::new(crate::config::ConfigPaths::from_home(&home)),
    );
    let mut events = runtime.product_events().subscribe();

    runtime.start_runtime().await.unwrap();

    let event = tokio::time::timeout(TEST_RUNTIME_TIMEOUT, events.recv())
        .await
        .unwrap()
        .unwrap();
    let StudioProductEventKind::McpHealthChanged { health } = event.kind else {
        panic!("expected McpHealthChanged event");
    };
    assert!(health.active_mcp_servers.is_empty());
    assert!(health.mcp_servers.iter().any(|server| {
        server.source_kind == "builtIn" && server.availability_kind == "missingCredential"
    }));

    runtime.shutdown().await;
    let _ = tokio::fs::remove_dir_all(home).await;
}

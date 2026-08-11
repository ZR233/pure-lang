use super::*;
use crate::{StudioProductEventKind, StudioRecoveryIssueCategory, StudioRecoveryIssueScope};
use pl_core::canonical_content_hash;
use pl_protocol::{AgentWorkingState, ThreadItem, ThreadItemContent, ThreadItemStatus};
use pretty_assertions::assert_eq;
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, ConnectionTrait, DatabaseBackend, EntityTrait,
    IntoActiveModel, QueryFilter, Statement,
};

use crate::studio::entity::{item, thread, thread_session_state, turn};

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
async fn corrupt_registered_session_is_scoped_and_cleanup_preserves_timeline() {
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!("pure-corrupt-session-{unique}"));
    let workspace = root.join("workspace");
    let home = root.join("home");
    tokio::fs::create_dir_all(&workspace).await.unwrap();
    let store = StudioStore::open_memory().await.unwrap();
    let project = store.upsert_project(&workspace).await.unwrap();
    let broken = store
        .create_thread(&project.id, "Broken session", StudioMode::Simple)
        .await
        .unwrap();
    let healthy = store
        .create_thread(&project.id, "Healthy session", StudioMode::Simple)
        .await
        .unwrap();
    persist_registered_session_state(&store, &broken.id, Some("sha256:corrupt")).await;
    persist_registered_session_state(&store, &healthy.id, None).await;
    persist_completed_user_message(&store, &broken.id).await;
    let runtime = StudioRuntime::with_runtime_state(
        store.clone(),
        ConfigStore::new(crate::config::ConfigPaths::from_home(&home)),
        StudioRuntimeState::new(),
    );

    let snapshot = runtime.initialize_runtime().await.unwrap();

    assert_eq!(snapshot.status, StudioRuntimeStatus::Ready);
    assert_eq!(snapshot.recovery_issues.len(), 1);
    let issue = &snapshot.recovery_issues[0];
    assert_eq!(issue.scope, StudioRecoveryIssueScope::Thread);
    assert_eq!(issue.category, StudioRecoveryIssueCategory::AgentState);
    assert_eq!(issue.action, StudioRecoveryIssueAction::CleanupThread);
    assert_eq!(issue.project_id.as_deref(), Some(project.id.as_str()));
    assert_eq!(issue.thread_id.as_deref(), Some(broken.id.as_str()));
    assert_eq!(issue.task_run_id, None);
    assert!(issue.message.contains("hash mismatch"));
    assert!(runtime.thread_snapshot(&healthy.id).await.is_ok());

    let preview = runtime
        .preview_recovery_issue_cleanup(&issue.id)
        .await
        .unwrap();
    assert_eq!(preview.thread_id.as_deref(), Some(broken.id.as_str()));
    assert!(preview.resources.is_empty());
    let cleaned = runtime
        .cleanup_recovery_issue(&issue.id, &preview.expected_revision)
        .await
        .unwrap();

    assert_eq!(cleaned.status, StudioRuntimeStatus::Ready);
    assert!(cleaned.recovery_issues.is_empty());
    let reset_thread = thread::Entity::find_by_id(&broken.id)
        .one(store.database())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(reset_thread.runtime_revision, None);
    assert_eq!(reset_thread.status, "idle");
    let reset_state = thread_session_state::Entity::find_by_id(&broken.id)
        .one(store.database())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(reset_state.revision, 0);
    assert_eq!(
        reset_state.state_hash,
        canonical_content_hash(reset_state.state_json.as_bytes())
    );
    let history = runtime
        .list_thread_turns(&broken.id, None, 10)
        .await
        .unwrap();
    assert!(
        history
            .turns
            .iter()
            .flat_map(|turn| &turn.items)
            .any(|item| {
                matches!(
                    &item.content,
                    ThreadItemContent::UserMessage { text, .. } if text == "preserve this history"
                )
            })
    );
    store
        .reset_agent_sessions_for_root(&broken.id)
        .await
        .unwrap();

    let restarted = StudioRuntime::with_runtime_state(
        store,
        ConfigStore::new(crate::config::ConfigPaths::from_home(&home)),
        StudioRuntimeState::new(),
    )
    .initialize_runtime()
    .await
    .unwrap();
    assert_eq!(restarted.status, StudioRuntimeStatus::Ready);
    assert!(restarted.recovery_issues.is_empty());
    let _ = tokio::fs::remove_dir_all(root).await;
}

async fn persist_registered_session_state(
    store: &StudioStore,
    thread_id: &str,
    hash_override: Option<&str>,
) {
    let state = AgentWorkingState::default();
    let state_json = serde_json::to_string(&state).unwrap();
    thread_session_state::ActiveModel {
        thread_id: Set(thread_id.to_string()),
        revision: Set(0),
        state_hash: Set(hash_override.map_or_else(
            || canonical_content_hash(state_json.as_bytes()),
            str::to_string,
        )),
        state_json: Set(state_json),
        updated_at: Set(1),
    }
    .insert(store.database())
    .await
    .unwrap();
    let model = thread::Entity::find_by_id(thread_id)
        .one(store.database())
        .await
        .unwrap()
        .unwrap();
    let mut active = model.into_active_model();
    active.runtime_revision = Set(Some(1));
    active.update(store.database()).await.unwrap();
}

async fn persist_completed_user_message(store: &StudioStore, thread_id: &str) {
    let turn_id = format!("turn-history-{thread_id}");
    turn::ActiveModel {
        id: Set(turn_id.clone()),
        thread_id: Set(thread_id.to_string()),
        ordinal: Set(1),
        revision: Set(1),
        status: Set("completed".to_string()),
        phase: Set(None),
        reason: Set(None),
        model_json: Set(None),
        usage_json: Set(serde_json::to_string(&pl_model::TokenUsage::default()).unwrap()),
        failure_json: Set(None),
        budget_limit_json: Set(None),
        rollover_compacted: Set(0),
        rollover_compaction_error: Set(None),
        metadata_json: Set(None),
        started_at: Set(Some(1)),
        updated_at: Set(1),
        completed_at: Set(Some(1)),
    }
    .insert(store.database())
    .await
    .unwrap();
    let item_id = format!("item-history-{thread_id}");
    let value = ThreadItem {
        id: item_id.clone(),
        thread_id: thread_id.to_string(),
        turn_id: turn_id.clone(),
        ordinal: 1,
        revision: 1,
        status: ThreadItemStatus::Completed,
        created_at: 1,
        updated_at: 1,
        completed_at: Some(1),
        error: None,
        content: ThreadItemContent::UserMessage {
            text: "preserve this history".to_string(),
            attachments: Vec::new(),
        },
        usage: None,
    };
    item::ActiveModel {
        id: Set(item_id),
        thread_id: Set(thread_id.to_string()),
        turn_id: Set(turn_id),
        ordinal: Set(1),
        revision: Set(1),
        item_kind: Set("userMessage".to_string()),
        status: Set("completed".to_string()),
        payload_json: Set(serde_json::to_string(&value).unwrap()),
        created_at: Set(1),
        updated_at: Set(1),
        completed_at: Set(Some(1)),
    }
    .insert(store.database())
    .await
    .unwrap();
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
    let _ = runtime.ensure_thread_agent(&session.id).await.unwrap();
    runtime
        .interactions
        .create(
            interaction.clone(),
            runtime.interaction_emitter(session.id.clone()),
        )
        .await
        .unwrap();
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
    let persisted_interaction = store
        .read_interaction(&interaction.interaction_id)
        .await
        .unwrap()
        .expect("resolved interaction should remain persisted");
    assert_eq!(persisted_interaction.status, InteractionStatus::Resolved);
    assert_eq!(persisted_interaction.resolution, Some(resolution.clone()));
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

    let _ = release_tx.send(());
    wait_for_no_active_turn(&runtime).await;
    server.await.unwrap();
    runtime
        .resolve_interaction(interaction.interaction_id.clone(), resolution)
        .await
        .unwrap();
    let input_count = store
        .database()
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "SELECT COUNT(*) AS count FROM thread_inputs
             WHERE thread_id = ? AND mail_id = ?",
            [
                owner.to_string().into(),
                format!("interaction-resolution:{}", interaction.interaction_id).into(),
            ],
        ))
        .await
        .unwrap()
        .unwrap()
        .try_get::<i64>("", "count")
        .unwrap();
    assert_eq!(input_count, 1);
    runtime.shutdown().await;
    let _ = tokio::fs::remove_dir_all(workspace).await;
    let _ = tokio::fs::remove_dir_all(home).await;
}

#[tokio::test]
async fn request_user_input_ends_origin_turn_and_continues_in_fresh_turn() {
    let question_response = responses_function_tool_sse(
        "studio-user-input",
        "request_user_input",
        serde_json::json!({
            "questions": [{
                "id": "architecture",
                "header": "架构",
                "question": "选择配置边界",
                "options": [{
                    "label": "typed canonical route",
                    "description": "Use the canonical typed route."
                }]
            }]
        }),
    );
    let final_response = responses_final_text_sse(
        "after-interaction",
        "已在新的 Turn 中继续处理 typed canonical route",
    );
    let (base_url, server) = serve_http_sequence(vec![
        TestHttpResponse::sse(question_response),
        TestHttpResponse::sse(final_response),
    ])
    .await;
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!("pure-user-input-turn-boundary-{unique}"));
    let home = root.join("home");
    let workspace = root.join("workspace");
    tokio::fs::create_dir_all(&workspace).await.unwrap();
    let config_store = ConfigStore::new(crate::config::ConfigPaths::from_home(&home));
    config_store.save(&test_config(base_url)).unwrap();
    let store = StudioStore::open_memory().await.unwrap();
    let runtime = StudioRuntime::new(store.clone(), config_store);
    let project = runtime.open_project(&workspace).await.unwrap();
    let thread = runtime
        .create_thread(&project.id, "User input Turn boundary")
        .await
        .unwrap();
    let mut subscription = runtime
        .subscribe_thread(pl_protocol::ThreadSubscriptionRequest {
            thread_id: thread.id.clone(),
        })
        .await
        .unwrap();
    let _ = subscription.recv().await;

    let submitted = runtime
        .submit_prompt(StudioSubmitPromptRequest {
            thread_id: thread.id.clone(),
            prompt: "先询问架构边界，再根据答复继续".to_string(),
            attachment_ids: Vec::new(),
            options: StudioSubmitPromptOptions::default(),
        })
        .await
        .unwrap();
    let interaction = tokio::time::timeout(TEST_RUNTIME_TIMEOUT, async {
        loop {
            let Some(update) = subscription.recv().await else {
                panic!("Thread subscription closed before InteractionChanged");
            };
            if let pl_protocol::ThreadSubscriptionUpdate::Notification { notification } = update
                && let pl_protocol::ThreadNotification::InteractionChanged { interaction } =
                    notification.notification
                && interaction.status == InteractionStatus::Pending
            {
                break *interaction;
            }
        }
    })
    .await
    .unwrap();
    assert_eq!(interaction.scope.turn_id, submitted.turn_id);
    let resolution = crate::InteractionResolution::UserInput {
        answers: std::collections::HashMap::from([(
            "architecture".to_string(),
            crate::UserInputAnswer {
                answers: vec!["typed canonical route".to_string()],
            },
        )]),
    };

    runtime
        .resolve_interaction(interaction.interaction_id.clone(), resolution)
        .await
        .unwrap();
    tokio::time::timeout(TEST_RUNTIME_TIMEOUT, server)
        .await
        .unwrap()
        .unwrap();
    wait_for_no_active_turn(&runtime).await;

    let stored_interaction = store
        .read_interaction(&interaction.interaction_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(stored_interaction.status, InteractionStatus::Resolved);
    let origin = turn::Entity::find_by_id(&submitted.turn_id)
        .one(store.database())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(origin.status, "completed");
    assert_eq!(origin.budget_limit_json, None);
    let turns = turn::Entity::find()
        .filter(turn::Column::ThreadId.eq(&thread.id))
        .all(store.database())
        .await
        .unwrap();
    assert_eq!(turns.len(), 2);
    assert!(turns.iter().any(|turn| turn.id != submitted.turn_id));
    let snapshot = runtime.thread_snapshot(&thread.id).await.unwrap();
    assert!(snapshot.items.iter().any(|item| {
        matches!(
            &item.content,
            ThreadItemContent::AgentMessage { text, .. }
                if text.contains("新的 Turn") && text.contains("typed canonical route")
        )
    }));
    let owner = crate::studio::agent_host::root_agent_id(&thread.id);
    let input = store
        .database()
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "SELECT state, presentation FROM thread_inputs
             WHERE thread_id = ? AND mail_id = ?",
            [
                owner.to_string().into(),
                pl_core::AgentInteractionContinuationRequest::stable_mail_id(
                    &interaction.interaction_id,
                )
                .into(),
            ],
        ))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(input.try_get::<String>("", "state").unwrap(), "consumed");
    assert_eq!(
        input.try_get::<String>("", "presentation").unwrap(),
        "hidden"
    );

    runtime.shutdown().await;
    let _ = tokio::fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn task_root_user_input_boundary_completes_without_plan_exit() {
    let question_response = responses_function_tool_sse(
        "task-root-user-input",
        "request_user_input",
        serde_json::json!({
            "questions": [{
                "id": "architecture",
                "header": "架构",
                "question": "选择 Task 架构边界",
                "options": [{
                    "label": "durable fresh turn",
                    "description": "Continue the Task in a fresh Turn after resolution."
                }]
            }]
        }),
    );
    let (base_url, server) =
        serve_http_sequence(vec![TestHttpResponse::sse(question_response)]).await;
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!("pure-task-user-input-boundary-{unique}"));
    let home = root.join("home");
    let workspace = root.join("workspace");
    tokio::fs::create_dir_all(&workspace).await.unwrap();
    let config_store = ConfigStore::new(crate::config::ConfigPaths::from_home(&home));
    config_store.save(&test_config(base_url)).unwrap();
    let store = StudioStore::open_memory().await.unwrap();
    let runtime = StudioRuntime::new(store.clone(), config_store);
    let project = runtime.open_project(&workspace).await.unwrap();
    let thread = runtime
        .create_thread(&project.id, "Task user input boundary")
        .await
        .unwrap();
    runtime
        .set_thread_mode(&thread.id, StudioMode::Task)
        .await
        .unwrap();
    let mut subscription = runtime
        .subscribe_thread(pl_protocol::ThreadSubscriptionRequest {
            thread_id: thread.id.clone(),
        })
        .await
        .unwrap();
    let _ = subscription.recv().await;

    let submitted = runtime
        .submit_prompt(StudioSubmitPromptRequest {
            thread_id: thread.id.clone(),
            prompt: "先询问 Task 架构边界，再继续规划".to_string(),
            attachment_ids: Vec::new(),
            options: StudioSubmitPromptOptions::default(),
        })
        .await
        .unwrap();
    let interaction = tokio::time::timeout(TEST_RUNTIME_TIMEOUT, async {
        loop {
            let Some(update) = subscription.recv().await else {
                panic!("Thread subscription closed before InteractionChanged");
            };
            if let pl_protocol::ThreadSubscriptionUpdate::Notification { notification } = update
                && let pl_protocol::ThreadNotification::InteractionChanged { interaction } =
                    notification.notification
                && interaction.status == InteractionStatus::Pending
            {
                break *interaction;
            }
        }
    })
    .await
    .unwrap();
    assert_eq!(interaction.scope.turn_id, submitted.turn_id);

    tokio::time::timeout(TEST_RUNTIME_TIMEOUT, server)
        .await
        .unwrap()
        .unwrap();
    wait_for_no_active_turn(&runtime).await;

    let origin = turn::Entity::find_by_id(&submitted.turn_id)
        .one(store.database())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(origin.status, "completed");
    assert_eq!(origin.failure_json, None);
    assert_eq!(origin.budget_limit_json, None);
    assert_eq!(
        store
            .read_interaction(&interaction.interaction_id)
            .await
            .unwrap()
            .unwrap()
            .status,
        InteractionStatus::Pending
    );

    runtime.shutdown().await;
    let _ = tokio::fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn restarted_pending_user_input_resolves_once_with_stable_mail() {
    let (base_url, server, accepted_rx, release_tx) = serve_delayed_sse().await;
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!("pure-restarted-user-input-{unique}"));
    let home = root.join("home");
    let workspace = root.join("workspace");
    tokio::fs::create_dir_all(&workspace).await.unwrap();
    let config_store = ConfigStore::new(crate::config::ConfigPaths::from_home(&home));
    config_store.save(&test_config(base_url)).unwrap();
    let store = StudioStore::open_memory().await.unwrap();
    let first = StudioRuntime::new(store.clone(), config_store.clone());
    let project = first.open_project(&workspace).await.unwrap();
    let thread = first
        .create_thread(&project.id, "Restarted user input")
        .await
        .unwrap();
    let interaction = pending_interaction(
        "ask-after-restart",
        &thread.id,
        InteractionKind::UserInput,
        InteractionPayload::UserInput {
            questions: Vec::new(),
        },
    );
    let _ = first.ensure_thread_agent(&thread.id).await.unwrap();
    first
        .interactions
        .create(
            interaction.clone(),
            first.interaction_emitter(thread.id.clone()),
        )
        .await
        .unwrap();
    first.shutdown().await;

    let restarted = StudioRuntime::new(store.clone(), config_store);
    restarted.initialize_runtime().await.unwrap();
    let recovered = restarted.thread_snapshot(&thread.id).await.unwrap();
    assert!(recovered.interactions.iter().any(|candidate| {
        candidate.interaction_id == interaction.interaction_id
            && candidate.status == InteractionStatus::Pending
    }));
    let resolution = crate::InteractionResolution::UserInput {
        answers: Default::default(),
    };
    restarted
        .resolve_interaction(interaction.interaction_id.clone(), resolution.clone())
        .await
        .unwrap();
    tokio::time::timeout(TEST_RUNTIME_TIMEOUT, accepted_rx)
        .await
        .unwrap()
        .unwrap();
    let _ = release_tx.send(());
    wait_for_no_active_turn(&restarted).await;
    server.await.unwrap();

    restarted
        .resolve_interaction(interaction.interaction_id.clone(), resolution)
        .await
        .unwrap();
    let owner = crate::studio::agent_host::root_agent_id(&thread.id);
    let row = store
        .database()
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "SELECT COUNT(*) AS count, MAX(state) AS state FROM thread_inputs
             WHERE thread_id = ? AND mail_id = ?",
            [
                owner.to_string().into(),
                pl_core::AgentInteractionContinuationRequest::stable_mail_id(
                    &interaction.interaction_id,
                )
                .into(),
            ],
        ))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(row.try_get::<i64>("", "count").unwrap(), 1);
    assert_eq!(row.try_get::<String>("", "state").unwrap(), "consumed");
    assert_eq!(
        store
            .read_interaction(&interaction.interaction_id)
            .await
            .unwrap()
            .unwrap()
            .status,
        InteractionStatus::Resolved
    );

    restarted.shutdown().await;
    let _ = tokio::fs::remove_dir_all(root).await;
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

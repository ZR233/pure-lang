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
use crate::studio::task_coordinator::TaskRunStateKind;

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
    )
    .unwrap();

    let snapshot = runtime.initialize_runtime().await.unwrap();

    assert_eq!(snapshot.status, StudioRuntimeStatus::Ready);
    let recovery_issues = runtime.recovery_issues();
    assert_eq!(recovery_issues.len(), 1);
    let issue = &recovery_issues[0];
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
    let preview = runtime
        .preview_recovery_issue_cleanup(&issue.id)
        .await
        .unwrap();
    let cleaned = tokio::time::timeout(
        TEST_RUNTIME_TIMEOUT,
        runtime.cleanup_recovery_issue(&issue.id, &preview.expected_revision),
    )
    .await
    .expect("RemoveProject recovery cleanup must not re-enter the lifecycle lock")
    .unwrap();

    assert_eq!(cleaned.status, StudioRuntimeStatus::Ready);
    assert!(runtime.recovery_issues().is_empty());
    assert_eq!(
        runtime.list_projects().await.unwrap(),
        vec![healthy_project]
    );
    let _ = tokio::fs::remove_dir_all(healthy_workspace).await;
    let _ = tokio::fs::remove_dir_all(home).await;
}

#[tokio::test]
async fn start_runtime_registers_persisted_child_thread_identity() {
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!("pure-persisted-child-{unique}"));
    let workspace = root.join("workspace");
    let home = root.join("home");
    tokio::fs::create_dir_all(&workspace).await.unwrap();
    let store = StudioStore::open_memory().await.unwrap();
    let project = store.upsert_project(&workspace).await.unwrap();
    let root_thread = store
        .create_thread(&project.id, "Root session", StudioMode::Task)
        .await
        .unwrap();
    let child_id = format!("{}-child", root_thread.id);
    let child = store
        .create_child_thread(crate::studio::ChildThreadSpec {
            id: child_id.clone(),
            parent_thread_id: root_thread.id.clone(),
            agent_path: child_id,
            role: "executor".to_string(),
            title: "Persisted child".to_string(),
        })
        .await
        .unwrap();
    let runtime = StudioRuntime::with_runtime_state(
        store,
        ConfigStore::new(crate::config::ConfigPaths::from_home(&home)),
        StudioRuntimeState::new(),
    )
    .unwrap();

    let snapshot = runtime.start_runtime().await.unwrap();

    assert_eq!(snapshot.status, StudioRuntimeStatus::Ready);
    // 惰性驻留：持久化 child 在显式激活后按需注册，身份链保持不变。
    runtime.ensure_thread_agent(&child.id).await.unwrap();
    let framework = runtime.agent_framework().await.unwrap();
    let child_agent = framework
        .handle()
        .snapshot(pl_core::ThreadId::new(child.id).unwrap())
        .await
        .unwrap();
    assert_eq!(
        child_agent.identity.parent_id,
        Some(crate::studio::agent_host::root_agent_id(&root_thread.id))
    );
    runtime.shutdown().await;
    let _ = tokio::fs::remove_dir_all(root).await;
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
    )
    .unwrap();

    let snapshot = runtime.initialize_runtime().await.unwrap();

    assert_eq!(snapshot.status, StudioRuntimeStatus::Ready);
    let recovery_issues = runtime.recovery_issues();
    assert_eq!(recovery_issues.len(), 1);
    let issue = &recovery_issues[0];
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
    assert!(runtime.recovery_issues().is_empty());
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
    .unwrap();
    let restarted_snapshot = restarted.initialize_runtime().await.unwrap();
    assert_eq!(restarted_snapshot.status, StudioRuntimeStatus::Ready);
    assert!(restarted.recovery_issues().is_empty());
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
    )
    .unwrap();

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
    use crate::studio::task_coordinator::CreateTaskRun;

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
            project_id: project.id.clone(),
            root_thread_id: thread.id.clone(),
            plan: "# Plan".to_string(),
            workspace_root: "C:/work/archive-active-task".to_string(),
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
    )
    .unwrap();

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
    use crate::studio::task_coordinator::CreateTaskRun;

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
            project_id: project.id.clone(),
            root_thread_id: session.id,
            plan: "# Plan".to_string(),
            workspace_root: "C:/work/update-busy".to_string(),
        })
        .await
        .unwrap();
    let busy_home = std::env::temp_dir().join(format!("pure-update-busy-{unique}"));
    let busy_runtime = StudioRuntime::with_runtime_state(
        busy_store,
        ConfigStore::new(crate::config::ConfigPaths::from_home(&busy_home)),
        StudioRuntimeState::new(),
    )
    .unwrap();

    assert!(
        busy_runtime
            .shutdown_runtime_if_idle()
            .await
            .unwrap()
            .is_none()
    );
    assert_ne!(
        busy_runtime.runtime_snapshot().await.unwrap().status,
        StudioRuntimeStatus::Stopped
    );

    let idle_home = std::env::temp_dir().join(format!("pure-update-idle-{unique}"));
    let idle_runtime = StudioRuntime::with_runtime_state(
        StudioStore::open_memory().await.unwrap(),
        ConfigStore::new(crate::config::ConfigPaths::from_home(&idle_home)),
        StudioRuntimeState::new(),
    )
    .unwrap();
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
    )
    .unwrap();

    let snapshot = runtime.initialize_runtime().await.unwrap();

    assert_eq!(snapshot.status, StudioRuntimeStatus::Ready);
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
    let runtime = StudioRuntime::new(store.clone(), config_store).unwrap();
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
        .interactions()
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
    let runtime = StudioRuntime::new(store.clone(), config_store).unwrap();
    let project = runtime.open_project(&workspace).await.unwrap();
    let thread = runtime
        .create_thread(&project.id, "User input Turn boundary")
        .await
        .unwrap();
    runtime.start_runtime().await.unwrap();
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
    let (base_url, server, mut requests) =
        serve_http_sequence_recording(vec![TestHttpResponse::sse(question_response)]).await;
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
    let runtime = StudioRuntime::new(store.clone(), config_store).unwrap();
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
    let request = requests.recv().await.unwrap();
    let tool_names = response_tool_names(&request);
    assert!(
        tool_names.contains(&"plan_exit"),
        "planning request omitted plan_exit: {tool_names:?}"
    );
    for unavailable in [
        "exec",
        "write_stdin",
        "search_files",
        "task_status",
        "task_spawn_executor",
        "task_finalize_design",
        "task_record_merge",
        "task_request_delivery_review",
        "task_request_integrated_review",
        "task_complete",
        "task_stop",
    ] {
        assert!(
            !tool_names.contains(&unavailable),
            "planning request unexpectedly exposed {unavailable}: {tool_names:?}"
        );
    }
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
async fn fresh_task_run_turn_exposes_normal_tools_and_finalizes_without_edits() {
    let design_update = responses_function_tool_sse(
        "active-task-design",
        "task_finalize_design",
        serde_json::json!({
            "summary": "No repository edits are needed before implementation."
        }),
    );
    let design_ack = responses_final_text_sse("active-task-design-ack", "TaskRun design updated");
    let (base_url, server, mut requests) = serve_http_sequence_recording(vec![
        TestHttpResponse::sse(design_update),
        TestHttpResponse::sse(design_ack),
    ])
    .await;
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!("pure-active-task-tools-{unique}"));
    let home = root.join("home");
    let workspace = root.join("workspace");
    tokio::fs::create_dir_all(&workspace).await.unwrap();
    for args in [
        vec!["init", "-b", "main"],
        vec!["config", "user.email", "pure@example.com"],
        vec!["config", "user.name", "Pure Test"],
    ] {
        assert!(
            std::process::Command::new("git")
                .arg("-C")
                .arg(&workspace)
                .args(args)
                .status()
                .unwrap()
                .success()
        );
    }
    std::fs::write(workspace.join("README.md"), "active task tools\n").unwrap();
    for args in [vec!["add", "README.md"], vec!["commit", "-m", "init"]] {
        assert!(
            std::process::Command::new("git")
                .arg("-C")
                .arg(&workspace)
                .args(args)
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
    let thread = store
        .create_thread(&project.id, "Active Task tools", StudioMode::Task)
        .await
        .unwrap();
    let run = runtime
        .task_coordinator
        .start_confirmed_task(&thread.id, "implement active task tools", &workspace)
        .await
        .unwrap();

    runtime
        .submit_prompt(StudioSubmitPromptRequest {
            thread_id: thread.id.clone(),
            prompt: "读取当前 Task 状态".to_string(),
            attachment_ids: Vec::new(),
            options: StudioSubmitPromptOptions::default(),
        })
        .await
        .unwrap();
    tokio::time::timeout(TEST_RUNTIME_TIMEOUT, server)
        .await
        .unwrap()
        .unwrap();
    wait_for_no_active_turn(&runtime).await;

    let request = requests.recv().await.unwrap();
    let tool_names = response_tool_names(&request);
    for required in ["task_status", "task_finalize_design", "task_spawn_executor"] {
        assert!(
            tool_names.contains(&required),
            "active Task request omitted {required}: {tool_names:?}"
        );
    }
    for available in ["exec", "write_stdin"] {
        assert!(
            tool_names.contains(&available),
            "active Task request omitted {available}: {tool_names:?}"
        );
    }
    assert!(!tool_names.contains(&"search_files"));
    let acknowledgement = requests.recv().await.unwrap();
    let acknowledgement_tools = response_tool_names(&acknowledgement);
    assert!(acknowledgement_tools.contains(&"exec"));
    assert!(acknowledgement_tools.contains(&"write_stdin"));
    let durable = store.read_task_run(&run.id).await.unwrap().unwrap();
    assert_eq!(durable.id, run.id);
    assert_eq!(durable.root_thread_id, thread.id);
    assert_eq!(durable.kind(), TaskRunStateKind::Implementing);
    assert!(durable.design_summary().is_some());

    runtime.shutdown().await;
    let _ = tokio::fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn plan_implementation_continues_in_a_fresh_task_planner_turn() {
    let plan_content = "# 计划\n\n1. 更新 design/task.md。\n2. 启动 executor。";
    let initial_plan_response = responses_function_tool_sse(
        "implementation-plan",
        "plan_exit",
        serde_json::json!({ "content": plan_content }),
    );
    let initial_ack = responses_final_text_sse("implementation-plan-ack", "计划已提交确认");
    let implementation_design = responses_function_tool_sse(
        "implementation-fresh-turn",
        "task_finalize_design",
        serde_json::json!({
            "summary": "The confirmed plan is sufficient without repository edits."
        }),
    );
    let implementation_ack = responses_final_text_sse(
        "implementation-fresh-turn-ack",
        "已在 fresh Task planner Turn 中提交设计并继续。",
    );
    let (base_url, server, mut requests) = serve_http_sequence_recording(vec![
        TestHttpResponse::sse(initial_plan_response),
        TestHttpResponse::sse(initial_ack),
        TestHttpResponse::sse(implementation_design),
        TestHttpResponse::sse(implementation_ack),
    ])
    .await;
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!("pure-plan-implementation-{unique}"));
    let home = root.join("home");
    let workspace = root.join("workspace");
    tokio::fs::create_dir_all(&workspace).await.unwrap();
    for args in [
        vec!["init", "-b", "main"],
        vec!["config", "user.email", "pure@example.com"],
        vec!["config", "user.name", "Pure Test"],
    ] {
        assert!(
            std::process::Command::new("git")
                .arg("-C")
                .arg(&workspace)
                .args(args)
                .status()
                .unwrap()
                .success()
        );
    }
    std::fs::write(workspace.join("README.md"), "fresh Task planner turn\n").unwrap();
    for args in [vec!["add", "README.md"], vec!["commit", "-m", "init"]] {
        assert!(
            std::process::Command::new("git")
                .arg("-C")
                .arg(&workspace)
                .args(args)
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
    let thread = runtime
        .create_thread(&project.id, "Fresh Task implementation")
        .await
        .unwrap();
    runtime
        .set_thread_mode(&thread.id, StudioMode::Task)
        .await
        .unwrap();

    let submitted = runtime
        .submit_prompt(StudioSubmitPromptRequest {
            thread_id: thread.id.clone(),
            prompt: "先制定计划，确认后再进入 Task 实施".to_string(),
            attachment_ids: Vec::new(),
            options: StudioSubmitPromptOptions::default(),
        })
        .await
        .unwrap();
    let confirmation = tokio::time::timeout(TEST_RUNTIME_TIMEOUT, async {
        loop {
            if let Some(interaction) = store
                .list_pending_interactions(&thread.id)
                .await
                .unwrap()
                .into_iter()
                .find(|interaction| interaction.kind == InteractionKind::PlanConfirmation)
            {
                break interaction;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(confirmation.scope.turn_id, submitted.turn_id);

    let response = runtime
        .resolve_interaction(
            confirmation.interaction_id.clone(),
            crate::InteractionResolution::PlanConfirmation {
                decision: crate::PlanConfirmationResolution::ImplementFreshContext,
                content: None,
                reason: None,
            },
        )
        .await
        .unwrap();
    assert_eq!(response.interaction.status, InteractionStatus::Resolved);

    assert_eq!(
        tokio::time::timeout(TEST_RUNTIME_TIMEOUT, server)
            .await
            .unwrap()
            .unwrap(),
        4
    );
    wait_for_no_active_turn(&runtime).await;

    let planning_request = requests.recv().await.unwrap();
    let planning_ack_request = requests.recv().await.unwrap();
    let implementation_request = requests.recv().await.unwrap();
    let implementation_ack_request = requests.recv().await.unwrap();
    for request in [&planning_request, &planning_ack_request] {
        let tool_names = response_tool_names(request);
        assert!(!tool_names.contains(&"task_finalize_design"));
        assert!(!tool_names.contains(&"task_spawn_executor"));
    }
    let implementation_tools = response_tool_names(&implementation_request);
    for required in ["task_status", "task_finalize_design", "task_spawn_executor"] {
        assert!(
            implementation_tools.contains(&required),
            "fresh Task request omitted {required}: {implementation_tools:?}"
        );
    }
    for request in [&implementation_request, &implementation_ack_request] {
        let tool_names = response_tool_names(request);
        for available in ["exec", "write_stdin"] {
            assert!(
                tool_names.contains(&available),
                "fresh Task request omitted {available}: {tool_names:?}"
            );
        }
    }

    let turns = turn::Entity::find()
        .filter(turn::Column::ThreadId.eq(&thread.id))
        .all(store.database())
        .await
        .unwrap();
    assert_eq!(turns.len(), 2);
    assert!(turns.iter().any(|turn| turn.id == submitted.turn_id));
    let fresh_turn = turns
        .iter()
        .find(|turn| turn.id != submitted.turn_id)
        .expect("implementation should use a fresh planner Turn");
    assert_eq!(fresh_turn.status, "completed");

    let durable_run = store
        .read_active_task_run_for_root_thread(&thread.id)
        .await
        .unwrap();
    assert_eq!(durable_run.kind(), TaskRunStateKind::Implementing);
    assert!(durable_run.design_summary().is_some());

    let owner = crate::studio::agent_host::root_agent_id(&thread.id);
    let mail_id =
        pl_core::AgentInteractionContinuationRequest::stable_mail_id(&confirmation.interaction_id);
    let input = store
        .database()
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "SELECT turn_id, state, presentation, metadata_json FROM thread_inputs
             WHERE thread_id = ? AND mail_id = ?",
            [owner.to_string().into(), mail_id.clone().into()],
        ))
        .await
        .unwrap()
        .expect("plan implementation should be stored as a durable input");
    assert_eq!(
        input.try_get::<String>("", "turn_id").unwrap(),
        fresh_turn.id
    );
    assert_eq!(input.try_get::<String>("", "state").unwrap(), "consumed");
    assert_eq!(
        input.try_get::<String>("", "presentation").unwrap(),
        "hidden"
    );
    let metadata: serde_json::Value =
        serde_json::from_str(&input.try_get::<String>("", "metadata_json").unwrap()).unwrap();
    assert_eq!(
        metadata["interactionResolutionId"],
        confirmation.interaction_id
    );
    assert_eq!(metadata["mailId"], mail_id);
    assert_eq!(metadata["planLifecycle"]["threadId"], thread.id);

    runtime.shutdown().await;
    let _ = tokio::fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn plan_adjustment_resolves_and_continues_in_a_fresh_planner_turn() {
    let original_plan = "# 原计划\n\n1. 使用英文提示词。";
    let revised_plan = "# 修订后的计划\n\n1. 所有提示词都使用中文。";
    let initial_plan_response = responses_function_tool_sse(
        "initial-plan",
        "plan_exit",
        serde_json::json!({ "content": original_plan }),
    );
    let initial_ack = responses_final_text_sse("initial-plan-ack", "计划已提交确认");
    let revised_plan_response = responses_function_tool_sse(
        "revised-plan",
        "plan_exit",
        serde_json::json!({ "content": revised_plan }),
    );
    let revised_ack = responses_final_text_sse("revised-plan-ack", "修订计划已重新提交确认");
    let (base_url, server) = serve_http_sequence(vec![
        TestHttpResponse::sse(initial_plan_response),
        TestHttpResponse::sse(initial_ack),
        TestHttpResponse::sse(revised_plan_response),
        TestHttpResponse::sse(revised_ack),
    ])
    .await;
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!("pure-plan-adjustment-{unique}"));
    let home = root.join("home");
    let workspace = root.join("workspace");
    tokio::fs::create_dir_all(&workspace).await.unwrap();
    let config_store = ConfigStore::new(crate::config::ConfigPaths::from_home(&home));
    config_store.save(&test_config(base_url)).unwrap();
    let store = StudioStore::open_memory().await.unwrap();
    let runtime = StudioRuntime::new(store.clone(), config_store).unwrap();
    let project = runtime.open_project(&workspace).await.unwrap();
    let thread = runtime
        .create_thread(&project.id, "Plan adjustment continuation")
        .await
        .unwrap();
    runtime
        .set_thread_mode(&thread.id, StudioMode::Task)
        .await
        .unwrap();

    let submitted = runtime
        .submit_prompt(StudioSubmitPromptRequest {
            thread_id: thread.id.clone(),
            prompt: "先制定计划，等待确认后再实施".to_string(),
            attachment_ids: Vec::new(),
            options: StudioSubmitPromptOptions::default(),
        })
        .await
        .unwrap();
    let confirmation = tokio::time::timeout(TEST_RUNTIME_TIMEOUT, async {
        loop {
            if let Some(interaction) = store
                .list_pending_interactions(&thread.id)
                .await
                .unwrap()
                .into_iter()
                .find(|interaction| interaction.kind == InteractionKind::PlanConfirmation)
            {
                break interaction;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    assert!(matches!(
        &confirmation.payload,
        InteractionPayload::PlanConfirmation { content, .. } if content == original_plan
    ));

    let resolution = crate::InteractionResolution::PlanConfirmation {
        decision: crate::PlanConfirmationResolution::ContinuePlanning,
        content: Some("涉及的提示词都用中文".to_string()),
        reason: Some("continue planning".to_string()),
    };
    let response = runtime
        .resolve_interaction(confirmation.interaction_id.clone(), resolution.clone())
        .await
        .unwrap();
    assert_eq!(response.interaction.status, InteractionStatus::Resolved);

    assert_eq!(
        tokio::time::timeout(TEST_RUNTIME_TIMEOUT, server)
            .await
            .unwrap()
            .unwrap(),
        4
    );
    wait_for_no_active_turn(&runtime).await;

    let persisted = store
        .read_interaction(&confirmation.interaction_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(persisted.status, InteractionStatus::Resolved);
    assert_eq!(persisted.resolution, Some(resolution.clone()));
    let revised_confirmation = tokio::time::timeout(TEST_RUNTIME_TIMEOUT, async {
        loop {
            if let Some(interaction) = store
                .list_pending_interactions(&thread.id)
                .await
                .unwrap()
                .into_iter()
                .find(|interaction| interaction.kind == InteractionKind::PlanConfirmation)
            {
                break interaction;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("revised plan should require confirmation again");
    assert_ne!(
        revised_confirmation.interaction_id,
        confirmation.interaction_id
    );
    assert!(matches!(
        revised_confirmation.payload,
        InteractionPayload::PlanConfirmation { content, .. } if content == revised_plan
    ));

    let turns = turn::Entity::find()
        .filter(turn::Column::ThreadId.eq(&thread.id))
        .all(store.database())
        .await
        .unwrap();
    assert_eq!(turns.len(), 2);
    assert!(turns.iter().all(|turn| turn.status == "completed"));
    assert!(turns.iter().all(|turn| turn.failure_json.is_none()));
    assert!(turns.iter().all(|turn| turn.budget_limit_json.is_none()));
    assert!(turns.iter().any(|turn| turn.id == submitted.turn_id));

    let owner = crate::studio::agent_host::root_agent_id(&thread.id);
    let mail_id =
        pl_core::AgentInteractionContinuationRequest::stable_mail_id(&confirmation.interaction_id);
    let input = store
        .database()
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "SELECT state, content, metadata_json, presentation FROM thread_inputs
             WHERE thread_id = ? AND mail_id = ?",
            [owner.to_string().into(), mail_id.clone().into()],
        ))
        .await
        .unwrap()
        .expect("plan adjustment should be stored as a durable input");
    assert_eq!(input.try_get::<String>("", "state").unwrap(), "consumed");
    assert_eq!(
        input.try_get::<String>("", "presentation").unwrap(),
        "hidden"
    );
    let hidden_prompt = input.try_get::<String>("", "content").unwrap();
    assert!(hidden_prompt.contains("只修订计划，不要开始实施"));
    assert!(hidden_prompt.contains(original_plan));
    assert!(hidden_prompt.contains("涉及的提示词都用中文"));
    let metadata: serde_json::Value =
        serde_json::from_str(&input.try_get::<String>("", "metadata_json").unwrap()).unwrap();
    assert_eq!(
        metadata["interactionResolutionId"],
        confirmation.interaction_id
    );
    assert_eq!(metadata["interactionKind"], "planConfirmation");
    assert_eq!(metadata["mailId"], mail_id);
    let task_run_count = store
        .database()
        .query_one_raw(Statement::from_string(
            DatabaseBackend::Sqlite,
            "SELECT COUNT(*) AS count FROM task_runs".to_string(),
        ))
        .await
        .unwrap()
        .unwrap()
        .try_get::<i64>("", "count")
        .unwrap();
    assert_eq!(task_run_count, 0);

    runtime
        .resolve_interaction(confirmation.interaction_id.clone(), resolution)
        .await
        .unwrap();
    let repeated_input_count = store
        .database()
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "SELECT COUNT(*) AS count FROM thread_inputs
             WHERE thread_id = ? AND mail_id = ?",
            [owner.to_string().into(), mail_id.into()],
        ))
        .await
        .unwrap()
        .unwrap()
        .try_get::<i64>("", "count")
        .unwrap();
    assert_eq!(repeated_input_count, 1);
    assert_eq!(
        turn::Entity::find()
            .filter(turn::Column::ThreadId.eq(&thread.id))
            .all(store.database())
            .await
            .unwrap()
            .len(),
        2
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
    let first = StudioRuntime::new(store.clone(), config_store.clone()).unwrap();
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
        .interactions()
        .create(
            interaction.clone(),
            first.interaction_emitter(thread.id.clone()),
        )
        .await
        .unwrap();
    first.shutdown().await;

    let restarted = StudioRuntime::new(store.clone(), config_store).unwrap();
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
async fn start_runtime_returns_while_mcp_discovery_is_pending() {
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let home = std::env::temp_dir().join(format!("pure-mcp-background-home-{unique}"));
    let (base_url, server, accepted, release) = serve_delayed_sse().await;
    let config_store = ConfigStore::new(crate::config::ConfigPaths::from_home(&home));
    let mut config = StudioConfig::default_config();
    config.mcp.servers.insert(
        "slow-startup".to_string(),
        crate::McpServerConfig {
            transport: crate::McpServerTransport::StreamableHttp,
            url: Some(format!("{base_url}/mcp")),
            startup_timeout_secs: Some(TEST_RUNTIME_TIMEOUT.as_secs()),
            ..crate::McpServerConfig::default()
        },
    );
    config_store.save(&config).unwrap();
    let runtime =
        StudioRuntime::new(StudioStore::open_memory().await.unwrap(), config_store).unwrap();
    let mut events = runtime.product_events().subscribe();
    let startup_runtime = runtime.clone();
    let mut startup = tokio::spawn(async move { startup_runtime.start_runtime().await });

    tokio::time::timeout(TEST_RUNTIME_TIMEOUT, accepted)
        .await
        .unwrap()
        .unwrap();
    let snapshot = tokio::time::timeout(std::time::Duration::from_secs(1), &mut startup)
        .await
        .expect("Studio startup must not wait for MCP discovery")
        .unwrap()
        .unwrap();

    assert_eq!(snapshot.status, StudioRuntimeStatus::Ready);
    let running = runtime.read_mcp_state().await.unwrap();
    assert!(matches!(
        running.meta.phase,
        pl_protocol::ObservedStatePhase::Running {
            operation: pl_protocol::StateOperation::Reconcile,
            ..
        }
    ));

    release.send(()).unwrap();
    let ready = tokio::time::timeout(TEST_RUNTIME_TIMEOUT, async {
        loop {
            let event = events.recv().await.unwrap();
            if let StudioProductEventKind::McpStateChanged(state) = event.kind
                && matches!(state.meta.phase, pl_protocol::ObservedStatePhase::Ready)
            {
                break state;
            }
        }
    })
    .await
    .unwrap();
    assert!(
        ready
            .health
            .mcp_servers
            .iter()
            .any(|server| server.id == "slow-startup")
    );
    server.await.unwrap();
    runtime.shutdown().await;
    let _ = tokio::fs::remove_dir_all(home).await;
}

#[tokio::test]
async fn failed_mcp_startup_is_projected_to_its_server_without_blocking_runtime() {
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let home = std::env::temp_dir().join(format!("pure-mcp-unavailable-home-{unique}"));
    let missing_command = format!("pure-mcp-missing-command-{unique}");
    let config_store = ConfigStore::new(crate::config::ConfigPaths::from_home(&home));
    let mut config = StudioConfig::default_config();
    config.mcp.servers.insert(
        "unavailable-startup".to_string(),
        crate::McpServerConfig {
            transport: crate::McpServerTransport::Stdio,
            command: Some(missing_command.clone()),
            startup_timeout_secs: Some(2),
            ..crate::McpServerConfig::default()
        },
    );
    config_store.save(&config).unwrap();
    let runtime =
        StudioRuntime::new(StudioStore::open_memory().await.unwrap(), config_store).unwrap();
    let mut events = runtime.product_events().subscribe();

    let startup = runtime.start_runtime().await.unwrap();

    assert_eq!(startup.status, StudioRuntimeStatus::Ready);
    let state = tokio::time::timeout(TEST_RUNTIME_TIMEOUT, async {
        loop {
            let event = events.recv().await.unwrap();
            if let StudioProductEventKind::McpStateChanged(state) = event.kind
                && matches!(state.meta.phase, pl_protocol::ObservedStatePhase::Ready)
                && state.health.mcp_servers.iter().any(|server| {
                    server.id == "unavailable-startup"
                        && server.availability_kind == "unavailable"
                        && server
                            .availability_message
                            .as_deref()
                            .is_some_and(|message| message.contains(&missing_command))
                })
            {
                break state;
            }
        }
    })
    .await
    .unwrap();
    let server = state
        .health
        .mcp_servers
        .iter()
        .find(|server| server.id == "unavailable-startup")
        .unwrap();
    assert_eq!(server.availability_kind, "unavailable");
    assert!(
        server
            .availability_message
            .as_deref()
            .is_some_and(|message| message.contains(&missing_command))
    );
    assert!(
        !state
            .health
            .active_mcp_servers
            .contains(&"unavailable-startup".to_string())
    );

    runtime.shutdown().await;
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
    )
    .unwrap();
    let mut events = runtime.product_events().subscribe();

    runtime.start_runtime().await.unwrap();

    let state = tokio::time::timeout(TEST_RUNTIME_TIMEOUT, async {
        loop {
            let event = events.recv().await.unwrap();
            if let StudioProductEventKind::McpStateChanged(state) = event.kind
                && matches!(state.meta.phase, pl_protocol::ObservedStatePhase::Ready)
            {
                break state;
            }
        }
    })
    .await
    .unwrap();
    let health = state.health;
    assert!(health.active_mcp_servers.is_empty());
    assert!(health.mcp_servers.iter().any(|server| {
        server.source_kind == "builtIn" && server.availability_kind == "missingCredential"
    }));

    runtime.shutdown().await;
    let _ = tokio::fs::remove_dir_all(home).await;
}

#[tokio::test]
async fn unchanged_mcp_reconcile_is_noop_and_shutdown_snapshot_remains_readable() {
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let home = std::env::temp_dir().join(format!("pure-mcp-owner-home-{unique}"));
    let runtime = StudioRuntime::new(
        StudioStore::open_memory().await.unwrap(),
        ConfigStore::new(crate::config::ConfigPaths::from_home(&home)),
    )
    .unwrap();

    runtime.start_runtime().await.unwrap();
    runtime.reconcile_mcp_runtime().await.unwrap();
    let ready = runtime.read_mcp_state().await.unwrap();
    runtime.reconcile_mcp_runtime().await.unwrap();
    let unchanged = runtime.read_mcp_state().await.unwrap();
    assert_eq!(unchanged, ready);

    runtime.shutdown_runtime().await.unwrap();
    let stopped = runtime.read_mcp_state().await.unwrap();
    assert_eq!(stopped.meta.revision, ready.meta.revision + 1);
    assert!(matches!(
        stopped.meta.phase,
        pl_protocol::ObservedStatePhase::Stopped
    ));
    assert_eq!(stopped.health, ready.health);
    let _ = tokio::fs::remove_dir_all(home).await;
}

#[tokio::test]
async fn activate_project_is_idempotent_for_same_workspace_fingerprint() {
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!("pure-activation-owner-{unique}"));
    let workspace = root.join("workspace");
    let home = root.join("home");
    tokio::fs::create_dir_all(&workspace).await.unwrap();
    let store = StudioStore::open_memory().await.unwrap();
    let project = store.upsert_project(&workspace).await.unwrap();
    let runtime = StudioRuntime::new(
        store,
        ConfigStore::new(crate::config::ConfigPaths::from_home(&home)),
    )
    .unwrap();

    runtime.activate_project(&project.id).await.unwrap();
    let first_lsp = runtime.read_lsp_state().await;
    let first_skills = runtime.skill_catalog_runtime().read(&project.id).await;

    runtime.activate_project(&project.id).await.unwrap();

    assert_eq!(runtime.read_lsp_state().await, first_lsp);
    assert_eq!(
        runtime.skill_catalog_runtime().read(&project.id).await,
        first_skills
    );
    runtime.shutdown().await;
    let _ = tokio::fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn external_state_reads_are_stable_and_lsp_stopped_snapshot_remains_readable() {
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let home = std::env::temp_dir().join(format!("pure-read-only-owner-home-{unique}"));
    let runtime = StudioRuntime::new(
        StudioStore::open_memory().await.unwrap(),
        ConfigStore::new(crate::config::ConfigPaths::from_home(&home)),
    )
    .unwrap();

    let lsp = runtime.read_lsp_state().await;
    assert_eq!(runtime.read_lsp_state().await, lsp);
    let usage = runtime.read_provider_usage_state().await;
    assert_eq!(runtime.read_provider_usage_state().await, usage);
    let update = runtime.read_update_state().await;
    assert_eq!(runtime.read_update_state().await, update);

    runtime.shutdown_runtime().await.unwrap();
    let stopped = runtime.read_lsp_state().await;
    assert_eq!(stopped.meta.revision, lsp.meta.revision + 1);
    assert!(matches!(
        stopped.meta.phase,
        pl_protocol::ObservedStatePhase::Stopped
    ));
    assert_eq!(stopped.health, lsp.health);
    let _ = tokio::fs::remove_dir_all(home).await;
}

fn lazy_home(unique: u128, label: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!("pure-lazy-{label}-{unique}"))
}

#[tokio::test]
async fn idle_registered_thread_stays_lazy_until_subscription_activates_it() {
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let workspace = lazy_home(unique, "ws");
    let home = lazy_home(unique, "home");
    tokio::fs::create_dir_all(&workspace).await.unwrap();
    let store = StudioStore::open_memory().await.unwrap();
    let project = store.upsert_project(&workspace).await.unwrap();
    let thread = store
        .create_thread(&project.id, "Lazy session", StudioMode::Simple)
        .await
        .unwrap();
    // 第一段 runtime：激活并 durable 注册，然后关机排空 write-behind。
    {
        let runtime = StudioRuntime::new(
            store.clone(),
            ConfigStore::new(crate::config::ConfigPaths::from_home(&home)),
        )
        .unwrap();
        runtime.start_runtime().await.unwrap();
        runtime.ensure_thread_agent(&thread.id).await.unwrap();
        runtime.shutdown().await;
    }

    // 第二段 runtime：空闲（无 pending input/活动 Task）Thread 不驻留。
    let runtime = StudioRuntime::new(
        store.clone(),
        ConfigStore::new(crate::config::ConfigPaths::from_home(&home)),
    )
    .unwrap();
    runtime.start_runtime().await.unwrap();
    let framework = runtime.agent_framework().await.unwrap();
    let agent_id = pl_core::ThreadId::new(thread.id.clone()).unwrap();
    assert!(matches!(
        framework.handle().snapshot(agent_id.clone()).await,
        Err(pl_core::AgentRuntimeError::NotFound(_))
    ));

    // 订阅是显式激活命令：按需恢复驻留。
    let mut subscription = runtime
        .subscribe_thread(pl_protocol::ThreadSubscriptionRequest {
            thread_id: thread.id.clone(),
        })
        .await
        .unwrap();
    let _ = subscription.recv().await;
    let actor = framework.handle().snapshot(agent_id).await.unwrap();
    assert_eq!(actor.identity.id.as_str(), thread.id);

    runtime.shutdown().await;
    let _ = tokio::fs::remove_dir_all(workspace).await;
    let _ = tokio::fs::remove_dir_all(home).await;
}

#[tokio::test]
async fn residency_evicts_idle_actors_beyond_capacity_and_restores_on_demand() {
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let workspace = lazy_home(unique, "evict-ws");
    let home = lazy_home(unique, "evict-home");
    tokio::fs::create_dir_all(&workspace).await.unwrap();
    let store = StudioStore::open_memory().await.unwrap();
    let project = store.upsert_project(&workspace).await.unwrap();
    let runtime = StudioRuntime::new(
        store,
        ConfigStore::new(crate::config::ConfigPaths::from_home(&home)),
    )
    .unwrap();
    runtime.start_runtime().await.unwrap();

    // 超过 LRU 容量（16）后，最久未访问的空闲 Thread 被淘汰。
    let mut threads = Vec::new();
    for index in 0..17 {
        let thread = runtime
            .create_thread(&project.id, &format!("Session {index}"))
            .await
            .unwrap();
        runtime.ensure_thread_agent(&thread.id).await.unwrap();
        threads.push(thread);
    }
    let framework = runtime.agent_framework().await.unwrap();
    let first_id = pl_core::ThreadId::new(threads[0].id.clone()).unwrap();
    assert!(
        matches!(
            framework.handle().snapshot(first_id.clone()).await,
            Err(pl_core::AgentRuntimeError::NotFound(_))
        ),
        "LRU front idle actor must be evicted beyond capacity"
    );
    let last_id = pl_core::ThreadId::new(threads[16].id.clone()).unwrap();
    assert!(framework.handle().snapshot(last_id).await.is_ok());

    // 被淘汰 Thread 再次访问时按需恢复。
    runtime.ensure_thread_agent(&threads[0].id).await.unwrap();
    assert!(framework.handle().snapshot(first_id).await.is_ok());

    runtime.shutdown().await;
    let _ = tokio::fs::remove_dir_all(workspace).await;
    let _ = tokio::fs::remove_dir_all(home).await;
}

#[tokio::test]
async fn pinned_thread_is_not_evicted_beyond_capacity() {
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let workspace = lazy_home(unique, "pin-ws");
    let home = lazy_home(unique, "pin-home");
    tokio::fs::create_dir_all(&workspace).await.unwrap();
    let store = StudioStore::open_memory().await.unwrap();
    let project = store.upsert_project(&workspace).await.unwrap();
    let runtime = StudioRuntime::new(
        store,
        ConfigStore::new(crate::config::ConfigPaths::from_home(&home)),
    )
    .unwrap();
    runtime.start_runtime().await.unwrap();

    let mut threads = Vec::new();
    let mut pin_guard = Option::None;
    for index in 0..17 {
        let thread = runtime
            .create_thread(&project.id, &format!("Session {index}"))
            .await
            .unwrap();
        runtime.ensure_thread_agent(&thread.id).await.unwrap();
        if index == 0 {
            // 在后续 ensure 触发淘汰前 pin 队首线程；guard 必须存活到断言后
            // （Drop 即解除 pin），模拟活跃 GUI 订阅的完整生命周期。
            pin_guard = Some(runtime.pin_thread(&thread.id));
        }
        threads.push(thread);
    }
    // 再触发一次 ensure 驱动 enforce_residency_limit。
    runtime.ensure_thread_agent(&threads[16].id).await.unwrap();

    let framework = runtime.agent_framework().await.unwrap();
    let pinned_id = pl_core::ThreadId::new(threads[0].id.clone()).unwrap();
    assert!(
        runtime.residency.is_pinned(&threads[0].id),
        "pin guard must keep the thread pinned"
    );
    assert!(
        framework.handle().snapshot(pinned_id.clone()).await.is_ok(),
        "pinned thread must stay resident beyond capacity"
    );
    // pin 期间容量是软上限：唯一候选被钉住时不淘汰其他线程。
    let neighbor_id = pl_core::ThreadId::new(threads[1].id.clone()).unwrap();
    assert!(
        framework.handle().snapshot(neighbor_id).await.is_ok(),
        "pinned candidate blocks eviction; capacity is soft while observed"
    );

    // 解除 pin 后再次触发淘汰：队首（原被钉线程）按 LRU 被淘汰。
    drop(pin_guard);
    runtime.ensure_thread_agent(&threads[16].id).await.unwrap();
    assert!(
        matches!(
            framework.handle().snapshot(pinned_id).await,
            Err(pl_core::AgentRuntimeError::NotFound(_))
        ),
        "unpinned LRU front must be evicted after the pin is released"
    );

    runtime.shutdown().await;
    let _ = tokio::fs::remove_dir_all(workspace).await;
    let _ = tokio::fs::remove_dir_all(home).await;
}

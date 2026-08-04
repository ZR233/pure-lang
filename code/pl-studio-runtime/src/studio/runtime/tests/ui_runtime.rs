use super::*;
use crate::studio::task_coordinator::TaskRunPhase;
use pl_protocol::SessionTurnState;
use pretty_assertions::assert_eq;
use sea_orm::ConnectionTrait;

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
    let runtime = StudioRuntime::new(store.clone(), config_store);
    let project = runtime.open_project(&workspace).await.unwrap();
    let session = store
        .create_session(&project.id, "Task runtime", StudioMode::Task)
        .await
        .unwrap();
    let run = runtime
        .task_coordinator
        .start_confirmed_task(&session.id, "implement task runtime", &workspace)
        .await
        .unwrap();

    let error = runtime
        .set_session_mode(&session.id, StudioMode::Simple)
        .await
        .unwrap_err();
    let task = runtime
        .session_task_view(&session.id)
        .await
        .unwrap()
        .unwrap();

    assert!(error.to_string().contains("task is active"));
    assert_eq!(task.run_id, run.id);
    assert_eq!(task.phase, "designUpdating");
    assert_eq!(task.branch, run.branch);
    runtime
        .task_coordinator
        .finish_task(&run.id, TaskRunPhase::Cancelled, None)
        .await
        .unwrap();
    let _ = std::fs::remove_dir_all(home);
    let _ = std::fs::remove_dir_all(workspace);
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
    let runtime = StudioRuntime::new(store.clone(), config_store);
    let project = runtime.open_project(&workspace).await.unwrap();
    let session = store
        .create_session(&project.id, "UI runtime", StudioMode::Simple)
        .await
        .unwrap();

    let submitted = runtime
        .submit_prompt(StudioSubmitPromptRequest {
            session_id: session.id.clone(),
            prompt: "wait until stopped".to_string(),
            attachment_ids: Vec::new(),
            options: StudioSubmitPromptOptions::default(),
        })
        .await
        .unwrap();

    assert_eq!(submitted.session_id, session.id);
    assert_eq!(runtime.runtime_snapshot().active_turns.len(), 1);
    tokio::time::timeout(TEST_RUNTIME_TIMEOUT, accepted_rx)
        .await
        .unwrap()
        .unwrap();
    let stopped = runtime.stop_prompt(session.id.clone()).await.unwrap();

    assert_eq!(stopped.session_id, session.id);
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
    let runtime = StudioRuntime::new(store.clone(), config_store);
    let project = runtime.open_project(&workspace).await.unwrap();
    let session = store
        .create_session(&project.id, "Paused Task", StudioMode::Task)
        .await
        .unwrap();
    let run = runtime
        .task_coordinator
        .start_confirmed_task(&session.id, "resume canonical task", &workspace)
        .await
        .unwrap();
    store
        .update_agent_session_status(
            &session.id,
            "interrupted",
            None,
            None,
            crate::studio::ids::unix_seconds(),
        )
        .await
        .unwrap();

    let resumed = runtime.resume_task(session.id.clone()).await.unwrap();
    assert_eq!(resumed.session_id, session.id);
    tokio::time::timeout(TEST_RUNTIME_TIMEOUT, accepted_rx)
        .await
        .unwrap()
        .unwrap();

    let owner = crate::studio::agent_host::root_agent_id(&session.id);
    let input = store
        .database()
        .query_one(sea_orm::Statement::from_sql_and_values(
            sea_orm::DatabaseBackend::Sqlite,
            "SELECT input_json FROM agent_active_inputs WHERE agent_id = ?",
            [owner.to_string().into()],
        ))
        .await
        .unwrap()
        .unwrap();
    let input_json = input.try_get::<String>("", "input_json").unwrap();
    let input: serde_json::Value = serde_json::from_str(&input_json).unwrap();
    assert!(
        input["mailId"]
            .as_str()
            .unwrap()
            .starts_with(&format!("task-resume:{}:", run.id))
    );
    assert_eq!(input["presentation"]["type"], "hidden");
    assert_eq!(input["metadata"]["kind"], "taskResume");
    assert_eq!(input["metadata"]["taskRunId"], run.id);
    assert!(
        input["message"]
            .as_str()
            .unwrap()
            .contains("Read task_status and list_agents")
    );
    assert!(
        runtime
            .session_event_snapshot(&session.id)
            .await
            .unwrap()
            .messages
            .iter()
            .all(|message| message.role != crate::SessionMessageRole::User)
    );
    runtime
        .resume_task(session.id.clone())
        .await
        .expect_err("a running or no-longer-paused Planner must reject duplicate resume");
    let active_input_count = store
        .database()
        .query_one(sea_orm::Statement::from_sql_and_values(
            sea_orm::DatabaseBackend::Sqlite,
            "SELECT COUNT(*) AS count FROM agent_active_inputs WHERE agent_id = ?",
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
        .finish_task(&run.id, TaskRunPhase::Cancelled, None)
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
    let runtime = StudioRuntime::new(store.clone(), config_store);
    let project = runtime.open_project(&workspace).await.unwrap();
    let session = store
        .create_session(&project.id, "Project cleanup", StudioMode::Simple)
        .await
        .unwrap();

    runtime
        .submit_prompt(StudioSubmitPromptRequest {
            session_id: session.id.clone(),
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
    let snapshot = runtime
        .cleanup_project(&project.id, &preview.expected_revision)
        .await
        .unwrap();
    let framework = runtime.agent_framework().await.unwrap();
    let root = framework
        .handle()
        .snapshot(crate::studio::agent_host::root_agent_id(&session.id))
        .await
        .unwrap();

    assert_eq!(root.lifecycle, crate::AgentLifecycleState::Closed);
    assert!(snapshot.active_turns.is_empty());
    assert!(runtime.list_projects().await.unwrap().is_empty());
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
    let runtime = StudioRuntime::new(store.clone(), config_store);
    let project = runtime.open_project(&workspace).await.unwrap();
    let session = store
        .create_session(&project.id, "UI overload retry", StudioMode::Simple)
        .await
        .unwrap();

    runtime
        .submit_prompt(StudioSubmitPromptRequest {
            session_id: session.id.clone(),
            prompt: "complete after overload".to_string(),
            attachment_ids: Vec::new(),
            options: StudioSubmitPromptOptions::default(),
        })
        .await
        .unwrap();

    wait_for_no_active_turn(&runtime).await;
    let snapshot = runtime.session_event_snapshot(&session.id).await.unwrap();
    let request_count = handle.await.unwrap();

    assert_eq!(request_count, 2);
    assert_eq!(
        snapshot.turn.as_ref().map(|turn| turn.state.clone()),
        Some(SessionTurnState::Completed),
        "{snapshot:#?}"
    );
    let _ = tokio::fs::remove_dir_all(home).await;
    let _ = tokio::fs::remove_dir_all(workspace).await;
}

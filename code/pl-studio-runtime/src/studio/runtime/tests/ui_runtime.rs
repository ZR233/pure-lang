use super::*;
use crate::studio::task_coordinator::TaskRunPhase;
use pretty_assertions::assert_eq;

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
async fn ui_submit_clears_active_runtime_snapshot_after_completion() {
    let sse_body = concat!(
        "data: {\"type\":\"response.output_text.delta\",\"item_id\":\"msg_1\",\"delta\":\"done\"}\n\n",
        "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_1\",\"usage\":{\"input_tokens\":1,\"output_tokens\":1,\"total_tokens\":2}}}\n\n",
        "data: [DONE]\n\n"
    )
    .to_string();
    let (base_url, handle) = serve_sse_once(sse_body).await;
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let home = std::env::temp_dir().join(format!("pure-ui-complete-home-{unique}"));
    let workspace = std::env::temp_dir().join(format!("pure-ui-complete-workspace-{unique}"));
    tokio::fs::create_dir_all(&workspace).await.unwrap();
    let config_store = ConfigStore::new(crate::config::ConfigPaths::from_home(&home));
    config_store.save(&test_config(base_url)).unwrap();
    let store = StudioStore::open_memory().await.unwrap();
    let runtime = StudioRuntime::new(store.clone(), config_store);
    let project = runtime.open_project(&workspace).await.unwrap();
    let session = store
        .create_session(&project.id, "UI completion", StudioMode::Simple)
        .await
        .unwrap();

    runtime
        .submit_prompt(StudioSubmitPromptRequest {
            session_id: session.id.clone(),
            prompt: "complete".to_string(),
            attachment_ids: Vec::new(),
            options: StudioSubmitPromptOptions::default(),
        })
        .await
        .unwrap();
    assert_eq!(runtime.runtime_snapshot().active_turns.len(), 1);

    wait_for_no_active_turn(&runtime).await;
    handle.await.unwrap();
    let _ = tokio::fs::remove_dir_all(home).await;
    let _ = tokio::fs::remove_dir_all(workspace).await;
}

use super::*;
use crate::StudioProductEventKind;
use pretty_assertions::assert_eq;

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

#[tokio::test]
async fn failed_initial_commit_hook_keeps_plan_confirmation_pending() {
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let repository = std::env::temp_dir().join(format!("pure-plan-hook-{unique}"));
    std::fs::create_dir_all(&repository).unwrap();
    let status = std::process::Command::new("git")
        .arg("-C")
        .arg(&repository)
        .args(["init", "-b", "main"])
        .status()
        .unwrap();
    assert!(status.success());
    std::fs::write(repository.join("README.md"), "initial\n").unwrap();
    let hook = repository.join(".git/hooks/pre-commit");
    std::fs::write(&hook, "#!/bin/sh\nexit 1\n").unwrap();
    make_hook_executable(&hook);

    assert_failed_task_preflight_keeps_confirmation_pending(&repository, "hook").await;
    let head = std::process::Command::new("git")
        .arg("-C")
        .arg(&repository)
        .args(["rev-parse", "--verify", "HEAD"])
        .output()
        .unwrap();
    assert!(!head.status.success());
    let _ = std::fs::remove_dir_all(repository);
}

async fn assert_failed_task_preflight_keeps_confirmation_pending(
    repository: &std::path::Path,
    suffix: &str,
) {
    let store = StudioStore::open_memory().await.unwrap();
    let project = store.upsert_project(&repository).await.unwrap();
    let session = store
        .create_session(&project.id, "Task", StudioMode::Task)
        .await
        .unwrap();
    let interaction = pending_interaction(
        &format!("plan-confirm-{suffix}"),
        &session.id,
        InteractionKind::PlanConfirmation,
        InteractionPayload::PlanConfirmation {
            plan_id: format!("plan-{suffix}"),
            content: "Implement the plan".to_string(),
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

#[cfg(unix)]
fn make_hook_executable(path: &std::path::Path) {
    use std::os::unix::fs::PermissionsExt;

    let mut permissions = std::fs::metadata(path).unwrap().permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(path, permissions).unwrap();
}

#[cfg(windows)]
fn make_hook_executable(_path: &std::path::Path) {}

#[tokio::test]
async fn initialize_runtime_cancels_recovered_transient_interactions() {
    let store = StudioStore::open_memory().await.unwrap();
    let project = store.upsert_project("C:/work/recovered").await.unwrap();
    let session = store
        .create_session(&project.id, "Recovered", StudioMode::Simple)
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
    assert_eq!(ask.status, InteractionStatus::Cancelled);
    assert_eq!(approval.status, InteractionStatus::Cancelled);
    assert_eq!(plan.status, InteractionStatus::Pending);
    let canonical = runtime.session_event_snapshot(&session.id).await.unwrap();
    let mut subscription = runtime
        .subscribe_session_events(pl_protocol::SessionSubscriptionRequest {
            session_id: session.id.clone(),
            after_sequence: Some(0),
        })
        .await
        .unwrap();
    let mut cancelled_interactions = 0;
    for _ in 0..canonical.through_sequence {
        let Some(pl_protocol::SessionStreamFrame::Event { event }) = subscription.recv().await
        else {
            continue;
        };
        if matches!(
            &event.kind,
            pl_protocol::SessionEventKind::InteractionChanged { event }
                if event.interaction.status == InteractionStatus::Cancelled
        ) {
            cancelled_interactions += 1;
        }
    }
    assert_eq!(cancelled_interactions, 2);
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

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use super::super::TaskAgentRuntimeRegistry;
use crate::config::{ConfigPaths, ConfigStore};
use crate::studio::task_coordinator::TaskRunRecord;
use crate::tool::{
    CloseAgentTool, ListAgentsTool, SpawnAgentTool, Tool, ToolContext, ToolInput, WaitAgentTool,
    WorkspaceAccess,
};
use crate::{CompileMode, CoreSession, PureCoreBuilder, StudioRuntime, StudioStore, TurnOptions};

#[tokio::test]
async fn task_sessions_reuse_only_their_own_supervisor() {
    let registry = TaskAgentRuntimeRegistry::new();
    let repository = Path::new("C:/repo");

    let first = registry
        .supervisor_for_task("session-a", repository, 7)
        .await
        .unwrap();
    let continuation = registry
        .supervisor_for_task("session-a", repository, 7)
        .await
        .unwrap();
    let other_session = registry
        .supervisor_for_task("session-b", repository, 7)
        .await
        .unwrap();

    assert!(first.shares_runtime_with(&continuation));
    assert!(!first.shares_runtime_with(&other_session));
    assert_eq!(registry.len().await, 2);
}

#[tokio::test]
async fn simple_mode_remains_turn_local() {
    let registry = TaskAgentRuntimeRegistry::new();

    let first = registry
        .supervisor_for_mode(CompileMode::Simple, "session", Path::new("C:/repo"), 1)
        .await
        .unwrap();
    let second = registry
        .supervisor_for_mode(CompileMode::Simple, "session", Path::new("C:/repo"), 1)
        .await
        .unwrap();

    assert!(first.is_none());
    assert!(second.is_none());
    assert_eq!(registry.len().await, 0);
}

#[tokio::test]
async fn repository_or_epoch_drift_is_rejected_without_replacing_runtime() {
    let registry = TaskAgentRuntimeRegistry::new();
    let original = registry
        .supervisor_for_task("session", Path::new("C:/repo"), 3)
        .await
        .unwrap();

    let repository_error = registry
        .supervisor_for_task("session", Path::new("C:/other"), 3)
        .await
        .unwrap_err();
    let epoch_error = registry
        .supervisor_for_task("session", Path::new("C:/repo"), 4)
        .await
        .unwrap_err();
    let still_original = registry
        .supervisor_for_task("session", Path::new("C:/repo"), 3)
        .await
        .unwrap();

    assert!(repository_error.to_string().contains("repository"));
    assert!(epoch_error.to_string().contains("epoch"));
    assert!(original.shares_runtime_with(&still_original));
}

#[tokio::test]
async fn shutdown_quiesces_then_clears_registry_for_next_epoch() {
    let registry = TaskAgentRuntimeRegistry::new();
    let before = registry
        .supervisor_for_task("session", Path::new("C:/repo"), 2)
        .await
        .unwrap();

    registry.quiesce_and_clear().await.unwrap();

    assert_eq!(registry.len().await, 0);
    let after = registry
        .supervisor_for_task("session", Path::new("C:/repo"), 3)
        .await
        .unwrap();
    assert!(!before.shares_runtime_with(&after));
}

#[tokio::test]
async fn planning_generation_binds_first_run_then_rotates_after_terminal() {
    let registry = TaskAgentRuntimeRegistry::new();
    let planning = registry
        .supervisor_for_task_generation("session", Path::new("C:/repo"), 5, None)
        .await
        .unwrap();
    let first_run = registry
        .supervisor_for_task_generation("session", Path::new("C:/repo"), 5, Some("run-1"))
        .await
        .unwrap();
    let same_run = registry
        .supervisor_for_task_generation("session", Path::new("C:/repo"), 5, Some("run-1"))
        .await
        .unwrap();
    let next_planning = registry
        .supervisor_for_task_generation("session", Path::new("C:/repo"), 5, None)
        .await
        .unwrap();

    assert!(planning.shares_runtime_with(&first_run));
    assert!(first_run.shares_runtime_with(&same_run));
    assert!(!first_run.shares_runtime_with(&next_planning));
    assert_eq!(registry.len().await, 1);
}

#[tokio::test]
async fn second_task_core_controls_first_core_agent_without_crossing_sessions_or_epoch() {
    let store = StudioStore::open_memory().await.unwrap();
    let runtime = StudioRuntime::new(
        store.clone(),
        ConfigStore::new(ConfigPaths::from_home(unique_temp_path(
            "agent-runtime-home",
        ))),
    );
    let first_repo = init_task_repository("shared-first");
    let second_repo = init_task_repository("shared-second");
    let (first_session, first_run) = start_task(&runtime, &store, &first_repo, "first").await;
    let (second_session, second_run) = start_task(&runtime, &store, &second_repo, "second").await;
    let provider_info = test_provider_info();
    let provider = pl_model::create_provider(provider_info.clone()).unwrap();

    let first_supervisor = runtime
        .task_agent_runtimes
        .supervisor_for_task_generation(
            &first_session,
            Path::new(&first_run.workspace_root),
            runtime.lifecycle_epoch(),
            Some(&first_run.id),
        )
        .await
        .unwrap();
    let mut first_core = PureCoreBuilder::from_provider_info(provider_info.clone())
        .unwrap()
        .with_agent_supervisor(first_supervisor.clone())
        .build();
    runtime
        .task_coordinator
        .install_tools(&mut first_core, &first_session);
    let first_context = tool_context(
        first_supervisor.clone(),
        PathBuf::from(&first_run.workspace_root),
    );
    let spawn_tool = SpawnAgentTool::new(provider.clone(), None, None, None, None, None);
    let first_spawn = spawn_tool
        .execute(
            tool_input(
                &first_session,
                "spawn-first",
                serde_json::json!({
                    "taskName": "shared_name",
                    "message": "inspect first",
                    "agentType": "explorer"
                }),
            ),
            first_context.clone(),
        )
        .await
        .unwrap();
    let first_agent_id = serde_json::from_str::<serde_json::Value>(&first_spawn.description)
        .unwrap()["agentId"]
        .as_str()
        .unwrap()
        .to_string();

    let continuation_supervisor = runtime
        .task_agent_runtimes
        .supervisor_for_task_generation(
            &first_session,
            Path::new(&first_run.workspace_root),
            runtime.lifecycle_epoch(),
            Some(&first_run.id),
        )
        .await
        .unwrap();
    let mut continuation_core = PureCoreBuilder::from_provider_info(provider_info.clone())
        .unwrap()
        .with_agent_supervisor(continuation_supervisor.clone())
        .build();
    runtime
        .task_coordinator
        .install_tools(&mut continuation_core, &first_session);
    let continuation_context = tool_context(
        continuation_supervisor.clone(),
        PathBuf::from(&first_run.workspace_root),
    );
    let listed = ListAgentsTool
        .execute(
            tool_input(&first_session, "list-first", serde_json::json!({})),
            continuation_context.clone(),
        )
        .await
        .unwrap();
    assert!(listed.description.contains("/root/shared_name"));
    assert!(listed.description.contains("\"status\":\"running\""));
    let waited = WaitAgentTool
        .execute(
            tool_input(
                &first_session,
                "wait-first",
                serde_json::json!({"target": "/root/shared_name", "timeoutMs": 250}),
            ),
            continuation_context.clone(),
        )
        .await
        .unwrap();
    assert!(waited.description.contains("/root/shared_name"));
    CloseAgentTool
        .execute(
            tool_input(
                &first_session,
                "close-first",
                serde_json::json!({"target": "/root/shared_name", "merge": false}),
            ),
            continuation_context,
        )
        .await
        .unwrap();

    let second_supervisor = runtime
        .task_agent_runtimes
        .supervisor_for_task_generation(
            &second_session,
            Path::new(&second_run.workspace_root),
            runtime.lifecycle_epoch(),
            Some(&second_run.id),
        )
        .await
        .unwrap();
    let mut second_core = PureCoreBuilder::from_provider_info(provider_info)
        .unwrap()
        .with_agent_supervisor(second_supervisor.clone())
        .build();
    runtime
        .task_coordinator
        .install_tools(&mut second_core, &second_session);
    let second_context = tool_context(
        second_supervisor.clone(),
        PathBuf::from(&second_run.workspace_root),
    );
    let second_spawn = spawn_tool
        .execute(
            tool_input(
                &second_session,
                "spawn-second",
                serde_json::json!({
                    "taskName": "shared_name",
                    "message": "inspect second",
                    "agentType": "explorer"
                }),
            ),
            second_context,
        )
        .await
        .unwrap();
    let second_agent_id = serde_json::from_str::<serde_json::Value>(&second_spawn.description)
        .unwrap()["agentId"]
        .as_str()
        .unwrap()
        .to_string();
    assert_ne!(first_agent_id, second_agent_id);
    assert_eq!(
        store.list_agent_outcomes(&first_run.id).await.unwrap()[0].agent_id,
        first_agent_id
    );
    assert_eq!(
        store.list_agent_outcomes(&second_run.id).await.unwrap()[0].agent_id,
        second_agent_id
    );

    runtime.shutdown_runtime().await.unwrap();
    let mut visible_events = runtime.events().subscribe();
    let (event_tx, event_rx) = tokio::sync::broadcast::channel(8);
    let drain_runtime = runtime.clone();
    let drain_session = second_session.clone();
    let drain = tokio::spawn(async move {
        drain_runtime
            .drain_prompt_agent_events(drain_session, "old-turn".to_string(), event_rx)
            .await;
    });
    event_tx
        .send(pl_trace::AgentEvent::AgentStateChanged {
            id: second_agent_id,
            path: "/root/shared_name".to_string(),
            parent_path: Some("/root".to_string()),
            role: "explorer".to_string(),
            task: "inspect second".to_string(),
            status: pl_protocol::AgentStatus::Shutdown,
            summary: None,
            depth: 1,
            error: None,
            reason: Some("old epoch".to_string()),
            budget_limit_kind: None,
            budget_usage: None,
            updated_at: 1,
        })
        .unwrap();
    drop(event_tx);
    drain.await.unwrap();
    assert!(matches!(
        visible_events.try_recv(),
        Err(tokio::sync::broadcast::error::TryRecvError::Empty)
    ));

    drop(first_core);
    drop(continuation_core);
    drop(second_core);
    std::fs::remove_dir_all(first_repo).ok();
    std::fs::remove_dir_all(second_repo).ok();
}

async fn start_task(
    runtime: &StudioRuntime,
    store: &StudioStore,
    repository: &Path,
    title: &str,
) -> (String, TaskRunRecord) {
    let project = store.upsert_project(repository).await.unwrap();
    let session = store
        .create_session(&project.id, title, CompileMode::Task)
        .await
        .unwrap();
    let run = runtime
        .task_coordinator
        .start_confirmed_task(&session.id, "plan", repository)
        .await
        .unwrap();
    (session.id, run)
}

fn tool_context(supervisor: crate::AgentSupervisor, workspace_root: PathBuf) -> ToolContext {
    ToolContext {
        event_tx: tokio::sync::broadcast::channel(32).0,
        options: TurnOptions::default(),
        workspace_access: WorkspaceAccess::WorkspaceOnly,
        mode: CompileMode::Task,
        workspace_root,
        workspace_instructions: None,
        instruction_snapshot: None,
        provider_call_id: None,
        active_subagent: None,
        agent_supervisor: supervisor,
        agent_tool_registrar: None,
        lsp_runtime: None,
        parent_session: Arc::new(CoreSession::new()),
    }
}

fn tool_input(session_id: &str, tool_id: &str, arguments: serde_json::Value) -> ToolInput {
    ToolInput {
        arguments,
        session_id: session_id.to_string(),
        tool_id: tool_id.to_string(),
        revision_base: 0,
    }
}

fn test_provider_info() -> pl_model::ProviderInfo {
    let mut provider = pl_model::ProviderInfo::openai(Some("http://example.invalid".to_string()));
    provider.default_model = "test-model".to_string();
    provider
}

fn init_task_repository(label: &str) -> PathBuf {
    let repository = unique_temp_path(label);
    std::fs::create_dir_all(&repository).unwrap();
    run_git(&repository, &["init"]);
    run_git(&repository, &["checkout", "-b", "main"]);
    run_git(
        &repository,
        &["config", "user.email", "pure@example.invalid"],
    );
    run_git(&repository, &["config", "user.name", "Pure Test"]);
    std::fs::write(repository.join("README.md"), "initial\n").unwrap();
    run_git(&repository, &["add", "README.md"]);
    run_git(&repository, &["commit", "-m", "initial"]);
    repository
}

fn run_git(repository: &Path, args: &[&str]) {
    let output = Command::new("git")
        .arg("-C")
        .arg(repository)
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {} failed: {}",
        args.join(" "),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn unique_temp_path(label: &str) -> PathBuf {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!("pure-{label}-{}-{stamp}", std::process::id()))
}

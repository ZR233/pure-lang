use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use pl_trace::TraceEventKind;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::Mutex;

use super::super::TaskAgentRuntimeRegistry;
use super::{RunPromptRequest, emitter, test_config};
use crate::config::{ConfigPaths, ConfigStore};
use crate::studio::task_coordinator::{AgentOutcomeStatus, TaskRunRecord};
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
async fn real_task_turns_reuse_agent_session_and_mailbox_without_crossing_sessions() {
    tokio::time::timeout(
        super::TEST_RUNTIME_TIMEOUT,
        run_real_task_turns_reuse_agent_session_and_mailbox_without_crossing_sessions(),
    )
    .await
    .expect("real Task agent turns timed out");
}

async fn run_real_task_turns_reuse_agent_session_and_mailbox_without_crossing_sessions() {
    let first_repo = init_task_repository("prompt-shared-first");
    let second_repo = init_task_repository("prompt-shared-second");
    let root_responses = vec![
        tool_call_sse(
            "spawn-first",
            "spawn_agent",
            serde_json::json!({
                "taskName": "shared_name",
                "message": "inspect the first repository",
                "agentType": "explorer"
            }),
        ),
        tool_call_sse(
            "wait-first-spawn",
            "wait_agent",
            serde_json::json!({"target": "/root/shared_name", "timeoutMs": 2_000}),
        ),
        final_sse("first-ready", "First explorer is waiting."),
        tool_call_sse("list-first", "list_agents", serde_json::json!({})),
        tool_call_sse(
            "wait-first",
            "wait_agent",
            serde_json::json!({"target": "/root/shared_name", "timeoutMs": 250}),
        ),
        tool_call_sse(
            "resume-first",
            "resume_agent",
            serde_json::json!({"target": "/root/shared_name"}),
        ),
        tool_call_sse(
            "send-first",
            "send_input",
            serde_json::json!({
                "target": "/root/shared_name",
                "message": "remember this queued follow-up",
                "triggerTurn": false
            }),
        ),
        tool_call_sse(
            "close-first",
            "close_agent",
            serde_json::json!({"target": "/root/shared_name", "merge": false}),
        ),
        final_sse("first-closed", "First explorer closed."),
        tool_call_sse(
            "spawn-second",
            "spawn_agent",
            serde_json::json!({
                "taskName": "shared_name",
                "message": "inspect the second repository",
                "agentType": "explorer"
            }),
        ),
        tool_call_sse(
            "wait-second-spawn",
            "wait_agent",
            serde_json::json!({"target": "/root/shared_name", "timeoutMs": 2_000}),
        ),
        final_sse("second-ready", "Second explorer is waiting."),
    ];
    let explorer_responses = vec![
        final_sse("explorer-first", "first repository inspected"),
        final_sse("explorer-second", "second repository inspected"),
    ];
    let (base_url, server) = serve_task_agent_sse(root_responses, explorer_responses).await;
    let home = unique_temp_path("prompt-agent-runtime-home");
    let config_store = ConfigStore::new(ConfigPaths::from_home(&home));
    let mut config = test_config(base_url);
    config.skills.auto_learn = false;
    config_store.save(&config).unwrap();
    let store = StudioStore::open_memory().await.unwrap();
    let runtime = StudioRuntime::new(store.clone(), config_store);
    let (first_session, first_run) = start_task(&runtime, &store, &first_repo, "first").await;
    let (second_session, second_run) = start_task(&runtime, &store, &second_repo, "second").await;

    let first_spawn = run_task_prompt(
        &runtime,
        &first_session,
        "first-spawn-turn",
        "start explorer",
    )
    .await;
    assert_tool_result_contains(
        &first_spawn.trace_events,
        "spawn_agent",
        "/root/shared_name",
    );
    let first_outcome = store.list_agent_outcomes(&first_run.id).await.unwrap()[0].clone();
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
    let first_agent_session = first_supervisor
        .load_session(&first_outcome.agent_id)
        .await
        .expect("first explorer core session");
    assert!(!first_agent_session.messages().is_empty());

    let first_control = run_task_prompt(
        &runtime,
        &first_session,
        "first-control-turn",
        "control explorer",
    )
    .await;
    assert_tool_result_contains(
        &first_control.trace_events,
        "list_agents",
        "first repository inspected",
    );
    assert_tool_result_contains(
        &first_control.trace_events,
        "wait_agent",
        "first repository inspected",
    );
    assert_tool_result_contains(&first_control.trace_events, "send_input", "\"queued\":true");
    assert_tool_result_contains(
        &first_control.trace_events,
        "close_agent",
        "\"status\":\"shutdown\"",
    );
    let first_session_after_control = first_supervisor
        .load_session(&first_outcome.agent_id)
        .await
        .expect("first explorer session remains attached after control turn");
    assert_eq!(
        first_session_after_control.messages(),
        first_agent_session.messages()
    );
    let first_durable = store.list_agent_outcomes(&first_run.id).await.unwrap();
    assert_eq!(first_durable.len(), 1);
    assert_eq!(first_durable[0].agent_id, first_outcome.agent_id);
    assert_eq!(first_durable[0].owner_path, "/root");
    assert_eq!(first_durable[0].initiated_by, "planner");
    assert!(
        first_durable[0]
            .requested_by_call_id
            .contains("spawn-first")
    );
    assert_eq!(first_durable[0].status, AgentOutcomeStatus::Completed);

    let second_spawn = run_task_prompt(
        &runtime,
        &second_session,
        "second-spawn-turn",
        "start same path explorer",
    )
    .await;
    assert_tool_result_contains(
        &second_spawn.trace_events,
        "spawn_agent",
        "/root/shared_name",
    );
    let second_outcome = store.list_agent_outcomes(&second_run.id).await.unwrap()[0].clone();
    assert_ne!(first_outcome.agent_id, second_outcome.agent_id);
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
    let second_agents = second_supervisor.list_agents(None).await.unwrap();
    assert_eq!(second_agents.len(), 1);
    assert_eq!(second_agents[0].id, second_outcome.agent_id);
    assert_eq!(second_agents[0].path, "/root/shared_name");
    assert_eq!(
        second_agents[0].summary.as_deref(),
        Some("second repository inspected")
    );
    let second_durable = store.list_agent_outcomes(&second_run.id).await.unwrap();
    assert_eq!(second_durable.len(), 1);
    assert_eq!(second_durable[0].agent_id, second_outcome.agent_id);
    assert!(
        second_durable[0]
            .requested_by_call_id
            .contains("spawn-second")
    );

    server.await.unwrap();
    runtime.shutdown_runtime().await.unwrap();
    std::fs::remove_dir_all(first_repo).ok();
    std::fs::remove_dir_all(second_repo).ok();
    std::fs::remove_dir_all(home).ok();
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

async fn serve_task_agent_sse(
    root_responses: Vec<String>,
    explorer_responses: Vec<String>,
) -> (String, tokio::task::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let request_count = root_responses.len() + explorer_responses.len();
    let root_responses = Arc::new(Mutex::new(VecDeque::from(root_responses)));
    let explorer_responses = Arc::new(Mutex::new(VecDeque::from(explorer_responses)));
    let handle = tokio::spawn(async move {
        let mut requests = tokio::task::JoinSet::new();
        for _ in 0..request_count {
            let (socket, _) = listener.accept().await.unwrap();
            let root_responses = root_responses.clone();
            let explorer_responses = explorer_responses.clone();
            requests.spawn(async move {
                serve_task_agent_request(socket, root_responses, explorer_responses).await;
            });
        }
        while let Some(result) = requests.join_next().await {
            result.unwrap();
        }
        assert!(root_responses.lock().await.is_empty());
        assert!(explorer_responses.lock().await.is_empty());
    });
    (format!("http://{address}"), handle)
}

async fn serve_task_agent_request(
    mut socket: tokio::net::TcpStream,
    root_responses: Arc<Mutex<VecDeque<String>>>,
    explorer_responses: Arc<Mutex<VecDeque<String>>>,
) {
    let mut buffer = Vec::new();
    let mut chunk = [0_u8; 1024];
    let (header_end, content_length) = loop {
        let count = socket.read(&mut chunk).await.unwrap();
        assert_ne!(count, 0);
        buffer.extend_from_slice(&chunk[..count]);
        if let Some(header_end) = buffer.windows(4).position(|window| window == b"\r\n\r\n") {
            let headers = String::from_utf8_lossy(&buffer[..header_end]);
            let content_length = headers
                .lines()
                .find_map(|line| {
                    let (name, value) = line.split_once(':')?;
                    name.eq_ignore_ascii_case("content-length")
                        .then(|| value.trim().parse::<usize>().ok())?
                })
                .unwrap_or(0);
            break (header_end, content_length);
        }
    };
    while buffer.len() < header_end + 4 + content_length {
        let count = socket.read(&mut chunk).await.unwrap();
        assert_ne!(count, 0);
        buffer.extend_from_slice(&chunk[..count]);
    }
    let body_bytes = &buffer[header_end + 4..header_end + 4 + content_length];
    let body: serde_json::Value = serde_json::from_slice(body_bytes).unwrap();
    let is_root = body["tools"].as_array().is_some_and(|tools| {
        tools.iter().any(|tool| {
            matches!(
                tool["name"].as_str(),
                Some(
                    "spawn_agent"
                        | "wait_agent"
                        | "list_agents"
                        | "send_input"
                        | "close_agent"
                        | "resume_agent"
                        | "plan_exit"
                        | "task_update_design"
                )
            )
        })
    });
    let responses = if is_root {
        root_responses
    } else {
        explorer_responses
    };
    let sse_body = responses
        .lock()
        .await
        .pop_front()
        .unwrap_or_else(|| panic!("unexpected model request body: {body}"));
    let response = format!(
        "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
        sse_body.len(),
        sse_body
    );
    socket.write_all(response.as_bytes()).await.unwrap();
    socket.shutdown().await.unwrap();
}

async fn run_task_prompt(
    runtime: &StudioRuntime,
    session_id: &str,
    turn_id: &str,
    prompt: &str,
) -> crate::studio::StudioPromptOutcome {
    let interaction_events = Arc::new(tokio::sync::Mutex::new(Vec::new()));
    let interaction_emitter = emitter(interaction_events);
    let interaction_callback = runtime
        .interactions()
        .callback(session_id.to_string(), interaction_emitter.clone());
    runtime
        .run_prompt(RunPromptRequest {
            session_id: session_id.to_string(),
            turn_id: turn_id.to_string(),
            prompt: prompt.to_string(),
            attachment_ids: Vec::new(),
            interaction_callback,
            interaction_emitter,
            options: TurnOptions::default(),
        })
        .await
        .unwrap()
}

fn tool_call_sse(id: &str, name: &str, arguments: serde_json::Value) -> String {
    let item_id = format!("fc_{id}");
    let call_id = format!("call_{id}");
    let arguments = arguments.to_string();
    let events = [
        serde_json::json!({
            "type": "response.output_item.added",
            "item": {
                "type": "function_call",
                "id": item_id,
                "call_id": call_id,
                "name": name
            }
        }),
        serde_json::json!({
            "type": "response.function_call_arguments.delta",
            "item_id": item_id,
            "call_id": call_id,
            "delta": arguments
        }),
        serde_json::json!({
            "type": "response.output_item.done",
            "item": {
                "type": "function_call",
                "id": item_id,
                "call_id": call_id,
                "name": name,
                "arguments": arguments
            }
        }),
        serde_json::json!({
            "type": "response.completed",
            "response": {
                "id": format!("response_{id}"),
                "usage": {"input_tokens": 1, "output_tokens": 1, "total_tokens": 2}
            }
        }),
    ];
    events
        .into_iter()
        .map(|event| format!("data: {event}\n\n"))
        .chain(std::iter::once("data: [DONE]\n\n".to_string()))
        .collect()
}

fn final_sse(id: &str, content: &str) -> String {
    let item_id = format!("msg_{id}");
    let events = [
        serde_json::json!({
            "type": "response.output_item.added",
            "item": {
                "id": item_id,
                "type": "message",
                "role": "assistant",
                "phase": "final_answer"
            }
        }),
        serde_json::json!({
            "type": "response.output_text.delta",
            "item_id": item_id,
            "delta": content
        }),
        serde_json::json!({
            "type": "response.output_item.done",
            "item": {
                "id": item_id,
                "type": "message",
                "role": "assistant",
                "phase": "final_answer",
                "content": [{"type": "output_text", "text": content}]
            }
        }),
        serde_json::json!({
            "type": "response.completed",
            "response": {
                "id": format!("response_{id}"),
                "usage": {"input_tokens": 1, "output_tokens": 1, "total_tokens": 2}
            }
        }),
    ];
    events
        .into_iter()
        .map(|event| format!("data: {event}\n\n"))
        .chain(std::iter::once("data: [DONE]\n\n".to_string()))
        .collect()
}

fn assert_tool_result_contains(events: &[pl_trace::TraceEvent], name: &str, expected: &str) {
    let result = tool_result(events, name);
    assert!(
        result.contains(expected),
        "{name} result did not contain {expected:?}: {result}"
    );
}

fn tool_result<'a>(events: &'a [pl_trace::TraceEvent], name: &str) -> &'a str {
    if let Some(result) = events.iter().rev().find_map(|event| match &event.kind {
        TraceEventKind::TracePartCompleted { item }
            if item.tool.as_ref().is_some_and(|tool| tool.name == name) =>
        {
            item.tool.as_ref()?.result.as_deref()
        }
        _ => None,
    }) {
        return result;
    }
    let failures = events
        .iter()
        .filter_map(|event| match &event.kind {
            TraceEventKind::TracePartFailed { item, error }
                if item.tool.as_ref().is_some_and(|tool| tool.name == name) =>
            {
                Some(error.as_str())
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    panic!("completed {name} result; failures: {failures:?}")
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

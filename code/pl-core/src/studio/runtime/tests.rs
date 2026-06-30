use std::time::Duration;

use pl_model::{ModelInfo, ProviderInfo};
use pl_protocol::{
    InteractionKind, InteractionPayload, InteractionScope, InteractionStatus, PlanLifecycleState,
    StudioTextChannel, StudioTurnStatus,
};
use pl_trace::{
    TraceEvent, TraceEventKind, TracePart, TracePartKind, TracePartSource, TracePartStatus,
    TraceTextChannel,
};
use pretty_assertions::assert_eq;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::{Mutex, oneshot};

use super::plan_confirmation::plan_confirmation_id;
use super::*;
use crate::config::{ModelRole, ProviderConfig, RoleConfig, RoleConfigs};
use crate::studio::runtime::self_learning::{started_tool_snapshot_count, tool_call_count};
use crate::{CompileMode, StudioRuntimeStatus, TurnResultStatus};

const TEST_RUNTIME_TIMEOUT: Duration = Duration::from_secs(20);

async fn serve_sse_once(sse_body: String) -> (String, tokio::task::JoinHandle<()>) {
    serve_sse_sequence(vec![sse_body]).await
}

async fn serve_sse_sequence(sse_bodies: Vec<String>) -> (String, tokio::task::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let handle = tokio::spawn(async move {
        for sse_body in sse_bodies {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut buffer = Vec::new();
            let mut temp = [0_u8; 1024];
            let (header_end, content_length) = loop {
                let n = socket.read(&mut temp).await.unwrap();
                assert_ne!(n, 0);
                buffer.extend_from_slice(&temp[..n]);
                if let Some(header_end) = buffer.windows(4).position(|window| window == b"\r\n\r\n")
                {
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
                let n = socket.read(&mut temp).await.unwrap();
                assert_ne!(n, 0);
                buffer.extend_from_slice(&temp[..n]);
            }

            let response = format!(
                "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
                sse_body.len(),
                sse_body
            );
            socket.write_all(response.as_bytes()).await.unwrap();
            socket.shutdown().await.unwrap();
        }
    });

    (format!("http://{addr}"), handle)
}

async fn serve_delayed_sse() -> (
    String,
    tokio::task::JoinHandle<()>,
    oneshot::Receiver<()>,
    oneshot::Sender<()>,
) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let (accepted_tx, accepted_rx) = oneshot::channel();
    let (release_tx, release_rx) = oneshot::channel();
    let handle = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut buffer = Vec::new();
        let mut temp = [0_u8; 1024];
        loop {
            let n = socket.read(&mut temp).await.unwrap_or(0);
            if n == 0 {
                return;
            }
            buffer.extend_from_slice(&temp[..n]);
            if buffer.windows(4).any(|window| window == b"\r\n\r\n") {
                break;
            }
        }
        let _ = accepted_tx.send(());
        let _ = release_rx.await;
        let sse_body = "data: [DONE]\n\n";
        let response = format!(
            "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
            sse_body.len(),
            sse_body
        );
        let _ = socket.write_all(response.as_bytes()).await;
        let _ = socket.shutdown().await;
    });

    (format!("http://{addr}"), handle, accepted_rx, release_tx)
}

fn test_config(base_url: String) -> crate::config::PureConfig {
    let mut model = ModelInfo::fallback("local-responses");
    model.parameters = vec![crate::ModelParameter {
        name: "effort".to_string(),
        label: None,
        candidates: vec!["none".to_string()],
        wire: std::collections::BTreeMap::new(),
    }];
    let mut info = ProviderInfo::openai(Some(base_url));
    info.default_model = "local-responses".to_string();
    let provider = ProviderConfig::from_provider_info(info, vec![model]);
    let role = RoleConfig {
        provider: "local".to_string(),
        model: "local-responses".to_string(),
        effort: crate::config::ReasoningEffort::new("none"),
    };
    crate::config::PureConfig {
        roles: RoleConfigs::from_default_role(role),
        providers: std::collections::BTreeMap::from([("local".to_string(), provider)]),
        ..crate::config::PureConfig::default_config()
    }
}

fn test_chat_config(base_url: String) -> crate::config::PureConfig {
    let mut model = ModelInfo::fallback("local-chat");
    model.context_window = Some(128_000);
    model.parameters = vec![crate::ModelParameter {
        name: "effort".to_string(),
        label: None,
        candidates: vec!["none".to_string()],
        wire: std::collections::BTreeMap::new(),
    }];
    let mut info = ProviderInfo::deepseek(Some(base_url));
    info.default_model = "local-chat".to_string();
    let provider = ProviderConfig::from_provider_info(info, vec![model]);
    let role = RoleConfig {
        provider: "local".to_string(),
        model: "local-chat".to_string(),
        effort: crate::config::ReasoningEffort::new("none"),
    };
    crate::config::PureConfig {
        roles: RoleConfigs::from_default_role(role),
        providers: std::collections::BTreeMap::from([("local".to_string(), provider)]),
        ..crate::config::PureConfig::default_config()
    }
}

fn emitter(
    events: std::sync::Arc<Mutex<Vec<InteractionRequest>>>,
) -> crate::studio::InteractionEmitter {
    std::sync::Arc::new(move |interaction| {
        let events = events.clone();
        Box::pin(async move {
            events.lock().await.push(interaction);
            Ok(())
        })
    })
}

fn pending_interaction(
    id: &str,
    session_id: &str,
    kind: InteractionKind,
    payload: InteractionPayload,
) -> InteractionRequest {
    InteractionRequest {
        interaction_id: id.to_string(),
        kind,
        status: InteractionStatus::Pending,
        scope: InteractionScope {
            session_id: session_id.to_string(),
            turn_id: "turn-recovered".to_string(),
            item_id: Some(id.to_string()),
            tool_id: Some(id.to_string()),
            agent_path: None,
        },
        payload,
        created_at: 1,
        updated_at: 1,
        resolved_at: None,
        resolution: None,
    }
}

async fn wait_for_no_active_turn(runtime: &StudioRuntime) {
    tokio::time::timeout(TEST_RUNTIME_TIMEOUT, async {
        loop {
            if runtime.runtime_snapshot().active_turns.is_empty() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn set_model_role_persists_planner_model_and_default_effort() {
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let home = std::env::temp_dir().join(format!("pure-role-runtime-home-{unique}"));
    let config_store = ConfigStore::new(crate::config::ConfigPaths::from_home(&home));
    let mut config = test_config("http://127.0.0.1:9".to_string());
    let mut fast_model = ModelInfo::fallback("local-fast");
    fast_model.parameters = vec![crate::ModelParameter {
        name: "effort".to_string(),
        label: None,
        candidates: vec!["low".to_string(), "high".to_string()],
        wire: std::collections::BTreeMap::new(),
    }];
    config
        .providers
        .get_mut("local")
        .unwrap()
        .models
        .push(fast_model);
    config_store.save(&config).unwrap();
    let runtime = StudioRuntime::new(StudioStore::open_memory().await.unwrap(), config_store);

    let next = runtime
        .set_model_role(ModelRole::Planner, "local", "local-fast", None)
        .unwrap();

    assert_eq!(next.roles.planner.provider, "local");
    assert_eq!(next.roles.planner.model, "local-fast");
    assert_eq!(next.roles.planner.effort.as_str(), "low");
    let saved = runtime.config_store().load_or_default().unwrap();
    assert_eq!(saved.roles.planner, next.roles.planner);
    let _ = tokio::fs::remove_dir_all(home).await;
}

#[tokio::test]
async fn initialize_runtime_cancels_recovered_transient_interactions() {
    let store = StudioStore::open_memory().await.unwrap();
    let project = store.upsert_project("C:/work/recovered").await.unwrap();
    let session = store
        .create_session(&project.id, "Recovered", CompileMode::Auto)
        .await
        .unwrap();
    store
        .create_turn(
            &session.id,
            "turn-recovered",
            StudioTurnStatus::WaitingForModel,
            1,
        )
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
                name: "bash".to_string(),
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
    let studio_events = store
        .load_studio_events(&session.id, None, None)
        .await
        .unwrap();
    let cancelled_interactions = studio_events
        .iter()
        .filter(|event| {
            matches!(
                &event.kind,
                StudioEventKind::InteractionChanged { event }
                    if event.interaction.status == InteractionStatus::Cancelled
            )
        })
        .count();
    assert_eq!(cancelled_interactions, 2);
    let _ = tokio::fs::remove_dir_all(home).await;
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
        .create_session(&project.id, "UI runtime", CompileMode::Auto)
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
        .create_session(&project.id, "UI completion", CompileMode::Auto)
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

#[test]
fn counts_started_tool_items_for_self_learning_threshold() {
    let mut item = TracePart {
        turn_id: "turn".to_string(),
        item_id: "tool".to_string(),
        started_sequence: 1,
        revision: 0,
        kind: TracePartKind::Tool,
        status: TracePartStatus::Started,
        created_at: 1,
        updated_at: 1,
        source: TracePartSource::Model,
        text_channel: None,
        content: String::new(),
        attachments: Vec::new(),
        thinking_chunks: Vec::new(),
        tool: None,
        agent: None,
        inference: None,
        usage: None,
    };
    let started = TraceEvent {
        session_id: "session".to_string(),
        sequence: 1,
        timestamp: 1,
        kind: TraceEventKind::TracePartStarted { item: item.clone() },
    };
    item.status = TracePartStatus::Running;
    let running = TraceEvent {
        session_id: "session".to_string(),
        sequence: 2,
        timestamp: 2,
        kind: TraceEventKind::TracePartStarted { item },
    };

    assert_eq!(
        started_tool_snapshot_count(&[started.clone(), running.clone()]),
        2
    );
    assert_eq!(tool_call_count(&[started, running]), 1);
}

#[tokio::test]
async fn proposed_plan_tag_does_not_create_pending_confirmation_interaction() {
    let sse_body = concat!(
        "data: {\"choices\":[{\"delta\":{\"content\":\"<proposed_plan>\"},\"finish_reason\":null}]}\n\n",
        "data: {\"choices\":[{\"delta\":{\"content\":\"1. Inspect\\\\n2. Implement\"},\"finish_reason\":null}]}\n\n",
        "data: {\"choices\":[{\"delta\":{\"content\":\"</proposed_plan><final>Ready</final>\"},\"finish_reason\":null}]}\n\n",
        "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}],\"usage\":{\"prompt_tokens\":1,\"completion_tokens\":2,\"total_tokens\":3}}\n\n",
        "data: [DONE]\n\n"
    )
    .to_string();
    let (base_url, handle) = serve_sse_once(sse_body).await;
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let home = std::env::temp_dir().join(format!("pure-runtime-home-{unique}"));
    let workspace = std::env::temp_dir().join(format!("pure-runtime-workspace-{unique}"));
    tokio::fs::create_dir_all(&workspace).await.unwrap();
    let config_store = ConfigStore::new(crate::config::ConfigPaths::from_home(&home));
    config_store.save(&test_chat_config(base_url)).unwrap();
    let store = StudioStore::open_memory().await.unwrap();
    let runtime = StudioRuntime::new(store.clone(), config_store);
    let project = runtime.open_project(&workspace).await.unwrap();
    let session = store
        .create_session(&project.id, "Plan test", CompileMode::Plan)
        .await
        .unwrap();
    let interaction_events = std::sync::Arc::new(Mutex::new(Vec::new()));
    let interaction_emitter = emitter(interaction_events.clone());
    let interaction_callback = runtime
        .interactions()
        .callback(session.id.clone(), interaction_emitter.clone());

    let outcome = runtime
        .run_prompt(RunPromptRequest {
            session_id: session.id.clone(),
            turn_id: "turn-plan-test".to_string(),
            prompt: "make a plan".to_string(),
            attachment_ids: Vec::new(),
            interaction_callback,
            interaction_emitter,
            options: TurnOptions::default(),
        })
        .await
        .unwrap();
    handle.await.unwrap();

    assert_eq!(outcome.result.status, TurnResultStatus::Completed);
    assert!(outcome.result.content.contains("Ready"));
    let plan_item = outcome
        .trace_events
        .iter()
        .find_map(|event| match &event.kind {
            TraceEventKind::TracePartCompleted { item } if item.kind == TracePartKind::Plan => {
                Some(item)
            }
            TraceEventKind::TracePartStarted { .. }
            | TraceEventKind::TracePartDelta { .. }
            | TraceEventKind::TracePartCompleted { .. }
            | TraceEventKind::TracePartFailed { .. }
            | TraceEventKind::PlanLifecycleChanged { .. }
            | TraceEventKind::InteractionChanged { .. }
            | TraceEventKind::SkillActivated { .. }
            | TraceEventKind::EnabledToolsRecorded { .. } => None,
        });
    assert!(plan_item.is_none());
    assert!(outcome.trace_events.iter().any(|event| matches!(
        &event.kind,
        TraceEventKind::TracePartCompleted { item }
            if item.text_channel == Some(TraceTextChannel::Final)
                && item.content.contains("Ready")
    )));
    let studio_events = store
        .load_studio_events(&session.id, None, None)
        .await
        .unwrap();
    assert!(!studio_events.iter().any(|envelope| {
        matches!(
            &envelope.kind,
            StudioEventKind::PlanLifecycleChanged { event }
                if event.state == PlanLifecycleState::PendingConfirmation
        )
    }));
    assert!(interaction_events.lock().await.is_empty());
    let _ = tokio::fs::remove_dir_all(home).await;
    let _ = tokio::fs::remove_dir_all(workspace).await;
}

#[tokio::test]
async fn plan_exit_tool_creates_pending_confirmation_interaction() {
    let tool_sse = concat!(
        "data: {\"type\":\"response.output_item.added\",\"item\":{\"type\":\"function_call\",\"id\":\"fc_1\",\"call_id\":\"call_1\",\"name\":\"plan_exit\"}}\n\n",
        "data: {\"type\":\"response.function_call_arguments.delta\",\"item_id\":\"fc_1\",\"call_id\":\"call_1\",\"delta\":\"{\\\"content\\\":\\\"# Plan\\\\n\\\\n- Inspect\\\\n- Implement\\\"}\"}\n\n",
        "data: {\"type\":\"response.output_item.done\",\"item\":{\"type\":\"function_call\",\"id\":\"fc_1\",\"call_id\":\"call_1\",\"name\":\"plan_exit\",\"arguments\":\"{\\\"content\\\":\\\"# Plan\\\\n\\\\n- Inspect\\\\n- Implement\\\"}\"}}\n\n",
        "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_1\",\"usage\":{\"input_tokens\":1,\"output_tokens\":2,\"total_tokens\":3}}}\n\n",
        "data: [DONE]\n\n"
    )
    .to_string();
    let final_sse = concat!(
        "data: {\"type\":\"response.output_item.added\",\"item\":{\"id\":\"msg_2\",\"type\":\"message\",\"role\":\"assistant\",\"phase\":\"final_answer\"}}\n\n",
        "data: {\"type\":\"response.output_text.delta\",\"item_id\":\"msg_2\",\"delta\":\"Plan submitted.\"}\n\n",
        "data: {\"type\":\"response.output_item.done\",\"item\":{\"id\":\"msg_2\",\"type\":\"message\",\"role\":\"assistant\",\"phase\":\"final_answer\",\"content\":[{\"type\":\"output_text\",\"text\":\"Plan submitted.\"}]}}\n\n",
        "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_2\",\"usage\":{\"input_tokens\":2,\"output_tokens\":1,\"total_tokens\":3}}}\n\n",
        "data: [DONE]\n\n"
    )
    .to_string();
    let (base_url, handle) = serve_sse_sequence(vec![tool_sse, final_sse]).await;
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let home = std::env::temp_dir().join(format!("pure-runtime-home-{unique}"));
    let workspace = std::env::temp_dir().join(format!("pure-runtime-workspace-{unique}"));
    tokio::fs::create_dir_all(&workspace).await.unwrap();
    let config_store = ConfigStore::new(crate::config::ConfigPaths::from_home(&home));
    config_store.save(&test_config(base_url)).unwrap();
    let store = StudioStore::open_memory().await.unwrap();
    let runtime = StudioRuntime::new(store.clone(), config_store);
    let project = runtime.open_project(&workspace).await.unwrap();
    let session = store
        .create_session(&project.id, "Plan exit test", CompileMode::Plan)
        .await
        .unwrap();
    let interaction_events = std::sync::Arc::new(Mutex::new(Vec::new()));
    let interaction_emitter = emitter(interaction_events.clone());
    let interaction_callback = runtime
        .interactions()
        .callback(session.id.clone(), interaction_emitter.clone());

    let outcome = runtime
        .run_prompt(RunPromptRequest {
            session_id: session.id.clone(),
            turn_id: "turn-plan-exit-test".to_string(),
            prompt: "make a plan".to_string(),
            attachment_ids: Vec::new(),
            interaction_callback,
            interaction_emitter,
            options: TurnOptions::default(),
        })
        .await
        .unwrap();
    handle.await.unwrap();

    assert_eq!(outcome.result.status, TurnResultStatus::Completed);
    assert_eq!(outcome.result.content, "Plan submitted.");
    let plan_item = outcome
        .trace_events
        .iter()
        .rev()
        .find_map(|event| match &event.kind {
            TraceEventKind::TracePartCompleted { item } if item.kind == TracePartKind::Plan => {
                Some(item)
            }
            TraceEventKind::TracePartStarted { .. }
            | TraceEventKind::TracePartDelta { .. }
            | TraceEventKind::TracePartCompleted { .. }
            | TraceEventKind::TracePartFailed { .. }
            | TraceEventKind::PlanLifecycleChanged { .. }
            | TraceEventKind::InteractionChanged { .. }
            | TraceEventKind::SkillActivated { .. }
            | TraceEventKind::EnabledToolsRecorded { .. } => None,
        })
        .expect("completed plan item");
    assert_eq!(plan_item.content, "# Plan\n\n- Inspect\n- Implement");

    let interaction = store
        .read_interaction(&plan_confirmation_id(&plan_item.item_id))
        .await
        .unwrap()
        .expect("plan confirmation interaction");
    assert_eq!(interaction.kind, InteractionKind::PlanConfirmation);
    assert_eq!(interaction.status, InteractionStatus::Pending);
    assert_eq!(
        interaction.payload,
        InteractionPayload::PlanConfirmation {
            plan_id: plan_item.item_id.clone(),
            content: "# Plan\n\n- Inspect\n- Implement".to_string(),
        }
    );
    let studio_events = store
        .load_studio_events(&session.id, None, None)
        .await
        .unwrap();
    assert!(studio_events.iter().any(|envelope| {
        matches!(
            &envelope.kind,
            StudioEventKind::PlanLifecycleChanged { event }
                if event.plan_id == plan_item.item_id
                    && event.state == PlanLifecycleState::PendingConfirmation
        )
    }));
    assert_eq!(interaction_events.lock().await.len(), 1);
    let _ = tokio::fs::remove_dir_all(home).await;
    let _ = tokio::fs::remove_dir_all(workspace).await;
}

#[tokio::test]
async fn tool_boundary_with_reused_provider_ids_creates_new_parts_after_tool() {
    let tool_sse = concat!(
        "data: {\"type\":\"response.reasoning_summary_text.delta\",\"item_id\":\"thinking\",\"delta\":\"before tool\"}\n\n",
        "data: {\"type\":\"response.output_text.delta\",\"item_id\":\"msg_1\",\"delta\":\"before \"}\n\n",
        "data: {\"type\":\"response.output_item.added\",\"item\":{\"type\":\"function_call\",\"id\":\"fc_1\",\"call_id\":\"call_1\",\"name\":\"list_files\"}}\n\n",
        "data: {\"type\":\"response.function_call_arguments.delta\",\"item_id\":\"fc_1\",\"call_id\":\"call_1\",\"delta\":\"{\\\"path\\\":\\\".\\\"}\"}\n\n",
        "data: {\"type\":\"response.output_item.done\",\"item\":{\"type\":\"function_call\",\"id\":\"fc_1\",\"call_id\":\"call_1\",\"name\":\"list_files\",\"arguments\":\"{\\\"path\\\":\\\".\\\"}\"}}\n\n",
        "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_1\",\"usage\":{\"input_tokens\":1,\"output_tokens\":2,\"total_tokens\":3}}}\n\n",
        "data: [DONE]\n\n"
    )
    .to_string();
    let final_sse = concat!(
        "data: {\"type\":\"response.reasoning_summary_text.delta\",\"item_id\":\"thinking\",\"delta\":\"after tool\"}\n\n",
        "data: {\"type\":\"response.output_text.delta\",\"item_id\":\"msg_1\",\"delta\":\"after\"}\n\n",
        "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_2\",\"usage\":{\"input_tokens\":2,\"output_tokens\":1,\"total_tokens\":3}}}\n\n",
        "data: [DONE]\n\n"
    )
    .to_string();
    let (base_url, handle) = serve_sse_sequence(vec![tool_sse, final_sse]).await;
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let home = std::env::temp_dir().join(format!("pure-runtime-home-{unique}"));
    let workspace = std::env::temp_dir().join(format!("pure-runtime-workspace-{unique}"));
    tokio::fs::create_dir_all(&workspace).await.unwrap();
    tokio::fs::write(workspace.join("alpha.txt"), "alpha")
        .await
        .unwrap();
    let config_store = ConfigStore::new(crate::config::ConfigPaths::from_home(&home));
    config_store.save(&test_config(base_url)).unwrap();
    let store = StudioStore::open_memory().await.unwrap();
    let runtime = StudioRuntime::new(store.clone(), config_store);
    let project = runtime.open_project(&workspace).await.unwrap();
    let session = store
        .create_session(&project.id, "Tool boundary test", CompileMode::Auto)
        .await
        .unwrap();
    let interaction_events = std::sync::Arc::new(Mutex::new(Vec::new()));
    let interaction_emitter = emitter(interaction_events.clone());
    let interaction_callback = runtime
        .interactions()
        .callback(session.id.clone(), interaction_emitter.clone());

    let outcome = runtime
        .run_prompt(RunPromptRequest {
            session_id: session.id.clone(),
            turn_id: "turn-tool-boundary-test".to_string(),
            prompt: "list files and continue".to_string(),
            attachment_ids: Vec::new(),
            interaction_callback,
            interaction_emitter,
            options: TurnOptions::default(),
        })
        .await
        .unwrap();
    handle.await.unwrap();

    assert_eq!(outcome.result.status, TurnResultStatus::Completed);
    assert_eq!(outcome.result.content, "after");

    let parts = store.load_message_parts(&session.id).await.unwrap();
    let assistant_parts = parts
        .iter()
        .filter(|record| {
            record.part.message_id == "turn-tool-boundary-test:assistant" && !record.part.synthetic
        })
        .map(|record| &record.part)
        .collect::<Vec<_>>();
    let compact = assistant_parts
        .iter()
        .filter_map(|part| match part.part_type {
            pl_protocol::StudioPartType::Reasoning | pl_protocol::StudioPartType::Text => Some((
                part.part_id.as_str(),
                part.part_type,
                part.text.as_str(),
                part.order,
            )),
            pl_protocol::StudioPartType::Tool
            | pl_protocol::StudioPartType::Agent
            | pl_protocol::StudioPartType::Turn
            | pl_protocol::StudioPartType::Inference
            | pl_protocol::StudioPartType::Plan
            | pl_protocol::StudioPartType::File => None,
        })
        .collect::<Vec<_>>();
    let compact_identity = compact
        .iter()
        .map(|(part_id, part_type, text, _)| {
            assert!(part_id.starts_with("turn-tool-boundary-test:part-"));
            (*part_type, *text)
        })
        .collect::<Vec<_>>();

    assert_eq!(
        compact_identity,
        vec![
            (pl_protocol::StudioPartType::Reasoning, "before tool",),
            (pl_protocol::StudioPartType::Text, "before ",),
            (pl_protocol::StudioPartType::Reasoning, "after tool",),
            (pl_protocol::StudioPartType::Text, "after",),
        ]
    );
    assert!(compact[0].3 < compact[1].3);
    assert!(compact[1].3 < compact[2].3);
    assert!(compact[2].3 < compact[3].3);
    let tool = assistant_parts
        .iter()
        .find(|part| part.part_type == pl_protocol::StudioPartType::Tool)
        .expect("tool part");
    assert!(tool.part_id.starts_with("turn-tool-boundary-test:part-"));
    assert_eq!(
        tool.tool
            .as_ref()
            .and_then(|tool| tool.provider_item_id.as_deref()),
        Some("fc_1")
    );
    assert!(tool.order > compact[1].3 && tool.order < compact[2].3);

    let _ = tokio::fs::remove_dir_all(home).await;
    let _ = tokio::fs::remove_dir_all(workspace).await;
}

#[tokio::test]
async fn late_responses_phase_reopens_text_block_as_new_part() {
    let sse_body = concat!(
        "data: {\"type\":\"response.output_text.delta\",\"item_id\":\"msg_1\",\"delta\":\"default \"}\n\n",
        "data: {\"type\":\"response.output_item.added\",\"item\":{\"id\":\"msg_1\",\"type\":\"message\",\"role\":\"assistant\",\"phase\":\"commentary\"}}\n\n",
        "data: {\"type\":\"response.output_text.delta\",\"item_id\":\"msg_1\",\"delta\":\"commentary\"}\n\n",
        "data: {\"type\":\"response.output_item.done\",\"item\":{\"id\":\"msg_1\",\"type\":\"message\",\"role\":\"assistant\",\"phase\":\"commentary\",\"content\":[{\"type\":\"output_text\",\"text\":\"commentary\"}]}}\n\n",
        "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_1\",\"usage\":{\"input_tokens\":1,\"output_tokens\":2,\"total_tokens\":3}}}\n\n",
        "data: [DONE]\n\n"
    )
    .to_string();
    let (base_url, handle) = serve_sse_once(sse_body).await;
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let home = std::env::temp_dir().join(format!("pure-late-phase-home-{unique}"));
    let workspace = std::env::temp_dir().join(format!("pure-late-phase-workspace-{unique}"));
    tokio::fs::create_dir_all(&workspace).await.unwrap();
    let config_store = ConfigStore::new(crate::config::ConfigPaths::from_home(&home));
    config_store.save(&test_config(base_url)).unwrap();
    let store = StudioStore::open_memory().await.unwrap();
    let runtime = StudioRuntime::new(store.clone(), config_store);
    let project = runtime.open_project(&workspace).await.unwrap();
    let session = store
        .create_session(&project.id, "Late phase test", CompileMode::Auto)
        .await
        .unwrap();
    let interaction_events = std::sync::Arc::new(Mutex::new(Vec::new()));
    let interaction_emitter = emitter(interaction_events.clone());
    let interaction_callback = runtime
        .interactions()
        .callback(session.id.clone(), interaction_emitter.clone());

    let outcome = runtime
        .run_prompt(RunPromptRequest {
            session_id: session.id.clone(),
            turn_id: "turn-late-phase-test".to_string(),
            prompt: "stream late phase".to_string(),
            attachment_ids: Vec::new(),
            interaction_callback,
            interaction_emitter,
            options: TurnOptions::default(),
        })
        .await
        .unwrap();
    handle.await.unwrap();

    assert_eq!(outcome.result.status, TurnResultStatus::Completed);
    assert_eq!(outcome.result.content, "default ");

    let parts = store.load_message_parts(&session.id).await.unwrap();
    let text_parts = parts
        .iter()
        .filter_map(|record| {
            (record.part.message_id == "turn-late-phase-test:assistant"
                && record.part.part_type == pl_protocol::StudioPartType::Text
                && !record.part.synthetic)
                .then_some(&record.part)
        })
        .collect::<Vec<_>>();

    assert_eq!(text_parts.len(), 2);
    assert_ne!(text_parts[0].part_id, text_parts[1].part_id);
    assert!(text_parts[0].order < text_parts[1].order);
    assert_eq!(text_parts[0].text_channel, Some(StudioTextChannel::Final));
    assert_eq!(text_parts[0].text, "default ");
    assert_eq!(
        text_parts[1].text_channel,
        Some(StudioTextChannel::Commentary)
    );
    assert_eq!(text_parts[1].text, "commentary");

    let _ = tokio::fs::remove_dir_all(home).await;
    let _ = tokio::fs::remove_dir_all(workspace).await;
}

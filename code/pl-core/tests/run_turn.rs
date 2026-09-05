#![allow(linker_messages)]

mod support;

use std::collections::HashMap;

use pretty_assertions::assert_eq;

use support::{
    TestHttpResponse, route, serve_http_sequence, serve_sse_once, serve_sse_sequence,
    serve_sse_sequence_with_raw_requests,
};

use pl_core::tool::{
    AgentToolSet, BeforeModelStepHook, DynTool, GlobalToolInheritance, StaticTool,
    StaticToolDefinition, ToolCallContext, ToolGroupId, ToolInstallGroup, ToolManager, ToolName,
    ToolPolicy, ToolResult,
};
use pl_core::turn::TurnBudget;
use pl_core::{
    AgentSession, ContextCompactionConfig, ContextCompactionPhase, ContextCompactionTrigger,
    CoreRuntimeProfile, ManualContextCompactionRequest, ModelTurnClient, ModelTurnOptions,
    ModelTurnRequest, PureError, TraceRecorder, TurnEngineBuilder, TurnOptions, TurnRequest,
};
use pl_model::completion::OpenAiCompactionMode;
use pl_model::model::ModelInfo;
use pl_model::provider::ProviderEndpoint;
use pl_protocol::{Message, MessageContent, MessagePresentation, MessageRole};
use pl_trace::{TraceEvent, TraceEventKind, TracePartKind, TracePartSource, TraceTextChannel};

#[tokio::test]
async fn run_turn_records_user_trace_part_before_internal_parts() {
    let sse_body = concat!(
        "data: {\"type\":\"response.output_item.added\",\"item\":{\"id\":\"msg_1\",\"type\":\"message\",\"role\":\"assistant\",\"phase\":\"final_answer\"}}\n\n",
        "data: {\"type\":\"response.output_text.delta\",\"item_id\":\"msg_1\",\"delta\":\"ok\"}\n\n",
        "data: {\"type\":\"response.output_item.done\",\"item\":{\"id\":\"msg_1\",\"type\":\"message\",\"role\":\"assistant\",\"phase\":\"final_answer\",\"content\":[{\"type\":\"output_text\",\"text\":\"ok\"}]}}\n\n",
        "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_1\",\"usage\":{\"input_tokens\":1,\"output_tokens\":2,\"total_tokens\":3}}}\n\n",
        "data: [DONE]\n\n"
    )
    .to_string();
    let (base_url, handle) = serve_sse_once(sse_body).await;
    let mut endpoint = ProviderEndpoint::openai(Some(base_url));
    endpoint.bearer_token = Some("test-token".to_string());
    let core = test_turn_engine_builder(endpoint, local_responses_model()).build();
    let (event_tx, _event_rx) = tokio::sync::broadcast::channel(32);
    let mut recorder = TraceRecorder::new("session-1".to_string(), event_tx, 0);
    let mut session = AgentSession::new();

    let result = core
        .run_turn_with_trace(
            &mut session,
            TurnRequest::new("Build the thing".to_string())
                .with_budget(TurnBudget::new(std::time::Duration::from_millis(60_000))),
            &mut recorder,
            TurnOptions::default(),
        )
        .await
        .unwrap();
    handle.await.unwrap();

    assert!(result.is_completed());
    let events = &result.trace_events;
    let started_kinds = trace_started_kinds(events);
    assert_eq!(started_kinds[0], TracePartKind::Text);
    assert_eq!(started_kinds[1], TracePartKind::Turn);
    assert!(started_kinds[2..].contains(&TracePartKind::Inference));

    let user_item = events
        .iter()
        .find_map(|event| match &event.kind {
            TraceEventKind::TracePartStarted { item }
                if item.kind() == TracePartKind::Text
                    && item
                        .text()
                        .is_some_and(|text| text.channel() == TraceTextChannel::User) =>
            {
                Some(item)
            }
            TraceEventKind::TracePartStarted { .. }
            | TraceEventKind::TracePartDelta { .. }
            | TraceEventKind::TracePartCompleted { .. }
            | TraceEventKind::TracePartFailed { .. }
            | TraceEventKind::InteractionChanged { .. }
            | TraceEventKind::SkillActivated { .. }
            | TraceEventKind::EnabledToolsRecorded { .. } => None,
        })
        .expect("user trace part");
    assert_eq!(user_item.started_sequence(), 0);
    assert_eq!(
        user_item.text().expect("user text part").content(),
        "Build the thing",
    );
}

#[tokio::test]
async fn hidden_turn_input_remains_in_provider_context_and_session_protocol() {
    let sse_body = concat!(
        "data: {\"type\":\"response.output_item.added\",\"item\":{\"id\":\"msg_1\",\"type\":\"message\",\"role\":\"assistant\",\"phase\":\"final_answer\"}}\n\n",
        "data: {\"type\":\"response.output_text.delta\",\"item_id\":\"msg_1\",\"delta\":\"ok\"}\n\n",
        "data: {\"type\":\"response.output_item.done\",\"item\":{\"id\":\"msg_1\",\"type\":\"message\",\"role\":\"assistant\",\"phase\":\"final_answer\",\"content\":[{\"type\":\"output_text\",\"text\":\"ok\"}]}}\n\n",
        "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_1\",\"usage\":{\"input_tokens\":1,\"output_tokens\":2,\"total_tokens\":3}}}}\n\n",
        "data: [DONE]\n\n"
    )
    .to_string();
    let (base_url, requests, handle) = serve_sse_sequence(vec![sse_body]).await;
    let mut endpoint = ProviderEndpoint::openai(Some(base_url));
    endpoint.bearer_token = Some("test-token".to_string());
    let core = test_turn_engine_builder(endpoint, local_responses_model()).build();
    let (event_tx, _event_rx) = tokio::sync::broadcast::channel(32);
    let mut recorder = TraceRecorder::new("session-hidden".to_string(), event_tx, 0);
    let mut session = AgentSession::new();

    core.run_turn_with_trace(
        &mut session,
        TurnRequest::new("# Approved Plan\n\nImplement it.")
            .with_user_presentation(MessagePresentation::Hidden),
        &mut recorder,
        TurnOptions::default(),
    )
    .await
    .unwrap();
    handle.await.unwrap();

    let user = session
        .messages()
        .iter()
        .find(|message| message.role == MessageRole::User)
        .expect("hidden user message remains canonical");
    assert_eq!(user.presentation, MessagePresentation::Hidden);
    let requests = requests.lock().unwrap();
    assert!(
        requests[0]
            .to_string()
            .contains("# Approved Plan\\n\\nImplement it."),
        "Hidden controls GUI projection, not provider delivery"
    );
}

#[tokio::test]
async fn run_turn_emits_runtime_progress_commentary() {
    let sse_body = concat!(
        "data: {\"type\":\"response.output_item.added\",\"item\":{\"id\":\"msg_1\",\"type\":\"message\",\"role\":\"assistant\",\"phase\":\"final_answer\"}}\n\n",
        "data: {\"type\":\"response.output_text.delta\",\"item_id\":\"msg_1\",\"delta\":\"ok\"}\n\n",
        "data: {\"type\":\"response.output_item.done\",\"item\":{\"id\":\"msg_1\",\"type\":\"message\",\"role\":\"assistant\",\"phase\":\"final_answer\",\"content\":[{\"type\":\"output_text\",\"text\":\"ok\"}]}}\n\n",
        "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_1\",\"usage\":{\"input_tokens\":1,\"output_tokens\":2,\"total_tokens\":3}}}\n\n",
        "data: [DONE]\n\n"
    )
    .to_string();
    let (base_url, handle) = serve_sse_once(sse_body).await;
    let mut endpoint = ProviderEndpoint::openai(Some(base_url));
    endpoint.bearer_token = Some("test-token".to_string());
    let core = test_turn_engine_builder(endpoint, local_responses_model()).build();
    let (event_tx, mut event_rx) = tokio::sync::broadcast::channel(64);
    let mut recorder = TraceRecorder::new("session-1".to_string(), event_tx, 0);
    let mut session = AgentSession::new();

    let result = core
        .run_turn_with_trace(
            &mut session,
            TurnRequest::new("Build the thing".to_string())
                .with_budget(TurnBudget::new(std::time::Duration::from_millis(60_000))),
            &mut recorder,
            TurnOptions::default(),
        )
        .await
        .unwrap();
    handle.await.unwrap();

    assert!(result.is_completed());
    assert_eq!(
        runtime_progress_texts(&mut event_rx),
        vec![
            "已接收请求，正在准备上下文。".to_string(),
            "上下文已整理，准备调用模型。".to_string(),
            "模型已完成正文生成。".to_string(),
            "本轮已完成。".to_string(),
        ]
    );
}

#[tokio::test]
async fn run_turn_persists_only_final_text_to_session_history() {
    let sse_body = concat!(
        "data: {\"type\":\"response.output_item.added\",\"item\":{\"id\":\"msg_commentary\",\"type\":\"message\",\"role\":\"assistant\",\"phase\":\"commentary\"}}\n\n",
        "data: {\"type\":\"response.output_text.delta\",\"item_id\":\"msg_commentary\",\"delta\":\"正在检查。\"}\n\n",
        "data: {\"type\":\"response.output_item.done\",\"item\":{\"id\":\"msg_commentary\",\"type\":\"message\",\"role\":\"assistant\",\"phase\":\"commentary\",\"content\":[{\"type\":\"output_text\",\"text\":\"正在检查。\"}]}}\n\n",
        "data: {\"type\":\"response.output_item.added\",\"item\":{\"id\":\"msg_final\",\"type\":\"message\",\"role\":\"assistant\",\"phase\":\"final_answer\"}}\n\n",
        "data: {\"type\":\"response.output_text.delta\",\"item_id\":\"msg_final\",\"delta\":\"Done\"}\n\n",
        "data: {\"type\":\"response.output_item.done\",\"item\":{\"id\":\"msg_final\",\"type\":\"message\",\"role\":\"assistant\",\"phase\":\"final_answer\",\"content\":[{\"type\":\"output_text\",\"text\":\"Done\"}]}}\n\n",
        "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_1\",\"usage\":{\"input_tokens\":1,\"output_tokens\":2,\"total_tokens\":3}}}\n\n",
        "data: [DONE]\n\n"
    )
    .to_string();
    let (base_url, handle) = serve_sse_once(sse_body).await;
    let mut endpoint = ProviderEndpoint::openai(Some(base_url));
    endpoint.bearer_token = Some("test-token".to_string());
    let core = test_turn_engine_builder(endpoint, local_responses_model()).build();
    let (event_tx, _event_rx) = tokio::sync::broadcast::channel(32);
    let mut recorder = TraceRecorder::new("session-1".to_string(), event_tx, 0);
    let mut session = AgentSession::new();

    let result = core
        .run_turn_with_trace(
            &mut session,
            TurnRequest::new("Build the thing".to_string())
                .with_budget(TurnBudget::new(std::time::Duration::from_millis(60_000))),
            &mut recorder,
            TurnOptions::default(),
        )
        .await
        .unwrap();
    handle.await.unwrap();

    assert!(result.is_completed());
    assert_eq!(result.content, "Done");
    let assistant_messages = session
        .messages()
        .iter()
        .filter(|message| message.role == MessageRole::Assistant)
        .collect::<Vec<_>>();
    assert_eq!(assistant_messages.len(), 1);
    assert_eq!(
        assistant_messages[0].content,
        MessageContent::text("Done".to_string())
    );
}

#[tokio::test]
async fn run_turn_exposes_context_compaction_snapshot() {
    let compact_sse = concat!(
        "data: {\"type\":\"response.output_item.added\",\"item\":{\"id\":\"msg_compact\",\"type\":\"message\",\"role\":\"assistant\",\"phase\":\"final_answer\"}}\n\n",
        "data: {\"type\":\"response.output_text.delta\",\"item_id\":\"msg_compact\",\"delta\":\"compressed memory\"}\n\n",
        "data: {\"type\":\"response.output_item.done\",\"item\":{\"id\":\"msg_compact\",\"type\":\"message\",\"role\":\"assistant\",\"phase\":\"final_answer\",\"content\":[{\"type\":\"output_text\",\"text\":\"compressed memory\"}]}}\n\n",
        "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_compact\",\"usage\":{\"input_tokens\":9,\"output_tokens\":2,\"total_tokens\":11}}}\n\n",
        "data: [DONE]\n\n"
    )
    .to_string();
    let answer_sse = concat!(
        "data: {\"type\":\"response.output_item.added\",\"item\":{\"id\":\"msg_answer\",\"type\":\"message\",\"role\":\"assistant\",\"phase\":\"final_answer\"}}\n\n",
        "data: {\"type\":\"response.output_text.delta\",\"item_id\":\"msg_answer\",\"delta\":\"ok\"}\n\n",
        "data: {\"type\":\"response.output_item.done\",\"item\":{\"id\":\"msg_answer\",\"type\":\"message\",\"role\":\"assistant\",\"phase\":\"final_answer\",\"content\":[{\"type\":\"output_text\",\"text\":\"ok\"}]}}\n\n",
        "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_answer\",\"usage\":{\"input_tokens\":3,\"output_tokens\":1,\"total_tokens\":4}}}\n\n",
        "data: [DONE]\n\n"
    )
    .to_string();
    let (base_url, _requests, handle) = serve_sse_sequence(vec![compact_sse, answer_sse]).await;
    let mut endpoint = ProviderEndpoint::openai(Some(base_url));
    endpoint.bearer_token = Some("test-token".to_string());
    let mut model = local_responses_model();
    model.auto_compact_token_limit = Some(1);
    let core = test_turn_engine_builder(endpoint, model)
        .with_runtime_profile(CoreRuntimeProfile::minimal().with_context_compaction(
            ContextCompactionConfig::default().with_openai_mode(OpenAiCompactionMode::Local),
        ))
        .build();
    let (event_tx, _event_rx) = tokio::sync::broadcast::channel(32);
    let mut recorder = TraceRecorder::new("session-1".to_string(), event_tx, 0);
    let mut session = AgentSession::new();
    session.push_user_prompt("old context".to_string());

    let result = core
        .run_turn_with_trace(
            &mut session,
            TurnRequest::new("continue".to_string())
                .with_turn_id("turn-compaction")
                .with_budget(TurnBudget::new(std::time::Duration::from_millis(60_000))),
            &mut recorder,
            TurnOptions::default(),
        )
        .await
        .unwrap();
    handle.await.unwrap();

    assert!(result.is_completed());
    assert_eq!(result.context_compactions.len(), 1);
    let compaction = &result.context_compactions[0];
    assert_eq!(compaction.summary.as_deref(), Some("compressed memory"));
    assert_eq!(compaction.trigger.as_str(), "estimatedTokens");
    assert_eq!(compaction.provider_prompt_tokens, None);
    assert_eq!(compaction.auto_compact_limit, 1);
    let usages = result
        .trace_events
        .iter()
        .filter_map(|event| match &event.kind {
            TraceEventKind::TracePartCompleted { item } => match item.state() {
                pl_trace::TracePartState::Inference(inference) => inference.state().usage(),
                pl_trace::TracePartState::Text(_)
                | pl_trace::TracePartState::Thinking(_)
                | pl_trace::TracePartState::Tool(_)
                | pl_trace::TracePartState::Agent(_)
                | pl_trace::TracePartState::Turn(_) => None,
            },
            TraceEventKind::TracePartStarted { .. }
            | TraceEventKind::TracePartDelta { .. }
            | TraceEventKind::TracePartFailed { .. }
            | TraceEventKind::InteractionChanged { .. }
            | TraceEventKind::SkillActivated { .. }
            | TraceEventKind::EnabledToolsRecorded { .. } => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(usages.len(), 1);
    assert_eq!(result.usage.total_tokens, 15);
    assert!(
        compaction
            .replacement_tokens
            .is_some_and(|tokens| tokens > 0)
    );
}

#[tokio::test]
async fn manual_compaction_runs_standalone_for_single_message_and_resets_history() {
    let compact_sse = concat!(
        "data: {\"type\":\"response.output_item.added\",\"item\":{\"id\":\"msg_compact\",\"type\":\"message\",\"role\":\"assistant\",\"phase\":\"final_answer\"}}\n\n",
        "data: {\"type\":\"response.output_text.delta\",\"item_id\":\"msg_compact\",\"delta\":\"manual summary\"}\n\n",
        "data: {\"type\":\"response.output_item.done\",\"item\":{\"id\":\"msg_compact\",\"type\":\"message\",\"role\":\"assistant\",\"phase\":\"final_answer\",\"content\":[{\"type\":\"output_text\",\"text\":\"manual summary\"}]}}\n\n",
        "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_compact\",\"usage\":{\"input_tokens\":5,\"output_tokens\":2,\"total_tokens\":7}}}\n\n",
        "data: [DONE]\n\n"
    )
    .to_string();
    let (base_url, _requests, handle) = serve_sse_sequence(vec![compact_sse]).await;
    let mut endpoint = ProviderEndpoint::openai(Some(base_url));
    endpoint.bearer_token = Some("test-token".to_string());
    let core = test_turn_engine_builder(endpoint, local_responses_model())
        .with_runtime_profile(CoreRuntimeProfile::minimal().with_context_compaction(
            ContextCompactionConfig::default().with_openai_mode(OpenAiCompactionMode::Local),
        ))
        .build();
    let mut session = AgentSession::from_messages(vec![Message {
        presentation: Default::default(),
        role: MessageRole::User,
        content: MessageContent::text("only message".to_string()),
        reasoning_content: None,
        tool_calls: None,
        tool_result: None,
        metadata: HashMap::new(),
    }]);
    let original_revision = session.revision();

    let snapshot = core
        .compact_session(&mut session, ManualContextCompactionRequest::new())
        .await
        .unwrap()
        .unwrap();
    handle.await.unwrap();

    assert_eq!(snapshot.trigger, ContextCompactionTrigger::Manual);
    assert_eq!(snapshot.phase, ContextCompactionPhase::Standalone);
    assert_eq!(snapshot.summary.as_deref(), Some("manual summary"));
    assert_eq!(session.revision(), original_revision + 1);
    // 持久化 wire 契约：压缩摘要消息以 context_compaction metadata 键标记。
    assert!(
        session
            .messages()
            .last()
            .is_some_and(|message| message.metadata.contains_key("context_compaction"))
    );
}

#[tokio::test]
async fn custom_openai_endpoint_omits_responses_hosted_tools_by_default() {
    let request = capture_default_tools_request().await;
    let tool_types = request["tools"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|tool| tool["type"].as_str())
        .collect::<Vec<_>>();

    assert!(!tool_types.contains(&"tool_search"));
    assert!(!tool_types.contains(&"programmatic_tool_calling"));
}

#[tokio::test]
async fn before_model_step_adds_replaces_and_removes_tools_for_the_next_step() {
    let responses = vec![
        tool_call_sse("alpha", "step_alpha"),
        tool_call_sse("beta", "step_beta"),
        final_sse("dynamic-complete", "done"),
    ];
    let (base_url, bodies, handle) = serve_sse_sequence(responses).await;
    let mut endpoint = ProviderEndpoint::openai(Some(base_url));
    endpoint.bearer_token = Some("test-token".to_string());
    let manager = ToolManager::new();
    let agent_tools = manager.agent_tool_set("root", GlobalToolInheritance::Isolated);
    let other_agent = manager.agent_tool_set("other", GlobalToolInheritance::Isolated);
    let hook = BeforeModelStepHook::new(|context| async move {
        let group = ToolGroupId::new("dynamic-step");
        match context.step {
            0 => context
                .agent_tools
                .install(pl_core::tool::ToolInstallGroup::direct(
                    group,
                    vec![step_tool("step_alpha", "alpha")],
                ))?,
            1 => context
                .agent_tools
                .install(pl_core::tool::ToolInstallGroup::direct(
                    group,
                    vec![step_tool("step_beta", "beta")],
                ))?,
            2 => {
                context.agent_tools.uninstall(&group);
            }
            _ => {}
        }
        Ok(())
    });
    let core = test_turn_engine_builder(endpoint, local_responses_model())
        .with_agent_tool_set(agent_tools)
        .with_before_model_step(hook)
        .build();
    let (event_tx, _) = tokio::sync::broadcast::channel(32);
    let mut recorder = TraceRecorder::new("session-dynamic-tools".to_string(), event_tx, 0);
    let mut session = AgentSession::new();

    let result = core
        .run_turn_with_trace(
            &mut session,
            TurnRequest::new("exercise dynamic tools".to_string())
                .with_budget(TurnBudget::new(std::time::Duration::from_millis(60_000))),
            &mut recorder,
            TurnOptions::default(),
        )
        .await
        .expect("dynamic tool turn");
    handle.await.expect("fixture provider");

    assert!(result.is_completed());
    let bodies = bodies.lock().unwrap();
    assert_eq!(request_tool_names(&bodies[0]), vec!["step_alpha"]);
    assert_eq!(request_tool_names(&bodies[1]), vec!["step_beta"]);
    assert!(request_tool_names(&bodies[2]).is_empty());
    assert!(other_agent.freeze().specs().is_empty());
}

#[tokio::test]
async fn model_transport_retry_reuses_the_same_frozen_tool_plan() {
    let success = final_sse("retry-plan", "retry ok");
    let (base_url, bodies, handle) = serve_http_sequence(vec![
        TestHttpResponse {
            status: 500,
            content_type: "application/json",
            body: serde_json::json!({
                "error": {"message": "temporary provider failure", "type": "server_error"}
            })
            .to_string(),
        },
        TestHttpResponse::sse(success),
    ])
    .await;
    let mut endpoint = ProviderEndpoint::openai(Some(base_url));
    endpoint.bearer_token = Some("test-token".to_string());
    let manager = ToolManager::new();
    let agent_tools = manager.agent_tool_set("root", GlobalToolInheritance::Isolated);
    let refreshes = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let hook_refreshes = refreshes.clone();
    let hook = BeforeModelStepHook::new(move |context| {
        let hook_refreshes = hook_refreshes.clone();
        async move {
            hook_refreshes.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            context
                .agent_tools
                .install(pl_core::tool::ToolInstallGroup::direct(
                    ToolGroupId::new("retry"),
                    vec![step_tool("retry_visible", "retry")],
                ))
        }
    });
    let core = test_turn_engine_builder(endpoint, local_responses_model())
        .with_agent_tool_set(agent_tools)
        .with_before_model_step(hook)
        .build();
    let (event_tx, _) = tokio::sync::broadcast::channel(32);
    let mut recorder = TraceRecorder::new("session-retry-plan".to_string(), event_tx, 0);
    let mut session = AgentSession::new();

    let result = core
        .run_turn_with_trace(
            &mut session,
            TurnRequest::new("retry once".to_string())
                .with_budget(TurnBudget::new(std::time::Duration::from_millis(60_000))),
            &mut recorder,
            TurnOptions::default(),
        )
        .await
        .expect("retry succeeds");
    handle.await.expect("fixture provider");

    assert!(result.is_completed());
    assert_eq!(refreshes.load(std::sync::atomic::Ordering::SeqCst), 1);
    let bodies = bodies.lock().unwrap();
    assert_eq!(bodies.len(), 2);
    assert_eq!(bodies[0], bodies[1]);
}

#[tokio::test]
async fn randomized_registration_order_keeps_full_provider_request_bytes_identical() {
    let responses = (0..4)
        .map(|index| final_sse(&format!("wire-{index}"), "ok"))
        .collect();
    let (base_url, raw_requests, handle) = serve_sse_sequence_with_raw_requests(responses).await;
    let permutations = [
        vec![("zeta", "z"), ("alpha", "a"), ("middle", "m")],
        vec![("middle", "m"), ("zeta", "z"), ("alpha", "a")],
        vec![("alpha", "a"), ("middle", "m"), ("zeta", "z")],
        vec![("zeta", "z"), ("middle", "m"), ("alpha", "a")],
    ];

    for permutation in permutations {
        let mut endpoint = ProviderEndpoint::openai(Some(base_url.clone()));
        endpoint.bearer_token = Some("test-token".to_string());
        let manager = ToolManager::new();
        let agent_tools = manager.agent_tool_set("root", GlobalToolInheritance::Isolated);
        agent_tools
            .install(pl_core::tool::ToolInstallGroup::direct(
                ToolGroupId::new("wire"),
                permutation
                    .into_iter()
                    .map(|(name, output)| step_tool(name, output))
                    .collect(),
            ))
            .expect("install randomized tools");
        let core = test_turn_engine_builder(endpoint, local_responses_model())
            .with_agent_tool_set(agent_tools)
            .build();
        let (event_tx, _) = tokio::sync::broadcast::channel(16);
        let mut recorder = TraceRecorder::new("wire-session".to_string(), event_tx, 0);
        let mut session = AgentSession::new();
        core.run_turn_with_trace(
            &mut session,
            TurnRequest::new("stable request".to_string()),
            &mut recorder,
            TurnOptions::default(),
        )
        .await
        .expect("stable request turn");
    }
    handle.await.expect("fixture provider");

    let requests = raw_requests.lock().unwrap();
    assert_eq!(requests.len(), 4);
    for request in requests.iter().skip(1) {
        assert_eq!(request, &requests[0]);
    }
}

#[tokio::test]
async fn each_step_trace_pairs_tool_fingerprint_with_reported_provider_cache_usage() {
    let sse = concat!(
        "data: {\"type\":\"response.output_item.added\",\"item\":{\"id\":\"msg_cache_trace\",\"type\":\"message\",\"role\":\"assistant\",\"phase\":\"final_answer\"}}\n\n",
        "data: {\"type\":\"response.output_text.delta\",\"item_id\":\"msg_cache_trace\",\"delta\":\"ok\"}\n\n",
        "data: {\"type\":\"response.output_item.done\",\"item\":{\"id\":\"msg_cache_trace\",\"type\":\"message\",\"role\":\"assistant\",\"phase\":\"final_answer\",\"content\":[{\"type\":\"output_text\",\"text\":\"ok\"}]}}\n\n",
        "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_cache_trace\",\"usage\":{\"input_tokens\":100,\"output_tokens\":2,\"total_tokens\":102,\"input_tokens_details\":{\"cached_tokens\":40,\"cache_write_tokens\":15}}}}\n\n",
        "data: [DONE]\n\n"
    )
    .to_string();
    let (base_url, handle) = serve_sse_once(sse).await;
    let mut endpoint = ProviderEndpoint::openai(Some(base_url));
    endpoint.bearer_token = Some("test-token".to_string());
    let manager = ToolManager::new();
    let agent_tools = manager.agent_tool_set("root", GlobalToolInheritance::Isolated);
    agent_tools
        .install(pl_core::tool::ToolInstallGroup::direct(
            ToolGroupId::new("trace"),
            vec![step_tool("trace_visible", "ok")],
        ))
        .expect("install trace tool");
    let core = test_turn_engine_builder(endpoint, local_responses_model())
        .with_agent_tool_set(agent_tools)
        .build();
    let (event_tx, _) = tokio::sync::broadcast::channel(16);
    let mut recorder = TraceRecorder::new("cache-trace-session".to_string(), event_tx, 0);
    let mut session = AgentSession::new();

    let result = core
        .run_turn_with_trace(
            &mut session,
            TurnRequest::new("trace cache usage".to_string()),
            &mut recorder,
            TurnOptions::default(),
        )
        .await
        .expect("cache trace turn");
    handle.await.expect("fixture provider");

    let enabled = result
        .trace_events
        .iter()
        .find_map(|event| match &event.kind {
            TraceEventKind::EnabledToolsRecorded { event } => Some(event),
            _ => None,
        })
        .expect("enabled tools trace");
    assert_eq!(enabled.step, 0);
    assert_eq!(enabled.tools, vec!["trace_visible"]);
    assert!(!enabled.wire_fingerprint.is_empty());
    let usage = result
        .trace_events
        .iter()
        .find_map(|event| match &event.kind {
            TraceEventKind::TracePartCompleted { item } => match item.state() {
                pl_trace::TracePartState::Inference(inference) => inference.state().usage(),
                _ => None,
            },
            _ => None,
        })
        .expect("completed inference usage");
    assert_eq!(usage.cached_prompt_tokens, 40);
    assert_eq!(usage.cache_write_tokens, 15);
}

#[tokio::test]
async fn before_model_step_failure_stops_before_provider_io() {
    let mut endpoint = ProviderEndpoint::openai(Some("http://127.0.0.1:1".to_string()));
    endpoint.bearer_token = Some("test-token".to_string());
    let hook = BeforeModelStepHook::new(|_context| async {
        Err(PureError::ConfigError(
            "injected tool refresh conflict".to_string(),
        ))
    });
    let core = test_turn_engine_builder(endpoint, local_responses_model())
        .with_before_model_step(hook)
        .build();
    let (event_tx, _) = tokio::sync::broadcast::channel(8);
    let mut recorder = TraceRecorder::new("session-refresh-error".to_string(), event_tx, 0);
    let mut session = AgentSession::new();

    let error = core
        .run_turn_with_trace(
            &mut session,
            TurnRequest::new("must not reach provider".to_string()),
            &mut recorder,
            TurnOptions::default(),
        )
        .await
        .expect_err("refresh failure must abort the step");

    assert!(error.to_string().contains("injected tool refresh conflict"));
}

#[tokio::test]
async fn responses_http_uses_prompt_cache_and_full_canonical_history() {
    let first_sse = concat!(
        "data: {\"type\":\"response.output_item.added\",\"item\":{\"id\":\"msg_1\",\"type\":\"message\",\"role\":\"assistant\",\"phase\":\"final_answer\"}}\n\n",
        "data: {\"type\":\"response.output_text.delta\",\"item_id\":\"msg_1\",\"delta\":\"first ok\"}\n\n",
        "data: {\"type\":\"response.output_item.done\",\"item\":{\"id\":\"msg_1\",\"type\":\"message\",\"role\":\"assistant\",\"phase\":\"final_answer\",\"content\":[{\"type\":\"output_text\",\"text\":\"first ok\"}]}}\n\n",
        "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_1\",\"usage\":{\"input_tokens\":1,\"output_tokens\":2,\"total_tokens\":3}}}\n\n",
        "data: [DONE]\n\n"
    )
    .to_string();
    let second_sse = concat!(
        "data: {\"type\":\"response.output_item.added\",\"item\":{\"id\":\"msg_2\",\"type\":\"message\",\"role\":\"assistant\",\"phase\":\"final_answer\"}}\n\n",
        "data: {\"type\":\"response.output_text.delta\",\"item_id\":\"msg_2\",\"delta\":\"second ok\"}\n\n",
        "data: {\"type\":\"response.output_item.done\",\"item\":{\"id\":\"msg_2\",\"type\":\"message\",\"role\":\"assistant\",\"phase\":\"final_answer\",\"content\":[{\"type\":\"output_text\",\"text\":\"second ok\"}]}}\n\n",
        "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_2\",\"usage\":{\"input_tokens\":1,\"output_tokens\":2,\"total_tokens\":3}}}\n\n",
        "data: [DONE]\n\n"
    )
    .to_string();
    let (base_url, bodies, handle) = serve_sse_sequence(vec![first_sse, second_sse]).await;
    let mut endpoint = ProviderEndpoint::openai(Some(base_url));
    endpoint.bearer_token = Some("test-token".to_string());
    let core = test_turn_engine_builder(endpoint, local_responses_model()).build();
    let (event_tx, _event_rx) = tokio::sync::broadcast::channel(32);
    let mut recorder = TraceRecorder::new("session-1".to_string(), event_tx, 0);
    let mut session = AgentSession::new();
    let options = TurnOptions::default().with_prompt_cache_key("cache-session".to_string());

    core.run_turn_with_trace(
        &mut session,
        TurnRequest::new("first prompt".to_string())
            .with_budget(TurnBudget::new(std::time::Duration::from_millis(60_000))),
        &mut recorder,
        options.clone(),
    )
    .await
    .unwrap();
    core.run_turn_with_trace(
        &mut session,
        TurnRequest::new("second prompt".to_string())
            .with_budget(TurnBudget::new(std::time::Duration::from_millis(60_000))),
        &mut recorder,
        options,
    )
    .await
    .unwrap();
    handle.await.unwrap();

    let bodies = bodies.lock().unwrap();
    assert_eq!(bodies.len(), 2);
    assert_eq!(bodies[0]["store"], false);
    assert_eq!(
        bodies[0]["prompt_cache_key"],
        serde_json::json!("cache-session")
    );
    assert!(bodies[0].get("previous_response_id").is_none());
    assert_eq!(bodies[1]["store"], false);
    assert_eq!(
        bodies[1]["prompt_cache_key"],
        serde_json::json!("cache-session")
    );
    assert!(bodies[1].get("previous_response_id").is_none());
    let second_input = serde_json::to_string(&bodies[1]["input"]).unwrap();
    assert!(second_input.contains("second prompt"));
    assert!(second_input.contains("first prompt"));
    assert!(second_input.contains("first ok"));
}

#[tokio::test]
async fn run_turn_uses_runtime_profile_default_turn_options() {
    let sse_body = concat!(
        "data: {\"type\":\"response.output_item.added\",\"item\":{\"id\":\"msg_1\",\"type\":\"message\",\"role\":\"assistant\",\"phase\":\"final_answer\"}}\n\n",
        "data: {\"type\":\"response.output_text.delta\",\"item_id\":\"msg_1\",\"delta\":\"ok\"}\n\n",
        "data: {\"type\":\"response.output_item.done\",\"item\":{\"id\":\"msg_1\",\"type\":\"message\",\"role\":\"assistant\",\"phase\":\"final_answer\",\"content\":[{\"type\":\"output_text\",\"text\":\"ok\"}]}}\n\n",
        "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_1\",\"usage\":{\"input_tokens\":1,\"output_tokens\":2,\"total_tokens\":3}}}\n\n",
        "data: [DONE]\n\n"
    )
    .to_string();
    let (base_url, bodies, handle) = serve_sse_sequence(vec![sse_body]).await;
    let mut endpoint = ProviderEndpoint::openai(Some(base_url));
    endpoint.bearer_token = Some("test-token".to_string());
    let runtime = CoreRuntimeProfile::minimal()
        .with_default_turn_options(TurnOptions::default().with_prompt_cache_key("profile-cache"));
    let core = test_turn_engine_builder(endpoint, local_responses_model())
        .with_runtime_profile(runtime)
        .build();
    let mut session = AgentSession::new();

    core.run_turn(
        &mut session,
        TurnRequest::new("profile prompt".to_string())
            .with_budget(TurnBudget::new(std::time::Duration::from_millis(60_000))),
    )
    .await
    .unwrap();
    handle.await.unwrap();

    let bodies = bodies.lock().unwrap();
    assert_eq!(bodies.len(), 1);
    assert_eq!(bodies[0]["store"], false);
    assert_eq!(
        bodies[0]["prompt_cache_key"],
        serde_json::json!("profile-cache")
    );
}

#[tokio::test]
async fn run_turn_http_sends_full_history_without_a_retry_path() {
    let retry_sse = concat!(
        "data: {\"type\":\"response.output_item.added\",\"item\":{\"id\":\"msg_retry\",\"type\":\"message\",\"role\":\"assistant\",\"phase\":\"final_answer\"}}\n\n",
        "data: {\"type\":\"response.output_text.delta\",\"item_id\":\"msg_retry\",\"delta\":\"retry ok\"}\n\n",
        "data: {\"type\":\"response.output_item.done\",\"item\":{\"id\":\"msg_retry\",\"type\":\"message\",\"role\":\"assistant\",\"phase\":\"final_answer\",\"content\":[{\"type\":\"output_text\",\"text\":\"retry ok\"}]}}\n\n",
        "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_retry\",\"usage\":{\"input_tokens\":1,\"output_tokens\":2,\"total_tokens\":3}}}\n\n",
        "data: [DONE]\n\n"
    )
    .to_string();
    let (base_url, bodies, handle) =
        serve_http_sequence(vec![TestHttpResponse::sse(retry_sse)]).await;
    let mut endpoint = ProviderEndpoint::openai(Some(base_url));
    endpoint.bearer_token = Some("test-token".to_string());
    let core = test_turn_engine_builder(endpoint, local_responses_model()).build();
    let (event_tx, _event_rx) = tokio::sync::broadcast::channel(32);
    let mut recorder = TraceRecorder::new("session-1".to_string(), event_tx, 0);
    let mut session = AgentSession::new();
    session.push_user_prompt("old prompt".to_string());
    session.push_assistant_response("old answer".to_string(), None);
    session.set_prompt_cache_key("cache-session".to_string());

    let result = core
        .run_turn_with_trace(
            &mut session,
            TurnRequest::new("new prompt".to_string())
                .with_budget(TurnBudget::new(std::time::Duration::from_millis(60_000))),
            &mut recorder,
            TurnOptions::default().with_prompt_cache_key("cache-session".to_string()),
        )
        .await
        .unwrap();
    handle.await.unwrap();

    assert!(result.is_completed());
    let bodies = bodies.lock().unwrap();
    assert_eq!(bodies.len(), 1);
    assert!(bodies[0].get("previous_response_id").is_none());
    let retry_input = serde_json::to_string(&bodies[0]["input"]).unwrap();
    assert!(retry_input.contains("old prompt"));
    assert!(retry_input.contains("old answer"));
    assert!(retry_input.contains("new prompt"));
}

#[tokio::test]
async fn model_turn_helper_http_sends_full_history_once() {
    let retry_sse = concat!(
        "data: {\"type\":\"response.output_item.added\",\"item\":{\"id\":\"msg_retry\",\"type\":\"message\",\"role\":\"assistant\",\"phase\":\"final_answer\"}}\n\n",
        "data: {\"type\":\"response.output_text.delta\",\"item_id\":\"msg_retry\",\"delta\":\"retry ok\"}\n\n",
        "data: {\"type\":\"response.output_item.done\",\"item\":{\"id\":\"msg_retry\",\"type\":\"message\",\"role\":\"assistant\",\"phase\":\"final_answer\",\"content\":[{\"type\":\"output_text\",\"text\":\"retry ok\"}]}}\n\n",
        "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_retry\",\"usage\":{\"input_tokens\":4,\"output_tokens\":2,\"total_tokens\":6}}}\n\n",
        "data: [DONE]\n\n"
    )
    .to_string();
    let (base_url, bodies, handle) =
        serve_http_sequence(vec![TestHttpResponse::sse(retry_sse)]).await;
    let mut endpoint = ProviderEndpoint::openai(Some(base_url));
    endpoint.bearer_token = Some("test-token".to_string());
    let client =
        ModelTurnClient::from_route(&test_route(endpoint, local_responses_model())).unwrap();
    let mut session = AgentSession::new();
    session.push_user_prompt("old prompt".to_string());
    session.push_assistant_response("old answer".to_string(), None);
    session.set_prompt_cache_key("cache-session".to_string());

    let response = client
        .complete(
            &session,
            ModelTurnRequest::new().with_instructions("reply briefly"),
            ModelTurnOptions::default(),
        )
        .await
        .unwrap();
    handle.await.unwrap();

    assert_eq!(response.output()[0].as_message(), Some("retry ok"));
    assert_eq!(response.accounting().usage.totals().total_tokens, 6);
    let bodies = bodies.lock().unwrap();
    assert_eq!(bodies.len(), 1);
    assert!(bodies[0].get("previous_response_id").is_none());
    let retry_input = serde_json::to_string(&bodies[0]["input"]).unwrap();
    assert!(retry_input.contains("old prompt"));
    assert!(retry_input.contains("old answer"));
}

#[tokio::test]
async fn model_turn_text_helper_returns_assistant_message_text() {
    let sse_body = concat!(
        "data: {\"type\":\"response.output_item.added\",\"item\":{\"id\":\"msg_1\",\"type\":\"message\",\"role\":\"assistant\",\"phase\":\"final_answer\"}}\n\n",
        "data: {\"type\":\"response.output_text.delta\",\"item_id\":\"msg_1\",\"delta\":\"title\"}\n\n",
        "data: {\"type\":\"response.output_item.done\",\"item\":{\"id\":\"msg_1\",\"type\":\"message\",\"role\":\"assistant\",\"phase\":\"final_answer\",\"content\":[{\"type\":\"output_text\",\"text\":\"title\"}]}}\n\n",
        "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_title\",\"usage\":{\"input_tokens\":1,\"output_tokens\":1,\"total_tokens\":2}}}\n\n",
        "data: [DONE]\n\n"
    )
    .to_string();
    let (base_url, handle) = serve_sse_once(sse_body).await;
    let mut endpoint = ProviderEndpoint::openai(Some(base_url));
    endpoint.bearer_token = Some("test-token".to_string());
    let client =
        ModelTurnClient::from_route(&test_route(endpoint, local_responses_model())).unwrap();
    let mut session = AgentSession::new();
    session.push_user_prompt("summarize this".to_string());

    let text = client
        .complete_text(
            &session,
            ModelTurnRequest::new().with_instructions("title only"),
            ModelTurnOptions::default(),
        )
        .await
        .unwrap();
    handle.await.unwrap();

    assert_eq!(text, "title");
}

#[tokio::test]
async fn model_turn_client_keeps_independent_http_sessions_full_history() {
    let retry_sse = concat!(
        "data: {\"type\":\"response.output_item.added\",\"item\":{\"id\":\"msg_retry\",\"type\":\"message\",\"role\":\"assistant\",\"phase\":\"final_answer\"}}\n\n",
        "data: {\"type\":\"response.output_text.delta\",\"item_id\":\"msg_retry\",\"delta\":\"retry ok\"}\n\n",
        "data: {\"type\":\"response.output_item.done\",\"item\":{\"id\":\"msg_retry\",\"type\":\"message\",\"role\":\"assistant\",\"phase\":\"final_answer\",\"content\":[{\"type\":\"output_text\",\"text\":\"retry ok\"}]}}\n\n",
        "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_retry\",\"usage\":{\"input_tokens\":4,\"output_tokens\":2,\"total_tokens\":6}}}\n\n",
        "data: [DONE]\n\n"
    )
    .to_string();
    let second_sse = concat!(
        "data: {\"type\":\"response.output_item.added\",\"item\":{\"id\":\"msg_second\",\"type\":\"message\",\"role\":\"assistant\",\"phase\":\"final_answer\"}}\n\n",
        "data: {\"type\":\"response.output_text.delta\",\"item_id\":\"msg_second\",\"delta\":\"second ok\"}\n\n",
        "data: {\"type\":\"response.output_item.done\",\"item\":{\"id\":\"msg_second\",\"type\":\"message\",\"role\":\"assistant\",\"phase\":\"final_answer\",\"content\":[{\"type\":\"output_text\",\"text\":\"second ok\"}]}}\n\n",
        "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_second\",\"usage\":{\"input_tokens\":5,\"output_tokens\":2,\"total_tokens\":7}}}\n\n",
        "data: [DONE]\n\n"
    )
    .to_string();
    let (base_url, bodies, handle) = serve_http_sequence(vec![
        TestHttpResponse::sse(retry_sse),
        TestHttpResponse::sse(second_sse),
    ])
    .await;
    let mut endpoint = ProviderEndpoint::openai(Some(base_url));
    endpoint.bearer_token = Some("test-token".to_string());
    let client =
        ModelTurnClient::from_route(&test_route(endpoint, local_responses_model())).unwrap();

    let mut first_session = AgentSession::new();
    first_session.push_user_prompt("old prompt".to_string());
    first_session.push_assistant_response("old answer".to_string(), None);
    let first = client
        .complete(
            &first_session,
            ModelTurnRequest::new().with_instructions("reply briefly"),
            ModelTurnOptions::default(),
        )
        .await
        .unwrap();
    assert_eq!(first.output()[0].as_message(), Some("retry ok"));

    let mut second_session = AgentSession::new();
    second_session.push_user_prompt("new prompt".to_string());
    second_session.push_assistant_response("new answer".to_string(), None);
    let second = client
        .complete(
            &second_session,
            ModelTurnRequest::new().with_instructions("reply briefly"),
            ModelTurnOptions::default(),
        )
        .await
        .unwrap();
    handle.await.unwrap();

    assert_eq!(second.output()[0].as_message(), Some("second ok"));
    let bodies = bodies.lock().unwrap();
    assert_eq!(bodies.len(), 2);
    assert!(bodies[0].get("previous_response_id").is_none());
    assert!(bodies[1].get("previous_response_id").is_none());
    assert!(
        serde_json::to_string(&bodies[0]["input"])
            .unwrap()
            .contains("old answer")
    );
    assert!(
        serde_json::to_string(&bodies[1]["input"])
            .unwrap()
            .contains("new answer")
    );
}

#[tokio::test]
async fn tool_context_keeps_full_session_history_across_responses_http_requests() {
    let responses = vec![
        tool_call_sse("history-marker", "history_marker"),
        tool_call_sse("history-probe", "parent_history_probe"),
        final_sse("history-complete", "history checked"),
    ];
    let (base_url, bodies, handle) = serve_sse_sequence(responses).await;
    let mut endpoint = ProviderEndpoint::openai(Some(base_url));
    endpoint.bearer_token = Some("test-token".to_string());
    let manager = ToolManager::new();
    let agent_tools = manager.agent_tool_set("root", GlobalToolInheritance::Isolated);
    let core = test_turn_engine_builder(endpoint, local_responses_model())
        .with_agent_tool_set(agent_tools.clone())
        .build();
    install_test_tool(&agent_tools, HistoryMarkerTool);
    let session_runtime = core.tool_session_runtime();
    install_test_tool(&agent_tools, ParentHistoryProbeTool { session_runtime });
    let (event_tx, _) = tokio::sync::broadcast::channel(32);
    let mut recorder = TraceRecorder::new("session-history".to_string(), event_tx, 0);
    let mut session = AgentSession::new();

    let result = core
        .run_turn_with_trace(
            &mut session,
            TurnRequest::new("check tool history".to_string())
                .with_budget(TurnBudget::new(std::time::Duration::from_millis(60_000))),
            &mut recorder,
            TurnOptions::default(),
        )
        .await
        .unwrap();
    handle.await.unwrap();

    assert!(result.is_completed());
    let probe_result = result
        .trace_events
        .iter()
        .find_map(|event| match &event.kind {
            TraceEventKind::TracePartCompleted { item }
                if item
                    .tool()
                    .is_some_and(|tool| tool.invocation().name() == "parent_history_probe") =>
            {
                item.tool()
                    .and_then(pl_trace::TraceToolPart::terminal_output)
                    .map(|output| output.result().to_string())
            }
            TraceEventKind::TracePartStarted { .. }
            | TraceEventKind::TracePartDelta { .. }
            | TraceEventKind::TracePartCompleted { .. }
            | TraceEventKind::TracePartFailed { .. }
            | TraceEventKind::InteractionChanged { .. }
            | TraceEventKind::SkillActivated { .. }
            | TraceEventKind::EnabledToolsRecorded { .. } => None,
        })
        .expect("parent history probe result");
    assert_eq!(probe_result, "history marker visible");
    let bodies = bodies.lock().unwrap();
    assert_eq!(bodies.len(), 3);
    assert!(
        bodies
            .iter()
            .all(|body| body.get("previous_response_id").is_none())
    );
    assert!(
        serde_json::to_string(&bodies[2]["input"])
            .unwrap()
            .contains("history_marker")
    );
}

#[tokio::test]
async fn ending_tool_content_is_published_as_the_canonical_final_trace_item() {
    let (base_url, bodies, handle) = serve_sse_sequence(vec![text_then_tool_call_sse(
        "end-turn-content",
        "Submitting final completion.",
        "end_turn_content",
    )])
    .await;
    let mut endpoint = ProviderEndpoint::openai(Some(base_url));
    endpoint.bearer_token = Some("test-token".to_string());
    let manager = ToolManager::new();
    let agent_tools = manager.agent_tool_set("root", GlobalToolInheritance::Isolated);
    install_test_tool(&agent_tools, EndTurnContentTool);
    let core = test_turn_engine_builder(endpoint, local_responses_model())
        .with_agent_tool_set(agent_tools)
        .build();
    let (event_tx, _) = tokio::sync::broadcast::channel(32);
    let mut recorder = TraceRecorder::new("session-end-turn-content".to_string(), event_tx, 0);
    let mut session = AgentSession::new();

    let result = core
        .run_turn_with_trace(
            &mut session,
            TurnRequest::new("finish with a visible marker".to_string())
                .with_budget(TurnBudget::new(std::time::Duration::from_millis(60_000))),
            &mut recorder,
            TurnOptions::default(),
        )
        .await
        .unwrap();
    handle.await.unwrap();

    assert!(result.is_completed());
    assert_eq!(result.content, "TASK_E2E_DONE");
    assert_eq!(bodies.lock().unwrap().len(), 1);
    assert!(session.messages().iter().any(|message| {
        message.role == MessageRole::Assistant
            && message.content == MessageContent::text("TASK_E2E_DONE".to_string())
    }));
    assert!(result.trace_events.iter().any(|event| match &event.kind {
        TraceEventKind::TracePartCompleted { item } => item.text().is_some_and(|text| {
            text.channel() == TraceTextChannel::Final && text.content() == "TASK_E2E_DONE"
        }),
        TraceEventKind::TracePartStarted { .. }
        | TraceEventKind::TracePartDelta { .. }
        | TraceEventKind::TracePartFailed { .. }
        | TraceEventKind::InteractionChanged { .. }
        | TraceEventKind::SkillActivated { .. }
        | TraceEventKind::EnabledToolsRecorded { .. } => false,
    }));
    assert!(result.trace_events.iter().any(|event| match &event.kind {
        TraceEventKind::TracePartCompleted { item } => item.text().is_some_and(|text| {
            text.channel() == TraceTextChannel::Final
                && text.content() == "Submitting final completion."
        }),
        TraceEventKind::TracePartStarted { .. }
        | TraceEventKind::TracePartDelta { .. }
        | TraceEventKind::TracePartFailed { .. }
        | TraceEventKind::InteractionChanged { .. }
        | TraceEventKind::SkillActivated { .. }
        | TraceEventKind::EnabledToolsRecorded { .. } => false,
    }));
}

#[tokio::test]
async fn large_tool_artifact_does_not_break_tool_history_or_evidence() {
    let responses = vec![
        tool_call_sse("large-artifact", "large_artifact"),
        final_sse("large-artifact-complete", "artifact checked"),
    ];
    let (base_url, bodies, handle) = serve_sse_sequence(responses).await;
    let mut endpoint = ProviderEndpoint::openai(Some(base_url));
    endpoint.bearer_token = Some("test-token".to_string());
    let manager = ToolManager::new();
    let agent_tools = manager.agent_tool_set("root", GlobalToolInheritance::Isolated);
    install_test_tool(&agent_tools, LargeArtifactTool);
    let core = test_turn_engine_builder(endpoint, local_responses_model())
        .with_agent_tool_set(agent_tools)
        .build();
    let (event_tx, _) = tokio::sync::broadcast::channel(32);
    let mut recorder = TraceRecorder::new("session-large-artifact".to_string(), event_tx, 0);
    let mut session = AgentSession::new();

    let result = core
        .run_turn_with_trace(
            &mut session,
            TurnRequest::new("check a large artifact".to_string())
                .with_budget(TurnBudget::new(std::time::Duration::from_millis(60_000))),
            &mut recorder,
            TurnOptions::default(),
        )
        .await
        .unwrap();
    handle.await.unwrap();

    assert!(result.is_completed());
    assert_eq!(result.content, "artifact checked");
    assert_eq!(bodies.lock().unwrap().len(), 2);
    let receipt = session
        .items()
        .iter()
        .find_map(|item| match item {
            pl_protocol::ModelContextItem::ToolResult { receipt, .. } => Some(receipt),
            pl_protocol::ModelContextItem::Message { .. }
            | pl_protocol::ModelContextItem::ToolMedia { .. }
            | pl_protocol::ModelContextItem::Compaction { .. }
            | pl_protocol::ModelContextItem::Responses { .. } => None,
        })
        .expect("tool receipt");
    assert_eq!(receipt.artifacts[0]["kind"], "largeArtifact");
    assert_eq!(receipt.artifacts[0].get("payload"), None);
}

async fn capture_default_tools_request() -> serde_json::Value {
    let (base_url, bodies, handle) =
        serve_sse_sequence(vec![final_sse("hosted-tools", "ok")]).await;
    let mut endpoint = ProviderEndpoint::openai(Some(base_url));
    endpoint.bearer_token = Some("test-token".to_string());
    let mut model = pl_model::model::default_models()
        .into_iter()
        .find(|model| model.slug == "gpt-5.6-sol")
        .unwrap();
    model.binding.transport.default_connection_mode =
        pl_model::provider::ProviderConnectionMode::Http;
    let manager = ToolManager::new();
    let agent_tools = manager.agent_tool_set("root", GlobalToolInheritance::Isolated);
    install_test_tool(&agent_tools, HostedToolProbe);
    let mut core = test_turn_engine_builder(endpoint, model)
        .with_agent_tool_set(agent_tools)
        .build();
    core.install_default_tools(std::env::temp_dir(), Some("rules".to_string()))
        .await
        .expect("install default tools");
    let (event_tx, _) = tokio::sync::broadcast::channel(32);
    let mut recorder = TraceRecorder::new("session-hosted-tools".to_string(), event_tx, 0);
    let mut session = AgentSession::new();

    let result = core
        .run_turn_with_trace(
            &mut session,
            TurnRequest::new("check hosted tools".to_string())
                .with_budget(TurnBudget::new(std::time::Duration::from_millis(60_000))),
            &mut recorder,
            TurnOptions::default(),
        )
        .await
        .unwrap();
    handle.await.unwrap();

    assert!(result.is_completed());
    bodies.lock().unwrap()[0].clone()
}

fn local_responses_model() -> pl_model::model::ModelInfo {
    let mut model = pl_model::model::ModelInfo::compatible("local-responses");
    model
        .binding
        .set_transport(pl_model::model::ModelTransportProfile::responses_http());
    model
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
struct StepToolInput {}

fn step_tool(name: &'static str, output: &'static str) -> pl_core::tool::DynTool {
    pl_core::tool::static_tool::<StepToolInput>(pl_core::tool::StaticToolDefinition::new(
        pl_core::tool::ToolName::bare(name).unwrap(),
        name,
    ))
    .policy(pl_core::tool::ToolPolicy::read_only())
    .build(move |_input, _context| async move { Ok(ToolResult::success(output)) })
}

fn request_tool_names(body: &serde_json::Value) -> Vec<&str> {
    body["tools"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|tool| tool.get("name").and_then(serde_json::Value::as_str))
        .collect()
}

#[derive(Debug)]
struct HistoryMarkerTool;

#[derive(Debug)]
struct HostedToolProbe;

#[derive(Debug)]
struct LargeArtifactTool;

#[derive(Debug)]
struct EndTurnContentTool;

impl StaticTool for HostedToolProbe {
    type Input = serde_json::Value;

    fn definition(&self) -> pl_core::tool::StaticToolDefinition {
        test_static_tool_definition(
            "git_status",
            "Provides a read-only hosted tool orchestration probe",
        )
    }

    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {},
            "additionalProperties": false
        })
    }

    fn policy(&self) -> ToolPolicy {
        ToolPolicy::read_only()
    }

    async fn execute(
        &self,
        _input: Self::Input,
        _context: ToolCallContext,
    ) -> pl_core::Result<ToolResult> {
        Ok(ToolResult::success("clean"))
    }
}

impl StaticTool for LargeArtifactTool {
    type Input = serde_json::Value;

    fn definition(&self) -> pl_core::tool::StaticToolDefinition {
        test_static_tool_definition("large_artifact", "Returns a large artifact payload")
    }

    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {},
            "additionalProperties": false
        })
    }

    fn policy(&self) -> ToolPolicy {
        ToolPolicy::default()
    }

    async fn execute(
        &self,
        _input: Self::Input,
        _context: ToolCallContext,
    ) -> pl_core::Result<ToolResult> {
        let mut result = ToolResult::success("large artifact ready");
        result
            .runtime_events
            .push(pl_core::tool::ToolDirective::OutputArtifacts {
                artifacts: vec![serde_json::json!({
                        "kind": "largeArtifact",
                        "payload": "x".repeat(64 * 1024),
                })],
            });
        Ok(result)
    }
}

impl StaticTool for EndTurnContentTool {
    type Input = serde_json::Value;

    fn definition(&self) -> pl_core::tool::StaticToolDefinition {
        test_static_tool_definition(
            "end_turn_content",
            "Ends the turn with canonical final assistant content",
        )
    }

    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {},
            "additionalProperties": false
        })
    }

    fn policy(&self) -> ToolPolicy {
        ToolPolicy::default()
    }

    async fn execute(
        &self,
        _input: Self::Input,
        _context: ToolCallContext,
    ) -> pl_core::Result<ToolResult> {
        Ok(pl_core::tool::ToolResult::success("completed")
            .ending_turn_with_content("TASK_E2E_DONE"))
    }
}

impl StaticTool for HistoryMarkerTool {
    type Input = serde_json::Value;

    fn definition(&self) -> pl_core::tool::StaticToolDefinition {
        test_static_tool_definition("history_marker", "Records a marker in tool history")
    }

    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {},
            "additionalProperties": false
        })
    }

    fn policy(&self) -> ToolPolicy {
        ToolPolicy::default()
    }

    async fn execute(
        &self,
        _input: Self::Input,
        _context: ToolCallContext,
    ) -> pl_core::Result<ToolResult> {
        Ok(ToolResult::success("history marker"))
    }
}

#[derive(Debug)]
struct ParentHistoryProbeTool {
    session_runtime: pl_core::tool::ToolSessionRuntime,
}

impl StaticTool for ParentHistoryProbeTool {
    type Input = serde_json::Value;

    fn definition(&self) -> pl_core::tool::StaticToolDefinition {
        test_static_tool_definition(
            "parent_history_probe",
            "Reports whether prior tool history is visible",
        )
    }

    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {},
            "additionalProperties": false
        })
    }

    fn policy(&self) -> ToolPolicy {
        ToolPolicy::default()
    }

    async fn execute(
        &self,
        _input: Self::Input,
        _context: ToolCallContext,
    ) -> pl_core::Result<ToolResult> {
        let parent_session = self.session_runtime.parent_session();
        let marker_visible = parent_session.messages().iter().any(|message| {
            message
                .tool_result
                .as_ref()
                .is_some_and(|record| record.name == "history_marker")
        });
        Ok(ToolResult::success(if marker_visible {
            "history marker visible"
        } else {
            "history marker missing"
        }))
    }
}

fn tool_call_sse(id: &str, name: &str) -> String {
    let item_id = format!("fc_{id}");
    let call_id = format!("call_{id}");
    [
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
            "type": "response.output_item.done",
            "item": {
                "type": "function_call",
                "id": item_id,
                "call_id": call_id,
                "name": name,
                "arguments": "{}"
            }
        }),
        serde_json::json!({
            "type": "response.completed",
            "response": {
                "id": format!("response_{id}"),
                "usage": {"input_tokens": 1, "output_tokens": 1, "total_tokens": 2}
            }
        }),
    ]
    .into_iter()
    .map(|event| format!("data: {event}\n\n"))
    .chain(std::iter::once("data: [DONE]\n\n".to_string()))
    .collect()
}

fn text_then_tool_call_sse(id: &str, content: &str, name: &str) -> String {
    let message_id = format!("msg_{id}");
    let item_id = format!("fc_{id}");
    let call_id = format!("call_{id}");
    [
        serde_json::json!({
            "type": "response.output_item.added",
            "item": {
                "id": message_id,
                "type": "message",
                "role": "assistant",
                "phase": "final_answer"
            }
        }),
        serde_json::json!({
            "type": "response.output_text.delta",
            "item_id": message_id,
            "delta": content
        }),
        serde_json::json!({
            "type": "response.output_item.done",
            "item": {
                "id": message_id,
                "type": "message",
                "role": "assistant",
                "phase": "final_answer",
                "content": [{"type": "output_text", "text": content}]
            }
        }),
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
            "type": "response.output_item.done",
            "item": {
                "type": "function_call",
                "id": item_id,
                "call_id": call_id,
                "name": name,
                "arguments": "{}"
            }
        }),
        serde_json::json!({
            "type": "response.completed",
            "response": {
                "id": format!("response_{id}"),
                "usage": {"input_tokens": 1, "output_tokens": 1, "total_tokens": 2}
            }
        }),
    ]
    .into_iter()
    .map(|event| format!("data: {event}\n\n"))
    .chain(std::iter::once("data: [DONE]\n\n".to_string()))
    .collect()
}

fn final_sse(id: &str, content: &str) -> String {
    let item_id = format!("msg_{id}");
    [
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
    ]
    .into_iter()
    .map(|event| format!("data: {event}\n\n"))
    .chain(std::iter::once("data: [DONE]\n\n".to_string()))
    .collect()
}

fn test_route(endpoint: ProviderEndpoint, model: ModelInfo) -> pl_core::ResolvedModelRoute {
    route("test", endpoint, model, None)
}

fn test_turn_engine_builder(endpoint: ProviderEndpoint, model: ModelInfo) -> TurnEngineBuilder {
    TurnEngineBuilder::from_route(&test_route(endpoint, model)).unwrap()
}

fn test_static_tool_definition(
    name: &'static str,
    description: &'static str,
) -> StaticToolDefinition {
    StaticToolDefinition::new(
        ToolName::bare(name).expect("builtin tool name"),
        description,
    )
}

fn install_test_tool(agent_tools: &AgentToolSet, tool: impl Into<DynTool>) {
    let tool = tool.into();
    let name = tool.definition().name().wire_name().to_string();
    agent_tools
        .install(ToolInstallGroup::direct(
            ToolGroupId::new(format!("test:{name}")),
            vec![tool],
        ))
        .expect("install test tool");
}

fn trace_started_kinds(events: &[TraceEvent]) -> Vec<TracePartKind> {
    events
        .iter()
        .filter_map(|event| match &event.kind {
            TraceEventKind::TracePartStarted { item } => Some(item.kind()),
            TraceEventKind::TracePartDelta { .. }
            | TraceEventKind::TracePartCompleted { .. }
            | TraceEventKind::TracePartFailed { .. }
            | TraceEventKind::InteractionChanged { .. }
            | TraceEventKind::SkillActivated { .. }
            | TraceEventKind::EnabledToolsRecorded { .. } => None,
        })
        .collect()
}

fn runtime_progress_texts(
    event_rx: &mut tokio::sync::broadcast::Receiver<pl_trace::AgentEvent>,
) -> Vec<String> {
    let mut progress_texts = Vec::new();
    while let Ok(event) = event_rx.try_recv() {
        match event {
            pl_trace::AgentEvent::TracePartCompleted { item }
                if item.source() == TracePartSource::Runtime
                    && item
                        .text()
                        .is_some_and(|text| text.channel() == TraceTextChannel::Commentary) =>
            {
                progress_texts.push(
                    item.text()
                        .expect("runtime commentary text")
                        .content()
                        .to_string(),
                );
            }
            _ => {}
        }
    }
    progress_texts
}

#[derive(Debug)]
struct TariffClock;
impl pl_model::runtime::InferenceClock for TariffClock {
    fn unix_seconds(&self) -> pl_protocol::Result<i64> {
        Ok(1_788_483_600)
    }
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
struct DoubleInput {
    value: i64,
}

#[tokio::test]
async fn provider_tool_tasks_preserve_native_optimizations_and_account_for_the_complete_turn() {
    use pl_model::provider::{ProviderAdapterKind, ProviderConnectionMode, ProviderWireProtocol};
    let cases = [
        (
            "gpt-6-astra",
            ProviderAdapterKind::OpenAi,
            Some("low"),
            Some(0.0328),
        ),
        (
            "deepseek-v4-flash",
            ProviderAdapterKind::DeepSeek,
            Some("high"),
            Some(0.00728),
        ),
        (
            "glm-5.3",
            ProviderAdapterKind::Zhipu,
            Some("low"),
            Some(0.0224),
        ),
        (
            "mimo-v2.5-pro",
            ProviderAdapterKind::MiMo,
            Some("enabled"),
            Some(0.00602),
        ),
        (
            "custom-model",
            ProviderAdapterKind::OpenAiCompatible,
            None,
            None,
        ),
    ];
    for (slug, adapter, effort, expected_cost) in cases {
        let mut model = pl_model::model::default_models()
            .into_iter()
            .find(|model| model.slug == slug)
            .unwrap_or_else(|| ModelInfo::compatible(slug));
        model.binding.transport.default_connection_mode = ProviderConnectionMode::Http;
        let protocol = model.binding.transport.protocol;
        let responses = [false, true].into_iter().map(|final_answer| {
            let usage = match protocol {
                ProviderWireProtocol::Responses => serde_json::json!({"input_tokens":1000,"output_tokens":200,"total_tokens":1200,"input_tokens_details":{"cached_tokens":400,"cache_write_tokens":0}}),
                ProviderWireProtocol::ChatCompletions => serde_json::json!({"prompt_tokens":1000,"completion_tokens":200,"total_tokens":1200,"prompt_tokens_details":{"cached_tokens":400}}),
            };
            let mut events = match (protocol, final_answer) {
                (ProviderWireProtocol::Responses, false) => vec![
                    serde_json::json!({"type":"response.output_item.added","item":{"id":"tool-item","type":"function_call","call_id":"native-call","name":"double"}}),
                    serde_json::json!({"type":"response.function_call_arguments.delta","item_id":"tool-item","delta":"{\"value\":2}"}),
                    serde_json::json!({"type":"response.output_item.done","item":{"id":"tool-item","type":"function_call","call_id":"native-call","name":"double","arguments":"{\"value\":2}"}}),
                ],
                (ProviderWireProtocol::Responses, true) => vec![serde_json::json!({"type":"response.output_text.delta","item_id":"answer","delta":"4"})],
                (ProviderWireProtocol::ChatCompletions, false) => vec![
                    serde_json::json!({"choices":[{"delta":{"reasoning_content":"calculate double","tool_calls":[{"index":0,"id":"native-call","type":"function","function":{"name":"double","arguments":"{\"value\":2}"}}]},"finish_reason":null}]}),
                    serde_json::json!({"choices":[{"delta":{},"finish_reason":"tool_calls"}]}),
                ],
                (ProviderWireProtocol::ChatCompletions, true) => vec![serde_json::json!({"choices":[{"delta":{"content":"4"},"finish_reason":"stop"}]})],
            };
            events.push(match protocol {
                ProviderWireProtocol::Responses => serde_json::json!({"type":"response.completed","response":{"id":if final_answer {"final"} else {"tool"},"usage":usage}}),
                ProviderWireProtocol::ChatCompletions => serde_json::json!({"choices":[],"usage":usage}),
            });
            events.into_iter().map(|event| format!("data: {event}\n\n")).collect::<String>() + "data: [DONE]\n\n"
        }).collect();
        let (url, server) = support::serve_checked_sse_sequence(responses, move |step, body| {
            if adapter == ProviderAdapterKind::Zhipu
                && (body["tool_stream"] != true || body["thinking"]["clear_thinking"] != false)
            {
                return false;
            }
            if adapter == ProviderAdapterKind::MiMo && body["thinking"]["type"] != "enabled" {
                return false;
            }
            if step == 0 {
                return body["tools"].as_array().is_some_and(|tools| {
                    tools.iter().any(|tool| {
                        tool["name"] == "double" || tool["function"]["name"] == "double"
                    })
                });
            }
            match protocol {
                ProviderWireProtocol::Responses => body["input"].as_array().is_some_and(|items| {
                    items.iter().any(|item| {
                        item["type"] == "function_call_output"
                            && item["call_id"] == "native-call"
                            && item["output"] == "4"
                    })
                }),
                ProviderWireProtocol::ChatCompletions => {
                    body["messages"].as_array().is_some_and(|messages| {
                        messages.iter().any(|message| {
                            message["role"] == "tool"
                                && message["tool_call_id"] == "native-call"
                                && message["content"] == "4"
                        }) && messages.iter().any(|message| {
                            message["role"] == "assistant"
                                && message["reasoning_content"] == "calculate double"
                        })
                    })
                }
            }
        })
        .await;
        let endpoint = ProviderEndpoint::compatible("task fixture", url).with_adapter(adapter);
        let mut route = support::route("fixture", endpoint, model, effort);
        if expected_cost.is_none() {
            route.pricing_mode = pl_protocol::PricingMode::Disabled;
        }
        let manager = ToolManager::new();
        let tools = manager.agent_tool_set("accounted-task", GlobalToolInheritance::Isolated);
        let tool = pl_core::tool::static_tool::<DoubleInput>(StaticToolDefinition::new(
            ToolName::bare("double").unwrap(),
            "Double an integer",
        ))
        .policy(ToolPolicy::read_only())
        .build(|input, _| async move { Ok(ToolResult::success((input.value * 2).to_string())) });
        install_test_tool(&tools, tool);
        let engine = TurnEngineBuilder::from_route(&route)
            .unwrap()
            .with_clock(std::sync::Arc::new(TariffClock))
            .with_agent_tool_set(tools)
            .build();
        let (tx, mut events) = tokio::sync::broadcast::channel(512);
        let mut recorder = TraceRecorder::new(format!("task-{slug}"), tx, 0);
        let mut session = AgentSession::new();
        let result = engine
            .run_turn_with_trace(
                &mut session,
                TurnRequest::new("Double 2 with the tool"),
                &mut recorder,
                TurnOptions::default(),
            )
            .await
            .expect(slug);
        server.await.expect("strict fixture server");
        assert!(result.is_completed(), "{slug}: {:?}", result.outcome);
        assert_eq!(result.content, "4", "{slug}");
        assert_eq!(
            result.usage.total_tokens, 2400,
            "final usage includes the complete tool roundtrip"
        );
        let mut costs = Vec::new();
        while let Ok(event) = events.try_recv() {
            if let pl_trace::AgentEvent::AgentRuntimeUpdated { delta } = event {
                assert!(!delta.has_unpriced_usage, "{slug}");
                pl_core::runtime_usage::merge_costs(&mut costs, &delta.estimated_costs);
            }
        }
        match expected_cost {
            Some(expected) => assert!(
                (costs.first().expect("final cost").amount - expected).abs() < 1e-12,
                "{slug}: {costs:?}"
            ),
            None => assert!(
                costs.is_empty(),
                "disabled monetary accounting still completed the tool task"
            ),
        }
    }
}

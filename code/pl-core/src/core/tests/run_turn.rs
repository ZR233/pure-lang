use super::*;
use pretty_assertions::assert_eq;

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
    let mut provider = ProviderInfo::openai(Some(base_url));
    provider.connection_mode = pl_model::ProviderConnectionMode::Http;
    provider.bearer_token = Some("test-token".to_string());
    provider.default_model = "local-responses".to_string();
    let core = TurnEngine::from_provider_info(provider).unwrap();
    let (event_tx, _event_rx) = tokio::sync::broadcast::channel(32);
    let mut recorder = TraceRecorder::new("session-1".to_string(), event_tx, 0);
    let mut session = AgentSession::new();

    let result = core
        .run_turn_with_trace(
            &mut session,
            TurnRequest::new("Build the thing".to_string())
                .with_budget(crate::turn::TurnBudget::new(60_000)),
            &mut recorder,
            TurnOptions::default(),
        )
        .await
        .unwrap();
    handle.await.unwrap();

    assert_eq!(result.status, TurnResultStatus::Completed);
    let events = &result.trace_events;
    let started_kinds = trace_started_kinds(events);
    assert_eq!(started_kinds[0], TracePartKind::Text);
    assert_eq!(started_kinds[1], TracePartKind::Turn);
    assert_eq!(started_kinds[2], TracePartKind::Inference);

    let user_item = events
        .iter()
        .find_map(|event| match &event.kind {
            TraceEventKind::TracePartStarted { item }
                if item.kind == TracePartKind::Text
                    && item.text_channel == Some(TraceTextChannel::User) =>
            {
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
        .expect("user trace part");
    assert_eq!(user_item.started_sequence, 0);
    assert_eq!(user_item.content, "Build the thing");
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
    let mut provider = ProviderInfo::openai(Some(base_url));
    provider.connection_mode = pl_model::ProviderConnectionMode::Http;
    provider.bearer_token = Some("test-token".to_string());
    provider.default_model = "local-responses".to_string();
    let core = TurnEngine::from_provider_info(provider).unwrap();
    let (event_tx, mut event_rx) = tokio::sync::broadcast::channel(64);
    let mut recorder = TraceRecorder::new("session-1".to_string(), event_tx, 0);
    let mut session = AgentSession::new();

    let result = core
        .run_turn_with_trace(
            &mut session,
            TurnRequest::new("Build the thing".to_string())
                .with_budget(crate::turn::TurnBudget::new(60_000)),
            &mut recorder,
            TurnOptions::default(),
        )
        .await
        .unwrap();
    handle.await.unwrap();

    assert_eq!(result.status, TurnResultStatus::Completed);
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
    let mut provider = ProviderInfo::openai(Some(base_url));
    provider.connection_mode = pl_model::ProviderConnectionMode::Http;
    provider.bearer_token = Some("test-token".to_string());
    provider.default_model = "local-responses".to_string();
    let core = TurnEngine::from_provider_info(provider).unwrap();
    let (event_tx, _event_rx) = tokio::sync::broadcast::channel(32);
    let mut recorder = TraceRecorder::new("session-1".to_string(), event_tx, 0);
    let mut session = AgentSession::new();

    let result = core
        .run_turn_with_trace(
            &mut session,
            TurnRequest::new("Build the thing".to_string())
                .with_budget(crate::turn::TurnBudget::new(60_000)),
            &mut recorder,
            TurnOptions::default(),
        )
        .await
        .unwrap();
    handle.await.unwrap();

    assert_eq!(result.status, TurnResultStatus::Completed);
    assert_eq!(result.content, "Done");
    assert_eq!(session.messages().len(), 2);
    assert_eq!(session.messages()[1].role, MessageRole::Assistant);
    assert_eq!(
        session.messages()[1].content,
        MessageContent::Text("Done".to_string())
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
    let mut provider = ProviderInfo::openai(Some(base_url));
    provider.connection_mode = pl_model::ProviderConnectionMode::Http;
    provider.bearer_token = Some("test-token".to_string());
    provider.default_model = "local-responses".to_string();
    let mut model = pl_model::ModelInfo::fallback("local-responses");
    model.auto_compact_token_limit = Some(1);
    let core = TurnEngineBuilder::from_provider_info_with_models(provider, vec![model])
        .unwrap()
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
                .with_budget(crate::turn::TurnBudget::new(60_000)),
            &mut recorder,
            TurnOptions::default(),
        )
        .await
        .unwrap();
    handle.await.unwrap();

    assert_eq!(result.status, TurnResultStatus::Completed);
    assert_eq!(result.context_compactions.len(), 1);
    let compaction = &result.context_compactions[0];
    assert_eq!(compaction.summary.as_deref(), Some("compressed memory"));
    assert_eq!(compaction.trigger.as_str(), "estimatedTokens");
    assert_eq!(compaction.provider_prompt_tokens, None);
    assert_eq!(compaction.auto_compact_limit, 1);
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
    let mut provider = ProviderInfo::openai(Some(base_url));
    provider.connection_mode = pl_model::ProviderConnectionMode::Http;
    provider.bearer_token = Some("test-token".to_string());
    provider.default_model = "local-responses".to_string();
    let core = TurnEngineBuilder::from_provider_info_with_models(
        provider,
        vec![pl_model::ModelInfo::fallback("local-responses")],
    )
    .unwrap()
    .with_runtime_profile(CoreRuntimeProfile::minimal().with_context_compaction(
        ContextCompactionConfig::default().with_openai_mode(OpenAiCompactionMode::Local),
    ))
    .build();
    let mut session = AgentSession::from_messages(vec![Message {
        role: MessageRole::User,
        content: MessageContent::Text("only message".to_string()),
        reasoning_content: None,
        metadata: HashMap::new(),
    }]);
    let original_revision = session.revision();
    let (event_tx, _) = tokio::sync::broadcast::channel(16);

    let snapshot = core
        .compact_session(
            &mut session,
            ManualContextCompactionRequest::new(),
            event_tx,
        )
        .await
        .unwrap()
        .unwrap();
    handle.await.unwrap();

    assert_eq!(snapshot.trigger, ContextCompactionTrigger::Manual);
    assert_eq!(snapshot.phase, ContextCompactionPhase::Standalone);
    assert_eq!(snapshot.summary.as_deref(), Some("manual summary"));
    assert_eq!(session.revision(), original_revision + 1);
    assert!(session.messages().last().is_some_and(|message| {
        message
            .metadata
            .contains_key(crate::context_compaction::SUMMARY_METADATA_KEY)
    }));
}

#[tokio::test]
async fn enabled_tools_snapshot_remains_internal_trace_event() {
    let mut core = TurnEngine::default_provider().unwrap();
    core.register_default_tools(std::env::temp_dir(), Some("rules".to_string()))
        .await;
    let events = record_enabled_tools_for_core(&core, "session-1", "turn-1");
    let event = enabled_tools_event(&events);

    assert_eq!(event.turn_id, "turn-1");
    assert!(event.tools.contains(&"read_file".to_string()));
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
    let mut provider = ProviderInfo::openai(Some(base_url));
    provider.connection_mode = pl_model::ProviderConnectionMode::Http;
    provider.bearer_token = Some("test-token".to_string());
    provider.default_model = "local-responses".to_string();
    let core = TurnEngine::from_provider_info(provider).unwrap();
    let (event_tx, _event_rx) = tokio::sync::broadcast::channel(32);
    let mut recorder = TraceRecorder::new("session-1".to_string(), event_tx, 0);
    let mut session = AgentSession::new();
    let options = TurnOptions::default().with_prompt_cache_key("cache-session".to_string());

    core.run_turn_with_trace(
        &mut session,
        TurnRequest::new("first prompt".to_string())
            .with_budget(crate::turn::TurnBudget::new(60_000)),
        &mut recorder,
        options.clone(),
    )
    .await
    .unwrap();
    core.run_turn_with_trace(
        &mut session,
        TurnRequest::new("second prompt".to_string())
            .with_budget(crate::turn::TurnBudget::new(60_000)),
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
    let mut provider = ProviderInfo::openai(Some(base_url));
    provider.connection_mode = pl_model::ProviderConnectionMode::Http;
    provider.bearer_token = Some("test-token".to_string());
    provider.default_model = "local-responses".to_string();
    let runtime = CoreRuntimeProfile::minimal().with_runtime_options(
        CoreRuntimeOptions::default()
            .with_turn_options(TurnOptions::default().with_prompt_cache_key("profile-cache")),
    );
    let core = TurnEngineBuilder::from_provider_info(provider)
        .unwrap()
        .with_runtime_profile(runtime)
        .build();
    let (event_tx, _event_rx) = tokio::sync::broadcast::channel(32);
    let mut session = AgentSession::new();

    core.run_turn(
        &mut session,
        TurnRequest::new("profile prompt".to_string())
            .with_budget(crate::turn::TurnBudget::new(60_000)),
        event_tx,
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
    let mut provider = ProviderInfo::openai(Some(base_url));
    provider.connection_mode = pl_model::ProviderConnectionMode::Http;
    provider.bearer_token = Some("test-token".to_string());
    provider.default_model = "local-responses".to_string();
    let core = TurnEngine::from_provider_info(provider).unwrap();
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
                .with_budget(crate::turn::TurnBudget::new(60_000)),
            &mut recorder,
            TurnOptions::default().with_prompt_cache_key("cache-session".to_string()),
        )
        .await
        .unwrap();
    handle.await.unwrap();

    assert_eq!(result.status, TurnResultStatus::Completed);
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
    let mut provider_info = ProviderInfo::openai(Some(base_url));
    provider_info.connection_mode = pl_model::ProviderConnectionMode::Http;
    provider_info.bearer_token = Some("test-token".to_string());
    provider_info.default_model = "local-responses".to_string();
    let provider = pl_model::create_provider(provider_info).unwrap();
    let mut session = AgentSession::new();
    session.push_user_prompt("old prompt".to_string());
    session.push_assistant_response("old answer".to_string(), None);
    session.set_prompt_cache_key("cache-session".to_string());

    let response = stream_session_completion_response(
        provider,
        &mut session,
        CoreModelTurnRequest::new("local-responses").with_instructions("reply briefly"),
        CoreModelTurnOptions::default(),
    )
    .await
    .unwrap();
    handle.await.unwrap();

    assert_eq!(response.content.as_deref(), Some("retry ok"));
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
    let mut provider_info = ProviderInfo::openai(Some(base_url));
    provider_info.connection_mode = pl_model::ProviderConnectionMode::Http;
    provider_info.bearer_token = Some("test-token".to_string());
    provider_info.default_model = "local-responses".to_string();
    let provider = pl_model::create_provider(provider_info).unwrap();
    let mut session = AgentSession::new();
    session.push_user_prompt("summarize this".to_string());

    let text = stream_session_completion_message_text(
        provider,
        &mut session,
        CoreModelTurnRequest::new("local-responses").with_instructions("title only"),
        CoreModelTurnOptions::default(),
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
    let mut provider_info = ProviderInfo::openai(Some(base_url));
    provider_info.connection_mode = pl_model::ProviderConnectionMode::Http;
    provider_info.bearer_token = Some("test-token".to_string());
    provider_info.default_model = "local-responses".to_string();
    let provider = pl_model::create_provider(provider_info).unwrap();
    let client = CoreModelTurnClient::new();

    let mut first_session = AgentSession::new();
    first_session.push_user_prompt("old prompt".to_string());
    first_session.push_assistant_response("old answer".to_string(), None);
    let first = client
        .stream_session_completion_response(
            provider.clone(),
            &mut first_session,
            CoreModelTurnRequest::new("local-responses").with_instructions("reply briefly"),
            CoreModelTurnOptions::default(),
        )
        .await
        .unwrap();
    assert_eq!(first.content.as_deref(), Some("retry ok"));

    let mut second_session = AgentSession::new();
    second_session.push_user_prompt("new prompt".to_string());
    second_session.push_assistant_response("new answer".to_string(), None);
    let second = client
        .stream_session_completion_response(
            provider,
            &mut second_session,
            CoreModelTurnRequest::new("local-responses").with_instructions("reply briefly"),
            CoreModelTurnOptions::default(),
        )
        .await
        .unwrap();
    handle.await.unwrap();

    assert_eq!(second.content.as_deref(), Some("second ok"));
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
    let mut provider = ProviderInfo::openai(Some(base_url));
    provider.connection_mode = pl_model::ProviderConnectionMode::Http;
    provider.bearer_token = Some("test-token".to_string());
    provider.default_model = "local-responses".to_string();
    let mut core = TurnEngine::from_provider_info(provider).unwrap();
    core.register_tool(HistoryMarkerTool);
    core.register_tool(ParentHistoryProbeTool);
    let (event_tx, _) = tokio::sync::broadcast::channel(32);
    let mut recorder = TraceRecorder::new("session-history".to_string(), event_tx, 0);
    let mut session = AgentSession::new();

    let result = core
        .run_turn_with_trace(
            &mut session,
            TurnRequest::new("check tool history".to_string())
                .with_budget(crate::turn::TurnBudget::new(60_000)),
            &mut recorder,
            TurnOptions::default(),
        )
        .await
        .unwrap();
    handle.await.unwrap();

    assert_eq!(result.status, TurnResultStatus::Completed);
    let probe_result = result
        .trace_events
        .iter()
        .find_map(|event| match &event.kind {
            TraceEventKind::TracePartCompleted { item }
                if item
                    .tool
                    .as_ref()
                    .is_some_and(|tool| tool.name == "parent_history_probe") =>
            {
                item.tool.as_ref().and_then(|tool| tool.result.as_deref())
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
async fn large_tool_artifact_does_not_break_tool_history_or_evidence() {
    let responses = vec![
        tool_call_sse("large-artifact", "large_artifact"),
        final_sse("large-artifact-complete", "artifact checked"),
    ];
    let (base_url, bodies, handle) = serve_sse_sequence(responses).await;
    let mut provider = ProviderInfo::openai(Some(base_url));
    provider.connection_mode = pl_model::ProviderConnectionMode::Http;
    provider.bearer_token = Some("test-token".to_string());
    provider.default_model = "local-responses".to_string();
    let mut core = TurnEngine::from_provider_info(provider).unwrap();
    core.register_tool(LargeArtifactTool);
    let (event_tx, _) = tokio::sync::broadcast::channel(32);
    let mut recorder = TraceRecorder::new("session-large-artifact".to_string(), event_tx, 0);
    let mut session = AgentSession::new();

    let result = core
        .run_turn_with_trace(
            &mut session,
            TurnRequest::new("check a large artifact".to_string())
                .with_budget(crate::turn::TurnBudget::new(60_000)),
            &mut recorder,
            TurnOptions::default(),
        )
        .await
        .unwrap();
    handle.await.unwrap();

    assert_eq!(result.status, TurnResultStatus::Completed);
    assert_eq!(result.content, "artifact checked");
    assert_eq!(bodies.lock().unwrap().len(), 2);
    let receipt = session
        .items()
        .iter()
        .find_map(|item| match item {
            pl_protocol::ModelContextItem::ToolResult { receipt, .. } => Some(receipt),
            pl_protocol::ModelContextItem::Message { .. }
            | pl_protocol::ModelContextItem::PinnedContext { .. }
            | pl_protocol::ModelContextItem::Compaction { .. } => None,
        })
        .expect("tool receipt");
    assert_eq!(receipt.artifacts[0]["kind"], "largeArtifact");
    assert_eq!(receipt.artifacts[0].get("payload"), None);
}

#[derive(Debug)]
struct HistoryMarkerTool;

#[derive(Debug)]
struct LargeArtifactTool;

impl Tool for LargeArtifactTool {
    fn name(&self) -> &str {
        "large_artifact"
    }

    fn description(&self) -> &str {
        "Returns a large artifact payload"
    }

    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {},
            "additionalProperties": false
        })
    }

    fn execute<'a>(
        &'a self,
        _input: ToolInput,
        _context: ToolContext,
    ) -> std::pin::Pin<
        Box<
            dyn std::future::Future<Output = std::result::Result<ToolOutput, PureError>>
                + Send
                + 'a,
        >,
    > {
        Box::pin(async {
            Ok(ToolOutput {
                description: "large artifact ready".to_string(),
                truncated: OutputTruncation::empty(),
                output_file: std::path::PathBuf::new(),
                exit_code: Some(0),
                timed_out: false,
                runtime_events: vec![crate::tool::ToolRuntimeEvent::OutputArtifacts {
                    artifacts: vec![serde_json::json!({
                        "kind": "largeArtifact",
                        "payload": "x".repeat(64 * 1024),
                    })],
                }],
            })
        })
    }
}

impl Tool for HistoryMarkerTool {
    fn name(&self) -> &str {
        "history_marker"
    }

    fn description(&self) -> &str {
        "Records a marker in tool history"
    }

    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {},
            "additionalProperties": false
        })
    }

    fn execute<'a>(
        &'a self,
        _input: ToolInput,
        _context: ToolContext,
    ) -> std::pin::Pin<
        Box<
            dyn std::future::Future<Output = std::result::Result<ToolOutput, PureError>>
                + Send
                + 'a,
        >,
    > {
        Box::pin(async {
            Ok(ToolOutput {
                description: "history marker".to_string(),
                truncated: OutputTruncation::empty(),
                output_file: std::path::PathBuf::new(),
                exit_code: None,
                timed_out: false,
                runtime_events: Vec::new(),
            })
        })
    }
}

#[derive(Debug)]
struct ParentHistoryProbeTool;

impl Tool for ParentHistoryProbeTool {
    fn name(&self) -> &str {
        "parent_history_probe"
    }

    fn description(&self) -> &str {
        "Reports whether prior tool history is visible"
    }

    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {},
            "additionalProperties": false
        })
    }

    fn execute<'a>(
        &'a self,
        _input: ToolInput,
        context: ToolContext,
    ) -> std::pin::Pin<
        Box<
            dyn std::future::Future<Output = std::result::Result<ToolOutput, PureError>>
                + Send
                + 'a,
        >,
    > {
        Box::pin(async move {
            let marker_visible = context.parent_session.messages().iter().any(|message| {
                pl_protocol::ToolResultMetadata::from_metadata(&message.metadata)
                    .ok()
                    .is_some_and(|metadata| metadata.tool_name == "history_marker")
            });
            Ok(ToolOutput {
                description: if marker_visible {
                    "history marker visible"
                } else {
                    "history marker missing"
                }
                .to_string(),
                truncated: OutputTruncation::empty(),
                output_file: std::path::PathBuf::new(),
                exit_code: None,
                timed_out: false,
                runtime_events: Vec::new(),
            })
        })
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

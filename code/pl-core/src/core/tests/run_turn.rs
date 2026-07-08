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
    provider.bearer_token = Some("test-token".to_string());
    provider.default_model = "local-responses".to_string();
    let core = PureCore::from_provider_info(provider).unwrap();
    let (event_tx, _event_rx) = tokio::sync::broadcast::channel(32);
    let mut recorder = TraceRecorder::new("session-1".to_string(), event_tx, 0);
    let mut session = CoreSession::new();

    let result = core
        .run_turn_with_trace(
            &mut session,
            TurnRequest::new("Build the thing".to_string(), CompileMode::Auto)
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
    provider.bearer_token = Some("test-token".to_string());
    provider.default_model = "local-responses".to_string();
    let core = PureCore::from_provider_info(provider).unwrap();
    let (event_tx, mut event_rx) = tokio::sync::broadcast::channel(64);
    let mut recorder = TraceRecorder::new("session-1".to_string(), event_tx, 0);
    let mut session = CoreSession::new();

    let result = core
        .run_turn_with_trace(
            &mut session,
            TurnRequest::new("Build the thing".to_string(), CompileMode::Auto)
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
    provider.bearer_token = Some("test-token".to_string());
    provider.default_model = "local-responses".to_string();
    let core = PureCore::from_provider_info(provider).unwrap();
    let (event_tx, _event_rx) = tokio::sync::broadcast::channel(32);
    let mut recorder = TraceRecorder::new("session-1".to_string(), event_tx, 0);
    let mut session = CoreSession::new();

    let result = core
        .run_turn_with_trace(
            &mut session,
            TurnRequest::new("Build the thing".to_string(), CompileMode::Auto)
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
    provider.bearer_token = Some("test-token".to_string());
    provider.default_model = "local-responses".to_string();
    let mut model = pl_model::ModelInfo::fallback("local-responses");
    model.auto_compact_token_limit = Some(1);
    let core = PureCoreBuilder::from_provider_info_with_models(provider, vec![model])
        .unwrap()
        .build();
    let (event_tx, _event_rx) = tokio::sync::broadcast::channel(32);
    let mut recorder = TraceRecorder::new("session-1".to_string(), event_tx, 0);
    let mut session = CoreSession::new();
    session.push_user_prompt("old context".to_string());

    let result = core
        .run_turn_with_trace(
            &mut session,
            TurnRequest::new("continue".to_string(), CompileMode::Auto)
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
    assert_eq!(compaction.summary, "compressed memory");
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
async fn enabled_tools_snapshot_remains_internal_trace_event() {
    let mut core = PureCore::default_provider().unwrap();
    core.register_default_tools(std::env::temp_dir(), Some("rules".to_string()))
        .await;
    let events = record_enabled_tools_for_core(&core, "session-1", "turn-1", CompileMode::Auto);
    let event = enabled_tools_event(&events);

    assert_eq!(event.turn_id, "turn-1");
    assert!(event.tools.contains(&"read_file".to_string()));
}

#[tokio::test]
async fn run_turn_uses_prompt_cache_and_previous_response_id_incrementally() {
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
    provider.bearer_token = Some("test-token".to_string());
    provider.default_model = "local-responses".to_string();
    let core = PureCore::from_provider_info(provider).unwrap();
    let (event_tx, _event_rx) = tokio::sync::broadcast::channel(32);
    let mut recorder = TraceRecorder::new("session-1".to_string(), event_tx, 0);
    let mut session = CoreSession::new();
    let options = TurnOptions::default().with_prompt_cache_key("cache-session".to_string());

    core.run_turn_with_trace(
        &mut session,
        TurnRequest::new("first prompt".to_string(), CompileMode::Auto)
            .with_budget(crate::turn::TurnBudget::new(60_000)),
        &mut recorder,
        options.clone(),
    )
    .await
    .unwrap();
    core.run_turn_with_trace(
        &mut session,
        TurnRequest::new("second prompt".to_string(), CompileMode::Auto)
            .with_budget(crate::turn::TurnBudget::new(60_000)),
        &mut recorder,
        options,
    )
    .await
    .unwrap();
    handle.await.unwrap();

    let bodies = bodies.lock().unwrap();
    assert_eq!(bodies.len(), 2);
    assert_eq!(bodies[0]["store"], serde_json::json!(true));
    assert_eq!(
        bodies[0]["prompt_cache_key"],
        serde_json::json!("cache-session")
    );
    assert!(bodies[0].get("previous_response_id").is_none());
    assert_eq!(bodies[1]["store"], serde_json::json!(true));
    assert_eq!(
        bodies[1]["prompt_cache_key"],
        serde_json::json!("cache-session")
    );
    assert_eq!(
        bodies[1]["previous_response_id"],
        serde_json::json!("resp_1")
    );
    let second_input = serde_json::to_string(&bodies[1]["input"]).unwrap();
    assert!(second_input.contains("second prompt"));
    assert!(!second_input.contains("first prompt"));
    assert!(!second_input.contains("first ok"));
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
    provider.bearer_token = Some("test-token".to_string());
    provider.default_model = "local-responses".to_string();
    let runtime = CoreRuntimeProfile::minimal().with_runtime_options(
        CoreRuntimeOptions::default()
            .with_turn_options(TurnOptions::default().with_prompt_cache_key("profile-cache")),
    );
    let core = PureCoreBuilder::from_provider_info(provider)
        .unwrap()
        .with_runtime_profile(runtime)
        .build();
    let (event_tx, _event_rx) = tokio::sync::broadcast::channel(32);
    let mut session = CoreSession::new();

    core.run_turn(
        &mut session,
        TurnRequest::new("profile prompt".to_string(), CompileMode::Auto)
            .with_budget(crate::turn::TurnBudget::new(60_000)),
        event_tx,
    )
    .await
    .unwrap();
    handle.await.unwrap();

    let bodies = bodies.lock().unwrap();
    assert_eq!(bodies.len(), 1);
    assert_eq!(bodies[0]["store"], serde_json::json!(true));
    assert_eq!(
        bodies[0]["prompt_cache_key"],
        serde_json::json!("profile-cache")
    );
}

#[tokio::test]
async fn run_turn_retries_full_history_when_continuation_is_unsupported() {
    let retry_sse = concat!(
        "data: {\"type\":\"response.output_item.added\",\"item\":{\"id\":\"msg_retry\",\"type\":\"message\",\"role\":\"assistant\",\"phase\":\"final_answer\"}}\n\n",
        "data: {\"type\":\"response.output_text.delta\",\"item_id\":\"msg_retry\",\"delta\":\"retry ok\"}\n\n",
        "data: {\"type\":\"response.output_item.done\",\"item\":{\"id\":\"msg_retry\",\"type\":\"message\",\"role\":\"assistant\",\"phase\":\"final_answer\",\"content\":[{\"type\":\"output_text\",\"text\":\"retry ok\"}]}}\n\n",
        "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_retry\",\"usage\":{\"input_tokens\":1,\"output_tokens\":2,\"total_tokens\":3}}}\n\n",
        "data: [DONE]\n\n"
    )
    .to_string();
    let unsupported = serde_json::json!({
        "error": {
            "message": "previous_response_id is not supported by this endpoint"
        }
    })
    .to_string();
    let (base_url, bodies, handle) = serve_http_sequence(vec![
        TestHttpResponse::json(400, unsupported),
        TestHttpResponse::sse(retry_sse),
    ])
    .await;
    let mut provider = ProviderInfo::openai(Some(base_url));
    provider.bearer_token = Some("test-token".to_string());
    provider.default_model = "local-responses".to_string();
    let core = PureCore::from_provider_info(provider).unwrap();
    let (event_tx, _event_rx) = tokio::sync::broadcast::channel(32);
    let mut recorder = TraceRecorder::new("session-1".to_string(), event_tx, 0);
    let mut session = CoreSession::new();
    session.push_user_prompt("old prompt".to_string());
    session.push_assistant_response("old answer".to_string(), None);
    session.set_prompt_cache_key("cache-session".to_string());
    session.acknowledge_model_response(session.len(), Some("resp_old".to_string()));

    let result = core
        .run_turn_with_trace(
            &mut session,
            TurnRequest::new("new prompt".to_string(), CompileMode::Auto)
                .with_budget(crate::turn::TurnBudget::new(60_000)),
            &mut recorder,
            TurnOptions::default().with_prompt_cache_key("cache-session".to_string()),
        )
        .await
        .unwrap();
    handle.await.unwrap();

    assert_eq!(result.status, TurnResultStatus::Completed);
    let bodies = bodies.lock().unwrap();
    assert_eq!(bodies.len(), 2);
    assert_eq!(
        bodies[0]["previous_response_id"],
        serde_json::json!("resp_old")
    );
    assert!(bodies[1].get("previous_response_id").is_none());
    let retry_input = serde_json::to_string(&bodies[1]["input"]).unwrap();
    assert!(retry_input.contains("old prompt"));
    assert!(retry_input.contains("old answer"));
    assert!(retry_input.contains("new prompt"));
}

#[tokio::test]
async fn model_turn_helper_retries_full_history_when_continuation_is_unsupported() {
    let retry_sse = concat!(
        "data: {\"type\":\"response.output_item.added\",\"item\":{\"id\":\"msg_retry\",\"type\":\"message\",\"role\":\"assistant\",\"phase\":\"final_answer\"}}\n\n",
        "data: {\"type\":\"response.output_text.delta\",\"item_id\":\"msg_retry\",\"delta\":\"retry ok\"}\n\n",
        "data: {\"type\":\"response.output_item.done\",\"item\":{\"id\":\"msg_retry\",\"type\":\"message\",\"role\":\"assistant\",\"phase\":\"final_answer\",\"content\":[{\"type\":\"output_text\",\"text\":\"retry ok\"}]}}\n\n",
        "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_retry\",\"usage\":{\"input_tokens\":4,\"output_tokens\":2,\"total_tokens\":6}}}\n\n",
        "data: [DONE]\n\n"
    )
    .to_string();
    let unsupported = serde_json::json!({
        "error": {
            "message": "previous_response_id is only supported on Responses WebSocket v2"
        }
    })
    .to_string();
    let (base_url, bodies, handle) = serve_http_sequence(vec![
        TestHttpResponse::json(400, unsupported),
        TestHttpResponse::sse(retry_sse),
    ])
    .await;
    let mut provider_info = ProviderInfo::openai(Some(base_url));
    provider_info.bearer_token = Some("test-token".to_string());
    provider_info.default_model = "local-responses".to_string();
    let provider = pl_model::create_provider(provider_info).unwrap();
    let mut session = CoreSession::new();
    session.push_user_prompt("old prompt".to_string());
    session.push_assistant_response("old answer".to_string(), None);
    session.set_prompt_cache_key("cache-session".to_string());
    session.acknowledge_model_response(session.len(), Some("resp_old".to_string()));

    let response = stream_session_completion_response(
        provider,
        &mut session,
        CoreModelTurnRequest::new("local-responses")
            .with_instructions("reply briefly")
            .with_continuation(true),
        CoreModelTurnOptions::default(),
    )
    .await
    .unwrap();
    handle.await.unwrap();

    assert_eq!(response.content.as_deref(), Some("retry ok"));
    assert!(session.continuation_disabled());
    assert_eq!(session.previous_response_id(), None);
    let bodies = bodies.lock().unwrap();
    assert_eq!(bodies.len(), 2);
    assert_eq!(
        bodies[0]["previous_response_id"],
        serde_json::json!("resp_old")
    );
    assert!(bodies[1].get("previous_response_id").is_none());
    let retry_input = serde_json::to_string(&bodies[1]["input"]).unwrap();
    assert!(retry_input.contains("old prompt"));
    assert!(retry_input.contains("old answer"));
}

#[tokio::test]
async fn model_turn_client_caches_unsupported_continuation_by_key() {
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
    let unsupported = serde_json::json!({
        "error": {
            "message": "previous_response_id is not supported"
        }
    })
    .to_string();
    let (base_url, bodies, handle) = serve_http_sequence(vec![
        TestHttpResponse::json(400, unsupported),
        TestHttpResponse::sse(retry_sse),
        TestHttpResponse::sse(second_sse),
    ])
    .await;
    let mut provider_info = ProviderInfo::openai(Some(base_url));
    provider_info.bearer_token = Some("test-token".to_string());
    provider_info.default_model = "local-responses".to_string();
    let provider = pl_model::create_provider(provider_info).unwrap();
    let client = CoreModelTurnClient::new();

    let mut first_session = CoreSession::new();
    first_session.push_user_prompt("old prompt".to_string());
    first_session.push_assistant_response("old answer".to_string(), None);
    first_session.acknowledge_model_response(first_session.len(), Some("resp_old".to_string()));
    let first = client
        .stream_session_completion_response(
            provider.clone(),
            &mut first_session,
            CoreModelTurnRequest::new("local-responses")
                .with_instructions("reply briefly")
                .with_continuation(true)
                .with_continuation_cache_key("openai|local-responses"),
            CoreModelTurnOptions::default(),
        )
        .await
        .unwrap();
    assert_eq!(first.content.as_deref(), Some("retry ok"));

    let mut second_session = CoreSession::new();
    second_session.push_user_prompt("new prompt".to_string());
    second_session.push_assistant_response("new answer".to_string(), None);
    second_session
        .acknowledge_model_response(second_session.len(), Some("resp_second_old".to_string()));
    let second = client
        .stream_session_completion_response(
            provider,
            &mut second_session,
            CoreModelTurnRequest::new("local-responses")
                .with_instructions("reply briefly")
                .with_continuation(true)
                .with_continuation_cache_key("openai|local-responses"),
            CoreModelTurnOptions::default(),
        )
        .await
        .unwrap();
    handle.await.unwrap();

    assert_eq!(second.content.as_deref(), Some("second ok"));
    let bodies = bodies.lock().unwrap();
    assert_eq!(bodies.len(), 3);
    assert_eq!(
        bodies[0]["previous_response_id"],
        serde_json::json!("resp_old")
    );
    assert!(bodies[1].get("previous_response_id").is_none());
    assert!(bodies[2].get("previous_response_id").is_none());
    assert_eq!(bodies[2]["store"], serde_json::json!(false));
}

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
async fn enabled_tools_snapshot_remains_internal_trace_event() {
    let mut core = PureCore::default_provider().unwrap();
    core.register_default_tools(std::env::temp_dir(), Some("rules".to_string()))
        .await;
    let events = record_enabled_tools_for_core(&core, "session-1", "turn-1", CompileMode::Auto);
    let event = enabled_tools_event(&events);

    assert_eq!(event.turn_id, "turn-1");
    assert!(event.tools.contains(&"read_file".to_string()));
}

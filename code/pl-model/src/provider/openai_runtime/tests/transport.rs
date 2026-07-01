use super::*;
use pretty_assertions::assert_eq;

#[tokio::test]
async fn stream_complete_uses_chat_endpoint_without_auth_when_token_missing() {
    let sse_body = concat!(
        "data: {\"choices\":[{\"delta\":{\"content\":\"<final>ok</final>\"},\"finish_reason\":null}]}\n\n",
        "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}],\"usage\":{\"prompt_tokens\":1,\"completion_tokens\":2,\"total_tokens\":3}}\n\n",
        "data: [DONE]\n\n"
    )
    .to_string();
    let (base_url, handle) = serve_sse_once(sse_body).await;
    let mut model = ModelInfo::fallback("local-chat");
    model.context_window = Some(128_000);
    let provider = OpenAiProvider::new(
        ProviderInfo {
            provider_kind: crate::provider_info::ProviderKind::DeepSeek,
            name: "Local Chat".to_string(),
            base_url,
            default_model: "local-chat".to_string(),
            bearer_token: None,
            http_headers: None,
            tool_wire_policy: crate::provider_info::ToolWirePolicy::FunctionFallback,
            apply_patch_tool_type: None,
        },
        vec![model],
    )
    .unwrap();
    let (event_tx, _event_rx) = tokio::sync::broadcast::channel(8);

    let response = provider
        .stream_complete(minimal_request("local-chat"), event_tx)
        .await
        .unwrap();
    let captured = handle.await.unwrap();

    assert_eq!(response.content.as_deref(), Some("ok"));
    assert_eq!(response.usage.total_tokens, 3);
    assert_eq!(captured.request_line, "POST /chat/completions HTTP/1.1");
    assert!(!captured.headers.contains_key("authorization"));
    assert_eq!(captured.body["stream"], serde_json::json!(true));
}

#[tokio::test]
async fn stream_complete_chat_tags_project_commentary_and_final_only() {
    let sse_body = concat!(
        "data: {\"choices\":[{\"delta\":{\"content\":\"<commentary>检查配置。</commentary>\"},\"finish_reason\":null}]}\n\n",
        "data: {\"choices\":[{\"delta\":{\"content\":\"<final>Ready</final>\"},\"finish_reason\":null}]}\n\n",
        "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}],\"usage\":{\"prompt_tokens\":1,\"completion_tokens\":2,\"total_tokens\":3}}\n\n",
        "data: [DONE]\n\n"
    )
    .to_string();
    let (base_url, handle) = serve_sse_once(sse_body).await;
    let mut model = ModelInfo::fallback("local-chat");
    model.context_window = Some(128_000);
    let provider = OpenAiProvider::new(
        ProviderInfo {
            provider_kind: crate::provider_info::ProviderKind::DeepSeek,
            name: "Local Chat".to_string(),
            base_url,
            default_model: "local-chat".to_string(),
            bearer_token: None,
            http_headers: None,
            tool_wire_policy: crate::provider_info::ToolWirePolicy::FunctionFallback,
            apply_patch_tool_type: None,
        },
        vec![model],
    )
    .unwrap();
    let request = CompletionRequest {
        trace: Some(CompletionTraceContext {
            session_id: "session-1".to_string(),
            turn_id: "turn-1".to_string(),
            inference_id: "inf-1".to_string(),
            plan_mode: true,
            trace_sequence_base: 0,
        }),
        ..minimal_request("local-chat")
    };
    let (event_tx, _event_rx) = tokio::sync::broadcast::channel(32);

    let response = provider.stream_complete(request, event_tx).await.unwrap();
    let captured = handle.await.unwrap();

    assert_eq!(captured.request_line, "POST /chat/completions HTTP/1.1");
    assert_eq!(response.content.as_deref(), Some("Ready"));
    assert!(response.trace_events.iter().any(|event| matches!(
        &event.kind,
        TraceEventKind::TracePartCompleted { item }
            if item.text_channel == Some(pl_trace::TraceTextChannel::Commentary)
                && item.content == "检查配置。"
    )));
    assert!(!response.trace_events.iter().any(|event| matches!(
        &event.kind,
        TraceEventKind::TracePartCompleted { item } if item.kind == TracePartKind::Plan
    )));
    assert!(response.trace_events.iter().any(|event| matches!(
        &event.kind,
        TraceEventKind::TracePartCompleted { item }
            if item.text_channel == Some(pl_trace::TraceTextChannel::Final)
                && item.content == "Ready"
    )));
}

#[tokio::test]
async fn stream_complete_sends_responses_bearer_and_custom_headers() {
    let sse_body = concat!(
        "data: {\"type\":\"response.output_item.added\",\"item\":{\"id\":\"msg_1\",\"type\":\"message\",\"role\":\"assistant\",\"phase\":\"final_answer\"}}\n\n",
        "data: {\"type\":\"response.output_text.delta\",\"item_id\":\"msg_1\",\"delta\":\"ok\"}\n\n",
        "data: {\"type\":\"response.output_item.done\",\"item\":{\"id\":\"msg_1\",\"type\":\"message\",\"role\":\"assistant\",\"phase\":\"final_answer\",\"content\":[{\"type\":\"output_text\",\"text\":\"ok\"}]}}\n\n",
        "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_1\",\"usage\":{\"input_tokens\":1,\"output_tokens\":2,\"total_tokens\":3}}}\n\n",
        "data: [DONE]\n\n"
    )
    .to_string();
    let (base_url, handle) = serve_sse_once(sse_body).await;
    let mut model = ModelInfo::fallback("local-responses");
    model.context_window = Some(128_000);
    let provider = OpenAiProvider::new(
        ProviderInfo {
            provider_kind: crate::provider_info::ProviderKind::OpenAi,
            name: "Local Responses".to_string(),
            base_url,
            bearer_token: Some("test-token".to_string()),
            http_headers: Some(HashMap::from([(
                "x-provider-test".to_string(),
                "present".to_string(),
            )])),
            default_model: "local-responses".to_string(),
            tool_wire_policy: crate::provider_info::ToolWirePolicy::NativeCustomTools,
            apply_patch_tool_type: Some(crate::provider_info::ApplyPatchToolType::Freeform),
        },
        vec![model],
    )
    .unwrap();
    let (event_tx, _event_rx) = tokio::sync::broadcast::channel(8);

    let response = provider
        .stream_complete(minimal_request("local-responses"), event_tx)
        .await
        .unwrap();
    let captured = handle.await.unwrap();

    assert_eq!(response.content.as_deref(), Some("ok"));
    assert_eq!(response.usage.total_tokens, 3);
    assert_eq!(captured.request_line, "POST /responses HTTP/1.1");
    assert_eq!(
        captured.headers.get("authorization").map(String::as_str),
        Some("Bearer test-token")
    );
    assert_eq!(
        captured.headers.get("x-provider-test").map(String::as_str),
        Some("present")
    );
    assert_eq!(captured.body["stream"], serde_json::json!(true));
}

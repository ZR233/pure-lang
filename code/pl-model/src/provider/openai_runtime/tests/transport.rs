use super::*;
use crate::request::{
    ModelCompactionRequest, OpenAiCompactionMode, ReasoningConfig, ReasoningSummary, ToolSchema,
};
use pl_protocol::ModelContextItem;
use pretty_assertions::assert_eq;

fn openai_provider(base_url: String) -> OpenAiProvider {
    let mut model = default_models()
        .into_iter()
        .find(|model| model.slug == "gpt-5.5")
        .expect("bundled reasoning model");
    model.slug = "local-responses".to_string();
    model.context_window = Some(128_000);
    OpenAiProvider::new(
        ProviderInfo {
            provider_kind: crate::provider_info::ProviderKind::OpenAi,
            name: "Local Responses".to_string(),
            base_url,
            bearer_token: Some("test-token".to_string()),
            http_headers: Some(HashMap::from([
                ("x-provider-test".to_string(), "present".to_string()),
                (
                    "x-codex-beta-features".to_string(),
                    "existing_feature".to_string(),
                ),
            ])),
            default_model: "local-responses".to_string(),
            tool_wire_policy: crate::provider_info::ToolWirePolicy::NativeCustomTools,
            apply_patch_tool_type: Some(crate::provider_info::ApplyPatchToolType::Freeform),
        },
        vec![model],
    )
    .unwrap()
}

fn compaction_request(mode: OpenAiCompactionMode) -> ModelCompactionRequest {
    ModelCompactionRequest {
        mode,
        model: "local-responses".to_string(),
        instructions: "canonical instructions".to_string(),
        input: vec![ModelContextItem::from(Message {
            role: MessageRole::User,
            content: MessageContent::Text("hello".to_string()),
            reasoning_content: None,
            metadata: HashMap::new(),
        })],
        tools: vec![ToolSchema::function(
            "read_file",
            "Read a file",
            serde_json::json!({"type": "object", "properties": {}}),
        )],
        parallel_tool_calls: true,
        reasoning: Some(ReasoningConfig {
            effort: Some("medium".to_string()),
            summary: Some(ReasoningSummary::Enabled),
        }),
        prompt_cache_key: Some("cache-key".to_string()),
    }
}

#[tokio::test]
async fn legacy_compaction_uses_compact_endpoint_and_common_request_fields() {
    let body = serde_json::json!({
        "output": [
            {
                "type": "message",
                "role": "assistant",
                "content": [{"type": "output_text", "text": "summary"}]
            },
            {"type": "compaction", "encrypted_content": "encrypted"},
            {"type": "reasoning", "summary": []}
        ]
    })
    .to_string();
    let (base_url, handle) = serve_sse_once(body).await;
    let provider = openai_provider(base_url);

    let response = provider
        .compact_context(compaction_request(OpenAiCompactionMode::RemoteLegacy))
        .await
        .unwrap();
    let captured = handle.await.unwrap();

    assert_eq!(captured.request_line, "POST /responses/compact HTTP/1.1");
    assert_eq!(captured.headers["authorization"], "Bearer test-token");
    assert_eq!(captured.headers["x-provider-test"], "present");
    assert_eq!(captured.body["instructions"], "canonical instructions");
    assert_eq!(captured.body["parallel_tool_calls"], true);
    assert_eq!(captured.body["prompt_cache_key"], "cache-key");
    assert_eq!(captured.body["reasoning"]["effort"], "medium");
    assert_eq!(captured.body["tools"][0]["name"], "read_file");
    assert_eq!(response.input.len(), 2);
    assert!(
        response
            .input
            .last()
            .is_some_and(ModelContextItem::is_compaction)
    );
}

#[tokio::test]
async fn v2_compaction_uses_responses_trigger_feature_and_completed_usage() {
    let sse_body = concat!(
        "data: {\"type\":\"response.output_item.done\",\"item\":{\"type\":\"message\",\"role\":\"assistant\",\"content\":[{\"type\":\"output_text\",\"text\":\"ignored\"}]}}\n\n",
        "data: {\"type\":\"response.output_item.done\",\"item\":{\"type\":\"compaction\",\"encrypted_content\":\"encrypted-v2\"}}\n\n",
        "data: {\"type\":\"response.completed\",\"response\":{\"usage\":{\"input_tokens\":12,\"output_tokens\":3,\"total_tokens\":15}}}\n\n",
        "data: [DONE]\n\n"
    )
    .to_string();
    let (base_url, handle) = serve_sse_once(sse_body).await;
    let provider = openai_provider(base_url);

    let response = provider
        .compact_context(compaction_request(OpenAiCompactionMode::RemoteV2))
        .await
        .unwrap();
    let captured = handle.await.unwrap();

    assert_eq!(captured.request_line, "POST /responses HTTP/1.1");
    assert_eq!(
        captured.headers["x-codex-beta-features"],
        "existing_feature,remote_compaction_v2"
    );
    assert_eq!(
        captured.body["input"].as_array().unwrap().last().unwrap(),
        &serde_json::json!({"type": "compaction_trigger"})
    );
    assert_eq!(response.input.len(), 1);
    assert_eq!(response.usage.unwrap().total_tokens, 15);
}

#[tokio::test]
async fn v2_compaction_retries_stream_close_at_most_twice() {
    let closed = "data: [DONE]\n\n".to_string();
    let completed = concat!(
        "data: {\"type\":\"response.output_item.done\",\"item\":{\"type\":\"compaction\",\"encrypted_content\":\"retried\"}}\n\n",
        "data: {\"type\":\"response.completed\",\"response\":{\"usage\":{\"input_tokens\":1,\"output_tokens\":1,\"total_tokens\":2}}}\n\n",
        "data: [DONE]\n\n"
    )
    .to_string();
    let (base_url, handle) = serve_sse_sequence(vec![closed.clone(), closed, completed]).await;
    let provider = openai_provider(base_url);

    let response = provider
        .compact_context(compaction_request(OpenAiCompactionMode::RemoteV2))
        .await
        .unwrap();
    let captured = handle.await.unwrap();

    assert_eq!(captured.len(), 3);
    assert_eq!(
        response.input,
        vec![ModelContextItem::Compaction {
            encrypted_content: "retried".to_string()
        }]
    );
}

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
async fn openai_compatible_chat_provider_uses_chat_endpoint() {
    let sse_body = concat!(
        "data: {\"choices\":[{\"delta\":{\"content\":\"<final>mimo ok</final>\"},\"finish_reason\":null}]}\n\n",
        "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}],\"usage\":{\"prompt_tokens\":1,\"completion_tokens\":2,\"total_tokens\":3}}\n\n",
        "data: [DONE]\n\n"
    )
    .to_string();
    let (base_url, handle) = serve_sse_once(sse_body).await;
    let mut model = ModelInfo::fallback("mimo-chat");
    model.context_window = Some(128_000);
    let provider = OpenAiProvider::new(
        ProviderInfo::openai_compatible_chat("MiMo", base_url, "mimo-chat"),
        vec![model],
    )
    .unwrap();
    let (event_tx, _event_rx) = tokio::sync::broadcast::channel(8);

    let response = provider
        .stream_complete(minimal_request("mimo-chat"), event_tx)
        .await
        .unwrap();
    let captured = handle.await.unwrap();

    assert_eq!(response.content.as_deref(), Some("mimo ok"));
    assert_eq!(captured.request_line, "POST /chat/completions HTTP/1.1");
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

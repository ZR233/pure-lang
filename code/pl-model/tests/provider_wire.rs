use pl_core::{
    context::{ContextContent, ContextSource},
    model::ModelFactory,
    thread::{ModelStepLimit, ThreadHandle, TurnInput, TurnOutcome, TurnState},
};
use pl_model::{
    completion::{
        AttachmentInput, AttachmentModality, AttachmentSource, CompletionPresentationItemKind,
        CompletionPresentationPartKind, CompletionRequest, CompletionTraceContext, ContentPart,
        HostedWebSearchOptions, Message, MessageContent, MessageRole, ModelContextItem,
        ReasoningConfig, ReasoningSummary, ToolCallKind, ToolSpec, programmatic_tool_declaration,
    },
    config::builtin_provider_catalog,
    model::{ModelInfo, ModelTransportProfile, default_models},
    provider::{
        FileUploadCapability, PromptCacheDialect, ProviderClient, ProviderEndpoint,
        openai::{CacheMode, OpenAiCompletion, OpenAiCompletionOptions, PromptCacheOptions},
    },
    runtime::{CancellationToken, ModelInvocationContext, ModelRuntime, ModelSession},
};
use pl_protocol::trace::{InMemoryTraceEventSink, TraceEventKind, TracePartKind, TraceTextChannel};
use pl_protocol::{PricingOutcome, ToolCallCaller, ToolResultRecord};
use pl_provider_fixture::{
    FixtureServer, GUI_PROMPT, Protocol, Reply, RequestMatch, Step, gui_script, responses_text,
};
use pretty_assertions::assert_eq;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::sync::Arc;

fn user(text: &str) -> Message {
    Message {
        presentation: Default::default(),
        role: MessageRole::User,
        content: MessageContent::text(text),
        reasoning_content: None,
        tool_calls: None,
        tool_result: None,
        metadata: Default::default(),
    }
}

fn assistant(text: &str) -> Message {
    Message {
        role: MessageRole::Assistant,
        content: MessageContent::text(text),
        ..user("")
    }
}

fn request(prompt: &str) -> CompletionRequest {
    CompletionRequest::builder()
        .messages(vec![user(prompt)])
        .build()
}

fn model(slug: &str, profile: ModelTransportProfile) -> ModelInfo {
    let mut model = ModelInfo::compatible(slug);
    model.binding.set_transport(profile);
    model
}

fn bundled(slug: &str) -> ModelInfo {
    default_models()
        .into_iter()
        .find(|model| model.slug == slug)
        .unwrap()
}

async fn complete(
    fixture: &FixtureServer,
    model: ModelInfo,
    request: CompletionRequest,
    session: ModelSession,
) -> pl_model::completion::CompletionResponse {
    let runtime = ModelRuntime::new(
        ProviderEndpoint::compatible("fixture", fixture.base_url()),
        model,
    )
    .unwrap();
    runtime
        .complete(request, ModelInvocationContext::new(session))
        .await
        .unwrap()
}

fn chat_events(text: &str, model: &str) -> Vec<Value> {
    vec![
        json!({"id":"chat-1","model":model,"choices":[{"delta":{"reasoning_content":"thinking "},"finish_reason":null}]}),
        json!({"id":"chat-1","model":model,"choices":[{"delta":{"content":text},"finish_reason":null}]}),
        json!({"id":"chat-1","model":model,"choices":[{"delta":{},"finish_reason":"stop"}]}),
        json!({"id":"chat-1","model":model,"choices":[],"usage":{"prompt_tokens":17,"completion_tokens":7,"total_tokens":24,"prompt_tokens_details":{"cached_tokens":3},"completion_tokens_details":{"reasoning_tokens":2}}}),
    ]
}

fn image_request(prompt: &str, bytes: Vec<u8>) -> CompletionRequest {
    let message = Message {
        content: MessageContent::new(vec![
            ContentPart::Text {
                text: prompt.into(),
            },
            ContentPart::Attachment {
                attachment_id: "image-1".into(),
                modality: AttachmentModality::Image,
                media_type: "image/png".into(),
                filename: Some("tiny.png".into()),
            },
        ]),
        ..user("")
    };
    CompletionRequest::builder()
        .messages(vec![message])
        .attachments(vec![AttachmentInput {
            attachment_id: "image-1".into(),
            modality: AttachmentModality::Image,
            media_type: "image/png".into(),
            filename: Some("tiny.png".into()),
            source: AttachmentSource::Bytes {
                bytes: Arc::from(bytes),
            },
        }])
        .build()
}

fn tiny_png() -> Vec<u8> {
    let mut image = std::io::Cursor::new(Vec::new());
    image::DynamicImage::new_rgb8(1, 1)
        .write_to(&mut image, image::ImageFormat::Png)
        .unwrap();
    image.into_inner()
}

#[tokio::test]
async fn core_thread_runs_provider_backed_turn_and_commits_effects() {
    let fixture = FixtureServer::start(vec![Step::prompt(
        Protocol::ResponsesHttp,
        "cross-crate request",
        0,
        Reply::Sse(responses_text(
            "cross-crate answer",
            "thread-response",
            "core-fixture",
        )),
    )])
    .await
    .unwrap();
    let runtime = ModelRuntime::new(
        ProviderEndpoint::compatible("fixture", fixture.base_url()),
        model("core-fixture", ModelTransportProfile::responses_http()),
    )
    .unwrap();
    let session = ModelFactory::new(pl_model::runtime::ThreadModel::new(runtime, None))
        .open_session()
        .await
        .unwrap();
    let thread = ThreadHandle::start("fixture-thread".into(), session).unwrap();
    let completion = thread
        .run_turn(TurnInput {
            turn_id: "wire-turn".into(),
            attempt_prefix: "wire-attempt".into(),
            content: vec![ContextContent::Text {
                text: Arc::from("cross-crate request"),
            }],
            max_model_steps: ModelStepLimit::Limited(1.try_into().unwrap()),
            cancellation: Default::default(),
        })
        .await
        .unwrap();
    assert_eq!(completion.outcome, TurnOutcome::Completed);
    assert_eq!(completion.model_steps, 1);
    assert!(thread.snapshot().context.records.iter().any(|record| {
        record.source == ContextSource::Assistant
            && record.content.contains(&ContextContent::Text {
                text: Arc::from("cross-crate answer"),
            })
    }));
    let effects = thread.effects().await.unwrap();
    assert!(effects.iter().any(|effect| {
        effect.turn.as_ref().is_some_and(|turn| {
            turn.turn_id == "wire-turn" && turn.state == TurnState::Finished(TurnOutcome::Completed)
        })
    }));
    thread.close().await.unwrap();
    let records = fixture.finish().await.unwrap();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].path, "/v1/responses");
}

#[tokio::test]
async fn gui_fixture_accepts_either_order_of_main_and_optional_title_requests() {
    let title = match &gui_script()[0].request {
        RequestMatch::Prompt { text, .. } => text.clone(),
        _ => unreachable!(),
    };
    for (first, second) in [(GUI_PROMPT, title.as_str()), (title.as_str(), GUI_PROMPT)] {
        let fixture = FixtureServer::start(gui_script()).await.unwrap();
        let model = model("fixture-model", ModelTransportProfile::responses_http());
        complete(
            &fixture,
            model.clone(),
            request(first),
            ModelSession::default(),
        )
        .await;
        complete(&fixture, model, request(second), ModelSession::default()).await;
        let requests = fixture.finish().await.unwrap();
        assert_eq!(requests.len(), 2);
        assert!(requests.iter().all(|request| request.accepted));
    }
}

#[tokio::test]
async fn paced_provider_stream_preserves_all_mixed_events_and_limits_rate() {
    let fixture = FixtureServer::start(vec![Step::prompt(
        Protocol::ResponsesHttp,
        "paced stress wire",
        0,
        Reply::PacedSse {
            events: 1_200,
            tokens_per_second: 5_000,
        },
    )])
    .await
    .unwrap();
    let runtime = ModelRuntime::new(
        ProviderEndpoint::compatible("fixture", fixture.base_url()),
        model("fixture-model", ModelTransportProfile::responses_http()),
    )
    .unwrap();
    let sink = Arc::new(InMemoryTraceEventSink::new("paced-session", 0));
    let result = runtime
        .complete(
            request("paced stress wire"),
            ModelInvocationContext::new(ModelSession::default()).with_trace(
                CompletionTraceContext {
                    session_id: "paced-session".into(),
                    turn_id: "paced-turn".into(),
                    inference_id: "paced-inference".into(),
                },
                sink.clone(),
            ),
        )
        .await
        .unwrap();
    assert_eq!(result.accounting.usage.output_tokens, Some(1_200));
    assert_eq!(result.presentation_items.len(), 1_200);
    assert_eq!(
        result
            .content
            .as_deref()
            .unwrap_or_default()
            .matches("answer-")
            .count(),
        300
    );
    assert_eq!(
        result
            .reasoning_content
            .as_deref()
            .unwrap_or_default()
            .matches("thought-")
            .count(),
        300
    );
    let mut provider_ids = std::collections::BTreeSet::new();
    for (index, item) in result.presentation_items.iter().enumerate() {
        assert_eq!(item.provider_item_id, format!("stress-item-{index}"));
        assert_eq!(item.output_index, Some(index as u32));
        assert!(provider_ids.insert(&item.provider_item_id));
        assert_eq!(item.parts.len(), 1);
        assert_eq!(item.parts[0].content_index, 0);
        assert_eq!(
            item.parts[0].provider_part_id.as_deref(),
            Some(format!("stress-part-{index}").as_str())
        );
        assert_eq!(
            item.parts[0].text,
            format!(
                "{}-{index} ",
                ["answer", "comment", "note", "thought"][index % 4]
            )
        );
    }
    assert_eq!(
        result.presentation_items[0].kind,
        CompletionPresentationItemKind::Text(TraceTextChannel::Final)
    );
    assert_eq!(
        result.presentation_items[1].kind,
        CompletionPresentationItemKind::Text(TraceTextChannel::Commentary)
    );
    assert_eq!(
        result.presentation_items[2].kind,
        CompletionPresentationItemKind::Reasoning
    );
    assert_eq!(
        result.presentation_items[2].parts[0].kind,
        CompletionPresentationPartKind::SummaryText
    );
    assert_eq!(
        result.presentation_items[3].parts[0].kind,
        CompletionPresentationPartKind::ReasoningText
    );
    let trace = sink.events();
    let started = trace
        .iter()
        .filter_map(|event| match &event.kind {
            TraceEventKind::TracePartStarted { item }
                if matches!(item.kind(), TracePartKind::Text | TracePartKind::Thinking) =>
            {
                Some(item.item_id())
            }
            _ => None,
        })
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(started.len(), 1_200);
    assert_eq!(
        trace
            .iter()
            .filter(|event| matches!(event.kind, TraceEventKind::TracePartCompleted { .. }))
            .count(),
        1_200
    );
    let report = fixture.shutdown().await.unwrap();
    report.verify().unwrap();
    let stress = report.stress.unwrap();
    assert_eq!(stress.emitted_events, 1_200);
    assert!(
        stress.elapsed_millis >= 230,
        "fixture exceeded its configured nominal token rate"
    );
}

#[tokio::test]
async fn responses_message_parts_retain_distinct_provider_identity_and_trace_rows() {
    let events = vec![
        json!({"type":"response.created","response":{"id":"parts-response","model":"parts-model"}}),
        json!({"type":"response.output_item.added","output_index":0,"item":{"id":"message-a","type":"message","role":"assistant","content":[]}}),
        json!({"type":"response.output_text.delta","item_id":"message-a","content_index":1,"delta":"first "}),
        json!({"type":"response.output_text.delta","item_id":"message-a","content_index":2,"delta":"second"}),
        json!({"type":"response.output_item.done","output_index":0,"item":{"id":"message-a","type":"message","role":"assistant","content":[{"type":"refusal","refusal":""},{"id":"part-a","type":"output_text","text":"first "},{"id":"part-b","type":"output_text","text":"second"}]}}),
        json!({"type":"response.completed","response":{"id":"parts-response","model":"parts-model","usage":{"input_tokens":1,"output_tokens":2}}}),
    ];
    let fixture = FixtureServer::start(vec![Step::prompt(
        Protocol::ResponsesHttp,
        "two parts",
        0,
        Reply::Sse(events),
    )])
    .await
    .unwrap();
    let runtime = ModelRuntime::new(
        ProviderEndpoint::compatible("fixture", fixture.base_url()),
        model("parts-model", ModelTransportProfile::responses_http()),
    )
    .unwrap();
    let sink = Arc::new(InMemoryTraceEventSink::new("parts-session", 0));
    let result = runtime
        .complete(
            request("two parts"),
            ModelInvocationContext::new(ModelSession::default()).with_trace(
                CompletionTraceContext {
                    session_id: "parts-session".into(),
                    turn_id: "parts-turn".into(),
                    inference_id: "parts-inference".into(),
                },
                sink.clone(),
            ),
        )
        .await
        .unwrap();
    assert_eq!(result.content.as_deref(), Some("first second"));
    assert_eq!(result.presentation_items.len(), 1);
    let item = &result.presentation_items[0];
    assert_eq!(item.provider_item_id, "message-a");
    assert_eq!(item.output_index, Some(0));
    assert_eq!(
        item.parts
            .iter()
            .map(|part| (part.content_index, part.provider_part_id.as_deref()))
            .collect::<Vec<_>>(),
        vec![(1, Some("part-a")), (2, Some("part-b"))]
    );
    let started = sink
        .events()
        .into_iter()
        .filter_map(|event| match event.kind {
            TraceEventKind::TracePartStarted { item } if item.kind() == TracePartKind::Text => {
                Some(item.item_id().to_owned())
            }
            _ => None,
        })
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(started.len(), 2);
    fixture.finish().await.unwrap();
}

#[tokio::test]
async fn full_gui_stress_stream_reaches_model_public_api_without_loss() {
    let fixture = FixtureServer::start(vec![Step::prompt(
        Protocol::ResponsesHttp,
        "full paced stress wire",
        0,
        Reply::PacedSse {
            events: pl_provider_fixture::STRESS_EVENT_COUNT,
            tokens_per_second: pl_provider_fixture::STRESS_TOKENS_PER_SECOND,
        },
    )])
    .await
    .unwrap();
    let result = complete(
        &fixture,
        model("fixture-model", ModelTransportProfile::responses_http()),
        request("full paced stress wire"),
        ModelSession::default(),
    )
    .await;
    assert_eq!(result.accounting.usage.output_tokens, Some(20_000));
    assert_eq!(result.presentation_items.len(), 20_000);
    assert_eq!(
        result.presentation_items[19_999].provider_item_id,
        "stress-item-19999"
    );
    assert_eq!(result.presentation_items[19_999].output_index, Some(19_999));
    assert_eq!(
        result
            .content
            .as_deref()
            .unwrap_or_default()
            .matches("answer-")
            .count(),
        5_000
    );
    assert_eq!(
        result
            .reasoning_content
            .as_deref()
            .unwrap_or_default()
            .matches("thought-")
            .count(),
        5_000
    );
    let report = fixture.shutdown().await.unwrap();
    report.verify().unwrap();
    let stress = report.stress.unwrap();
    assert_eq!(stress.emitted_events, 20_000);
    assert!(stress.finished_unix_millis.is_some());
    assert!(stress.elapsed_millis >= 3_800);
}

#[tokio::test]
async fn responses_http_preserves_text_reasoning_and_reported_usage() {
    let mut events = responses_text("answer", "response-1", "gpt-fixture");
    events.insert(
        1,
        json!({"type":"response.reasoning_text.delta","item_id":"reason-1","delta":"considering"}),
    );
    events.insert(2, json!({"type":"response.reasoning_summary_text.delta","item_id":"reason-1","summary_index":0,"delta":"brief thought"}));
    let fixture = FixtureServer::start(vec![Step::prompt(
        Protocol::ResponsesHttp,
        "question",
        0,
        Reply::Sse(events),
    )])
    .await
    .unwrap();
    let model = model("gpt-fixture", ModelTransportProfile::responses_http());
    let result = complete(
        &fixture,
        model,
        request("question"),
        ModelSession::default(),
    )
    .await;
    assert_eq!(result.content.as_deref(), Some("answer"));
    assert_eq!(result.reasoning_content.as_deref(), Some("considering"));
    assert_eq!(result.response_id.as_deref(), Some("response-1"));
    assert_eq!(result.accounting.usage.input_tokens, Some(11));
    assert_eq!(result.accounting.usage.output_tokens, Some(5));
    assert_eq!(result.accounting.usage.cache_read_tokens, Some(2));
    assert_eq!(result.accounting.usage.reasoning_tokens, Some(1));
    let records = fixture.finish().await.unwrap();
    assert_eq!(records[0].body["model"], "gpt-fixture");
    assert_eq!(records[0].body["store"], false);
    assert_eq!(records[0].body["stream"], true);
}

#[tokio::test]
async fn websocket_keeps_session_connection_and_continuation_scoped_to_it() {
    let fixture = FixtureServer::start(vec![
        Step::prompt(
            Protocol::ResponsesWebSocket,
            "first",
            0,
            Reply::WebSocket(responses_text("one", "ws-1", "ws-fixture")),
        ),
        Step::prompt(
            Protocol::ResponsesWebSocket,
            "second",
            1,
            Reply::WebSocket(responses_text("two", "ws-2", "ws-fixture")),
        ),
    ])
    .await
    .unwrap();
    let session = ModelSession::default();
    let model = model("ws-fixture", ModelTransportProfile::responses_websocket());
    let runtime = ModelRuntime::new(
        ProviderEndpoint::compatible("fixture", fixture.base_url()),
        model,
    )
    .unwrap();
    let first = runtime
        .complete(
            request("first"),
            ModelInvocationContext::new(session.clone()),
        )
        .await
        .unwrap();
    assert_eq!(first.content.as_deref(), Some("one"));
    let second_input = CompletionRequest::builder()
        .messages(vec![user("first"), assistant("one"), user("second")])
        .build();
    let second = runtime
        .complete(second_input, ModelInvocationContext::new(session.clone()))
        .await
        .unwrap();
    assert_eq!(second.content.as_deref(), Some("two"));
    session.close().await.unwrap();
    let records = fixture.finish().await.unwrap();
    assert_eq!(records.len(), 2);
    assert_eq!(records[0].body["type"], "response.create");
    assert_eq!(records[1].body["type"], "response.create");
    assert_eq!(records[1].body["store"], false);
    assert_eq!(records[1].body["previous_response_id"], "ws-1");
    assert_eq!(records[1].body["input"].as_array().unwrap().len(), 1);
    assert_eq!(records[1].body["input"][0]["content"][0]["text"], "second");
}

#[tokio::test]
async fn chat_consumes_separate_usage_chunk_and_reasoning_content() {
    let fixture = FixtureServer::start(vec![Step::prompt(
        Protocol::Chat,
        "chat request",
        0,
        Reply::Sse(chat_events("chat answer", "chat-fixture")),
    )])
    .await
    .unwrap();
    let result = complete(
        &fixture,
        model(
            "chat-fixture",
            ModelTransportProfile::chat_completions_http(),
        ),
        request("chat request"),
        ModelSession::default(),
    )
    .await;
    assert_eq!(result.content.as_deref(), Some("chat answer"));
    assert_eq!(result.reasoning_content.as_deref(), Some("thinking "));
    assert_eq!(result.accounting.usage.total_tokens, Some(24));
    assert_eq!(result.accounting.usage.cache_read_tokens, Some(3));
    assert_eq!(result.accounting.usage.reasoning_tokens, Some(2));
    let records = fixture.finish().await.unwrap();
    assert_eq!(records[0].body["stream_options"]["include_usage"], true);
    assert_eq!(records[0].path, "/v1/chat/completions");
}

#[tokio::test]
async fn responses_function_call_has_stable_item_and_call_id() {
    let events = vec![
        json!({"type":"response.created","response":{"id":"tools-1","model":"tools-fixture"}}),
        json!({"type":"response.output_item.added","item":{"id":"item-7","type":"function_call","name":"lookup"}}),
        json!({"type":"response.function_call_arguments.delta","item_id":"item-7","call_id":"call-7","delta":"{\"term\":\"rust\"}"}),
        json!({"type":"response.output_item.done","item":{"id":"item-7","call_id":"call-7","type":"function_call","name":"lookup","arguments":"{\"term\":\"rust\"}"}}),
        json!({"type":"response.completed","response":{"id":"tools-1","model":"tools-fixture","usage":{"input_tokens":8,"output_tokens":4}}}),
    ];
    let fixture = FixtureServer::start(vec![Step::prompt(
        Protocol::ResponsesHttp,
        "find it",
        0,
        Reply::Sse(events),
    )])
    .await
    .unwrap();
    let call = complete(
        &fixture,
        model("tools-fixture", ModelTransportProfile::responses_http()),
        CompletionRequest::builder()
            .messages(vec![user("find it")])
            .tools(vec![ToolSpec::function(
                "lookup",
                "Look up term",
                json!({"type":"object","properties":{"term":{"type":"string"}}}),
            )])
            .build(),
        ModelSession::default(),
    )
    .await;
    assert_eq!(call.tool_calls.len(), 1);
    assert_eq!(call.tool_calls[0].id, "item-7");
    assert_eq!(call.tool_calls[0].call_id, "call-7");
    assert_eq!(call.tool_calls[0].name, "lookup");
    assert_eq!(
        call.tool_calls[0].history_record().arguments,
        json!("{\"term\":\"rust\"}")
    );
    let records = fixture.finish().await.unwrap();
    assert_eq!(records[0].body["tools"][0]["name"], "lookup");
}

#[tokio::test]
async fn responses_custom_call_preserves_raw_input_and_programmatic_caller() {
    let events = vec![
        json!({"type":"response.created","response":{"id":"custom-1"}}),
        json!({"type":"response.output_item.added","item":{"id":"custom-item","call_id":"custom-call","type":"custom_tool_call","name":"apply_patch"}}),
        json!({"type":"response.custom_tool_call_input.delta","item_id":"custom-item","call_id":"custom-call","delta":"*** Begin Patch"}),
        json!({"type":"response.output_item.done","item":{"id":"custom-item","call_id":"custom-call","type":"custom_tool_call","name":"apply_patch","input":"*** Begin Patch","caller":{"type":"program","caller_id":"program-1"}}}),
        json!({"type":"response.completed","response":{"id":"custom-1","usage":{"input_tokens":8,"output_tokens":4}}}),
    ];
    let fixture = FixtureServer::start(vec![Step::prompt(
        Protocol::ResponsesHttp,
        "patch it",
        0,
        Reply::Sse(events),
    )])
    .await
    .unwrap();
    let mut info = bundled("gpt-5.5");
    info.binding
        .set_transport(ModelTransportProfile::responses_http());
    let runtime =
        ModelRuntime::new(ProviderEndpoint::openai(Some(fixture.base_url())), info).unwrap();
    let result = runtime
        .complete(
            CompletionRequest::builder()
                .messages(vec![user("patch it")])
                .tools(vec![ToolSpec::custom_grammar(
                    "apply_patch",
                    "Apply patch",
                    "lark",
                    "start: WORD",
                )])
                .build(),
            ModelInvocationContext::default(),
        )
        .await
        .unwrap();
    assert_eq!(result.tool_calls.len(), 1);
    let call = &result.tool_calls[0];
    assert_eq!(call.id, "custom-item");
    assert_eq!(call.call_id, "custom-call");
    assert_eq!(call.kind(), ToolCallKind::Custom);
    assert_eq!(call.payload_text(), "*** Begin Patch");
    assert_eq!(
        call.caller,
        Some(ToolCallCaller::Program {
            caller_id: "program-1".into()
        })
    );
    let records = fixture.finish().await.unwrap();
    assert_eq!(records[0].body["tools"][0]["type"], "custom");
}

#[tokio::test]
async fn native_openai_client_applies_typed_cache_options_to_the_shared_transport() {
    let fixture = FixtureServer::start(vec![Step::prompt(
        Protocol::ResponsesHttp,
        "native cache",
        0,
        Reply::Sse(responses_text("cached", "native-1", "gpt-5.5")),
    )])
    .await
    .unwrap();
    let mut info = bundled("gpt-5.5");
    info.binding
        .set_transport(ModelTransportProfile::responses_http());
    let runtime =
        ModelRuntime::new(ProviderEndpoint::openai(Some(fixture.base_url())), info).unwrap();
    let ProviderClient::OpenAi(client) = runtime.provider() else {
        panic!("OpenAI endpoint must expose its typed client");
    };
    let result = client
        .complete(
            OpenAiCompletion {
                request: request("native cache"),
                options: OpenAiCompletionOptions {
                    prompt_cache_options: Some(PromptCacheOptions {
                        mode: CacheMode::Explicit,
                        ttl: Some("24h".into()),
                    }),
                },
            },
            ModelInvocationContext::default(),
        )
        .await
        .unwrap();
    assert_eq!(result.content.as_deref(), Some("cached"));
    assert_eq!(result.accounting.usage.cache_read_tokens, Some(2));
    let records = fixture.finish().await.unwrap();
    assert_eq!(records[0].body["prompt_cache_options"]["mode"], "explicit");
    assert_eq!(records[0].body["prompt_cache_options"]["ttl"], "24h");
}

#[tokio::test]
async fn chat_tool_result_replays_the_provider_item_id_on_the_next_step() {
    let tool_events = vec![
        json!({"id":"chat-tool-1","choices":[{"delta":{"tool_calls":[{"index":0,"id":"chat-call-1","type":"function","function":{"name":"lookup","arguments":"{\"term\":\"rust\"}"}}]},"finish_reason":null}]}),
        json!({"id":"chat-tool-1","choices":[{"delta":{},"finish_reason":"tool_calls"}]}),
        json!({"id":"chat-tool-1","choices":[],"usage":{"prompt_tokens":9,"completion_tokens":3,"total_tokens":12}}),
    ];
    let fixture = FixtureServer::start(vec![
        Step::prompt(Protocol::Chat, "lookup", 0, Reply::Sse(tool_events)),
        Step::prompt(
            Protocol::Chat,
            "lookup",
            1,
            Reply::Sse(chat_events("found", "chat-tools")),
        ),
    ])
    .await
    .unwrap();
    let runtime = ModelRuntime::new(
        ProviderEndpoint::compatible("fixture", fixture.base_url()),
        model("chat-tools", ModelTransportProfile::chat_completions_http()),
    )
    .unwrap();
    let tools = vec![ToolSpec::function(
        "lookup",
        "Look up term",
        json!({"type":"object"}),
    )];
    let first = runtime
        .complete(
            CompletionRequest::builder()
                .messages(vec![user("lookup")])
                .tools(tools.clone())
                .build(),
            ModelInvocationContext::default(),
        )
        .await
        .unwrap();
    assert_eq!(first.tool_calls.len(), 1);
    assert_eq!(first.tool_calls[0].id, "chat-call-1");
    assert_eq!(first.tool_calls[0].call_id, "chat-call-1");
    assert_eq!(first.accounting.usage.total_tokens, Some(12));
    let tool_result = Message {
        role: MessageRole::Tool,
        content: MessageContent::text("entry found"),
        tool_result: Some(ToolResultRecord {
            item_id: first.tool_calls[0].id.clone(),
            call_id: first.tool_calls[0].call_id.clone(),
            name: "lookup".into(),
            kind: ToolCallKind::Function,
        }),
        ..user("")
    };
    let previous_call = Message {
        role: MessageRole::Assistant,
        content: MessageContent::text(""),
        tool_calls: Some(vec![first.tool_calls[0].history_record()]),
        ..user("")
    };
    let second = runtime
        .complete(
            CompletionRequest::builder()
                .messages(vec![user("lookup"), previous_call, tool_result])
                .tools(tools)
                .build(),
            ModelInvocationContext::default(),
        )
        .await
        .unwrap();
    assert_eq!(second.content.as_deref(), Some("found"));
    let records = fixture.finish().await.unwrap();
    assert_eq!(
        records[1].body["messages"][1]["tool_calls"][0]["id"],
        "chat-call-1"
    );
    assert_eq!(
        records[1].body["messages"][2]["tool_call_id"],
        "chat-call-1"
    );
    assert_eq!(records[1].body["messages"][2]["content"], "entry found");
}

#[tokio::test]
async fn deepseek_and_zhipu_use_their_declared_protocol_and_effort_wire() {
    let deepseek = FixtureServer::start(vec![Step::prompt(
        Protocol::ResponsesHttp,
        "deep prompt",
        0,
        Reply::Sse(responses_text("deep answer", "deep-1", "deepseek-v4-pro")),
    )])
    .await
    .unwrap();
    let mut endpoint = ProviderEndpoint::deepseek(Some(deepseek.base_url()));
    endpoint.bearer_token = Some("local-only".into());
    let runtime = ModelRuntime::new(endpoint, bundled("deepseek-v4-pro")).unwrap();
    let reply = runtime
        .complete(
            CompletionRequest::builder()
                .messages(vec![user("deep prompt")])
                .reasoning(Some(ReasoningConfig {
                    effort: Some("high".into()),
                    summary: None,
                }))
                .build(),
            ModelInvocationContext::default(),
        )
        .await
        .unwrap();
    assert_eq!(reply.content.as_deref(), Some("deep answer"));
    let deep_records = deepseek.finish().await.unwrap();
    assert_eq!(deep_records[0].path, "/v1/responses");
    assert_eq!(deep_records[0].body["thinking"]["type"], "enabled");
    assert_eq!(deep_records[0].body["reasoning_effort"], "high");

    let zhipu = FixtureServer::start(vec![Step::prompt(
        Protocol::Chat,
        "zhipu prompt",
        0,
        Reply::Sse(chat_events("zhipu answer", "glm-5.3")),
    )])
    .await
    .unwrap();
    let mut endpoint = ProviderEndpoint::zhipu(Some(zhipu.base_url()));
    endpoint.bearer_token = Some("local-only".into());
    let runtime = ModelRuntime::new(endpoint, bundled("glm-5.3")).unwrap();
    let reply = runtime
        .complete(
            CompletionRequest::builder()
                .messages(vec![user("zhipu prompt")])
                .reasoning(Some(ReasoningConfig {
                    effort: Some("high".into()),
                    summary: Some(ReasoningSummary::Disabled),
                }))
                .build(),
            ModelInvocationContext::default(),
        )
        .await
        .unwrap();
    assert_eq!(reply.content.as_deref(), Some("zhipu answer"));
    let zhipu_records = zhipu.finish().await.unwrap();
    assert_eq!(zhipu_records[0].path, "/v1/chat/completions");
    assert_eq!(zhipu_records[0].body["thinking"]["type"], "enabled");
    assert_eq!(zhipu_records[0].body["reasoning_effort"], "high");
    assert_eq!(zhipu_records[0].body["tool_stream"], true);
}

#[tokio::test]
async fn cancellation_unwinds_a_stalled_sse_without_fabricating_usage() {
    let fixture = FixtureServer::start(vec![Step::prompt(
        Protocol::ResponsesHttp,
        "cancel prompt",
        0,
        Reply::HangingSse(vec![
            json!({"type":"response.created","response":{"id":"cancel-1"}}),
            json!({"type":"response.output_text.delta","item_id":"message-1","delta":"partial"}),
        ]),
    )])
    .await
    .unwrap();
    let runtime = ModelRuntime::new(
        ProviderEndpoint::compatible("fixture", fixture.base_url()),
        model("cancel-fixture", ModelTransportProfile::responses_http()),
    )
    .unwrap();
    let token = CancellationToken::new();
    let invocation = ModelInvocationContext::default().with_cancellation(Some(token.clone()));
    let task =
        tokio::spawn(async move { runtime.complete(request("cancel prompt"), invocation).await });
    tokio::time::timeout(std::time::Duration::from_secs(2), async {
        while fixture.recorded().is_empty() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    token.cancel();
    let failure = task.await.unwrap().unwrap_err();
    assert!(failure.is_cancelled());
    assert_eq!(failure.accounting.usage.input_tokens, None);
    fixture.finish().await.unwrap();
}

#[tokio::test]
async fn failed_stream_retains_every_received_item_beyond_the_live_window() {
    let events = std::iter::once(json!({
        "type": "response.created",
        "response": { "id": "partial-response", "model": "fixture-model" },
    }))
    .chain((0..150).map(|index| {
        json!({
            "type": "response.output_item.done",
            "output_index": index,
            "item": {
                "id": format!("partial-item-{index}"),
                "type": "message",
                "role": "assistant",
                "phase": "final_answer",
                "content": [{"type": "output_text", "text": format!("part-{index}")}],
            },
        })
    }))
    .collect();
    let fixture = FixtureServer::start(vec![Step::prompt(
        Protocol::ResponsesHttp,
        "incomplete items",
        0,
        Reply::Sse(events),
    )])
    .await
    .unwrap();
    let runtime = ModelRuntime::new(
        ProviderEndpoint::compatible("fixture", fixture.base_url()),
        model("fixture-model", ModelTransportProfile::responses_http()),
    )
    .unwrap();
    let failure = runtime
        .complete(
            request("incomplete items"),
            ModelInvocationContext::new(ModelSession::default()),
        )
        .await
        .unwrap_err();
    assert_eq!(failure.presentation_items.len(), 150);
    for (index, item) in failure.presentation_items.iter().enumerate() {
        assert_eq!(item.provider_item_id, format!("partial-item-{index}"));
        assert_eq!(item.parts[0].text, format!("part-{index}"));
    }
    assert_eq!(fixture.finish().await.unwrap().len(), 1);
}

#[tokio::test]
async fn provider_error_is_permanent_and_unknown_prompt_is_rejected() {
    let fixture = FixtureServer::start(vec![Step::prompt(
        Protocol::Chat,
        "bad request",
        0,
        Reply::HttpError {
            status: 400,
            code: "invalid_request_error".into(),
            message: "bad field".into(),
        },
    )])
    .await
    .unwrap();
    let runtime = ModelRuntime::new(
        ProviderEndpoint::compatible("fixture", fixture.base_url()),
        model(
            "error-fixture",
            ModelTransportProfile::chat_completions_http(),
        ),
    )
    .unwrap();
    let failure = runtime
        .complete(request("bad request"), ModelInvocationContext::default())
        .await
        .unwrap_err();
    assert!(!failure.is_cancelled());
    assert!(failure.to_string().contains("bad field"), "{failure}");
    let records = fixture.finish().await.unwrap();
    assert_eq!(records.len(), 1);

    let fixture = FixtureServer::start(vec![Step::prompt(
        Protocol::Chat,
        "expected",
        0,
        Reply::Sse(chat_events("never", "error-fixture")),
    )])
    .await
    .unwrap();
    let runtime = ModelRuntime::new(
        ProviderEndpoint::compatible("fixture", fixture.base_url()),
        model(
            "error-fixture",
            ModelTransportProfile::chat_completions_http(),
        ),
    )
    .unwrap();
    assert!(
        runtime
            .complete(request("unexpected"), ModelInvocationContext::default())
            .await
            .is_err()
    );
    let report = fixture.shutdown().await.unwrap();
    assert_eq!(report.requests.len(), 1);
    assert!(!report.requests[0].accepted);
    assert!(report.verify().is_err());
}

#[tokio::test]
async fn openai_hosted_search_and_native_custom_programmatic_tools_use_responses_wire() {
    let search = json!({"type":"web_search_call","id":"search-1","status":"completed","action":{"type":"search","query":"rust release"},"results":[]});
    let mut events = responses_text("result", "search-1", "gpt-5.5");
    events.insert(1, json!({"type":"response.output_item.added","item":{"type":"web_search_call","id":"search-1","action":{"type":"search","query":"rust release"}}}));
    events.insert(
        2,
        json!({"type":"response.output_item.done","item":search.clone()}),
    );
    let fixture = FixtureServer::start(vec![Step::prompt(
        Protocol::ResponsesHttp,
        "search it",
        0,
        Reply::Sse(events),
    )])
    .await
    .unwrap();
    let mut endpoint = ProviderEndpoint::openai(Some(fixture.base_url()));
    endpoint.service_capabilities.web_search.hosted_responses = true;
    endpoint
        .service_capabilities
        .responses_tools
        .programmatic_tool_calling = true;
    let mut info = bundled("gpt-5.5");
    info.binding
        .set_transport(ModelTransportProfile::responses_http());
    let runtime = ModelRuntime::new(endpoint, info).unwrap();
    let tools = vec![
        ToolSpec::WebSearch {
            options: HostedWebSearchOptions::OpenAi {
                external_web_access: false,
                indexed_web_access: None,
                filters: None,
                user_location: None,
                search_context_size: None,
                search_content_types: None,
            },
        },
        ToolSpec::custom_grammar("apply_patch", "Patch source", "lark", "start: WORD"),
        ToolSpec::ProgrammaticToolCalling,
        programmatic_tool_declaration(ToolSpec::function(
            "lookup",
            "Look up",
            json!({"type":"object"}),
        )),
    ];
    let result = runtime
        .complete(
            CompletionRequest::builder()
                .messages(vec![user("search it")])
                .tools(tools)
                .build(),
            ModelInvocationContext::default(),
        )
        .await
        .unwrap();
    assert_eq!(result.content.as_deref(), Some("result"));
    assert_eq!(result.responses_context_items.len(), 1);
    assert_eq!(result.responses_context_items[0].value, search);
    let records = fixture.finish().await.unwrap();
    let types: Vec<_> = records[0].body["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|tool| tool["type"].as_str().unwrap())
        .collect();
    assert_eq!(
        types,
        vec![
            "custom",
            "function",
            "programmatic_tool_calling",
            "web_search"
        ]
    );
    assert_eq!(records[0].body["tools"][0]["format"]["type"], "grammar");
    assert_eq!(
        records[0].body["tools"][1]["allowed_callers"],
        json!(["direct", "programmatic"])
    );
    assert_eq!(records[0].body["tools"][3]["external_web_access"], false);
}

#[tokio::test]
async fn deepseek_hosted_search_has_its_own_minimal_wire_dialect() {
    let fixture = FixtureServer::start(vec![Step::prompt(
        Protocol::ResponsesHttp,
        "native search",
        0,
        Reply::Sse(responses_text(
            "native result",
            "search-2",
            "deepseek-v4-pro",
        )),
    )])
    .await
    .unwrap();
    let endpoint = ProviderEndpoint::deepseek(Some(fixture.base_url()));
    let runtime = ModelRuntime::new(endpoint, bundled("deepseek-v4-pro")).unwrap();
    let result = runtime
        .complete(
            CompletionRequest::builder()
                .messages(vec![user("native search")])
                .tools(vec![ToolSpec::WebSearch {
                    options: HostedWebSearchOptions::DeepSeek,
                }])
                .build(),
            ModelInvocationContext::default(),
        )
        .await
        .unwrap();
    assert_eq!(result.content.as_deref(), Some("native result"));
    let records = fixture.finish().await.unwrap();
    assert_eq!(records[0].body["tools"], json!([{"type":"web_search"}]));
}

#[tokio::test]
async fn coding_plan_uses_responses_http_and_model_effort_not_chat_thinking() {
    let preset = builtin_provider_catalog()
        .presets
        .into_iter()
        .find(|item| item.id.as_str() == "zhipu-coding-plan")
        .unwrap();
    let model = preset
        .provider
        .effective_models()
        .unwrap()
        .into_iter()
        .find(|model| model.slug == "glm-5.3")
        .unwrap();
    let fixture = FixtureServer::start(vec![Step::prompt(
        Protocol::ResponsesHttp,
        "coding prompt",
        0,
        Reply::Sse(responses_text("code", "coding-1", "glm-5.3")),
    )])
    .await
    .unwrap();
    let runtime = ModelRuntime::new(
        ProviderEndpoint::zhipu_coding_plan(Some(fixture.base_url())),
        model,
    )
    .unwrap();
    let result = runtime
        .complete(
            CompletionRequest::builder()
                .messages(vec![user("coding prompt")])
                .reasoning(Some(ReasoningConfig {
                    effort: Some("high".into()),
                    summary: None,
                }))
                .build(),
            ModelInvocationContext::default(),
        )
        .await
        .unwrap();
    assert_eq!(result.content.as_deref(), Some("code"));
    let records = fixture.finish().await.unwrap();
    assert_eq!(records[0].body["reasoning"]["effort"], "high");
    assert!(records[0].body.get("thinking").is_none());
}

#[tokio::test]
async fn openai_image_is_sent_as_replayable_input_image_and_cache_usage_is_billed() {
    let bytes = tiny_png();
    let fixture = FixtureServer::start(vec![Step::prompt(
        Protocol::ResponsesHttp,
        "describe",
        0,
        Reply::Sse(responses_text("a pixel", "image-1", "gpt-5.5")),
    )])
    .await
    .unwrap();
    let mut endpoint = ProviderEndpoint::openai(Some(fixture.base_url()));
    endpoint.service_capabilities.prompt_cache.dialect = PromptCacheDialect::OpenAiPromptCacheKey;
    let mut info = bundled("gpt-5.5");
    info.binding
        .set_transport(ModelTransportProfile::responses_http());
    let runtime = ModelRuntime::new(endpoint, info).unwrap();
    let result = runtime
        .complete(
            image_request("describe", bytes.clone()),
            ModelInvocationContext::default().with_prompt_cache_key(Some("stable-key".into())),
        )
        .await
        .unwrap();
    assert_eq!(result.content.as_deref(), Some("a pixel"));
    assert_eq!(result.accounting.usage.cache_read_tokens, Some(2));
    assert!(
        matches!(result.accounting.pricing, PricingOutcome::Estimated { .. }),
        "{:?}",
        result.accounting.pricing
    );
    let records = fixture.finish().await.unwrap();
    assert_eq!(records[0].body["prompt_cache_key"], "stable-key");
    let image_url = records[0].body["input"][0]["content"][1]["image_url"]
        .as_str()
        .unwrap();
    let encoded = image_url.strip_prefix("data:image/png;base64,").unwrap();
    use base64::Engine;
    assert_eq!(
        base64::engine::general_purpose::STANDARD
            .decode(encoded)
            .unwrap(),
        bytes
    );
}

#[tokio::test]
async fn deepseek_file_upload_precedes_responses_and_sends_only_file_reference() {
    let bytes = tiny_png();
    let fixture = FixtureServer::start(vec![
        Step::exact(Protocol::Files, json!({
            "purpose":"user_data", "expires_after[anchor]":"created_at", "expires_after[seconds]":"86400",
            "file":{"filename":"tiny.png","mime_type":"image/png","sha256":hex::encode(Sha256::digest(&bytes))}
        }), Reply::Json(json!({"id":"file-fixture"}))),
        Step::prompt(Protocol::ResponsesHttp, "inspect", 1,
            Reply::Sse(responses_text("uploaded", "deep-file", "deepseek-flash"))),
    ]).await.unwrap();
    let mut endpoint = ProviderEndpoint::deepseek(Some(fixture.base_url()));
    endpoint.service_capabilities.files = FileUploadCapability::DeepSeek;
    let runtime = ModelRuntime::new(endpoint, bundled("deepseek-flash")).unwrap();
    let result = runtime
        .complete(
            image_request("inspect", bytes),
            ModelInvocationContext::default(),
        )
        .await
        .unwrap();
    assert_eq!(result.content.as_deref(), Some("uploaded"));
    let records = fixture.finish().await.unwrap();
    assert_eq!(records[0].path, "/v1/files");
    assert_eq!(
        records[1].body["input"][0]["content"][1]["file_id"],
        "file-fixture"
    );
}

#[tokio::test]
async fn native_compaction_returns_checkpoint_and_retains_reported_usage() {
    let checkpoint = json!({"type":"compaction","encrypted_content":"fixture-checkpoint"});
    let events = vec![
        json!({"type":"response.created","response":{"id":"compact-1"}}),
        json!({"type":"response.output_item.done","item":checkpoint}),
        json!({"type":"response.completed","response":{"id":"compact-1","usage":{"input_tokens":20,"output_tokens":3}}}),
    ];
    let fixture = FixtureServer::start(vec![Step::prompt(
        Protocol::ResponsesHttp,
        "compact",
        0,
        Reply::Sse(events),
    )])
    .await
    .unwrap();
    let mut endpoint = ProviderEndpoint::openai(Some(fixture.base_url()));
    endpoint.service_capabilities.remote_compaction = true;
    let mut info = bundled("gpt-5.5");
    info.binding
        .set_transport(ModelTransportProfile::responses_http());
    let runtime = ModelRuntime::new(endpoint, info).unwrap();
    let compacted = runtime
        .compaction()
        .unwrap()
        .checkpoint(request("compact"), ModelInvocationContext::default())
        .await
        .unwrap();
    assert!(
        matches!(compacted.item, ModelContextItem::Compaction { encrypted_content } if encrypted_content == "fixture-checkpoint")
    );
    assert_eq!(compacted.accounting.usage.input_tokens, Some(20));
    let records = fixture.finish().await.unwrap();
    assert_eq!(
        records[0].body["input"].as_array().unwrap().last().unwrap()["type"],
        "compaction_trigger"
    );
}

#[tokio::test]
async fn transient_provider_error_retries_one_logical_request_without_double_counting() {
    let fixture = FixtureServer::start(vec![
        Step::prompt(
            Protocol::Chat,
            "retry",
            0,
            Reply::HttpError {
                status: 429,
                code: "rate_limit_exceeded".into(),
                message: "try again".into(),
            },
        ),
        Step::prompt(
            Protocol::Chat,
            "retry",
            1,
            Reply::Sse(chat_events("recovered", "retry-model")),
        ),
    ])
    .await
    .unwrap();
    let result = complete(
        &fixture,
        model(
            "retry-model",
            ModelTransportProfile::chat_completions_http(),
        ),
        request("retry"),
        ModelSession::default(),
    )
    .await;
    assert_eq!(result.content.as_deref(), Some("recovered"));
    assert_eq!(result.accounting.usage.total_tokens, Some(24));
    assert_eq!(fixture.finish().await.unwrap().len(), 2);
}

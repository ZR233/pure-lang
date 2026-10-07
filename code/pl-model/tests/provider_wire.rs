use pl_core::{
    context::{ContextContent, ContextSource},
    model::ModelFactory,
    thread::{AttemptOutcome, ModelStepLimit, ThreadHandle, TurnInput, TurnOutcome, TurnState},
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

#[test]
fn retained_media_is_validated_before_model_admission() {
    use pl_core::context::ResourceReference;
    use pl_model::model::{
        MediaRepresentation, MediaWireFormat, ModelInputCapability, ModelInputSource,
        ModelMediaInputProfile, ModelModality,
    };
    use pl_model::runtime::{attachment_content, validate_attachment_content};
    let content = attachment_content(
        ResourceReference::new(
            "retained-image".into(),
            format!("sha256:{}", "0".repeat(64)),
            16,
            "image/png".into(),
        )
        .unwrap(),
        AttachmentModality::Image,
    )
    .unwrap();
    let mut model = ModelInfo::compatible("media-admission");
    assert!(validate_attachment_content([&content], &model).is_err());
    let mut capability =
        ModelInputCapability::media(ModelModality::Image, vec![ModelInputSource::Local]);
    capability.limits.max_count = Some(1);
    model.capabilities.input.push(capability);
    model.binding.request.media.push(ModelMediaInputProfile {
        modality: ModelModality::Image,
        wire: MediaWireFormat::ChatImageUrl,
        first_send: vec![MediaRepresentation::DataUrl],
        replay: vec![MediaRepresentation::DataUrl],
    });
    validate_attachment_content([&content], &model).unwrap();
    // One historical reference plus the new input counts twice even with shared bytes.
    assert!(validate_attachment_content([&content, &content], &model).is_err());
    model.binding.request.media[0].replay = vec![MediaRepresentation::RemoteUrl];
    assert!(validate_attachment_content([&content], &model).is_err());
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
        .unwrap()
        .into_iter()
        .find(|model| model.slug == slug)
        .unwrap()
}

#[test]
fn deepseek_catalog_does_not_advertise_ignored_responses_web_search() {
    let endpoint = ProviderEndpoint::deepseek(None);
    assert!(!endpoint.service_capabilities.web_search.hosted_responses);

    for slug in ["deepseek-flash", "deepseek-v4-pro"] {
        assert!(!bundled(slug).capabilities.web_search, "{slug}");
    }
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
    let mut events = responses_text("cross-crate answer", "thread-response", "core-fixture");
    events.last_mut().unwrap()["response"]["usage"]["total_tokens"] = json!(16);
    let fixture = FixtureServer::start(vec![Step::prompt(
        Protocol::ResponsesHttp,
        "cross-crate request",
        0,
        Reply::Sse(events),
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
    assert!(effects.iter().any(|effect| {
        matches!(
            effect.attempt.as_ref().map(|attempt| &attempt.outcome),
            Some(AttemptOutcome::Committed(output)) if output.usage.total_tokens == Some(16)
        )
    }));
    thread.close().await.unwrap();
    let records = fixture.finish().await.unwrap();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].path, "/v1/responses");
}

#[tokio::test]
async fn thread_replays_all_assistant_output_without_losing_progress_or_wire_identity() {
    for (protocol, profile) in [
        (
            Protocol::ResponsesHttp,
            ModelTransportProfile::responses_http(),
        ),
        (
            Protocol::ResponsesWebSocket,
            ModelTransportProfile::responses_websocket(),
        ),
        (
            Protocol::Chat,
            ModelTransportProfile::chat_completions_http(),
        ),
    ] {
        let output = vec![
            json!({"id":"progress","type":"message","role":"assistant","phase":"commentary","content":[{"type":"output_text","text":"Already inspected. ","annotations":[]}],"status":"completed"}),
            json!({"id":"reason","type":"reasoning","summary":[],"encrypted_content":"frozen-reasoning"}),
            json!({"id":"answer","type":"message","role":"assistant","phase":"final_answer","content":[{"type":"output_text","text":"Continue next.","annotations":[]}],"status":"completed"}),
        ];
        let raw_chat = "<commentary>Already inspected. </commentary><final>Continue next.</final>";
        let first = if protocol == Protocol::Chat {
            chat_events(raw_chat, "replay-fixture")
        } else {
            let mut events = vec![json!({"type":"response.created","response":{"id":"replay-1"}})];
            for (index, item) in output.iter().enumerate() {
                events.push(
                    json!({"type":"response.output_item.added","output_index":index,"item":item}),
                );
                events.push(
                    json!({"type":"response.output_item.done","output_index":index,"item":item}),
                );
            }
            events.push(json!({"type":"response.completed","response":{"id":"replay-1","output":output,"usage":{"input_tokens":10,"output_tokens":3}}}));
            events
        };
        let wrap = |events| {
            if protocol == Protocol::ResponsesWebSocket {
                Reply::WebSocket(events)
            } else {
                Reply::Sse(events)
            }
        };
        let second = if protocol == Protocol::Chat {
            chat_events("done", "replay-fixture")
        } else {
            responses_text("done", "replay-2", "replay-fixture")
        };
        let fixture = FixtureServer::start(vec![
            Step::prompt(protocol, "inspect", 0, wrap(first)),
            Step::prompt(protocol, "continue", 1, wrap(second)),
            Step::prompt(
                protocol,
                "resume",
                2,
                wrap(if protocol == Protocol::Chat {
                    chat_events("resumed", "replay-fixture")
                } else {
                    responses_text("resumed", "replay-3", "replay-fixture")
                }),
            ),
            Step::prompt(
                protocol,
                "resume again",
                3,
                wrap(if protocol == Protocol::Chat {
                    chat_events("resumed again", "replay-fixture")
                } else {
                    responses_text("resumed again", "replay-4", "replay-fixture")
                }),
            ),
        ])
        .await
        .unwrap();
        let mut replay_model = model("replay-fixture", profile);
        replay_model.capabilities.interleaved = Some(pl_model::model::ReasoningInterleaved {
            field: pl_model::model::ReasoningInterleavedField::ReasoningContent,
        });
        let runtime = ModelRuntime::new(
            ProviderEndpoint::compatible("fixture", fixture.base_url()),
            replay_model,
        )
        .unwrap();
        let factory = ModelFactory::new(pl_model::runtime::ThreadModel::new(runtime, None));
        let session = factory.open_session().await.unwrap();
        let thread = ThreadHandle::start("replay-thread".into(), session).unwrap();
        for (index, prompt) in ["inspect", "continue"].iter().enumerate() {
            thread
                .run_turn(TurnInput {
                    turn_id: format!("replay-turn-{index}"),
                    attempt_prefix: format!("replay-attempt-{index}"),
                    content: vec![ContextContent::Text {
                        text: Arc::from(*prompt),
                    }],
                    max_model_steps: ModelStepLimit::Limited(1.try_into().unwrap()),
                    cancellation: Default::default(),
                })
                .await
                .unwrap();
        }
        let before = thread.snapshot().context;
        let checkpoint = thread
            .checkpoint(thread.snapshot().commit_sequence)
            .unwrap();
        let saved = serde_json::to_vec(&checkpoint).unwrap();
        thread.close().await.unwrap();
        let restored = ThreadHandle::resume(
            "replay-thread".into(),
            factory.open_session().await.unwrap(),
            Some(serde_json::from_slice(&saved).unwrap()),
        )
        .unwrap();
        assert_eq!(restored.snapshot().context, before);
        for (index, prompt) in ["resume", "resume again"].iter().enumerate() {
            restored
                .run_turn(TurnInput {
                    turn_id: format!("restored-turn-{index}"),
                    attempt_prefix: format!("restored-attempt-{index}"),
                    content: vec![ContextContent::Text {
                        text: Arc::from(*prompt),
                    }],
                    max_model_steps: ModelStepLimit::Limited(1.try_into().unwrap()),
                    cancellation: Default::default(),
                })
                .await
                .unwrap();
        }
        restored.close().await.unwrap();
        let requests = fixture.finish().await.unwrap();
        assert_eq!(requests.len(), 4);
        assert!(requests[2].body.get("previous_response_id").is_none());
        if protocol == Protocol::Chat {
            let previous = requests[1].body["messages"].as_array().unwrap();
            assert_eq!(
                &requests[2].body["messages"].as_array().unwrap()[..previous.len()],
                previous
            );
        } else {
            assert_eq!(requests[2].body["input"].as_array().unwrap()[1..4], output);
            if protocol == Protocol::ResponsesWebSocket {
                assert_eq!(requests[3].body["previous_response_id"], "replay-3");
                assert_eq!(requests[3].body["input"].as_array().unwrap().len(), 1);
            } else {
                let previous = requests[1].body["input"].as_array().unwrap();
                assert_eq!(
                    &requests[2].body["input"].as_array().unwrap()[..previous.len()],
                    previous
                );
            }
        }
        if protocol == Protocol::Chat {
            assert_eq!(
                requests[1].body["messages"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|message| message["role"] == "assistant")
                    .unwrap()["content"],
                raw_chat
            );
            assert_eq!(
                requests[1].body["messages"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|message| message["role"] == "assistant")
                    .unwrap()["reasoning_content"],
                "thinking "
            );
        } else if protocol == Protocol::ResponsesWebSocket {
            assert_eq!(requests[1].body["previous_response_id"], "replay-1");
            assert_eq!(requests[1].body["input"].as_array().unwrap().len(), 1);
        } else {
            assert_eq!(requests[1].body["input"].as_array().unwrap()[1..4], output);
        }
    }
}

/// 跨 provider 历史回放：来源 provider 的原生 reasoning 由该账户/部署加密，目标 provider 无法
/// 解密（严格兼容网关以 `invalid_encrypted_content` 失败）。按帧记录来源隔离身份，出站请求必须
/// 只移除这类 reasoning，保留 assistant 文本、工具身份与工具输出，且不重放工具副作用；同一
/// provider 的 reasoning 仍按原样回放（见
/// `thread_replays_all_assistant_output_without_losing_progress_or_wire_identity`）。
#[tokio::test]
async fn cross_provider_replay_omits_unreadable_reasoning_but_keeps_assistant_text() {
    let output = vec![
        json!({"id":"source-reason","type":"reasoning","summary":[],"encrypted_content":"source-encrypted","status":"completed"}),
        json!({"id":"source-message","type":"message","role":"assistant","phase":"final_answer","status":"completed","content":[{"type":"output_text","text":"Provider A answer.","annotations":[]}]}),
    ];
    let mut source_events =
        vec![json!({"type":"response.created","response":{"id":"source-response"}})];
    for (index, item) in output.iter().enumerate() {
        source_events
            .push(json!({"type":"response.output_item.added","output_index":index,"item":item}));
        source_events
            .push(json!({"type":"response.output_item.done","output_index":index,"item":item}));
    }
    source_events.push(json!({"type":"response.completed","response":{"id":"source-response","output":output,"usage":{"input_tokens":5,"output_tokens":2}}}));
    let source = FixtureServer::start(vec![Step::prompt(
        Protocol::ResponsesHttp,
        "first",
        0,
        Reply::Sse(source_events),
    )])
    .await
    .unwrap();
    let source_runtime = ModelRuntime::new(
        ProviderEndpoint::compatible("provider-a", source.base_url()),
        model("provider-a", ModelTransportProfile::responses_http()),
    )
    .unwrap();
    let source_factory =
        ModelFactory::new(pl_model::runtime::ThreadModel::new(source_runtime, None));
    let thread = ThreadHandle::start(
        "cross-provider-thread".into(),
        source_factory.open_session().await.unwrap(),
    )
    .unwrap();
    thread
        .run_turn(TurnInput {
            turn_id: "cross-turn-0".into(),
            attempt_prefix: "cross-attempt-0".into(),
            content: vec![ContextContent::Text {
                text: Arc::from("first"),
            }],
            max_model_steps: ModelStepLimit::Limited(1.try_into().unwrap()),
            cancellation: Default::default(),
        })
        .await
        .unwrap();
    let checkpoint = thread
        .checkpoint(thread.snapshot().commit_sequence)
        .unwrap();
    let saved = serde_json::to_vec(&checkpoint).unwrap();
    thread.close().await.unwrap();

    let target = FixtureServer::start(vec![Step::prompt(
        Protocol::ResponsesHttp,
        "second",
        0,
        Reply::Sse(responses_text("done", "target-response", "provider-b")),
    )])
    .await
    .unwrap();
    let target_runtime = ModelRuntime::new(
        ProviderEndpoint::compatible("provider-b", target.base_url()),
        model("provider-b", ModelTransportProfile::responses_http()),
    )
    .unwrap();
    let target_factory =
        ModelFactory::new(pl_model::runtime::ThreadModel::new(target_runtime, None));
    let restored = ThreadHandle::resume(
        "cross-provider-thread".into(),
        target_factory.open_session().await.unwrap(),
        Some(serde_json::from_slice(&saved).unwrap()),
    )
    .unwrap();
    restored
        .run_turn(TurnInput {
            turn_id: "cross-turn-1".into(),
            attempt_prefix: "cross-attempt-1".into(),
            content: vec![ContextContent::Text {
                text: Arc::from("second"),
            }],
            max_model_steps: ModelStepLimit::Limited(1.try_into().unwrap()),
            cancellation: Default::default(),
        })
        .await
        .unwrap();
    restored.close().await.unwrap();

    let requests = target.finish().await.unwrap();
    let input = requests[0].body["input"].as_array().unwrap();
    assert!(
        !input.iter().any(|item| item["type"] == "reasoning"),
        "cross-provider reasoning must not be replayed to a provider that cannot decrypt it"
    );
    let assistant = input
        .iter()
        .find(|item| item["type"] == "message" && item["role"] == "assistant")
        .expect("assistant text must be replayed");
    assert_eq!(assistant["content"][0]["text"], "Provider A answer.");
}

/// 同一账户不能代替目标模型的原生材料适用性声明；切换仅投影历史，不丢可见答复。
#[tokio::test]
async fn same_account_model_switch_requires_explicit_native_reasoning_compatibility() {
    let output = vec![
        json!({"id":"source-reason","type":"reasoning","summary":[],"encrypted_content":"source-encrypted","status":"completed"}),
        json!({"id":"source-message","type":"message","role":"assistant","status":"completed","content":[{"type":"output_text","text":"Source answer.","annotations":[]}]}),
    ];
    let mut events = vec![json!({"type":"response.created","response":{"id":"source-response"}})];
    for (index, item) in output.iter().enumerate() {
        events.push(json!({"type":"response.output_item.added","output_index":index,"item":item}));
        events.push(json!({"type":"response.output_item.done","output_index":index,"item":item}));
    }
    events.push(json!({"type":"response.completed","response":{"id":"source-response","output":output,"usage":{"input_tokens":5,"output_tokens":2}}}));
    for (source_family, target_family, replay_reasoning) in [
        (None, None, false),
        (Some("native-family"), None, false),
        (Some("native-family"), Some("other-family"), false),
        (Some("native-family"), Some("native-family"), true),
        (Some(""), Some(""), false),
    ] {
        let server = FixtureServer::start(vec![
            Step::prompt(
                Protocol::ResponsesHttp,
                "first",
                0,
                Reply::Sse(events.clone()),
            ),
            Step::prompt(
                Protocol::ResponsesHttp,
                "second",
                1,
                Reply::Sse(responses_text("done", "target-response", "target-model")),
            ),
        ])
        .await
        .unwrap();
        let endpoint = ProviderEndpoint::compatible("same-account", server.base_url());
        let mut source_model = model("source-model", ModelTransportProfile::responses_http());
        source_model.capabilities.native_context_family = source_family.map(str::to_owned);
        let source = ModelFactory::new(pl_model::runtime::ThreadModel::new(
            ModelRuntime::new(endpoint.clone(), source_model).unwrap(),
            None,
        ));
        let thread = ThreadHandle::start(
            "model-switch-thread".into(),
            source.open_session().await.unwrap(),
        )
        .unwrap();
        thread
            .run_turn(TurnInput {
                turn_id: "source-turn".into(),
                attempt_prefix: "source-attempt".into(),
                content: vec![ContextContent::Text {
                    text: Arc::from("first"),
                }],
                max_model_steps: ModelStepLimit::Limited(1.try_into().unwrap()),
                cancellation: Default::default(),
            })
            .await
            .unwrap();
        let saved = serde_json::to_vec(
            &thread
                .checkpoint(thread.snapshot().commit_sequence)
                .unwrap(),
        )
        .unwrap();
        thread.close().await.unwrap();
        let mut target_model = model("target-model", ModelTransportProfile::responses_http());
        target_model.capabilities.native_context_family = target_family.map(str::to_owned);
        let target = ModelFactory::new(pl_model::runtime::ThreadModel::new(
            ModelRuntime::new(endpoint, target_model).unwrap(),
            None,
        ));
        let restored = ThreadHandle::resume(
            "model-switch-thread".into(),
            target.open_session().await.unwrap(),
            Some(serde_json::from_slice(&saved).unwrap()),
        )
        .unwrap();
        restored
            .run_turn(TurnInput {
                turn_id: "target-turn".into(),
                attempt_prefix: "target-attempt".into(),
                content: vec![ContextContent::Text {
                    text: Arc::from("second"),
                }],
                max_model_steps: ModelStepLimit::Limited(1.try_into().unwrap()),
                cancellation: Default::default(),
            })
            .await
            .unwrap();
        restored.close().await.unwrap();
        let requests = server.finish().await.unwrap();
        let input = requests[1].body["input"].as_array().unwrap();
        assert_eq!(
            input.iter().any(|item| item["type"] == "reasoning"),
            replay_reasoning
        );
        let assistant = input
            .iter()
            .find(|item| item["type"] == "message" && item["role"] == "assistant")
            .unwrap();
        assert_eq!(assistant["content"][0]["text"], "Source answer.");
    }
}

#[derive(Debug)]
struct ReplayTool(std::sync::Arc<std::sync::atomic::AtomicUsize>);

impl pl_core::tool::opaque::Tool for ReplayTool {
    async fn execute(
        &self,
        input: pl_core::context::OpaquePayload,
        _: pl_core::tool::opaque::CallContext,
    ) -> Result<pl_core::tool::ToolOutput, pl_core::tool::opaque::ToolError> {
        assert_eq!(
            input.content(),
            pl_provider_fixture::replay_tool_arguments()
        );
        self.0.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        Ok(pl_core::tool::ToolOutput::new(
            pl_core::context::OpaquePayload::text("replay-tool-marker"),
            vec![ContextContent::Text {
                text: Arc::from("replay-tool-marker"),
            }],
        ))
    }
}

fn replay_registration(
    count: &Arc<std::sync::atomic::AtomicUsize>,
) -> pl_core::tool::opaque::Registration {
    let spec = ToolSpec::function(
        "exec",
        "Return a recorded marker",
        json!({"type":"object","properties":{"command":{"type":"string"}},"required":["command"]}),
    );
    pl_core::tool::opaque::Registration::new(
        "exec".into(),
        pl_model::runtime::thread_tool_declaration(&spec).unwrap(),
        ReplayTool(count.clone()),
    )
    .unwrap()
    .foreground_coexisting()
}

fn replay_turn(prompt: &str, cancellation: CancellationToken) -> TurnInput {
    TurnInput {
        turn_id: prompt.into(),
        attempt_prefix: prompt.into(),
        content: vec![ContextContent::Text {
            text: Arc::from(prompt),
        }],
        max_model_steps: ModelStepLimit::Limited(2.try_into().unwrap()),
        cancellation,
    }
}

#[tokio::test]
async fn tool_replay_survives_checkpoint_without_reexecuting_tools_or_failed_output() {
    let fixture = FixtureServer::start(pl_provider_fixture::gui_context_replay_recovery_script())
        .await
        .unwrap();
    for (label, protocol, profile) in [
        (
            "http",
            Protocol::ResponsesHttp,
            ModelTransportProfile::responses_http(),
        ),
        (
            "ws",
            Protocol::ResponsesWebSocket,
            ModelTransportProfile::responses_websocket(),
        ),
        (
            "chat",
            Protocol::Chat,
            ModelTransportProfile::chat_completions_http(),
        ),
    ] {
        let mut fixture_model = model("fixture-model", profile);
        fixture_model.capabilities.interleaved = Some(pl_model::model::ReasoningInterleaved {
            field: pl_model::model::ReasoningInterleavedField::ReasoningContent,
        });
        let runtime = ModelRuntime::new(
            ProviderEndpoint::compatible("fixture", fixture.base_url()),
            fixture_model,
        )
        .unwrap();
        let factory = ModelFactory::new(pl_model::runtime::ThreadModel::new(runtime, None));
        let count = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let thread =
            ThreadHandle::start(label.into(), factory.open_session().await.unwrap()).unwrap();
        thread
            .register_tools(vec![replay_registration(&count)])
            .await
            .unwrap();
        for action in ["inspect", "continue"] {
            assert_eq!(
                thread
                    .run_turn(replay_turn(
                        &format!("Replay {action} {label}"),
                        Default::default()
                    ))
                    .await
                    .unwrap()
                    .outcome,
                TurnOutcome::Completed
            );
        }
        assert_eq!(count.load(std::sync::atomic::Ordering::Relaxed), 1);
        let responses = thread
            .snapshot()
            .context
            .records
            .iter()
            .flat_map(|record| record.content.iter())
            .filter_map(|content| match content {
                ContextContent::Opaque { payload } if payload.format() == "pl.model.assistant" => {
                    let frame: Value = serde_json::from_str(payload.content()).unwrap();
                    Some(
                        serde_json::from_value::<pl_model::completion::CompletionResponse>(
                            frame["receipt"]["response"].clone(),
                        )
                        .unwrap(),
                    )
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        assert!(
            responses.iter().any(|response| {
                response.presentation_items.iter().any(|item| {
                    matches!(item.kind,
                    pl_model::completion::CompletionPresentationItemKind::Text(channel)
                    if channel.as_str() == "commentary")
                        && item
                            .parts
                            .iter()
                            .any(|part| part.text == format!("replay progress {label} first"))
                })
            }),
            "{label} lost completed commentary presentation"
        );
        let token = CancellationToken::new();
        let failed = replay_turn(&format!("Replay failed {label}"), token.clone());
        if protocol == Protocol::Chat {
            let active = thread.clone();
            let call = tokio::spawn(async move { active.run_turn(failed).await });
            tokio::time::timeout(std::time::Duration::from_secs(5), async {
                loop {
                    if thread
                        .snapshot()
                        .model_progress
                        .as_ref()
                        .is_some_and(|progress| {
                            progress
                                .progress
                                .parts()
                                .iter()
                                .any(|part| part.text().contains("unfinished replay note"))
                        })
                    {
                        break;
                    }
                    tokio::task::yield_now().await;
                }
            })
            .await
            .unwrap();
            token.cancel();
            assert!(call.await.unwrap().is_err());
        } else {
            assert!(thread.run_turn(failed).await.is_err());
        }
        let before = thread.snapshot().context;
        assert!(
            !serde_json::to_string(&before)
                .unwrap()
                .contains("unfinished replay note")
        );
        let checkpoint = serde_json::to_vec(
            &thread
                .checkpoint(thread.snapshot().commit_sequence)
                .unwrap(),
        )
        .unwrap();
        thread.close().await.unwrap();
        let restored = ThreadHandle::resume(
            label.into(),
            factory.open_session().await.unwrap(),
            Some(serde_json::from_slice(&checkpoint).unwrap()),
        )
        .unwrap();
        restored
            .register_tools(vec![replay_registration(&count)])
            .await
            .unwrap();
        assert_eq!(restored.snapshot().context, before);
        let result = restored
            .run_turn(replay_turn(
                &format!("Replay resume {label}"),
                Default::default(),
            ))
            .await
            .unwrap();
        assert_eq!(result.outcome, TurnOutcome::Completed);
        assert_eq!(count.load(std::sync::atomic::Ordering::Relaxed), 2);
        let saved = serde_json::to_vec(
            &restored
                .checkpoint(restored.snapshot().commit_sequence)
                .unwrap(),
        )
        .unwrap();
        let final_context = restored.snapshot().context;
        restored.close().await.unwrap();
        let recheck = ThreadHandle::resume_without_model(
            label.into(),
            Some(serde_json::from_slice(&saved).unwrap()),
        )
        .unwrap();
        assert_eq!(recheck.snapshot().context, final_context);
        assert_eq!(count.load(std::sync::atomic::Ordering::Relaxed), 2);
        recheck.close().await.unwrap();
    }
    let requests = fixture.finish().await.unwrap();
    assert_eq!(requests.len(), 18);
    assert!(requests.iter().all(|request| request.accepted));
}

#[tokio::test]
async fn auxiliary_requests_keep_frozen_replay_and_mutated_input_is_rejected() {
    for (protocol, profile) in [
        (
            Protocol::ResponsesHttp,
            ModelTransportProfile::responses_http(),
        ),
        (
            Protocol::Chat,
            ModelTransportProfile::chat_completions_http(),
        ),
    ] {
        let raw = format!(
            "<commentary>{}</commentary><final>done</final>",
            "P".repeat(4096)
        );
        let output = pl_provider_fixture::replay_output("aux-source", "frozen answer", None);
        let mut native_events =
            vec![json!({"type":"response.created","response":{"id":"aux-source"}})];
        for (index, item) in output.iter().enumerate() {
            native_events.push(
                json!({"type":"response.output_item.added","output_index":index,"item":item}),
            );
            native_events
                .push(json!({"type":"response.output_item.done","output_index":index,"item":item}));
        }
        native_events.push(
            json!({"type":"response.completed","response":{"id":"aux-source","output":output}}),
        );
        let mut steps = vec![
            Step::prompt(
                protocol,
                "question",
                0,
                Reply::Sse(if protocol == Protocol::Chat {
                    chat_events(&raw, "fixture-model")
                } else {
                    native_events
                }),
            ),
            Step::prompt(
                protocol,
                "Summarize recorded facts\n\nsummarize",
                1,
                Reply::Sse(if protocol == Protocol::Chat {
                    chat_events("summary", "fixture-model")
                } else {
                    responses_text("summary", "aux-summary", "fixture-model")
                }),
            ),
        ];
        if protocol == Protocol::ResponsesHttp {
            let checkpoint = json!({"type":"compaction","encrypted_content":"retained-checkpoint"});
            steps.push(Step::prompt(protocol, "question", 2, Reply::Sse(vec![json!({"type":"response.output_item.done","item":checkpoint}), json!({"type":"response.completed","response":{"id":"aux-compaction","output":[checkpoint]}})])));
        }
        let fixture = FixtureServer::start(steps).await.unwrap();
        let mut endpoint = ProviderEndpoint::compatible("fixture", fixture.base_url());
        endpoint.service_capabilities.remote_compaction = true;
        let runtime = ModelRuntime::new(endpoint, model("fixture-model", profile)).unwrap();
        let first = runtime
            .complete(request("question"), ModelInvocationContext::default())
            .await
            .unwrap();
        let mut prefix = request("question");
        prefix.instructions = Some("Original authoritative instructions".into());
        prefix.append_response(&first).unwrap();
        if protocol == Protocol::Chat {
            assert!(pl_model::completion::estimate_text_input_tokens(&prefix).unwrap() >= 1024);
        } else {
            assert!(pl_model::completion::estimate_text_input_tokens(&prefix).is_none());
            let mut changed = prefix.clone();
            if let ModelContextItem::Message { message } = changed.input.last_mut().unwrap() {
                message.content = MessageContent::text("changed");
            } else {
                panic!("assistant semantic projection missing");
            }
            assert!(
                runtime
                    .complete(changed, ModelInvocationContext::default())
                    .await
                    .is_err()
            );
        }
        let summary = runtime
            .summarize(
                pl_model::runtime::TextSummaryRequest {
                    instructions: "Summarize recorded facts",
                    prefix: prefix.clone(),
                    requirement: "summarize",
                    max_output_tokens: None,
                    empty_summary_error: "empty summary",
                },
                ModelInvocationContext::default(),
            )
            .await
            .unwrap();
        assert_eq!(summary.text, "summary");
        let records = fixture.recorded();
        let body = &records[1].body;
        if protocol == Protocol::Chat {
            assert_eq!(
                body["messages"][0]["content"],
                "Original authoritative instructions"
            );
        } else {
            assert_eq!(body["instructions"], "Original authoritative instructions");
        }
        assert!(body.to_string().contains("Summarize recorded facts"));
        if protocol == Protocol::ResponsesHttp {
            let compacted = runtime
                .compaction()
                .unwrap()
                .checkpoint(prefix, ModelInvocationContext::default())
                .await
                .unwrap();
            assert!(
                matches!(compacted.item, ModelContextItem::Compaction { encrypted_content } if encrypted_content == "retained-checkpoint")
            );
        }
        let requests = fixture.finish().await.unwrap();
        if protocol == Protocol::Chat {
            assert_eq!(requests[1].body["messages"][2]["content"], raw);
        } else {
            assert_eq!(requests[1].body["input"].as_array().unwrap()[1..3], output);
            assert_eq!(requests[2].body["input"].as_array().unwrap()[1..3], output);
        }
    }
}

#[tokio::test]
async fn terminal_replay_completes_missing_items_and_rejects_conflicting_authority() {
    let mut output = pl_provider_fixture::replay_output(
        "terminal-only",
        "terminal answer",
        Some(&pl_provider_fixture::RealtimeToolCall {
            item_id: "terminal-call",
            call_id: "terminal-call",
            name: "lookup",
            arguments: "{  \"key\" : \"value\" }".into(),
        }),
    );
    output.push(json!({"id":"terminal-call-second","type":"function_call","name":"lookup","call_id":"terminal-call-second","arguments":"{ \"key\" : \"second\" }","status":"completed"}));
    let fixture = FixtureServer::start(vec![
        Step::prompt(Protocol::ResponsesHttp, "terminal-only", 0, Reply::Sse(vec![
            json!({"type":"response.output_item.done","output_index":3,"item":output[3]}),
            json!({"type":"response.output_item.done","output_index":2,"item":output[2]}),
            json!({"type":"response.completed","response":{"id":"terminal-only","output":output,"usage":{"input_tokens":20,"output_tokens":5}}}),
        ])),
        Step::prompt(Protocol::ResponsesHttp, "terminal-only", 1,
            Reply::Sse(responses_text("complete", "terminal-next", "fixture-model")))
            .with_replay(pl_provider_fixture::ReplayExpectation { history: output.clone(), previous_response_id: None }),
    ]).await.unwrap();
    let runtime = ModelRuntime::new(
        ProviderEndpoint::compatible("fixture", fixture.base_url()),
        model("fixture-model", ModelTransportProfile::responses_http()),
    )
    .unwrap();
    let response = runtime
        .complete(request("terminal-only"), ModelInvocationContext::default())
        .await
        .unwrap();
    assert_eq!(response.tool_calls.len(), 2);
    assert_eq!(response.tool_calls[0].call_id, "terminal-call");
    assert_eq!(response.tool_calls[1].call_id, "terminal-call-second");
    let mut prefix = request("terminal-only");
    prefix.append_response(&response).unwrap();
    for call in &response.tool_calls {
        prefix.input.push(ModelContextItem::from(Message {
            role: MessageRole::Tool,
            content: MessageContent::text(format!("result for {}", call.call_id)),
            tool_result: Some(ToolResultRecord {
                item_id: call.id.clone(),
                call_id: call.call_id.clone(),
                name: call.name.clone(),
                kind: call.kind(),
            }),
            ..user("")
        }));
    }
    runtime
        .complete(prefix, ModelInvocationContext::default())
        .await
        .unwrap();
    assert!(
        serde_json::to_string(&response.replay)
            .unwrap()
            .contains("terminal answer")
    );
    let requests = fixture.finish().await.unwrap();
    let input = requests[1].body["input"].as_array().unwrap();
    let results = input
        .iter()
        .filter(|item| item["type"] == "function_call_output")
        .collect::<Vec<_>>();
    assert_eq!(results.len(), 2);
    assert_eq!(results[0]["call_id"], "terminal-call");
    assert_eq!(results[1]["call_id"], "terminal-call-second");
    for (kind, mutated) in [
        (
            "role",
            json!({"id":"bad","type":"message","role":"system","content":[{"type":"output_text","text":"elevated"}]}),
        ),
        (
            "tool-result",
            json!({"id":"bad","type":"function_call_output","call_id":"fabricated","output":"fabricated"}),
        ),
        (
            "conflict",
            json!({"id":"bad","type":"message","role":"assistant","content":[{"type":"output_text","text":"changed"}]}),
        ),
    ] {
        let initial = json!({"id":"bad","type":"message","role":"assistant","content":[{"type":"output_text","text":"original"}]});
        let events = vec![
            json!({"type":"response.output_item.done","output_index":0,"item":initial}),
            json!({"type":"response.completed","response":{"id":"rejected","output":[mutated],"usage":{"input_tokens":20,"output_tokens":5}}}),
        ];
        let fixture = FixtureServer::start(vec![Step::prompt(
            Protocol::ResponsesHttp,
            kind,
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
        assert!(
            runtime
                .complete(request(kind), ModelInvocationContext::default())
                .await
                .is_err(),
            "{kind}"
        );
        fixture.finish().await.unwrap();
    }
}

/// 跨 provider 历史回放：另一个 Responses provider 的原生 reasoning item 携带 output-only
/// `status`，严格 Responses 网关会以 `unknown_parameter` 拒绝 `input[*].status`。回放必须去掉
/// 该生命周期字段，同时保留 reasoning 身份/加密内容与 function_call 身份，使后续工具回合在
/// 严格输入下完成。
#[tokio::test]
async fn cross_provider_replay_strips_output_only_status_for_strict_responses_input() {
    let output = vec![
        json!({"id":"source-reason","type":"reasoning","summary":[],"content":[{"type":"reasoning_text","text":"plan the search"}],"encrypted_content":"source-encrypted","status":"completed"}),
        json!({"id":"source-call","type":"function_call","name":"lookup","call_id":"source-call","arguments":"{\"key\":\"rust\"}","status":"completed"}),
        json!({"id":"source-message","type":"message","role":"assistant","phase":"final_answer","status":"completed","content":[{"type":"output_text","text":"Found the docs.","annotations":[]}]}),
    ];
    let mut source_events =
        vec![json!({"type":"response.created","response":{"id":"source-response"}})];
    for (index, item) in output.iter().enumerate() {
        source_events
            .push(json!({"type":"response.output_item.added","output_index":index,"item":item}));
        source_events
            .push(json!({"type":"response.output_item.done","output_index":index,"item":item}));
    }
    source_events.push(json!({"type":"response.completed","response":{"id":"source-response","output":output,"usage":{"input_tokens":9,"output_tokens":4}}}));
    let source = FixtureServer::start(vec![Step::prompt(
        Protocol::ResponsesHttp,
        "cross provider",
        0,
        Reply::Sse(source_events),
    )])
    .await
    .unwrap();
    let source_runtime = ModelRuntime::new(
        ProviderEndpoint::compatible("source", source.base_url()),
        model("source-model", ModelTransportProfile::responses_http()),
    )
    .unwrap();
    let response = source_runtime
        .complete(request("cross provider"), ModelInvocationContext::default())
        .await
        .unwrap();
    assert_eq!(response.tool_calls.len(), 1);

    let mut prefix = request("cross provider");
    prefix.append_response(&response).unwrap();
    for call in &response.tool_calls {
        prefix.input.push(ModelContextItem::from(Message {
            role: MessageRole::Tool,
            content: MessageContent::text(format!("docs for {}", call.call_id)),
            tool_result: Some(ToolResultRecord {
                item_id: call.id.clone(),
                call_id: call.call_id.clone(),
                name: call.name.clone(),
                kind: call.kind(),
            }),
            ..user("")
        }));
    }

    // 严格网关的期望输入：原生 reasoning 去掉 output-only `status`，其余事实（reasoning 身份、
    // 加密内容、function_call 身份）保持不变。
    let expected = output
        .iter()
        .map(|item| {
            let mut item = item.clone();
            if item["type"] == "reasoning" {
                item.as_object_mut().unwrap().remove("status");
            }
            item
        })
        .collect::<Vec<_>>();
    let target = FixtureServer::start(vec![
        Step::prompt(
            Protocol::ResponsesHttp,
            "cross provider",
            0,
            Reply::Sse(responses_text("done", "target-response", "target-model")),
        )
        .with_replay(pl_provider_fixture::ReplayExpectation {
            history: expected,
            previous_response_id: None,
        }),
    ])
    .await
    .unwrap();
    let target_runtime = ModelRuntime::new(
        ProviderEndpoint::compatible("target", target.base_url()),
        model("target-model", ModelTransportProfile::responses_http()),
    )
    .unwrap();
    target_runtime
        .complete(prefix, ModelInvocationContext::default())
        .await
        .unwrap();

    let records = target.finish().await.unwrap();
    let input = records[0].body["input"].as_array().unwrap();
    let reasoning = input
        .iter()
        .find(|item| item["type"] == "reasoning")
        .expect("replayed reasoning item");
    assert!(reasoning.get("status").is_none());
    assert_eq!(reasoning["id"], "source-reason");
    assert_eq!(reasoning["encrypted_content"], "source-encrypted");
    let call = input
        .iter()
        .find(|item| item["type"] == "function_call")
        .expect("replayed function call");
    assert_eq!(call["call_id"], "source-call");
    assert_eq!(call["name"], "lookup");
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
    let mut second_input = request("first");
    second_input.append_response(&first).unwrap();
    second_input.input.push(user("second").into());
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
    // Plain completed items are safe to replay. The interrupted observation must
    // survive a later permanent failure, not be mistaken for the final result.
    let fixture = FixtureServer::start(vec![
        Step::prompt(
            Protocol::ResponsesHttp,
            "incomplete items",
            0,
            Reply::Sse(events),
        ),
        Step::prompt(
            Protocol::ResponsesHttp,
            "incomplete items",
            1,
            Reply::HttpError {
                status: 401,
                code: "invalid_api_key".into(),
                message: "recovery rejected".into(),
            },
        ),
    ])
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
    assert_eq!(
        failure
            .source
            .provider_failure_ref()
            .unwrap()
            .code
            .as_deref(),
        Some("invalid_api_key")
    );
    assert!(failure.presentation_items.is_empty());
    let progress = failure.partial_progress.unwrap();
    assert_eq!(progress.observation().generation, 1);
    assert_eq!(progress.observation().failed.len(), 1);
    assert_eq!(
        progress.observation().recovery.unwrap().phase,
        pl_core::model::ModelRecoveryPhase::Failed
    );
    let retained = &progress.observation().failed[0].parts;
    assert_eq!(retained.len(), 150);
    for (index, part) in retained.iter().enumerate() {
        let pl_core::model::ObservedPartIdentity::Provider(identity) = part.identity() else {
            panic!("received provider item must retain its original identity");
        };
        assert_eq!(identity.item_id.as_ref(), format!("partial-item-{index}"));
        assert_eq!(part.text(), format!("part-{index}"));
    }
    let records = fixture.finish().await.unwrap();
    assert_eq!(records.len(), 2);
    assert_eq!(records[0].body["input"], records[1].body["input"]);
}

#[tokio::test]
async fn provider_error_is_permanent_and_unknown_prompt_is_rejected() {
    // Business failures may use HTTP 200 and JSON even for stream requests.
    // They must retain the reason and stop after one request, not masquerade
    // as an empty SSE stream and exhaust the transport retry budget.
    for (reply, code, status, kind, message) in [
        (
            Reply::Json(json!({"code":1000,"msg":"身份验证失败。","success":false})),
            "1000",
            200,
            pl_protocol::ProviderFailureKind::Authentication,
            "身份验证失败。",
        ),
        (
            Reply::Json(
                json!({"error":{"code":"invalid_api_key","message":"credential rejected"}}),
            ),
            "invalid_api_key",
            200,
            pl_protocol::ProviderFailureKind::Authentication,
            "credential rejected",
        ),
        (
            Reply::HttpError {
                status: 400,
                code: "invalid_request_error".into(),
                message: "bad field".into(),
            },
            "invalid_request_error",
            400,
            pl_protocol::ProviderFailureKind::Configuration,
            "bad field",
        ),
    ] {
        let fixture =
            FixtureServer::start(vec![Step::prompt(Protocol::Chat, "bad request", 0, reply)])
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
        assert!(failure.to_string().contains(message), "{failure}");
        let pl_protocol::PureError::Provider(error) = failure.source.as_ref() else {
            panic!("expected a classified provider failure: {failure}");
        };
        assert_eq!(error.code.as_deref(), Some(code));
        assert_eq!(error.http_status, Some(status));
        assert_eq!(error.kind, kind);
        assert_eq!(error.retry, pl_protocol::RetryDisposition::Permanent);
        let records = fixture.finish().await.unwrap();
        assert_eq!(records.len(), 1);
    }

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
async fn explicitly_enabled_deepseek_hosted_search_has_its_own_minimal_wire_dialect() {
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
    let mut model = bundled("deepseek-v4-pro");
    model.capabilities.web_search = true;
    let runtime = ModelRuntime::new(endpoint, model).unwrap();
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
        .unwrap()
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
async fn new_openai_provider_routes_gpt6_sol_none_effort_through_responses() {
    let preset = builtin_provider_catalog()
        .unwrap()
        .presets
        .into_iter()
        .find(|item| item.id.as_str() == "openai")
        .unwrap();
    let mut model = preset
        .provider
        .effective_models()
        .unwrap()
        .into_iter()
        .find(|model| model.slug == preset.suggested_model)
        .unwrap();
    let fixture = FixtureServer::start(vec![Step::prompt(
        Protocol::ResponsesHttp,
        "new provider prompt",
        0,
        Reply::Sse(responses_text("sol result", "sol-1", "gpt-6-sol")),
    )])
    .await
    .unwrap();
    model
        .binding
        .set_transport(ModelTransportProfile::responses_http());
    let runtime =
        ModelRuntime::new(ProviderEndpoint::openai(Some(fixture.base_url())), model).unwrap();
    let reply = runtime
        .complete(
            CompletionRequest::builder()
                .messages(vec![user("new provider prompt")])
                .reasoning(Some(ReasoningConfig {
                    effort: Some("none".into()),
                    summary: None,
                }))
                .build(),
            ModelInvocationContext::default(),
        )
        .await
        .unwrap();
    assert_eq!(reply.content.as_deref(), Some("sol result"));
    let records = fixture.finish().await.unwrap();
    assert_eq!(records[0].path, "/v1/responses");
    assert_eq!(records[0].body["model"], "gpt-6-sol");
    assert_eq!(records[0].body["reasoning"]["effort"], "none");
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
async fn attachment_preparation_and_inference_share_retry_budget_and_keep_successful_upload() {
    let bytes = tiny_png();
    let upload = json!({
        "purpose":"user_data", "expires_after[anchor]":"created_at", "expires_after[seconds]":"86400",
        "file":{"filename":"tiny.png","mime_type":"image/png","sha256":hex::encode(Sha256::digest(&bytes))}
    });
    let unavailable = || Reply::HttpError {
        status: 503,
        code: "server_error".into(),
        message: "temporary failure".into(),
    };
    let mut steps = (0..4)
        .map(|_| Step::exact(Protocol::Files, upload.clone(), unavailable()))
        .collect::<Vec<_>>();
    steps.push(Step::exact(
        Protocol::Files,
        upload,
        Reply::Json(json!({"id":"file-once"})),
    ));
    for step in 5..7 {
        steps.push(Step::prompt(
            Protocol::ResponsesHttp,
            "inspect",
            step,
            unavailable(),
        ));
    }
    steps.push(Step::prompt(
        Protocol::ResponsesHttp,
        "next",
        7,
        Reply::Sse(responses_text("recovered", "next", "deepseek-flash")),
    ));
    let fixture = FixtureServer::start(steps).await.unwrap();
    let mut endpoint = ProviderEndpoint::deepseek(Some(fixture.base_url()));
    endpoint.service_capabilities.files = FileUploadCapability::DeepSeek;
    let runtime = ModelRuntime::new(endpoint, bundled("deepseek-flash")).unwrap();
    let session = ModelSession::default();
    let failure = runtime
        .complete(
            image_request("inspect", bytes.clone()),
            ModelInvocationContext::new(session.clone()),
        )
        .await
        .unwrap_err();
    assert!(failure.is_transient_model_transport());
    let recovery = failure
        .partial_progress
        .as_ref()
        .unwrap()
        .observation()
        .recovery
        .unwrap();
    assert_eq!(
        recovery.phase,
        pl_core::model::ModelRecoveryPhase::Exhausted
    );
    assert_eq!(recovery.retry, 5);
    let next = runtime
        .complete(
            image_request("next", bytes),
            ModelInvocationContext::new(session.clone()),
        )
        .await
        .unwrap();
    assert_eq!(next.content.as_deref(), Some("recovered"));
    assert_eq!(next.orchestration.transport_attempts, 1);
    session.close().await.unwrap();
    let records = fixture.finish().await.unwrap();
    assert_eq!(records.len(), 8);
    assert_eq!(
        records
            .iter()
            .filter(|r| r.path.ends_with("/files"))
            .count(),
        5
    );
    for record in &records[5..] {
        assert_eq!(
            record.body["input"][0]["content"][1]["file_id"],
            "file-once"
        );
    }
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

#[tokio::test]
async fn empty_native_checkpoint_is_rejected_before_commit_and_keeps_usage() {
    let fixture = FixtureServer::start(vec![Step::prompt(Protocol::ResponsesHttp, "empty-native", 0,
        Reply::Sse(vec![json!({"type":"response.output_item.done","item":{"type":"compaction","encrypted_content":" "}}),
        json!({"type":"response.completed","response":{"id":"empty","usage":{"input_tokens":31,"output_tokens":2}}})]))]).await.unwrap();
    let mut endpoint = ProviderEndpoint::compatible("fixture", fixture.base_url());
    endpoint.service_capabilities.remote_compaction = true;
    let runtime = ModelRuntime::new(
        endpoint,
        model("origin-model", ModelTransportProfile::responses_http()),
    )
    .unwrap();
    let failure = runtime
        .compaction()
        .unwrap()
        .checkpoint(request("empty-native"), ModelInvocationContext::default())
        .await
        .unwrap_err();
    assert_eq!(failure.accounting.usage.input_tokens, Some(31));
    assert!(failure.to_string().contains("empty checkpoint"));
    fixture.finish().await.unwrap();
}

#[tokio::test]
async fn native_checkpoint_replay_requires_bound_origin_and_explicit_model_family() {
    use pl_core::{
        context::{ContextRecord, ContextSnapshot, OpaquePayload},
        model::{ModelRequest, ToolCallMode},
    };
    use pl_model::runtime::{
        ThreadCompactionOptions, ThreadCompactionStrategy, ThreadModel, migrate_legacy_compaction,
    };
    let fixture = FixtureServer::start(vec![Step::prompt(Protocol::ResponsesHttp, "native-origin", 0,
        Reply::Sse(vec![json!({"type":"response.output_item.done","item":{"type":"compaction","encrypted_content":"origin-bound-material"}}),
        json!({"type":"response.completed","response":{"id":"origin","usage":{"input_tokens":20,"output_tokens":2}}})]))]).await.unwrap();
    let mut endpoint = ProviderEndpoint::compatible("fixture", fixture.base_url());
    endpoint.service_capabilities.remote_compaction = true;
    let mut info = model("source-model", ModelTransportProfile::responses_http());
    info.capabilities.native_context_family = Some("explicit-fixture-family".into());
    let source = ThreadModel::new(
        ModelRuntime::new_with_provider_id("source", endpoint.clone(), info.clone()).unwrap(),
        None,
    );
    let result = source
        .compact(
            ModelRequest {
                thread_id: "origin-thread".into(),
                turn_id: "origin-turn".into(),
                attempt_id: "origin-attempt".into(),
                context: ContextSnapshot {
                    revision: 1,
                    records: vec![ContextRecord {
                        id: "input".into(),
                        turn_id: Some("origin-turn".into()),
                        source: ContextSource::User,
                        content: vec![ContextContent::Text {
                            text: "native-origin".into(),
                        }],
                        tool_calls: vec![],
                    }]
                    .into(),
                },
                tools: Default::default(),
                tool_call_mode: ToolCallMode::Parallel,
                solo_tool_ids: Default::default(),
                committed_private_context: None,
                resources: Default::default(),
                cancellation: Default::default(),
                progress: None,
            },
            ThreadCompactionOptions {
                strategy: ThreadCompactionStrategy::PreferNative,
                instructions: "compact".into(),
                requirement: "compact".into(),
                summary_prefix: "summary".into(),
                max_output_tokens: None,
            },
        )
        .await
        .unwrap();
    let context = ContextSnapshot {
        revision: 2,
        records: result.replacement.records.into(),
    };
    source.validate_context_origin(&context).unwrap();
    let other_account = ThreadModel::new(
        ModelRuntime::new_with_provider_id("other-account", endpoint.clone(), info.clone())
            .unwrap(),
        None,
    );
    assert!(other_account.validate_context_origin(&context).is_err());
    info.slug = "target-model".into();
    info.capabilities.native_context_family = None;
    let unproven = ThreadModel::new(
        ModelRuntime::new_with_provider_id("source", endpoint.clone(), info.clone()).unwrap(),
        None,
    );
    assert!(unproven.validate_context_origin(&context).is_err());
    info.capabilities.native_context_family = Some("explicit-fixture-family".into());
    let explicit = ThreadModel::new(
        ModelRuntime::new_with_provider_id("source", endpoint, info).unwrap(),
        None,
    );
    explicit.validate_context_origin(&context).unwrap();
    let mut duplicate = context.clone();
    let mut records = duplicate.records.to_vec();
    let mut second = records[0].clone();
    second.id = "second-native".into();
    records.push(second);
    duplicate.records = records.into();
    assert!(source.validate_context_origin(&duplicate).is_err());
    let mut old = context.clone();
    let records = Arc::make_mut(&mut old.records);
    let record = &mut records[0];
    let ContextContent::Opaque { payload } = &record.content[0] else {
        panic!("native material missing")
    };
    let item = serde_json::from_str::<Value>(payload.content()).unwrap()["item"].clone();
    record.content = vec![ContextContent::Opaque {
        payload: OpaquePayload::new("pl.model.compaction", 1, item.to_string()).unwrap(),
    }];
    assert!(!migrate_legacy_compaction(record, "wrong-receipt", result.binding.clone()).unwrap());
    assert!(migrate_legacy_compaction(record, "origin-attempt", result.binding).unwrap());
    source.validate_context_origin(&old).unwrap();
    fixture.finish().await.unwrap();
}

#[tokio::test]
async fn instruction_snapshots_use_only_jointly_declared_endpoint_and_model_semantics() {
    use pl_core::context::ContextRecord;
    use pl_core::thread::{ContextReplacementReason, ReplaceContext};
    use pl_model::provider::HistoryInstructionRole;
    for (model_support, endpoint_support, append) in [
        (false, false, false),
        (true, false, false),
        (false, true, false),
        (true, true, true),
    ] {
        let fixture = FixtureServer::start(vec![
            Step::prompt(
                Protocol::ResponsesHttp,
                "snapshot-probe",
                0,
                Reply::Sse(responses_text("first", "first", "snapshot-model")),
            ),
            Step::prompt(
                Protocol::ResponsesHttp,
                "snapshot-probe",
                1,
                Reply::Sse(responses_text("second", "second", "snapshot-model")),
            ),
        ])
        .await
        .unwrap();
        let mut endpoint = ProviderEndpoint::compatible("fixture", fixture.base_url());
        endpoint.service_capabilities.history_instruction_role = if endpoint_support {
            HistoryInstructionRole::Developer
        } else {
            HistoryInstructionRole::None
        };
        let mut info = model("snapshot-model", ModelTransportProfile::responses_http());
        info.capabilities.instruction_snapshot_overrides = model_support;
        let session = ModelFactory::new(pl_model::runtime::ThreadModel::new(
            ModelRuntime::new(endpoint, info).unwrap(),
            None,
        ))
        .open_session()
        .await
        .unwrap();
        let thread = ThreadHandle::start("snapshot-thread".into(), session).unwrap();
        let base = ContextRecord {
            id: "initial-instructions".into(),
            turn_id: None,
            source: ContextSource::Instruction,
            content: vec![ContextContent::Text {
                text: "initial authority".into(),
            }],
            tool_calls: vec![],
        };
        thread
            .replace_context(ReplaceContext {
                expected_revision: 0,
                reason: ContextReplacementReason::Rebuild,
                records: vec![base],
            })
            .await
            .unwrap();
        for index in 0..2 {
            if index == 1 {
                let state = thread.snapshot();
                let mut records = state.context.records.to_vec();
                records.push(ContextRecord {
                    id: "instruction-revision".into(),
                    turn_id: None,
                    source: ContextSource::InstructionSnapshot {
                        revision: 1,
                        content_hash: pl_core::context::content_hash(b"new complete authority"),
                        replaces: None,
                    },
                    content: vec![ContextContent::Text {
                        text: "new complete authority".into(),
                    }],
                    tool_calls: vec![],
                });
                thread
                    .replace_context(ReplaceContext {
                        expected_revision: state.context.revision,
                        reason: ContextReplacementReason::Rebuild,
                        records,
                    })
                    .await
                    .unwrap();
            }
            thread
                .run_turn(TurnInput {
                    turn_id: format!("turn-{index}"),
                    attempt_prefix: format!("attempt-{index}"),
                    content: vec![ContextContent::Text {
                        text: "snapshot-probe".into(),
                    }],
                    max_model_steps: ModelStepLimit::Limited(1.try_into().unwrap()),
                    cancellation: Default::default(),
                })
                .await
                .unwrap();
        }
        thread.close().await.unwrap();
        let records = fixture.finish().await.unwrap();
        let body = &records[1].body;
        if append {
            assert!(
                body["instructions"]
                    .as_str()
                    .unwrap()
                    .contains("initial authority")
            );
            assert!(
                body["input"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|item| item["role"] == "developer"
                        && item.to_string().contains("new complete authority"))
            );
        } else {
            assert!(
                body["instructions"]
                    .as_str()
                    .unwrap()
                    .contains("new complete authority")
            );
            assert!(
                !body["instructions"]
                    .as_str()
                    .unwrap()
                    .contains("initial authority")
            );
        }
    }
}

#[tokio::test]
async fn chat_reasoning_replay_uses_target_capability_and_rejects_untyped_details() {
    use pl_model::model::{ReasoningInterleaved, ReasoningInterleavedField};
    for field in [
        None,
        Some(ReasoningInterleavedField::ReasoningContent),
        Some(ReasoningInterleavedField::Reasoning),
        Some(ReasoningInterleavedField::ReasoningDetails),
    ] {
        let rejected = field == Some(ReasoningInterleavedField::ReasoningDetails);
        let fixture = FixtureServer::start(if rejected {
            vec![]
        } else {
            vec![Step::prompt(
                Protocol::Chat,
                "target-probe",
                0,
                Reply::Sse(chat_events("done", "target-model")),
            )]
        })
        .await
        .unwrap();
        let mut info = model(
            "target-model",
            ModelTransportProfile::chat_completions_http(),
        );
        info.capabilities.interleaved = field.map(|field| ReasoningInterleaved { field });
        let runtime = ModelRuntime::new(
            ProviderEndpoint::compatible("target", fixture.base_url()),
            info,
        )
        .unwrap();
        let assistant = Message {
            role: MessageRole::Assistant,
            reasoning_content: Some("retained reasoning".into()),
            ..user("previous answer")
        };
        let result = runtime
            .complete(
                CompletionRequest::builder()
                    .messages(vec![assistant, user("target-probe")])
                    .build(),
                ModelInvocationContext::default(),
            )
            .await;
        let records = fixture.finish().await.unwrap();
        if rejected {
            assert!(result.is_err());
            assert!(records.is_empty());
            continue;
        }
        result.unwrap();
        let message = records[0].body["messages"]
            .as_array()
            .unwrap()
            .iter()
            .find(|message| message["role"] == "assistant")
            .unwrap();
        assert_eq!(
            message.get("reasoning_content").and_then(Value::as_str),
            (field == Some(ReasoningInterleavedField::ReasoningContent))
                .then_some("retained reasoning")
        );
        assert_eq!(
            message.get("reasoning").and_then(Value::as_str),
            (field == Some(ReasoningInterleavedField::Reasoning)).then_some("retained reasoning")
        );
        assert!(message.get("reasoning_details").is_none());
    }
}

#[tokio::test]
async fn stable_cache_hint_keeps_diagnostics_separate_from_changed_instructions() {
    use pl_core::context::{ContextRecord, ContextSnapshot};
    use pl_core::model::{ModelRequest, ToolCallMode};
    use pl_model::runtime::{ThreadModel, model_request_receipt};
    use pl_protocol::PromptPrefixChangedReason;
    let fixture = FixtureServer::start(vec![
        Step::prompt(
            Protocol::ResponsesHttp,
            "cache-probe",
            0,
            Reply::Sse(responses_text("first", "cache-1", "cache-model")),
        ),
        Step::prompt(
            Protocol::ResponsesHttp,
            "cache-probe",
            1,
            Reply::Sse(responses_text("second", "cache-2", "cache-model")),
        ),
    ])
    .await
    .unwrap();
    let mut endpoint = ProviderEndpoint::compatible("cache", fixture.base_url());
    endpoint.service_capabilities.prompt_cache.dialect = PromptCacheDialect::OpenAiPromptCacheKey;
    let factory = ModelFactory::new(ThreadModel::new(
        ModelRuntime::new(
            endpoint,
            model("cache-model", ModelTransportProfile::responses_http()),
        )
        .unwrap(),
        None,
    ));
    let mut session = factory.open_session().await.unwrap();
    let mut prompts = Vec::new();
    for index in 0..2 {
        let call = session
            .prepare(ModelRequest {
                thread_id: "stable-thread".into(),
                turn_id: format!("turn-{index}"),
                attempt_id: format!("attempt-{index}"),
                context: ContextSnapshot {
                    revision: index + 1,
                    records: vec![
                        ContextRecord {
                            id: "instructions".into(),
                            turn_id: None,
                            source: ContextSource::Instruction,
                            content: vec![ContextContent::Text {
                                text: format!("complete authority {index}").into(),
                            }],
                            tool_calls: vec![],
                        },
                        ContextRecord {
                            id: format!("input-{index}"),
                            turn_id: Some(format!("turn-{index}")),
                            source: ContextSource::User,
                            content: vec![ContextContent::Text {
                                text: "cache-probe".into(),
                            }],
                            tool_calls: vec![],
                        },
                    ]
                    .into(),
                },
                tools: Default::default(),
                tool_call_mode: ToolCallMode::Sequential,
                solo_tool_ids: Default::default(),
                committed_private_context: None,
                resources: None,
                cancellation: Default::default(),
                progress: None,
            })
            .await
            .unwrap();
        prompts.push(
            model_request_receipt(call.request_metadata().unwrap())
                .unwrap()
                .prompt
                .unwrap(),
        );
        call.execute().await.unwrap();
    }
    session.close().await.unwrap();
    let records = fixture.finish().await.unwrap();
    assert!(records[0].body["prompt_cache_key"].as_str().is_some());
    assert_eq!(
        records[0].body["prompt_cache_key"],
        records[1].body["prompt_cache_key"]
    );
    assert_ne!(prompts[0].fixed_prefix_hash, prompts[1].fixed_prefix_hash);
    assert_eq!(
        prompts[1].prefix_changed_reason,
        PromptPrefixChangedReason::FixedPrefixChanged
    );
    assert_eq!(prompts[1].generation, prompts[0].generation + 1);
}

#[tokio::test]
async fn compaction_keeps_recent_execution_proof_without_reexecuting_the_tool() {
    use pl_core::{
        context::{ContextRecord, ContextSnapshot},
        model::{ModelRequest, ModelToolDeclaration, ToolCallMode},
        thread::{ContextReplacementReason, ReplaceContext},
    };
    use pl_model::runtime::{ThreadCompactionOptions, ThreadCompactionStrategy, ThreadModel};
    let call = json!({"type":"function_call","id":"proof-output","call_id":"proof-call","name":"exec","arguments":pl_provider_fixture::replay_tool_arguments()});
    let fixture = FixtureServer::start(vec![
        Step::prompt(Protocol::ResponsesHttp, "proof-task", 0, Reply::Sse(vec![json!({"type":"response.output_item.done","item":call}),json!({"type":"response.completed","response":{"id":"proof-response","output":[call]}})])),
        Step::prompt(Protocol::ResponsesHttp, "proof-task", 1, Reply::Sse(responses_text("done", "proof-final", "proof-model"))),
        Step::prompt(Protocol::ResponsesHttp, "summarize proof\n\nsummary requirement", 2, Reply::Sse(responses_text("summary omits the execution proof", "proof-summary", "proof-model"))),
        Step::prompt(Protocol::ResponsesHttp, "resume-proof", 3, Reply::Sse(responses_text("already completed", "proof-resume", "proof-model"))),
    ]).await.unwrap();
    let source = ThreadModel::new(
        ModelRuntime::new(
            ProviderEndpoint::compatible("proof", fixture.base_url()),
            model("proof-model", ModelTransportProfile::responses_http()),
        )
        .unwrap(),
        None,
    );
    let factory = ModelFactory::new(source.clone());
    let thread =
        ThreadHandle::start("proof-thread".into(), factory.open_session().await.unwrap()).unwrap();
    let count = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    thread
        .register_tools(vec![replay_registration(&count)])
        .await
        .unwrap();
    let spec = ToolSpec::function(
        "exec",
        "Return a recorded marker",
        json!({"type":"object","properties":{"command":{"type":"string"}},"required":["command"]}),
    );
    thread
        .replace_context(ReplaceContext {
            expected_revision: 0,
            reason: ContextReplacementReason::Rebuild,
            records: vec![ContextRecord {
                id: "delegated-task".into(),
                turn_id: None,
                source: ContextSource::AgentMessage {
                    source_id: "parent".into(),
                    purpose: pl_core::context::AgentMessageKind::Task,
                },
                content: vec![ContextContent::Text {
                    text: "explicit private task input".into(),
                }],
                tool_calls: vec![],
            }],
        })
        .await
        .unwrap();
    thread
        .run_turn(replay_turn("proof-task", Default::default()))
        .await
        .unwrap();
    let state = thread.snapshot();
    let compacted = source
        .compact(
            ModelRequest {
                thread_id: "proof-thread".into(),
                turn_id: "proof-task".into(),
                attempt_id: "proof-compaction".into(),
                context: state.context.clone(),
                tools: vec![ModelToolDeclaration {
                    tool_id: "exec".into(),
                    declaration: pl_model::runtime::thread_tool_declaration(&spec).unwrap(),
                }]
                .into(),
                tool_call_mode: ToolCallMode::Sequential,
                solo_tool_ids: Default::default(),
                committed_private_context: None,
                resources: None,
                cancellation: Default::default(),
                progress: None,
            },
            ThreadCompactionOptions {
                strategy: ThreadCompactionStrategy::TextSummary,
                instructions: "summarize proof".into(),
                requirement: "summary requirement".into(),
                summary_prefix: "summary".into(),
                max_output_tokens: None,
            },
        )
        .await
        .unwrap();
    let reduced = ContextSnapshot {
        revision: state.context.revision + 1,
        records: compacted.replacement.records.clone().into(),
    };
    assert!(
        reduced
            .records
            .iter()
            .any(|record| record.id == "delegated-task")
    );
    assert!(reduced.records.iter().any(|record| {
        record
            .tool_calls
            .iter()
            .any(|call| call.call_id == "proof-call")
    }));
    assert!(reduced.records.iter().any(|record| matches!(&record.source, ContextSource::ToolResult { call_id, .. } if call_id == "proof-call")));
    thread.replace_context(compacted.replacement).await.unwrap();
    thread
        .run_turn(replay_turn("resume-proof", Default::default()))
        .await
        .unwrap();
    thread.close().await.unwrap();
    assert_eq!(count.load(std::sync::atomic::Ordering::Relaxed), 1);
    let records = fixture.finish().await.unwrap();
    let input = records[3].body["input"].as_array().unwrap();
    assert!(
        input
            .iter()
            .any(|item| item["type"] == "function_call" && item["call_id"] == "proof-call")
    );
    assert!(
        input
            .iter()
            .any(|item| item["type"] == "function_call_output"
                && item["call_id"] == "proof-call"
                && item["output"] == "replay-tool-marker")
    );
}

#[tokio::test]
async fn native_capacity_preview_counts_new_input_and_requires_bound_prior_usage() {
    use pl_core::thread::{ModelUsageOrigin, context_preparation::PreviousContextUsage};
    use pl_core::{
        context::{ContextRecord, ContextSnapshot},
        model::{ModelRequest, ModelUsage, ToolCallMode},
    };
    use pl_model::runtime::ThreadModel;
    let checkpoint_item = json!({"type":"compaction","encrypted_content":"capacity-native"});
    let fixture = FixtureServer::start(vec![
        Step::prompt(Protocol::ResponsesHttp, "old-input", 0, Reply::Sse(responses_text("retained output", "capacity-output", "capacity-model"))),
        Step::prompt(Protocol::ResponsesHttp, "old-input", 1, Reply::Sse(vec![json!({"type":"response.output_item.done","item":checkpoint_item}), json!({"type":"response.completed","response":{"id":"capacity-compaction","output":[checkpoint_item]}})])),
    ]).await.unwrap();
    let mut endpoint = ProviderEndpoint::compatible("capacity", fixture.base_url());
    endpoint.service_capabilities.remote_compaction = true;
    let source = ThreadModel::new(
        ModelRuntime::new(
            endpoint,
            model("capacity-model", ModelTransportProfile::responses_http()),
        )
        .unwrap(),
        None,
    );
    let mut session = ModelFactory::new(source.clone())
        .open_session()
        .await
        .unwrap();
    let mut request = ModelRequest {
        thread_id: "capacity-thread".into(),
        turn_id: "old-turn".into(),
        attempt_id: "old-attempt".into(),
        context: ContextSnapshot {
            revision: 1,
            records: vec![ContextRecord {
                id: "old-input".into(),
                turn_id: Some("old-turn".into()),
                source: ContextSource::User,
                content: vec![ContextContent::Text {
                    text: "old-input".into(),
                }],
                tool_calls: vec![],
            }]
            .into(),
        },
        tools: Default::default(),
        tool_call_mode: ToolCallMode::Sequential,
        solo_tool_ids: Default::default(),
        committed_private_context: None,
        resources: None,
        cancellation: Default::default(),
        progress: None,
    };
    let call = session.prepare(request.clone()).await.unwrap();
    let binding = call.usage_binding().unwrap().clone();
    let output = call.execute().await.unwrap();
    session.close().await.unwrap();
    let compacted = source
        .compact(
            request.clone(),
            pl_model::runtime::ThreadCompactionOptions {
                strategy: pl_model::runtime::ThreadCompactionStrategy::PreferNative,
                instructions: "compact".into(),
                requirement: "compact".into(),
                summary_prefix: "summary".into(),
                max_output_tokens: None,
            },
        )
        .await
        .unwrap();
    let mut records = compacted.replacement.records;
    let input_record_count = records.len();
    records.push(ContextRecord {
        id: "old-output".into(),
        turn_id: Some("old-turn".into()),
        source: ContextSource::Assistant,
        content: output.content,
        tool_calls: output.tool_calls,
    });
    records.push(ContextRecord {
        id: "new-input".into(),
        turn_id: Some("new-turn".into()),
        source: ContextSource::User,
        content: vec![ContextContent::Text {
            text: "01234567".into(),
        }],
        tool_calls: vec![],
    });
    request.context.records = records.into();
    assert!(
        source
            .estimate_input(&request, None)
            .await
            .unwrap()
            .is_none()
    );
    let mut previous = PreviousContextUsage {
        usage: ModelUsage {
            input_tokens: Some(100),
            output_tokens: Some(20),
            ..Default::default()
        },
        origin: ModelUsageOrigin {
            binding,
            input_revision: 1,
            context_revision: 1,
            input_record_count,
            input_hash: Some("core-validated-prefix".into()),
        },
    };
    assert_eq!(
        source
            .estimate_input(&request, Some(&previous))
            .await
            .unwrap()
            .unwrap()
            .tokens,
        122
    );
    previous.origin.binding.route_identity = Some("another-route".into());
    assert!(
        source
            .estimate_input(&request, Some(&previous))
            .await
            .unwrap()
            .is_none()
    );
    fixture.finish().await.unwrap();
}

use std::collections::HashMap;

use pl_protocol::{
    AttachmentModality, ContentPart, Message, MessageContent, MessageRole, ModelContextItem,
    PureError, ResponsesContextItem, ResponsesContextItemKind, ToolCallCaller, ToolCallKind,
    ToolCallRecord, ToolMediaContext, ToolResultReceipt, ToolResultRecord, ToolSpec,
};
use pretty_assertions::assert_eq;

use super::*;
use crate::completion::tool_schema::CustomToolProjection;
use crate::completion::{ReasoningConfig, ReasoningSummary, ToolCallPayload};
use crate::model::info::{
    MaxTokensField, MediaRepresentation, ModelMediaInputProfile, ResponsesMaxTokensField,
};

fn text_message(role: MessageRole, content: &str) -> Message {
    Message {
        presentation: Default::default(),
        role,
        content: MessageContent::text(content.to_string()),
        reasoning_content: None,
        tool_calls: None,
        tool_result: None,
        metadata: HashMap::new(),
    }
}

fn image_message() -> Message {
    Message {
        presentation: Default::default(),
        role: MessageRole::User,
        content: MessageContent::new(vec![
            ContentPart::Text {
                text: "describe".to_string(),
            },
            ContentPart::Attachment {
                attachment_id: "attachment-1".to_string(),
                modality: AttachmentModality::Image,
                media_type: "image/png".to_string(),
                filename: Some("sample.png".to_string()),
            },
        ]),
        reasoning_content: None,
        tool_calls: None,
        tool_result: None,
        metadata: HashMap::new(),
    }
}

fn image_prepared_content() -> Vec<crate::completion::PreparedContentPart> {
    vec![crate::completion::PreparedContentPart {
        attachment_id: "attachment-1".to_string(),
        modality: AttachmentModality::Image,
        media_type: "image/png".to_string(),
        filename: Some("sample.png".to_string()),
        sources: vec![crate::completion::PreparedContentSource::DataUrl {
            base64: "aGVsbG8=".to_string(),
        }],
    }]
}

fn two_image_message() -> Message {
    Message {
        presentation: Default::default(),
        role: MessageRole::User,
        content: MessageContent::new(vec![
            ContentPart::Attachment {
                attachment_id: "attachment-1".to_string(),
                modality: AttachmentModality::Image,
                media_type: "image/png".to_string(),
                filename: Some("first.png".to_string()),
            },
            ContentPart::Attachment {
                attachment_id: "attachment-2".to_string(),
                modality: AttachmentModality::Image,
                media_type: "image/png".to_string(),
                filename: Some("second.png".to_string()),
            },
        ]),
        reasoning_content: None,
        tool_calls: None,
        tool_result: None,
        metadata: HashMap::new(),
    }
}

fn prepared_image(
    attachment_id: &str,
    remote_url: Option<&str>,
    base64: Option<&str>,
) -> crate::completion::PreparedContentPart {
    let mut sources = Vec::new();
    if let Some(remote_url) = remote_url {
        sources.push(crate::completion::PreparedContentSource::RemoteUrl {
            url: remote_url.to_string(),
        });
    }
    if let Some(base64) = base64 {
        sources.push(crate::completion::PreparedContentSource::DataUrl {
            base64: base64.to_string(),
        });
    }
    crate::completion::PreparedContentPart {
        attachment_id: attachment_id.to_string(),
        modality: AttachmentModality::Image,
        media_type: "image/png".to_string(),
        filename: Some(format!("{attachment_id}.png")),
        sources,
    }
}

fn media_message(attachment_id: &str, modality: AttachmentModality, media_type: &str) -> Message {
    Message {
        presentation: Default::default(),
        role: MessageRole::User,
        content: MessageContent::new(vec![ContentPart::Attachment {
            attachment_id: attachment_id.to_string(),
            modality,
            media_type: media_type.to_string(),
            filename: Some(attachment_id.to_string()),
        }]),
        reasoning_content: None,
        tool_calls: None,
        tool_result: None,
        metadata: HashMap::new(),
    }
}

fn data_media(
    attachment_id: &str,
    modality: AttachmentModality,
    media_type: &str,
) -> crate::completion::PreparedContentPart {
    crate::completion::PreparedContentPart {
        attachment_id: attachment_id.to_string(),
        modality,
        media_type: media_type.to_string(),
        filename: Some(attachment_id.to_string()),
        sources: vec![crate::completion::PreparedContentSource::DataUrl {
            base64: "cGF5bG9hZA==".to_string(),
        }],
    }
}

fn profiled_media_model(modality: crate::model::ModelModality) -> ModelInfo {
    let mut model = ModelInfo::fallback("profiled-media-model");
    model.request_profile.media = vec![ModelMediaInputProfile {
        modality,
        wire: match modality {
            crate::model::ModelModality::Image => crate::model::MediaWireFormat::ChatImageUrl,
            crate::model::ModelModality::Video => crate::model::MediaWireFormat::ChatVideoUrl,
            crate::model::ModelModality::File => crate::model::MediaWireFormat::ChatFileUrl,
            crate::model::ModelModality::Text | crate::model::ModelModality::Audio => {
                panic!("unsupported media test modality")
            }
        },
        first_send: vec![MediaRepresentation::DataUrl],
        replay: vec![MediaRepresentation::DataUrl],
    }];
    model
}

fn context_items(messages: Vec<Message>) -> Vec<ModelContextItem> {
    messages.into_iter().map(ModelContextItem::from).collect()
}

fn tool_batch_with_image_media() -> Vec<ModelContextItem> {
    let calls = ["call-1", "call-2"]
        .into_iter()
        .map(|call_id| ToolCallRecord {
            item_id: format!("item-{call_id}"),
            call_id: call_id.to_string(),
            name: "view_image".to_string(),
            kind: ToolCallKind::Function,
            arguments: serde_json::json!({"path": format!("{call_id}.png")}),
            caller: None,
        })
        .collect::<Vec<_>>();
    let mut items = vec![ModelContextItem::from(Message {
        presentation: Default::default(),
        role: MessageRole::Assistant,
        content: MessageContent::text(""),
        reasoning_content: None,
        tool_calls: Some(calls.clone()),
        tool_result: None,
        metadata: HashMap::new(),
    })];
    for call in &calls {
        items.push(ModelContextItem::ToolResult {
            message: Message {
                presentation: Default::default(),
                role: MessageRole::Tool,
                content: MessageContent::text(format!("read {}", call.call_id)),
                reasoning_content: None,
                tool_calls: None,
                tool_result: Some(ToolResultRecord {
                    item_id: call.item_id.clone(),
                    call_id: call.call_id.clone(),
                    name: call.name.clone(),
                    kind: call.kind,
                }),
                metadata: HashMap::new(),
            },
            receipt: ToolResultReceipt {
                call_id: call.call_id.clone(),
                tool_name: call.name.clone(),
                arguments_hash: "arguments".to_string(),
                result_hash: "result".to_string(),
                total_bytes: 4,
                visible_bytes: 4,
                truncated: false,
                artifacts: Vec::new(),
                continuation: None,
                reused_from_call_id: None,
            },
        });
    }
    items.push(ModelContextItem::ToolMedia {
        items: vec![ToolMediaContext {
            call_id: "call-1".to_string(),
            label: "pure-7429.png".to_string(),
            attachment: pl_protocol::ThreadAttachment {
                id: "attachment-1".to_string(),
                modality: AttachmentModality::Image,
                media_type: "image/png".to_string(),
                filename: Some("pure-7429.png".to_string()),
                width: Some(640),
                height: Some(480),
                byte_size: 5,
            },
        }],
    });
    items
}

fn request_with_effort(effort: &str) -> CompletionRequest {
    CompletionRequest::builder()
        .input(context_items(vec![text_message(
            MessageRole::User,
            "hello",
        )]))
        .parallel_tool_calls(true)
        .reasoning(Some(ReasoningConfig {
            effort: Some(effort.to_string()),
            summary: None,
        }))
        .build()
}

#[test]
fn responses_use_top_level_instructions_and_developer_messages() {
    let request = CompletionRequest::builder()
        .instructions("base")
        .input(context_items(vec![
            text_message(MessageRole::System, "developer"),
            text_message(MessageRole::User, "user context"),
            text_message(MessageRole::User, "real prompt"),
        ]))
        .build();

    let responses_body = OpenAiProtocol::responses().build_request_body(&request);
    let chat_body = OpenAiProtocol::chat().build_request_body(&request);

    assert_eq!(responses_body["instructions"], serde_json::json!("base"),);
    assert_eq!(
        responses_body["input"]
            .as_array()
            .unwrap()
            .iter()
            .map(|item| item["role"].as_str().unwrap())
            .collect::<Vec<_>>(),
        vec!["developer", "user", "user"],
    );
    assert_eq!(
        chat_body["messages"]
            .as_array()
            .unwrap()
            .iter()
            .map(|item| item["role"].as_str().unwrap())
            .collect::<Vec<_>>(),
        vec!["system", "system", "user", "user"],
    );
    assert_eq!(
        chat_body["messages"][0]["content"],
        serde_json::json!("base"),
    );
}

#[test]
fn responses_maps_image_parts_to_input_image() {
    let request = CompletionRequest::builder()
        .input(context_items(vec![image_message()]))
        .prepared_content(image_prepared_content())
        .build();

    let model = bundled_model("gpt-5.4");
    let body = OpenAiProtocol::responses().build_request_body_with_model(&request, &model);

    assert_eq!(body["input"][0]["content"][0]["type"], "input_text");
    assert_eq!(body["input"][0]["content"][0]["text"], "describe");
    assert_eq!(body["input"][0]["content"][1]["type"], "input_image");
    assert_eq!(
        body["input"][0]["content"][1]["image_url"],
        "data:image/png;base64,aGVsbG8="
    );
}

#[test]
fn responses_maps_deepseek_vision_image_to_its_exact_input_image_wire() {
    let request = CompletionRequest::builder()
        .input(context_items(vec![image_message()]))
        .prepared_content(image_prepared_content())
        .build();

    let model = bundled_model("deepseek-v4-flash-vision-exp");
    let body = OpenAiProtocol::responses().build_request_body_with_model(&request, &model);

    assert_eq!(body["model"], "deepseek-v4-flash-vision-exp");
    assert_eq!(body["input"][0]["role"], "user");
    assert_eq!(body["input"][0]["content"][0]["type"], "input_text");
    assert_eq!(body["input"][0]["content"][0]["text"], "describe");
    assert_eq!(body["input"][0]["content"][1]["type"], "input_image");
    assert_eq!(
        body["input"][0]["content"][1]["image_url"],
        "data:image/png;base64,aGVsbG8="
    );
}

#[test]
fn responses_places_complete_tool_batch_before_internal_image_context() {
    let request = CompletionRequest::builder()
        .input(tool_batch_with_image_media())
        .prepared_content(image_prepared_content())
        .build();

    let body = OpenAiProtocol::responses()
        .build_request_body_with_model(&request, &bundled_model("deepseek-v4-flash-vision-exp"));

    assert_eq!(body["input"][0]["type"], "function_call");
    assert_eq!(body["input"][1]["type"], "function_call");
    assert_eq!(body["input"][2]["type"], "function_call_output");
    assert_eq!(body["input"][2]["call_id"], "call-1");
    assert_eq!(body["input"][3]["type"], "function_call_output");
    assert_eq!(body["input"][3]["call_id"], "call-2");
    assert_eq!(body["input"][4]["role"], "user");
    assert_eq!(body["input"][4]["content"][0]["type"], "input_text");
    assert_eq!(
        body["input"][4]["content"][0]["text"],
        "Image from view_image call call-1: pure-7429.png"
    );
    assert_eq!(body["input"][4]["content"][1]["type"], "input_image");
    assert_eq!(
        body["input"][4]["content"][1]["image_url"],
        "data:image/png;base64,aGVsbG8="
    );
}

#[test]
fn chat_maps_image_parts_to_content_array() {
    let request = CompletionRequest::builder()
        .input(context_items(vec![image_message()]))
        .prepared_content(image_prepared_content())
        .build();

    let model = bundled_model("glm-5.3-flash");
    let body = OpenAiProtocol::chat().build_request_body_with_model(&request, &model);

    assert_eq!(body["messages"][0]["content"][0]["type"], "text");
    assert_eq!(body["messages"][0]["content"][1]["type"], "image_url");
    assert_eq!(
        body["messages"][0]["content"][1]["image_url"]["url"],
        "data:image/png;base64,aGVsbG8="
    );
}

#[test]
fn chat_places_complete_tool_message_batch_before_internal_image_context() {
    let request = CompletionRequest::builder()
        .input(tool_batch_with_image_media())
        .prepared_content(image_prepared_content())
        .build();

    let body = OpenAiProtocol::chat()
        .build_request_body_with_model(&request, &bundled_model("glm-5.3-flash"));

    assert_eq!(body["messages"][0]["role"], "assistant");
    assert_eq!(body["messages"][1]["role"], "tool");
    assert_eq!(body["messages"][1]["tool_call_id"], "item-call-1");
    assert_eq!(body["messages"][2]["role"], "tool");
    assert_eq!(body["messages"][2]["tool_call_id"], "item-call-2");
    assert_eq!(body["messages"][3]["role"], "user");
    assert_eq!(body["messages"][3]["content"][0]["type"], "text");
    assert_eq!(body["messages"][3]["content"][1]["type"], "image_url");
    assert_eq!(
        body["messages"][3]["content"][1]["image_url"]["url"],
        "data:image/png;base64,aGVsbG8="
    );
}

#[test]
fn chat_uses_one_remote_url_representation_for_the_entire_image_batch() {
    let request = CompletionRequest::builder()
        .input(context_items(vec![two_image_message()]))
        .prepared_content(vec![
            prepared_image(
                "attachment-1",
                Some("https://cdn.example/first.png"),
                Some("Zmlyc3Q="),
            ),
            prepared_image(
                "attachment-2",
                Some("https://cdn.example/second.png"),
                Some("c2Vjb25k"),
            ),
        ])
        .build();

    let body = OpenAiProtocol::chat()
        .build_request_body_with_model(&request, &bundled_model("glm-5.3-flash"));

    assert_eq!(
        body["messages"][0]["content"][0]["image_url"]["url"],
        "https://cdn.example/first.png"
    );
    assert_eq!(
        body["messages"][0]["content"][1]["image_url"]["url"],
        "https://cdn.example/second.png"
    );
}

#[test]
fn chat_falls_back_the_entire_image_batch_to_data_urls() {
    let request = CompletionRequest::builder()
        .input(context_items(vec![two_image_message()]))
        .prepared_content(vec![
            prepared_image(
                "attachment-1",
                Some("https://cdn.example/first.png"),
                Some("Zmlyc3Q="),
            ),
            prepared_image("attachment-2", None, Some("c2Vjb25k")),
        ])
        .build();

    let body = OpenAiProtocol::chat()
        .build_request_body_with_model(&request, &bundled_model("glm-5.3-flash"));

    assert_eq!(
        body["messages"][0]["content"][0]["image_url"]["url"],
        "data:image/png;base64,Zmlyc3Q="
    );
    assert_eq!(
        body["messages"][0]["content"][1]["image_url"]["url"],
        "data:image/png;base64,c2Vjb25k"
    );
}

#[test]
fn media_planning_failure_is_structured_and_does_not_expose_source_values() {
    let secret_url = "https://secret.example/private.png";
    let request = CompletionRequest::builder()
        .input(context_items(vec![two_image_message()]))
        .prepared_content(vec![
            prepared_image("attachment-1", Some(secret_url), None),
            prepared_image("attachment-2", None, Some("c2Vuc2l0aXZlLWJ5dGVz")),
        ])
        .build();

    let error = OpenAiProtocol::chat()
        .build_request(&request, &bundled_model("glm-5.3-flash"), None)
        .expect_err("a batch without a common representation must fail");
    let message = error.to_string();

    assert!(message.contains("model=glm-5.3-flash"));
    assert!(message.contains("modality=image"));
    assert!(message.contains("count=2"));
    assert!(!message.contains(secret_url));
    assert!(!message.contains("c2Vuc2l0aXZlLWJ5dGVz"));
}

#[test]
fn chat_serializes_exact_video_url_and_file_url_parts() {
    for (modality, model_modality, media_type, wire_field) in [
        (
            AttachmentModality::Video,
            crate::model::ModelModality::Video,
            "video/mp4",
            "video_url",
        ),
        (
            AttachmentModality::File,
            crate::model::ModelModality::File,
            "application/pdf",
            "file_url",
        ),
    ] {
        let request = CompletionRequest::builder()
            .input(context_items(vec![media_message(
                "attachment-1",
                modality,
                media_type,
            )]))
            .prepared_content(vec![data_media("attachment-1", modality, media_type)])
            .build();

        let body = OpenAiProtocol::chat()
            .build_request_body_with_model(&request, &profiled_media_model(model_modality));

        assert_eq!(body["messages"][0]["content"][0]["type"], wire_field);
        assert_eq!(
            body["messages"][0]["content"][0][wire_field]["url"],
            format!("data:{media_type};base64,cGF5bG9hZA==")
        );
    }
}

#[test]
fn responses_rejects_video_and_file_attachments() {
    for (modality, model_modality, media_type) in [
        (
            AttachmentModality::Video,
            crate::model::ModelModality::Video,
            "video/mp4",
        ),
        (
            AttachmentModality::File,
            crate::model::ModelModality::File,
            "application/pdf",
        ),
    ] {
        let request = CompletionRequest::builder()
            .input(context_items(vec![media_message(
                "attachment-1",
                modality,
                media_type,
            )]))
            .prepared_content(vec![data_media("attachment-1", modality, media_type)])
            .build();

        let error = OpenAiProtocol::responses()
            .build_request(&request, &profiled_media_model(model_modality), None)
            .expect_err("Responses must fail closed for video and file attachments");

        assert!(error.to_string().contains("Responses does not support"));
    }
}

#[test]
fn chat_omits_parallel_tool_calls_without_profile_opt_in() {
    let request = request_with_effort("high");
    let model = ModelInfo::fallback("chat-compatible");

    let body = OpenAiProtocol::chat().build_request_body_with_model(&request, &model);

    assert!(body.get("parallel_tool_calls").is_none());
}

#[test]
fn chat_writes_enabled_parallel_tool_calls_after_profile_opt_in() {
    let request = request_with_effort("high");
    let mut model = ModelInfo::fallback("chat-compatible");
    model.request_profile.chat_parallel_tool_calls = true;

    let body = OpenAiProtocol::chat().build_request_body_with_model(&request, &model);

    assert_eq!(body["parallel_tool_calls"], serde_json::json!(true));
}

#[test]
fn chat_writes_disabled_parallel_tool_calls_after_profile_opt_in() {
    let mut request = request_with_effort("high");
    request.parallel_tool_calls = false;
    let mut model = ModelInfo::fallback("chat-compatible");
    model.request_profile.chat_parallel_tool_calls = true;

    let body = OpenAiProtocol::chat().build_request_body_with_model(&request, &model);

    assert_eq!(body["parallel_tool_calls"], serde_json::json!(false));
}

#[test]
fn responses_parallel_tool_calls_wire_is_unchanged() {
    let request = request_with_effort("high");
    let model = ModelInfo::fallback("responses-compatible");

    let body = OpenAiProtocol::responses().build_request_body_with_model(&request, &model);

    assert_eq!(body["parallel_tool_calls"], serde_json::json!(true));
}

fn custom_tool_call_record() -> ToolCallRecord {
    ToolCallRecord {
        item_id: "ctc_1".to_string(),
        call_id: "call_1".to_string(),
        name: "apply_patch".to_string(),
        kind: ToolCallKind::Custom,
        arguments: serde_json::Value::String("*** Begin Patch\n*** End Patch".to_string()),
        caller: None,
    }
}

fn function_tool_call_record() -> ToolCallRecord {
    ToolCallRecord {
        item_id: "fc_1".to_string(),
        call_id: "call_1".to_string(),
        name: "read_file".to_string(),
        kind: ToolCallKind::Function,
        arguments: serde_json::json!({ "path": "Cargo.toml" }),
        caller: None,
    }
}

fn tool_call_result_record(call: &ToolCallRecord) -> ToolResultRecord {
    ToolResultRecord {
        item_id: call.item_id.clone(),
        call_id: call.call_id.clone(),
        name: call.name.clone(),
        kind: call.kind,
    }
}

fn assistant_tool_call_history(call: ToolCallRecord) -> Message {
    Message {
        presentation: Default::default(),
        role: MessageRole::Assistant,
        content: MessageContent::text(String::new()),
        reasoning_content: None,
        tool_calls: Some(vec![call]),
        tool_result: None,
        metadata: HashMap::new(),
    }
}

fn tool_result_history(record: ToolResultRecord, output: &str) -> Message {
    Message {
        presentation: Default::default(),
        role: MessageRole::Tool,
        content: MessageContent::text(output.to_string()),
        reasoning_content: None,
        tool_calls: None,
        tool_result: Some(record),
        metadata: HashMap::new(),
    }
}

fn request_with_tool_history(tool_result: Option<ToolResultRecord>) -> CompletionRequest {
    let call = custom_tool_call_record();
    CompletionRequest::builder()
        .input(context_items(match tool_result {
            Some(record) => vec![
                assistant_tool_call_history(call),
                tool_result_history(record, "ok"),
            ],
            None => vec![assistant_tool_call_history(call)],
        }))
        .build()
}

fn request_with_function_tool_history(tool_result: Option<ToolResultRecord>) -> CompletionRequest {
    let call = function_tool_call_record();
    CompletionRequest::builder()
        .input(context_items(match tool_result {
            Some(record) => vec![
                assistant_tool_call_history(call),
                tool_result_history(record, "ok"),
            ],
            None => vec![assistant_tool_call_history(call)],
        }))
        .build()
}

fn bundled_model(slug: &str) -> ModelInfo {
    crate::model::default_models()
        .into_iter()
        .find(|model| model.slug == slug)
        .unwrap_or_else(|| panic!("test bundled model not found: {slug}"))
}

#[test]
fn responses_body_writes_effort_via_parameter_wire() {
    let model = bundled_model("gpt-5.5");
    let body = OpenAiProtocol::responses()
        .build_request_body_with_model(&request_with_effort("high"), &model);

    assert_eq!(body["reasoning"]["effort"], serde_json::json!("high"));
}

#[test]
fn responses_body_writes_gpt56_max_effort_via_parameter_wire() {
    let model = bundled_model("gpt-5.6-sol");
    let body = OpenAiProtocol::responses()
        .build_request_body_with_model(&request_with_effort("max"), &model);

    assert_eq!(body["reasoning"]["effort"], serde_json::json!("max"));
}

#[test]
fn responses_body_maps_enabled_reasoning_summary_to_auto() {
    let model = bundled_model("gpt-5.5");
    let mut request = request_with_effort("medium");
    request.reasoning.as_mut().unwrap().summary = Some(ReasoningSummary::Enabled);

    let body = OpenAiProtocol::responses().build_request_body_with_model(&request, &model);

    assert_eq!(body["reasoning"]["summary"], serde_json::json!("auto"));
}

#[test]
fn responses_body_omits_disabled_reasoning_summary() {
    let model = bundled_model("gpt-5.5");
    let mut request = request_with_effort("medium");
    request.reasoning.as_mut().unwrap().summary = Some(ReasoningSummary::Disabled);

    let body = OpenAiProtocol::responses().build_request_body_with_model(&request, &model);

    assert!(body["reasoning"].get("summary").is_none());
}

#[test]
fn request_codec_only_writes_invocation_prompt_cache_key_for_responses() {
    let request = request_with_effort("medium");
    let model = ModelInfo::fallback("gpt-5.5");

    let responses_body = serde_json::to_value(
        OpenAiProtocol::responses()
            .build_request(&request, &model, Some("project-cache-key"))
            .unwrap(),
    )
    .unwrap();
    let chat_body = serde_json::to_value(
        OpenAiProtocol::chat()
            .build_request(&request, &model, Some("project-cache-key"))
            .unwrap(),
    )
    .unwrap();

    assert_eq!(responses_body["store"], serde_json::json!(false));
    assert!(responses_body.get("previous_response_id").is_none());
    assert_eq!(
        responses_body["prompt_cache_key"],
        serde_json::json!("project-cache-key")
    );
    assert!(chat_body.get("store").is_none());
    assert!(chat_body.get("previous_response_id").is_none());
    assert!(chat_body.get("prompt_cache_key").is_none());
}

#[test]
fn responses_body_omits_profiled_max_tokens_by_default() {
    let mut request = request_with_effort("medium");
    request.max_tokens = Some(8192);

    let body = OpenAiProtocol::responses().build_request_body(&request);

    assert!(body.get("max_output_tokens").is_none());
    assert!(body.get("max_tokens").is_none());
    assert!(body.get("max_completion_tokens").is_none());
}

#[test]
fn responses_body_can_use_profiled_max_output_tokens_field() {
    let mut model = ModelInfo::fallback("responses-like");
    model.request_profile.responses_max_tokens_field = ResponsesMaxTokensField::MaxOutputTokens;
    let mut request = request_with_effort("medium");
    request.max_tokens = Some(8192);

    let body = OpenAiProtocol::responses().build_request_body_with_model(&request, &model);

    assert_eq!(body["max_output_tokens"], serde_json::json!(8192));
    assert!(body.get("max_tokens").is_none());
    assert!(body.get("max_completion_tokens").is_none());
}

#[test]
fn responses_body_can_use_profiled_compatible_max_tokens_fields() {
    let mut request = request_with_effort("medium");
    request.max_tokens = Some(8192);

    let mut max_tokens_model = ModelInfo::fallback("responses-like");
    max_tokens_model.request_profile.responses_max_tokens_field =
        ResponsesMaxTokensField::MaxTokens;
    let max_tokens_body =
        OpenAiProtocol::responses().build_request_body_with_model(&request, &max_tokens_model);

    let mut max_completion_model = ModelInfo::fallback("responses-like");
    max_completion_model
        .request_profile
        .responses_max_tokens_field = ResponsesMaxTokensField::MaxCompletionTokens;
    let max_completion_body =
        OpenAiProtocol::responses().build_request_body_with_model(&request, &max_completion_model);

    assert_eq!(max_tokens_body["max_tokens"], serde_json::json!(8192));
    assert!(max_tokens_body.get("max_output_tokens").is_none());
    assert_eq!(
        max_completion_body["max_completion_tokens"],
        serde_json::json!(8192)
    );
    assert!(max_completion_body.get("max_output_tokens").is_none());
}

#[test]
fn chat_body_can_use_profiled_max_completion_tokens_field() {
    let mut model = ModelInfo::fallback("mimo-chat");
    model.request_profile.max_tokens_field = MaxTokensField::MaxCompletionTokens;
    let mut request = request_with_effort("high");
    request.max_tokens = Some(8192);

    let body = OpenAiProtocol::chat().build_request_body_with_model(&request, &model);

    assert!(body.get("max_tokens").is_none());
    assert_eq!(body["max_completion_tokens"], serde_json::json!(8192));
}

#[test]
fn mimo_chat_body_uses_catalog_wire_policy_without_reasoning_effort() {
    let model = bundled_model("mimo-v2.5-pro");
    let mut request = request_with_effort("enabled");
    request.max_tokens = Some(131_072);

    let body = OpenAiProtocol::chat().build_request_body_with_model(&request, &model);

    assert_eq!(body["max_completion_tokens"], serde_json::json!(131_072));
    assert_eq!(body["thinking"]["type"], serde_json::json!("enabled"));
    assert!(body.get("reasoning_effort").is_none());
    assert!(model.capabilities.tools.function_calling);
    assert!(!model.capabilities.tools.parallel_tool_calls);
    assert_eq!(
        model.capabilities.interleaved.unwrap().field,
        crate::model::ReasoningInterleavedField::ReasoningContent
    );
}

#[test]
fn chat_body_without_effort_parameter_omits_reasoning_fields() {
    let body = OpenAiProtocol::chat().build_request_body(&request_with_effort("max"));

    assert!(body.get("reasoning_effort").is_none());
    assert!(body.get("thinking").is_none());
}

#[test]
fn deepseek_chat_body_writes_effort_and_base_body_thinking() {
    let model = bundled_model("deepseek-v4-flash");
    let body =
        OpenAiProtocol::chat().build_request_body_with_model(&request_with_effort("max"), &model);

    assert_eq!(body["reasoning_effort"], serde_json::json!("max"));
    assert_eq!(body["thinking"]["type"], serde_json::json!("enabled"));
}

#[test]
fn zhipu_plain_chat_body_maps_effort_to_thinking_type() {
    let model = bundled_model("glm-5");
    let body = OpenAiProtocol::chat()
        .build_request_body_with_model(&request_with_effort("enabled"), &model);

    assert!(body.get("reasoning_effort").is_none());
    assert_eq!(body["thinking"]["type"], serde_json::json!("enabled"));
    assert_eq!(body["thinking"]["clear_thinking"], serde_json::json!(false));
}

#[test]
fn glm52_chat_body_links_reasoning_effort_and_thinking() {
    let model = bundled_model("glm-5.2");
    for effort in ["high", "max"] {
        let body = OpenAiProtocol::chat()
            .build_request_body_with_model(&request_with_effort(effort), &model);

        assert_eq!(body["reasoning_effort"], serde_json::json!(effort));
        assert_eq!(body["thinking"]["type"], serde_json::json!("enabled"));
        assert_eq!(body["thinking"]["clear_thinking"], serde_json::json!(false));
    }
}

#[test]
fn glm52_chat_body_none_disables_thinking_and_removes_reasoning_effort() {
    let model = bundled_model("glm-5.2");
    let body =
        OpenAiProtocol::chat().build_request_body_with_model(&request_with_effort("none"), &model);

    assert!(body.get("reasoning_effort").is_none());
    assert_eq!(body["thinking"]["type"], serde_json::json!("disabled"));
    assert!(body["thinking"].get("clear_thinking").is_none());
}

#[test]
fn glm53_flash_chat_body_links_thinking_and_sends_image_parts() {
    let model = bundled_model("glm-5.3-flash");
    let request = CompletionRequest::builder()
        .input(context_items(vec![image_message()]))
        .prepared_content(image_prepared_content())
        .reasoning(Some(ReasoningConfig {
            effort: Some("max".to_string()),
            summary: None,
        }))
        .build();

    let body = OpenAiProtocol::chat().build_request_body_with_model(&request, &model);

    assert_eq!(body["messages"][0]["content"][0]["type"], "text");
    assert_eq!(body["messages"][0]["content"][1]["type"], "image_url");
    assert_eq!(
        body["messages"][0]["content"][1]["image_url"]["url"],
        "data:image/png;base64,aGVsbG8="
    );
    assert_eq!(body["reasoning_effort"], serde_json::json!("max"));
    assert_eq!(body["thinking"]["type"], serde_json::json!("enabled"));
    assert_eq!(body["thinking"]["clear_thinking"], serde_json::json!(false));
}

#[test]
fn chat_body_writes_assistant_reasoning_content() {
    let mut request = request_with_effort("high");
    request.input = vec![pl_protocol::ModelContextItem::from(Message {
        presentation: Default::default(),
        role: MessageRole::Assistant,
        content: MessageContent::text("9.11 更大。".to_string()),
        reasoning_content: Some("比较小数位。".to_string()),
        tool_calls: None,
        tool_result: None,
        metadata: HashMap::new(),
    })];

    let body = OpenAiProtocol::chat().build_request_body(&request);

    assert_eq!(
        body["messages"][0]["reasoning_content"],
        serde_json::json!("比较小数位。")
    );
}

#[test]
fn chat_parse_response_reads_reasoning_content() {
    let response = OpenAiProtocol::chat()
        .parse_response(serde_json::json!({
            "model": "deepseek-v4-flash",
            "choices": [{
                "message": {
                    "role": "assistant",
                    "reasoning_content": "先比较整数，再比较小数。",
                    "content": "9.11 更大。"
                },
                "finish_reason": "stop"
            }],
            "usage": {
                "prompt_tokens": 4,
                "completion_tokens": 8,
                "total_tokens": 12
            }
        }))
        .unwrap();

    assert_eq!(response.content.as_deref(), Some("9.11 更大。"));
    assert_eq!(
        response.reasoning_content.as_deref(),
        Some("先比较整数，再比较小数。")
    );
}

#[test]
fn chat_parse_response_reads_deepseek_cached_token_aliases() {
    for cached_usage in [
        serde_json::json!({"prompt_cache_hit_tokens": 40}),
        serde_json::json!({"cached_prompt_tokens": 40}),
        serde_json::json!({"prompt_tokens_details": {"cached_tokens": 40}}),
    ] {
        let mut usage = serde_json::json!({
            "prompt_tokens": 100,
            "completion_tokens": 20,
            "total_tokens": 120
        });
        usage.as_object_mut().unwrap().extend(
            cached_usage
                .as_object()
                .unwrap()
                .iter()
                .map(|(key, value)| (key.clone(), value.clone())),
        );
        let response = OpenAiProtocol::chat()
            .parse_response(serde_json::json!({
                "model": "deepseek-v4-flash",
                "choices": [{
                    "message": {
                        "role": "assistant",
                        "content": "ok"
                    },
                    "finish_reason": "stop"
                }],
                "usage": usage
            }))
            .unwrap();

        assert_eq!(response.usage.cached_prompt_tokens, 40);
    }
}

#[test]
fn deepseek_chat_body_never_writes_openai_cache_fields() {
    let request = request_with_effort("medium");
    let body = serde_json::to_value(
        OpenAiProtocol::chat()
            .build_request(
                &request,
                &ModelInfo::fallback("deepseek-v4-flash"),
                Some("must-not-cross-chat-wire"),
            )
            .unwrap(),
    )
    .unwrap();

    assert!(body.get("prompt_cache_key").is_none());
    assert!(body.get("prompt_cache_breakpoint").is_none());
    assert!(body.get("prompt_cache_options").is_none());
}

#[test]
fn responses_parse_response_reads_cached_input_tokens() {
    let response = OpenAiProtocol::responses()
        .parse_response(serde_json::json!({
            "model": "gpt-5.5",
            "output": [{
                "type": "message",
                "content": [{ "text": "ok" }]
            }],
            "usage": {
                "input_tokens": 100,
                "output_tokens": 20,
                "total_tokens": 120,
                "input_tokens_details": {
                    "cached_tokens": 55,
                    "cache_write_tokens": 12
                }
            }
        }))
        .unwrap();

    assert_eq!(response.usage.cached_prompt_tokens, 55);
    assert_eq!(response.usage.cache_write_tokens, 12);
}

#[test]
fn responses_body_writes_custom_grammar_tool() {
    let mut request = request_with_effort("xhigh");
    request.tools = vec![ToolSpec::custom_grammar(
        "apply_patch",
        "edit files",
        "lark",
        "start: patch",
    )];

    let body = OpenAiProtocol::responses().build_request_body(&request);

    assert_eq!(body["tools"][0]["type"], serde_json::json!("custom"));
    assert_eq!(body["tools"][0]["name"], serde_json::json!("apply_patch"));
    assert_eq!(
        body["tools"][0]["format"],
        serde_json::json!({
            "type": "grammar",
            "syntax": "lark",
            "definition": "start: patch"
        })
    );
}

#[test]
fn responses_body_writes_programmatic_tool_callers() {
    let model = bundled_model("gpt-5.6-sol");
    let mut request = request_with_effort("xhigh");
    let read_file = ToolSpec::function(
        "read_file",
        "read a file",
        serde_json::json!({"type": "object"}),
    )
    .allow_programmatic(serde_json::json!({
        "type": "object",
        "additionalProperties": true
    }));
    request.tools = vec![read_file, ToolSpec::ProgrammaticToolCalling];

    let body = OpenAiProtocol::responses().build_request_body_with_model(&request, &model);

    assert_eq!(body["tools"][0]["type"], "function");
    assert_eq!(body["tools"][0]["name"], "read_file");
    assert_eq!(
        body["tools"][0]["allowed_callers"],
        serde_json::json!(["direct", "programmatic"])
    );
    assert_eq!(body["tools"][0]["output_schema"]["type"], "object");
    assert_eq!(body["tools"][1]["type"], "programmatic_tool_calling");
}

#[test]
fn responses_replays_program_caller_and_native_items_in_order() {
    let call = ToolCallRecord {
        item_id: "fc_1".to_string(),
        call_id: "call_1".to_string(),
        name: "read_file".to_string(),
        kind: ToolCallKind::Function,
        arguments: serde_json::json!({"path": "README.md"}),
        caller: Some(ToolCallCaller::Program {
            caller_id: "program-1".to_string(),
        }),
    };
    let mut request = request_with_effort("xhigh");
    request.input = vec![
        ModelContextItem::Responses {
            item: ResponsesContextItem {
                kind: ResponsesContextItemKind::Program,
                value: serde_json::json!({"type": "program", "id": "program-1"}),
            },
        },
        ModelContextItem::from(assistant_tool_call_history(call.clone())),
        ModelContextItem::from(tool_result_history(
            tool_call_result_record(&call),
            r#"{"content":"ok"}"#,
        )),
        ModelContextItem::Responses {
            item: ResponsesContextItem {
                kind: ResponsesContextItemKind::ProgramOutput,
                value: serde_json::json!({
                    "type": "program_output",
                    "id": "program-output-1"
                }),
            },
        },
    ];

    let body = OpenAiProtocol::responses().build_request_body(&request);

    assert_eq!(body["input"][0]["type"], "program");
    assert_eq!(body["input"][1]["type"], "function_call");
    assert_eq!(body["input"][1]["caller"]["caller_id"], "program-1");
    assert_eq!(body["input"][2]["type"], "function_call_output");
    assert_eq!(body["input"][2]["caller"]["caller_id"], "program-1");
    assert_eq!(body["input"][3]["type"], "program_output");

    let error = OpenAiProtocol::chat()
        .build_request(&request, &ModelInfo::fallback("gpt-5.5"), None)
        .unwrap_err();
    assert!(error.to_string().contains("Responses native items"));
}

#[test]
fn responses_parse_response_preserves_orchestration_items_and_caller() {
    let response = OpenAiProtocol::responses()
        .parse_response(serde_json::json!({
            "id": "resp-1",
            "model": "gpt-5.6-sol",
            "output": [
                {"type": "program", "id": "program-1"},
                {
                    "type": "function_call",
                    "id": "fc-1",
                    "call_id": "call-1",
                    "name": "git_status",
                    "arguments": "{}",
                    "caller": {"type": "program", "caller_id": "program-1"}
                }
            ],
            "usage": {"input_tokens": 10, "output_tokens": 2, "total_tokens": 12}
        }))
        .unwrap();

    assert_eq!(response.responses_context_items.len(), 1);
    assert_eq!(response.orchestration.program_count, 1);
    assert_eq!(response.orchestration.program_tool_calls, 1);
    assert_eq!(
        response.tool_calls[0].caller,
        Some(ToolCallCaller::Program {
            caller_id: "program-1".to_string()
        })
    );
}

#[test]
fn responses_parse_response_preserves_unknown_native_items_for_stateless_replay() {
    let response = OpenAiProtocol::responses()
        .parse_response(serde_json::json!({
            "id": "resp-unknown",
            "model": "gpt-5.6-sol",
            "output": [
                {"type": "future_hosted_result", "id": "future-1", "opaque": {"value": 1}},
                {"type": "message", "id": "message-1", "content": [{"type": "output_text", "text": "done"}]}
            ],
            "usage": {"input_tokens": 1, "output_tokens": 1, "total_tokens": 2}
        }))
        .unwrap();

    assert_eq!(response.responses_context_items.len(), 1);
    assert_eq!(
        response.responses_context_items[0].kind,
        ResponsesContextItemKind::Unknown
    );
    assert_eq!(
        response.responses_context_items[0].value["type"],
        "future_hosted_result"
    );
}

#[test]
fn responses_body_writes_current_hosted_web_search_shape() {
    let mut request = request_with_effort("xhigh");
    request.tools = vec![ToolSpec::WebSearch {
        dialect: pl_protocol::HostedWebSearchDialect::OpenAiResponses,
        external_web_access: true,
        indexed_web_access: Some(true),
        filters: Some(pl_protocol::WebSearchFilters {
            allowed_domains: vec!["example.com".to_string()],
        }),
        user_location: Some(pl_protocol::WebSearchUserLocation {
            kind: pl_protocol::WebSearchUserLocationType::Approximate,
            country: Some("US".to_string()),
            region: Some("CA".to_string()),
            city: None,
            timezone: Some("America/Los_Angeles".to_string()),
        }),
        search_context_size: Some(pl_protocol::WebSearchContextSize::High),
        search_content_types: None,
    }];

    let body = OpenAiProtocol::responses().build_request_body(&request);

    assert_eq!(body["tools"][0]["type"], "web_search");
    assert_eq!(body["tools"][0]["external_web_access"], true);
    assert_eq!(body["tools"][0]["indexed_web_access"], true);
    assert_eq!(
        body["tools"][0]["filters"]["allowed_domains"][0],
        "example.com"
    );
    assert_eq!(body["tools"][0]["user_location"]["type"], "approximate");
    assert_eq!(body["tools"][0]["search_context_size"], "high");
    assert!(body["tools"][0].get("search_content_types").is_none());
}

#[test]
fn deepseek_responses_body_writes_only_native_web_search_type() {
    let mut request = request_with_effort("high");
    request.tools = vec![ToolSpec::WebSearch {
        dialect: pl_protocol::HostedWebSearchDialect::DeepSeekResponses,
        external_web_access: true,
        indexed_web_access: None,
        filters: None,
        user_location: None,
        search_context_size: None,
        search_content_types: None,
    }];
    let model = bundled_model("deepseek-v4-flash");

    let body = OpenAiProtocol::responses().build_request_body_with_model(&request, &model);

    assert_eq!(body["tools"], serde_json::json!([{"type": "web_search"}]));
}

#[test]
fn chat_body_writes_custom_grammar_tool() {
    let mut request = request_with_effort("xhigh");
    request.tools = vec![ToolSpec::custom_grammar(
        "apply_patch",
        "edit files",
        "lark",
        "start: patch",
    )];

    let body = OpenAiProtocol::chat().build_request_body(&request);

    assert_eq!(body["tools"][0]["type"], serde_json::json!("custom"));
    assert_eq!(
        body["tools"][0]["custom"]["name"],
        serde_json::json!("apply_patch")
    );
}

#[test]
fn provider_compatible_turns_custom_apply_patch_into_function_fallback() {
    let mut request = request_with_effort("high");
    request.tools = vec![ToolSpec::custom_grammar(
        "apply_patch",
        "edit files",
        "lark",
        "start: patch",
    )];

    let request = request.provider_compatible(CustomToolProjection::ToFunction);
    let body = OpenAiProtocol::chat().build_request_body(&request);

    assert_eq!(body["tools"][0]["type"], serde_json::json!("function"));
    assert_eq!(
        body["tools"][0]["function"]["parameters"]["required"],
        serde_json::json!(["input"])
    );
    let description =
        body["tools"][0]["function"]["parameters"]["properties"]["input"]["description"]
            .as_str()
            .unwrap();
    assert!(description.contains("*** Add File:"));
    assert!(description.contains("*** Update File:"));
    assert!(description.contains("---/+++ unified diff"));
    assert!(description.contains("*** File: metadata"));
    assert!(description.contains("Insert after"));
    assert!(description.contains("previous patch failed"));
    assert!(description.contains("Minimal update example:"));
    assert!(description.contains("*** Update File: notes.txt"));
    assert!(description.contains("-old line"));
    assert!(description.contains("+new line"));
}

#[test]
fn responses_parse_response_reads_custom_tool_call() {
    let response = OpenAiProtocol::responses()
        .parse_response(serde_json::json!({
            "model": "gpt-5.5",
            "output": [{
                "type": "custom_tool_call",
                "id": "ctc_1",
                "call_id": "call_1",
                "name": "apply_patch",
                "input": "*** Begin Patch\n*** End Patch"
            }],
            "usage": {
                "input_tokens": 1,
                "output_tokens": 1,
                "total_tokens": 2
            }
        }))
        .unwrap();

    assert_eq!(response.tool_calls.len(), 1);
    assert_eq!(response.tool_calls[0].name, "apply_patch");
    match &response.tool_calls[0].payload {
        ToolCallPayload::Custom { input } => {
            assert_eq!(input, "*** Begin Patch\n*** End Patch");
        }
        other => panic!("unexpected payload: {other:?}"),
    }
}

#[test]
fn responses_parse_response_canonicalizes_id_only_tool_identity() {
    let response = OpenAiProtocol::responses()
        .parse_response(serde_json::json!({
            "model": "gpt-5.5",
            "output": [
                {
                    "type": "function_call",
                    "id": "fc_1",
                    "name": "read_file",
                    "arguments": "{}"
                },
                {
                    "type": "custom_tool_call",
                    "id": "ctc_1",
                    "name": "apply_patch",
                    "input": "*** Begin Patch\n*** End Patch"
                }
            ]
        }))
        .unwrap();

    assert_eq!(response.tool_calls[0].id, "fc_1");
    assert_eq!(response.tool_calls[0].call_id, "fc_1");
    assert_eq!(response.tool_calls[1].id, "ctc_1");
    assert_eq!(response.tool_calls[1].call_id, "ctc_1");
}

#[test]
fn responses_parse_response_uses_call_id_as_missing_item_id() {
    let response = OpenAiProtocol::responses()
        .parse_response(serde_json::json!({
            "model": "gpt-5.5",
            "output": [
                {
                    "type": "function_call",
                    "call_id": "call_1",
                    "name": "read_file",
                    "arguments": "{}"
                },
                {
                    "type": "custom_tool_call",
                    "call_id": "call_2",
                    "name": "apply_patch",
                    "input": "*** Begin Patch\n*** End Patch"
                }
            ]
        }))
        .unwrap();

    assert_eq!(response.tool_calls[0].id, "call_1");
    assert_eq!(response.tool_calls[0].call_id, "call_1");
    assert_eq!(response.tool_calls[1].id, "call_2");
    assert_eq!(response.tool_calls[1].call_id, "call_2");
}

#[test]
fn responses_parse_response_rejects_empty_tool_identity() {
    let error = OpenAiProtocol::responses()
        .parse_response(serde_json::json!({
            "model": "gpt-5.5",
            "output": [{
                "type": "function_call",
                "id": "",
                "call_id": "",
                "name": "read_file",
                "arguments": "{}"
            }]
        }))
        .unwrap_err();

    assert!(matches!(
        error,
        PureError::LlmError(message) if message.contains("missing id and call_id")
    ));
}

#[test]
fn responses_id_only_tool_identity_survives_strict_history_replay() {
    let response = OpenAiProtocol::responses()
        .parse_response(serde_json::json!({
            "model": "gpt-5.5",
            "output": [{
                "type": "function_call",
                "id": "fc_1",
                "name": "read_file",
                "arguments": "{}"
            }]
        }))
        .unwrap();
    let call = ToolCallRecord {
        item_id: response.tool_calls[0].id.clone(),
        call_id: response.tool_calls[0].call_id.clone(),
        name: "read_file".to_string(),
        kind: ToolCallKind::Function,
        arguments: serde_json::json!({}),
        caller: None,
    };
    let request = CompletionRequest::builder()
        .input(context_items(vec![
            assistant_tool_call_history(call.clone()),
            tool_result_history(tool_call_result_record(&call), "ok"),
        ]))
        .build();

    let body = serde_json::to_value(
        OpenAiProtocol::responses()
            .build_request(&request, &ModelInfo::fallback("gpt-5.5"), None)
            .unwrap(),
    )
    .unwrap();

    assert_eq!(body["input"][0]["call_id"], "fc_1");
    assert_eq!(body["input"][1]["call_id"], "fc_1");
}

#[test]
fn responses_parse_response_preserves_hosted_web_search_context_items() {
    let response = OpenAiProtocol::responses()
        .parse_response(serde_json::json!({
            "id": "resp_1",
            "model": "gpt-5.5",
            "output": [
                {
                    "type": "web_search_call",
                    "id": "search_1",
                    "action": {"type": "search", "queries": ["alpha", "beta"]},
                    "results": [{"url": "https://example.com/search", "future": 1}]
                },
                {
                    "type": "web_search_call",
                    "id": "open_1",
                    "action": {"type": "open_page", "url": "https://example.com/page"}
                },
                {
                    "type": "web_search_call",
                    "id": "find_1",
                    "action": {
                        "type": "find_in_page",
                        "url": "https://example.com/page",
                        "pattern": "needle"
                    }
                },
                {
                    "type": "web_search_call",
                    "id": "future_1",
                    "action": {"type": "future_action", "opaque": true}
                }
            ]
        }))
        .unwrap();

    assert_eq!(response.responses_context_items.len(), 4);
    assert_eq!(
        response.responses_context_items[0].value["action"]["queries"],
        serde_json::json!(["alpha", "beta"])
    );
    assert_eq!(
        response.responses_context_items[0].value["results"][0]["future"],
        1
    );
    assert_eq!(
        response.responses_context_items[1].value["action"]["url"],
        "https://example.com/page"
    );
    assert_eq!(
        response.responses_context_items[2].value["action"]["pattern"],
        "needle"
    );
    assert_eq!(
        response.responses_context_items[3].value["action"]["type"],
        "future_action"
    );

    let expected = response
        .responses_context_items
        .iter()
        .map(|item| item.value.clone())
        .collect::<Vec<_>>();
    let mut request = request_with_effort("high");
    request.input = response
        .responses_context_items
        .into_iter()
        .map(|item| ModelContextItem::Responses { item })
        .collect();
    let body = OpenAiProtocol::responses().build_request_body(&request);

    assert_eq!(body["input"], serde_json::json!(expected));
}

#[test]
fn responses_parse_response_preserves_invalid_function_arguments() {
    let response = OpenAiProtocol::responses()
        .parse_response(serde_json::json!({
            "model": "gpt-5.5",
            "output": [{
                "type": "function_call",
                "id": "fc_1",
                "call_id": "call_1",
                "name": "read_file",
                "arguments": "{bad"
            }]
        }))
        .unwrap();

    let call = &response.tool_calls[0];
    assert_eq!(call.payload_text(), "{bad");
    assert_eq!(call.invalid_arguments.as_ref().unwrap().raw, "{bad");
    assert!(
        call.invalid_arguments_message()
            .unwrap()
            .contains("read_file")
    );
}

#[test]
fn chat_parse_response_reads_custom_tool_call() {
    let response = OpenAiProtocol::chat()
        .parse_response(serde_json::json!({
            "model": "gpt-5.5",
            "choices": [{
                "message": {
                    "role": "assistant",
                    "tool_calls": [{
                        "id": "call_1",
                        "type": "custom",
                        "custom": {
                            "name": "apply_patch",
                            "input": "*** Begin Patch\n*** End Patch"
                        }
                    }]
                },
                "finish_reason": "tool_calls"
            }],
            "usage": {
                "prompt_tokens": 1,
                "completion_tokens": 1,
                "total_tokens": 2
            }
        }))
        .unwrap();

    assert_eq!(response.tool_calls.len(), 1);
    assert!(matches!(
        response.tool_calls[0].payload,
        ToolCallPayload::Custom { .. }
    ));
}

#[test]
fn chat_parse_response_preserves_invalid_function_arguments() {
    let response = OpenAiProtocol::chat()
        .parse_response(serde_json::json!({
            "model": "gpt-5.5",
            "choices": [{
                "message": {
                    "role": "assistant",
                    "tool_calls": [{
                        "id": "call_1",
                        "type": "function",
                        "function": {
                            "name": "read_file",
                            "arguments": "{bad"
                        }
                    }]
                },
                "finish_reason": "tool_calls"
            }]
        }))
        .unwrap();

    let call = &response.tool_calls[0];
    assert_eq!(call.payload_text(), "{bad");
    assert_eq!(call.invalid_arguments.as_ref().unwrap().raw, "{bad");
    assert!(
        call.invalid_arguments_message()
            .unwrap()
            .contains("read_file")
    );
}

#[test]
fn responses_history_replays_custom_tool_call_and_output() {
    let call = custom_tool_call_record();
    let request = request_with_tool_history(Some(tool_call_result_record(&call)));

    let body = OpenAiProtocol::responses().build_request_body(&request);

    assert_eq!(
        body["input"][0]["type"],
        serde_json::json!("custom_tool_call")
    );
    assert!(body["input"][0]["id"].is_null());
    assert_eq!(body["input"][0]["call_id"], serde_json::json!("call_1"));
    assert_eq!(
        body["input"][1]["type"],
        serde_json::json!("custom_tool_call_output")
    );
    assert!(
        !body["input"][1]
            .as_object()
            .expect("custom tool output should serialize as object")
            .contains_key("name")
    );
}

#[test]
fn tool_result_ids_are_protocol_specific() {
    let call = custom_tool_call_record();
    let request = request_with_tool_history(Some(tool_call_result_record(&call)));

    let responses_body = OpenAiProtocol::responses().build_request_body(&request);
    let chat_body = OpenAiProtocol::chat().build_request_body(&request);

    assert_eq!(
        responses_body["input"][1]["call_id"],
        serde_json::json!("call_1")
    );
    assert!(responses_body["input"][0]["id"].is_null());
    assert_eq!(
        chat_body["messages"][1]["tool_call_id"],
        serde_json::json!("ctc_1")
    );
}

#[test]
fn function_tool_result_ids_are_protocol_specific() {
    let call = function_tool_call_record();
    let request = request_with_function_tool_history(Some(tool_call_result_record(&call)));

    let responses_body = OpenAiProtocol::responses().build_request_body(&request);
    let chat_body = OpenAiProtocol::chat().build_request_body(&request);

    assert_eq!(
        responses_body["input"][1]["call_id"],
        serde_json::json!("call_1")
    );
    assert!(responses_body["input"][0]["id"].is_null());
    assert_eq!(
        chat_body["messages"][1]["tool_call_id"],
        serde_json::json!("fc_1")
    );
}

#[test]
fn tool_result_without_typed_record_fails_request_build() {
    let mut request = request_with_function_tool_history(None);
    request.input.push(ModelContextItem::from(Message {
        presentation: Default::default(),
        role: MessageRole::Tool,
        content: MessageContent::text("ok".to_string()),
        reasoning_content: None,
        tool_calls: None,
        tool_result: None,
        metadata: HashMap::new(),
    }));

    let error = OpenAiProtocol::responses()
        .build_request(&request, &ModelInfo::fallback("gpt-5.5"), None)
        .unwrap_err();

    match error {
        PureError::LlmError(message) => {
            assert!(message.contains("tool result message missing typed tool_result record"));
        }
        other => panic!("unexpected error: {other:?}"),
    }
}

#[test]
fn missing_tool_output_fails_request_build() {
    let request = request_with_function_tool_history(None);

    let error = OpenAiProtocol::responses()
        .build_request(&request, &ModelInfo::fallback("gpt-5.5"), None)
        .unwrap_err();

    match error {
        PureError::LlmError(message) => {
            assert!(message.contains("assistant tool call fc_1 is missing tool output"));
        }
        other => panic!("unexpected error: {other:?}"),
    }
}

#[test]
fn chat_history_with_item_id_call_id_replays_on_both_endpoints() {
    // Chat Completions 解码确定性赋 call_id = item_id；Responses 回放没有
    // missing call_id 路径。
    let call = ToolCallRecord {
        item_id: "fc_1".to_string(),
        call_id: "fc_1".to_string(),
        name: "read_file".to_string(),
        kind: ToolCallKind::Function,
        arguments: serde_json::json!({ "path": "Cargo.toml" }),
        caller: None,
    };
    let request = CompletionRequest::builder()
        .input(context_items(vec![
            assistant_tool_call_history(call.clone()),
            tool_result_history(tool_call_result_record(&call), "ok"),
        ]))
        .build();

    let responses_body = OpenAiProtocol::responses()
        .build_request_body_with_model(&request, &ModelInfo::fallback("gpt-5.5"));
    let chat_body = OpenAiProtocol::chat().build_request_body(&request);

    assert_eq!(
        responses_body["input"][0]["call_id"],
        serde_json::json!("fc_1")
    );
    assert_eq!(
        responses_body["input"][1]["call_id"],
        serde_json::json!("fc_1")
    );
    assert_eq!(
        chat_body["messages"][1]["tool_call_id"],
        serde_json::json!("fc_1")
    );
}

#[test]
fn chat_then_responses_replay_pairs_call_ids_across_protocols() {
    // 第一段：Chat provider 解码工具调用，确定性赋 call_id = item_id。
    let decoded = OpenAiProtocol::chat()
        .parse_response(serde_json::json!({
            "model": "glm-5",
            "choices": [{
                "message": {
                    "role": "assistant",
                    "tool_calls": [{
                        "id": "chat-call-1",
                        "type": "function",
                        "function": {
                            "name": "read_file",
                            "arguments": "{\"path\":\"Cargo.toml\"}"
                        }
                    }]
                },
                "finish_reason": "tool_calls"
            }]
        }))
        .unwrap();
    let decoded_call = &decoded.tool_calls[0];
    assert_eq!(decoded_call.id, "chat-call-1");
    assert_eq!(decoded_call.call_id, "chat-call-1");

    // 第二段：会话保存 typed 记录后切换 Responses provider 继续对话。
    // 记录形状与 pl-core `session::tool_history::tool_call_record` 一致。
    let call = ToolCallRecord {
        item_id: decoded_call.id.clone(),
        call_id: decoded_call.call_id.clone(),
        name: decoded_call.name.clone(),
        kind: ToolCallKind::Function,
        arguments: decoded_call.arguments_for_tool(),
        caller: None,
    };
    let request = CompletionRequest::builder()
        .input(context_items(vec![
            assistant_tool_call_history(call.clone()),
            tool_result_history(tool_call_result_record(&call), "ok"),
        ]))
        .build();

    let body = serde_json::to_value(
        OpenAiProtocol::responses()
            .build_request(&request, &ModelInfo::fallback("gpt-5.5"), None)
            .expect("Responses replay must not hit a missing call_id path"),
    )
    .unwrap();

    assert_eq!(body["input"][0]["type"], "function_call");
    assert_eq!(body["input"][0]["call_id"], "chat-call-1");
    assert_eq!(body["input"][1]["type"], "function_call_output");
    assert_eq!(body["input"][1]["call_id"], "chat-call-1");
    assert_eq!(
        body["input"][0]["call_id"], body["input"][1]["call_id"],
        "assistant function_call 与 function_call_output 必须按 call_id 配对"
    );
}

#[test]
fn responses_then_chat_replay_pairs_tool_call_ids_across_protocols() {
    // 第一段：Responses provider 解码工具调用，保留独立 call_id。
    let decoded = OpenAiProtocol::responses()
        .parse_response(serde_json::json!({
            "model": "gpt-5.5",
            "output": [{
                "type": "function_call",
                "id": "fc_1",
                "call_id": "call_1",
                "name": "read_file",
                "arguments": "{\"path\":\"Cargo.toml\"}"
            }]
        }))
        .unwrap();
    let decoded_call = &decoded.tool_calls[0];
    assert_eq!(decoded_call.id, "fc_1");
    assert_eq!(decoded_call.call_id, "call_1");

    // 第二段：切换 Chat provider 继续对话；Chat wire 使用 item_id 配对。
    let call = ToolCallRecord {
        item_id: decoded_call.id.clone(),
        call_id: decoded_call.call_id.clone(),
        name: decoded_call.name.clone(),
        kind: ToolCallKind::Function,
        arguments: decoded_call.arguments_for_tool(),
        caller: None,
    };
    let request = CompletionRequest::builder()
        .input(context_items(vec![
            assistant_tool_call_history(call.clone()),
            tool_result_history(tool_call_result_record(&call), "ok"),
        ]))
        .build();

    let chat_body = OpenAiProtocol::chat().build_request_body(&request);
    let responses_body = serde_json::to_value(
        OpenAiProtocol::responses()
            .build_request(&request, &ModelInfo::fallback("glm-5"), None)
            .expect("Responses replay must keep the provider call_id"),
    )
    .unwrap();

    assert_eq!(chat_body["messages"][0]["tool_calls"][0]["id"], "fc_1");
    assert_eq!(chat_body["messages"][1]["tool_call_id"], "fc_1");
    assert_eq!(responses_body["input"][0]["call_id"], "call_1");
    assert_eq!(responses_body["input"][1]["call_id"], "call_1");
}

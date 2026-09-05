use std::collections::HashMap;

use pl_protocol::{
    AttachmentModality, ContentPart, MessageContent, ModelContextItem, Result, ToolMediaContext,
};

use crate::completion::{CompletionRequest, PreparedContentPart, PreparedContentSource};
use crate::model::info::{MediaMixPolicy, MediaRepresentation, MediaWireFormat, ModelInfo};

use super::protocol_error;
pub(super) fn message_content_text(content: &MessageContent) -> String {
    content.text_value()
}

#[derive(Debug, Default)]
pub(super) struct MediaRepresentationPlan {
    representations: HashMap<AttachmentModality, MediaRepresentation>,
    wires: HashMap<AttachmentModality, MediaWireFormat>,
}

impl MediaRepresentationPlan {
    pub(super) fn for_request(request: &CompletionRequest, model: &ModelInfo) -> Result<Self> {
        let mut attachment_ids = HashMap::<AttachmentModality, Vec<String>>::new();
        let mut seen = HashMap::<String, AttachmentModality>::new();

        for item in &request.input {
            let tool_media;
            let content = match item {
                ModelContextItem::Message { message }
                | ModelContextItem::ToolResult { message, .. } => &message.content,
                ModelContextItem::ToolMedia { items } => {
                    tool_media = tool_media_content(items);
                    &tool_media
                }
                ModelContextItem::Compaction { .. } | ModelContextItem::Responses { .. } => {
                    continue;
                }
            };
            for part in &content.parts {
                let ContentPart::Attachment {
                    attachment_id,
                    modality,
                    ..
                } = part
                else {
                    continue;
                };
                if let Some(existing) = seen.insert(attachment_id.clone(), *modality) {
                    if existing != *modality {
                        return Err(protocol_error(format!(
                            "model={} attachment modality conflict for attachment {attachment_id}",
                            model.slug
                        )));
                    }
                    continue;
                }
                attachment_ids
                    .entry(*modality)
                    .or_default()
                    .push(attachment_id.clone());
            }
        }

        if model.binding.request.media_mix_policy == MediaMixPolicy::SingleModality
            && attachment_ids.len() > 1
        {
            return Err(protocol_error(format!(
                "model={} rejects mixed media modalities",
                model.slug
            )));
        }

        let mut representations = HashMap::new();
        let mut wires = HashMap::new();
        for (modality, ids) in attachment_ids {
            let media = ids
                .iter()
                .map(|attachment_id| prepared_content(request, attachment_id, modality, model))
                .collect::<Result<Vec<_>>>()?;
            let profile = model
                .binding
                .request
                .media_profile(model_modality(modality))
                .ok_or_else(|| {
                    protocol_error(format!(
                        "model={} modality={} count={} has no media request profile",
                        model.slug,
                        modality_label(modality),
                        media.len()
                    ))
                })?;
            let candidates = if media
                .iter()
                .all(|item| item.sources.iter().all(is_snapshot_source))
            {
                &profile.replay
            } else {
                &profile.first_send
            };
            let representation = candidates
                .iter()
                .copied()
                .find(|candidate| {
                    media
                        .iter()
                        .all(|item| has_representation(item, *candidate))
                })
                .ok_or_else(|| {
                    protocol_error(format!(
                        "model={} modality={} count={} has no common media representation",
                        model.slug,
                        modality_label(modality),
                        media.len()
                    ))
                })?;
            tracing::info!(
                model = %model.slug,
                modality = modality_label(modality),
                representation = representation_label(representation),
                count = media.len(),
                "planned media request representation"
            );
            representations.insert(modality, representation);
            wires.insert(modality, profile.wire);
        }

        Ok(Self {
            representations,
            wires,
        })
    }

    fn representation(&self, modality: AttachmentModality) -> Result<MediaRepresentation> {
        self.representations.get(&modality).copied().ok_or_else(|| {
            protocol_error(format!(
                "modality={} was not planned before serialization",
                modality_label(modality)
            ))
        })
    }

    pub(super) fn wire(&self, modality: AttachmentModality) -> Result<MediaWireFormat> {
        self.wires.get(&modality).copied().ok_or_else(|| {
            protocol_error(format!(
                "modality={} has no planned provider wire mapping",
                modality_label(modality)
            ))
        })
    }
}

pub(super) fn tool_media_content(items: &[ToolMediaContext]) -> MessageContent {
    let mut parts = Vec::with_capacity(items.len().saturating_mul(2));
    for item in items {
        parts.push(ContentPart::Text {
            text: format!(
                "Image from view_image call {}: {}",
                item.call_id, item.label
            ),
        });
        parts.push(ContentPart::Attachment {
            attachment_id: item.attachment.id.clone(),
            modality: item.attachment.modality,
            media_type: item.attachment.media_type.clone(),
            filename: item.attachment.filename.clone(),
        });
    }
    MessageContent::new(parts)
}

pub(super) fn media_url(
    attachment_id: &str,
    media_type: &str,
    modality: AttachmentModality,
    prepared_content: &[PreparedContentPart],
    plan: &MediaRepresentationPlan,
) -> Result<String> {
    let media = prepared_content
        .iter()
        .find(|media| media.attachment_id == attachment_id)
        .ok_or_else(|| {
            protocol_error(format!(
                "attachment {attachment_id} was not prepared before model request"
            ))
        })?;
    if media.modality != modality {
        return Err(protocol_error(format!(
            "attachment {attachment_id} prepared modality does not match message content"
        )));
    }
    let representation = plan.representation(modality)?;
    let source = media
        .sources
        .iter()
        .find(|source| source_matches(source, representation))
        .ok_or_else(|| {
            protocol_error(format!(
                "attachment {attachment_id} is missing its planned media representation"
            ))
        })?;
    match source {
        PreparedContentSource::DataUrl { base64 } => {
            let actual_media_type = if media_type.is_empty() {
                media.media_type.as_str()
            } else {
                media_type
            };
            Ok(format!("data:{actual_media_type};base64,{base64}"))
        }
        PreparedContentSource::RemoteUrl { url } => Ok(url.clone()),
        PreparedContentSource::ProviderFile { .. } => Err(protocol_error(
            "provider file cannot be serialized as a URL content part",
        )),
    }
}

fn prepared_content<'a>(
    request: &'a CompletionRequest,
    attachment_id: &str,
    modality: AttachmentModality,
    model: &ModelInfo,
) -> Result<&'a PreparedContentPart> {
    let matching = request
        .prepared_content
        .iter()
        .filter(|media| media.attachment_id == attachment_id)
        .collect::<Vec<_>>();
    let [media] = matching.as_slice() else {
        return Err(protocol_error(format!(
            "model={} modality={} attachment {attachment_id} must have exactly one prepared payload",
            model.slug,
            modality_label(modality)
        )));
    };
    if media.modality != modality {
        return Err(protocol_error(format!(
            "model={} attachment {attachment_id} prepared modality does not match message content",
            model.slug
        )));
    }
    Ok(media)
}

fn has_representation(media: &PreparedContentPart, representation: MediaRepresentation) -> bool {
    media
        .sources
        .iter()
        .any(|source| source_matches(source, representation))
}

fn source_matches(source: &PreparedContentSource, representation: MediaRepresentation) -> bool {
    matches!(
        (source, representation),
        (
            PreparedContentSource::RemoteUrl { .. },
            MediaRepresentation::RemoteUrl
        ) | (
            PreparedContentSource::ProviderFile { .. },
            MediaRepresentation::ProviderFile
        ) | (
            PreparedContentSource::DataUrl { .. },
            MediaRepresentation::DataUrl
        )
    )
}

fn is_snapshot_source(source: &PreparedContentSource) -> bool {
    matches!(source, PreparedContentSource::DataUrl { .. })
}

fn model_modality(modality: AttachmentModality) -> crate::model::ModelModality {
    match modality {
        AttachmentModality::Image => crate::model::ModelModality::Image,
        AttachmentModality::Video => crate::model::ModelModality::Video,
        AttachmentModality::File => crate::model::ModelModality::File,
    }
}

fn modality_label(modality: AttachmentModality) -> &'static str {
    match modality {
        AttachmentModality::Image => "image",
        AttachmentModality::Video => "video",
        AttachmentModality::File => "file",
    }
}

fn representation_label(representation: MediaRepresentation) -> &'static str {
    match representation {
        MediaRepresentation::RemoteUrl => "remoteUrl",
        MediaRepresentation::ProviderFile => "providerFile",
        MediaRepresentation::DataUrl => "dataUrl",
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;
    use crate::model::info::ModelMediaInputProfile;
    use crate::runtime::openai::OpenAiProtocol;
    use crate::runtime::openai::test_support::{
        bundled_model, context_items, image_message, image_prepared_content,
    };
    use pl_protocol::{
        AttachmentModality, ContentPart, Message, MessageContent, MessageRole, ToolCallKind,
        ToolCallRecord, ToolMediaContext, ToolResultReceipt, ToolResultRecord,
    };
    use std::collections::HashMap;

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

    fn media_message(
        attachment_id: &str,
        modality: AttachmentModality,
        media_type: &str,
    ) -> Message {
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
        let mut model = ModelInfo::compatible("profiled-media-model");
        model
            .binding
            .set_transport(crate::model::ModelTransportProfile::responses_http());
        model.binding.request.media = vec![ModelMediaInputProfile {
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

    #[test]
    fn responses_maps_image_parts_to_input_image() {
        let request = CompletionRequest::builder()
            .input(context_items(vec![image_message()]))
            .prepared_content(image_prepared_content())
            .build();

        let model = bundled_model("gpt-6-astra");
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

        let body = OpenAiProtocol::responses().build_request_body_with_model(
            &request,
            &bundled_model("deepseek-v4-flash-vision-exp"),
        );

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
            .build_request_for_fixture(&request, &bundled_model("glm-5.3-flash"), None)
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
                .build_request_for_fixture(&request, &profiled_media_model(model_modality), None)
                .expect_err("Responses must fail closed for video and file attachments");

            assert!(error.to_string().contains("Responses does not support"));
        }
    }
}

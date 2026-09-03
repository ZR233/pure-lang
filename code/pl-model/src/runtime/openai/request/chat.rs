use pl_protocol::{
    ContentPart, MessageContent, MessageRole, Result, ToolCallKind, ToolCallRecord, ToolSpec,
};
use serde::Serialize;

use crate::completion::CompletionRequest;
use crate::model::info::{MaxTokensField, MediaWireFormat, ModelInfo};

use super::body::ToolFormatBody;
use super::content::{
    MediaRepresentationPlan, media_url, message_content_text, tool_media_content,
};
use super::protocol_error;
use super::tool_history::{record_arguments_text, record_custom_input};
#[derive(Debug, Clone, Serialize)]
pub(super) struct ChatRequestBody {
    model: String,
    messages: Vec<ChatMessage>,
    stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<ChatTool>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_choice: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    parallel_tool_calls: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_completion_tokens: Option<u64>,
}

impl ChatRequestBody {
    pub(super) fn from_request(request: &CompletionRequest, model: &ModelInfo) -> Result<Self> {
        let mut messages = Vec::new();
        let media_plan = MediaRepresentationPlan::for_request(request, model)?;

        if let Some(instructions) = &request.instructions {
            messages.push(ChatMessage::System {
                content: instructions.clone(),
            });
        }

        for item in &request.input {
            if let pl_protocol::ModelContextItem::ToolMedia { items } = item {
                let content = tool_media_content(items);
                messages.push(ChatMessage::User {
                    content: chat_content_for_user(
                        &content,
                        &request.prepared_content,
                        &media_plan,
                    )?,
                });
                continue;
            }
            let msg = match item {
                pl_protocol::ModelContextItem::Message { message }
                | pl_protocol::ModelContextItem::ToolResult { message, .. } => message,
                pl_protocol::ModelContextItem::ToolMedia { .. } => unreachable!(),
                pl_protocol::ModelContextItem::Compaction { .. } => {
                    return Err(protocol_error(
                        "Chat Completions cannot consume remote compaction items",
                    ));
                }
                pl_protocol::ModelContextItem::Responses { .. } => {
                    return Err(protocol_error(
                        "Chat Completions cannot consume Responses native items",
                    ));
                }
            };
            match msg.role {
                MessageRole::Assistant if msg.tool_calls.is_some() => {
                    let text = message_content_text(&msg.content);
                    messages.push(ChatMessage::Assistant {
                        content: (!text.is_empty()).then_some(text),
                        reasoning_content: msg.reasoning_content.clone(),
                        tool_calls: msg
                            .tool_calls
                            .as_ref()
                            .map(|calls| calls.iter().map(ChatMessageToolCall::from).collect()),
                    });
                }
                MessageRole::Tool => {
                    let record = msg.tool_result.as_ref().ok_or_else(|| {
                        protocol_error("tool result message missing typed tool_result record")
                    })?;
                    messages.push(ChatMessage::Tool {
                        tool_call_id: record.item_id.clone(),
                        content: message_content_text(&msg.content),
                    });
                }
                MessageRole::System => messages.push(ChatMessage::System {
                    content: message_content_text(&msg.content),
                }),
                MessageRole::User => messages.push(ChatMessage::User {
                    content: chat_content_for_user(
                        &msg.content,
                        &request.prepared_content,
                        &media_plan,
                    )?,
                }),
                MessageRole::Assistant => messages.push(ChatMessage::Assistant {
                    content: Some(message_content_text(&msg.content)),
                    reasoning_content: msg.reasoning_content.clone(),
                    tool_calls: None,
                }),
            }
        }

        let tools = if request.tools.is_empty() {
            None
        } else {
            Some(
                request
                    .tools
                    .iter()
                    .map(ChatTool::from_schema)
                    .collect::<Result<Vec<_>>>()?,
            )
        };
        let tool_choice = tools.as_ref().map(|_| request.tool_choice.clone());

        let (max_tokens, max_completion_tokens) = match model.request_profile.max_tokens_field {
            MaxTokensField::MaxTokens => (request.max_tokens, None),
            MaxTokensField::MaxCompletionTokens => (None, request.max_tokens),
        };

        Ok(Self {
            model: model
                .request_profile
                .api_model
                .clone()
                .unwrap_or_else(|| model.slug.clone()),
            messages,
            stream: true,
            tools,
            tool_choice,
            parallel_tool_calls: model
                .request_profile
                .chat_parallel_tool_calls
                .then_some(request.parallel_tool_calls),
            temperature: request.temperature,
            max_tokens,
            max_completion_tokens,
        })
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "role", rename_all = "lowercase")]
enum ChatMessage {
    System {
        content: String,
    },
    User {
        content: ChatMessageContent,
    },
    Assistant {
        content: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        reasoning_content: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        tool_calls: Option<Vec<ChatMessageToolCall>>,
    },
    Tool {
        tool_call_id: String,
        content: String,
    },
}

#[derive(Debug, Clone, Serialize)]
#[serde(untagged)]
enum ChatMessageContent {
    Text(String),
    Parts(Vec<ChatContentPart>),
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum ChatContentPart {
    Text { text: String },
    ImageUrl { image_url: ChatImageUrl },
    VideoUrl { video_url: ChatMediaUrl },
    FileUrl { file_url: ChatMediaUrl },
}

#[derive(Debug, Clone, Serialize)]
struct ChatImageUrl {
    url: String,
}

#[derive(Debug, Clone, Serialize)]
struct ChatMediaUrl {
    url: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum ChatMessageToolCall {
    Function {
        id: String,
        function: ChatFunctionCall,
    },
    Custom {
        id: String,
        custom: ChatCustomToolCall,
    },
}

impl From<&ToolCallRecord> for ChatMessageToolCall {
    fn from(record: &ToolCallRecord) -> Self {
        match record.kind {
            ToolCallKind::Function => Self::Function {
                id: record.item_id.clone(),
                function: ChatFunctionCall {
                    name: record.name.clone(),
                    arguments: record_arguments_text(&record.arguments),
                },
            },
            ToolCallKind::Custom => Self::Custom {
                id: record.item_id.clone(),
                custom: ChatCustomToolCall {
                    name: record.name.clone(),
                    input: record_custom_input(&record.arguments),
                },
            },
        }
    }
}

#[derive(Debug, Clone, Serialize)]
struct ChatFunctionCall {
    name: String,
    arguments: String,
}

#[derive(Debug, Clone, Serialize)]
struct ChatCustomToolCall {
    name: String,
    input: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum ChatTool {
    Function { function: ChatToolFunction },
    Custom { custom: ChatToolCustom },
}

impl ChatTool {
    fn from_schema(tool: &ToolSpec) -> Result<Self> {
        let tool = match tool {
            ToolSpec::Function {
                name,
                description,
                input_schema,
                ..
            } => Self::Function {
                function: ChatToolFunction {
                    name: name.clone(),
                    description: description.clone(),
                    parameters: input_schema.clone(),
                },
            },
            ToolSpec::Custom {
                name,
                description,
                format,
                ..
            } => Self::Custom {
                custom: ChatToolCustom {
                    name: name.clone(),
                    description: description.clone(),
                    format: ToolFormatBody::from(format),
                },
            },
            ToolSpec::ProgrammaticToolCalling | ToolSpec::WebSearch { .. } => {
                return Err(protocol_error(
                    "Responses-only tools cannot be consumed by Chat Completions",
                ));
            }
        };
        Ok(tool)
    }
}

#[derive(Debug, Clone, Serialize)]
struct ChatToolFunction {
    name: String,
    description: String,
    parameters: serde_json::Value,
}

#[derive(Debug, Clone, Serialize)]
struct ChatToolCustom {
    name: String,
    description: String,
    format: ToolFormatBody,
}

fn chat_content_for_user(
    content: &MessageContent,
    prepared_content: &[crate::completion::PreparedContentPart],
    media_plan: &MediaRepresentationPlan,
) -> Result<ChatMessageContent> {
    let mut has_media = false;
    let mut chat_parts = Vec::new();
    for part in &content.parts {
        match part {
            ContentPart::Text { text } => {
                chat_parts.push(ChatContentPart::Text { text: text.clone() });
            }
            ContentPart::Attachment {
                attachment_id,
                modality,
                media_type,
                ..
            } => {
                has_media = true;
                let url = media_url(
                    attachment_id,
                    media_type,
                    *modality,
                    prepared_content,
                    media_plan,
                )?;
                chat_parts.push(match media_plan.wire(*modality)? {
                    MediaWireFormat::ChatImageUrl => ChatContentPart::ImageUrl {
                        image_url: ChatImageUrl { url },
                    },
                    MediaWireFormat::ChatVideoUrl => ChatContentPart::VideoUrl {
                        video_url: ChatMediaUrl { url },
                    },
                    MediaWireFormat::ChatFileUrl => ChatContentPart::FileUrl {
                        file_url: ChatMediaUrl { url },
                    },
                    MediaWireFormat::ResponsesInputImage => {
                        return Err(protocol_error(
                            "Responses input_image wire cannot be serialized by Chat Completions",
                        ));
                    }
                });
            }
        }
    }
    if has_media {
        Ok(ChatMessageContent::Parts(chat_parts))
    } else {
        Ok(ChatMessageContent::Text(message_content_text(content)))
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;
    use crate::completion::ReasoningConfig;
    use crate::completion::tool_schema::CustomToolProjection;
    use crate::runtime::openai::OpenAiProtocol;
    use crate::runtime::openai::test_support::{
        bundled_model, context_items, image_message, image_prepared_content, request_with_effort,
    };
    use pl_protocol::{Message, MessageContent};
    use std::collections::HashMap;

    #[test]
    fn chat_parallel_tool_calls_wire_follows_request_after_profile_opt_in() {
        for (profile_opt_in, request_flag, expected) in [
            (false, true, None),
            (true, true, Some(true)),
            (true, false, Some(false)),
        ] {
            let mut request = request_with_effort("high");
            request.parallel_tool_calls = request_flag;
            let mut model = ModelInfo::fallback("chat-compatible");
            model.request_profile.chat_parallel_tool_calls = profile_opt_in;

            let body = OpenAiProtocol::chat().build_request_body_with_model(&request, &model);

            match expected {
                Some(expected) => {
                    assert_eq!(body["parallel_tool_calls"], serde_json::json!(expected))
                }
                None => assert!(body.get("parallel_tool_calls").is_none()),
            }
        }
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
        let body = OpenAiProtocol::chat()
            .build_request_body_with_model(&request_with_effort("max"), &model);

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
        let body = OpenAiProtocol::chat()
            .build_request_body_with_model(&request_with_effort("none"), &model);

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
}

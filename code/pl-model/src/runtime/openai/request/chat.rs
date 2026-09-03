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

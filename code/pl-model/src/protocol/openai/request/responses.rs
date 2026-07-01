use pl_protocol::{
    ContentPart, MessageContent, MessageRole, Result, TOOL_CALLS_METADATA_KEY, ToolCallKind,
    ToolResultMetadata,
};
use serde::Serialize;

use crate::request::{
    CompletionRequest, ReasoningConfig, ReasoningSummary, ToolCall, ToolCallPayload, ToolSchema,
};

use super::body::ToolFormatBody;
use super::content::{data_url, message_content_text};
use super::protocol_error;
use super::tool_history::parse_tool_calls_from_metadata;
#[derive(Debug, Clone, Serialize)]
pub(super) struct ResponsesRequestBody {
    model: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    instructions: Option<String>,
    input: Vec<ResponsesInputItem>,
    stream: bool,
    tool_choice: String,
    parallel_tool_calls: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<ResponsesTool>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_output_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    reasoning: Option<ResponsesReasoning>,
}

impl ResponsesRequestBody {
    pub(super) fn from_request(request: &CompletionRequest) -> Result<Self> {
        let mut input = Vec::new();

        for msg in &request.messages {
            match msg.role {
                MessageRole::Assistant if msg.metadata.contains_key(TOOL_CALLS_METADATA_KEY) => {
                    let text = message_content_text(&msg.content);
                    if !text.is_empty() {
                        input.push(ResponsesInputItem::message(
                            ResponsesRole::Assistant,
                            vec![ResponsesContent::OutputText { text }],
                        ));
                    }
                    if let Some(tool_calls) = parse_tool_calls_from_metadata(&msg.metadata)? {
                        input.extend(
                            tool_calls
                                .into_iter()
                                .map(ResponsesInputItem::from_tool_call),
                        );
                    }
                }
                MessageRole::Tool => {
                    let metadata =
                        ToolResultMetadata::from_metadata(&msg.metadata).map_err(protocol_error)?;
                    let call_id = metadata
                        .tool_call_call_id
                        .clone()
                        .unwrap_or_else(|| metadata.tool_call_id.clone());
                    let output = message_content_text(&msg.content);
                    match metadata.tool_call_kind {
                        ToolCallKind::Function => {
                            input.push(ResponsesInputItem::FunctionCallOutput { call_id, output });
                        }
                        ToolCallKind::Custom => {
                            input
                                .push(ResponsesInputItem::CustomToolCallOutput { call_id, output });
                        }
                    }
                }
                MessageRole::System | MessageRole::User | MessageRole::Assistant => {
                    input.push(ResponsesInputItem::message(
                        ResponsesRole::from_message_role(msg.role),
                        responses_content_for_message(&msg.content, msg.role)?,
                    ));
                }
            }
        }

        let tools = (!request.tools.is_empty()).then(|| {
            request
                .tools
                .iter()
                .map(ResponsesTool::from_schema)
                .collect()
        });

        Ok(Self {
            model: request.model.clone(),
            instructions: request.instructions.clone(),
            input,
            stream: true,
            tool_choice: request.tool_choice.clone(),
            parallel_tool_calls: request.parallel_tool_calls,
            tools,
            temperature: request.temperature,
            max_output_tokens: request.max_tokens,
            reasoning: request
                .reasoning
                .as_ref()
                .map(ResponsesReasoning::from_config),
        })
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum ResponsesInputItem {
    Message {
        role: ResponsesRole,
        content: Vec<ResponsesContent>,
    },
    FunctionCall {
        #[serde(skip_serializing_if = "Option::is_none")]
        id: Option<String>,
        name: String,
        arguments: String,
        call_id: String,
    },
    FunctionCallOutput {
        call_id: String,
        output: String,
    },
    CustomToolCall {
        #[serde(skip_serializing_if = "Option::is_none")]
        id: Option<String>,
        name: String,
        input: String,
        call_id: String,
    },
    CustomToolCallOutput {
        call_id: String,
        output: String,
    },
}

impl ResponsesInputItem {
    fn message(role: ResponsesRole, content: Vec<ResponsesContent>) -> Self {
        Self::Message { role, content }
    }

    fn from_tool_call(tool_call: ToolCall) -> Self {
        match tool_call.payload {
            ToolCallPayload::Function { arguments } => Self::FunctionCall {
                id: None,
                name: tool_call.name,
                arguments: serde_json::to_string(&arguments).unwrap_or_default(),
                call_id: tool_call.call_id.unwrap_or(tool_call.id),
            },
            ToolCallPayload::Custom { input } => Self::CustomToolCall {
                id: None,
                name: tool_call.name,
                input,
                call_id: tool_call.call_id.unwrap_or(tool_call.id),
            },
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "lowercase")]
enum ResponsesRole {
    System,
    User,
    Assistant,
    Tool,
}

impl ResponsesRole {
    fn from_message_role(role: MessageRole) -> Self {
        match role {
            MessageRole::System => Self::System,
            MessageRole::User => Self::User,
            MessageRole::Assistant => Self::Assistant,
            MessageRole::Tool => Self::Tool,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum ResponsesContent {
    InputText { text: String },
    InputImage { image_url: String },
    OutputText { text: String },
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum ResponsesTool {
    Function {
        name: String,
        description: String,
        parameters: serde_json::Value,
    },
    Custom {
        name: String,
        description: String,
        format: ToolFormatBody,
    },
}

impl ResponsesTool {
    fn from_schema(tool: &ToolSchema) -> Self {
        match tool {
            ToolSchema::Function {
                name,
                description,
                input_schema,
            } => Self::Function {
                name: name.clone(),
                description: description.clone(),
                parameters: input_schema.clone(),
            },
            ToolSchema::Custom {
                name,
                description,
                format,
            } => Self::Custom {
                name: name.clone(),
                description: description.clone(),
                format: ToolFormatBody::from_format(format),
            },
        }
    }
}

#[derive(Debug, Clone, Serialize)]
struct ResponsesReasoning {
    #[serde(skip_serializing_if = "Option::is_none")]
    summary: Option<ResponsesReasoningSummary>,
}

impl ResponsesReasoning {
    fn from_config(reasoning: &ReasoningConfig) -> Self {
        Self {
            summary: reasoning
                .summary
                .and_then(ResponsesReasoningSummary::from_summary),
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "lowercase")]
enum ResponsesReasoningSummary {
    Auto,
}

impl ResponsesReasoningSummary {
    fn from_summary(summary: ReasoningSummary) -> Option<Self> {
        match summary {
            ReasoningSummary::Auto | ReasoningSummary::Enabled => Some(Self::Auto),
            ReasoningSummary::Disabled => None,
        }
    }
}

fn responses_content_for_message(
    content: &MessageContent,
    role: MessageRole,
) -> Result<Vec<ResponsesContent>> {
    match content {
        MessageContent::Text(text) => {
            let part = match role {
                MessageRole::Assistant => ResponsesContent::OutputText { text: text.clone() },
                MessageRole::System | MessageRole::User | MessageRole::Tool => {
                    ResponsesContent::InputText { text: text.clone() }
                }
            };
            Ok(vec![part])
        }
        MessageContent::MultiPart(parts) => {
            let mut content = Vec::new();
            for part in parts {
                match part {
                    ContentPart::Text { text } => {
                        if role == MessageRole::Assistant {
                            content.push(ResponsesContent::OutputText { text: text.clone() });
                        } else {
                            content.push(ResponsesContent::InputText { text: text.clone() });
                        }
                    }
                    ContentPart::Image {
                        source, media_type, ..
                    } => {
                        content.push(ResponsesContent::InputImage {
                            image_url: data_url(source, media_type)?,
                        });
                    }
                }
            }
            Ok(content)
        }
    }
}

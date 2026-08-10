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
    store: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    previous_response_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    prompt_cache_key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    reasoning: Option<ResponsesReasoning>,
}

impl ResponsesRequestBody {
    pub(super) fn from_request(request: &CompletionRequest) -> Result<Self> {
        let mut input = Vec::new();

        for item in &request.input {
            let msg = match item {
                pl_protocol::ModelContextItem::Message { message }
                | pl_protocol::ModelContextItem::ToolResult { message, .. } => message,
                pl_protocol::ModelContextItem::Compaction { encrypted_content } => {
                    input.push(ResponsesInputItem::typed(
                        ResponsesTypedInputItem::Compaction {
                            encrypted_content: encrypted_content.clone(),
                        },
                    ));
                    continue;
                }
                pl_protocol::ModelContextItem::Responses { item } => {
                    input.push(ResponsesInputItem::Native(item.value.clone()));
                    continue;
                }
            };
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
                            input.push(ResponsesInputItem::typed(
                                ResponsesTypedInputItem::FunctionCallOutput {
                                    call_id,
                                    output,
                                    caller: metadata.tool_call_caller,
                                },
                            ));
                        }
                        ToolCallKind::Custom => {
                            input.push(ResponsesInputItem::typed(
                                ResponsesTypedInputItem::CustomToolCallOutput {
                                    call_id,
                                    output,
                                    caller: metadata.tool_call_caller,
                                },
                            ));
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
            store: request.store,
            previous_response_id: request.previous_response_id.clone(),
            prompt_cache_key: request.prompt_cache_key.clone(),
            reasoning: request
                .reasoning
                .as_ref()
                .map(ResponsesReasoning::from_config),
        })
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(untagged)]
enum ResponsesInputItem {
    Typed(ResponsesTypedInputItem),
    Native(serde_json::Value),
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum ResponsesTypedInputItem {
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
        #[serde(skip_serializing_if = "Option::is_none")]
        caller: Option<pl_protocol::ToolCallCaller>,
    },
    FunctionCallOutput {
        call_id: String,
        output: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        caller: Option<pl_protocol::ToolCallCaller>,
    },
    CustomToolCall {
        #[serde(skip_serializing_if = "Option::is_none")]
        id: Option<String>,
        name: String,
        input: String,
        call_id: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        caller: Option<pl_protocol::ToolCallCaller>,
    },
    CustomToolCallOutput {
        call_id: String,
        output: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        caller: Option<pl_protocol::ToolCallCaller>,
    },
    Compaction {
        encrypted_content: String,
    },
}

impl ResponsesInputItem {
    fn typed(item: ResponsesTypedInputItem) -> Self {
        Self::Typed(item)
    }

    fn message(role: ResponsesRole, content: Vec<ResponsesContent>) -> Self {
        Self::typed(ResponsesTypedInputItem::Message { role, content })
    }

    fn from_tool_call(tool_call: ToolCall) -> Self {
        let invalid_arguments = tool_call.invalid_arguments;
        match tool_call.payload {
            ToolCallPayload::Function { arguments } => {
                Self::typed(ResponsesTypedInputItem::FunctionCall {
                    id: None,
                    name: tool_call.name,
                    arguments: invalid_arguments
                        .map(|invalid| invalid.raw)
                        .unwrap_or_else(|| serde_json::to_string(&arguments).unwrap_or_default()),
                    call_id: tool_call.call_id.unwrap_or(tool_call.id),
                    caller: tool_call.caller,
                })
            }
            ToolCallPayload::Custom { input } => {
                Self::typed(ResponsesTypedInputItem::CustomToolCall {
                    id: None,
                    name: tool_call.name,
                    input,
                    call_id: tool_call.call_id.unwrap_or(tool_call.id),
                    caller: tool_call.caller,
                })
            }
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "lowercase")]
enum ResponsesRole {
    Developer,
    User,
    Assistant,
    Tool,
}

impl ResponsesRole {
    fn from_message_role(role: MessageRole) -> Self {
        match role {
            MessageRole::System => Self::Developer,
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
        #[serde(skip_serializing_if = "is_false")]
        defer_loading: bool,
        #[serde(skip_serializing_if = "Vec::is_empty")]
        allowed_callers: Vec<crate::ToolCallerMode>,
        #[serde(skip_serializing_if = "Option::is_none")]
        output_schema: Option<serde_json::Value>,
    },
    Custom {
        name: String,
        description: String,
        format: ToolFormatBody,
        #[serde(skip_serializing_if = "is_false")]
        defer_loading: bool,
        #[serde(skip_serializing_if = "Vec::is_empty")]
        allowed_callers: Vec<crate::ToolCallerMode>,
        #[serde(skip_serializing_if = "Option::is_none")]
        output_schema: Option<serde_json::Value>,
    },
    Namespace {
        name: String,
        description: String,
        tools: Vec<ResponsesTool>,
    },
    ToolSearch,
    ProgrammaticToolCalling,
    WebSearch {
        external_web_access: bool,
        #[serde(skip_serializing_if = "Option::is_none")]
        indexed_web_access: Option<bool>,
        #[serde(skip_serializing_if = "Option::is_none")]
        filters: Option<crate::WebSearchFilters>,
        #[serde(skip_serializing_if = "Option::is_none")]
        user_location: Option<crate::WebSearchUserLocation>,
        #[serde(skip_serializing_if = "Option::is_none")]
        search_context_size: Option<crate::WebSearchContextSize>,
        #[serde(skip_serializing_if = "Option::is_none")]
        search_content_types: Option<Vec<String>>,
    },
}

impl ResponsesTool {
    fn from_schema(tool: &ToolSchema) -> Self {
        match tool {
            ToolSchema::Function {
                name,
                description,
                input_schema,
                defer_loading,
                allowed_callers,
                output_schema,
            } => Self::Function {
                name: name.clone(),
                description: description.clone(),
                parameters: input_schema.clone(),
                defer_loading: *defer_loading,
                allowed_callers: allowed_callers.clone(),
                output_schema: output_schema.clone(),
            },
            ToolSchema::Custom {
                name,
                description,
                format,
                defer_loading,
                allowed_callers,
                output_schema,
            } => Self::Custom {
                name: name.clone(),
                description: description.clone(),
                format: ToolFormatBody::from_format(format),
                defer_loading: *defer_loading,
                allowed_callers: allowed_callers.clone(),
                output_schema: output_schema.clone(),
            },
            ToolSchema::Namespace {
                name,
                description,
                tools,
            } => Self::Namespace {
                name: name.clone(),
                description: description.clone(),
                tools: tools.iter().map(Self::from_schema).collect(),
            },
            ToolSchema::ToolSearch => Self::ToolSearch,
            ToolSchema::ProgrammaticToolCalling => Self::ProgrammaticToolCalling,
            ToolSchema::WebSearch {
                external_web_access,
                indexed_web_access,
                filters,
                user_location,
                search_context_size,
                search_content_types,
            } => Self::WebSearch {
                external_web_access: *external_web_access,
                indexed_web_access: *indexed_web_access,
                filters: filters.clone(),
                user_location: user_location.clone(),
                search_context_size: *search_context_size,
                search_content_types: search_content_types.clone(),
            },
        }
    }
}

fn is_false(value: &bool) -> bool {
    !*value
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

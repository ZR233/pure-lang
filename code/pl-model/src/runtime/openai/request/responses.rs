use pl_protocol::{
    AttachmentModality, ContentPart, MessageContent, MessageRole, Result, ToolCallKind,
    ToolCallRecord,
};
use serde::Serialize;

use crate::completion::{CompletionRequest, ReasoningConfig, ReasoningSummary, ToolSpec};
use crate::model::info::MediaWireFormat;

use super::body::ToolFormatBody;
use super::content::{MediaRepresentationPlan, media_url, message_content_text};
use super::protocol_error;
use super::tool_history::{record_arguments_text, record_custom_input, tool_callers_by_call_id};
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
    store: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    prompt_cache_key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    reasoning: Option<ResponsesReasoning>,
}

impl ResponsesRequestBody {
    pub(super) fn from_request(
        request: &CompletionRequest,
        model: &crate::ModelInfo,
        prompt_cache_key: Option<&str>,
    ) -> Result<Self> {
        let mut input = Vec::new();
        let media_plan = MediaRepresentationPlan::for_request(request, model)?;
        let history = request
            .input
            .iter()
            .filter_map(|item| item.as_message())
            .cloned()
            .collect::<Vec<_>>();
        let tool_callers = tool_callers_by_call_id(&history);

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
                MessageRole::Assistant if msg.tool_calls.is_some() => {
                    let text = message_content_text(&msg.content);
                    if !text.is_empty() {
                        input.push(ResponsesInputItem::message(
                            ResponsesRole::Assistant,
                            vec![ResponsesContent::OutputText { text }],
                        ));
                    }
                    if let Some(tool_calls) = msg.tool_calls.as_ref() {
                        input.extend(tool_calls.iter().map(ResponsesInputItem::from));
                    }
                }
                MessageRole::Tool => {
                    let record = msg.tool_result.as_ref().ok_or_else(|| {
                        protocol_error("tool result message missing typed tool_result record")
                    })?;
                    let caller = tool_callers.get(&record.call_id).cloned();
                    let output = message_content_text(&msg.content);
                    match record.kind {
                        ToolCallKind::Function => {
                            input.push(ResponsesInputItem::typed(
                                ResponsesTypedInputItem::FunctionCallOutput {
                                    call_id: record.call_id.clone(),
                                    output,
                                    caller,
                                },
                            ));
                        }
                        ToolCallKind::Custom => {
                            input.push(ResponsesInputItem::typed(
                                ResponsesTypedInputItem::CustomToolCallOutput {
                                    call_id: record.call_id.clone(),
                                    output,
                                    caller,
                                },
                            ));
                        }
                    }
                }
                MessageRole::System | MessageRole::User | MessageRole::Assistant => {
                    input.push(ResponsesInputItem::message(
                        ResponsesRole::from(msg.role),
                        responses_content_for_message(
                            &msg.content,
                            msg.role,
                            &request.prepared_content,
                            &media_plan,
                        )?,
                    ));
                }
            }
        }

        let tools = (!request.tools.is_empty())
            .then(|| request.tools.iter().map(ResponsesTool::from).collect());

        Ok(Self {
            model: model
                .request_profile
                .api_model
                .clone()
                .unwrap_or_else(|| model.slug.clone()),
            instructions: request.instructions.clone(),
            input,
            stream: true,
            tool_choice: request.tool_choice.clone(),
            parallel_tool_calls: request.parallel_tool_calls,
            tools,
            temperature: request.temperature,
            store: false,
            prompt_cache_key: prompt_cache_key.map(ToString::to_string),
            reasoning: request.reasoning.as_ref().map(ResponsesReasoning::from),
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
}

impl From<&ToolCallRecord> for ResponsesInputItem {
    fn from(record: &ToolCallRecord) -> Self {
        let caller = record.caller.clone();
        match record.kind {
            ToolCallKind::Function => Self::typed(ResponsesTypedInputItem::FunctionCall {
                id: None,
                name: record.name.clone(),
                arguments: record_arguments_text(&record.arguments),
                call_id: record.call_id.clone(),
                caller,
            }),
            ToolCallKind::Custom => Self::typed(ResponsesTypedInputItem::CustomToolCall {
                id: None,
                name: record.name.clone(),
                input: record_custom_input(&record.arguments),
                call_id: record.call_id.clone(),
                caller,
            }),
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

impl From<MessageRole> for ResponsesRole {
    fn from(role: MessageRole) -> Self {
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
        #[serde(skip_serializing_if = "Vec::is_empty")]
        allowed_callers: Vec<crate::ToolCallerMode>,
        #[serde(skip_serializing_if = "Option::is_none")]
        output_schema: Option<serde_json::Value>,
    },
    Custom {
        name: String,
        description: String,
        format: ToolFormatBody,
        #[serde(skip_serializing_if = "Vec::is_empty")]
        allowed_callers: Vec<crate::ToolCallerMode>,
        #[serde(skip_serializing_if = "Option::is_none")]
        output_schema: Option<serde_json::Value>,
    },
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

impl From<&ToolSpec> for ResponsesTool {
    fn from(tool: &ToolSpec) -> Self {
        match tool {
            ToolSpec::Function {
                name,
                description,
                input_schema,
                allowed_callers,
                output_schema,
            } => Self::Function {
                name: name.clone(),
                description: description.clone(),
                parameters: input_schema.clone(),
                allowed_callers: allowed_callers.clone(),
                output_schema: output_schema.clone(),
            },
            ToolSpec::Custom {
                name,
                description,
                format,
                allowed_callers,
                output_schema,
            } => Self::Custom {
                name: name.clone(),
                description: description.clone(),
                format: ToolFormatBody::from(format),
                allowed_callers: allowed_callers.clone(),
                output_schema: output_schema.clone(),
            },
            ToolSpec::ProgrammaticToolCalling => Self::ProgrammaticToolCalling,
            ToolSpec::WebSearch {
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

#[derive(Debug, Clone, Serialize)]
struct ResponsesReasoning {
    #[serde(skip_serializing_if = "Option::is_none")]
    summary: Option<ResponsesReasoningSummary>,
}

impl From<&ReasoningConfig> for ResponsesReasoning {
    fn from(reasoning: &ReasoningConfig) -> Self {
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
    prepared_content: &[crate::completion::PreparedContentPart],
    media_plan: &MediaRepresentationPlan,
) -> Result<Vec<ResponsesContent>> {
    let mut response_content = Vec::new();
    for part in &content.parts {
        match part {
            ContentPart::Text { text } => {
                if role == MessageRole::Assistant {
                    response_content.push(ResponsesContent::OutputText { text: text.clone() });
                } else {
                    response_content.push(ResponsesContent::InputText { text: text.clone() });
                }
            }
            ContentPart::Attachment {
                attachment_id,
                modality,
                media_type,
                ..
            } => match modality {
                AttachmentModality::Image => {
                    if media_plan.wire(*modality)? != MediaWireFormat::ResponsesInputImage {
                        return Err(protocol_error(
                            "Chat media wire cannot be serialized by Responses",
                        ));
                    }
                    response_content.push(ResponsesContent::InputImage {
                        image_url: media_url(
                            attachment_id,
                            media_type,
                            *modality,
                            prepared_content,
                            media_plan,
                        )?,
                    });
                }
                AttachmentModality::File | AttachmentModality::Video => {
                    return Err(protocol_error(format!(
                        "Responses does not support {:?} attachments",
                        modality
                    )));
                }
            },
        }
    }
    Ok(response_content)
}

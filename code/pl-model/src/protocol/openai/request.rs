use std::collections::{HashMap, VecDeque};

use pl_protocol::{
    ContentPart, ImageSource, Message, MessageContent, MessageRole, PureError, Result,
    TOOL_CALLS_METADATA_KEY, ToolCallHistoryMetadata, ToolCallKind, ToolMetadataCompatibility,
    ToolResultMetadata,
};
use serde::Serialize;
use serde_json::{Map, Value};

use super::OpenAiEndpoint;
use crate::model_info::ModelInfo;
use crate::request::{
    CompletionRequest, ReasoningConfig, ReasoningSummary, ToolCall, ToolCallPayload, ToolFormat,
    ToolSchema,
};

#[derive(Debug, Clone, Serialize)]
#[serde(untagged)]
pub(crate) enum OpenAiRequestBody {
    Responses(Map<String, Value>),
    Chat(Map<String, Value>),
}

pub(crate) fn build_openai_request_body(
    endpoint: OpenAiEndpoint,
    request: &CompletionRequest,
    model: &ModelInfo,
) -> Result<OpenAiRequestBody> {
    validate_tool_history(&request.messages, endpoint)?;
    match endpoint {
        OpenAiEndpoint::Responses => {
            let mut body = to_object_map(&ResponsesRequestBody::from_request(request)?)?;
            finalize_body(&mut body, model, &request.reasoning);
            Ok(OpenAiRequestBody::Responses(body))
        }
        OpenAiEndpoint::ChatCompletions => {
            let mut body = to_object_map(&ChatRequestBody::from_request(request)?)?;
            finalize_body(&mut body, model, &request.reasoning);
            Ok(OpenAiRequestBody::Chat(body))
        }
    }
}

#[derive(Debug, Clone, Serialize)]
struct ResponsesRequestBody {
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
    fn from_request(request: &CompletionRequest) -> Result<Self> {
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
                    let metadata = ToolResultMetadata::from_metadata(
                        &msg.metadata,
                        ToolMetadataCompatibility::LegacyMissingKindAsFunction,
                    )
                    .map_err(protocol_error)?;
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
#[serde(tag = "type", rename_all = "snake_case")]
enum ToolFormatBody {
    Text,
    Grammar { syntax: String, definition: String },
}

impl ToolFormatBody {
    fn from_format(format: &ToolFormat) -> Self {
        match format {
            ToolFormat::Text => Self::Text,
            ToolFormat::Grammar { syntax, definition } => Self::Grammar {
                syntax: syntax.clone(),
                definition: definition.clone(),
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

#[derive(Debug, Clone, Serialize)]
struct ChatRequestBody {
    model: String,
    messages: Vec<ChatMessage>,
    stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<ChatTool>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_choice: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_tokens: Option<u64>,
}

impl ChatRequestBody {
    fn from_request(request: &CompletionRequest) -> Result<Self> {
        let mut messages = Vec::new();

        if let Some(instructions) = &request.instructions {
            messages.push(ChatMessage::System {
                content: instructions.clone(),
            });
        }

        for msg in &request.messages {
            match msg.role {
                MessageRole::Assistant if msg.metadata.contains_key(TOOL_CALLS_METADATA_KEY) => {
                    let text = message_content_text(&msg.content);
                    messages.push(ChatMessage::Assistant {
                        content: (!text.is_empty()).then_some(text),
                        reasoning_content: msg.reasoning_content.clone(),
                        tool_calls: parse_tool_calls_from_metadata(&msg.metadata)?.map(|calls| {
                            calls
                                .into_iter()
                                .map(ChatMessageToolCall::from_tool_call)
                                .collect()
                        }),
                    });
                }
                MessageRole::Tool => {
                    let metadata = ToolResultMetadata::from_metadata(
                        &msg.metadata,
                        ToolMetadataCompatibility::LegacyMissingKindAsFunction,
                    )
                    .map_err(protocol_error)?;
                    messages.push(ChatMessage::Tool {
                        tool_call_id: metadata.tool_call_id,
                        content: message_content_text(&msg.content),
                    });
                }
                MessageRole::System => messages.push(ChatMessage::System {
                    content: message_content_text(&msg.content),
                }),
                MessageRole::User => messages.push(ChatMessage::User {
                    content: chat_content_for_user(&msg.content)?,
                }),
                MessageRole::Assistant => messages.push(ChatMessage::Assistant {
                    content: Some(message_content_text(&msg.content)),
                    reasoning_content: msg.reasoning_content.clone(),
                    tool_calls: None,
                }),
            }
        }

        let tools = (!request.tools.is_empty())
            .then(|| request.tools.iter().map(ChatTool::from_schema).collect());
        let tool_choice = tools.as_ref().map(|_| request.tool_choice.clone());

        Ok(Self {
            model: request.model.clone(),
            messages,
            stream: true,
            tools,
            tool_choice,
            temperature: request.temperature,
            max_tokens: request.max_tokens,
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
}

#[derive(Debug, Clone, Serialize)]
struct ChatImageUrl {
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

impl ChatMessageToolCall {
    fn from_tool_call(tool_call: ToolCall) -> Self {
        match tool_call.payload {
            ToolCallPayload::Function { arguments } => Self::Function {
                id: tool_call.id,
                function: ChatFunctionCall {
                    name: tool_call.name,
                    arguments: serde_json::to_string(&arguments).unwrap_or_default(),
                },
            },
            ToolCallPayload::Custom { input } => Self::Custom {
                id: tool_call.id,
                custom: ChatCustomToolCall {
                    name: tool_call.name,
                    input,
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
    fn from_schema(tool: &ToolSchema) -> Self {
        match tool {
            ToolSchema::Function {
                name,
                description,
                input_schema,
            } => Self::Function {
                function: ChatToolFunction {
                    name: name.clone(),
                    description: description.clone(),
                    parameters: input_schema.clone(),
                },
            },
            ToolSchema::Custom {
                name,
                description,
                format,
            } => Self::Custom {
                custom: ChatToolCustom {
                    name: name.clone(),
                    description: description.clone(),
                    format: ToolFormatBody::from_format(format),
                },
            },
        }
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

/// 把强类型请求序列化为 JSON 对象 Map，供 base body 与 parameter wire 注入。
fn to_object_map<T: Serialize>(value: &T) -> Result<Map<String, Value>> {
    let serialized = serde_json::to_value(value)?;
    match serialized {
        Value::Object(map) => Ok(map),
        _ => Err(protocol_error(
            "typed request body must serialize to a JSON object",
        )),
    }
}

/// 注入 base body 后应用 parameter wire，完成请求体动态字段组装。
fn finalize_body(
    body: &mut Map<String, Value>,
    model: &ModelInfo,
    reasoning: &Option<ReasoningConfig>,
) {
    merge_base_body(body, &model.request_profile.body);
    apply_parameters(body, model, reasoning);
}

/// 深合并 base body 到请求体：对象字段递归合并，其余字段覆盖。
fn merge_base_body(target: &mut Map<String, Value>, source: &Map<String, Value>) {
    for (key, value) in source {
        match (target.get_mut(key), value) {
            (Some(Value::Object(target_inner)), Value::Object(source_inner)) => {
                merge_base_body(target_inner, source_inner);
            }
            _ => {
                target.insert(key.clone(), value.clone());
            }
        }
    }
}

/// 遍历模型声明的可调参数，对用户选中的值应用 wire 写入请求体。
fn apply_parameters(
    body: &mut Map<String, Value>,
    model: &ModelInfo,
    reasoning: &Option<ReasoningConfig>,
) {
    for parameter in &model.parameters {
        let selected = if parameter.name == "effort" {
            reasoning
                .as_ref()
                .and_then(|config| config.effort.as_deref())
        } else {
            None
        };
        if let Some(value) = selected
            && let Some(wire) = parameter.wire_for(value)
        {
            wire.apply_to(body);
        }
    }
}

fn message_content_text(content: &MessageContent) -> String {
    match content {
        MessageContent::Text(text) => text.clone(),
        MessageContent::MultiPart(parts) => parts
            .iter()
            .filter_map(|part| match part {
                ContentPart::Text { text } => Some(text.as_str()),
                ContentPart::Image { .. } => None,
            })
            .collect::<Vec<_>>()
            .join("\n"),
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

fn chat_content_for_user(content: &MessageContent) -> Result<ChatMessageContent> {
    match content {
        MessageContent::Text(text) => Ok(ChatMessageContent::Text(text.clone())),
        MessageContent::MultiPart(parts) => {
            let mut has_image = false;
            let mut chat_parts = Vec::new();
            for part in parts {
                match part {
                    ContentPart::Text { text } => {
                        chat_parts.push(ChatContentPart::Text { text: text.clone() });
                    }
                    ContentPart::Image {
                        source, media_type, ..
                    } => {
                        has_image = true;
                        chat_parts.push(ChatContentPart::ImageUrl {
                            image_url: ChatImageUrl {
                                url: data_url(source, media_type)?,
                            },
                        });
                    }
                }
            }
            if has_image {
                Ok(ChatMessageContent::Parts(chat_parts))
            } else {
                Ok(ChatMessageContent::Text(message_content_text(content)))
            }
        }
    }
}

fn data_url(source: &ImageSource, media_type: &str) -> Result<String> {
    match source {
        ImageSource::InlineBase64 { data } => Ok(format!("data:{media_type};base64,{data}")),
        ImageSource::Attachment { attachment_id } => Err(protocol_error(format!(
            "image attachment {attachment_id} was not materialized before model request"
        ))),
    }
}

/// 从 metadata 中解析 tool_calls（由 CoreSession::push_assistant_tool_calls 写入）。
fn parse_tool_calls_from_metadata(
    metadata: &HashMap<String, String>,
) -> Result<Option<Vec<ToolCall>>> {
    ToolCallHistoryMetadata::from_metadata(metadata)
        .map(|metadata| {
            serde_json::from_str(&metadata.tool_calls_json).map_err(|error| {
                protocol_error(format!("invalid assistant tool_calls metadata: {error}"))
            })
        })
        .transpose()
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ExpectedToolOutput {
    tool_call_id: String,
    call_id: Option<String>,
    tool_call_kind: ToolCallKind,
}

fn validate_tool_history(messages: &[Message], endpoint: OpenAiEndpoint) -> Result<()> {
    let mut expected_outputs = VecDeque::new();

    for message in messages {
        match message.role {
            MessageRole::Assistant if message.metadata.contains_key(TOOL_CALLS_METADATA_KEY) => {
                let tool_calls = parse_tool_calls_from_metadata(&message.metadata)?
                    .ok_or_else(|| protocol_error("assistant tool_calls metadata missing"))?;
                for tool_call in tool_calls {
                    if tool_call.id.is_empty() {
                        return Err(protocol_error("assistant tool call has empty id"));
                    }
                    if tool_call.call_id.as_ref().is_some_and(String::is_empty) {
                        return Err(protocol_error("assistant tool call has empty call_id"));
                    }
                    if endpoint == OpenAiEndpoint::Responses && tool_call.call_id.is_none() {
                        return Err(protocol_error(format!(
                            "assistant tool call {} missing call_id for Responses history replay",
                            tool_call.id
                        )));
                    }
                    let tool_call_kind = tool_call.kind();
                    expected_outputs.push_back(ExpectedToolOutput {
                        tool_call_id: tool_call.id,
                        call_id: tool_call.call_id,
                        tool_call_kind,
                    });
                }
            }
            MessageRole::Tool => {
                let metadata = ToolResultMetadata::from_metadata(
                    &message.metadata,
                    ToolMetadataCompatibility::LegacyMissingKindAsFunction,
                )
                .map_err(protocol_error)?;
                let expected = expected_outputs.pop_front().ok_or_else(|| {
                    protocol_error("tool result has no preceding assistant tool call")
                })?;
                if metadata.tool_call_id != expected.tool_call_id {
                    return Err(protocol_error(format!(
                        "tool result id {} does not match assistant tool call id {}",
                        metadata.tool_call_id, expected.tool_call_id
                    )));
                }
                if endpoint == OpenAiEndpoint::Responses
                    && metadata.tool_call_call_id.as_deref() != expected.call_id.as_deref()
                {
                    return Err(protocol_error(format!(
                        "tool result call_id {:?} does not match assistant tool call call_id {:?}",
                        metadata.tool_call_call_id, expected.call_id
                    )));
                }
                if metadata.tool_call_kind != expected.tool_call_kind {
                    return Err(protocol_error(format!(
                        "tool result kind {} does not match assistant tool call kind {}",
                        metadata.tool_call_kind.as_str(),
                        expected.tool_call_kind.as_str()
                    )));
                }
            }
            MessageRole::System | MessageRole::User | MessageRole::Assistant => {}
        }
    }

    if let Some(expected) = expected_outputs.front() {
        return Err(protocol_error(format!(
            "assistant tool call {} is missing tool output",
            expected.tool_call_id
        )));
    }

    Ok(())
}

fn protocol_error(message: impl Into<String>) -> PureError {
    PureError::LlmError(format!("OpenAI request protocol error: {}", message.into()))
}

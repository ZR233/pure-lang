use std::collections::{HashMap, VecDeque};

use pl_protocol::{
    ContentPart, ImageSource, Message, MessageContent, MessageRole, PureError, Result,
    TOOL_CALLS_METADATA_KEY, ToolCallHistoryMetadata, ToolCallKind, ToolMetadataCompatibility,
    ToolResultMetadata,
};
use serde::Serialize;
use serde_json::Map;
use serde_json::Value;

use crate::model_info::ModelInfo;
#[cfg(test)]
use crate::request::CompletionResponse;
use crate::request::{
    CompletionRequest, ReasoningConfig, ReasoningSummary, ToolCall, ToolCallPayload, ToolFormat,
    ToolSchema,
};

#[cfg(test)]
mod response;
pub(crate) mod sse;

#[cfg(test)]
use response::{chat_parse_response, responses_parse_response};

/// OpenAI API 协议端点。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OpenAiEndpoint {
    Responses,
    ChatCompletions,
}

/// OpenAI 协议编解码器。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct OpenAiProtocol {
    endpoint: OpenAiEndpoint,
}

impl OpenAiProtocol {
    pub(crate) fn responses() -> Self {
        Self {
            endpoint: OpenAiEndpoint::Responses,
        }
    }

    pub(crate) fn chat() -> Self {
        Self {
            endpoint: OpenAiEndpoint::ChatCompletions,
        }
    }

    pub(crate) fn build_request(
        &self,
        request: &CompletionRequest,
        model: &ModelInfo,
    ) -> Result<OpenAiRequestBody> {
        validate_tool_history(&request.messages, self.endpoint)?;
        match self.endpoint {
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

    #[cfg(test)]
    fn build_request_body(&self, request: &CompletionRequest) -> serde_json::Value {
        let fallback = ModelInfo::fallback(&request.model);
        self.build_request_body_with_model(request, &fallback)
    }

    #[cfg(test)]
    fn build_request_body_with_model(
        &self,
        request: &CompletionRequest,
        model: &ModelInfo,
    ) -> serde_json::Value {
        serde_json::to_value(
            self.build_request(request, model)
                .expect("typed provider request should build"),
        )
        .expect("typed provider request should serialize")
    }

    #[cfg(test)]
    fn parse_response(&self, body: serde_json::Value) -> Result<CompletionResponse> {
        match self.endpoint {
            OpenAiEndpoint::Responses => responses_parse_response(body),
            OpenAiEndpoint::ChatCompletions => chat_parse_response(body),
        }
    }

    pub(crate) fn parse_stream_events(
        &self,
        event: &sse::SseStreamEvent,
    ) -> Result<Vec<sse::StreamEvent>> {
        Ok(sse::process_sse_events(event))
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(untagged)]
pub(crate) enum OpenAiRequestBody {
    Responses(Map<String, Value>),
    Chat(Map<String, Value>),
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct ResponsesRequestBody {
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
pub(crate) struct ChatRequestBody {
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

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use pl_protocol::{ContentPart, ImageSource, Message, MessageContent, MessageRole};
    use pretty_assertions::assert_eq;

    use super::*;
    use crate::request::{ReasoningConfig, ReasoningSummary};

    fn text_message(role: MessageRole, content: &str) -> Message {
        Message {
            role,
            content: MessageContent::Text(content.to_string()),
            reasoning_content: None,
            metadata: HashMap::new(),
        }
    }

    fn image_message() -> Message {
        Message {
            role: MessageRole::User,
            content: MessageContent::MultiPart(vec![
                ContentPart::Text {
                    text: "describe".to_string(),
                },
                ContentPart::Image {
                    source: ImageSource::InlineBase64 {
                        data: "aGVsbG8=".to_string(),
                    },
                    media_type: "image/png".to_string(),
                    filename: Some("sample.png".to_string()),
                },
            ]),
            reasoning_content: None,
            metadata: HashMap::new(),
        }
    }

    fn request_with_effort(effort: &str) -> CompletionRequest {
        CompletionRequest {
            model: "gpt-5.5".to_string(),
            instructions: None,
            messages: vec![text_message(MessageRole::User, "hello")],
            tools: Vec::new(),
            tool_choice: "auto".to_string(),
            parallel_tool_calls: true,
            temperature: None,
            max_tokens: None,
            reasoning: Some(ReasoningConfig {
                effort: Some(effort.to_string()),
                summary: None,
            }),
            stream: true,
            timeline: None,
        }
    }

    #[test]
    fn responses_use_top_level_instructions_and_chat_prepends_system_message() {
        let request = CompletionRequest {
            model: "gpt-5.5".to_string(),
            instructions: Some("base".to_string()),
            messages: vec![
                text_message(MessageRole::System, "developer"),
                text_message(MessageRole::User, "user context"),
                text_message(MessageRole::User, "real prompt"),
            ],
            tools: Vec::new(),
            tool_choice: "auto".to_string(),
            parallel_tool_calls: false,
            temperature: None,
            max_tokens: None,
            reasoning: None,
            stream: true,
            timeline: None,
        };

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
            vec!["system", "user", "user"],
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
        let request = CompletionRequest {
            model: "gpt-5.5".to_string(),
            instructions: None,
            messages: vec![image_message()],
            tools: Vec::new(),
            tool_choice: "auto".to_string(),
            parallel_tool_calls: false,
            temperature: None,
            max_tokens: None,
            reasoning: None,
            stream: true,
            timeline: None,
        };

        let body = OpenAiProtocol::responses().build_request_body(&request);

        assert_eq!(body["input"][0]["content"][0]["type"], "input_text");
        assert_eq!(body["input"][0]["content"][0]["text"], "describe");
        assert_eq!(body["input"][0]["content"][1]["type"], "input_image");
        assert_eq!(
            body["input"][0]["content"][1]["image_url"],
            "data:image/png;base64,aGVsbG8="
        );
    }

    #[test]
    fn chat_maps_image_parts_to_content_array() {
        let request = CompletionRequest {
            model: "glm-5v".to_string(),
            instructions: None,
            messages: vec![image_message()],
            tools: Vec::new(),
            tool_choice: "auto".to_string(),
            parallel_tool_calls: false,
            temperature: None,
            max_tokens: None,
            reasoning: None,
            stream: true,
            timeline: None,
        };

        let body = OpenAiProtocol::chat().build_request_body(&request);

        assert_eq!(body["messages"][0]["content"][0]["type"], "text");
        assert_eq!(body["messages"][0]["content"][1]["type"], "image_url");
        assert_eq!(
            body["messages"][0]["content"][1]["image_url"]["url"],
            "data:image/png;base64,aGVsbG8="
        );
    }

    fn request_with_tool_history(tool_metadata: HashMap<String, String>) -> CompletionRequest {
        let calls = vec![ToolCall::custom(
            "ctc_1",
            "apply_patch",
            "*** Begin Patch\n*** End Patch",
            Some("call_1".to_string()),
        )];
        let mut assistant_metadata = HashMap::new();
        assistant_metadata.insert(
            "tool_calls".to_string(),
            serde_json::to_string(&calls).unwrap(),
        );
        CompletionRequest {
            model: "gpt-5.5".to_string(),
            instructions: None,
            messages: vec![
                Message {
                    role: MessageRole::Assistant,
                    content: MessageContent::Text(String::new()),
                    reasoning_content: None,
                    metadata: assistant_metadata,
                },
                Message {
                    role: MessageRole::Tool,
                    content: MessageContent::Text("ok".to_string()),
                    reasoning_content: None,
                    metadata: tool_metadata,
                },
            ],
            tools: Vec::new(),
            tool_choice: "auto".to_string(),
            parallel_tool_calls: false,
            temperature: None,
            max_tokens: None,
            reasoning: None,
            stream: true,
            timeline: None,
        }
    }

    fn request_with_function_tool_history(
        tool_metadata: HashMap<String, String>,
    ) -> CompletionRequest {
        let calls = vec![ToolCall::function(
            "fc_1",
            "read_file",
            serde_json::json!({ "path": "Cargo.toml" }),
            Some("call_1".to_string()),
        )];
        let mut assistant_metadata = HashMap::new();
        assistant_metadata.insert(
            "tool_calls".to_string(),
            serde_json::to_string(&calls).unwrap(),
        );
        CompletionRequest {
            model: "gpt-5.5".to_string(),
            instructions: None,
            messages: vec![
                Message {
                    role: MessageRole::Assistant,
                    content: MessageContent::Text(String::new()),
                    reasoning_content: None,
                    metadata: assistant_metadata,
                },
                Message {
                    role: MessageRole::Tool,
                    content: MessageContent::Text("ok".to_string()),
                    reasoning_content: None,
                    metadata: tool_metadata,
                },
            ],
            tools: Vec::new(),
            tool_choice: "auto".to_string(),
            parallel_tool_calls: false,
            temperature: None,
            max_tokens: None,
            reasoning: None,
            stream: true,
            timeline: None,
        }
    }

    fn bundled_model(slug: &str) -> ModelInfo {
        crate::default_models::default_models()
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
    fn chat_body_writes_assistant_reasoning_content() {
        let mut request = request_with_effort("high");
        request.messages = vec![Message {
            role: MessageRole::Assistant,
            content: MessageContent::Text("9.11 更大。".to_string()),
            reasoning_content: Some("比较小数位。".to_string()),
            metadata: HashMap::new(),
        }];

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
    fn chat_parse_response_reads_cached_prompt_tokens() {
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
                "usage": {
                    "prompt_tokens": 100,
                    "completion_tokens": 20,
                    "total_tokens": 120,
                    "prompt_tokens_details": {
                        "cached_tokens": 40
                    }
                }
            }))
            .unwrap();

        assert_eq!(response.usage.cached_prompt_tokens, 40);
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
                        "cached_tokens": 55
                    }
                }
            }))
            .unwrap();

        assert_eq!(response.usage.cached_prompt_tokens, 55);
    }

    #[test]
    fn responses_body_writes_custom_grammar_tool() {
        let mut request = request_with_effort("xhigh");
        request.tools = vec![ToolSchema::custom_grammar(
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
    fn chat_body_writes_custom_grammar_tool() {
        let mut request = request_with_effort("xhigh");
        request.tools = vec![ToolSchema::custom_grammar(
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
        request.tools = vec![ToolSchema::custom_grammar(
            "apply_patch",
            "edit files",
            "lark",
            "start: patch",
        )];

        let request = request.provider_compatible(false);
        let body = OpenAiProtocol::chat().build_request_body(&request);

        assert_eq!(body["tools"][0]["type"], serde_json::json!("function"));
        assert_eq!(
            body["tools"][0]["function"]["parameters"]["required"],
            serde_json::json!(["patch"])
        );
        let description =
            body["tools"][0]["function"]["parameters"]["properties"]["patch"]["description"]
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
    fn responses_history_replays_custom_tool_call_and_output() {
        let mut tool_metadata = HashMap::new();
        tool_metadata.insert("tool_call_id".to_string(), "ctc_1".to_string());
        tool_metadata.insert("tool_call_call_id".to_string(), "call_1".to_string());
        tool_metadata.insert("tool_call_kind".to_string(), "custom".to_string());
        tool_metadata.insert("tool_name".to_string(), "apply_patch".to_string());
        let request = request_with_tool_history(tool_metadata);

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
        let mut tool_metadata = HashMap::new();
        tool_metadata.insert("tool_call_id".to_string(), "ctc_1".to_string());
        tool_metadata.insert("tool_call_call_id".to_string(), "call_1".to_string());
        tool_metadata.insert("tool_call_kind".to_string(), "custom".to_string());
        tool_metadata.insert("tool_name".to_string(), "apply_patch".to_string());
        let request = request_with_tool_history(tool_metadata);

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
        let mut tool_metadata = HashMap::new();
        tool_metadata.insert("tool_call_id".to_string(), "fc_1".to_string());
        tool_metadata.insert("tool_call_call_id".to_string(), "call_1".to_string());
        tool_metadata.insert("tool_call_kind".to_string(), "function".to_string());
        tool_metadata.insert("tool_name".to_string(), "read_file".to_string());
        let request = request_with_function_tool_history(tool_metadata);

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
    fn unknown_tool_call_kind_fails_request_build() {
        let mut tool_metadata = HashMap::new();
        tool_metadata.insert("tool_call_id".to_string(), "fc_1".to_string());
        tool_metadata.insert("tool_call_kind".to_string(), "mystery".to_string());
        tool_metadata.insert("tool_name".to_string(), "read_file".to_string());
        let request = request_with_function_tool_history(tool_metadata);

        let error = OpenAiProtocol::responses()
            .build_request(&request, &ModelInfo::fallback(&request.model))
            .unwrap_err();

        match error {
            PureError::LlmError(message) => {
                assert!(message.contains("unknown tool_call_kind: mystery"));
            }
            other => panic!("unexpected error: {other:?}"),
        }
    }

    #[test]
    fn missing_tool_output_fails_request_build() {
        let calls = vec![ToolCall::function(
            "fc_1",
            "read_file",
            serde_json::json!({ "path": "Cargo.toml" }),
            Some("call_1".to_string()),
        )];
        let mut assistant_metadata = HashMap::new();
        assistant_metadata.insert(
            "tool_calls".to_string(),
            serde_json::to_string(&calls).unwrap(),
        );
        let request = CompletionRequest {
            model: "gpt-5.5".to_string(),
            instructions: None,
            messages: vec![Message {
                role: MessageRole::Assistant,
                content: MessageContent::Text(String::new()),
                reasoning_content: None,
                metadata: assistant_metadata,
            }],
            tools: Vec::new(),
            tool_choice: "auto".to_string(),
            parallel_tool_calls: false,
            temperature: None,
            max_tokens: None,
            reasoning: None,
            stream: true,
            timeline: None,
        };

        let error = OpenAiProtocol::responses()
            .build_request(&request, &ModelInfo::fallback(&request.model))
            .unwrap_err();

        match error {
            PureError::LlmError(message) => {
                assert!(message.contains("assistant tool call fc_1 is missing tool output"));
            }
            other => panic!("unexpected error: {other:?}"),
        }
    }

    #[test]
    fn legacy_function_tool_result_without_kind_replays_as_function() {
        let mut tool_metadata = HashMap::new();
        tool_metadata.insert("tool_call_id".to_string(), "fc_1".to_string());
        tool_metadata.insert("tool_call_call_id".to_string(), "call_1".to_string());
        tool_metadata.insert("tool_name".to_string(), "read_file".to_string());
        let request = request_with_function_tool_history(tool_metadata);

        let body = OpenAiProtocol::responses().build_request_body(&request);

        assert_eq!(
            body["input"][1]["type"],
            serde_json::json!("function_call_output")
        );
    }

    #[test]
    fn responses_history_requires_call_id_but_chat_uses_tool_call_id() {
        let calls = vec![ToolCall::function(
            "fc_1",
            "read_file",
            serde_json::json!({ "path": "Cargo.toml" }),
            None,
        )];
        let mut assistant_metadata = HashMap::new();
        assistant_metadata.insert(
            "tool_calls".to_string(),
            serde_json::to_string(&calls).unwrap(),
        );
        let mut tool_metadata = HashMap::new();
        tool_metadata.insert("tool_call_id".to_string(), "fc_1".to_string());
        tool_metadata.insert("tool_call_kind".to_string(), "function".to_string());
        tool_metadata.insert("tool_name".to_string(), "read_file".to_string());
        let request = CompletionRequest {
            model: "gpt-5.5".to_string(),
            instructions: None,
            messages: vec![
                Message {
                    role: MessageRole::Assistant,
                    content: MessageContent::Text(String::new()),
                    reasoning_content: None,
                    metadata: assistant_metadata,
                },
                Message {
                    role: MessageRole::Tool,
                    content: MessageContent::Text("ok".to_string()),
                    reasoning_content: None,
                    metadata: tool_metadata,
                },
            ],
            tools: Vec::new(),
            tool_choice: "auto".to_string(),
            parallel_tool_calls: false,
            temperature: None,
            max_tokens: None,
            reasoning: None,
            stream: true,
            timeline: None,
        };

        let responses_error = OpenAiProtocol::responses()
            .build_request(&request, &ModelInfo::fallback(&request.model))
            .unwrap_err();
        let chat_body = OpenAiProtocol::chat().build_request_body(&request);

        match responses_error {
            PureError::LlmError(message) => {
                assert!(message.contains("missing call_id for Responses history replay"));
            }
            other => panic!("unexpected error: {other:?}"),
        }
        assert_eq!(
            chat_body["messages"][1]["tool_call_id"],
            serde_json::json!("fc_1")
        );
    }
}

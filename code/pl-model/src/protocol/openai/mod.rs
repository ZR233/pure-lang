use std::collections::{HashMap, VecDeque};

use pl_protocol::{
    Message, MessageContent, MessageRole, PureError, Result, TOOL_CALLS_METADATA_KEY,
    ToolCallHistoryMetadata, ToolCallKind, ToolMetadataCompatibility, ToolResultMetadata,
};
use serde::Serialize;

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
    chat_reasoning: ChatReasoningStyle,
}

impl OpenAiProtocol {
    pub(crate) fn responses() -> Self {
        Self {
            endpoint: OpenAiEndpoint::Responses,
            chat_reasoning: ChatReasoningStyle::Plain,
        }
    }

    pub(crate) fn chat(chat_reasoning: ChatReasoningStyle) -> Self {
        Self {
            endpoint: OpenAiEndpoint::ChatCompletions,
            chat_reasoning,
        }
    }

    pub(crate) fn build_request(&self, request: &CompletionRequest) -> Result<OpenAiRequestBody> {
        validate_tool_history(&request.messages, self.endpoint)?;
        match self.endpoint {
            OpenAiEndpoint::Responses => Ok(OpenAiRequestBody::Responses(
                ResponsesRequestBody::from_request(request)?,
            )),
            OpenAiEndpoint::ChatCompletions => Ok(OpenAiRequestBody::Chat(
                ChatRequestBody::from_request(request, self.chat_reasoning)?,
            )),
        }
    }

    #[cfg(test)]
    fn build_request_body(&self, request: &CompletionRequest) -> serde_json::Value {
        serde_json::to_value(
            self.build_request(request)
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

    pub(crate) fn parse_stream_event(
        &self,
        event: &sse::SseStreamEvent,
    ) -> Result<Option<sse::StreamEvent>> {
        Ok(sse::process_sse_event(event))
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(untagged)]
pub(crate) enum OpenAiRequestBody {
    Responses(ResponsesRequestBody),
    Chat(ChatRequestBody),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ChatReasoningStyle {
    Plain,
    DeepSeek,
    Zhipu,
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
                            input.push(ResponsesInputItem::CustomToolCallOutput {
                                call_id,
                                name: (!metadata.tool_name.is_empty())
                                    .then_some(metadata.tool_name),
                                output,
                            });
                        }
                    }
                }
                MessageRole::System | MessageRole::User | MessageRole::Assistant => {
                    input.push(ResponsesInputItem::message(
                        ResponsesRole::from_message_role(msg.role),
                        vec![ResponsesContent::InputText {
                            text: message_content_text(&msg.content),
                        }],
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
        name: Option<String>,
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
    effort: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    summary: Option<ResponsesReasoningSummary>,
}

impl ResponsesReasoning {
    fn from_config(reasoning: &ReasoningConfig) -> Self {
        Self {
            effort: reasoning.effort.clone(),
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
    #[serde(skip_serializing_if = "Option::is_none")]
    reasoning_effort: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    thinking: Option<ChatThinking>,
}

impl ChatRequestBody {
    fn from_request(
        request: &CompletionRequest,
        reasoning_style: ChatReasoningStyle,
    ) -> Result<Self> {
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
                    content: message_content_text(&msg.content),
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

        let (reasoning_effort, thinking) = match (reasoning_style, request.reasoning.as_ref()) {
            (ChatReasoningStyle::Plain, _) | (_, None) => (None, None),
            (ChatReasoningStyle::DeepSeek, Some(reasoning)) => (
                reasoning.effort.clone(),
                Some(ChatThinking {
                    kind: chat_thinking_type(reasoning).to_string(),
                    clear_thinking: None,
                }),
            ),
            (ChatReasoningStyle::Zhipu, Some(reasoning)) => {
                let kind = chat_thinking_type(reasoning).to_string();
                let clear_thinking = (kind == "enabled").then_some(false);
                (
                    None,
                    Some(ChatThinking {
                        kind,
                        clear_thinking,
                    }),
                )
            }
        };

        Ok(Self {
            model: request.model.clone(),
            messages,
            stream: true,
            tools,
            tool_choice,
            temperature: request.temperature,
            max_tokens: request.max_tokens,
            reasoning_effort,
            thinking,
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
        content: String,
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

#[derive(Debug, Clone, Serialize)]
struct ChatThinking {
    #[serde(rename = "type")]
    kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    clear_thinking: Option<bool>,
}

fn chat_thinking_type(reasoning: &ReasoningConfig) -> &'static str {
    match reasoning.summary {
        Some(ReasoningSummary::Disabled) => "disabled",
        Some(ReasoningSummary::Auto) | Some(ReasoningSummary::Enabled) | None => "enabled",
    }
}

fn message_content_text(content: &MessageContent) -> String {
    match content {
        MessageContent::Text(text) => text.clone(),
        MessageContent::MultiPart(parts) => parts
            .iter()
            .map(|part| part.text.as_str())
            .collect::<Vec<_>>()
            .join("\n"),
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

    use pl_protocol::{Message, MessageContent, MessageRole};
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
        let chat_body =
            OpenAiProtocol::chat(ChatReasoningStyle::Plain).build_request_body(&request);

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

    #[test]
    fn responses_body_writes_xhigh_reasoning_effort() {
        let body = OpenAiProtocol::responses().build_request_body(&request_with_effort("xhigh"));

        assert_eq!(body["reasoning"]["effort"], serde_json::json!("xhigh"));
    }

    #[test]
    fn responses_body_accepts_custom_reasoning_effort() {
        let body =
            OpenAiProtocol::responses().build_request_body(&request_with_effort("custom-effort"));

        assert_eq!(
            body["reasoning"]["effort"],
            serde_json::json!("custom-effort")
        );
    }

    #[test]
    fn responses_body_maps_enabled_reasoning_summary_to_auto() {
        let mut request = request_with_effort("medium");
        request.reasoning.as_mut().unwrap().summary = Some(ReasoningSummary::Enabled);

        let body = OpenAiProtocol::responses().build_request_body(&request);

        assert_eq!(body["reasoning"]["summary"], serde_json::json!("auto"));
    }

    #[test]
    fn responses_body_omits_disabled_reasoning_summary() {
        let mut request = request_with_effort("medium");
        request.reasoning.as_mut().unwrap().summary = Some(ReasoningSummary::Disabled);

        let body = OpenAiProtocol::responses().build_request_body(&request);

        assert!(body["reasoning"].get("summary").is_none());
    }

    #[test]
    fn generic_chat_body_omits_provider_specific_reasoning_fields() {
        let body = OpenAiProtocol::chat(ChatReasoningStyle::Plain)
            .build_request_body(&request_with_effort("max"));

        assert!(body.get("reasoning_effort").is_none());
        assert!(body.get("thinking").is_none());
    }

    #[test]
    fn deepseek_chat_body_writes_thinking_mode() {
        let body = OpenAiProtocol::chat(ChatReasoningStyle::DeepSeek)
            .build_request_body(&request_with_effort("max"));

        assert_eq!(body["reasoning_effort"], serde_json::json!("max"));
        assert_eq!(body["thinking"]["type"], serde_json::json!("enabled"));
    }

    #[test]
    fn deepseek_chat_body_writes_disabled_thinking_mode() {
        let mut request = request_with_effort("high");
        request.reasoning.as_mut().unwrap().summary = Some(ReasoningSummary::Disabled);

        let body = OpenAiProtocol::chat(ChatReasoningStyle::DeepSeek).build_request_body(&request);

        assert_eq!(body["thinking"]["type"], serde_json::json!("disabled"));
    }

    #[test]
    fn zhipu_chat_body_writes_official_thinking_mode() {
        let body = OpenAiProtocol::chat(ChatReasoningStyle::Zhipu)
            .build_request_body(&request_with_effort("enabled"));

        assert!(body.get("reasoning_effort").is_none());
        assert_eq!(body["thinking"]["type"], serde_json::json!("enabled"));
        assert_eq!(body["thinking"]["clear_thinking"], serde_json::json!(false));
    }

    #[test]
    fn zhipu_chat_body_writes_disabled_thinking_mode() {
        let mut request = request_with_effort("none");
        request.reasoning.as_mut().unwrap().summary = Some(ReasoningSummary::Disabled);

        let body = OpenAiProtocol::chat(ChatReasoningStyle::Zhipu).build_request_body(&request);

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

        let body = OpenAiProtocol::chat(ChatReasoningStyle::Plain).build_request_body(&request);

        assert_eq!(
            body["messages"][0]["reasoning_content"],
            serde_json::json!("比较小数位。")
        );
    }

    #[test]
    fn chat_parse_response_reads_reasoning_content() {
        let response = OpenAiProtocol::chat(ChatReasoningStyle::Plain)
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
        let response = OpenAiProtocol::chat(ChatReasoningStyle::Plain)
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

        let body = OpenAiProtocol::chat(ChatReasoningStyle::Plain).build_request_body(&request);

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
        let body = OpenAiProtocol::chat(ChatReasoningStyle::Plain).build_request_body(&request);

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
        let response = OpenAiProtocol::chat(ChatReasoningStyle::Plain)
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
        let chat_body =
            OpenAiProtocol::chat(ChatReasoningStyle::Plain).build_request_body(&request);

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
        let chat_body =
            OpenAiProtocol::chat(ChatReasoningStyle::Plain).build_request_body(&request);

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
            .build_request(&request)
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
            .build_request(&request)
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
            .build_request(&request)
            .unwrap_err();
        let chat_body =
            OpenAiProtocol::chat(ChatReasoningStyle::Plain).build_request_body(&request);

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

use serde::Deserialize;
use serde::Serialize;

use crate::{ModelCapabilities, ModelModality, ModelTransportSession};
use pl_protocol::{
    ContentPart, ImageSource, Message, MessageContent, ModelContextItem, PureError, ToolCallKind,
};
use pl_trace::TraceEvent;

const APPLY_PATCH_FUNCTION_FALLBACK_DESCRIPTION: &str = "Complete Codex-style apply_patch text beginning with *** Begin Patch and ending with *** End Patch. Each file operation must use one of these hunk headers: *** Add File: <path>, *** Delete File: <path>, or *** Update File: <path>. Do not use ---/+++ unified diff, *** File: metadata, or natural-language edit instructions such as Insert after. If a previous patch failed, read the target file again and retry with a smaller patch based on current content; do not repeat the same failed patch. Minimal update example:\n*** Begin Patch\n*** Update File: notes.txt\n@@\n-old line\n+new line\n*** End Patch";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompletionRequest {
    pub model: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instructions: Option<String>,
    pub input: Vec<ModelContextItem>,
    #[serde(default)]
    pub tools: Vec<ToolSchema>,
    #[serde(default = "default_tool_choice")]
    pub tool_choice: String,
    #[serde(default)]
    pub parallel_tool_calls: bool,
    pub temperature: Option<f32>,
    pub max_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub store: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub previous_response_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt_cache_key: Option<String>,
    pub reasoning: Option<ReasoningConfig>,
    #[serde(default = "default_true")]
    pub stream: bool,
    #[serde(skip)]
    pub trace: Option<CompletionTraceContext>,
    #[serde(skip)]
    pub transport_session: ModelTransportSession,
}

/// OpenAI provider 的上下文压缩协议选择。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OpenAiCompactionMode {
    #[default]
    RemoteV2,
    RemoteLegacy,
    Local,
}

impl OpenAiCompactionMode {
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }
}

/// 统一的 provider 压缩请求。
#[derive(Debug, Clone)]
pub struct ModelCompactionRequest {
    pub mode: OpenAiCompactionMode,
    pub model: String,
    pub instructions: String,
    pub input: Vec<ModelContextItem>,
    pub tools: Vec<ToolSchema>,
    pub parallel_tool_calls: bool,
    pub reasoning: Option<ReasoningConfig>,
    pub prompt_cache_key: Option<String>,
}

/// Provider 完成远程压缩后返回的替换历史。
#[derive(Debug, Clone)]
pub struct ModelCompactionResponse {
    pub input: Vec<ModelContextItem>,
    pub usage: Option<TokenUsage>,
}

fn default_tool_choice() -> String {
    "auto".into()
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone)]
pub struct CompletionRequestBuilder {
    request: CompletionRequest,
}

impl CompletionRequestBuilder {
    pub fn instructions(mut self, instructions: impl Into<String>) -> Self {
        self.request.instructions = Some(instructions.into());
        self
    }

    pub fn maybe_instructions(mut self, instructions: Option<String>) -> Self {
        self.request.instructions = instructions;
        self
    }

    pub fn messages(mut self, messages: Vec<Message>) -> Self {
        self.request.input = messages.into_iter().map(ModelContextItem::from).collect();
        self
    }

    pub fn input(mut self, input: Vec<ModelContextItem>) -> Self {
        self.request.input = input;
        self
    }

    pub fn tools(mut self, tools: Vec<ToolSchema>) -> Self {
        self.request.tools = tools;
        self
    }

    pub fn tool_choice(mut self, tool_choice: impl Into<String>) -> Self {
        self.request.tool_choice = tool_choice.into();
        self
    }

    pub fn parallel_tool_calls(mut self, parallel_tool_calls: bool) -> Self {
        self.request.parallel_tool_calls = parallel_tool_calls;
        self
    }

    pub fn temperature(mut self, temperature: Option<f32>) -> Self {
        self.request.temperature = temperature;
        self
    }

    pub fn max_tokens(mut self, max_tokens: u64) -> Self {
        self.request.max_tokens = Some(max_tokens);
        self
    }

    pub fn maybe_max_tokens(mut self, max_tokens: Option<u64>) -> Self {
        self.request.max_tokens = max_tokens;
        self
    }

    pub fn store(mut self, store: Option<bool>) -> Self {
        self.request.store = store;
        self
    }

    pub fn prompt_cache_key(mut self, prompt_cache_key: Option<String>) -> Self {
        self.request.prompt_cache_key = prompt_cache_key;
        self
    }

    pub fn reasoning(mut self, reasoning: Option<ReasoningConfig>) -> Self {
        self.request.reasoning = reasoning;
        self
    }

    pub fn stream(mut self, stream: bool) -> Self {
        self.request.stream = stream;
        self
    }

    pub fn trace(mut self, trace: Option<CompletionTraceContext>) -> Self {
        self.request.trace = trace;
        self
    }

    pub fn transport_session(mut self, transport_session: ModelTransportSession) -> Self {
        self.request.transport_session = transport_session;
        self
    }

    pub fn build(self) -> CompletionRequest {
        self.request
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompletionResponse {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub response_id: Option<String>,
    pub content: Option<String>,
    pub raw_content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning_content: Option<String>,
    #[serde(default)]
    pub tool_calls: Vec<ToolCall>,
    #[serde(default)]
    pub trace_events: Vec<TraceEvent>,
    #[serde(default)]
    pub next_sequence: u64,
    pub usage: TokenUsage,
    pub finish_reason: FinishReason,
    pub model: String,
}

#[derive(Debug, Clone)]
pub struct CompletionTraceContext {
    pub session_id: String,
    pub turn_id: String,
    pub inference_id: String,
    pub plan_mode: bool,
    pub trace_sequence_base: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub payload: ToolCallPayload,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub call_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub invalid_arguments: Option<InvalidToolArguments>,
}

/// Provider 返回的 function tool 参数无法解析为 JSON 时保留的诊断信息。
///
/// 该信息让执行层把模型输出错误作为失败的工具调用反馈给模型，而不是把整次
/// completion 误判为 provider 传输失败。原始参数同时用于历史回放与 trace。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct InvalidToolArguments {
    pub raw: String,
    pub error: String,
}

impl ToolCall {
    /// Returns the stable identity used to correlate a tool call across provider APIs.
    ///
    /// Responses providers expose a dedicated `call_id`; Chat Completions providers
    /// may only expose the tool item id. Empty provider ids are treated as missing.
    pub fn stable_call_id(&self) -> &str {
        self.call_id
            .as_deref()
            .filter(|call_id| !call_id.is_empty())
            .unwrap_or(&self.id)
    }

    pub fn function(
        id: impl Into<String>,
        name: impl Into<String>,
        arguments: serde_json::Value,
        call_id: Option<String>,
    ) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            payload: ToolCallPayload::Function { arguments },
            call_id,
            invalid_arguments: None,
        }
    }

    /// 构造 provider 已给出稳定身份、但 function 参数不是合法 JSON 的工具调用。
    pub fn invalid_function(
        id: impl Into<String>,
        name: impl Into<String>,
        raw: impl Into<String>,
        error: impl Into<String>,
        call_id: Option<String>,
    ) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            payload: ToolCallPayload::Function {
                arguments: serde_json::Value::Null,
            },
            call_id,
            invalid_arguments: Some(InvalidToolArguments {
                raw: raw.into(),
                error: error.into(),
            }),
        }
    }

    pub fn custom(
        id: impl Into<String>,
        name: impl Into<String>,
        input: impl Into<String>,
        call_id: Option<String>,
    ) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            payload: ToolCallPayload::Custom {
                input: input.into(),
            },
            call_id,
            invalid_arguments: None,
        }
    }

    pub fn kind(&self) -> ToolCallKind {
        match self.payload {
            ToolCallPayload::Function { .. } => ToolCallKind::Function,
            ToolCallPayload::Custom { .. } => ToolCallKind::Custom,
        }
    }

    pub fn arguments_for_tool(&self) -> serde_json::Value {
        match &self.payload {
            ToolCallPayload::Function { arguments } => arguments.clone(),
            ToolCallPayload::Custom { input } => serde_json::json!({
                "input": input,
                "patch": input,
            }),
        }
    }

    pub fn arguments_for_display(&self) -> serde_json::Value {
        if let Some(invalid) = &self.invalid_arguments {
            return serde_json::json!({
                "raw": invalid.raw,
                "parse_error": invalid.error,
            });
        }
        match &self.payload {
            ToolCallPayload::Function { arguments } => arguments.clone(),
            ToolCallPayload::Custom { input } => serde_json::json!({ "input": input }),
        }
    }

    pub fn payload_text(&self) -> String {
        if let Some(invalid) = &self.invalid_arguments {
            return invalid.raw.clone();
        }
        match &self.payload {
            ToolCallPayload::Function { arguments } => {
                serde_json::to_string(arguments).unwrap_or_default()
            }
            ToolCallPayload::Custom { input } => input.clone(),
        }
    }

    /// 返回可直接反馈给模型的非法参数诊断；合法调用返回 `None`。
    pub fn invalid_arguments_message(&self) -> Option<String> {
        let invalid = self.invalid_arguments.as_ref()?;
        Some(format!(
            "Invalid JSON arguments for function tool {}: {}. Call the tool again with exactly one valid JSON object.",
            self.name, invalid.error
        ))
    }
}

#[cfg(test)]
mod tool_call_tests {
    use super::ToolCall;

    #[test]
    fn stable_call_id_prefers_provider_call_id() {
        let call = ToolCall::function(
            "item-1",
            "read_file",
            serde_json::json!({}),
            Some("call-1".to_string()),
        );

        assert_eq!(call.stable_call_id(), "call-1");
    }

    #[test]
    fn stable_call_id_falls_back_to_item_id() {
        for call_id in [None, Some(String::new())] {
            let call = ToolCall::function("item-1", "read_file", serde_json::json!({}), call_id);

            assert_eq!(call.stable_call_id(), "item-1");
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ToolCallPayload {
    Function { arguments: serde_json::Value },
    Custom { input: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(tag = "kind")]
pub enum ToolSchema {
    Function {
        name: String,
        description: String,
        input_schema: serde_json::Value,
    },
    Custom {
        name: String,
        description: String,
        format: ToolFormat,
    },
}

impl ToolSchema {
    pub fn function(
        name: impl Into<String>,
        description: impl Into<String>,
        input_schema: serde_json::Value,
    ) -> Self {
        Self::Function {
            name: name.into(),
            description: description.into(),
            input_schema,
        }
    }

    pub fn custom_grammar(
        name: impl Into<String>,
        description: impl Into<String>,
        syntax: impl Into<String>,
        definition: impl Into<String>,
    ) -> Self {
        Self::Custom {
            name: name.into(),
            description: description.into(),
            format: ToolFormat::Grammar {
                syntax: syntax.into(),
                definition: definition.into(),
            },
        }
    }

    pub fn name(&self) -> &str {
        match self {
            Self::Function { name, .. } | Self::Custom { name, .. } => name,
        }
    }

    pub fn description(&self) -> &str {
        match self {
            Self::Function { description, .. } | Self::Custom { description, .. } => description,
        }
    }

    pub fn is_custom(&self) -> bool {
        matches!(self, Self::Custom { .. })
    }

    pub fn provider_compatible(self, supports_custom_tools: bool) -> Self {
        if supports_custom_tools {
            return self;
        }

        match self {
            Self::Custom {
                name, description, ..
            } if name == "apply_patch" => Self::function(
                name,
                description,
                serde_json::json!({
                    "type": "object",
                    "properties": {
                        "patch": {
                            "type": "string",
                            "description": APPLY_PATCH_FUNCTION_FALLBACK_DESCRIPTION
                        }
                    },
                    "required": ["patch"],
                    "additionalProperties": false
                }),
            ),
            Self::Custom {
                name, description, ..
            } => Self::function(
                name,
                description,
                serde_json::json!({
                    "type": "object",
                    "properties": {
                        "input": { "type": "string" }
                    },
                    "required": ["input"],
                    "additionalProperties": false
                }),
            ),
            function => function,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum ToolFormat {
    Text,
    Grammar { syntax: String, definition: String },
}

impl CompletionRequest {
    pub fn builder(model: impl Into<String>) -> CompletionRequestBuilder {
        CompletionRequestBuilder {
            request: CompletionRequest {
                model: model.into(),
                instructions: None,
                input: Vec::new(),
                tools: Vec::new(),
                tool_choice: default_tool_choice(),
                parallel_tool_calls: false,
                temperature: None,
                max_tokens: None,
                store: None,
                previous_response_id: None,
                prompt_cache_key: None,
                reasoning: None,
                stream: true,
                trace: None,
                transport_session: ModelTransportSession::default(),
            },
        }
    }

    pub fn provider_compatible(mut self, supports_custom_tools: bool) -> Self {
        self.tools = self
            .tools
            .into_iter()
            .map(|tool| tool.provider_compatible(supports_custom_tools))
            .collect();
        self
    }

    pub fn validate_against(&self, capabilities: &ModelCapabilities) -> pl_protocol::Result<()> {
        let requirements = RequestRequirements::from_input(&self.input)?;
        if requirements.text && !capabilities.supports_input_modality(ModelModality::Text) {
            return Err(PureError::ConfigError(format!(
                "model {} does not support text input",
                self.model
            )));
        }
        if requirements.image && !capabilities.supports_input_modality(ModelModality::Image) {
            return Err(PureError::ConfigError(format!(
                "model {} does not support image input",
                self.model
            )));
        }
        if self.temperature.is_some() && !capabilities.supports_temperature() {
            return Err(PureError::ConfigError(format!(
                "model {} does not support temperature",
                self.model
            )));
        }
        if self
            .reasoning
            .as_ref()
            .is_some_and(ReasoningConfig::is_enabled)
            && !capabilities.supports_reasoning()
        {
            return Err(PureError::ConfigError(format!(
                "model {} does not support reasoning",
                self.model
            )));
        }
        if !self.tools.is_empty() && !capabilities.supports_function_calling() {
            return Err(PureError::ConfigError(format!(
                "model {} does not support function calling",
                self.model
            )));
        }
        if self.parallel_tool_calls && !capabilities.supports_parallel_tool_calls() {
            return Err(PureError::ConfigError(format!(
                "model {} does not support parallel tool calls",
                self.model
            )));
        }
        if self.tools.iter().any(ToolSchema::is_custom)
            && (!capabilities.supports_custom_tools() || !capabilities.supports_freeform_tools())
        {
            return Err(PureError::ConfigError(format!(
                "model {} does not support custom/freeform tools",
                self.model
            )));
        }
        Ok(())
    }
}

#[derive(Debug, Default)]
struct RequestRequirements {
    text: bool,
    image: bool,
}

impl RequestRequirements {
    fn from_input(input: &[ModelContextItem]) -> pl_protocol::Result<Self> {
        let mut requirements = Self::default();
        for item in input {
            let Some(message) = item.as_message() else {
                continue;
            };
            match &message.content {
                MessageContent::Text(text) => {
                    if !text.is_empty() {
                        requirements.text = true;
                    }
                }
                MessageContent::MultiPart(parts) => {
                    for part in parts {
                        match part {
                            ContentPart::Text { text } => {
                                if !text.is_empty() {
                                    requirements.text = true;
                                }
                            }
                            ContentPart::Image { source, .. } => {
                                requirements.image = true;
                                if matches!(source, ImageSource::Attachment { .. }) {
                                    return Err(PureError::ConfigError(
                                        "image attachments must be materialized before model request"
                                            .to_string(),
                                    ));
                                }
                            }
                        }
                    }
                }
            }
        }
        Ok(requirements)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TokenUsage {
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub total_tokens: u64,
    #[serde(default)]
    pub cached_prompt_tokens: u64,
    #[serde(default)]
    pub reasoning_tokens: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FinishReason {
    Stop,
    ToolCalls,
    MaxTokens,
    ContentFilter,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReasoningConfig {
    pub effort: Option<String>,
    pub summary: Option<ReasoningSummary>,
}

impl ReasoningConfig {
    pub fn is_enabled(&self) -> bool {
        !matches!(
            self.effort.as_deref(),
            None | Some("") | Some("none") | Some("disabled")
        ) || !matches!(self.summary, None | Some(ReasoningSummary::Disabled))
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ReasoningSummary {
    Auto,
    Enabled,
    Disabled,
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use pl_protocol::{ContentPart, ImageSource, Message, MessageContent, MessageRole};
    use pretty_assertions::assert_eq;

    use super::*;
    use crate::{ModelCapabilities, ModelModality, ToolCapabilities};

    fn base_request(content: MessageContent) -> CompletionRequest {
        CompletionRequest {
            model: "model".to_string(),
            instructions: None,
            input: vec![ModelContextItem::from(Message {
                role: MessageRole::User,
                content,
                reasoning_content: None,
                metadata: HashMap::new(),
            })],
            tools: Vec::new(),
            tool_choice: "auto".to_string(),
            parallel_tool_calls: false,
            temperature: None,
            max_tokens: None,
            store: None,
            previous_response_id: None,
            prompt_cache_key: None,
            reasoning: None,
            stream: true,
            trace: None,
            transport_session: ModelTransportSession::default(),
        }
    }

    fn text_capabilities() -> ModelCapabilities {
        ModelCapabilities {
            streaming: true,
            temperature: false,
            reasoning: false,
            web_search: false,
            input: vec![ModelModality::Text],
            output: vec![ModelModality::Text],
            tools: ToolCapabilities::default(),
            interleaved: None,
        }
    }

    #[test]
    fn validation_rejects_image_when_model_is_text_only() {
        let request = base_request(MessageContent::MultiPart(vec![ContentPart::Image {
            source: ImageSource::InlineBase64 {
                data: "aGVsbG8=".to_string(),
            },
            media_type: "image/png".to_string(),
            filename: None,
        }]));

        let error = request.validate_against(&text_capabilities()).unwrap_err();

        assert_eq!(
            error.to_string(),
            "configuration error: model model does not support image input"
        );
    }

    #[test]
    fn validation_rejects_unmaterialized_attachment() {
        let request = base_request(MessageContent::MultiPart(vec![ContentPart::Image {
            source: ImageSource::Attachment {
                attachment_id: "attachment-1".to_string(),
            },
            media_type: "image/png".to_string(),
            filename: None,
        }]));

        let error = request.validate_against(&text_capabilities()).unwrap_err();

        assert_eq!(
            error.to_string(),
            "configuration error: image attachments must be materialized before model request"
        );
    }

    #[test]
    fn validation_allows_disabled_reasoning_for_text_model() {
        let mut request = base_request(MessageContent::Text("hello".to_string()));
        request.reasoning = Some(ReasoningConfig {
            effort: Some("none".to_string()),
            summary: Some(ReasoningSummary::Disabled),
        });

        request.validate_against(&text_capabilities()).unwrap();
    }

    #[test]
    fn validation_rejects_enabled_reasoning_for_text_model() {
        let mut request = base_request(MessageContent::Text("hello".to_string()));
        request.reasoning = Some(ReasoningConfig {
            effort: Some("high".to_string()),
            summary: Some(ReasoningSummary::Enabled),
        });

        let error = request.validate_against(&text_capabilities()).unwrap_err();

        assert_eq!(
            error.to_string(),
            "configuration error: model model does not support reasoning"
        );
    }
}

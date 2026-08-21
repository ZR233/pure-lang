//! Canonical completion 请求、响应、事件、工具与 Web Search 类型。

pub(crate) mod stream;
pub(crate) mod tool_arguments;
mod visible_text;
mod web_search;

pub use web_search::{
    ClickOperation, ExternalWebAccess, ExternalWebAccessMode, FinanceAssetType, FinanceOperation,
    FindOperation, OpenOperation, ScreenshotOperation, SearchAllowedCaller, SearchCommands,
    SearchQuery, SearchRequest, SearchResponse, SearchResponseLength, SearchSettings,
    SportsFunction, SportsLeague, SportsOperation, SportsToolName, TimeOperation, WeatherOperation,
    WebSearchAction, WebSearchConfig, WebSearchContextSize, WebSearchFilters, WebSearchLocation,
    WebSearchMode, WebSearchUserLocation, WebSearchUserLocationType,
};

use serde::Deserialize;
use serde::Serialize;

use crate::{ModelCapabilities, ModelModality};
use pl_protocol::{
    ContentPart, ImageSource, Message, MessageContent, ModelContextItem, PureError,
    ResponsesContextItem, ToolCallCaller, ToolCallKind,
};

const APPLY_PATCH_FUNCTION_FALLBACK_DESCRIPTION: &str = "Complete Codex-style apply_patch text beginning with *** Begin Patch and ending with *** End Patch. Each file operation must use one of these hunk headers: *** Add File: <path>, *** Delete File: <path>, or *** Update File: <path>. Do not use ---/+++ unified diff, *** File: metadata, or natural-language edit instructions such as Insert after. If a previous patch failed, read the target file again and retry with a smaller patch based on current content; do not repeat the same failed patch. Minimal update example:\n*** Begin Patch\n*** Update File: notes.txt\n@@\n-old line\n+new line\n*** End Patch";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompletionRequest {
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
    pub reasoning: Option<ReasoningConfig>,
}

/// OpenAI provider 的上下文压缩协议选择。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OpenAiCompactionMode {
    #[default]
    RemoteV2,
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

    pub fn reasoning(mut self, reasoning: Option<ReasoningConfig>) -> Self {
        self.request.reasoning = reasoning;
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning_content: Option<String>,
    #[serde(default)]
    pub tool_calls: Vec<ToolCall>,
    #[serde(default)]
    pub responses_context_items: Vec<ResponsesContextItem>,
    #[serde(default)]
    pub orchestration: pl_protocol::InferenceOrchestrationMetrics,
    pub usage: TokenUsage,
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
    /// provider 返回的工具调用 item id（`item.id`）。
    pub id: String,
    pub name: String,
    pub payload: ToolCallPayload,
    /// 跨协议回放的 canonical 调用 id；在协议解码边界一次性确定，必填。
    pub call_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub invalid_arguments: Option<InvalidToolArguments>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub caller: Option<ToolCallCaller>,
}

/// 一次工具调用的必填 typed 身份。
///
/// `item_id` 是 provider 返回的工具调用 item id；`call_id` 是跨协议回放使用的
/// canonical 调用 id。两者在解码边界确定，不存在 optional 回落路径。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolCallIdentity {
    pub item_id: String,
    pub call_id: String,
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
    pub fn identity(&self) -> ToolCallIdentity {
        ToolCallIdentity {
            item_id: self.id.clone(),
            call_id: self.call_id.clone(),
        }
    }

    pub fn function(
        id: impl Into<String>,
        name: impl Into<String>,
        arguments: serde_json::Value,
        call_id: impl Into<String>,
    ) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            payload: ToolCallPayload::Function { arguments },
            call_id: call_id.into(),
            invalid_arguments: None,
            caller: None,
        }
    }

    /// 构造 provider 已给出稳定身份、但 function 参数不是合法 JSON 的工具调用。
    pub fn invalid_function(
        id: impl Into<String>,
        name: impl Into<String>,
        raw: impl Into<String>,
        error: impl Into<String>,
        call_id: impl Into<String>,
    ) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            payload: ToolCallPayload::Function {
                arguments: serde_json::Value::Null,
            },
            call_id: call_id.into(),
            invalid_arguments: Some(InvalidToolArguments {
                raw: raw.into(),
                error: error.into(),
            }),
            caller: None,
        }
    }

    pub fn custom(
        id: impl Into<String>,
        name: impl Into<String>,
        input: impl Into<String>,
        call_id: impl Into<String>,
    ) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            payload: ToolCallPayload::Custom {
                input: input.into(),
            },
            call_id: call_id.into(),
            invalid_arguments: None,
            caller: None,
        }
    }

    pub fn with_caller(mut self, caller: Option<ToolCallCaller>) -> Self {
        self.caller = caller;
        self
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
            ToolCallPayload::Custom { input } => serde_json::json!({ "input": input }),
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
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        allowed_callers: Vec<ToolCallerMode>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        output_schema: Option<serde_json::Value>,
    },
    Custom {
        name: String,
        description: String,
        format: ToolFormat,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        allowed_callers: Vec<ToolCallerMode>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        output_schema: Option<serde_json::Value>,
    },
    ProgrammaticToolCalling,
    WebSearch {
        external_web_access: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        indexed_web_access: Option<bool>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        filters: Option<WebSearchFilters>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        user_location: Option<WebSearchUserLocation>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        search_context_size: Option<WebSearchContextSize>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        search_content_types: Option<Vec<String>>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolCallerMode {
    Direct,
    Programmatic,
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
            allowed_callers: Vec::new(),
            output_schema: None,
        }
    }

    pub fn allow_programmatic(mut self, output_schema: serde_json::Value) -> Self {
        match &mut self {
            Self::Function {
                allowed_callers,
                output_schema: schema,
                ..
            }
            | Self::Custom {
                allowed_callers,
                output_schema: schema,
                ..
            } => {
                *allowed_callers = vec![ToolCallerMode::Direct, ToolCallerMode::Programmatic];
                *schema = Some(output_schema);
            }
            Self::ProgrammaticToolCalling | Self::WebSearch { .. } => {}
        }
        self
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
            allowed_callers: Vec::new(),
            output_schema: None,
        }
    }

    pub fn name(&self) -> &str {
        match self {
            Self::Function { name, .. } | Self::Custom { name, .. } => name,
            Self::ProgrammaticToolCalling => "programmatic_tool_calling",
            Self::WebSearch { .. } => "web_search",
        }
    }

    pub fn description(&self) -> &str {
        match self {
            Self::Function { description, .. } | Self::Custom { description, .. } => description,
            Self::ProgrammaticToolCalling => "Coordinate eligible read-only tools in hosted code.",
            Self::WebSearch { .. } => "Search the web.",
        }
    }

    pub fn is_custom(&self) -> bool {
        matches!(self, Self::Custom { .. })
    }

    pub fn is_hosted(&self) -> bool {
        matches!(self, Self::WebSearch { .. } | Self::ProgrammaticToolCalling)
    }

    pub fn is_web_search(&self) -> bool {
        matches!(self, Self::WebSearch { .. })
    }

    pub fn is_programmatic_tool_calling(&self) -> bool {
        matches!(self, Self::ProgrammaticToolCalling)
    }

    pub fn provider_compatible(self, supports_custom_tools: bool) -> Self {
        if supports_custom_tools {
            return self;
        }

        match self {
            Self::Custom {
                name,
                description,
                allowed_callers,
                output_schema,
                ..
            } if name == "apply_patch" => Self::function(
                name,
                description,
                serde_json::json!({
                    "type": "object",
                    "properties": {
                        "input": {
                            "type": "string",
                            "description": APPLY_PATCH_FUNCTION_FALLBACK_DESCRIPTION
                        }
                    },
                    "required": ["input"],
                    "additionalProperties": false
                }),
            )
            .with_wire_options(allowed_callers, output_schema),
            Self::Custom {
                name,
                description,
                allowed_callers,
                output_schema,
                ..
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
            )
            .with_wire_options(allowed_callers, output_schema),
            function => function,
        }
    }

    fn with_wire_options(
        mut self,
        allowed_callers: Vec<ToolCallerMode>,
        output_schema: Option<serde_json::Value>,
    ) -> Self {
        if let Self::Function {
            allowed_callers: target_allowed_callers,
            output_schema: target_output_schema,
            ..
        } = &mut self
        {
            *target_allowed_callers = allowed_callers;
            *target_output_schema = output_schema;
        }
        self
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum ToolFormat {
    Text,
    Grammar { syntax: String, definition: String },
}

impl CompletionRequest {
    pub fn builder() -> CompletionRequestBuilder {
        CompletionRequestBuilder {
            request: CompletionRequest {
                instructions: None,
                input: Vec::new(),
                tools: Vec::new(),
                tool_choice: default_tool_choice(),
                parallel_tool_calls: false,
                temperature: None,
                max_tokens: None,
                reasoning: None,
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

    pub fn validate_against(
        &self,
        model: &str,
        capabilities: &ModelCapabilities,
    ) -> pl_protocol::Result<()> {
        let requirements = RequestRequirements::from_input(&self.input)?;
        if requirements.text && !capabilities.supports_input_modality(ModelModality::Text) {
            return Err(PureError::ConfigError(format!(
                "model {} does not support text input",
                model
            )));
        }
        if requirements.image && !capabilities.supports_input_modality(ModelModality::Image) {
            return Err(PureError::ConfigError(format!(
                "model {} does not support image input",
                model
            )));
        }
        if self.temperature.is_some() && !capabilities.supports_temperature() {
            return Err(PureError::ConfigError(format!(
                "model {} does not support temperature",
                model
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
                model
            )));
        }
        let has_function_tools = self.tools.iter().any(|tool| !tool.is_hosted());
        if has_function_tools && !capabilities.supports_function_calling() {
            return Err(PureError::ConfigError(format!(
                "model {} does not support function calling",
                model
            )));
        }
        if has_function_tools
            && self.parallel_tool_calls
            && !capabilities.supports_parallel_tool_calls()
        {
            return Err(PureError::ConfigError(format!(
                "model {} does not support parallel tool calls",
                model
            )));
        }
        if self.tools.iter().any(ToolSchema::is_web_search) && !capabilities.supports_web_search() {
            return Err(PureError::ConfigError(format!(
                "model {} does not support hosted web search",
                model
            )));
        }
        if self
            .tools
            .iter()
            .any(ToolSchema::is_programmatic_tool_calling)
            && !capabilities.supports_programmatic_tool_calling()
        {
            return Err(PureError::ConfigError(format!(
                "model {} does not support programmatic tool calling",
                model
            )));
        }
        if self.tools.iter().any(ToolSchema::is_custom)
            && (!capabilities.supports_custom_tools() || !capabilities.supports_freeform_tools())
        {
            return Err(PureError::ConfigError(format!(
                "model {} does not support custom/freeform tools",
                model
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
    pub cache_write_tokens: u64,
    #[serde(default)]
    pub reasoning_tokens: u64,
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
        CompletionRequest::builder()
            .input(vec![ModelContextItem::from(Message {
                role: MessageRole::User,
                content,
                reasoning_content: None,
                tool_calls: None,
                tool_result: None,
                metadata: HashMap::new(),
            })])
            .build()
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
            prompt_cache: Default::default(),
            interleaved: None,
        }
    }

    #[test]
    fn tool_call_identity_exposes_item_and_call_ids() {
        let call = ToolCall::function("item-1", "read_file", serde_json::json!({}), "call-1");

        assert_eq!(
            call.identity(),
            ToolCallIdentity {
                item_id: "item-1".to_string(),
                call_id: "call-1".to_string(),
            }
        );
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

        let error = request
            .validate_against("model", &text_capabilities())
            .unwrap_err();

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

        let error = request
            .validate_against("model", &text_capabilities())
            .unwrap_err();

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

        request
            .validate_against("model", &text_capabilities())
            .unwrap();
    }

    #[test]
    fn validation_rejects_enabled_reasoning_for_text_model() {
        let mut request = base_request(MessageContent::Text("hello".to_string()));
        request.reasoning = Some(ReasoningConfig {
            effort: Some("high".to_string()),
            summary: Some(ReasoningSummary::Enabled),
        });

        let error = request
            .validate_against("model", &text_capabilities())
            .unwrap_err();

        assert_eq!(
            error.to_string(),
            "configuration error: model model does not support reasoning"
        );
    }
}

use serde::Deserialize;
use serde::Serialize;

use crate::{ModelCapabilities, ModelModality};
use pl_protocol::{ContentPart, ImageSource, Message, MessageContent, PureError, ToolCallKind};
use pl_trace::TraceEvent;

const APPLY_PATCH_FUNCTION_FALLBACK_DESCRIPTION: &str = "Complete Codex-style apply_patch text beginning with *** Begin Patch and ending with *** End Patch. Each file operation must use one of these hunk headers: *** Add File: <path>, *** Delete File: <path>, or *** Update File: <path>. Do not use ---/+++ unified diff, *** File: metadata, or natural-language edit instructions such as Insert after. If a previous patch failed, read the target file again and retry with a smaller patch based on current content; do not repeat the same failed patch. Minimal update example:\n*** Begin Patch\n*** Update File: notes.txt\n@@\n-old line\n+new line\n*** End Patch";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompletionRequest {
    pub model: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instructions: Option<String>,
    pub messages: Vec<Message>,
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
}

fn default_tool_choice() -> String {
    "auto".into()
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompletionResponse {
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
}

impl ToolCall {
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
        match &self.payload {
            ToolCallPayload::Function { arguments } => arguments.clone(),
            ToolCallPayload::Custom { input } => serde_json::json!({ "input": input }),
        }
    }

    pub fn payload_text(&self) -> String {
        match &self.payload {
            ToolCallPayload::Function { arguments } => {
                serde_json::to_string(arguments).unwrap_or_default()
            }
            ToolCallPayload::Custom { input } => input.clone(),
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
    pub fn provider_compatible(mut self, supports_custom_tools: bool) -> Self {
        self.tools = self
            .tools
            .into_iter()
            .map(|tool| tool.provider_compatible(supports_custom_tools))
            .collect();
        self
    }

    pub fn validate_against(&self, capabilities: &ModelCapabilities) -> pl_protocol::Result<()> {
        let requirements = RequestRequirements::from_messages(&self.messages)?;
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
    fn from_messages(messages: &[Message]) -> pl_protocol::Result<Self> {
        let mut requirements = Self::default();
        for message in messages {
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

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
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
            messages: vec![Message {
                role: MessageRole::User,
                content,
                reasoning_content: None,
                metadata: HashMap::new(),
            }],
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

//! Canonical completion 请求、builder 与模型能力校验。

use serde::{Deserialize, Serialize};

use crate::completion::tool_schema::ToolSchema;
use crate::completion::usage::ReasoningConfig;
use crate::{ModelCapabilities, ModelModality};
use pl_protocol::{ContentPart, ImageSource, Message, MessageContent, ModelContextItem, PureError};

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

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use pl_protocol::{ContentPart, ImageSource, Message, MessageContent, MessageRole};
    use pretty_assertions::assert_eq;

    use super::*;
    use crate::completion::usage::{ReasoningConfig, ReasoningSummary};
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

use serde::Deserialize;
use serde::Serialize;

use pl_protocol::{Message, ToolCallKind, TraceEvent};

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
    pub reasoning: Option<ReasoningConfig>,
    #[serde(default = "default_true")]
    pub stream: bool,
    #[serde(skip)]
    pub timeline: Option<CompletionTimelineContext>,
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
    pub timeline_events: Vec<TraceEvent>,
    #[serde(default)]
    pub next_sequence: u64,
    pub usage: TokenUsage,
    pub finish_reason: FinishReason,
    pub model: String,
}

#[derive(Debug, Clone)]
pub struct CompletionTimelineContext {
    pub session_id: String,
    pub turn_id: String,
    pub inference_id: String,
    pub starting_sequence: u64,
    pub plan_mode: bool,
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
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TokenUsage {
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub total_tokens: u64,
    #[serde(default)]
    pub cached_prompt_tokens: u64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
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

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ReasoningSummary {
    Auto,
    Enabled,
    Disabled,
}

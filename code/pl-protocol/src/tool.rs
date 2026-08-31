//! Provider-neutral tool definitions shared by the model and core runtimes.

use serde::{Deserialize, Serialize};

/// Canonical model-visible tool specification.
///
/// Provider adapters map this value to their private wire structs. Agent ownership,
/// execution policy and handlers deliberately live outside this protocol type.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(tag = "kind")]
pub enum ToolSpec {
    /// JSON function tool executed by the local runtime.
    Function {
        name: String,
        description: String,
        input_schema: serde_json::Value,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        allowed_callers: Vec<ToolCallerMode>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        output_schema: Option<serde_json::Value>,
    },
    /// Free-form/custom tool executed by the local runtime.
    Custom {
        name: String,
        description: String,
        format: ToolFormat,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        allowed_callers: Vec<ToolCallerMode>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        output_schema: Option<serde_json::Value>,
    },
    /// Provider-hosted programmatic tool coordinator.
    ProgrammaticToolCalling,
    /// Provider-hosted web search tool.
    WebSearch {
        dialect: HostedWebSearchDialect,
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

/// Provider-hosted Responses Web Search wire dialect.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HostedWebSearchDialect {
    #[default]
    OpenAiResponses,
    DeepSeekResponses,
}

impl HostedWebSearchDialect {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::OpenAiResponses => "open_ai_responses",
            Self::DeepSeekResponses => "deepseek_responses",
        }
    }
}

impl std::str::FromStr for HostedWebSearchDialect {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "open_ai_responses" => Ok(Self::OpenAiResponses),
            "deepseek_responses" => Ok(Self::DeepSeekResponses),
            value => Err(format!("unsupported hosted web search dialect: {value}")),
        }
    }
}

impl ToolSpec {
    /// Builds a function tool specification.
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

    /// Allows direct and provider-hosted programmatic callers.
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

    /// Builds a custom tool backed by a grammar definition.
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

    /// Returns the stable model-visible name.
    pub fn name(&self) -> &str {
        match self {
            Self::Function { name, .. } | Self::Custom { name, .. } => name,
            Self::ProgrammaticToolCalling => "programmatic_tool_calling",
            Self::WebSearch { .. } => "web_search",
        }
    }

    /// Returns the model-visible description.
    pub fn description(&self) -> &str {
        match self {
            Self::Function { description, .. } | Self::Custom { description, .. } => description,
            Self::ProgrammaticToolCalling => "Coordinate eligible read-only tools in hosted code.",
            Self::WebSearch { .. } => "Search the web.",
        }
    }

    /// Returns whether the provider must encode a custom/free-form tool.
    pub fn is_custom(&self) -> bool {
        matches!(self, Self::Custom { .. })
    }

    /// Returns whether execution is owned by the provider.
    pub fn is_hosted(&self) -> bool {
        matches!(self, Self::WebSearch { .. } | Self::ProgrammaticToolCalling)
    }

    /// Returns whether this is hosted web search.
    pub fn is_web_search(&self) -> bool {
        matches!(self, Self::WebSearch { .. })
    }

    /// Returns whether this is hosted programmatic tool calling.
    pub fn is_programmatic_tool_calling(&self) -> bool {
        matches!(self, Self::ProgrammaticToolCalling)
    }
}

/// The caller mode accepted by a function or custom tool.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolCallerMode {
    Direct,
    Programmatic,
}

/// Provider-neutral custom tool input format.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum ToolFormat {
    Text,
    Grammar { syntax: String, definition: String },
}

/// Hosted search context size.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WebSearchContextSize {
    Low,
    Medium,
    High,
}

/// Hosted web search domain filter.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WebSearchFilters {
    pub allowed_domains: Vec<String>,
}

/// Hosted web search approximate location.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WebSearchUserLocation {
    #[serde(rename = "type")]
    pub kind: WebSearchUserLocationType,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub country: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub region: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub city: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timezone: Option<String>,
}

/// Hosted web search location type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WebSearchUserLocationType {
    Approximate,
}

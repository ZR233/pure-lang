//! 工具 schema、调用方模式与 wire 格式。

use serde::{Deserialize, Serialize};

use crate::completion::web_search::{
    WebSearchContextSize, WebSearchFilters, WebSearchUserLocation,
};

const APPLY_PATCH_FUNCTION_FALLBACK_DESCRIPTION: &str = "Complete Codex-style apply_patch text beginning with *** Begin Patch and ending with *** End Patch. Each file operation must use one of these hunk headers: *** Add File: <path>, *** Delete File: <path>, or *** Update File: <path>. Do not use ---/+++ unified diff, *** File: metadata, or natural-language edit instructions such as Insert after. If a previous patch failed, read the target file again and retry with a smaller patch based on current content; do not repeat the same failed patch. Minimal update example:\n*** Begin Patch\n*** Update File: notes.txt\n@@\n-old line\n+new line\n*** End Patch";

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

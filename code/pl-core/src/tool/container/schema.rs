use pl_model::ToolSchema;
use serde_json::{Value, json};

use super::helpers::object_schema;
pub const TOOL_CONTAINER_EXEC: &str = "container_exec";
pub const TOOL_CONTAINER_COPY: &str = "container_copy";

/// pl-core 共享的容器专属工具类型。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContainerToolKind {
    Exec,
    Copy,
}

impl ContainerToolKind {
    pub fn all() -> &'static [Self] {
        &[Self::Exec, Self::Copy]
    }

    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            TOOL_CONTAINER_EXEC => Some(Self::Exec),
            TOOL_CONTAINER_COPY => Some(Self::Copy),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Exec => TOOL_CONTAINER_EXEC,
            Self::Copy => TOOL_CONTAINER_COPY,
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            Self::Exec => {
                "Execute a shell command inside this agent's Docker container. timeoutSecs is optional; omit it for no command time limit."
            }
            Self::Copy => {
                "Copy data into or out of this agent's Docker container. Use direction=upload with contentBase64 or direction=download to return tarBase64."
            }
        }
    }

    pub fn input_schema(self) -> Value {
        match self {
            Self::Exec => object_schema(vec![
                ("command", json!({ "type": "string" }), true),
                ("cwd", json!({ "type": "string" }), false),
                (
                    "timeoutSecs",
                    json!({ "type": "integer", "minimum": 1 }),
                    false,
                ),
                (
                    "maxOutputTokens",
                    json!({ "type": "integer", "minimum": 1 }),
                    false,
                ),
                (
                    "outputBytesCap",
                    json!({ "type": "integer", "minimum": 1 }),
                    false,
                ),
            ]),
            Self::Copy => object_schema(vec![
                (
                    "direction",
                    json!({
                        "type": "string",
                        "enum": ["upload", "download"]
                    }),
                    true,
                ),
                ("path", json!({ "type": "string" }), true),
                (
                    "contentBase64",
                    json!({
                        "type": "string",
                        "description": "Base64 file content required when direction is upload."
                    }),
                    false,
                ),
            ]),
        }
    }

    pub fn to_schema(self) -> ToolSchema {
        ToolSchema::function(self.name(), self.description(), self.input_schema())
    }
}

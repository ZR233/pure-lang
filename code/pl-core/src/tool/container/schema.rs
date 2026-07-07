use pl_model::ToolSchema;
use serde_json::{Value, json};

use super::helpers::object_schema;
pub const TOOL_CONTAINER_EXEC: &str = "container_exec";
pub const TOOL_CONTAINER_CP_UPLOAD: &str = "container_cp_upload";
pub const TOOL_CONTAINER_CP_DOWNLOAD: &str = "container_cp_download";

/// pl-core 共享的容器专属工具类型。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContainerToolKind {
    Exec,
    CopyUpload,
    CopyDownload,
}

impl ContainerToolKind {
    pub fn all() -> &'static [Self] {
        &[Self::Exec, Self::CopyUpload, Self::CopyDownload]
    }

    pub fn from_name(name: &str) -> Option<Self> {
        let normalized;
        let name = if name.contains('.') {
            normalized = name.replace('.', "_");
            normalized.as_str()
        } else {
            name
        };
        match name {
            TOOL_CONTAINER_EXEC => Some(Self::Exec),
            TOOL_CONTAINER_CP_UPLOAD => Some(Self::CopyUpload),
            TOOL_CONTAINER_CP_DOWNLOAD => Some(Self::CopyDownload),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Exec => TOOL_CONTAINER_EXEC,
            Self::CopyUpload => TOOL_CONTAINER_CP_UPLOAD,
            Self::CopyDownload => TOOL_CONTAINER_CP_DOWNLOAD,
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            Self::Exec => {
                "Execute a shell command inside this agent's Docker container. timeout_secs is optional; omit it for no command time limit."
            }
            Self::CopyUpload => "Write a base64 encoded file into this agent's Docker container.",
            Self::CopyDownload => {
                "Export a file or directory from this agent's Docker container as a base64 encoded tar stream."
            }
        }
    }

    pub fn input_schema(self) -> Value {
        match self {
            Self::Exec => object_schema(vec![
                ("command", json!({ "type": "string" }), true),
                ("cwd", json!({ "type": "string" }), false),
                (
                    "timeout_secs",
                    json!({ "type": "integer", "minimum": 1 }),
                    false,
                ),
            ]),
            Self::CopyUpload => object_schema(vec![
                ("path", json!({ "type": "string" }), true),
                ("content_base64", json!({ "type": "string" }), true),
            ]),
            Self::CopyDownload => object_schema(vec![("path", json!({ "type": "string" }), true)]),
        }
    }

    pub fn to_schema(self) -> ToolSchema {
        ToolSchema::function(self.name(), self.description(), self.input_schema())
    }
}
